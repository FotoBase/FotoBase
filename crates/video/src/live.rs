//! Live Photo / Motion Photo detection and clip extraction (TASK-08-017,
//! PLAN §2 "Live/motion photos").
//!
//! Detection is pure container parsing — no ffmpeg, bounded reads, no panics
//! on malformed input (PLAN §9):
//!
//! - **Android Motion Photo**: a JPEG whose XMP declares an embedded clip
//!   (`GCamera:MicroVideoOffset`, or a `Container:Directory` item with
//!   `Item:Semantic="MotionPhoto"` and `Item:Length`). The clip is the file's
//!   tail; it is accepted only when an MP4 `ftyp` box really starts there,
//!   so a JPEG with arbitrary trailing bytes is never misdetected.
//! - **Apple Live Photo**: a still plus a sibling MOV that carry the same
//!   ContentIdentifier — the Apple MakerNote tag `0x0011` in the still's
//!   EXIF, and the `com.apple.quicktime.content.identifier` metadata key in
//!   the MOV. Finding the sibling is the scanner's job; [`live_pair_matches`]
//!   confirms the pair.
//!
//! Embedded clips are copied out byte-for-byte ([`extract_motion_clip`]):
//! the "demux" is a slice of the original, and the original is never
//! written (PLAN §3 rule 1).

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// An MP4 clip embedded at the tail of a Motion Photo JPEG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionClip {
    /// Byte offset of the clip inside the still.
    pub offset: u64,
    pub len: u64,
}

/// XMP lives before the scan data; a header that needs more than this to
/// reach it is not a camera-written Motion Photo.
const MAX_JPEG_HEADER: u64 = 4 * 1024 * 1024;
const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
/// The Apple MakerNote sits in the EXIF block near the start of the file.
const MAX_STILL_SCAN: u64 = 2 * 1024 * 1024;
/// Real `moov` boxes are small; refuse to buffer anything absurd.
const MAX_MOOV: u64 = 64 * 1024 * 1024;
const CONTENT_ID_KEY: &[u8] = b"com.apple.quicktime.content.identifier";

/// Detect an embedded Motion Photo clip in `path`. `Ok(None)` for anything
/// that is not one — plain JPEGs, other formats, malformed XMP, or a trailer
/// that is not an MP4.
pub fn detect_motion_photo(path: &Path) -> io::Result<Option<MotionClip>> {
    let mut f = File::open(path)?;
    let file_len = f.metadata()?.len();
    let Some(xmp) = read_jpeg_xmp(&mut f)? else { return Ok(None) };
    let Some(len) = clip_len_from_xmp(&xmp) else { return Ok(None) };
    // The clip must leave room for the JPEG it rides on.
    if len < 12 || len >= file_len.saturating_sub(4) {
        return Ok(None);
    }
    let offset = file_len - len;
    f.seek(SeekFrom::Start(offset))?;
    let mut head = [0u8; 8];
    if f.read_exact(&mut head).is_err() || &head[4..8] != b"ftyp" {
        return Ok(None);
    }
    Ok(Some(MotionClip { offset, len }))
}

/// Copy a detected clip out of the still into `dest` (temp file + rename, so
/// a crash never leaves a truncated clip under the final name).
pub fn extract_motion_clip(src: &Path, clip: MotionClip, dest: &Path) -> io::Result<()> {
    let mut input = File::open(src)?;
    input.seek(SeekFrom::Start(clip.offset))?;
    let mut tmp_name = dest.as_os_str().to_owned();
    tmp_name.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp_name);
    let copied = (|| {
        let mut out = File::create(&tmp)?;
        let n = io::copy(&mut input.take(clip.len), &mut out)?;
        if n != clip.len {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "short motion clip"));
        }
        out.sync_all()
    })();
    match copied.and_then(|()| std::fs::rename(&tmp, dest)) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// ContentIdentifier of an Apple still (HEIC/JPEG), from the EXIF MakerNote.
pub fn still_content_id(path: &Path) -> io::Result<Option<String>> {
    let mut buf = Vec::new();
    File::open(path)?.take(MAX_STILL_SCAN).read_to_end(&mut buf)?;
    Ok(apple_maker_note_content_id(&buf))
}

/// ContentIdentifier of a Live Photo's MOV, from the QuickTime `moov/meta`
/// key list.
pub fn video_content_id(path: &Path) -> io::Result<Option<String>> {
    let mut f = File::open(path)?;
    let file_len = f.metadata()?.len();
    let mut pos = 0u64;
    while pos + 8 <= file_len {
        f.seek(SeekFrom::Start(pos))?;
        let mut hdr = [0u8; 8];
        f.read_exact(&mut hdr)?;
        let size32 = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
        let (header_len, size) = match size32 {
            0 => (8, file_len - pos),
            1 => {
                let mut big = [0u8; 8];
                f.read_exact(&mut big)?;
                (16, u64::from_be_bytes(big))
            }
            n => (8, n),
        };
        if size < header_len {
            return Ok(None);
        }
        if &hdr[4..8] == b"moov" {
            let body = size - header_len;
            if body > MAX_MOOV || pos + size > file_len {
                return Ok(None);
            }
            let mut moov = vec![0u8; body as usize];
            f.read_exact(&mut moov)?;
            return Ok(content_id_from_moov(&moov));
        }
        pos += size;
    }
    Ok(None)
}

/// A still and a MOV form a Live Photo when both carry the same, non-empty
/// ContentIdentifier. A missing identifier on either side means "not
/// provable" — the files stay a plain still + video.
pub fn live_pair_matches(still: &Path, video: &Path) -> io::Result<bool> {
    let Some(a) = still_content_id(still)? else { return Ok(false) };
    let Some(b) = video_content_id(video)? else { return Ok(false) };
    Ok(a.eq_ignore_ascii_case(&b))
}

// --- Motion Photo XMP -------------------------------------------------------

/// Walk the JPEG segments up to the scan data and return the XMP packet text.
fn read_jpeg_xmp(f: &mut File) -> io::Result<Option<String>> {
    let mut soi = [0u8; 2];
    if f.read_exact(&mut soi).is_err() || soi != [0xFF, 0xD8] {
        return Ok(None);
    }
    let mut pos = 2u64;
    while pos < MAX_JPEG_HEADER {
        let mut marker = [0u8; 2];
        if f.read_exact(&mut marker).is_err() || marker[0] != 0xFF {
            return Ok(None);
        }
        pos += 2;
        match marker[1] {
            // Stuffing / standalone markers carry no length.
            0xFF => {
                f.seek(SeekFrom::Current(-1))?;
                pos -= 1;
                continue;
            }
            0x00 | 0x01 | 0xD0..=0xD8 => continue,
            // Start of scan or end of image: no XMP past this point.
            0xDA | 0xD9 => return Ok(None),
            _ => {}
        }
        let mut len = [0u8; 2];
        if f.read_exact(&mut len).is_err() {
            return Ok(None);
        }
        let seg_len = u16::from_be_bytes(len) as u64;
        if seg_len < 2 {
            return Ok(None);
        }
        let body = seg_len - 2;
        if marker[1] == 0xE1 && body >= XMP_HEADER.len() as u64 {
            let mut data = vec![0u8; body as usize];
            if f.read_exact(&mut data).is_err() {
                return Ok(None);
            }
            if data.starts_with(XMP_HEADER) {
                return Ok(Some(String::from_utf8_lossy(&data[XMP_HEADER.len()..]).into_owned()));
            }
        } else {
            f.seek(SeekFrom::Current(body as i64))?;
        }
        pos += 2 + body;
    }
    Ok(None)
}

/// The embedded clip's byte length as declared by the XMP.
fn clip_len_from_xmp(xmp: &str) -> Option<u64> {
    // Format v2: the `MotionPhoto` item of the container directory.
    if let Some(at) = xmp
        .find("Item:Semantic=\"MotionPhoto\"")
        .or_else(|| xmp.find("Item:Semantic='MotionPhoto'"))
    {
        let start = xmp[..at].rfind('<').unwrap_or(0);
        let end = xmp[at..].find('>').map_or(xmp.len(), |i| at + i);
        if let Some(n) = attr_value(&xmp[start..end], "Item:Length").and_then(|v| v.parse().ok()) {
            return Some(n);
        }
    }
    // Format v1: `MicroVideoOffset` counts from the end of the file.
    if attr_value(xmp, "GCamera:MicroVideo") == Some("0") {
        return None;
    }
    value_of(xmp, "GCamera:MicroVideoOffset").and_then(|v| v.parse().ok())
}

/// `name="value"` or `name='value'` inside `text`.
fn attr_value<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(i) = text[from..].find(name) {
        let after = from + i + name.len();
        let rest = &text[after..];
        let boundary = text[..from + i].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        if boundary {
            if let Some(r) = rest.strip_prefix('=') {
                let quote = r.chars().next().filter(|c| *c == '"' || *c == '\'')?;
                let r = &r[1..];
                return r.find(quote).map(|e| &r[..e]);
            }
        }
        from = after;
    }
    None
}

/// Attribute form, else `<name>value</name>` element form.
fn value_of<'a>(xmp: &'a str, name: &str) -> Option<&'a str> {
    if let Some(v) = attr_value(xmp, name) {
        return Some(v);
    }
    let open = format!("<{name}>");
    let at = xmp.find(&open)? + open.len();
    let end = xmp[at..].find('<')?;
    Some(xmp[at..at + end].trim())
}

// --- Apple ContentIdentifier --------------------------------------------------

fn is_uuid(s: &[u8]) -> bool {
    s.len() == 36
        && s.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

fn find_bytes(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Parse the Apple MakerNote (`Apple iOS\0` + version + byte order, IFD at
/// +14, value offsets relative to the note start) for tag `0x0011`.
fn apple_maker_note_content_id(buf: &[u8]) -> Option<String> {
    const SIG: &[u8] = b"Apple iOS\0";
    let mut from = 0;
    while let Some(i) = find_bytes(&buf[from..], SIG) {
        let note = from + i;
        if let Some(id) = parse_maker_note(&buf[note..]) {
            return Some(id);
        }
        from = note + SIG.len();
    }
    None
}

fn parse_maker_note(note: &[u8]) -> Option<String> {
    let big = match note.get(12..14)? {
        b"MM" => true,
        b"II" => false,
        _ => return None,
    };
    let u16_at = |o: usize| -> Option<u16> {
        let b: [u8; 2] = note.get(o..o + 2)?.try_into().ok()?;
        Some(if big { u16::from_be_bytes(b) } else { u16::from_le_bytes(b) })
    };
    let u32_at = |o: usize| -> Option<u32> {
        let b: [u8; 4] = note.get(o..o + 4)?.try_into().ok()?;
        Some(if big { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
    };
    let count = u16_at(14)? as usize;
    for n in 0..count.min(256) {
        let e = 16 + n * 12;
        if u16_at(e)? != 0x0011 || u16_at(e + 2)? != 2 {
            continue;
        }
        let len = u32_at(e + 4)? as usize;
        let value = if len <= 4 {
            note.get(e + 8..e + 8 + len)?
        } else {
            let off = u32_at(e + 8)? as usize;
            note.get(off..off.checked_add(len)?)?
        };
        let value = value.strip_suffix(&[0]).unwrap_or(value);
        return is_uuid(value).then(|| String::from_utf8_lossy(value).into_owned());
    }
    None
}

// --- QuickTime content identifier ------------------------------------------------

/// Iterate the ISO/QuickTime boxes in `data`: `(type, payload)`.
fn boxes(data: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    let mut pos = 0usize;
    std::iter::from_fn(move || {
        let hdr = data.get(pos..pos + 8)?;
        let size = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as usize;
        let size = if size == 0 { data.len() - pos } else { size };
        if size < 8 {
            return None;
        }
        let payload = data.get(pos + 8..pos + size)?;
        pos += size;
        Some(([hdr[4], hdr[5], hdr[6], hdr[7]], payload))
    })
}

fn content_id_from_moov(moov: &[u8]) -> Option<String> {
    let (_, meta) = boxes(moov).find(|(k, _)| k == b"meta")?;
    // QuickTime `meta` starts straight with `hdlr`; ISO `meta` is a full box.
    let children = if meta.get(4..8) == Some(b"hdlr") { meta } else { meta.get(4..)? };

    let (_, keys) = boxes(children).find(|(k, _)| k == b"keys")?;
    let entry_count = u32::from_be_bytes(keys.get(4..8)?.try_into().ok()?) as usize;
    let index =
        boxes(keys.get(8..)?).take(entry_count).position(|(_, name)| name == CONTENT_ID_KEY)? + 1;

    let (_, ilst) = boxes(children).find(|(k, _)| k == b"ilst")?;
    let (_, item) = boxes(ilst).find(|(k, _)| u32::from_be_bytes(*k) as usize == index)?;
    let (_, data) = boxes(item).find(|(k, _)| k == b"data")?;
    let value = data.get(8..)?;
    let value = value.strip_suffix(&[0]).unwrap_or(value);
    is_uuid(value).then(|| String::from_utf8_lossy(value).into_owned())
}
