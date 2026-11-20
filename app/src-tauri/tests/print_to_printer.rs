//! TASK-08-005: physical-print path. A fake CUPS (`lpstat`/`lpoptions`/`lp`
//! scripts) stands in for the virtual test printer: the job submitted to it
//! must arrive as a valid PDF with the expected page count.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use fotobase_catalog::Catalog;
use fotobase_lib::print_dialog::{prepare_print, print_to_printer, PrintDialogError, PrintRequest};
use fotobase_lib::printers::{Cups, Spooler as _};
use fotobase_render::print::{PaperSize, PrintLayout, PrintOptions};

fn script(dir: &Path, name: &str, body: &str) {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A fake spooler with printers `Fake` (default, Letter only) and `Other`.
/// `lp` keeps its arguments and the submitted document in `spool`.
fn fake_cups(dir: &Path, spool: &Path) -> Cups {
    script(
        dir,
        "lpstat",
        r#"case "$1" in -e) printf 'Fake\nOther\n';; -d) echo 'system default destination: Fake';; esac"#,
    );
    script(dir, "lpoptions", "echo 'PageSize/Media Size: *Letter Legal'");
    script(
        dir,
        "lp",
        &format!(
            r#"for a; do last="$a"; done
printf '%s\n' "$@" > '{s}/args'
cp "$last" '{s}/job.pdf'
echo 'request id is Fake-7 (1 file(s))'"#,
            s = spool.display()
        ),
    );
    Cups::in_dir(dir)
}

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

fn request(copies: u32, paper: PaperSize) -> PrintRequest {
    PrintRequest {
        copies,
        options: PrintOptions { paper, layout: PrintLayout::L4x6, ..PrintOptions::default() },
    }
}

#[test]
fn lists_printers_with_default_and_paper_mismatch() {
    let (bin, spool) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let cups = fake_cups(bin.path(), spool.path());
    let printers = cups.printers().unwrap();
    assert_eq!(printers.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Fake", "Other"]);
    assert!(printers[0].is_default && !printers[1].is_default);
    assert_eq!(printers[0].papers, [PaperSize::Letter]);
    assert_eq!(printers[0].mismatch, [PaperSize::A4], "A4 job on a Letter-only printer warns");
}

#[test]
fn submits_the_pdf_with_the_expected_page_count() {
    let (bin, spool) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let cups = fake_cups(bin.path(), spool.path());
    let dir = tempfile::tempdir().unwrap();
    let a = gen_jpeg(dir.path(), "a.jpg", 300, 200, [200, 40, 40]);
    let b = gen_jpeg(dir.path(), "b.jpg", 200, 300, [40, 80, 200]);
    let (cat, ids) = catalog_with_photos(dir.path(), &[(a, 300, 200), (b, 200, 300)]);

    // 2 photos × 2 copies on 4×6: two letter sheets.
    let job = prepare_print(&cat, &ids, &request(2, PaperSize::Letter)).unwrap();
    let mut done = Vec::new();
    let batch = print_to_printer(&job, &cups, "Fake", &mut |n| done.push(n), &|| false).unwrap();
    assert_eq!(batch.pages, 2);
    assert_eq!(done, [1, 2]);
    assert_eq!(batch.job_id.as_deref(), Some("Fake-7"));

    let args = std::fs::read_to_string(spool.path().join("args")).unwrap();
    assert!(args.lines().any(|l| l == "Fake"), "{args}");
    assert!(args.lines().any(|l| l == "media=Letter"), "{args}");
    let doc = lopdf::Document::load(spool.path().join("job.pdf")).unwrap();
    assert_eq!(doc.get_pages().len(), 2);
}

#[test]
fn a_cancelled_job_never_reaches_the_printer() {
    let (bin, spool) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let cups = fake_cups(bin.path(), spool.path());
    let dir = tempfile::tempdir().unwrap();
    let a = gen_jpeg(dir.path(), "a.jpg", 300, 200, [200, 40, 40]);
    let (cat, ids) = catalog_with_photos(dir.path(), &[(a, 300, 200)]);
    let job = prepare_print(&cat, &ids, &request(1, PaperSize::Letter)).unwrap();

    let err = print_to_printer(&job, &cups, "Fake", &mut |_| {}, &|| true).unwrap_err();
    assert!(matches!(err, PrintDialogError::Cancelled), "{err}");
    assert!(!spool.path().join("job.pdf").exists());
}

#[test]
fn only_listed_printers_are_accepted() {
    let (bin, spool) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let cups = fake_cups(bin.path(), spool.path());
    let dir = tempfile::tempdir().unwrap();
    let a = gen_jpeg(dir.path(), "a.jpg", 300, 200, [200, 40, 40]);
    let (cat, ids) = catalog_with_photos(dir.path(), &[(a, 300, 200)]);
    let job = prepare_print(&cat, &ids, &request(1, PaperSize::Letter)).unwrap();

    for name in ["Ghost", "-oraw"] {
        assert!(print_to_printer(&job, &cups, name, &mut |_| {}, &|| false).is_err(), "{name}");
    }
    assert!(!spool.path().join("args").exists(), "lp never ran");
}
