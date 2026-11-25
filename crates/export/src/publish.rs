//! Incremental static-site publishing for local-folder, S3-compatible and
//! WebDAV targets.
//!
//! The caller must persist the generated [`PublishToken`] for the album and
//! provide a public base URL that serves the selected target's files.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::{json, Value};
use thiserror::Error;
use url::Url;

use crate::{ContentKey, ExportError, RetryPolicy};

const PUBLISH_MANIFEST: &str = ".fotobase-publish-manifest.json";
const TOKEN_BYTES: usize = 16;

#[derive(Debug, Error)]
pub enum PublishError {
    #[error("invalid publish token")]
    InvalidToken,
    #[error("invalid public base URL")]
    InvalidBaseUrl,
    #[error("site directory is empty")]
    EmptySite,
    #[error("this export target cannot host a static site")]
    UnsupportedTarget,
    #[error("invalid publish manifest: {0}")]
    InvalidManifest(String),
    #[error("unsafe site path `{0}`")]
    UnsafePath(String),
    #[error("I/O: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Export(#[from] ExportError),
}

/// File-level operations required to publish a static site at exact paths.
#[async_trait]
pub trait StaticSiteTarget: Send {
    /// Reads an exact path relative to the target root.
    async fn read_site_file(&mut self, path: &str) -> Result<Option<Vec<u8>>, ExportError>;

    /// Writes or replaces an exact path with bytes from `source`.
    async fn write_site_file(
        &mut self,
        path: &str,
        source: &Path,
        content_key: &ContentKey,
    ) -> Result<(), ExportError>;

    /// Removes an exact file path. Missing files are successful no-ops.
    async fn delete_site_file(&mut self, path: &str) -> Result<(), ExportError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishToken(String);

impl PublishToken {
    pub fn parse(token: &str) -> Result<Self, PublishError> {
        if token.len() != TOKEN_BYTES * 2
            || !token.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(PublishError::InvalidToken);
        }
        Ok(Self(token.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn generate_token() -> Result<PublishToken, PublishError> {
    let mut bytes = [0; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|error| {
        PublishError::Export(ExportError::Fatal(format!(
            "cannot generate a publish token: {error}"
        )))
    })?;
    let token = bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    PublishToken::parse(&token)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishReport {
    pub url: Url,
    /// Public site files uploaded or replaced; excludes the internal diff manifest.
    pub uploaded: usize,
    /// Public site files whose manifest hash matched the previous publish.
    pub unchanged: usize,
    /// Stale public site files removed from the previous publish.
    pub removed: usize,
}

pub async fn publish_directory<T: StaticSiteTarget + ?Sized>(
    target: &mut T,
    source: &Path,
    public_base_url: &Url,
    token: &PublishToken,
) -> Result<PublishReport, PublishError> {
    let url = published_url(public_base_url, token)?;
    let root = source.to_path_buf();
    let files = tokio::task::spawn_blocking(move || collect_site_files(&root)).await.map_err(
        |error| {
            PublishError::Export(ExportError::Fatal(format!(
                "static-site scan worker failed: {error}"
            )))
        },
    )??;
    if files.is_empty() {
        return Err(PublishError::EmptySite);
    }

    let manifest_path = format!("{}/{}", token.as_str(), PUBLISH_MANIFEST);
    let previous_bytes = match call_with_retry(target, SiteOperation::Read(&manifest_path)).await? {
        SiteResult::Read(bytes) => bytes,
        _ => unreachable!("read operation returns read result"),
    };
    let previous = previous_bytes.as_deref().map(parse_manifest).transpose()?.unwrap_or_default();
    let current: BTreeMap<String, String> =
        files.iter().map(|(path, _, key)| (path.clone(), key.to_string())).collect();
    let (to_upload, stale) = manifest_diff(&previous, &current);
    let upload_paths: BTreeSet<_> = to_upload.iter().map(String::as_str).collect();

    let mut report = PublishReport { url, uploaded: 0, unchanged: 0, removed: 0 };
    let mut paths: Vec<_> = files.iter().collect();
    paths.sort_by_key(|(path, _, _)| path == "index.html");
    for (path, file, key) in paths {
        if !upload_paths.contains(path.as_str()) {
            report.unchanged += 1;
            continue;
        }
        call_with_retry(
            target,
            SiteOperation::Write(&format!("{}/{path}", token.as_str()), file, key),
        )
        .await?;
        report.uploaded += 1;
    }

    for stale in &stale {
        call_with_retry(target, SiteOperation::Delete(&format!("{}/{stale}", token.as_str())))
            .await?;
        report.removed += 1;
    }

    let manifest = serde_json::to_vec(&json!({ "version": 1, "files": current }))
        .map_err(|error| PublishError::InvalidManifest(error.to_string()))?;
    if previous_bytes.as_deref() != Some(manifest.as_slice()) {
        let temp = tempfile::NamedTempFile::new()?;
        fs::write(temp.path(), &manifest)?;
        let key = ContentKey::from_bytes(&manifest);
        call_with_retry(target, SiteOperation::Write(&manifest_path, temp.path(), &key)).await?;
    }

    Ok(report)
}

enum SiteOperation<'a> {
    Read(&'a str),
    Write(&'a str, &'a Path, &'a ContentKey),
    Delete(&'a str),
}

enum SiteResult {
    Read(Option<Vec<u8>>),
    Done,
}

async fn call_with_retry<T: StaticSiteTarget + ?Sized>(
    target: &mut T,
    operation: SiteOperation<'_>,
) -> Result<SiteResult, PublishError> {
    let retry = RetryPolicy::default();
    let max_attempts = retry.max_attempts.max(1);
    let mut attempt = 1;
    loop {
        let result = match &operation {
            SiteOperation::Read(path) => target.read_site_file(path).await.map(SiteResult::Read),
            SiteOperation::Write(path, source, key) => {
                target.write_site_file(path, source, key).await.map(|()| SiteResult::Done)
            }
            SiteOperation::Delete(path) => {
                target.delete_site_file(path).await.map(|()| SiteResult::Done)
            }
        };
        match result {
            Err(ExportError::Transient(_)) if attempt < max_attempts => {
                let delay = crate::types::retry_delay(retry, attempt);
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                attempt += 1;
            }
            Err(error) => return Err(error.into()),
            Ok(result) => return Ok(result),
        }
    }
}

fn published_url(base: &Url, token: &PublishToken) -> Result<Url, PublishError> {
    if !matches!(base.scheme(), "http" | "https")
        || base.host_str().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(PublishError::InvalidBaseUrl);
    }
    let mut base = base.clone();
    if !base.path().ends_with('/') {
        base.set_path(&format!("{}/", base.path()));
    }
    base.join(&format!("{}/index.html", token.as_str())).map_err(|_| PublishError::InvalidBaseUrl)
}

fn collect_site_files(root: &Path) -> Result<Vec<(String, PathBuf, ContentKey)>, PublishError> {
    fn walk(
        root: &Path,
        dir: &Path,
        files: &mut Vec<(String, PathBuf, ContentKey)>,
    ) -> Result<(), PublishError> {
        let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                return Err(PublishError::UnsafePath(path.display().to_string()));
            }
            if kind.is_dir() {
                walk(root, &path, files)?;
            } else if kind.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| PublishError::UnsafePath(path.display().to_string()))?;
                let name = relative
                    .components()
                    .map(|component| component.as_os_str().to_str())
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| PublishError::UnsafePath(path.display().to_string()))?
                    .join("/");
                validate_site_path(&name)?;
                if name == PUBLISH_MANIFEST {
                    return Err(PublishError::UnsafePath(name));
                }
                let key = ContentKey::from_reader(fs::File::open(&path)?)?;
                files.push((name, path, key));
            } else {
                return Err(PublishError::UnsafePath(path.display().to_string()));
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    Ok(files)
}

pub(crate) fn validate_site_path(path: &str) -> Result<(), ExportError> {
    let invalid = path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path.chars().any(char::is_control)
        || path.split('/').any(|part| part.is_empty() || part == "." || part == "..");
    if invalid {
        return Err(ExportError::InvalidConfig(format!("unsafe static-site path `{path}`")));
    }
    Ok(())
}

fn parse_manifest(bytes: &[u8]) -> Result<BTreeMap<String, String>, PublishError> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| PublishError::InvalidManifest(error.to_string()))?;
    if value.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(PublishError::InvalidManifest("unsupported version".into()));
    }
    let entries = value
        .get("files")
        .and_then(Value::as_object)
        .ok_or_else(|| PublishError::InvalidManifest("missing files object".into()))?;
    let mut files = BTreeMap::new();
    for (path, hash) in entries {
        validate_site_path(path)
            .map_err(|error| PublishError::InvalidManifest(error.to_string()))?;
        if path == PUBLISH_MANIFEST {
            return Err(PublishError::InvalidManifest(format!(
                "reserved path `{PUBLISH_MANIFEST}`"
            )));
        }
        let hash = hash
            .as_str()
            .filter(|value| {
                value.len() == 64
                    && value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            })
            .ok_or_else(|| PublishError::InvalidManifest(format!("invalid hash for `{path}`")))?;
        files.insert(path.clone(), hash.to_owned());
    }
    Ok(files)
}

fn manifest_diff(
    previous: &BTreeMap<String, String>,
    current: &BTreeMap<String, String>,
) -> (Vec<String>, Vec<String>) {
    let uploads = current
        .iter()
        .filter(|(path, hash)| previous.get(*path) != Some(*hash))
        .map(|(path, _)| path.clone())
        .collect();
    let removals = previous.keys().filter(|path| !current.contains_key(*path)).cloned().collect();
    (uploads, removals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_diff_tracks_only_changed_added_and_removed_paths() {
        let cases = [
            (
                BTreeMap::from([("a".into(), "1".into()), ("b".into(), "2".into())]),
                BTreeMap::from([
                    ("a".into(), "1".into()),
                    ("b".into(), "3".into()),
                    ("c".into(), "4".into()),
                ]),
                vec!["b", "c"],
                vec![],
            ),
            (BTreeMap::from([("old".into(), "1".into())]), BTreeMap::new(), vec![], vec!["old"]),
            (BTreeMap::new(), BTreeMap::new(), vec![], vec![]),
        ];
        for (previous, current, expected_uploads, expected_removals) in cases {
            let (uploads, removals) = manifest_diff(&previous, &current);
            assert_eq!(uploads, expected_uploads);
            assert_eq!(removals, expected_removals);
        }
    }
}
