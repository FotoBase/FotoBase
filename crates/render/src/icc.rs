//! Embedded-ICC extraction and JPEG tagging (TASK-02-001).
//!
//! Container parsing only — the profile data itself is always interpreted by
//! lcms2 (`color::transform_rgb`), never by hand.
//!
//! - JPEG: the ICC sits in `APP2` marker segments (`ICC_PROFILE\0`,
//!   sequence numbers when the data spans several markers).
//! - PNG: the `iCCP` chunk holds a keyword, compression method and a
//!   zlib-compressed profile.
//! - HEIF/AVIF: libheif exposes the container's color profile; handled in
//!   the `heif` module path (see `extract_from_heif`).

/// Extract the embedded ICC profile from a sniffed image file, if any.
/// Returns `None` for untagged files (the pipeline then assumes sRGB).
pub fn extract_icc(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        extract_jpeg_icc(bytes)
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        extract_png_iccp(bytes)
    } else {
        // Other containers (GIF/BMP/TIFF/WebP) are almost never tagged; the
        // sRGB assumption matches what every OS viewer does with them.
        None
    }
}

/// Collect the APP2 `ICC_PROFILE` segments of a JPEG and stitch them back
/// into one profile (segments carry 1-based sequence numbers and a total).
fn extract_jpeg_icc(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut i = 2usize; // skip SOI
    let mut segments: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut total = 0u8;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            break;
        }
        let marker = bytes[i + 1];
        if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) {
            i += 2; // standalone markers
            continue;
        }
        if marker == 0xD9 || marker == 0xDA {
            break; // EOI / start of scan — headers are over
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if len < 2 || i + 2 + len > bytes.len() {
            break;
        }
        let payload = &bytes[i + 4..i + 2 + len];
        if marker == 0xE2 && payload.starts_with(b"ICC_PROFILE\0") && payload.len() >= 14 {
            let seq = payload[12];
            let count = payload[13];
            if count == 0 || seq == 0 || seq > count {
                return None;
            }
            total = count;
            segments.push((seq, payload[14..].to_vec()));
        }
        i += 2 + len;
    }
    if segments.is_empty() || segments.len() as u8 != total {
        return None;
    }
    segments.sort_by_key(|(seq, _)| *seq);
    let mut out = Vec::new();
    for (_, part) in segments {
        out.extend_from_slice(&part);
    }
    (!out.is_empty()).then_some(out)
}

/// Attach an ICC profile to a JPEG bitstream as APP2 marker(s), inserted
/// right after SOI. Splits into 64 KiB-max segments per the ICC embedding
/// spec so large profiles stay marker-length compliant.
pub fn attach_to_jpeg(jpeg: &mut Vec<u8>, icc: &[u8]) {
    if icc.is_empty() || !jpeg.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return;
    }
    const CHUNK: usize = 65_533 - 14 - 2; // marker length budget minus ICC header
    let count = icc.len().div_ceil(CHUNK).max(1);
    let mut markers = Vec::new();
    for (idx, chunk) in icc.chunks(CHUNK).enumerate() {
        let payload_len = 14 + chunk.len();
        let mut m = Vec::with_capacity(payload_len + 4);
        m.extend_from_slice(&[0xFF, 0xE2]);
        m.extend_from_slice(&u16::try_from(payload_len + 2).unwrap_or(u16::MAX).to_be_bytes());
        m.extend_from_slice(b"ICC_PROFILE\0");
        m.push(u8::try_from(idx + 1).unwrap_or(1));
        m.push(u8::try_from(count).unwrap_or(1));
        m.extend_from_slice(chunk);
        markers.push(m);
    }
    // Insert after SOI (bytes 0..2).
    let mut out = Vec::with_capacity(jpeg.len() + markers.iter().map(Vec::len).sum::<usize>());
    out.extend_from_slice(&jpeg[..2]);
    for m in markers {
        out.extend_from_slice(&m);
    }
    out.extend_from_slice(&jpeg[2..]);
    *jpeg = out;
}

/// Parse a PNG's `iCCP` chunk: keyword \0 compression-method zlib-data.
fn extract_png_iccp(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut i = 8usize; // signature
    while i + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[i..i + 4].try_into().ok()?) as usize;
        let chunk_type = &bytes[i + 4..i + 8];
        let data_end = i + 8 + len;
        if data_end + 4 > bytes.len() {
            return None;
        }
        if chunk_type == b"iCCP" {
            let data = &bytes[i + 8..data_end];
            let nul = data.iter().position(|&b| b == 0)?;
            // data[nul+1] = compression method (0 = zlib)
            let zlib = &data[nul + 2..];
            return decompress_zlib(zlib);
        }
        if chunk_type == b"IDAT" {
            // ICC must precede the image data per the PNG spec.
            return None;
        }
        i = data_end + 4; // skip CRC
    }
    None
}

/// Raw zlib inflate without pulling a new dependency: the `image` crate
/// already compiles `fdeflate`/`miniz_oxide` via png, but its public surface
/// does not expose inflate. Rebuild the profile through the PNG decoder
/// instead — encode a 1×1 PNG with the profile? No: use `miniz_oxide`
/// directly, it is already in the dependency tree via `png`.
fn decompress_zlib(zlib: &[u8]) -> Option<Vec<u8>> {
    miniz_oxide::inflate::decompress_to_vec_zlib(zlib).ok().filter(|out| !out.is_empty())
}

/// libheif color-profile read: the primary handle carries either an ICC
/// (`rICC` reduced or `prof` full profile) or an NCLX profile; only the ICC
/// variants are usable here.
#[cfg(all(feature = "heif", not(target_os = "windows")))]
pub fn extract_from_heif(bytes: &[u8]) -> Option<Vec<u8>> {
    use libheif_rs::{
        color_profile_types::{PROF, R_ICC},
        ColorProfile as _,
    };
    let ctx = libheif_rs::HeifContext::read_from_bytes(bytes).ok()?;
    let handle = ctx.primary_image_handle().ok()?;
    let profile = handle.color_profile_raw()?;
    let is_icc = profile.profile_type() == R_ICC || profile.profile_type() == PROF;
    is_icc.then_some(profile.data).filter(|d| !d.is_empty())
}

#[cfg(any(not(feature = "heif"), target_os = "windows"))]
pub fn extract_from_heif(_bytes: &[u8]) -> Option<Vec<u8>> {
    None
}
