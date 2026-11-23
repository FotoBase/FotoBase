//! Synthetic Live/Motion Photo fixtures (TASK-08-017). Byte-level builders:
//! the detectors only read container structure, so no real codec data is
//! needed and nothing binary is committed. Shared with the scanner tests via
//! `#[path]`.
#![allow(dead_code)]

pub const UUID: &str = "6F3A1B52-9C4D-4E7A-8B21-0D5E7F9A1C33";

/// Minimal MP4 `ftyp` box followed by filler: what the detector validates.
pub fn fake_mp4(payload: usize) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&24u32.to_be_bytes());
    v.extend_from_slice(b"ftypisom");
    v.extend_from_slice(&0u32.to_be_bytes());
    v.extend_from_slice(b"isomiso2");
    v.extend(std::iter::repeat_n(0xAB, payload));
    v
}

fn xmp_packet(body: &str) -> Vec<u8> {
    let mut seg = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    seg.extend_from_slice(
        format!("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF>{body}</rdf:RDF></x:xmpmeta>")
            .as_bytes(),
    );
    seg
}

fn jpeg_with_xmp(xmp: Option<Vec<u8>>, trailer: &[u8]) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8];
    if let Some(seg) = xmp {
        v.extend_from_slice(&[0xFF, 0xE1]);
        v.extend_from_slice(&((seg.len() + 2) as u16).to_be_bytes());
        v.extend_from_slice(&seg);
    }
    // Start of scan, a little entropy-coded data, end of image.
    v.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 1, 2, 3, 4, 0xFF, 0xD9]);
    v.extend_from_slice(trailer);
    v
}

/// Google "MicroVideo" (v1) Motion Photo: offset counted from the file end.
pub fn micro_video_jpeg(clip: &[u8]) -> Vec<u8> {
    let xmp = xmp_packet(&format!(
        "<rdf:Description xmlns:GCamera=\"http://ns.google.com/photos/1.0/camera/\" \
         GCamera:MicroVideo=\"1\" GCamera:MicroVideoVersion=\"1\" \
         GCamera:MicroVideoOffset=\"{}\"/>",
        clip.len()
    ));
    jpeg_with_xmp(Some(xmp), clip)
}

/// Motion Photo format v2: `Container:Directory` with a `MotionPhoto` item.
pub fn container_jpeg(clip: &[u8]) -> Vec<u8> {
    let xmp = xmp_packet(&format!(
        "<rdf:Description xmlns:GCamera=\"http://ns.google.com/photos/1.0/camera/\" \
         xmlns:Container=\"http://ns.google.com/photos/1.0/container/\" \
         xmlns:Item=\"http://ns.google.com/photos/1.0/container/item/\" \
         GCamera:MotionPhoto=\"1\"><Container:Directory><rdf:Seq>\
         <rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Mime=\"image/jpeg\" \
         Item:Semantic=\"Primary\" Item:Length=\"0\"/></rdf:li>\
         <rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Mime=\"video/mp4\" \
         Item:Semantic=\"MotionPhoto\" Item:Length=\"{}\"/></rdf:li>\
         </rdf:Seq></Container:Directory></rdf:Description>",
        clip.len()
    ));
    jpeg_with_xmp(Some(xmp), clip)
}

/// A plain JPEG followed by arbitrary trailing bytes: must never be taken
/// for a Motion Photo.
pub fn jpeg_with_garbage(garbage: &[u8]) -> Vec<u8> {
    jpeg_with_xmp(None, garbage)
}

/// MicroVideo markers present but the trailer is not an MP4.
pub fn micro_video_jpeg_bad_trailer() -> Vec<u8> {
    micro_video_jpeg(&[0x11; 64])
}

/// A HEIC-shaped file (`ftypheic` + filler) whose EXIF carries an Apple
/// MakerNote with the ContentIdentifier (tag 0x0011), big-endian.
pub fn apple_heic(content_id: Option<&str>) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&24u32.to_be_bytes());
    v.extend_from_slice(b"ftypheic");
    v.extend_from_slice(&0u32.to_be_bytes());
    v.extend_from_slice(b"heicmif1");
    v.extend(std::iter::repeat_n(0u8, 200));
    // MakerNote: "Apple iOS\0" + version + byte order, IFD at +14.
    let mut mn = b"Apple iOS\0\0\x01MM".to_vec();
    match content_id {
        Some(id) => {
            mn.extend_from_slice(&2u16.to_be_bytes());
            // Entry 1: an unrelated tag with an inline value.
            mn.extend_from_slice(&0x0001u16.to_be_bytes());
            mn.extend_from_slice(&9u16.to_be_bytes());
            mn.extend_from_slice(&1u32.to_be_bytes());
            mn.extend_from_slice(&[0, 0, 0, 5]);
            // Entry 2: ContentIdentifier, ASCII, value placed after the IFD.
            let value_off = (mn.len() + 12 + 4) as u32;
            mn.extend_from_slice(&0x0011u16.to_be_bytes());
            mn.extend_from_slice(&2u16.to_be_bytes());
            mn.extend_from_slice(&((id.len() + 1) as u32).to_be_bytes());
            mn.extend_from_slice(&value_off.to_be_bytes());
            mn.extend_from_slice(&0u32.to_be_bytes()); // next IFD
            mn.extend_from_slice(id.as_bytes());
            mn.push(0);
        }
        None => {
            mn.extend_from_slice(&0u16.to_be_bytes());
            mn.extend_from_slice(&0u32.to_be_bytes());
        }
    }
    v.extend_from_slice(&mn);
    v.extend(std::iter::repeat_n(0u8, 500));
    v
}

fn mp4_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut b = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    b.extend_from_slice(kind);
    b.extend_from_slice(payload);
    b
}

/// A QuickTime MOV: `ftyp`, `mdat`, then `moov/meta` (QuickTime style, no
/// version/flags) holding `keys` + `ilst`, the way iPhones write it. With
/// `content_id: None` the key list holds only an unrelated key.
pub fn apple_mov(content_id: Option<&str>) -> Vec<u8> {
    let mut mov = mp4_box(b"ftyp", b"qt  \0\0\0\0qt  ");
    mov.extend(mp4_box(b"mdat", &[0x55; 128]));

    let mut keys = vec![0, 0, 0, 0];
    let mut names: Vec<&str> = vec!["com.apple.quicktime.make"];
    if content_id.is_some() {
        names.push("com.apple.quicktime.content.identifier");
    }
    keys.extend_from_slice(&(names.len() as u32).to_be_bytes());
    for n in &names {
        keys.extend(mp4_box(b"mdta", n.as_bytes()));
    }

    let data_item = |value: &str| {
        let mut d = vec![0, 0, 0, 1, 0, 0, 0, 0];
        d.extend_from_slice(value.as_bytes());
        mp4_box(b"data", &d)
    };
    let mut ilst = Vec::new();
    ilst.extend(mp4_box(&1u32.to_be_bytes(), &data_item("Apple")));
    if let Some(id) = content_id {
        ilst.extend(mp4_box(&2u32.to_be_bytes(), &data_item(id)));
    }

    let mut meta = mp4_box(b"hdlr", &[0u8; 24]);
    meta.extend(mp4_box(b"keys", &keys));
    meta.extend(mp4_box(b"ilst", &ilst));
    mov.extend(mp4_box(b"moov", &mp4_box(b"meta", &meta)));
    mov
}
