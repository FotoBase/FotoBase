//! TASK-10-02-02: the EXIF fields Lensfun needs to pick a lens profile
//! (camera, lens, focal length, aperture, focus distance), read-only from
//! the original.

use std::io::Cursor;
use std::path::Path;

use exif::{Field, In, Rational, Tag, Value};
use fotobase_metadata::{read_lens_exif, LensExif};

fn ascii(tag: Tag, s: &str) -> Field {
    Field { tag, ifd_num: In::PRIMARY, value: Value::Ascii(vec![s.as_bytes().to_vec()]) }
}

fn rational(tag: Tag, num: u32, denom: u32) -> Field {
    Field { tag, ifd_num: In::PRIMARY, value: Value::Rational(vec![Rational { num, denom }]) }
}

/// A small JPEG carrying `fields` as its EXIF APP1 segment.
fn jpeg_with_exif(path: &Path, fields: &[Field]) {
    let mut tiff = Cursor::new(Vec::new());
    let mut writer = exif::experimental::Writer::new();
    fields.iter().for_each(|f| writer.push_field(f));
    writer.write(&mut tiff, false).expect("exif encodes");
    let tiff = tiff.into_inner();

    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode(&[128u8; 8 * 8 * 3], 8, 8, image::ExtendedColorType::Rgb8)
        .unwrap();
    let mut app1 = vec![0xFF, 0xE1];
    app1.extend(((tiff.len() + 8) as u16).to_be_bytes());
    app1.extend(b"Exif\0\0");
    app1.extend(&tiff);
    let mut out = jpeg[..2].to_vec(); // SOI
    out.extend(app1);
    out.extend(&jpeg[2..]);
    std::fs::write(path, out).unwrap();
}

#[test]
fn reads_camera_lens_focal_aperture_and_distance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shot.jpg");
    jpeg_with_exif(
        &path,
        &[
            ascii(Tag::Make, "Canon"),
            ascii(Tag::Model, "Canon EOS 5D Mark II"),
            ascii(Tag::LensMake, "Canon"),
            ascii(Tag::LensModel, "EF50mm f/1.8 II"),
            rational(Tag::FocalLength, 50, 1),
            rational(Tag::FNumber, 28, 10),
            rational(Tag::SubjectDistance, 3, 2),
        ],
    );
    let before = std::fs::read(&path).unwrap();
    let exif = read_lens_exif(&path).expect("EXIF reads");
    assert_eq!(
        exif,
        LensExif {
            camera_maker: "Canon".into(),
            camera_model: "Canon EOS 5D Mark II".into(),
            lens_maker: Some("Canon".into()),
            lens_model: Some("EF50mm f/1.8 II".into()),
            focal: Some(50.0),
            aperture: Some(2.8),
            distance: Some(1.5),
        }
    );
    assert_eq!(std::fs::read(&path).unwrap(), before, "originals are read-only");
}

#[test]
fn missing_lens_fields_are_none_and_missing_exif_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("compact.jpg");
    jpeg_with_exif(&path, &[ascii(Tag::Make, "Testcam"), ascii(Tag::Model, "Testcam One")]);
    let exif = read_lens_exif(&path).unwrap();
    assert_eq!((exif.lens_model, exif.focal, exif.aperture), (None, None, None));

    let plain = dir.path().join("plain.jpg");
    std::fs::write(&plain, b"not an image").unwrap();
    assert!(read_lens_exif(&plain).is_err());
}
