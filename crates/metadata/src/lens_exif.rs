//! Read-only EXIF lookup of the fields lens correction needs (TASK-10-02-02).
//!
//! Uses `kamadak-exif`, the read-only fallback reader named in PLAN §10; the
//! original is opened for reading only.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use exif::{In, Tag, Value};

/// Camera and lens as recorded by the camera.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LensExif {
    pub camera_maker: String,
    pub camera_model: String,
    pub lens_maker: Option<String>,
    pub lens_model: Option<String>,
    /// Focal length in mm.
    pub focal: Option<f64>,
    /// f-number.
    pub aperture: Option<f64>,
    /// Subject distance in meters.
    pub distance: Option<f64>,
}

/// Read the lens-correction EXIF fields of `path`. A file without readable
/// EXIF is an error; individual missing fields are `None`.
pub fn read_lens_exif(path: &Path) -> Result<LensExif, String> {
    let file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let exif = exif::Reader::new()
        .read_from_container(&mut BufReader::new(file))
        .map_err(|e| format!("no readable EXIF in {}: {e}", path.display()))?;
    let text = |tag| {
        exif.get_field(tag, In::PRIMARY).and_then(|f| match &f.value {
            Value::Ascii(parts) => parts
                .iter()
                .map(|p| String::from_utf8_lossy(p).trim_end_matches('\0').trim().to_string())
                .find(|s| !s.is_empty()),
            _ => None,
        })
    };
    let number = |tag| {
        exif.get_field(tag, In::PRIMARY)
            .and_then(|f| match &f.value {
                Value::Rational(v) => v.first().map(|r| r.to_f64()),
                Value::SRational(v) => v.first().map(|r| r.to_f64()),
                _ => f.value.get_uint(0).map(f64::from),
            })
            .filter(|v| v.is_finite() && *v > 0.0)
    };
    Ok(LensExif {
        camera_maker: text(Tag::Make).unwrap_or_default(),
        camera_model: text(Tag::Model).unwrap_or_default(),
        lens_maker: text(Tag::LensMake),
        lens_model: text(Tag::LensModel),
        focal: number(Tag::FocalLength),
        aperture: number(Tag::FNumber),
        distance: number(Tag::SubjectDistance),
    })
}
