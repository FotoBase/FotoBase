//! TASK-02-001: color-managed preview with display ICC profile.
//!
//! Unit tests over the `render` crate's display-transform stage (lcms2):
//! a wide-gamut source rendered through a Display-P3 display profile must
//! differ from the sRGB-fallback path in the expected direction (wider
//! saturation preserved), and the cache keys must be profile-aware.

use std::path::Path;

use fotobase_render::{thumb_profile_tag, transform_rgb, ThumbOptions};
use tempfile::TempDir;

const TOLERANCE: i32 = 6;

/// Adobe RGB (1998) primaries, D65 white, gamma 2.2 — built through lcms2 so
/// the fixture never hand-rolls color math.
fn adobe_rgb_profile() -> Vec<u8> {
    fotobase_render::rgb_profile_bytes(
        [0.3127, 0.3290, 1.0],
        [[0.64, 0.33, 1.0], [0.21, 0.71, 1.0], [0.15, 0.06, 1.0]],
        2.19921875,
    )
    .expect("adobe rgb fixture profile")
}

/// Display-P3 primaries, D65 white, gamma 2.2 — stands in for a wide-gamut
/// display profile.
fn display_p3_profile() -> Vec<u8> {
    fotobase_render::rgb_profile_bytes(
        [0.3127, 0.3290, 1.0],
        [[0.680, 0.320, 1.0], [0.265, 0.690, 1.0], [0.150, 0.060, 1.0]],
        2.19921875,
    )
    .expect("display p3 fixture profile")
}

fn srgb_profile() -> Vec<u8> {
    fotobase_render::srgb_profile_bytes().expect("sRGB profile")
}

#[test]
fn p3_display_transform_preserves_more_saturation_than_srgb_fallback() {
    let src = adobe_rgb_profile();
    let p3 = display_p3_profile();
    let srgb = srgb_profile();

    // Teal-green: outside sRGB, inside Display-P3. The sRGB fallback must
    // gamut-clip (its chromaticity collapses onto the sRGB gamut boundary —
    // lcms2 clips, driving B toward 0), while the P3 path keeps the color's
    // chromaticity because P3 can represent it. "Wider saturation" in the
    // spec means exactly this: the display-transformed pixels track the
    // source chromaticity instead of collapsing to the smaller gamut.
    let px = vec![0u8, 200u8, 80u8];

    let mut via_p3 = px.clone();
    transform_rgb(&mut via_p3, Some(&src), &p3).expect("p3 transform");
    let mut via_srgb = px.clone();
    transform_rgb(&mut via_srgb, Some(&src), &srgb).expect("srgb transform");

    assert_ne!(via_p3, via_srgb, "wide-gamut source must render differently per display gamut");
    // Total per-channel deviation from the source: the P3 path keeps the
    // color's chromaticity (small total shift), the sRGB fallback collapses
    // B onto its gamut boundary (large shift).
    let dev = |o: &[u8]| {
        o.iter().zip(px.iter()).map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs()).sum::<i32>()
    };
    let dev_p3 = dev(&via_p3);
    let dev_srgb = dev(&via_srgb);
    assert!(
        dev_p3 < dev_srgb,
        "P3 path must preserve the out-of-sRGB chromaticity better: p3={via_p3:?} (ΣΔ={dev_p3}) \
         vs sRGB fallback {via_srgb:?} (ΣΔ={dev_srgb})"
    );
}

#[test]
fn srgb_source_to_srgb_display_is_identity() {
    let srgb = srgb_profile();
    let px: Vec<u8> = vec![10, 20, 30, 128, 128, 128, 255, 254, 253];
    let mut out = px.clone();
    transform_rgb(&mut out, None, &srgb).expect("identity transform");
    for (a, b) in px.chunks(3).zip(out.chunks(3)) {
        for ch in 0..3 {
            assert!(
                (i32::from(a[ch]) - i32::from(b[ch])).abs() <= TOLERANCE,
                "sRGB→sRGB must be a near-identity transform: {a:?} became {b:?}"
            );
        }
    }
}

#[test]
fn transform_rejects_corrupt_profile() {
    let mut px = vec![0u8; 3];
    assert!(transform_rgb(&mut px, None, b"not an icc profile").is_err());
}

#[test]
fn profile_tag_is_stable_and_distinct() {
    let p3 = display_p3_profile();
    let srgb = srgb_profile();
    let tag_p3 = thumb_profile_tag(Some(p3.as_slice()));
    let tag_p3_again = thumb_profile_tag(Some(p3.as_slice()));
    let tag_srgb = thumb_profile_tag(Some(srgb.as_slice()));
    assert_eq!(tag_p3, tag_p3_again, "same profile must give the same tag");
    assert_ne!(tag_p3, tag_srgb, "different profiles must give different tags");
    assert!(!tag_p3.is_empty());
}

#[test]
fn cached_previews_are_profile_aware() {
    let tmp = TempDir::new().expect("tmp");
    let cache = tmp.path().join("thumbs");
    let src = fixture_jpeg(tmp.path());
    let p3 = display_p3_profile();
    let srgb = srgb_profile();

    let opts_a = ThumbOptions { display_profile: Some(p3.clone()), ..Default::default() };
    let opts_b = ThumbOptions { display_profile: Some(srgb.clone()), ..Default::default() };

    let a = fotobase_render::thumbnail(&src, &cache, opts_a.clone()).expect("render p3");
    let b = fotobase_render::thumbnail(&src, &cache, opts_b.clone()).expect("render srgb");
    assert_ne!(a.path, b.path, "different display profiles must not share a cache entry");

    let a2 = fotobase_render::thumbnail(&src, &cache, opts_a).expect("render p3 again");
    let b2 = fotobase_render::thumbnail(&src, &cache, opts_b).expect("render srgb again");
    assert!(a2.from_cache, "profile-aware entry must be reused for the same profile");
    assert!(b2.from_cache, "profile-aware entry must be reused for the same profile");
    assert_eq!(a.path, a2.path);
    assert_eq!(b.path, b2.path);

    // The legacy untagged entry (no display profile) must still be produced
    // and stay valid alongside the profile-tagged ones.
    let legacy = fotobase_render::thumbnail(&src, &cache, ThumbOptions::default()).expect("legacy");
    assert!(legacy.path.file_name().unwrap().to_str().unwrap().len() == 32 + 4);
    assert_ne!(legacy.path, a.path);
}

#[test]
fn hidpi_and_profile_tags_coexist() {
    let tmp = TempDir::new().expect("tmp");
    let cache = tmp.path().join("thumbs");
    let src = fixture_jpeg(tmp.path());
    let p3 = display_p3_profile();

    let opts = ThumbOptions {
        cache_tag: Some("@2x".to_string()),
        display_profile: Some(p3),
        ..Default::default()
    };
    let t = fotobase_render::thumbnail(&src, &cache, opts).expect("render");
    let name = t.path.file_name().unwrap().to_str().unwrap().to_string();
    assert!(name.contains("@2x"), "2x tag must survive: {name}");
    assert!(name.contains('-') || name.len() > 36, "profile tag must be present: {name}");
}

/// Deterministic tiny JPEG (same LCG pattern as the pipeline tests).
fn fixture_jpeg(dir: impl AsRef<Path>) -> std::path::PathBuf {
    let path = dir.as_ref().join("color-fixture.jpg");
    if !path.exists() {
        let mut state = 7u32;
        let mut px = Vec::with_capacity(64 * 64 * 3);
        for _ in 0..64 * 64 {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            px.push((state >> 16) as u8);
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            px.push((state >> 16) as u8);
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            px.push((state >> 16) as u8);
        }
        let img = image::RgbImage::from_raw(64, 64, px).expect("fixture buffer");
        image::save_buffer(&path, img.as_raw(), 64, 64, image::ExtendedColorType::Rgb8)
            .expect("write fixture jpeg");
    }
    path
}

#[test]
fn jpeg_icc_roundtrip() {
    use fotobase_render::extract_icc;
    use image::ImageEncoder;
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("tagged.jpg");
    let src = fixture_jpeg(tmp.path());
    let profile = display_p3_profile();
    // Encode a JPEG, attach the profile, re-read it from the file bytes.
    let img = image::open(&src).expect("fixture");
    let mut jpeg: Vec<u8> = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85)
        .write_image(
            img.as_rgb8().unwrap().as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgb8,
        )
        .expect("encode");
    assert!(extract_icc(&jpeg).is_none(), "untagged JPEG must yield no profile");
    fotobase_render::icc::attach_to_jpeg(&mut jpeg, &profile);
    let extracted = extract_icc(&jpeg).expect("tagged JPEG must yield its profile");
    assert_eq!(extracted, profile, "APP2 roundtrip must be byte-exact");
    std::fs::write(&path, &jpeg).expect("write");
    let reread = extract_icc(&std::fs::read(&path).expect("read")).expect("profile survives disk");
    assert_eq!(reread, profile);
}

#[test]
fn png_iccp_extraction() {
    use fotobase_render::extract_icc;
    use image::ImageEncoder;
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("tagged.png");
    let profile = srgb_profile();
    // image's PNG encoder does not write iCCP; build the chunk by hand on a
    // 1x1 PNG and verify the parser reads it back.
    let img = image::RgbImage::from_raw(1, 1, vec![10, 20, 30]).expect("px");
    let mut png: Vec<u8> = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(img.as_raw(), 1, 1, image::ExtendedColorType::Rgb8)
        .expect("encode");
    assert!(extract_icc(&png).is_none(), "untagged PNG must yield no profile");
    let tagged = insert_iccp(png, &profile);
    std::fs::write(&path, &tagged).expect("write");
    let extracted = extract_icc(&std::fs::read(&path).expect("read")).expect("iCCP must be found");
    assert_eq!(extracted, profile, "iCCP roundtrip must be byte-exact (after inflate)");

    // Minimal iCCP chunk injector: keyword + method 0 + zlib(profile) + CRC.
    fn insert_iccp(mut png: Vec<u8>, profile: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"ICC\0");
        data.push(0); // compression method: zlib
        data.extend_from_slice(&miniz_oxide::deflate::compress_to_vec_zlib(profile, 8));
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
        chunk.extend_from_slice(b"iCCP");
        chunk.extend_from_slice(&data);
        chunk.extend_from_slice(&crc32(&chunk[4..4 + 4 + data.len()]).to_be_bytes());
        // Insert right after the IHDR chunk ends (signature 8 + IHDR chunk).
        let ihdr_len = u32::from_be_bytes(png[8..12].try_into().unwrap()) as usize;
        let insert_at = 8 + 12 + ihdr_len; // sig + len+type+data+crc
        png.splice(insert_at..insert_at, chunk);
        png
    }
    fn crc32(data: &[u8]) -> u32 {
        let mut table = [0u32; 256];
        for (i, e) in table.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *e = c;
        }
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc = table[((crc ^ u32::from(b)) & 0xFF) as usize] ^ (crc >> 8);
        }
        crc ^ 0xFFFF_FFFF
    }
}
