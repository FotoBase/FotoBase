//! TASK-08-004: print layouts, fit modes and page rasterization (PLAN §2J
//! Print, §6 P4). Layout math is pure data (inches); the fit math is shared
//! by the dialog preview and the final page raster.
//!
//! Regenerate the page golden with
//! `FB_GOLDEN_REGENERATE=1 cargo test -p fotobase-render --test print`
//! (then review the diff before committing).

use std::path::{Path, PathBuf};

use fotobase_render::print::{
    fit_rect, layout_template, plan_pages, render_pages, BorderOptions, FitMode, PaperSize,
    PrintItem, PrintLayout, PrintOptions, PrintPhotoSource, Rect, TextOptions, TextPosition,
    TextSource,
};

const EPS: f64 = 1e-9;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn rect_eq(r: &Rect, x: f64, y: f64, w: f64, h: f64) -> bool {
    close(r.x, x) && close(r.y, y) && close(r.w, w) && close(r.h, h)
}

fn opts(layout: PrintLayout) -> PrintOptions {
    PrintOptions { layout, ..PrintOptions::default() }
}

fn item(id: i64, w: u32, h: u32, copies: u32) -> PrintItem {
    PrintItem { file_id: id, width: w, height: h, copies, label: None }
}

#[test]
fn named_layouts_produce_expected_cell_grids_on_letter() {
    // (layout, cols, rows, cell w, cell h) on a 8.5×11 in page, 0.25 in margin.
    let expected = [
        (PrintLayout::Wallet, 3, 3, 2.5, 3.5),
        (PrintLayout::L3_5x5, 2, 2, 3.5, 5.0),
        (PrintLayout::L4x6, 2, 1, 4.0, 6.0),
        (PrintLayout::L5x7, 1, 1, 5.0, 7.0),
        (PrintLayout::L8x10, 1, 1, 8.0, 10.0),
        (PrintLayout::FullPage, 1, 1, 8.0, 10.5),
    ];
    for (layout, cols, rows, cw, ch) in expected {
        let t = layout_template(layout, PaperSize::Letter, 0);
        assert_eq!((t.cols, t.rows), (cols, rows), "{layout:?}");
        assert!(close(t.cell_w, cw) && close(t.cell_h, ch), "{layout:?}: {t:?}");
        let cells = t.cells();
        assert_eq!(cells.len(), (cols * rows) as usize, "{layout:?}");
        for c in &cells {
            assert!(c.x >= 0.25 - EPS && c.y >= 0.25 - EPS, "{layout:?} inside margin");
            assert!(c.x + c.w <= 8.25 + EPS && c.y + c.h <= 10.75 + EPS, "{layout:?} fits");
        }
    }
    // 4×6 on letter: two cells side by side filling the printable width.
    let cells = layout_template(PrintLayout::L4x6, PaperSize::Letter, 0).cells();
    assert!(rect_eq(&cells[0], 0.25, 2.5, 4.0, 6.0), "{:?}", cells[0]);
    assert!(rect_eq(&cells[1], 4.25, 2.5, 4.0, 6.0), "{:?}", cells[1]);
}

#[test]
fn oversized_print_size_shrinks_to_the_printable_area() {
    // 8×10 does not fit the 7.77 in printable width of A4: scaled, aspect kept.
    let t = layout_template(PrintLayout::L8x10, PaperSize::A4, 0);
    assert_eq!((t.cols, t.rows), (1, 1));
    assert!(t.cell_w <= 8.27 - 0.5 + EPS);
    assert!(close(t.cell_w / t.cell_h, 0.8));
}

#[test]
fn contact_sheet_honors_images_per_page() {
    for per_page in [1u32, 6, 12, 20, 30, 56] {
        let t = layout_template(PrintLayout::ContactSheet, PaperSize::Letter, per_page);
        let cells = t.cells();
        assert_eq!(cells.len(), per_page as usize, "{per_page} per page");
        for (i, a) in cells.iter().enumerate() {
            assert!(
                a.x >= -EPS && a.x + a.w <= 8.5 + EPS && a.y >= -EPS && a.y + a.h <= 11.0 + EPS
            );
            for b in &cells[i + 1..] {
                let apart = a.x + a.w <= b.x + EPS
                    || b.x + b.w <= a.x + EPS
                    || a.y + a.h <= b.y + EPS
                    || b.y + b.h <= a.y + EPS;
                assert!(apart, "cells overlap at {per_page} per page");
            }
        }
    }
}

#[test]
fn shrink_vs_crop_of_a_3_2_image_in_a_4x6_cell() {
    let cell = Rect { x: 0.25, y: 2.5, w: 4.0, h: 6.0 };
    // Crop-to-fit fills the cell: the source loses its left/right sides.
    let crop = fit_rect(3000.0, 2000.0, cell, FitMode::Crop);
    let keep = (4.0 / 6.0) / 1.5; // source width fraction with the cell aspect
    assert!(rect_eq(&crop.dst, 0.25, 2.5, 4.0, 6.0), "{:?}", crop.dst);
    assert!(rect_eq(&crop.src, (1.0 - keep) / 2.0, 0.0, keep, 1.0), "{:?}", crop.src);
    // Shrink-to-fit keeps the whole image, centered with bars above/below.
    let shrink = fit_rect(3000.0, 2000.0, cell, FitMode::Shrink);
    let h = 4.0 / 1.5;
    assert!(rect_eq(&shrink.src, 0.0, 0.0, 1.0, 1.0), "{:?}", shrink.src);
    assert!(rect_eq(&shrink.dst, 0.25, 2.5 + (6.0 - h) / 2.0, 4.0, h), "{:?}", shrink.dst);
}

#[test]
fn copies_repeat_each_photo_in_order_and_paginate() {
    // 2 photos × 2 copies on 4×6 → 4 cells over 2 pages of 2.
    let plan =
        plan_pages(&opts(PrintLayout::L4x6), &[item(1, 3000, 2000, 2), item(2, 2000, 3000, 2)]);
    assert_eq!(plan.len(), 2);
    let ids: Vec<i64> = plan.iter().flat_map(|p| p.cells.iter().map(|c| c.file_id)).collect();
    assert_eq!(ids, [1, 1, 2, 2]);
    assert!(plan.iter().all(|p| close(p.width, 8.5) && close(p.height, 11.0)));
}

#[test]
fn landscape_photo_rotates_into_a_portrait_cell() {
    let plan = plan_pages(&opts(PrintLayout::L4x6), &[item(1, 3000, 2000, 1)]);
    let cell = &plan[0].cells[0];
    assert!(cell.rotated);
    // Rotated, a 3:2 photo matches the 4×6 cell exactly: nothing cropped.
    assert!(rect_eq(&cell.fit.src, 0.0, 0.0, 1.0, 1.0), "{:?}", cell.fit.src);

    let no_rotate = PrintOptions { auto_rotate: false, ..opts(PrintLayout::L4x6) };
    let plan = plan_pages(&no_rotate, &[item(1, 3000, 2000, 1)]);
    assert!(!plan[0].cells[0].rotated);
}

#[test]
fn text_band_sits_above_or_below_and_wrap_grows_it() {
    let label = "a fairly long caption that cannot possibly fit on one line of a wallet print";
    let mut o = opts(PrintLayout::Wallet);
    o.text = TextOptions { source: TextSource::Caption, ..TextOptions::default() };
    let mut it = item(1, 3000, 2000, 1);
    it.label = Some(label.to_string());

    o.text.position = TextPosition::Below;
    let below = plan_pages(&o, std::slice::from_ref(&it));
    let cell = &below[0].cells[0];
    let band = cell.text.as_ref().expect("text band");
    assert!(band.rect.y >= cell.image_box.y + cell.image_box.h - EPS, "below the image");
    assert!(cell.image_box.h < cell.cell.h, "band takes room from the image");

    o.text.position = TextPosition::Above;
    let above = plan_pages(&o, std::slice::from_ref(&it));
    let band_above = above[0].cells[0].text.as_ref().unwrap();
    assert!(band_above.rect.y + band_above.rect.h <= above[0].cells[0].image_box.y + EPS);

    // Wrap on → several lines in a taller band; wrap off → one line.
    o.text.wrap = true;
    let wrapped = plan_pages(&o, std::slice::from_ref(&it));
    let w = wrapped[0].cells[0].text.as_ref().unwrap();
    o.text.wrap = false;
    let single = plan_pages(&o, std::slice::from_ref(&it));
    let s = single[0].cells[0].text.as_ref().unwrap();
    assert!(w.text.contains('\n') && !s.text.contains('\n'));
    assert!(w.rect.h > s.rect.h);

    // No label → no band even with a text source.
    let bare = plan_pages(&o, &[item(1, 3000, 2000, 1)]);
    assert!(bare[0].cells[0].text.is_none());
}

#[test]
fn border_insets_the_image_inside_the_cell() {
    let mut o = opts(PrintLayout::L4x6);
    o.border = BorderOptions { width_in: 0.1, color: "#000000".into() };
    // Crop-to-fit fills the inset box, so the frame hugs the cell.
    o.fit = FitMode::Crop;
    let plan = plan_pages(&o, &[item(1, 2000, 3000, 1)]);
    let cell = &plan[0].cells[0];
    let frame = cell.border.as_ref().expect("border rect");
    assert!(rect_eq(frame, cell.cell.x, cell.cell.y, cell.cell.w, cell.cell.h));
    assert!(close(cell.image_box.w, cell.cell.w - 0.2));
}

// -- raster -----------------------------------------------------------------

fn gen_jpeg(dir: &Path, name: &str, w: u32, h: u32, rgb: [u8; 3]) -> PathBuf {
    let p = dir.join(name);
    let mut buf = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            // A mark in the top-left corner makes orientation visible.
            let corner = x < w / 5 && y < h / 5;
            buf.extend_from_slice(if corner { &[250, 220, 40] } else { &rgb });
        }
    }
    let mut out = Vec::new();
    {
        use image::ImageEncoder as _;
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 95);
        enc.write_image(&buf, w, h, image::ExtendedColorType::Rgb8).unwrap();
    }
    std::fs::write(&p, &out).unwrap();
    p
}

fn sources(dir: &Path) -> (Vec<PrintItem>, Vec<PrintPhotoSource>) {
    let a = gen_jpeg(dir, "a.jpg", 300, 200, [200, 40, 40]);
    let b = gen_jpeg(dir, "b.jpg", 200, 300, [40, 80, 200]);
    let items = vec![item(1, 300, 200, 2), item(2, 200, 300, 2)];
    let srcs = vec![
        PrintPhotoSource { file_id: 1, path: a, recipe_json: String::new() },
        PrintPhotoSource { file_id: 2, path: b, recipe_json: String::new() },
    ];
    (items, srcs)
}

const DPI: u32 = 50;

#[test]
fn two_photos_two_copies_on_4x6_render_four_cells_and_match_the_golden() {
    let dir = tempfile::tempdir().unwrap();
    let (items, srcs) = sources(dir.path());
    let o = opts(PrintLayout::L4x6);
    let mut pages = Vec::new();
    render_pages(&o, &items, &srcs, DPI, &mut |i, page| {
        pages.push((i, page));
        Ok(())
    })
    .unwrap();
    assert_eq!(pages.len(), 2);
    let (w, h) = (425, 550); // 8.5×11 in at 50 dpi
    for (_, p) in &pages {
        assert_eq!((p.width, p.height), (w, h));
    }

    let px = |p: &fotobase_render::print::PageRaster, x: u32, y: u32| {
        let i = (y as usize * p.width as usize + x as usize) * 3;
        [p.rgb[i], p.rgb[i + 1], p.rgb[i + 2]]
    };
    // Page 1: two copies of photo 1 (red), page 2: two copies of photo 2 (blue).
    // Cell centers: (0.25+2, 2.5+3) in and (4.25+2, 2.5+3) in.
    for (idx, want_red) in [(0usize, true), (1, false)] {
        let p = &pages[idx].1;
        for cx in [2.25, 6.25] {
            let c = px(p, (cx * f64::from(DPI)) as u32, (5.5 * f64::from(DPI)) as u32);
            if want_red {
                assert!(c[0] > 150 && c[2] < 100, "red cell: {c:?}");
            } else {
                assert!(c[2] > 150 && c[0] < 100, "blue cell: {c:?}");
            }
        }
        // Paper white outside the cells.
        assert_eq!(px(p, 5, 5), [255, 255, 255]);
    }

    golden("print-4x6-page1.png", &pages[0].1);
}

fn golden(name: &str, page: &fotobase_render::print::PageRaster) {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/print-golden").join(name);
    if std::env::var_os("FB_GOLDEN_REGENERATE").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        image::save_buffer(&path, &page.rgb, page.width, page.height, image::ColorType::Rgb8)
            .unwrap();
        return;
    }
    let want = image::open(&path)
        .unwrap_or_else(|e| {
            panic!("missing golden {}: {e} (FB_GOLDEN_REGENERATE=1)", path.display())
        })
        .to_rgb8();
    assert_eq!((want.width(), want.height()), (page.width, page.height));
    let worst = want
        .as_raw()
        .iter()
        .zip(&page.rgb)
        .map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs())
        .max()
        .unwrap_or(0);
    assert!(worst <= 2, "page differs from golden by {worst}/255");
}

#[test]
fn crop_to_fit_fills_the_cell_and_shrink_leaves_paper_bars() {
    let dir = tempfile::tempdir().unwrap();
    let a = gen_jpeg(dir.path(), "wide.jpg", 300, 200, [200, 40, 40]);
    let items = vec![item(1, 300, 200, 1)];
    let srcs = vec![PrintPhotoSource { file_id: 1, path: a, recipe_json: String::new() }];
    // Without auto-rotate a landscape photo sits in the portrait 4×6 cell.
    let mk = |fit| PrintOptions { fit, auto_rotate: false, ..opts(PrintLayout::L4x6) };
    let render = |o: &PrintOptions| {
        let mut out = None;
        render_pages(o, &items, &srcs, DPI, &mut |_, p| {
            out = Some(p);
            Ok(())
        })
        .unwrap();
        out.unwrap()
    };
    // Cell 1 spans x 0.25..4.25 in, y 2.5..8.5 in; probe near its top edge.
    let probe = |p: &fotobase_render::print::PageRaster| {
        let (x, y) = (2.25 * f64::from(DPI), 2.7 * f64::from(DPI));
        let i = (y as usize * p.width as usize + x as usize) * 3;
        [p.rgb[i], p.rgb[i + 1], p.rgb[i + 2]]
    };
    assert!(probe(&render(&mk(FitMode::Crop)))[0] > 150, "crop fills the cell");
    assert_eq!(probe(&render(&mk(FitMode::Shrink))), [255, 255, 255], "shrink keeps a bar");
}

#[test]
fn border_and_text_band_are_drawn_on_the_page() {
    let dir = tempfile::tempdir().unwrap();
    let a = gen_jpeg(dir.path(), "tall.jpg", 200, 300, [200, 40, 40]);
    let mut it = item(1, 200, 300, 1);
    it.label = Some("Holiday".into());
    let srcs = vec![PrintPhotoSource { file_id: 1, path: a, recipe_json: String::new() }];
    let mut o = opts(PrintLayout::L5x7);
    o.fit = FitMode::Crop;
    o.border = BorderOptions { width_in: 0.2, color: "#0000ff".into() };
    o.text = TextOptions {
        source: TextSource::Caption,
        size_pt: 24.0,
        position: TextPosition::Below,
        ..TextOptions::default()
    };
    let mut page = None;
    render_pages(&o, &[it.clone()], &srcs, 100, &mut |_, p| {
        page = Some(p);
        Ok(())
    })
    .unwrap();
    let page = page.unwrap();
    let plan = &plan_pages(&o, &[it])[0].cells[0];
    let at = |x: f64, y: f64| {
        let i = ((y * 100.0) as usize * page.width as usize + (x * 100.0) as usize) * 3;
        [page.rgb[i], page.rgb[i + 1], page.rgb[i + 2]]
    };
    // Blue frame just inside the outer edge of the border rect, photo inside it.
    let frame = plan.border.unwrap();
    assert_eq!(at(frame.x + 0.05, frame.y + frame.h / 2.0), [0, 0, 255]);
    assert!(at(plan.image_box.x + plan.image_box.w / 2.0, plan.image_box.y + 1.0)[0] > 150);
    // The band holds dark glyph pixels on white paper.
    let band = &plan.text.as_ref().unwrap().rect;
    let mut dark = 0;
    let mut y = band.y;
    while y < band.y + band.h {
        let mut x = band.x;
        while x < band.x + band.w {
            if at(x, y)[0] < 100 {
                dark += 1;
            }
            x += 0.01;
        }
        y += 0.01;
    }
    assert!(dark > 20, "text ink in the band, found {dark}");
}
