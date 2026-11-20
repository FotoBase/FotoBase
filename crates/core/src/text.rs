//! Text overlay op, `algo: 2` (TASK-08-003, docs/edit-recipe-spec-v1.md §3
//! `text`): real glyph outlines (fontdue, pure Rust, no hinting) with an
//! optional outline stroke and overall transparency. One rasterization path
//! serves preview and export — everything is a fraction of the image
//! height/width, only the pixel scale differs (PLAN §3 rule 5).
//!
//! Fonts: the generic families `sans`, `serif` and `mono` are bundled
//! (DejaVu), so they render identically on every OS. Any other family name
//! goes through an optional system-font resolver installed by the app; when
//! it cannot supply the font the bundled sans is used and the substitution
//! is reported by [`font_substitutions`] (never written back to the recipe).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use fontdue::{Font, FontSettings};
use serde_json::{Map, Value};

use crate::buffer::LinearImage;
use crate::recipe::{Op, Recipe};

static SANS: &[u8] = include_bytes!("../assets/fonts/DejaVuSans.ttf");
static SERIF: &[u8] = include_bytes!("../assets/fonts/DejaVuSerif.ttf");
static MONO: &[u8] = include_bytes!("../assets/fonts/DejaVuSansMono.ttf");

/// Family used when the requested font is unavailable.
pub const DEFAULT_FONT: &str = "sans";

/// Layers larger than this many pixels are skipped (pathological text
/// length × size); keeps the working set bounded.
const MAX_LAYER_PIXELS: usize = 100_000_000;

/// A font file handed over by the system-font resolver.
pub struct SystemFont {
    pub data: Vec<u8>,
    /// Face index inside a `.ttc` collection (0 for plain files).
    pub collection_index: u32,
}

pub type FontResolver = Box<dyn Fn(&str) -> Option<SystemFont> + Send + Sync>;

static RESOLVER: OnceLock<FontResolver> = OnceLock::new();

/// Install the system-font lookup (once, at app start). Returns `false` if
/// a resolver was already installed.
pub fn set_system_font_resolver(resolver: FontResolver) -> bool {
    let installed = RESOLVER.set(resolver).is_ok();
    if installed {
        cache().lock().expect("font cache").clear();
    }
    installed
}

/// A text op whose requested font was not available.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontSubstitution {
    pub op_index: usize,
    pub requested: String,
    pub used: String,
}

/// Every `algo: 2` text op in `recipe` whose font had to be replaced by the
/// bundled default.
pub fn font_substitutions(recipe: &Recipe) -> Vec<FontSubstitution> {
    recipe
        .ops
        .iter()
        .enumerate()
        .filter(|(_, op)| is_modern_text(op))
        .filter_map(|(op_index, op)| {
            let requested = font_name(op);
            let resolved = resolve_font(&requested);
            resolved.substituted.then(|| FontSubstitution {
                op_index,
                requested,
                used: resolved.used.clone(),
            })
        })
        .collect()
}

/// Advance width of one line of `text` at `px`, kerning included.
fn line_width(font: &Font, text: &str, px: f32) -> f32 {
    let mut pen = 0.0f32;
    let mut prev: Option<char> = None;
    for ch in text.chars().filter(|c| !c.is_control()) {
        if let Some(p) = prev {
            pen += font.horizontal_kern(p, ch, px).unwrap_or(0.0);
        }
        pen += font.metrics(ch, px).advance_width;
        prev = Some(ch);
    }
    pen
}

/// Width in pixels of the widest line of `text` (lines split at `\n`) set
/// in `font_name` at `size_px`. Unknown fonts measure as the bundled sans,
/// exactly like [`render_text`].
pub fn text_width_px(font_name: &str, text: &str, size_px: f32) -> f32 {
    let font = resolve_font(font_name).font.clone();
    text.split('\n').map(|l| line_width(&font, l, size_px)).fold(0.0, f32::max)
}

/// Height in pixels of `lines` lines of `font_name` at `size_px`, as laid
/// out by [`render_text`].
pub fn text_height_px(font_name: &str, lines: usize, size_px: f32) -> f32 {
    let font = resolve_font(font_name).font.clone();
    let (ascent, descent, line_h) = match font.horizontal_line_metrics(size_px) {
        Some(m) => (m.ascent, m.descent, m.new_line_size),
        None => (size_px * 0.8, -size_px * 0.2, size_px * 1.2),
    };
    (ascent - descent) + lines.saturating_sub(1) as f32 * line_h
}

/// Greedy word wrap: break `text` at spaces so no line is wider than
/// `max_width_px` when set in `font_name` at `size_px`. Existing `\n` are
/// kept; a single word wider than the limit stays on its own line. The
/// result feeds the `text` op (which breaks lines at `\n`).
pub fn wrap_text(font_name: &str, text: &str, size_px: f32, max_width_px: f32) -> String {
    let font = resolve_font(font_name).font.clone();
    let mut lines: Vec<String> = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            let candidate =
                if current.is_empty() { word.to_string() } else { format!("{current} {word}") };
            if current.is_empty() || line_width(&font, &candidate, size_px) <= max_width_px {
                current = candidate;
            } else {
                lines.push(std::mem::take(&mut current));
                current = word.to_string();
            }
        }
        lines.push(current);
    }
    lines.join("\n")
}

/// True for `text` ops that use the glyph-outline renderer.
pub fn is_modern_text(op: &Op) -> bool {
    op.op_type == "text"
        && params(op).and_then(|p| p.get("algo")).and_then(Value::as_f64) == Some(2.0)
}

fn params(op: &Op) -> Option<&Map<String, Value>> {
    op.params.as_ref()
}

fn font_name(op: &Op) -> String {
    match params(op).and_then(|p| p.get("font")).and_then(Value::as_str) {
        Some(f) if !f.trim().is_empty() => f.trim().to_string(),
        _ => DEFAULT_FONT.to_string(),
    }
}

struct Resolved {
    font: Arc<Font>,
    used: String,
    substituted: bool,
}

type FontCache = Mutex<HashMap<String, Arc<Resolved>>>;

fn cache() -> &'static FontCache {
    static CACHE: OnceLock<FontCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn load(data: &[u8], collection_index: u32) -> Option<Font> {
    Font::from_bytes(data, FontSettings { collection_index, ..FontSettings::default() }).ok()
}

fn bundled(name: &str) -> Option<(&'static str, &'static [u8])> {
    match name.to_ascii_lowercase().as_str() {
        "sans" | "sans-serif" | "default" => Some(("sans", SANS)),
        "serif" => Some(("serif", SERIF)),
        "mono" | "monospace" => Some(("mono", MONO)),
        _ => None,
    }
}

fn resolve_font(requested: &str) -> Arc<Resolved> {
    let key = requested.to_ascii_lowercase();
    if let Some(hit) = cache().lock().expect("font cache").get(&key) {
        return hit.clone();
    }
    let resolved = if let Some((used, data)) = bundled(requested) {
        let font = load(data, 0).expect("bundled font parses");
        Resolved { font: Arc::new(font), used: used.to_string(), substituted: false }
    } else if let Some(font) =
        RESOLVER.get().and_then(|r| r(requested)).and_then(|f| load(&f.data, f.collection_index))
    {
        Resolved { font: Arc::new(font), used: requested.to_string(), substituted: false }
    } else {
        let font = load(SANS, 0).expect("bundled font parses");
        Resolved { font: Arc::new(font), used: DEFAULT_FONT.to_string(), substituted: true }
    };
    let resolved = Arc::new(resolved);
    cache().lock().expect("font cache").insert(key, resolved.clone());
    resolved
}

fn num(p: &Map<String, Value>, key: &str, default: f64) -> f64 {
    p.get(key).and_then(Value::as_f64).unwrap_or(default)
}

fn color(value: Option<&Value>, default: [f32; 3]) -> [f32; 3] {
    match value {
        Some(Value::String(hex)) => crate::apply::parse_hex_color(hex).unwrap_or(default),
        Some(Value::Array(c)) if c.len() == 3 => {
            let at = |i: usize, d: f32| c[i].as_f64().map_or(d, |v| v as f32);
            [at(0, default[0]), at(1, default[1]), at(2, default[2])]
        }
        _ => default,
    }
}

/// `(fx, fy)`: which point of the text block sits at `(x, y)`.
pub fn anchor_fractions(anchor: &str) -> Option<(f32, f32)> {
    Some(match anchor {
        "top-left" => (0.0, 0.0),
        "top" => (0.5, 0.0),
        "top-right" => (1.0, 0.0),
        "left" => (0.0, 0.5),
        "center" => (0.5, 0.5),
        "right" => (1.0, 0.5),
        "bottom-left" => (0.0, 1.0),
        "bottom" => (0.5, 1.0),
        "bottom-right" => (1.0, 1.0),
        _ => return None,
    })
}

struct Glyph {
    bitmap: Vec<u8>,
    w: usize,
    h: usize,
    /// Top-left of the bitmap relative to the block's top-left, in
    /// supersampled pixels.
    x: i32,
    y: i32,
}

struct Block {
    glyphs: Vec<Glyph>,
    /// Block size in supersampled pixels.
    width: f32,
    height: f32,
}

/// Supersampling factor of the glyph grid: glyphs are placed on a grid
/// `SUPERSAMPLE`× finer than the image, so the sub-pixel position of the
/// block (and therefore the look of the text) matches between preview and
/// export resolution to within 1/`SUPERSAMPLE` pixel.
const SUPERSAMPLE: usize = 4;

/// Above this many supersampled pixels the text is laid out without
/// supersampling (very long / very large text).
const MAX_SUPERSAMPLED_PIXELS: usize = 32_000_000;

fn layout(font: &Font, text: &str, px: f32) -> Block {
    let (ascent, descent, line_h) = match font.horizontal_line_metrics(px) {
        Some(m) => (m.ascent, m.descent, m.new_line_size),
        None => (px * 0.8, -px * 0.2, px * 1.2),
    };
    let mut glyphs = Vec::new();
    let mut max_w = 0.0f32;
    let mut lines = 0usize;
    for (row, line) in text.split('\n').enumerate() {
        lines = row + 1;
        let baseline = ascent + row as f32 * line_h;
        let mut pen = 0.0f32;
        let mut prev: Option<char> = None;
        for ch in line.chars().filter(|c| !c.is_control()) {
            if let Some(p) = prev {
                pen += font.horizontal_kern(p, ch, px).unwrap_or(0.0);
            }
            let (m, bitmap) = font.rasterize(ch, px);
            if m.width > 0 && m.height > 0 {
                glyphs.push(Glyph {
                    bitmap,
                    w: m.width,
                    h: m.height,
                    x: (pen + m.xmin as f32).round() as i32,
                    y: (baseline - (m.ymin + m.height as i32) as f32).round() as i32,
                });
            }
            pen += m.advance_width;
            prev = Some(ch);
        }
        max_w = max_w.max(pen);
    }
    let height = (ascent - descent) + lines.saturating_sub(1) as f32 * line_h;
    Block { glyphs, width: max_w, height }
}

/// Coverage layer in image pixels: per pixel, fill coverage and outline
/// coverage (the outline stroke is the fill grown by the outline width, so
/// it also covers the glyph interior) in `0..=1`; outline is all zero
/// without an outline.
struct Layer {
    w: usize,
    h: usize,
    fill: Vec<f32>,
    outline: Vec<f32>,
}

impl Layer {
    fn at(&self, i: i64, j: i64) -> (f32, f32) {
        if i < 0 || j < 0 || i >= self.w as i64 || j >= self.h as i64 {
            return (0.0, 0.0);
        }
        let k = j as usize * self.w + i as usize;
        (self.fill[k], self.outline[k])
    }

    /// Bilinear sample at continuous layer coordinates (pixel centers at +0.5).
    fn sample(&self, u: f32, v: f32) -> (f32, f32) {
        let (u, v) = (u - 0.5, v - 0.5);
        let (i, j) = (u.floor(), v.floor());
        let (fx, fy) = (u - i, v - j);
        let (i, j) = (i as i64, j as i64);
        let mut acc = (0.0f32, 0.0f32);
        for (di, dj, wgt) in [
            (0, 0, (1.0 - fx) * (1.0 - fy)),
            (1, 0, fx * (1.0 - fy)),
            (0, 1, (1.0 - fx) * fy),
            (1, 1, fx * fy),
        ] {
            if wgt > 0.0 {
                let (a, b) = self.at(i + di, j + dj);
                acc.0 += a * wgt;
                acc.1 += b * wgt;
            }
        }
        acc
    }
}

/// Rasterize `block` (laid out at `s`× the image scale) into a `w`×`h`
/// layer. `(origin_x, origin_y)` is the block's top-left inside the layer,
/// in image pixels.
fn build_layer(
    block: &Block,
    s: usize,
    (w, h): (usize, usize),
    (origin_x, origin_y): (f32, f32),
    outline_px: f32,
) -> Layer {
    let (gw, gh) = (w * s, h * s);
    let shift_x = (origin_x * s as f32).round() as i32;
    let shift_y = (origin_y * s as f32).round() as i32;
    let mut grid = vec![0u8; gw * gh];
    for g in &block.glyphs {
        for gy in 0..g.h {
            let y = g.y + gy as i32 + shift_y;
            if y < 0 || y >= gh as i32 {
                continue;
            }
            for gx in 0..g.w {
                let x = g.x + gx as i32 + shift_x;
                if x < 0 || x >= gw as i32 {
                    continue;
                }
                let cell = &mut grid[y as usize * gw + x as usize];
                *cell = (*cell).max(g.bitmap[gy * g.w + gx]);
            }
        }
    }
    // Box-filter down to image pixels. A pixel is exactly 1.0 only when
    // every sub-sample is fully covered.
    let norm = 255 * (s * s) as u32;
    let mut fill = vec![0.0f32; w * h];
    for j in 0..h {
        for i in 0..w {
            let mut sum = 0u32;
            for b in 0..s {
                let row = (j * s + b) * gw + i * s;
                sum += grid[row..row + s].iter().map(|&v| u32::from(v)).sum::<u32>();
            }
            fill[j * w + i] = sum as f32 / norm as f32;
        }
    }
    let outline = if outline_px > 0.0 { dilate(&fill, w, h, outline_px) } else { vec![0.0; w * h] };
    Layer { w, h, fill, outline }
}

/// Grow the coverage `fill` by `radius` pixels with an anti-aliased edge:
/// `max over q of fill[q] * clamp(radius + 0.5 - |p - q|, 0, 1)`. Only
/// boundary pixels (touching a not-fully-covered neighbor) can be the
/// nearest source for a pixel outside the shape, so only they are visited.
fn dilate(fill: &[f32], w: usize, h: usize, radius: f32) -> Vec<f32> {
    let reach = radius + 0.5;
    let r = reach.ceil() as i64;
    let mut out = fill.to_vec();
    let covered = |x: i64, y: i64| -> f32 {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            0.0
        } else {
            fill[y as usize * w + x as usize]
        }
    };
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let c = covered(x, y);
            if c <= 0.0 {
                continue;
            }
            let boundary = c < 1.0
                || [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .any(|&(dx, dy)| covered(x + dx, y + dy) < 1.0);
            if !boundary {
                continue;
            }
            for dy in -r..=r {
                let ty = y + dy;
                if ty < 0 || ty >= h as i64 {
                    continue;
                }
                for dx in -r..=r {
                    let tx = x + dx;
                    if tx < 0 || tx >= w as i64 {
                        continue;
                    }
                    let d = ((dx * dx + dy * dy) as f32).sqrt();
                    let v = c * (reach - d).clamp(0.0, 1.0);
                    let cell = &mut out[ty as usize * w + tx as usize];
                    if v > *cell {
                        *cell = v;
                    }
                }
            }
        }
    }
    out
}

/// Alpha-composite one pixel: outline color under fill color, the combined
/// layer scaled by `transparency`.
fn blend(bg: [f32; 3], fc: f32, oc: f32, fill: [f32; 3], line: [f32; 3], t: f32) -> [f32; 3] {
    let alpha = fc + (1.0 - fc) * oc;
    let mut out = bg;
    for c in 0..3 {
        let premult = fc * fill[c] + (1.0 - fc) * oc * line[c];
        out[c] = (bg[c] * (1.0 - t * alpha) + t * premult).clamp(0.0, 1.0);
    }
    out
}

/// Render an `algo: 2` text op.
pub fn render_text(img: &LinearImage, op: &Op) -> LinearImage {
    let mut out = img.clone();
    let Some(p) = params(op) else { return out };
    let Some(text) = p.get("text").and_then(Value::as_str).filter(|t| !t.is_empty()) else {
        return out;
    };
    let size_px = (num(p, "size", 0.05) * f64::from(img.height)) as f32;
    if size_px < 1.0 || img.width == 0 || img.height == 0 {
        return out;
    }
    let font = resolve_font(&font_name(op)).font.clone();
    let fill = color(p.get("color"), [1.0, 1.0, 1.0]);
    let transparency = num(p, "transparency", 1.0).clamp(0.0, 1.0) as f32;
    let (line, outline_px) = match p.get("outline").and_then(Value::as_object) {
        Some(o) => (
            color(o.get("color"), [0.0, 0.0, 0.0]),
            (num(o, "width", 0.0) * f64::from(img.height)).max(0.0) as f32,
        ),
        None => ([0.0; 3], 0.0),
    };
    let (fx, fy) =
        p.get("anchor").and_then(Value::as_str).and_then(anchor_fractions).unwrap_or((0.0, 0.0));
    let rotation = (num(p, "rotation", 0.0) as f32).to_radians();

    let pad = outline_px.ceil() as usize + 2;
    let mut s = SUPERSAMPLE;
    let mut block = layout(&font, text, size_px * s as f32);
    let area = |b: &Block, s: usize| {
        let (w, h) = (
            (b.width / s as f32).ceil() as usize + 2 * pad + 1,
            (b.height / s as f32).ceil() as usize + 2 * pad + 1,
        );
        (w, h, (w * s).saturating_mul(h * s))
    };
    if area(&block, s).2 > MAX_SUPERSAMPLED_PIXELS {
        s = 1;
        block = layout(&font, text, size_px);
    }
    let (lw, lh, grid_px) = area(&block, s);
    if block.glyphs.is_empty() || grid_px > MAX_LAYER_PIXELS {
        return out;
    }
    let (bw, bh) = (block.width / s as f32, block.height / s as f32);

    let px = (num(p, "x", 0.0) * f64::from(img.width)) as f32;
    let py = (num(p, "y", 0.0) * f64::from(img.height)) as f32;
    // Block top-left in image pixels, before rotation.
    let (bx, by) = (px - fx * bw, py - fy * bh);

    let mut put = |x: i64, y: i64, (fc, oc): (f32, f32)| {
        if (fc <= 0.0 && oc <= 0.0)
            || x < 0
            || y < 0
            || x >= i64::from(img.width)
            || y >= i64::from(img.height)
        {
            return;
        }
        let (x, y) = (x as u32, y as u32);
        out.set(x, y, blend(img.get(x, y), fc, oc, fill, line, transparency));
    };

    if rotation == 0.0 {
        // Layer pixel (i, j) is image pixel (ox + i, oy + j); the block's
        // sub-pixel offset lives inside the layer.
        let (ox, oy) = (bx.floor() as i64 - pad as i64, by.floor() as i64 - pad as i64);
        let origin =
            (pad as f32 + bx.fract().rem_euclid(1.0), pad as f32 + by.fract().rem_euclid(1.0));
        let layer = build_layer(&block, s, (lw, lh), origin, outline_px);
        for j in 0..lh as i64 {
            for i in 0..lw as i64 {
                put(ox + i, oy + j, layer.at(i, j));
            }
        }
    } else {
        let layer = build_layer(&block, s, (lw, lh), (pad as f32, pad as f32), outline_px);
        // The anchor point of the block, in layer pixels.
        let (ax, ay) = (pad as f32 + fx * bw, pad as f32 + fy * bh);
        let (sin, cos) = rotation.sin_cos();
        // Layer corners → image bounding box.
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (cx, cy) in [(0.0, 0.0), (lw as f32, 0.0), (0.0, lh as f32), (lw as f32, lh as f32)] {
            let (u, v) = (cx - ax, cy - ay);
            let (ix, iy) = (px + u * cos - v * sin, py + u * sin + v * cos);
            x0 = x0.min(ix);
            y0 = y0.min(iy);
            x1 = x1.max(ix);
            y1 = y1.max(iy);
        }
        let (x0, y0) = ((x0.floor() as i64).max(0), (y0.floor() as i64).max(0));
        let (x1, y1) = (
            (x1.ceil() as i64).min(i64::from(img.width)),
            (y1.ceil() as i64).min(i64::from(img.height)),
        );
        for y in y0..y1 {
            for x in x0..x1 {
                let (dx, dy) = (x as f32 + 0.5 - px, y as f32 + 0.5 - py);
                let (u, v) = (dx * cos + dy * sin, -dx * sin + dy * cos);
                put(x, y, layer.sample(u + ax, v + ay));
            }
        }
    }
    out
}
