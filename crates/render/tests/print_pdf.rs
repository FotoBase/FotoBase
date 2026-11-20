//! TASK-08-005: the PDF sink of the print layout engine. One rendering
//! path (`render_pages`), two sinks — PNG sheets and a PDF — so the PDF
//! carries exactly the rasters of the physical print.

use std::path::Path;

use fotobase_render::print::{
    plan_pages, render_pages, write_pdf, PaperSize, PrintItem, PrintLayout, PrintOptions,
    PrintPhotoSource,
};

fn gen_jpeg(dir: &Path, name: &str, w: u32, h: u32, rgb: [u8; 3]) -> std::path::PathBuf {
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

fn job(dir: &Path) -> (Vec<PrintItem>, Vec<PrintPhotoSource>) {
    let a = gen_jpeg(dir, "a.jpg", 300, 200, [200, 40, 40]);
    let b = gen_jpeg(dir, "b.jpg", 200, 300, [40, 80, 200]);
    let items = vec![
        PrintItem { file_id: 1, width: 300, height: 200, copies: 1, label: None },
        PrintItem { file_id: 2, width: 200, height: 300, copies: 1, label: None },
    ];
    let sources = vec![
        PrintPhotoSource { file_id: 1, path: a, recipe_json: String::new() },
        PrintPhotoSource { file_id: 2, path: b, recipe_json: String::new() },
    ];
    (items, sources)
}

fn opts(paper: PaperSize) -> PrintOptions {
    PrintOptions { paper, layout: PrintLayout::L4x6, ..PrintOptions::default() }
}

/// Page object ids of the document, in page order.
fn pages(doc: &lopdf::Document) -> Vec<lopdf::ObjectId> {
    doc.get_pages().values().copied().collect()
}

fn media_box(doc: &lopdf::Document, page: lopdf::ObjectId) -> Vec<f64> {
    let dict = doc.get_dictionary(page).unwrap();
    dict.get(b"MediaBox")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o.as_float().map(f64::from).unwrap())
        .collect()
}

#[test]
fn two_photo_4x6_layout_is_a_one_page_letter_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let (items, sources) = job(dir.path());
    let out = dir.path().join("sheet.pdf");
    let mut done = Vec::new();
    let failed =
        write_pdf(&opts(PaperSize::Letter), &items, &sources, 100, &out, &mut |n| done.push(n))
            .unwrap();
    assert!(failed.is_empty());
    assert_eq!(done, vec![1]);

    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    let doc = lopdf::Document::load_mem(&bytes).expect("valid PDF");
    let pages = pages(&doc);
    assert_eq!(pages.len(), 1, "2 photos of 4×6 share one letter sheet");
    let mb = media_box(&doc, pages[0]);
    assert_eq!(mb[..2], [0.0, 0.0]);
    assert!((mb[2] - 612.0).abs() < 0.01 && (mb[3] - 792.0).abs() < 0.01, "{mb:?}");
}

#[test]
fn a4_pdf_uses_the_a4_media_box() {
    let dir = tempfile::tempdir().unwrap();
    let (items, sources) = job(dir.path());
    let out = dir.path().join("a4.pdf");
    write_pdf(&opts(PaperSize::A4), &items, &sources, 100, &out, &mut |_| {}).unwrap();
    let doc = lopdf::Document::load(&out).unwrap();
    let mb = media_box(&doc, pages(&doc)[0]);
    assert!((mb[2] - 595.28).abs() < 0.01 && (mb[3] - 841.89).abs() < 0.01, "{mb:?}");
}

#[test]
fn pdf_pages_embed_the_same_rasters_as_the_png_sink() {
    let dir = tempfile::tempdir().unwrap();
    let (items, sources) = job(dir.path());
    let o = opts(PaperSize::Letter);
    let out = dir.path().join("sheet.pdf");
    write_pdf(&o, &items, &sources, 100, &out, &mut |_| {}).unwrap();

    // The raster dimensions the PNG path would write for the same plan.
    let mut sizes = Vec::new();
    render_pages(&o, &items, &sources, 100, &mut |_, page| {
        sizes.push((page.width, page.height));
        Ok(())
    })
    .unwrap();
    assert_eq!(sizes.len(), plan_pages(&o, &items).len());

    let doc = lopdf::Document::load(&out).unwrap();
    let images: Vec<(i64, i64)> = doc
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .filter(|s| s.dict.get(b"Subtype").and_then(|v| v.as_name()).ok() == Some(b"Image"))
        .map(|s| {
            (
                s.dict.get(b"Width").unwrap().as_i64().unwrap(),
                s.dict.get(b"Height").unwrap().as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(images, vec![(i64::from(sizes[0].0), i64::from(sizes[0].1))]);
}

#[test]
fn many_photos_make_many_pages_and_leave_no_tmp() {
    let dir = tempfile::tempdir().unwrap();
    let (mut items, sources) = job(dir.path());
    items[0].copies = 3; // 4 4×6 cells → 2 sheets at 2 per letter page
    let out = dir.path().join("many.pdf");
    let mut done = Vec::new();
    write_pdf(&opts(PaperSize::Letter), &items, &sources, 72, &out, &mut |n| done.push(n)).unwrap();
    assert_eq!(done, vec![1, 2]);
    let doc = lopdf::Document::load(&out).unwrap();
    assert_eq!(pages(&doc).len(), 2);
    let stray: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "tmp"))
        .collect();
    assert!(stray.is_empty(), "{stray:?}");
}
