//! Print layouts, fit modes and page rasterization (TASK-08-004, PLAN §2J
//! Print, §6 P4).
//!
//! Everything geometric is data in inches on a portrait page:
//! [`layout_template`] turns a named layout into a cell grid, [`fit_rect`]
//! (shrink-to-fit = contain, crop-to-fit = cover) is the single fit
//! implementation, and [`plan_pages`] composes cells, borders and text bands
//! into a [`PagePlan`]. The dialog's live preview positions its images from
//! that plan; [`render_pages`] rasterizes the same plan at print resolution —
//! so preview and paper cannot disagree (PLAN §3 rule 5). Photos reach the
//! page through the shared recipe engine, so edits are baked in; text bands
//! reuse the `text` op rasterizer (TASK-08-003).
//!
//! The output is a page raster per sheet. Handing it to a printer or a PDF
//! writer is TASK-08-005.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fotobase_core::buffer::LinearImage;
use fotobase_core::recipe::Op;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use thiserror::Error;

use crate::export::{encode_png, to_srgb8, ExportError};
use crate::{decode_raw, fit, icc, resize, rgb_to_linear};

/// Tolerance for floating-point grid math.
const EPS: f64 = 1e-9;

/// Resolution at which text is measured while planning. Widths scale
/// linearly with the font size, so the plan is independent of the output
/// dpi — preview and final page wrap identically.
const MEASURE_DPI: f64 = 300.0;

/// Padding around the text inside its band, inches.
const TEXT_PAD_IN: f64 = 0.02;

/// A text band never takes more than this share of its cell's height.
const MAX_BAND_SHARE: f64 = 0.4;

const NO_RECIPE: &str = r#"{"spec":"fotobase.recipe","version":1,"ops":[]}"#;

/// A rectangle in inches on the page (or normalized 0..1 on a source image).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn inset(self, d: f64) -> Rect {
        Rect { x: self.x + d, y: self.y + d, w: self.w - 2.0 * d, h: self.h - 2.0 * d }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperSize {
    Letter,
    A4,
}

impl PaperSize {
    /// Portrait width × height in inches.
    pub fn inches(self) -> (f64, f64) {
        match self {
            PaperSize::Letter => (8.5, 11.0),
            PaperSize::A4 => (210.0 / 25.4, 297.0 / 25.4),
        }
    }
}

/// Picasa's print sizes plus the contact sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrintLayout {
    #[serde(rename = "wallet")]
    Wallet,
    #[serde(rename = "3.5x5")]
    L3_5x5,
    #[serde(rename = "4x6")]
    L4x6,
    #[serde(rename = "5x7")]
    L5x7,
    #[serde(rename = "8x10")]
    L8x10,
    #[serde(rename = "full_page")]
    FullPage,
    #[serde(rename = "contact_sheet")]
    ContactSheet,
}

/// Shrink-to-fit keeps the whole image; crop-to-fit fills the cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FitMode {
    Shrink,
    Crop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextSource {
    None,
    Caption,
    Filename,
    Exif,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextPosition {
    Above,
    Below,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BorderOptions {
    /// Frame width in inches around each image; 0 = no border.
    pub width_in: f64,
    /// `#rrggbb`.
    pub color: String,
}

impl Default for BorderOptions {
    fn default() -> Self {
        Self { width_in: 0.0, color: "#000000".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextOptions {
    pub source: TextSource,
    /// `sans`, `serif`, `mono` or an installed family (see the `text` op).
    pub font: String,
    /// `#rrggbb`.
    pub color: String,
    pub size_pt: f64,
    pub position: TextPosition,
    pub wrap: bool,
}

impl Default for TextOptions {
    fn default() -> Self {
        Self {
            source: TextSource::None,
            font: "sans".into(),
            color: "#000000".into(),
            size_pt: 10.0,
            position: TextPosition::Below,
            wrap: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrintOptions {
    pub paper: PaperSize,
    pub layout: PrintLayout,
    /// Contact sheet only.
    pub images_per_page: u32,
    pub fit: FitMode,
    pub border: BorderOptions,
    pub text: TextOptions,
    /// Turn a photo 90° when its orientation differs from its cell's.
    pub auto_rotate: bool,
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            paper: PaperSize::Letter,
            layout: PrintLayout::L4x6,
            images_per_page: 20,
            fit: FitMode::Shrink,
            border: BorderOptions::default(),
            text: TextOptions::default(),
            auto_rotate: true,
        }
    }
}

/// One photo to print: its edited pixel size (only the aspect matters for
/// planning), copies, and the resolved text line (caption / file name /
/// EXIF summary) when the text source asks for one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintItem {
    pub file_id: i64,
    pub width: u32,
    pub height: u32,
    pub copies: u32,
    pub label: Option<String>,
}

/// Where a photo's pixels come from.
#[derive(Debug, Clone)]
pub struct PrintPhotoSource {
    pub file_id: i64,
    pub path: PathBuf,
    /// Edit recipe JSON; empty = unedited.
    pub recipe_json: String,
}

// -- layout templates ------------------------------------------------------

/// A named layout resolved against a paper size: a centered grid of equal
/// cells.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LayoutTemplate {
    pub cols: u32,
    pub rows: u32,
    pub cell_w: f64,
    pub cell_h: f64,
    pub x0: f64,
    pub y0: f64,
    pub gap: f64,
    /// Cells used per page (the contact sheet may leave grid slots empty).
    pub count: u32,
}

impl LayoutTemplate {
    /// Cell rectangles, row-major.
    pub fn cells(&self) -> Vec<Rect> {
        (0..self.count)
            .map(|i| {
                let (col, row) = (i % self.cols, i / self.cols);
                Rect {
                    x: self.x0 + f64::from(col) * (self.cell_w + self.gap),
                    y: self.y0 + f64::from(row) * (self.cell_h + self.gap),
                    w: self.cell_w,
                    h: self.cell_h,
                }
            })
            .collect()
    }
}

/// Maximum images per contact-sheet page.
pub const MAX_IMAGES_PER_PAGE: u32 = 100;

/// Resolve `layout` on `paper`. Fixed print sizes tile the printable area
/// with a 0.25 in margin (and shrink, aspect kept, if one cell exceeds it);
/// the contact sheet picks the grid with the largest cells for
/// `images_per_page`.
pub fn layout_template(
    layout: PrintLayout,
    paper: PaperSize,
    images_per_page: u32,
) -> LayoutTemplate {
    let (pw, ph) = paper.inches();
    let fixed = match layout {
        PrintLayout::Wallet => Some((2.5, 3.5)),
        PrintLayout::L3_5x5 => Some((3.5, 5.0)),
        PrintLayout::L4x6 => Some((4.0, 6.0)),
        PrintLayout::L5x7 => Some((5.0, 7.0)),
        PrintLayout::L8x10 => Some((8.0, 10.0)),
        PrintLayout::FullPage | PrintLayout::ContactSheet => None,
    };
    let (margin, gap) = if layout == PrintLayout::ContactSheet { (0.5, 0.1) } else { (0.25, 0.0) };
    let (uw, uh) = (pw - 2.0 * margin, ph - 2.0 * margin);

    let (cols, rows, cell_w, cell_h, count) = match (layout, fixed) {
        (_, Some((mut cw, mut ch))) => {
            if cw > uw + EPS || ch > uh + EPS {
                let s = (uw / cw).min(uh / ch);
                cw *= s;
                ch *= s;
            }
            let cols = (((uw + gap) / (cw + gap) + EPS).floor() as u32).max(1);
            let rows = (((uh + gap) / (ch + gap) + EPS).floor() as u32).max(1);
            (cols, rows, cw, ch, cols * rows)
        }
        (PrintLayout::ContactSheet, None) => {
            let n = images_per_page.clamp(1, MAX_IMAGES_PER_PAGE);
            let mut best: Option<(u32, u32, f64, f64)> = None;
            for cols in 1..=n {
                let rows = n.div_ceil(cols);
                let cw = (uw - f64::from(cols - 1) * gap) / f64::from(cols);
                let ch = (uh - f64::from(rows - 1) * gap) / f64::from(rows);
                if best.is_none_or(|(_, _, bw, bh)| cw * ch > bw * bh + EPS) {
                    best = Some((cols, rows, cw, ch));
                }
            }
            let (cols, rows, cw, ch) = best.expect("n >= 1");
            (cols, rows, cw, ch, n)
        }
        _ => (1, 1, uw, uh, 1),
    };
    let grid_w = f64::from(cols) * cell_w + f64::from(cols - 1) * gap;
    let grid_h = f64::from(rows) * cell_h + f64::from(rows - 1) * gap;
    LayoutTemplate {
        cols,
        rows,
        cell_w,
        cell_h,
        x0: margin + (uw - grid_w) / 2.0,
        y0: margin + (uh - grid_h) / 2.0,
        gap,
        count,
    }
}

// -- fit -------------------------------------------------------------------

/// Result of fitting an image into a box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Fit {
    /// Visible part of the source, normalized 0..1.
    pub src: Rect,
    /// Where it lands, in the units of the box passed to [`fit_rect`].
    pub dst: Rect,
}

/// The one fit implementation, shared by the preview and the page raster.
pub fn fit_rect(src_w: f64, src_h: f64, target: Rect, mode: FitMode) -> Fit {
    let full = Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };
    if src_w <= 0.0 || src_h <= 0.0 || target.w <= 0.0 || target.h <= 0.0 {
        return Fit { src: full, dst: target };
    }
    let (src_aspect, box_aspect) = (src_w / src_h, target.w / target.h);
    match mode {
        FitMode::Shrink => {
            let (w, h) = if src_aspect > box_aspect {
                (target.w, target.w / src_aspect)
            } else {
                (target.h * src_aspect, target.h)
            };
            Fit {
                src: full,
                dst: Rect {
                    x: target.x + (target.w - w) / 2.0,
                    y: target.y + (target.h - h) / 2.0,
                    w,
                    h,
                },
            }
        }
        FitMode::Crop => {
            let src = if src_aspect > box_aspect {
                let keep = box_aspect / src_aspect;
                Rect { x: (1.0 - keep) / 2.0, y: 0.0, w: keep, h: 1.0 }
            } else {
                let keep = src_aspect / box_aspect;
                Rect { x: 0.0, y: (1.0 - keep) / 2.0, w: 1.0, h: keep }
            };
            Fit { src, dst: target }
        }
    }
}

// -- plan ------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TextPlan {
    pub rect: Rect,
    /// Wrapped text; lines are separated by `\n`.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CellPlan {
    pub file_id: i64,
    pub cell: Rect,
    /// Space the photo may use (cell minus text band and border).
    pub image_box: Rect,
    /// Fit of the (possibly rotated) photo into `image_box`; `src` is
    /// relative to the rotated photo.
    pub fit: Fit,
    /// The photo is turned 90° clockwise before fitting.
    pub rotated: bool,
    /// Frame around the photo, when a border width is set.
    pub border: Option<Rect>,
    pub text: Option<TextPlan>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PagePlan {
    pub width: f64,
    pub height: f64,
    pub cells: Vec<CellPlan>,
}

fn text_band(text: &TextOptions, label: &str, cell: Rect) -> Option<(String, f64)> {
    let label = label.trim();
    if text.source == TextSource::None || label.is_empty() {
        return None;
    }
    let size_in = text.size_pt.clamp(4.0, 72.0) / 72.0;
    let ref_px = (size_in * MEASURE_DPI) as f32;
    let body = if text.wrap {
        let avail = (cell.w - 2.0 * TEXT_PAD_IN).max(0.1);
        fotobase_core::text::wrap_text(&text.font, label, ref_px, (avail * MEASURE_DPI) as f32)
    } else {
        label.replace('\n', " ")
    };
    let lines = body.split('\n').count();
    let height_in =
        f64::from(fotobase_core::text::text_height_px(&text.font, lines, ref_px)) / MEASURE_DPI;
    let band = (height_in + 2.0 * TEXT_PAD_IN).min(cell.h * MAX_BAND_SHARE);
    Some((body, band))
}

fn plan_cell(opts: &PrintOptions, item: &PrintItem, cell: Rect) -> CellPlan {
    let mut image_area = cell;
    let mut text = None;
    let label = item.label.as_deref().unwrap_or("");
    if let Some((body, band)) = text_band(&opts.text, label, cell) {
        let (rect, area) = match opts.text.position {
            TextPosition::Above => (
                Rect { x: cell.x, y: cell.y, w: cell.w, h: band },
                Rect { x: cell.x, y: cell.y + band, w: cell.w, h: cell.h - band },
            ),
            TextPosition::Below => (
                Rect { x: cell.x, y: cell.y + cell.h - band, w: cell.w, h: band },
                Rect { x: cell.x, y: cell.y, w: cell.w, h: cell.h - band },
            ),
        };
        image_area = area;
        text = Some(TextPlan { rect, text: body });
    }

    let bw = opts.border.width_in.max(0.0);
    let framed = bw > 0.0 && image_area.w > 2.0 * bw && image_area.h > 2.0 * bw;
    let image_box = if framed { image_area.inset(bw) } else { image_area };

    let (w, h) = (f64::from(item.width), f64::from(item.height));
    let box_square = (image_box.w - image_box.h).abs() < EPS;
    let rotated =
        opts.auto_rotate && w != h && !box_square && (w > h) != (image_box.w > image_box.h);
    let (sw, sh) = if rotated { (h, w) } else { (w, h) };
    let fit = fit_rect(sw, sh, image_box, opts.fit);
    let border = framed.then(|| fit.dst.inset(-bw));

    CellPlan { file_id: item.file_id, cell, image_box, fit, rotated, border, text }
}

/// Lay `items` (each repeated `copies` times, in order) out on as many
/// pages as the layout needs.
pub fn plan_pages(opts: &PrintOptions, items: &[PrintItem]) -> Vec<PagePlan> {
    let (width, height) = opts.paper.inches();
    let template = layout_template(opts.layout, opts.paper, opts.images_per_page);
    let cells = template.cells();
    let slots: Vec<&PrintItem> = items
        .iter()
        .flat_map(|it| std::iter::repeat_n(it, it.copies.clamp(1, 99) as usize))
        .collect();
    slots
        .chunks(cells.len().max(1))
        .map(|chunk| PagePlan {
            width,
            height,
            cells: chunk.iter().zip(&cells).map(|(it, c)| plan_cell(opts, it, *c)).collect(),
        })
        .collect()
}

/// Pixel size of the (unrotated) edited photo, scaled down to a 96 px
/// proxy: the recipe runs on a blank image of the source's aspect, so crop,
/// rotate and straighten change the answer exactly as in the engine without
/// decoding the photo. Only the aspect is meaningful.
pub fn edited_dimensions(width: u32, height: u32, recipe_json: &str) -> (u32, u32) {
    let (pw, ph) = fit(width.max(1), height.max(1), 96);
    if recipe_json.trim().is_empty() {
        return (pw, ph);
    }
    match fotobase_core::recipe::parse(recipe_json) {
        Ok((recipe, _)) if !recipe.ops.is_empty() => {
            let out = fotobase_core::apply::apply_recipe(&LinearImage::new(pw, ph), &recipe);
            (out.width, out.height)
        }
        _ => (pw, ph),
    }
}

// -- raster ----------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PageRaster {
    pub rgb: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// A photo that could not be placed; its cells stay blank.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrintFailure {
    pub file_id: i64,
    pub error: String,
}

#[derive(Debug, Error)]
pub enum PrintError {
    #[error(transparent)]
    Export(#[from] ExportError),
    #[error("I/O during print: {0}")]
    Io(#[from] std::io::Error),
}

struct Loaded {
    rgb: Vec<u8>,
    width: u32,
    height: u32,
}

fn px(inches: f64, dpi: u32) -> i64 {
    (inches * f64::from(dpi)).round() as i64
}

/// Pixel box `(x0, y0, x1, y1)` of `r`; edges are rounded from absolute
/// coordinates so neighbouring cells tile without gaps.
fn px_box(r: Rect, dpi: u32) -> (i64, i64, i64, i64) {
    (px(r.x, dpi), px(r.y, dpi), px(r.x + r.w, dpi), px(r.y + r.h, dpi))
}

fn hex_rgb(hex: &str, default: [u8; 3]) -> [u8; 3] {
    fotobase_core::apply::parse_hex_color(hex)
        .map_or(default, |c| [0, 1, 2].map(|i| (c[i].clamp(0.0, 1.0) * 255.0).round() as u8))
}

fn fill_rect(page: &mut PageRaster, (x0, y0, x1, y1): (i64, i64, i64, i64), color: [u8; 3]) {
    let (pw, ph) = (i64::from(page.width), i64::from(page.height));
    for y in y0.max(0)..y1.min(ph) {
        for x in x0.max(0)..x1.min(pw) {
            let i = (y as usize * page.width as usize + x as usize) * 3;
            page.rgb[i..i + 3].copy_from_slice(&color);
        }
    }
}

fn blit(page: &mut PageRaster, x0: i64, y0: i64, rgb: &[u8], w: u32, h: u32) {
    let (pw, ph) = (i64::from(page.width), i64::from(page.height));
    for sy in 0..i64::from(h) {
        let y = y0 + sy;
        if y < 0 || y >= ph {
            continue;
        }
        let from = x0.max(0) - x0;
        let to = (pw - x0).min(i64::from(w));
        if from >= to {
            continue;
        }
        let src = (sy as usize * w as usize + from as usize) * 3
            ..(sy as usize * w as usize + to as usize) * 3;
        let dst = (y as usize * page.width as usize + (x0 + from) as usize) * 3;
        page.rgb[dst..dst + src.len()].copy_from_slice(&rgb[src]);
    }
}

fn rotate_cw(rgb: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let mut out = vec![0u8; rgb.len()];
    for y in 0..h {
        for x in 0..w {
            let to = (x * h + (h - 1 - y)) * 3;
            out[to..to + 3].copy_from_slice(&rgb[(y * w + x) * 3..(y * w + x) * 3 + 3]);
        }
    }
    out
}

fn crop_rgb(rgb: &[u8], w: u32, x: u32, y: u32, cw: u32, ch: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(cw as usize * ch as usize * 3);
    for row in y..y + ch {
        let start = (row as usize * w as usize + x as usize) * 3;
        out.extend_from_slice(&rgb[start..start + cw as usize * 3]);
    }
    out
}

/// Decode a photo, bake its recipe in through the shared engine, and
/// convert it to sRGB. The source is shrunk to `max_edge` first so the
/// linear-light working set stays bounded.
fn load_photo(src: &PrintPhotoSource, max_edge: u32, srgb: &[u8]) -> Result<Loaded, ExportError> {
    let bytes = std::fs::read(&src.path)?;
    let json = if src.recipe_json.trim().is_empty() { NO_RECIPE } else { &src.recipe_json };
    let (recipe, _skipped) =
        fotobase_core::recipe::parse(json).map_err(|e| ExportError::Recipe(e.0))?;
    let (rgb, w, h) = decode_raw(&bytes, &src.path)?;
    let profile = icc::extract_icc(&bytes).or_else(|| icc::extract_from_heif(&bytes));
    let (dw, dh) = fit(w, h, max_edge);
    let rgb = resize(&rgb, w, h, dw, dh);
    let (mut rgb, width, height) = if recipe.ops.is_empty() {
        (rgb, dw, dh)
    } else {
        let out = fotobase_core::apply::apply_recipe(&rgb_to_linear(&rgb, dw, dh), &recipe);
        (to_srgb8(&out.data), out.width, out.height)
    };
    crate::color::transform_rgb(&mut rgb, profile.as_deref(), srgb)?;
    Ok(Loaded { rgb, width, height })
}

fn draw_text(page: &mut PageRaster, opts: &TextOptions, plan: &TextPlan, dpi: u32) {
    let (x0, y0, x1, y1) = px_box(plan.rect, dpi);
    let (w, h) = ((x1 - x0).max(0) as u32, (y1 - y0).max(0) as u32);
    if w == 0 || h == 0 {
        return;
    }
    let size_px = opts.size_pt.clamp(4.0, 72.0) / 72.0 * f64::from(dpi);
    let mut params = Map::new();
    for (k, v) in [
        ("algo", json!(2)),
        ("text", json!(plan.text)),
        ("x", json!(0.5)),
        ("y", json!(TEXT_PAD_IN * f64::from(dpi) / f64::from(h))),
        ("anchor", json!("top")),
        ("size", json!(size_px / f64::from(h))),
        ("font", json!(opts.font)),
        ("color", Value::String(opts.color.clone())),
    ] {
        params.insert(k.to_string(), v);
    }
    let op = Op { op_type: "text".into(), params: Some(params) };
    let band = fotobase_core::text::render_text(&LinearImage::from_fn(w, h, |_, _| [1.0; 3]), &op);
    blit(page, x0, y0, &to_srgb8(&band.data), w, h);
}

fn draw_cell(
    page: &mut PageRaster,
    opts: &PrintOptions,
    cell: &CellPlan,
    photo: &Loaded,
    dpi: u32,
) {
    let (mut rgb, mut w, mut h) =
        (std::borrow::Cow::Borrowed(&photo.rgb[..]), photo.width, photo.height);
    if cell.rotated {
        rgb = std::borrow::Cow::Owned(rotate_cw(&rgb, w, h));
        std::mem::swap(&mut w, &mut h);
    }
    // Re-fit against the real pixel size; the plan used a proxy aspect.
    let fit = fit_rect(f64::from(w), f64::from(h), cell.image_box, opts.fit);

    if let Some(frame) = cell.border {
        fill_rect(page, px_box(frame, dpi), hex_rgb(&opts.border.color, [0, 0, 0]));
    }
    let (x0, y0, x1, y1) = px_box(fit.dst, dpi);
    let (dw, dh) = ((x1 - x0).max(1) as u32, (y1 - y0).max(1) as u32);
    let sx = ((fit.src.x * f64::from(w)).round() as u32).min(w - 1);
    let sy = ((fit.src.y * f64::from(h)).round() as u32).min(h - 1);
    let sw = ((fit.src.w * f64::from(w)).round() as u32).clamp(1, w - sx);
    let sh = ((fit.src.h * f64::from(h)).round() as u32).clamp(1, h - sy);
    let cropped = crop_rgb(&rgb, w, sx, sy, sw, sh);
    let scaled = resize(&cropped, sw, sh, dw, dh);
    blit(page, x0, y0, &scaled, dw, dh);

    if let Some(text) = &cell.text {
        draw_text(page, &opts.text, text, dpi);
    }
}

/// Rasterize every page of the print job at `dpi`, handing each finished
/// page to `sink` (so only one page is resident at a time). A photo that
/// cannot be loaded leaves its cells blank and is reported in the result.
pub fn render_pages(
    opts: &PrintOptions,
    items: &[PrintItem],
    sources: &[PrintPhotoSource],
    dpi: u32,
    sink: &mut dyn FnMut(usize, PageRaster) -> std::io::Result<()>,
) -> Result<Vec<PrintFailure>, PrintError> {
    let srgb = crate::color::srgb_profile_bytes().map_err(ExportError::from)?;
    let by_id: HashMap<i64, &PrintPhotoSource> = sources.iter().map(|s| (s.file_id, s)).collect();
    let mut failures: Vec<PrintFailure> = Vec::new();
    for (index, plan) in plan_pages(opts, items).iter().enumerate() {
        let mut page = PageRaster {
            rgb: vec![255; px(plan.width, dpi) as usize * px(plan.height, dpi) as usize * 3],
            width: px(plan.width, dpi) as u32,
            height: px(plan.height, dpi) as u32,
        };
        // Copies of one photo on a page decode once.
        let mut loaded: HashMap<i64, Option<Loaded>> = HashMap::new();
        for cell in &plan.cells {
            let photo = loaded.entry(cell.file_id).or_insert_with(|| {
                let max_edge = (px(cell.cell.w.max(cell.cell.h), dpi) * 2).max(256) as u32;
                let result = match by_id.get(&cell.file_id) {
                    Some(src) => load_photo(src, max_edge, &srgb).map_err(|e| e.to_string()),
                    None => Err("photo source missing".to_string()),
                };
                result
                    .map_err(|error| failures.push(PrintFailure { file_id: cell.file_id, error }))
                    .ok()
            });
            if let Some(photo) = photo {
                draw_cell(&mut page, opts, cell, photo, dpi);
            }
        }
        sink(index, page)?;
    }
    Ok(failures)
}

/// Render the job and write each page as `<prefix>-NNN.png` (sRGB-tagged,
/// atomic `.tmp` + rename) into `dest`, never overwriting an earlier print.
/// Returns the page files and the photos that failed.
pub fn write_pages(
    opts: &PrintOptions,
    items: &[PrintItem],
    sources: &[PrintPhotoSource],
    dpi: u32,
    dest: &Path,
    progress: &mut dyn FnMut(usize),
) -> Result<(Vec<PathBuf>, Vec<PrintFailure>), PrintError> {
    std::fs::create_dir_all(dest)?;
    let mut prefix = "fotobase-print".to_string();
    for n in 2.. {
        if !dest.join(format!("{prefix}-001.png")).exists() {
            break;
        }
        prefix = format!("fotobase-print-{n}");
    }
    let srgb = crate::color::srgb_profile_bytes().map_err(ExportError::from)?;
    let mut files = Vec::new();
    let failures = render_pages(opts, items, sources, dpi, &mut |i, page| {
        let out = dest.join(format!("{prefix}-{:03}.png", i + 1));
        let tmp = out.with_extension("png.tmp");
        std::fs::write(&tmp, encode_png(&page.rgb, page.width, page.height, &srgb))?;
        if let Err(e) = std::fs::rename(&tmp, &out) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        files.push(out);
        progress(i + 1);
        Ok(())
    })?;
    Ok((files, failures))
}
