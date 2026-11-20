//! TASK-08-006: email path. Photos are resized into a scratch folder (edits
//! baked in by the shared export engine, originals untouched) and handed to
//! the OS mail flow; a fake mailer stands in for the platform launchers.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use fotobase_catalog::Catalog;
use fotobase_lib::email_share::{
    deliver, mailto_url, osascript_args, prepare_email, sweep_stale, xdg_email_args, Delivery,
    EmailError, EmailRequest, MailError, Mailer,
};

fn gen_jpeg(dir: &Path, name: &str, w: u32, h: u32, rgb: [u8; 3]) -> PathBuf {
    let p = dir.join(name);
    let buf: Vec<u8> = (0..w * h).flat_map(|_| rgb).collect();
    let mut out = Vec::new();
    {
        use image::ImageEncoder as _;
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92);
        enc.write_image(&buf, w, h, image::ExtendedColorType::Rgb8).unwrap();
    }
    std::fs::write(&p, &out).unwrap();
    p
}

fn catalog_with_photos(dir: &Path, photos: &[(PathBuf, u32, u32)]) -> (Catalog, Vec<i64>) {
    let mut cat = Catalog::open(dir.join("catalog.db")).unwrap();
    let folder = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    let mut ids = Vec::new();
    for (src, w, h) in photos {
        cat.conn()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, width, height, media_type)
                 VALUES (?1, ?2, ?3, 10, 0.0, ?4, ?5, 'image')",
                rusqlite::params![
                    folder,
                    src.to_string_lossy(),
                    src.file_name().unwrap().to_str().unwrap(),
                    w,
                    h
                ],
            )
            .unwrap();
        ids.push(cat.conn().last_insert_rowid());
    }
    (cat, ids)
}

fn dims(p: &Path) -> (u32, u32) {
    image::image_dimensions(p).unwrap()
}

#[test]
fn six_thousand_pixel_photo_becomes_a_resized_temp_jpeg() {
    let lib = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let src = gen_jpeg(lib.path(), "big.jpg", 6000, 4000, [200, 120, 40]);
    let before = std::fs::read(&src).unwrap();
    let (cat, ids) = catalog_with_photos(lib.path(), &[(src.clone(), 6000, 4000)]);

    let prepared =
        prepare_email(&cat, &ids, &EmailRequest { max_edge: 1600 }, root.path(), &mut |_, _| {})
            .unwrap();

    assert_eq!(prepared.files().len(), 1);
    let out = &prepared.files()[0];
    assert!(out.starts_with(root.path()), "resized copy lives under the scratch root");
    let (w, h) = dims(out);
    assert!(w.max(h) <= 1600, "longest edge {w}x{h} within the limit");
    assert_eq!((w, h), (1600, 1067), "aspect ratio kept");
    assert_eq!(std::fs::read(&src).unwrap(), before, "original is byte-identical");
}

#[test]
fn small_photo_is_never_upscaled() {
    let lib = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let src = gen_jpeg(lib.path(), "tiny.jpg", 400, 300, [10, 20, 30]);
    let (cat, ids) = catalog_with_photos(lib.path(), &[(src, 400, 300)]);
    let prepared =
        prepare_email(&cat, &ids, &EmailRequest { max_edge: 1600 }, root.path(), &mut |_, _| {})
            .unwrap();
    assert_eq!(dims(&prepared.files()[0]), (400, 300));
}

#[test]
fn multi_selection_keeps_order_and_reports_progress_and_failures() {
    let lib = tempfile::tempdir().unwrap();
    let sub = lib.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    let root = tempfile::tempdir().unwrap();
    let a = gen_jpeg(lib.path(), "same.jpg", 800, 600, [255, 0, 0]);
    let b = gen_jpeg(&sub, "same.jpg", 600, 800, [0, 255, 0]);
    let missing = lib.path().join("gone.jpg");
    let c = gen_jpeg(lib.path(), "c.jpg", 640, 480, [0, 0, 255]);
    let (cat, ids) = catalog_with_photos(
        lib.path(),
        &[(a, 800, 600), (b, 600, 800), (missing, 10, 10), (c, 640, 480)],
    );

    let mut ticks = Vec::new();
    let prepared =
        prepare_email(&cat, &ids, &EmailRequest { max_edge: 500 }, root.path(), &mut |d, t| {
            ticks.push((d, t))
        })
        .unwrap();

    assert_eq!(prepared.files().len(), 3);
    assert_eq!(prepared.failed.len(), 1);
    assert_eq!(prepared.failed[0].file_id, ids[2]);
    let mut names: Vec<_> =
        prepared.files().iter().map(|f| f.file_name().unwrap().to_owned()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 3, "equal source names stay distinct");
    assert_eq!(dims(&prepared.files()[0]), (500, 375));
    assert_eq!(dims(&prepared.files()[1]), (375, 500));
    assert_eq!(ticks.last(), Some(&(4, 4)));
}

#[test]
fn nothing_to_attach_is_an_error() {
    let lib = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let missing = lib.path().join("gone.jpg");
    let (cat, ids) = catalog_with_photos(lib.path(), &[(missing, 10, 10)]);
    let err =
        prepare_email(&cat, &ids, &EmailRequest { max_edge: 500 }, root.path(), &mut |_, _| {})
            .unwrap_err();
    assert!(matches!(err, EmailError::NothingExported));
    let leftovers = std::fs::read_dir(root.path()).unwrap().count();
    assert_eq!(leftovers, 0, "an empty batch leaves no scratch folder behind");
}

#[derive(Default)]
struct FakeMailer {
    native: Option<&'static str>,
    mailto_fails: bool,
    calls: RefCell<Vec<String>>,
}

impl Mailer for FakeMailer {
    fn compose_with_attachments(&self, files: &[PathBuf]) -> Result<(), MailError> {
        self.calls.borrow_mut().push(format!("native:{}", files.len()));
        match self.native {
            None => Ok(()),
            Some(why) => Err(MailError::Unavailable(why.into())),
        }
    }
    fn open_mailto(&self, url: &str) -> Result<(), MailError> {
        self.calls.borrow_mut().push(format!("mailto:{url}"));
        if self.mailto_fails {
            Err(MailError::Unavailable("no handler".into()))
        } else {
            Ok(())
        }
    }
    fn reveal(&self, path: &Path) -> Result<(), MailError> {
        self.calls.borrow_mut().push(format!("reveal:{}", path.display()));
        Ok(())
    }
}

fn files() -> Vec<PathBuf> {
    vec![PathBuf::from("/t/a.jpg"), PathBuf::from("/t/b.jpg")]
}

#[test]
fn native_composer_attaches_and_skips_the_fallback() {
    let mailer = FakeMailer::default();
    assert_eq!(deliver(&files(), &mailer).unwrap(), Delivery::Attached);
    assert_eq!(*mailer.calls.borrow(), ["native:2"]);
}

#[test]
fn falls_back_to_mailto_and_reveals_the_scratch_folder() {
    let mailer = FakeMailer { native: Some("no mail client"), ..FakeMailer::default() };
    assert_eq!(deliver(&files(), &mailer).unwrap(), Delivery::MailtoRevealed);
    let calls = mailer.calls.borrow();
    assert_eq!(calls.len(), 3);
    assert!(calls[1].starts_with("mailto:mailto:?subject="));
    assert_eq!(calls[2], format!("reveal:{}", PathBuf::from("/t/a.jpg").display()));
}

#[test]
fn both_flows_failing_is_reported() {
    let mailer = FakeMailer { native: Some("x"), mailto_fails: true, ..FakeMailer::default() };
    assert!(deliver(&files(), &mailer).is_err());
}

#[test]
fn mailto_url_percent_encodes_subject_and_body() {
    let url = mailto_url("Photos & more", "a b\nc");
    assert_eq!(url, "mailto:?subject=Photos%20%26%20more&body=a%20b%0Ac");
}

#[test]
fn launcher_arguments_pass_paths_as_separate_arguments() {
    let f = vec![PathBuf::from("/t/a b.jpg"), PathBuf::from("/t/it's \"x\".jpg")];
    let args = xdg_email_args(&f);
    assert_eq!(args, ["--attach", "/t/a b.jpg", "--attach", "/t/it's \"x\".jpg"]);

    let args = osascript_args(&f);
    assert_eq!(args[0], "-e");
    assert!(args[1].contains("tell application \"Mail\""));
    assert_eq!(args[2..], ["/t/a b.jpg", "/t/it's \"x\".jpg"], "paths are argv, never script text");
}

#[test]
fn sweep_removes_only_stale_scratch_folders() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("old");
    let fresh = root.path().join("fresh");
    std::fs::create_dir(&old).unwrap();
    std::fs::create_dir(&fresh).unwrap();
    std::fs::write(old.join("a.jpg"), b"x").unwrap();
    let day_later = SystemTime::now() + Duration::from_secs(25 * 3600);
    // "now" is a day ahead of the folders' mtime, so both look 25 h old …
    sweep_stale(root.path(), day_later, Duration::from_secs(24 * 3600));
    assert!(!old.exists() && !fresh.exists());

    std::fs::create_dir(&fresh).unwrap();
    // … but at the real "now" nothing is stale.
    sweep_stale(root.path(), SystemTime::now(), Duration::from_secs(24 * 3600));
    assert!(fresh.exists());
}
