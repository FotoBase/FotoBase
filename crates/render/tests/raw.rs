//! Integration tests for RAW decoding (TASK-04-013, TDD).
//!
//! Per PLAN §4/§10: RAW files decode via LibRaw in the render crate; when
//! LibRaw cannot handle a body (unsupported camera, corrupt header), the
//! embedded JPEG preview is extracted from the RAW container instead — never
//! a hard failure (§9 robustness: malformed camera media must not crash).
//!
//! Fixtures: real vendor RAW files (Canon/Nikon/Sony/Fujifilm) are several
//! MB each and generated camera data is never committed (repo hygiene), so
//! the vendor leg reads whatever samples exist under
//! `tools/fixture-samples/raw/` and skips with a notice when absent. The
//! LibRaw decode path itself is exercised by a synthetic minimal linear DNG
//! built in-process (a DNG is the only RAW container we can generate
//! losslessly without a camera).

use std::path::{Path, PathBuf};

use fotobase_render::{thumbnail, RenderError, ThumbOptions, RAW_AVAILABLE};
use tempfile::TempDir;

/// A fresh cache dir held for the whole call (avoids borrow/temporaries).
fn temp_cache() -> TempDir {
    TempDir::new().unwrap()
}

/// Directory where real vendor RAW samples live (never committed; see the
/// module docs).
fn raw_samples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/fixture-samples/raw")
}

/// Assert a source of `w × h` produces a thumbnail of the same size (no
/// upscale, and nothing to shrink below the default 512 max edge).
fn assert_thumb_dims(src: &Path, w: u32, h: u32) {
    let tmp = TempDir::new().unwrap();
    let t = thumbnail(src, tmp.path(), ThumbOptions::default()).unwrap();
    assert_eq!((t.width, t.height), (w, h), "wrong thumbnail dims for {}", src.display());
    assert!(!t.from_cache);
    let img = image::ImageReader::open(&t.path).unwrap().decode().unwrap();
    assert_eq!((img.width(), img.height()), (w, h));
}

// ---------------------------------------------------------------------------
// Synthetic minimal linear DNG (validates the LibRaw decode path end-to-end).
// ---------------------------------------------------------------------------

const W: u32 = 64;
const H: u32 = 48;

/// Build a minimal uncompressed linear DNG: one CFA IFD, 16-bit samples,
/// identity color matrix, `AsShotNeutral` = (1,1,1).
fn minimal_dng() -> Vec<u8> {
    const RAW_OFFSET: u32 = 2048;
    let pixel_len = (W * H * 2) as usize;

    let mut b = Vec::with_capacity(RAW_OFFSET as usize + pixel_len);
    // TIFF header: little-endian, magic 42, IFD0 at 8.
    b.extend_from_slice(&[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00]);

    // Tags must be sorted by tag id. Types: 1 BYTE, 2 ASCII, 3 SHORT,
    // 4 LONG, 5 RATIONAL, 10 SRATIONAL. Value-area offsets are absolute
    // (rooted right after the IFD), so the value area is built before the
    // IFD is serialized.
    const ENTRIES: u32 = 20;
    let ifd_size = 2 + ENTRIES * 12 + 4;
    let values_base = 8 + ifd_size;
    let mut values = Vec::new();
    let mut add_value = |v: &[u8]| -> u32 {
        let off = values_base + values.len() as u32;
        values.extend_from_slice(v);
        if values.len() % 2 == 1 {
            values.push(0);
        }
        off
    };

    let mut entries: Vec<(u16, u16, u32, u32)> = Vec::new();
    macro_rules! inl {
        ($t:expr, $ty:expr, $c:expr, $v:expr) => {
            entries.push(($t, $ty, $c, $v))
        };
    }
    macro_rules! ptr {
        ($t:expr, $ty:expr, $c:expr, $v:expr) => {{
            let off = add_value($v);
            entries.push(($t, $ty, $c, off));
        }};
    }

    inl!(254, 4, 1, 0); // NewSubfileType: primary
    inl!(256, 4, 1, W); // ImageWidth
    inl!(257, 4, 1, H); // ImageLength
    inl!(258, 3, 1, 16); // BitsPerSample
    inl!(259, 3, 1, 1); // Compression: none
    inl!(262, 3, 1, 32803); // PhotometricInterpretation: CFA
    ptr!(271, 2, 5, b"Foto\0"); // Make
    ptr!(272, 2, 5, b"Test\0"); // Model
    inl!(273, 4, 1, RAW_OFFSET); // StripOffsets
    inl!(277, 3, 1, 1); // SamplesPerPixel
    inl!(278, 4, 1, H); // RowsPerStrip
    inl!(279, 4, 1, W * H * 2); // StripByteCounts
    ptr!(33421, 3, 2, &[2u16.to_le_bytes(), 2u16.to_le_bytes()].concat()); // CFARepeatPatternDim
    ptr!(33422, 1, 4, &[0, 1, 1, 2]); // CFAPattern: RGGB
    ptr!(50706, 1, 4, &[1, 4, 0, 0]); // DNGVersion 1.4.0.0
    ptr!(50708, 2, 5, b"Test\0"); // UniqueCameraModel
                                  // ColorMatrix1: identity (SRATIONAL, 9 entries).
    let mut cm = Vec::new();
    for i in 0..9 {
        let num: u32 = if i % 4 == 0 { 10000 } else { 0 };
        cm.extend_from_slice(&num.to_le_bytes());
        cm.extend_from_slice(&10000u32.to_le_bytes());
    }
    ptr!(50721, 10, 9, &cm); // ColorMatrix1
    inl!(50714, 4, 1, 0); // BlackLevel
    inl!(50717, 4, 1, 65535); // WhiteLevel
    ptr!(50728, 5, 3, &{
        let mut v = Vec::new();
        for _ in 0..3 {
            v.extend_from_slice(&1u32.to_le_bytes());
            v.extend_from_slice(&1u32.to_le_bytes());
        }
        v
    }); // AsShotNeutral (1,1,1)

    entries.sort_by_key(|e| e.0);

    // Rebuild the IFD with sorted entries.
    let mut out = Vec::with_capacity(RAW_OFFSET as usize + pixel_len);
    out.extend_from_slice(&[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, ty, count, val) in &entries {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&ty.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&val.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes()); // next IFD: none
    assert_eq!(out.len() as u32, values_base, "IFD layout drift");
    out.extend_from_slice(&values);
    out.resize(RAW_OFFSET as usize, 0);
    // Raw CFA data: 16-bit LE gradient.
    for y in 0..H {
        for x in 0..W {
            let v = ((x * 511 / W.max(1)) + (y * 511 / H.max(1)) * 4) as u16;
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

fn write_dng(dir: &Path, name: &str, corrupt_magic: bool) -> PathBuf {
    let mut dng = minimal_dng();
    if corrupt_magic {
        dng[0] = 0xAB; // break the TIFF byte-order mark
    }
    let p = dir.join(name);
    std::fs::write(&p, &dng).unwrap();
    p
}

#[test]
fn raw_available_flag_matches_cfg() {
    assert_eq!(RAW_AVAILABLE, cfg!(all(feature = "raw", not(target_os = "windows"))));
}

#[test]
fn dng_decodes_to_thumbnail() {
    if !RAW_AVAILABLE {
        eprintln!("skipping: raw feature not compiled in");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let src = write_dng(tmp.path(), "a.dng", false);
    assert_thumb_dims(&src, W, H);
}

#[test]
fn corrupt_raw_header_falls_back_to_embedded_jpeg() {
    if !RAW_AVAILABLE {
        eprintln!("skipping: raw feature not compiled in");
        return;
    }
    let tmp = TempDir::new().unwrap();
    // A valid JPEG embedded in an otherwise corrupt RAW container.
    let mut raw = minimal_dng();
    raw[0] = 0xAB;
    raw.extend_from_slice(&make_jpeg(96, 64));
    let src = tmp.path().join("corrupt.dng");
    std::fs::write(&src, &raw).unwrap();

    let t = thumbnail(&src, temp_cache().path(), ThumbOptions::default()).unwrap();
    assert_eq!((t.width, t.height), (96, 64), "expected embedded-JPEG fallback dims");
}

#[test]
fn vendor_raw_samples_decode() {
    if !RAW_AVAILABLE {
        eprintln!("skipping: raw feature not compiled in");
        return;
    }
    let dir = raw_samples();
    let mut found = 0;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let p = e.path();
            let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
            // Only real camera RAW containers; .md readme files are skipped.
            if !matches!(
                ext.as_deref(),
                Some("cr2" | "cr3" | "nef" | "arw" | "raf" | "orf" | "rw2" | "dng")
            ) {
                continue;
            }
            found += 1;
            let t = thumbnail(&p, temp_cache().path(), ThumbOptions::default())
                .unwrap_or_else(|e| panic!("vendor sample {} failed: {e}", p.display()));
            assert!(t.width > 0 && t.height > 0);
        }
    }
    if found == 0 {
        eprintln!(
            "skipping: no vendor RAW samples under {} (real camera files are never committed; \
             drop small samples there to run the vendor leg)",
            dir.display()
        );
    }
}

/// Malformed RAW bytes must come back as an error (or a fallback), never a
/// panic (§9 robustness). Runs under catch_unwind because the decoder is C++.
#[test]
fn fuzz_malformed_raw_never_panics() {
    if !RAW_AVAILABLE {
        eprintln!("skipping: raw feature not compiled in");
        return;
    }
    let mut corpus: Vec<Vec<u8>> = Vec::new();
    let valid = minimal_dng();
    // Truncations and single-byte corruptions of a valid container, plus
    // hand-rolled garbage shaped like RAW containers.
    for cut in [1usize, 8, 64, 512, valid.len().saturating_sub(1)] {
        corpus.push(valid[..cut.min(valid.len())].to_vec());
    }
    for i in (0..valid.len()).step_by(97) {
        let mut v = valid.clone();
        v[i] = v[i].wrapping_add(0xA5);
        corpus.push(v);
    }
    corpus.push(b"FUJIFILMCCD-RAW\x00garbage".to_vec());
    corpus.push(vec![0xFF; 4096]);
    corpus.push(vec![0x00; 16]);
    for bytes in &corpus {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("fuzz.raw");
        std::fs::write(&src, bytes).unwrap();
        let res = std::panic::catch_unwind(|| {
            // Either a decoded thumbnail or an error — never a process abort.
            let _ = thumbnail(&src, dir.path().join("cache").as_path(), ThumbOptions::default());
        });
        assert!(res.is_ok(), "decoder panicked on {} bytes", bytes.len());
    }
}

/// Trivially valid JPEG bytes (1×1-ish gradient) used as an embedded preview.
fn make_jpeg(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

#[test]
fn raw_decode_error_is_error_not_panic() {
    if !RAW_AVAILABLE {
        eprintln!("skipping: raw feature not compiled in");
        return;
    }
    // TIFF magic but garbage payload: LibRaw errors, no embedded JPEG, so the
    // render must return an error (degraded state is handled upstream).
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("garbage.tif");
    std::fs::write(
        &src,
        std::iter::repeat_n(0xABu8, 512).chain([0x49u8, 0x49, 0x2A, 0x00]).collect::<Vec<u8>>(),
    )
    .unwrap();
    let err = thumbnail(&src, tmp.path().join("c").as_path(), ThumbOptions::default()).unwrap_err();
    assert!(matches!(err, RenderError::Decode { .. }), "unexpected: {err:?}");
}
