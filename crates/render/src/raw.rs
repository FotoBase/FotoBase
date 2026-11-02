//! LibRaw FFI bindings (TASK-04-013).
//!
//! Per the task spec these are *own* bindings: a thin, hand-written FFI over
//! the LibRaw C API (`libraw_c_api.cpp` exports), isolated in this module so
//! the LGPL/CDDL library stays at a single crate boundary. Only the handful
//! of entry points the thumbnail/preview pipeline needs are bound; the huge
//! `libraw_data_t` struct is always handled through an opaque pointer — we
//! never construct or name its layout, so minor LibRaw version drift cannot
//! corrupt field offsets.
//!
//! Decode flow: `open_buffer` → `unpack` → `dcraw_process` →
//! `dcraw_make_mem_image`. Defaults are used for all processing parameters
//! (8-bit sRGB output, automatic flip), which is what the shared
//! thumbnail/preview pipeline expects; no special cases.

#![allow(
    clippy::undocumented_unsafe_blocks,
    // The workspace denies `unsafe_code` globally; this module is the single
    // sanctioned FFI boundary (own LibRaw bindings, TASK-04-013).
    unsafe_code
)]

use std::ffi::{c_char, c_int, c_uint, CStr};
use std::fmt;

/// `LIBRAW_OPTIONS_NONE` (libraw_const.h).
const LIBRAW_OPTIONS_NONE: c_uint = 0;

/// Opaque handle — really a `libraw_data_t*`.
#[repr(C)]
struct LibRawData {
    _private: [u8; 0],
}

/// `libraw_processed_image_t` (libraw_types.h). We only need the header
/// fields; `data` is the flexible-array member, read as a slice of
/// `data_size` bytes from the offset the struct layout guarantees.
#[repr(C)]
struct ProcessedImage {
    /// `LIBRAW_IMAGE_JPEG` (1) or `LIBRAW_IMAGE_BITMAP` (2).
    kind: c_int,
    height: u16,
    width: u16,
    colors: u16,
    bits: u16,
    data_size: u32,
    _data_start: u8,
}

const LIBRAW_IMAGE_JPEG: c_int = 1;
const LIBRAW_IMAGE_BITMAP: c_int = 2;

unsafe extern "C" {
    fn libraw_init(flags: c_uint) -> *mut LibRawData;
    fn libraw_close(lr: *mut LibRawData);
    fn libraw_open_buffer(lr: *mut LibRawData, buffer: *const c_char, size: usize) -> c_int;
    fn libraw_unpack(lr: *mut LibRawData) -> c_int;
    fn libraw_dcraw_process(lr: *mut LibRawData) -> c_int;
    fn libraw_dcraw_make_mem_image(lr: *mut LibRawData, errc: *mut c_int) -> *mut ProcessedImage;
    fn libraw_dcraw_clear_mem(img: *mut ProcessedImage);
    fn libraw_strerror(err: c_int) -> *const c_char;
}

/// Decode a RAW container into interleaved 8-bit RGB pixels plus dimensions.
///
/// Any LibRaw failure comes back as an `Err(String)` — never a panic — which
/// the caller turns into the embedded-JPEG fallback or a degraded state.
pub fn decode_raw(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    let result = std::panic::catch_unwind(|| unsafe { decode_raw_inner(bytes) });
    match result {
        Ok(inner) => inner,
        Err(_) => Err("libraw panicked on malformed input".to_string()),
    }
}

unsafe fn decode_raw_inner(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    let lr = libraw_init(LIBRAW_OPTIONS_NONE);
    if lr.is_null() {
        return Err("libraw_init failed".to_string());
    }
    // The handle must be released on every path, whatever the pipeline does.
    let out = if libraw_open_buffer(lr, bytes.as_ptr().cast::<c_char>(), bytes.len()) == 0 {
        process(lr)
    } else {
        Err("libraw open failed".to_string())
    };
    libraw_close(lr);
    out
}

unsafe fn process(lr: *mut LibRawData) -> Result<(Vec<u8>, u32, u32), String> {
    let rc = libraw_unpack(lr);
    if rc != 0 {
        return Err(format!("libraw unpack failed: {}", libraw_error(rc)));
    }
    let rc = libraw_dcraw_process(lr);
    if rc != 0 {
        return Err(format!("libraw process failed: {}", libraw_error(rc)));
    }
    let mut errc: c_int = 0;
    let img = libraw_dcraw_make_mem_image(lr, &mut errc);
    if img.is_null() {
        return Err(format!("libraw make_mem_image failed: {}", libraw_error(errc)));
    }
    let decoded = read_processed(img);
    libraw_dcraw_clear_mem(img);
    decoded
}

/// # Safety
/// `img` must be a valid `libraw_processed_image_t*` returned by
/// `libraw_dcraw_make_mem_image` (still owned by the caller).
unsafe fn read_processed(img: *mut ProcessedImage) -> Result<(Vec<u8>, u32, u32), String> {
    let kind = (*img).kind;
    let (w, h) = (u32::from((*img).width), u32::from((*img).height));
    let colors = u32::from((*img).colors);
    let bits = u32::from((*img).bits);
    let data_size = (*img).data_size;
    if w == 0 || h == 0 {
        return Err("libraw returned empty image".to_string());
    }
    // The flexible `data` member starts right after the header fields,
    // padded to the struct's alignment (4, because of the int member). The
    // header is 4+2*4 = 12 bytes for the enum + ushorts… but the enum makes
    // the size 16 with 4-byte alignment; rely on `data_size` plus a computed
    // offset instead of hardcoding: the address of `_data_start` is the
    // documented first data byte only when padding is zero. LibRaw's struct
    // has no padding between `bits` (u16) and `data` (u8), but the compiler
    // may insert two bytes to align `data_size` before it — hence compute
    // from the field address, not arithmetic.
    let base = img.cast::<u8>();
    let data_off =
        std::ptr::addr_of!((*img)._data_start).byte_offset_from(base as *const u8) as usize;
    let data = std::slice::from_raw_parts(base.add(data_off), data_size as usize);

    match (kind, bits) {
        (LIBRAW_IMAGE_BITMAP, 8) => {
            // dcraw_process output: interleaved `colors` channels, 8-bit.
            // The pipeline is RGB-first; 3 channels is the standard sRGB
            // output, 1 channel is a monochrome body.
            let px = w * h;
            let expected = (px * colors) as usize;
            if data.len() < expected {
                return Err(format!("libraw bitmap too short: {} < {expected}", data.len()));
            }
            match colors {
                3 => Ok((data[..expected].to_vec(), w, h)),
                1 => Ok((data[..expected].iter().flat_map(|&v| [v, v, v]).collect(), w, h)),
                4 => {
                    // CMYK-ish output is not expected from dcraw_process with
                    // default params; convert by dropping K conservatively.
                    let mut rgb = Vec::with_capacity(px as usize * 3);
                    for c in data[..expected].as_chunks::<4>().0 {
                        rgb.push(c[0]);
                        rgb.push(c[1]);
                        rgb.push(c[2]);
                    }
                    Ok((rgb, w, h))
                }
                n => Err(format!("libraw returned {n} channels")),
            }
        }
        (LIBRAW_IMAGE_JPEG, _) => {
            // Some bodies only provide a JPEG "processed" image; hand it to
            // the existing JPEG decoder.
            crate::zune_jpeg_decode(data)
                .or_else(|_| crate::decode_with_image(data))
                .map_err(|e| format!("embedded processed JPEG failed: {e}"))
        }
        (kind, bits) => {
            Err(format!("libraw returned unsupported image format {kind} ({bits}-bit)"))
        }
    }
}

fn libraw_error(code: c_int) -> LibRawError {
    LibRawError(code)
}

/// Human-readable LibRaw error string (`libraw_strerror`), kept alive for the
/// duration of the borrow via a leak-free owned copy.
struct LibRawError(c_int);

impl fmt::Display for LibRawError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cstr = unsafe { libraw_strerror(self.0) };
        if cstr.is_null() {
            return write!(f, "libraw error {}", self.0);
        }
        let s = unsafe { CStr::from_ptr(cstr) }.to_string_lossy();
        write!(f, "{s}")
    }
}
