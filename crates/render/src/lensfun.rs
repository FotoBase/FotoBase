//! Lensfun FFI bindings (TASK-10-02-02, PLAN §7 F2 item 2).
//!
//! Own, hand-written bindings over the Lensfun 0.3.x C API, isolated here
//! like the LibRaw bindings (`raw.rs`) so the LGPL-3.0 library stays at one
//! crate boundary. Every Lensfun object is handled through an opaque
//! pointer: struct layouts are never named, so we read lens data only
//! through Lensfun's own XML serializer.
//!
//! Two jobs:
//! - [`LensDb::profile`]: EXIF camera + lens → the matching Lensfun entry,
//!   serialized as Lensfun XML into the `lens_correction` recipe params.
//! - [`LensfunBackend`]: renders a `lens_correction` op by loading that XML
//!   into a private database and running `lfModifier` over the linear-light
//!   buffer. The stored profile makes the render independent of the
//!   installed database version (edit-recipe spec §4 reproducibility).

#![allow(
    clippy::undocumented_unsafe_blocks,
    // The workspace denies `unsafe_code` globally; this module is the
    // sanctioned Lensfun FFI boundary (own bindings, TASK-10-02-02).
    unsafe_code
)]

use std::ffi::{c_char, c_float, c_int, c_void, CStr, CString};
use std::ptr;
use std::sync::{Mutex, MutexGuard};

use fotobase_core::apply::OpBackend;
use fotobase_core::buffer::LinearImage;
use fotobase_core::recipe::Op;
use serde_json::{json, Value};
use thiserror::Error;

#[repr(C)]
struct LfDatabase {
    _private: [u8; 0],
}
#[repr(C)]
struct LfCamera {
    _private: [u8; 0],
}
#[repr(C)]
struct LfLens {
    _private: [u8; 0],
}
#[repr(C)]
struct LfMount {
    _private: [u8; 0],
}
#[repr(C)]
struct LfModifier {
    _private: [u8; 0],
}

const LF_NO_ERROR: c_int = 0;
const LF_MODIFY_TCA: c_int = 0x01;
const LF_MODIFY_VIGNETTING: c_int = 0x02;
const LF_MODIFY_DISTORTION: c_int = 0x08;
const LF_MODIFY_SCALE: c_int = 0x20;
const LF_PF_F32: c_int = 3;
const LF_RECTILINEAR: c_int = 1;
/// `LF_CR_3(RED, GREEN, BLUE)`: interleaved RGB.
const LF_CR_RGB: c_int = 4 | (5 << 4) | (6 << 8);
/// Focus distance (m) used when EXIF has none: Lensfun's "infinity".
const DEFAULT_DISTANCE: f64 = 1000.0;

unsafe extern "C" {
    fn lf_db_new() -> *mut LfDatabase;
    fn lf_db_destroy(db: *mut LfDatabase);
    fn lf_db_load(db: *mut LfDatabase) -> c_int;
    fn lf_db_load_data(
        db: *mut LfDatabase,
        errcontext: *const c_char,
        data: *const c_char,
        data_size: usize,
    ) -> c_int;
    fn lf_db_save(
        mounts: *const *const LfMount,
        cameras: *const *const LfCamera,
        lenses: *const *const LfLens,
    ) -> *mut c_char;
    fn lf_db_find_cameras(
        db: *const LfDatabase,
        maker: *const c_char,
        model: *const c_char,
    ) -> *mut *const LfCamera;
    fn lf_db_find_lenses_hd(
        db: *const LfDatabase,
        camera: *const LfCamera,
        maker: *const c_char,
        lens: *const c_char,
        sflags: c_int,
    ) -> *mut *const LfLens;
    fn lf_db_get_lenses(db: *const LfDatabase) -> *const *const LfLens;
    fn lf_free(data: *mut c_void);
    fn lf_modifier_new(
        lens: *const LfLens,
        crop: c_float,
        width: c_int,
        height: c_int,
    ) -> *mut LfModifier;
    fn lf_modifier_destroy(modifier: *mut LfModifier);
    fn lf_modifier_initialize(
        modifier: *mut LfModifier,
        lens: *const LfLens,
        format: c_int,
        focal: c_float,
        aperture: c_float,
        distance: c_float,
        scale: c_float,
        targeom: c_int,
        flags: c_int,
        reverse: c_int,
    ) -> c_int;
    fn lf_modifier_apply_color_modification(
        modifier: *mut LfModifier,
        pixels: *mut c_void,
        x: c_float,
        y: c_float,
        width: c_int,
        height: c_int,
        comp_role: c_int,
        row_stride: c_int,
    ) -> c_int;
    fn lf_modifier_apply_subpixel_geometry_distortion(
        modifier: *mut LfModifier,
        xu: c_float,
        yu: c_float,
        width: c_int,
        height: c_int,
        res: *mut c_float,
    ) -> c_int;
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("{0}")]
pub struct LensError(pub String);

fn fail<T>(msg: impl Into<String>) -> Result<T, LensError> {
    Err(LensError(msg.into()))
}

pub use crate::LensQuery;

// Lensfun 0.3.x shares unsynchronized lens regexes and changes LC_NUMERIC
// while loading, searching and saving databases. Destruction also frees the
// shared regexes, so all of these operations must use the same lock.
static DATABASE_LOCK: Mutex<()> = Mutex::new(());

fn database_lock() -> MutexGuard<'static, ()> {
    DATABASE_LOCK.lock().unwrap_or_else(|poisoned| {
        tracing::error!("Lensfun database lock was poisoned");
        poisoned.into_inner()
    })
}

/// An owned Lensfun database.
pub struct LensDb(*mut LfDatabase);

impl Drop for LensDb {
    fn drop(&mut self) {
        let _guard = database_lock();
        unsafe { lf_db_destroy(self.0) }
    }
}

impl LensDb {
    /// The database installed with the Lensfun library.
    pub fn system() -> Result<Self, LensError> {
        let db = Self(unsafe { lf_db_new() });
        let rc = {
            let _guard = database_lock();
            unsafe { lf_db_load(db.0) }
        };
        if rc != LF_NO_ERROR {
            return fail("the Lensfun database is not installed");
        }
        Ok(db)
    }

    /// A database from Lensfun XML (a stored recipe profile, or a test DB).
    pub fn from_xml(xml: &str) -> Result<Self, LensError> {
        let db = Self(unsafe { lf_db_new() });
        let context = c"fotobase recipe";
        let rc = {
            let _guard = database_lock();
            unsafe { lf_db_load_data(db.0, context.as_ptr(), xml.as_ptr().cast(), xml.len()) }
        };
        if rc != LF_NO_ERROR {
            return fail("lens profile is not valid Lensfun XML");
        }
        Ok(db)
    }

    /// Resolve the `lens_correction` params for one shot. Unknown cameras or
    /// lenses and missing EXIF are errors: a wrong profile would distort the
    /// photo, so nothing is guessed.
    pub fn profile(&self, q: &LensQuery) -> Result<Value, LensError> {
        let _guard = database_lock();
        let Some(lens_model) = q.lens_model.as_deref().filter(|m| !m.trim().is_empty()) else {
            return fail("the photo's EXIF has no lens model");
        };
        let Some(focal) = q.focal.filter(|f| f.is_finite() && *f > 0.0) else {
            return fail("the photo's EXIF has no focal length");
        };
        let maker = cstring(&q.camera_maker)?;
        let model = cstring(&q.camera_model)?;
        let cameras = unsafe { lf_db_find_cameras(self.0, maker.as_ptr(), model.as_ptr()) };
        let camera = unsafe { take_first(cameras) };
        if camera.is_null() {
            return fail(format!(
                "camera \"{} {}\" is not in the Lensfun database",
                q.camera_maker, q.camera_model
            ));
        }
        let camera_xml = unsafe { save(ptr::null(), [camera, ptr::null()].as_ptr(), ptr::null()) }?;
        let crop = xml_text(&camera_xml, "cropfactor")
            .and_then(|c| c.parse::<f64>().ok())
            .filter(|c| *c > 0.0)
            .ok_or_else(|| LensError("Lensfun camera entry has no crop factor".into()))?;

        let lens_maker = q.lens_maker.as_deref().filter(|m| !m.trim().is_empty()).map(cstring);
        let lens_maker = lens_maker.transpose()?;
        let lens_name = cstring(lens_model)?;
        let lenses = unsafe {
            lf_db_find_lenses_hd(
                self.0,
                camera,
                lens_maker.as_ref().map_or(ptr::null(), |m| m.as_ptr()),
                lens_name.as_ptr(),
                0,
            )
        };
        let lens = unsafe { take_first(lenses) };
        if lens.is_null() {
            return fail(format!(
                "lens \"{lens_model}\" is not in the Lensfun database for \"{} {}\"",
                q.camera_maker, q.camera_model
            ));
        }
        let profile = unsafe { save(ptr::null(), ptr::null(), [lens, ptr::null()].as_ptr()) }?;
        let name = xml_text(&profile, "model").unwrap_or_else(|| lens_model.to_string());

        let mut params = json!({
            "profile": profile,
            "lens": name,
            "crop": crop,
            "focal": focal,
            "distortion": true,
            "tca": true,
            // Vignetting depends on the aperture; without it there is no
            // calibration row to pick.
            "vignetting": q.aperture.is_some(),
        });
        if let Some(aperture) = q.aperture.filter(|a| a.is_finite() && *a > 0.0) {
            params["aperture"] = json!(aperture);
        }
        if let Some(distance) = q.distance.filter(|d| d.is_finite() && *d > 0.0) {
            params["distance"] = json!(distance);
        }
        Ok(params)
    }

    fn first_lens(&self) -> *const LfLens {
        let lenses = unsafe { lf_db_get_lenses(self.0) };
        if lenses.is_null() {
            ptr::null()
        } else {
            unsafe { *lenses }
        }
    }
}

fn cstring(s: &str) -> Result<CString, LensError> {
    CString::new(s).map_err(|_| LensError("EXIF text contains a NUL byte".into()))
}

/// First entry of a Lensfun search result; frees the list (not the entries,
/// which the database owns).
unsafe fn take_first<T>(list: *mut *const T) -> *const T {
    if list.is_null() {
        return ptr::null();
    }
    let first = *list;
    lf_free(list.cast());
    first
}

unsafe fn save(
    mounts: *const *const LfMount,
    cameras: *const *const LfCamera,
    lenses: *const *const LfLens,
) -> Result<String, LensError> {
    let xml = lf_db_save(mounts, cameras, lenses);
    if xml.is_null() {
        return fail("Lensfun could not serialize the profile");
    }
    let out = CStr::from_ptr(xml).to_string_lossy().into_owned();
    lf_free(xml.cast());
    Ok(out)
}

/// Text of the first `<tag>…</tag>` (no attributes) in Lensfun's own XML.
fn xml_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = start + xml[start..].find(&format!("</{tag}>"))?;
    Some(
        xml[start..end]
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&")
            .trim()
            .to_string(),
    )
}

fn number(op: &Op, key: &str) -> Option<f64> {
    op.params.as_ref()?.get(key)?.as_f64()
}

fn flag(op: &Op, key: &str) -> bool {
    op.params.as_ref().and_then(|p| p.get(key)).and_then(Value::as_bool).unwrap_or(true)
}

/// Apply one `lens_correction` op with Lensfun: vignetting first (color
/// stage), then distortion + TCA as one resampling pass, auto-scaled so no
/// empty border appears (Lensfun's documented order).
pub fn correct(img: &LinearImage, op: &Op) -> Result<LinearImage, LensError> {
    let profile = op
        .params
        .as_ref()
        .and_then(|p| p.get("profile"))
        .and_then(Value::as_str)
        .ok_or_else(|| LensError("lens_correction has no profile".into()))?;
    let db = LensDb::from_xml(profile)?;
    let lens = db.first_lens();
    if lens.is_null() {
        return fail("lens profile contains no lens");
    }
    let (Some(crop), Some(focal)) = (number(op, "crop"), number(op, "focal")) else {
        return fail("lens_correction needs crop and focal");
    };
    let aperture = number(op, "aperture");
    let distance = number(op, "distance").unwrap_or(DEFAULT_DISTANCE);
    let (w, h) = (img.width as c_int, img.height as c_int);
    if w == 0 || h == 0 {
        return Ok(img.clone());
    }

    let mut flags = 0;
    if flag(op, "distortion") {
        flags |= LF_MODIFY_DISTORTION | LF_MODIFY_SCALE;
    }
    if flag(op, "tca") {
        flags |= LF_MODIFY_TCA;
    }
    if flag(op, "vignetting") && aperture.is_some() {
        flags |= LF_MODIFY_VIGNETTING;
    }

    let modifier = unsafe { lf_modifier_new(lens, crop as c_float, w, h) };
    if modifier.is_null() {
        return fail("Lensfun could not create a modifier");
    }
    let enabled = unsafe {
        lf_modifier_initialize(
            modifier,
            lens,
            LF_PF_F32,
            focal as c_float,
            aperture.unwrap_or(0.0) as c_float,
            distance as c_float,
            // 0 = automatic scale (only used with LF_MODIFY_SCALE).
            0.0,
            LF_RECTILINEAR,
            flags,
            0,
        )
    };

    let mut src = img.clone();
    if enabled & LF_MODIFY_VIGNETTING != 0 {
        unsafe {
            lf_modifier_apply_color_modification(
                modifier,
                src.data.as_mut_ptr().cast(),
                0.0,
                0.0,
                w,
                h,
                LF_CR_RGB,
                w * 3 * std::mem::size_of::<f32>() as c_int,
            );
        }
    }
    let mut out = src.clone();
    if enabled & (LF_MODIFY_DISTORTION | LF_MODIFY_TCA | LF_MODIFY_SCALE) != 0 {
        let mut coords = vec![0.0f32; img.width as usize * 6];
        for y in 0..img.height {
            let ok = unsafe {
                lf_modifier_apply_subpixel_geometry_distortion(
                    modifier,
                    0.0,
                    y as c_float,
                    w,
                    1,
                    coords.as_mut_ptr(),
                )
            };
            if ok == 0 {
                out = src.clone();
                break;
            }
            for x in 0..img.width {
                let at = x as usize * 6;
                let px =
                    [0, 1, 2].map(|c| sample(&src, coords[at + 2 * c], coords[at + 2 * c + 1], c));
                out.set(x, y, px);
            }
        }
    }
    unsafe { lf_modifier_destroy(modifier) };
    // The engine's linear-light buffer is [0, 1] (spec §2); brightening
    // vignetting correction can overshoot it.
    out.data.iter_mut().for_each(|v| *v = v.clamp(0.0, 1.0));
    Ok(out)
}

/// Bilinear sample of one channel at pixel coordinates (integer = pixel
/// center), edges replicated.
fn sample(img: &LinearImage, x: f32, y: f32, c: usize) -> f32 {
    let x = x.clamp(0.0, (img.width - 1) as f32);
    let y = y.clamp(0.0, (img.height - 1) as f32);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(img.width - 1), (y0 + 1).min(img.height - 1));
    let (tx, ty) = (x - x0 as f32, y - y0 as f32);
    let at = |px: u32, py: u32| img.data[(py as usize * img.width as usize + px as usize) * 3 + c];
    let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
    let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
    top + (bottom - top) * ty
}

/// Recipe-engine backend for `lens_correction`. A broken stored profile is
/// logged and leaves the image unchanged; the op was validated when it was
/// resolved from EXIF, so this only fires on hand-edited recipes.
pub struct LensfunBackend;

impl OpBackend for LensfunBackend {
    fn apply(&self, img: &LinearImage, op: &Op) -> Option<LinearImage> {
        (op.op_type == "lens_correction").then(|| {
            correct(img, op).unwrap_or_else(|e| {
                tracing::error!("lens_correction not applied: {e}");
                img.clone()
            })
        })
    }
}
