//! Picasa.ini reader (TASK-03-022, PLAN §1 onboarding, §5 mapping).
//!
//! Picasa kept a folder's user data in a hidden `Picasa.ini` / `.picasa.ini`:
//! one `[<photo file name>]` section per photo (`star=yes`, `caption=`,
//! `keywords=a,b`, `albums=<token>,<token>`, `faces=rect64(<hex>),<contact>;…`),
//! `[.album:<token>]` album definitions and `[Contacts2]` (`<id>=<name>;<mail>`)
//! / legacy `[Contacts]` (`<id>=<name>`) tables that name the faces.
//!
//! This module only *reads*: the ini is never modified or deleted. The
//! parser is lenient by design — Picasa wrote a lot of keys we do not use
//! (`backuphash`, `rotate`, `filters`, `P2…`), and a damaged line must never
//! fail a migration — so unknown keys, sections and junk lines are ignored.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::sidecar::FaceRegion;

/// Picasa's "face without a contact" marker (an all-ones 64-bit id).
pub const UNNAMED_CONTACT: &str = "ffffffffffffffff";

/// Name given to an album whose definition is missing from the ini.
pub const UNNAMED_ALBUM: &str = "Picasa album";

/// File names Picasa used for the per-folder ini (case-insensitive: the
/// Windows variant is `Picasa.ini`, the Mac/newer one `.picasa.ini`).
const INI_NAMES: [&str; 2] = ["picasa.ini", ".picasa.ini"];

/// Whether a directory entry name is a Picasa ini.
pub fn is_picasa_ini_name(name: &str) -> bool {
    INI_NAMES.iter().any(|n| name.eq_ignore_ascii_case(n))
}

/// Every Picasa ini in `dir` (sorted, so reruns see the same order).
pub fn picasa_ini_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| e.file_name().to_str().is_some_and(is_picasa_ini_name))
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}

/// A face rectangle as fractions of the image (0..=1), edges not centre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceRect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl FaceRect {
    /// Decode a `rect64(<hex>)` payload: a 64-bit number holding four
    /// 16-bit edges (left, top, right, bottom), each `fraction * 65535`.
    /// Picasa strips leading zeros, so short hex is left-padded. Rejects
    /// non-hex, more than 16 digits and empty or reversed rectangles.
    pub fn from_rect64(hex: &str) -> Option<Self> {
        let hex = hex.trim();
        if hex.is_empty() || hex.len() > 16 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let v = u64::from_str_radix(hex, 16).ok()?;
        let edge = |shift: u32| ((v >> shift) & 0xffff) as f64 / 65535.0;
        let rect = FaceRect { left: edge(48), top: edge(32), right: edge(16), bottom: edge(0) };
        (rect.right > rect.left && rect.bottom > rect.top).then_some(rect)
    }

    /// MWG region geometry: centre point plus size, all normalized.
    pub fn to_region(&self, name: &str) -> FaceRegion {
        FaceRegion {
            name: name.to_string(),
            x: (self.left + self.right) / 2.0,
            y: (self.top + self.bottom) / 2.0,
            w: self.right - self.left,
            h: self.bottom - self.top,
        }
    }
}

/// One `faces=` entry. `name` is `None` for faces Picasa never named (the
/// all-ones contact) and for contact ids missing from the contact tables.
#[derive(Debug, Clone, PartialEq)]
pub struct PicasaFace {
    pub rect: FaceRect,
    pub name: Option<String>,
}

/// One photo section. `albums` holds album tokens, resolved against
/// [`PicasaIni::albums`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PicasaPhoto {
    pub filename: String,
    pub star: bool,
    pub caption: Option<String>,
    pub keywords: Vec<String>,
    pub albums: Vec<String>,
    pub faces: Vec<PicasaFace>,
}

/// A `[.album:<token>]` definition. `date` is `YYYY-MM-DD`.
#[derive(Debug, Clone, PartialEq)]
pub struct PicasaAlbum {
    pub token: String,
    pub name: String,
    pub date: Option<String>,
    pub place: Option<String>,
    pub description: Option<String>,
}

/// A parsed `Picasa.ini`: photo sections with something to migrate, plus
/// the album definitions.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PicasaIni {
    pub photos: Vec<PicasaPhoto>,
    pub albums: Vec<PicasaAlbum>,
}

type Keys = HashMap<String, String>;

impl PicasaIni {
    /// Parse ini text (callers decode bytes lossily: Picasa wrote UTF-8 or
    /// the system code page, and a stray byte must not fail the migration).
    pub fn parse(text: &str) -> Self {
        let sections = read_sections(text);

        let mut contacts: HashMap<String, String> = HashMap::new();
        for (name, keys) in &sections {
            if name.eq_ignore_ascii_case("contacts2") || name.eq_ignore_ascii_case("contacts") {
                for (id, value) in keys {
                    // `Name;email` — the name ends at the first `;`.
                    let who = value.split(';').next().unwrap_or("").trim();
                    if !who.is_empty() {
                        contacts.insert(id.clone(), who.to_string());
                    }
                }
            }
        }

        let mut photos = Vec::new();
        let mut albums = Vec::new();
        for (name, keys) in &sections {
            if let Some(token) = name.strip_prefix(".album:") {
                albums.push(album(token, keys));
            } else if is_photo_section(name) {
                let photo = photo(name, keys, &contacts);
                let has_data = photo.star
                    || photo.caption.is_some()
                    || !photo.keywords.is_empty()
                    || !photo.albums.is_empty()
                    || !photo.faces.is_empty();
                if has_data {
                    photos.push(photo);
                }
            }
        }
        PicasaIni { photos, albums }
    }

    /// A photo's section by file name (case-insensitive, like the Windows
    /// file system Picasa came from).
    pub fn photo(&self, filename: &str) -> Option<&PicasaPhoto> {
        let wanted = filename.to_lowercase();
        self.photos.iter().find(|p| p.filename.to_lowercase() == wanted)
    }

    /// The definition of an album token, if the ini has one.
    pub fn album(&self, token: &str) -> Option<&PicasaAlbum> {
        self.albums.iter().find(|a| a.token == token)
    }
}

fn is_photo_section(name: &str) -> bool {
    !(name.starts_with('.')
        || name.eq_ignore_ascii_case("picasa")
        || name.eq_ignore_ascii_case("contacts")
        || name.eq_ignore_ascii_case("contacts2"))
}

/// Section name → keys, in file order. Repeated sections merge (later keys
/// win); photo section names merge case-insensitively but keep the first
/// spelling. Key names are lowercased.
fn read_sections(text: &str) -> Vec<(String, Keys)> {
    let mut sections: Vec<(String, Keys)> = Vec::new();
    let mut current: Option<usize> = None;
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let name = name.trim();
            let at = sections.iter().position(|(n, _)| n.eq_ignore_ascii_case(name));
            current = Some(at.unwrap_or_else(|| {
                sections.push((name.to_string(), Keys::new()));
                sections.len() - 1
            }));
            continue;
        }
        let (Some(at), Some((key, value))) = (current, line.split_once('=')) else { continue };
        let key = key.trim().to_lowercase();
        if !key.is_empty() {
            sections[at].1.insert(key, value.trim().to_string());
        }
    }
    sections
}

fn list(value: Option<&String>, sep: char) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in value.map(String::as_str).unwrap_or("").split(sep) {
        let item = item.trim();
        if !item.is_empty() && !out.iter().any(|o| o == item) {
            out.push(item.to_string());
        }
    }
    out
}

fn non_empty(value: Option<&String>) -> Option<String> {
    value.map(|v| v.trim()).filter(|v| !v.is_empty()).map(str::to_string)
}

fn photo(filename: &str, keys: &Keys, contacts: &HashMap<String, String>) -> PicasaPhoto {
    let faces = keys
        .get("faces")
        .map(|v| v.split(';').filter_map(|f| face(f, contacts)).collect())
        .unwrap_or_default();
    PicasaPhoto {
        filename: filename.to_string(),
        star: keys.get("star").is_some_and(|v| v.eq_ignore_ascii_case("yes")),
        caption: non_empty(keys.get("caption")),
        keywords: list(keys.get("keywords"), ','),
        albums: list(keys.get("albums"), ','),
        faces,
    }
}

/// `rect64(<hex>),<contact id>`; an entry without a decodable rectangle is
/// dropped.
fn face(entry: &str, contacts: &HashMap<String, String>) -> Option<PicasaFace> {
    let entry = entry.trim().strip_prefix("rect64(")?;
    let (hex, rest) = entry.split_once(')')?;
    let rect = FaceRect::from_rect64(hex)?;
    let id = rest.trim().trim_start_matches(',').trim().to_lowercase();
    let name = (!id.is_empty() && id != UNNAMED_CONTACT).then(|| contacts.get(&id).cloned());
    Some(PicasaFace { rect, name: name.flatten() })
}

fn album(section_token: &str, keys: &Keys) -> PicasaAlbum {
    PicasaAlbum {
        token: non_empty(keys.get("token")).unwrap_or_else(|| section_token.to_string()),
        name: non_empty(keys.get("name")).unwrap_or_else(|| UNNAMED_ALBUM.to_string()),
        date: keys.get("date").and_then(|d| picasa_date(d)),
        place: non_empty(keys.get("location")),
        description: non_empty(keys.get("description")),
    }
}

/// `2009-04-14T21:05:42+00:00` (take the calendar day as written) or an old
/// spreadsheet serial such as `39917.8789` (days since 1899-12-30).
fn picasa_date(value: &str) -> Option<String> {
    let value = value.trim();
    let b = value.as_bytes();
    let iso = b.len() >= 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b[..4].iter().chain(&b[5..7]).chain(&b[8..10]).all(u8::is_ascii_digit);
    if iso {
        return Some(value[..10].to_string());
    }
    let serial: f64 = value.parse().ok()?;
    if !(1.0..2_958_466.0).contains(&serial) {
        return None;
    }
    let (y, m, d) = civil_from_days(serial.floor() as i64 - 25_569);
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// Days since 1970-01-01 → proleptic Gregorian (year, month, day).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}
