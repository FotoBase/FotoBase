use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use async_trait::async_trait;
use fotobase_export::contract::ContractFixture;
use fotobase_export::gallery::{
    builtin_template, generate, GalleryOptions, GalleryPhoto, RenderPipeline,
};
use fotobase_export::local_folder::LocalFolderPlugin;
use fotobase_export::mock::{MockBehavior, MockFixture};
use fotobase_export::publish::{
    generate_token, publish_directory, PublishError, PublishToken, StaticSiteTarget,
};
use fotobase_export::{ContentKey, ExportError, Registry};
use url::Url;

#[derive(Default)]
struct MemoryTarget {
    files: BTreeMap<String, Vec<u8>>,
    writes: Vec<String>,
    removals: Vec<String>,
    write_attempts: usize,
    write_failures_remaining: usize,
}

#[async_trait]
impl StaticSiteTarget for MemoryTarget {
    async fn read_site_file(&mut self, path: &str) -> Result<Option<Vec<u8>>, ExportError> {
        Ok(self.files.get(path).cloned())
    }

    async fn write_site_file(
        &mut self,
        path: &str,
        source: &Path,
        _content_key: &ContentKey,
    ) -> Result<(), ExportError> {
        self.write_attempts += 1;
        if self.write_failures_remaining > 0 {
            self.write_failures_remaining -= 1;
            return Err(ExportError::Transient("simulated network drop".into()));
        }
        self.files.insert(path.into(), fs::read(source)?);
        self.writes.push(path.into());
        Ok(())
    }

    async fn delete_site_file(&mut self, path: &str) -> Result<(), ExportError> {
        self.files.remove(path);
        self.removals.push(path.into());
        Ok(())
    }
}

fn write_site(root: &Path, files: &[(&str, &[u8])]) {
    for (name, bytes) in files {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
}

fn write_jpeg(path: &Path, seed: u8) {
    let image =
        image::RgbImage::from_fn(160, 120, |x, y| image::Rgb([x as u8 ^ seed, y as u8, seed]));
    image.save_with_format(path, image::ImageFormat::Jpeg).unwrap();
}

#[tokio::test]
async fn second_publish_uploads_only_changed_assets_and_prunes_removed_assets() {
    let source = tempfile::tempdir().unwrap();
    write_site(
        source.path(),
        &[
            ("index.html", b"v1"),
            ("images/unchanged.jpg", b"same"),
            ("images/old.jpg", b"old"),
            ("viewer.js", b"viewer"),
        ],
    );
    let token = PublishToken::parse("0123456789abcdef0123456789abcdef").unwrap();
    let base_url = Url::parse("https://photos.example/albums/").unwrap();
    let mut target = MemoryTarget::default();

    let first = publish_directory(&mut target, source.path(), &base_url, &token).await.unwrap();
    assert_eq!(
        first.url.as_str(),
        "https://photos.example/albums/0123456789abcdef0123456789abcdef/index.html"
    );
    assert_eq!(first.uploaded, 4);

    target.writes.clear();
    target.removals.clear();
    fs::write(source.path().join("index.html"), b"v2").unwrap();
    fs::remove_file(source.path().join("images/old.jpg")).unwrap();
    fs::write(source.path().join("images/changed.jpg"), b"new").unwrap();

    let second = publish_directory(&mut target, source.path(), &base_url, &token).await.unwrap();
    let visible_writes: Vec<_> = target
        .writes
        .iter()
        .filter(|path| !path.ends_with(".fotobase-publish-manifest.json"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        visible_writes,
        [
            "0123456789abcdef0123456789abcdef/images/changed.jpg",
            "0123456789abcdef0123456789abcdef/index.html"
        ]
    );
    assert_eq!(target.removals, ["0123456789abcdef0123456789abcdef/images/old.jpg"]);
    assert_eq!(second.uploaded, 2);
    assert_eq!(second.unchanged, 2);
    assert_eq!(second.removed, 1);
}

#[tokio::test]
async fn republishing_gallery_with_one_changed_photo_is_incremental() {
    let sources = tempfile::tempdir().unwrap();
    let site = tempfile::tempdir().unwrap();
    let source_a = sources.path().join("a.jpg");
    let source_b = sources.path().join("b.jpg");
    write_jpeg(&source_a, 10);
    write_jpeg(&source_b, 20);
    let photos = vec![
        GalleryPhoto { source: source_a, title: None, caption: None, recipe: None },
        GalleryPhoto { source: source_b.clone(), title: None, caption: None, recipe: None },
    ];
    let template = builtin_template("grid").unwrap();
    let options =
        GalleryOptions { image_max_edge: 128, thumb_max_edge: 32, ..GalleryOptions::default() };
    generate(site.path(), &photos, &options, &template, &RenderPipeline).unwrap();

    let token = PublishToken::parse("abcdef0123456789abcdef0123456789").unwrap();
    let base_url = Url::parse("https://photos.example/").unwrap();
    let mut target = MemoryTarget::default();
    publish_directory(&mut target, site.path(), &base_url, &token).await.unwrap();

    target.writes.clear();
    target.removals.clear();
    write_jpeg(&source_b, 200);
    generate(site.path(), &photos, &options, &template, &RenderPipeline).unwrap();

    let report = publish_directory(&mut target, site.path(), &base_url, &token).await.unwrap();
    let published_prefix = format!("{}/", token.as_str());
    let updated: Vec<_> = target
        .writes
        .iter()
        .filter_map(|path| path.strip_prefix(&published_prefix))
        .filter(|path| *path != ".fotobase-publish-manifest.json")
        .collect();
    assert_eq!(report.uploaded, 4);
    assert_eq!(report.unchanged, 5);
    assert_eq!(report.removed, 2);
    assert_eq!(updated.len(), 4, "{updated:?}");
    assert!(updated.contains(&"index.html"));
    assert!(updated.contains(&"manifest.json"));
    assert_eq!(updated.iter().filter(|path| path.starts_with("images/")).count(), 1);
    assert_eq!(updated.iter().filter(|path| path.starts_with("thumbs/")).count(), 1);
    assert_eq!(target.removals.len(), 2);
    assert!(target.removals.iter().any(|path| path.contains("/images/")));
    assert!(target.removals.iter().any(|path| path.contains("/thumbs/")));
}

#[tokio::test]
async fn registry_publishes_to_local_folder_at_exact_paths() {
    let source = tempfile::tempdir().unwrap();
    let target_root = tempfile::tempdir().unwrap();
    write_site(source.path(), &[("index.html", b"v1"), ("images/a.jpg", b"old")]);
    let token = PublishToken::parse("abcdef0123456789abcdef0123456789").unwrap();
    let base_url = Url::parse("https://photos.example/").unwrap();
    let mut registry = Registry::new();
    registry.register(Box::new(LocalFolderPlugin::new(target_root.path()))).unwrap();

    registry.publish_directory("local-folder", source.path(), &base_url, &token).await.unwrap();
    assert_eq!(
        fs::read(target_root.path().join(token.as_str()).join("images/a.jpg")).unwrap(),
        b"old"
    );

    fs::write(source.path().join("images/a.jpg"), b"new").unwrap();
    registry.publish_directory("local-folder", source.path(), &base_url, &token).await.unwrap();
    assert_eq!(
        fs::read(target_root.path().join(token.as_str()).join("images/a.jpg")).unwrap(),
        b"new"
    );
    assert!(registry.get_mut("local-folder").unwrap().static_site_target().is_some());
}

#[tokio::test]
async fn invalid_remote_manifest_aborts_before_upload_or_delete() {
    let source = tempfile::tempdir().unwrap();
    write_site(source.path(), &[("index.html", b"new")]);
    let token = PublishToken::parse("abcdef0123456789abcdef0123456789").unwrap();
    let base_url = Url::parse("https://photos.example/").unwrap();
    let manifest = format!("{}/.fotobase-publish-manifest.json", token.as_str());
    let mut target = MemoryTarget::default();
    target.files.insert(
        manifest,
        br#"{"version":1,"files":{"../outside":"0000000000000000000000000000000000000000000000000000000000000000"}}"#
            .to_vec(),
    );

    assert!(publish_directory(&mut target, source.path(), &base_url, &token).await.is_err());
    assert!(target.writes.is_empty());
    assert!(target.removals.is_empty());
}

#[tokio::test]
async fn non_hosting_export_targets_are_rejected_explicitly() {
    let fixture = MockFixture::new(MockBehavior::default());
    let mut registry = Registry::new();
    registry.register(Box::new(fixture.plugin())).unwrap();
    let token = PublishToken::parse("abcdef0123456789abcdef0123456789").unwrap();
    let base_url = Url::parse("https://photos.example/").unwrap();

    let error = registry
        .publish_directory("mock", Path::new("unused"), &base_url, &token)
        .await
        .unwrap_err();

    assert!(matches!(error, PublishError::UnsupportedTarget));
}

#[tokio::test]
async fn transient_target_errors_retry_with_the_shared_upload_policy() {
    let source = tempfile::tempdir().unwrap();
    write_site(source.path(), &[("index.html", b"page")]);
    let token = PublishToken::parse("abcdef0123456789abcdef0123456789").unwrap();
    let base_url = Url::parse("https://photos.example/").unwrap();
    let mut target = MemoryTarget { write_failures_remaining: 1, ..MemoryTarget::default() };

    let report = publish_directory(&mut target, source.path(), &base_url, &token).await.unwrap();

    assert_eq!(report.uploaded, 1);
    assert_eq!(target.write_attempts, 3);
    assert_eq!(target.writes.len(), 2);
}

#[test]
fn generated_tokens_have_128_bits_and_parse_rejects_bad_tokens() {
    let token = generate_token().unwrap();
    let value = token.as_str();
    assert_eq!(value.len(), 32);
    assert!(value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    assert_ne!(generate_token().unwrap(), token);

    for invalid in
        ["", "0123", "../0123456789abcdef0123456789abcdef", "g123456789abcdef0123456789abcdef"]
    {
        assert!(PublishToken::parse(invalid).is_err(), "{invalid:?}");
    }
}
