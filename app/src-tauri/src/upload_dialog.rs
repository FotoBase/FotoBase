use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fotobase_catalog::Catalog;
use fotobase_export::google_photos::{
    GoogleAuth, GooglePhotosConfig, GooglePhotosPlugin, LoopbackReceiver, OAuthTokens, TokenStore,
    UploadLedger,
};
use fotobase_export::immich::{ImmichConfig, ImmichPlugin};
use fotobase_export::local_folder::LocalFolderPlugin;
use fotobase_export::s3::{S3Config, S3Credentials, S3Plugin};
use fotobase_export::webdav::{WebDavConfig, WebDavPlugin};
use fotobase_export::{
    AlbumRef, AlbumTarget, CancelToken, ExportError, ExportPlugin, FileStatus, ProgressEvent,
    Registry, SizeChoice, UploadFile, UploadJob, UploadOptions, UploadReport,
};
use fotobase_render::{export_image, ExportFormat, ExportOptions};
use keyring::Entry;
use serde::{Deserialize, Serialize};

const NO_RECIPE: &str = r#"{"spec":"fotobase.recipe","version":1,"ops":[]}"#;
const KEYRING_SERVICE: &str = "org.fotobase.google-photos";

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "plugin", rename_all = "kebab-case")]
pub enum UploadTargetConfig {
    LocalFolder {
        root: String,
    },
    Immich {
        server_url: String,
        api_key: String,
    },
    S3 {
        endpoint: String,
        bucket: String,
        region: String,
        access_key: String,
        secret_key: String,
        session_token: Option<String>,
        prefix: String,
    },
    Webdav {
        url: String,
        username: String,
        password: String,
        folder: String,
        allow_insecure_http: bool,
    },
    GooglePhotos {
        client_id: String,
        client_secret: Option<String>,
    },
}

impl UploadTargetConfig {
    pub(crate) fn plugin_id(&self) -> &'static str {
        match self {
            Self::LocalFolder { .. } => "local-folder",
            Self::Immich { .. } => "immich",
            Self::S3 { .. } => "s3",
            Self::Webdav { .. } => "webdav",
            Self::GooglePhotos { .. } => "google-photos",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum UploadSize {
    Original,
    Resized { max_edge: u32 },
}

impl UploadSize {
    pub(crate) fn choice(&self) -> SizeChoice {
        match self {
            Self::Original => SizeChoice::Original,
            Self::Resized { max_edge } => SizeChoice::MaxEdge(*max_edge),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadRequest {
    pub config: UploadTargetConfig,
    pub size: UploadSize,
    pub quality: u8,
    pub album: Option<UploadAlbumSelection>,
    pub job_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum UploadAlbumSelection {
    Existing { id: String, name: String },
    Create { name: String },
}

impl From<UploadAlbumSelection> for AlbumTarget {
    fn from(value: UploadAlbumSelection) -> Self {
        match value {
            UploadAlbumSelection::Existing { id, name } => Self::Existing(AlbumRef { id, name }),
            UploadAlbumSelection::Create { name } => Self::Create(name),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadTargetCapabilities {
    pub albums: bool,
    pub size_choice: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadTargetInfo {
    pub id: String,
    pub name: String,
    pub capabilities: UploadTargetCapabilities,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadAlbum {
    pub id: String,
    pub name: String,
}

impl From<AlbumRef> for UploadAlbum {
    fn from(album: AlbumRef) -> Self {
        Self { id: album.id, name: album.name }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UploadFileState {
    Preparing,
    Uploading,
    Retrying,
    Uploaded,
    AlreadyPresent,
    Failed,
    Pending,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadProgress {
    pub job_id: String,
    pub file_id: i64,
    pub name: String,
    pub index: usize,
    pub total: usize,
    pub state: UploadFileState,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadFileResult {
    pub file_id: i64,
    pub name: String,
    pub state: UploadFileState,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadBatchResult {
    pub job_id: String,
    pub files: Vec<UploadFileResult>,
    pub cancelled: bool,
    pub paused: bool,
}

pub struct UploadInput {
    pub file_id: i64,
    pub name: String,
    pub file: Result<UploadFile, String>,
}

pub struct UploadBatchOptions {
    pub plugin_id: String,
    pub job_id: String,
    pub album: Option<AlbumTarget>,
    pub upload_options: UploadOptions,
}

pub fn target_infos() -> Result<Vec<UploadTargetInfo>, String> {
    let mut registry = Registry::new();
    register_plugins(&mut registry, None, Path::new("."))?;
    Ok(registry
        .list()
        .into_iter()
        .map(|info| UploadTargetInfo {
            id: info.id,
            name: info.name,
            capabilities: UploadTargetCapabilities {
                albums: info.capabilities.albums,
                size_choice: info.capabilities.size_choice,
            },
        })
        .collect())
}

pub fn build_registry(config: &UploadTargetConfig, data_dir: &Path) -> Result<Registry, String> {
    let mut registry = Registry::new();
    register_plugins(&mut registry, Some(config), data_dir)?;
    Ok(registry)
}

fn register_plugins(
    registry: &mut Registry,
    selected: Option<&UploadTargetConfig>,
    data_dir: &Path,
) -> Result<(), String> {
    if let Some(config) = selected {
        registry.register(build_plugin(config, data_dir)?).map_err(|error| error.to_string())?;
        return Ok(());
    }

    registry.register(Box::new(LocalFolderPlugin::new("."))).map_err(|e| e.to_string())?;
    registry
        .register(Box::new(
            ImmichPlugin::new(
                ImmichConfig::new("https://photos.example.invalid", "placeholder-key")
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?,
        ))
        .map_err(|e| e.to_string())?;
    registry
        .register(Box::new(
            S3Plugin::new(
                S3Config::new(
                    "https://s3.example.invalid",
                    "fotobase-placeholder",
                    "us-east-1",
                    S3Credentials::new("placeholder-access", "placeholder-secret")
                        .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?,
        ))
        .map_err(|e| e.to_string())?;
    registry
        .register(Box::new(
            WebDavPlugin::new(
                WebDavConfig::new("https://dav.example.invalid/", "placeholder-user", "")
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?,
        ))
        .map_err(|e| e.to_string())?;
    registry
        .register(Box::new(
            GooglePhotosPlugin::new(
                GooglePhotosConfig::new("placeholder.apps.googleusercontent.com", None)
                    .map_err(|e| e.to_string())?,
                Arc::new(fotobase_export::google_photos::MemoryTokenStore::default()),
                UploadLedger::in_memory(),
            )
            .map_err(|e| e.to_string())?,
        ))
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn build_plugin(
    config: &UploadTargetConfig,
    data_dir: &Path,
) -> Result<Box<dyn ExportPlugin>, String> {
    let plugin: Box<dyn ExportPlugin> = match config {
        UploadTargetConfig::LocalFolder { root } => {
            Box::new(LocalFolderPlugin::new(PathBuf::from(root)))
        }
        UploadTargetConfig::Immich { server_url, api_key } => Box::new(
            ImmichPlugin::new(ImmichConfig::new(server_url, api_key).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
        ),
        UploadTargetConfig::S3 {
            endpoint,
            bucket,
            region,
            access_key,
            secret_key,
            session_token,
            prefix,
        } => {
            let mut credentials =
                S3Credentials::new(access_key, secret_key).map_err(|e| e.to_string())?;
            if let Some(token) = session_token.as_deref().filter(|value| !value.trim().is_empty()) {
                credentials = credentials.with_session_token(token).map_err(|e| e.to_string())?;
            }
            let config = S3Config::new(endpoint, bucket, region, credentials)
                .and_then(|config| config.with_prefix(prefix))
                .map_err(|e| e.to_string())?;
            Box::new(S3Plugin::new(config).map_err(|e| e.to_string())?)
        }
        UploadTargetConfig::Webdav { url, username, password, folder, allow_insecure_http } => {
            let mut config = WebDavConfig::new(url, username, password)
                .and_then(|config| config.with_folder(folder))
                .map_err(|e| e.to_string())?;
            if *allow_insecure_http {
                config = config.with_insecure_http();
            }
            Box::new(WebDavPlugin::new(config).map_err(|e| e.to_string())?)
        }
        UploadTargetConfig::GooglePhotos { client_id, client_secret } => {
            let google = GooglePhotosConfig::new(client_id, client_secret.as_deref())
                .map_err(|e| e.to_string())?;
            let store = Arc::new(KeyringTokenStore::new(client_id).map_err(|e| e.to_string())?);
            let tokens = store.load().map_err(|error| error.to_string())?;
            let ledger_name = google_ledger_name(client_id, tokens.as_ref());
            let ledger = UploadLedger::open(&data_dir.join("google-photos").join(ledger_name))
                .map_err(|e| e.to_string())?;
            Box::new(GooglePhotosPlugin::new(google, store, ledger).map_err(|e| e.to_string())?)
        }
    };
    Ok(plugin)
}

pub async fn list_albums(
    config: &UploadTargetConfig,
    data_dir: &Path,
) -> Result<Vec<UploadAlbum>, String> {
    let mut plugin = build_plugin(config, data_dir)?;
    plugin
        .list_albums()
        .await
        .map(|albums| albums.into_iter().map(UploadAlbum::from).collect())
        .map_err(|error| error.to_string())
}

pub fn google_signed_in(client_id: &str) -> Result<bool, String> {
    let store = KeyringTokenStore::new(client_id).map_err(|error| error.to_string())?;
    let auth = GoogleAuth::new(
        GooglePhotosConfig::new(client_id, None).map_err(|error| error.to_string())?,
        Arc::new(store),
    )
    .map_err(|error| error.to_string())?;
    auth.is_signed_in().map_err(|error| error.to_string())
}

pub fn google_sign_out(client_id: &str) -> Result<(), String> {
    let store = KeyringTokenStore::new(client_id).map_err(|error| error.to_string())?;
    let auth = GoogleAuth::new(
        GooglePhotosConfig::new(client_id, None).map_err(|error| error.to_string())?,
        Arc::new(store),
    )
    .map_err(|error| error.to_string())?;
    auth.sign_out().map_err(|error| error.to_string())
}

pub async fn google_sign_in(client_id: &str, client_secret: Option<&str>) -> Result<(), String> {
    let config =
        GooglePhotosConfig::new(client_id, client_secret).map_err(|error| error.to_string())?;
    let store = Arc::new(KeyringTokenStore::new(client_id).map_err(|error| error.to_string())?);
    let auth = GoogleAuth::new(config, store).map_err(|error| error.to_string())?;
    let receiver = LoopbackReceiver::bind().await.map_err(|error| error.to_string())?;
    let request =
        auth.authorization_request(receiver.redirect_uri()).map_err(|error| error.to_string())?;
    let url = request.url().to_string();
    let state = request.state().to_owned();
    tauri_plugin_opener::open_url(&url, None::<&str>)
        .map_err(|error| format!("could not open Google sign-in: {error}"))?;
    let code = receiver
        .wait_for_code(&state, Duration::from_secs(300))
        .await
        .map_err(|error| error.to_string())?;
    auth.complete(&request, &code, &state).await.map_err(|error| error.to_string())
}

pub fn render_resized_file(
    source: &Path,
    recipe: &str,
    output_dir: &Path,
    file_id: i64,
    max_edge: u32,
    quality: u8,
) -> Result<UploadFile, String> {
    if max_edge == 0 || quality > 100 {
        return Err("invalid resized upload options".into());
    }
    let stem = source.file_stem().and_then(|value| value.to_str()).unwrap_or("photo");
    let output_dir = output_dir.join(file_id.to_string());
    std::fs::create_dir_all(&output_dir)
        .map_err(|error| format!("create upload temp directory: {error}"))?;
    let output = output_dir.join(format!("{stem}.jpg"));
    let options = ExportOptions {
        format: ExportFormat::Jpeg,
        quality,
        resize_max_width: Some(max_edge),
        resize_max_height: Some(max_edge),
        ..ExportOptions::default()
    };
    export_image(source, &output, recipe, &options)
        .map_err(|error| format!("render resized upload: {error}"))?;
    UploadFile::from_path(&output).map_err(|error| error.to_string())
}

pub fn prepare_upload_input(
    catalog_path: &Path,
    file_id: i64,
    size: &UploadSize,
    quality: u8,
    output_dir: &Path,
) -> Result<UploadInput, String> {
    let catalog = Catalog::open(catalog_path).map_err(|error| format!("open catalog: {error}"))?;
    let row: Result<(String, Option<String>), rusqlite::Error> = catalog.conn().query_row(
        "SELECT path, filename FROM files WHERE id = ?1",
        [file_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    );
    let (path, filename) = match row {
        Ok(row) => row,
        Err(error) => {
            return Ok(UploadInput {
                file_id,
                name: format!("Photo {file_id}"),
                file: Err(error.to_string()),
            })
        }
    };
    let source = PathBuf::from(path);
    let name = filename
        .or_else(|| source.file_name().map(|value| value.to_string_lossy().into_owned()))
        .unwrap_or_else(|| format!("Photo {file_id}"));
    let file = match size {
        UploadSize::Original => UploadFile::from_path(&source).map_err(|error| error.to_string()),
        UploadSize::Resized { max_edge } => match catalog.file_recipe(file_id) {
            Ok(recipe) => render_resized_file(
                &source,
                recipe.as_deref().unwrap_or(NO_RECIPE),
                output_dir,
                file_id,
                *max_edge,
                quality,
            ),
            Err(error) => Err(error.to_string()),
        },
    };
    Ok(UploadInput { file_id, name, file })
}

pub async fn run_upload_batch(
    registry: &mut Registry,
    inputs: Vec<UploadInput>,
    options: UploadBatchOptions,
    cancel: &CancelToken,
    on_progress: &(dyn Fn(UploadProgress) + Send + Sync),
) -> UploadBatchResult {
    let UploadBatchOptions { plugin_id, job_id, album, upload_options } = options;
    let total = inputs.len();
    let mut results = Vec::with_capacity(total);
    let active = Arc::new(Mutex::new(None::<(i64, usize, String)>));
    let active_for_retry = Arc::clone(&active);
    let retry_job_id = job_id.clone();
    let progress_callback = on_progress;
    let retry_progress = move |event: ProgressEvent| {
        if let ProgressEvent::Retrying { attempt, error, .. } = event {
            if let Some((file_id, index, name)) =
                active_for_retry.lock().ok().and_then(|active| active.clone())
            {
                progress_callback(UploadProgress {
                    job_id: retry_job_id.clone(),
                    file_id,
                    name,
                    index,
                    total,
                    state: UploadFileState::Retrying,
                    message: Some(format!("Attempt {attempt}: {error}")),
                });
            }
        }
    };

    for (index, input) in inputs.iter().enumerate() {
        if cancel.is_cancelled() {
            results.extend(inputs[index..].iter().map(pending_or_failed));
            break;
        }
        let UploadInput { file_id, file, .. } = input;
        let upload_file = match file {
            Ok(file) => file,
            Err(error) => {
                let result = UploadFileResult {
                    file_id: *file_id,
                    name: input.name.clone(),
                    state: UploadFileState::Failed,
                    error: Some(error.clone()),
                };
                emit_result(&job_id, index, total, &result, on_progress);
                results.push(result);
                continue;
            }
        };
        if let Ok(mut current) = active.lock() {
            *current = Some((*file_id, index, upload_file.name.clone()));
        }
        on_progress(UploadProgress {
            job_id: job_id.clone(),
            file_id: *file_id,
            name: upload_file.name.clone(),
            index,
            total,
            state: UploadFileState::Uploading,
            message: None,
        });
        let job = UploadJob {
            id: job_id.clone(),
            files: vec![upload_file.clone()],
            album: album.clone(),
            options: upload_options,
            retry: Default::default(),
        };
        let (result, pause) = match registry.upload(&plugin_id, job, &retry_progress, cancel).await
        {
            Ok(UploadReport { files, .. }) => (
                match files.first() {
                    Some(file) => UploadFileResult {
                        file_id: *file_id,
                        name: file.name.clone(),
                        state: match file.status {
                            FileStatus::Uploaded => UploadFileState::Uploaded,
                            FileStatus::AlreadyPresent => UploadFileState::AlreadyPresent,
                        },
                        error: None,
                    },
                    None => UploadFileResult {
                        file_id: *file_id,
                        name: upload_file.name.clone(),
                        state: UploadFileState::Failed,
                        error: Some("plugin returned no upload result".into()),
                    },
                },
                false,
            ),
            Err(ExportError::Cancelled) => {
                results.extend(inputs[index..].iter().map(pending_or_failed));
                break;
            }
            Err(error @ ExportError::Transient(_)) => (
                UploadFileResult {
                    file_id: *file_id,
                    name: upload_file.name.clone(),
                    state: UploadFileState::Failed,
                    error: Some(error.to_string()),
                },
                true,
            ),
            Err(error) => (
                UploadFileResult {
                    file_id: *file_id,
                    name: upload_file.name.clone(),
                    state: UploadFileState::Failed,
                    error: Some(error.to_string()),
                },
                false,
            ),
        };
        emit_result(&job_id, index, total, &result, on_progress);
        results.push(result);
        if pause {
            results.extend(inputs[index + 1..].iter().map(pending_or_failed));
            return UploadBatchResult { job_id, files: results, cancelled: false, paused: true };
        }
    }

    UploadBatchResult { job_id, files: results, cancelled: cancel.is_cancelled(), paused: false }
}

fn pending_or_failed(input: &UploadInput) -> UploadFileResult {
    match &input.file {
        Ok(file) => UploadFileResult {
            file_id: input.file_id,
            name: file.name.clone(),
            state: UploadFileState::Pending,
            error: None,
        },
        Err(error) => UploadFileResult {
            file_id: input.file_id,
            name: input.name.clone(),
            state: UploadFileState::Failed,
            error: Some(error.clone()),
        },
    }
}

fn emit_result(
    job_id: &str,
    index: usize,
    total: usize,
    result: &UploadFileResult,
    callback: &(dyn Fn(UploadProgress) + Send + Sync),
) {
    callback(UploadProgress {
        job_id: job_id.to_owned(),
        file_id: result.file_id,
        name: result.name.clone(),
        index,
        total,
        state: result.state,
        message: result.error.clone(),
    });
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn google_ledger_name(client_id: &str, tokens: Option<&OAuthTokens>) -> String {
    let account = tokens
        .map(|tokens| tokens.refresh_token.as_bytes())
        .unwrap_or_else(|| client_id.as_bytes());
    format!("{}.jsonl", sha256_hex(account))
}

struct KeyringTokenStore {
    entry: Entry,
}

impl KeyringTokenStore {
    fn new(client_id: &str) -> Result<Self, ExportError> {
        let entry = Entry::new(KEYRING_SERVICE, client_id).map_err(|_| {
            ExportError::Fatal("cannot open the OS credential store for Google Photos".into())
        })?;
        Ok(Self { entry })
    }
}

impl TokenStore for KeyringTokenStore {
    fn load(&self) -> Result<Option<OAuthTokens>, ExportError> {
        match self.entry.get_password() {
            Ok(json) => OAuthTokens::from_json(&json).map(Some),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(ExportError::Fatal(
                "cannot read Google Photos sign-in from the OS credential store".into(),
            )),
        }
    }

    fn save(&self, tokens: &OAuthTokens) -> Result<(), ExportError> {
        self.entry.set_password(&tokens.to_json()).map_err(|_| {
            ExportError::Fatal(
                "cannot save Google Photos sign-in to the OS credential store".into(),
            )
        })
    }

    fn clear(&self) -> Result<(), ExportError> {
        match self.entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(ExportError::Fatal(
                "cannot remove Google Photos sign-in from the OS credential store".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use fotobase_export::{
        local_folder::LocalFolderPlugin, AlbumTarget, CancelToken, Registry, SizeChoice,
        UploadFile, UploadOptions,
    };

    use super::{
        render_resized_file, run_upload_batch, target_infos, UploadBatchOptions, UploadFileState,
        UploadInput, NO_RECIPE,
    };

    #[test]
    fn target_list_comes_from_all_five_registered_plugins() {
        let targets = target_infos().unwrap();
        let ids = targets.iter().map(|target| target.id.as_str()).collect::<Vec<_>>();

        assert_eq!(ids, ["google-photos", "immich", "local-folder", "s3", "webdav"]);
        assert!(targets
            .iter()
            .all(|target| { target.capabilities.albums && target.capabilities.size_choice }));
    }

    #[test]
    fn google_upload_ledger_is_scoped_to_the_signed_in_account() {
        let account_a = fotobase_export::google_photos::OAuthTokens {
            access_token: "access-a".into(),
            refresh_token: "refresh-a".into(),
            expires_at_unix: 0,
        };
        let account_b = fotobase_export::google_photos::OAuthTokens {
            access_token: "access-b".into(),
            refresh_token: "refresh-b".into(),
            expires_at_unix: 0,
        };

        assert_ne!(
            super::google_ledger_name("client-id", Some(&account_a)),
            super::google_ledger_name("client-id", Some(&account_b))
        );
    }

    #[tokio::test]
    async fn uploads_three_resized_photos_to_local_folder() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let rendered = temp.path().join("rendered");
        let destination = temp.path().join("destination");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&rendered).unwrap();

        let mut files = Vec::new();
        for id in 1..=3 {
            let path = source.join(format!("photo-{id}.png"));
            image::RgbImage::from_pixel(128, 64, image::Rgb([120, 80, 40])).save(&path).unwrap();
            let upload = render_resized_file(&path, NO_RECIPE, &rendered, id, 32, 90).unwrap();
            files.push(UploadInput {
                file_id: id,
                name: format!("photo-{id}.png"),
                file: Ok(upload),
            });
        }

        let mut registry = Registry::new();
        registry.register(Box::new(LocalFolderPlugin::new(&destination))).unwrap();
        let result = run_upload_batch(
            &mut registry,
            files,
            UploadBatchOptions {
                plugin_id: "local-folder".into(),
                job_id: "resize-job".into(),
                album: Some(AlbumTarget::Create("Vacation".into())),
                upload_options: UploadOptions { size: SizeChoice::MaxEdge(32), quality: 90 },
            },
            &CancelToken::new(),
            &|_| {},
        )
        .await;

        assert_eq!(result.files.len(), 3);
        assert!(result.files.iter().all(|file| file.state == UploadFileState::Uploaded));
        for id in 1..=3 {
            let uploaded = destination.join("Vacation").join(format!("photo-{id}.jpg"));
            assert!(uploaded.is_file());
            assert_eq!(image::image_dimensions(uploaded).unwrap(), (32, 16));
        }
    }

    #[tokio::test]
    async fn cancellation_keeps_completed_files_and_marks_remaining_pending() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        std::fs::create_dir_all(&source).unwrap();
        let mut files = Vec::new();
        for id in 1..=3 {
            let path = source.join(format!("photo-{id}.jpg"));
            std::fs::write(&path, format!("photo {id}")).unwrap();
            files.push(UploadInput {
                file_id: id,
                name: format!("photo-{id}.jpg"),
                file: Ok(UploadFile::from_path(&path).unwrap()),
            });
        }

        let mut registry = Registry::new();
        registry.register(Box::new(LocalFolderPlugin::new(&destination))).unwrap();
        let cancel = CancelToken::new();
        let signal = cancel.clone();
        let result = run_upload_batch(
            &mut registry,
            files,
            UploadBatchOptions {
                plugin_id: "local-folder".into(),
                job_id: "cancel-job".into(),
                album: Some(AlbumTarget::Create("Vacation".into())),
                upload_options: UploadOptions::default(),
            },
            &cancel,
            &move |progress| {
                if progress.state == UploadFileState::Uploaded {
                    signal.cancel();
                }
            },
        )
        .await;

        assert!(result.cancelled);
        assert_eq!(
            result.files.iter().map(|file| file.state).collect::<Vec<_>>(),
            vec![UploadFileState::Uploaded, UploadFileState::Pending, UploadFileState::Pending]
        );
        assert!(destination.join("Vacation/photo-1.jpg").is_file());
        assert!(!destination.join("Vacation/photo-2.jpg").exists());
        assert!(!destination.join("Vacation/photo-3.jpg").exists());
        assert!(std::fs::read_dir(destination.join("Vacation")).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".fb-part")));
    }

    #[test]
    fn resized_file_keeps_original_untouched() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("portrait.png");
        let output = temp.path().join("output");
        std::fs::create_dir_all(&output).unwrap();
        image::RgbImage::from_pixel(128, 64, image::Rgb([120, 80, 40])).save(&source).unwrap();
        let original = std::fs::read(&source).unwrap();

        let upload = render_resized_file(&source, NO_RECIPE, &output, 1, 32, 90).unwrap();

        assert_eq!(upload.name, "portrait.jpg");
        assert_eq!(image::image_dimensions(Path::new(&upload.path)).unwrap(), (32, 16));
        assert_eq!(std::fs::read(source).unwrap(), original);
    }
}
