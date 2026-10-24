//! Display-transform stage (TASK-02-001, PLAN §6 P1.5).
//!
//! All color math runs through lcms2 (LittleCMS) — the task spec forbids
//! hand-rolled transforms. The stage takes the decoded RGB8 pixels plus:
//!
//! - the *source* profile: the ICC embedded in the file (`icc::extract`),
//!   or `None` meaning "assume sRGB",
//! - the *destination* profile: the display's ICC profile on macOS/Windows
//!   (acquired in the app layer), or the bundled sRGB profile on Linux.
//!
//! The output is written back as packed RGB8 in the destination space and is
//! tagged with that profile by the pipeline (`icc::attach_to_jpeg`), so the
//! WebView never applies a second transform (no double-transform, Linux
//! sRGB fallback included).

use lcms2::{Intent, PixelFormat, Profile, ToneCurve, Transform};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Errors raised by the color stage. Profile bytes come from disk or the OS,
/// so a corrupt profile is an expected runtime condition, not a panic.
#[derive(Debug, Error)]
pub enum ColorError {
    #[error("invalid ICC profile data ({len} bytes)")]
    InvalidProfile { len: usize },
    #[error("lcms2 transform failed")]
    Transform,
}

impl From<lcms2::Error> for ColorError {
    fn from(_: lcms2::Error) -> Self {
        Self::Transform
    }
}

/// Build a synthetic RGB display profile (white point `xyY`, primaries
/// `[R, G, B]` as `xyY` triples, gamma) through lcms2. Used by the app layer
/// for the Linux sRGB fallback and by the tests for Adobe-RGB/Display-P3
/// fixtures — never a hand-rolled transform.
pub fn rgb_profile_bytes(
    white_point: [f64; 3],
    primaries: [[f64; 3]; 3],
    gamma: f64,
) -> Result<Vec<u8>, ColorError> {
    let white = lcms2::CIExyY { x: white_point[0], y: white_point[1], Y: white_point[2] };
    let prim = lcms2::CIExyYTRIPLE {
        Red: to_xyy(primaries[0]),
        Green: to_xyy(primaries[1]),
        Blue: to_xyy(primaries[2]),
    };
    let curves = [ToneCurve::new(gamma), ToneCurve::new(gamma), ToneCurve::new(gamma)];
    let profile = Profile::new_rgb(&white, &prim, &[&curves[0], &curves[1], &curves[2]])
        .map_err(|_| ColorError::Transform)?;
    profile.icc().map_err(|_| ColorError::Transform)
}

fn to_xyy(t: [f64; 3]) -> lcms2::CIExyY {
    lcms2::CIExyY { x: t[0], y: t[1], Y: t[2] }
}

/// Serialize the standard lcms2 sRGB profile (the Linux fallback destination).
pub fn srgb_profile_bytes() -> Result<Vec<u8>, ColorError> {
    Profile::new_srgb().icc().map_err(|_| ColorError::Transform)
}

/// Short stable fingerprint of a profile, used as the cache-file tag so
/// thumbnails rendered for different display profiles never share a cache
/// entry (first 8 bytes of SHA-256 of the profile data, lowercase hex).
pub fn profile_fingerprint(profile: &[u8]) -> String {
    let d = Sha256::digest(profile);
    d[..8].iter().fold(String::new(), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

/// Cache-file tag for a display profile: `-p<fingerprint>`.
/// Empty for `None` — the legacy untagged cache names stay valid.
pub fn thumb_profile_tag(display_profile: Option<&[u8]>) -> String {
    match display_profile {
        None => String::new(),
        Some(p) => format!("-p{}", profile_fingerprint(p)),
    }
}

/// Transform packed RGB8 pixels from `src_profile` into `dst_profile`.
///
/// `src_profile = None` means "untagged source, assume sRGB". Both profiles
/// are parsed with lcms2; the transform runs with perceptual intent in place.
pub fn transform_rgb(
    rgb: &mut [u8],
    src_profile: Option<&[u8]>,
    dst_profile: &[u8],
) -> Result<(), ColorError> {
    if !rgb.len().is_multiple_of(3) {
        return Err(ColorError::Transform);
    }
    let src = match src_profile {
        Some(bytes) => {
            Profile::new_icc(bytes).map_err(|_| ColorError::InvalidProfile { len: bytes.len() })?
        }
        None => Profile::new_srgb(),
    };
    let dst = Profile::new_icc(dst_profile)
        .map_err(|_| ColorError::InvalidProfile { len: dst_profile.len() })?;
    if rgb.is_empty() {
        return Ok(());
    }
    // transform_in_place wants a slice of the pixel type; repack u8 triples
    // into [u8; 3] (a plain copy, not color math).
    let mut px: Vec<[u8; 3]> = rgb.as_chunks::<3>().0.to_vec();
    let t = Transform::new(
        &src,
        PixelFormat::RGB_8,
        &dst,
        PixelFormat::RGB_8,
        Intent::RelativeColorimetric,
    )?;
    t.transform_in_place(&mut px);
    for (out, p) in rgb.as_chunks_mut::<3>().0.iter_mut().zip(px) {
        out.copy_from_slice(&p);
    }
    Ok(())
}
