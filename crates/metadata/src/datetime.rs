//! Capture-time arithmetic for the date/time adjustment tools (TASK-03-019,
//! PLAN §6 P2a): shift, timezone fix and multi-camera clock alignment.
//!
//! A [`CaptureTime`] is a wall-clock reading plus an optional UTC offset,
//! the shape of `exif:DateTimeOriginal` in XMP. Wall clocks are carried as
//! seconds-since-epoch *as if* they were UTC, so calendar math never depends
//! on the machine's timezone. A reading without an offset is "naive"; for
//! instant comparisons it is taken as UTC (the usual "camera left on UTC"
//! case). Every operation returns `None` instead of producing a date outside
//! years 1–9999 or an offset outside the real-world −12:00…+14:00 range.

use std::fmt::Write as _;

const MIN_OFFSET: i32 = -12 * 60;
const MAX_OFFSET: i32 = 14 * 60;
const SECS_PER_DAY: i64 = 86_400;

/// One capture time: wall clock, fractional seconds (kept verbatim) and the
/// UTC offset when the source recorded one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureTime {
    /// Wall-clock seconds since 1970-01-01T00:00:00, read as if UTC.
    wall: i64,
    /// Fractional-second digits as written (`"25"` for `.25`), may be empty.
    frac: String,
    /// Offset from UTC in minutes; `None` = naive.
    offset: Option<i32>,
}

impl CaptureTime {
    /// Parse the XMP/ISO-8601 form (`2024-06-01T12:00:00+02:00`, `…Z`,
    /// naive, minutes-only) and EXIF's `2024:06:01 12:00:00`. Anything that
    /// is not a real calendar date and clock time yields `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if !text.is_ascii() {
            return None;
        }
        let (date, rest) = (text.get(..10)?, text.get(11..)?);
        if !matches!(text.as_bytes()[10], b'T' | b' ') {
            return None;
        }
        let db = date.as_bytes();
        if !matches!(db[4], b'-' | b':') || db[7] != db[4] {
            return None;
        }
        let year = digits(&date[0..4])?;
        let month = digits(&date[5..7])?;
        let day = digits(&date[8..10])?;
        if !(1..=9999).contains(&year) || !(1..=12).contains(&month) {
            return None;
        }
        if !(1..=days_in_month(year, month)).contains(&day) {
            return None;
        }

        let hour = digits(rest.get(0..2)?)?;
        if rest.as_bytes().get(2) != Some(&b':') {
            return None;
        }
        let minute = digits(rest.get(3..5)?)?;
        let mut tail = &rest[5..];
        let mut second = 0;
        if let Some(after) = tail.strip_prefix(':') {
            second = digits(after.get(0..2)?)?;
            tail = &after[2..];
        }
        if hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        let mut frac = String::new();
        if let Some(after) = tail.strip_prefix('.') {
            let end = after.find(|c: char| !c.is_ascii_digit()).unwrap_or(after.len());
            if end == 0 {
                return None;
            }
            frac = after[..end].to_string();
            tail = &after[end..];
        }
        let offset = match tail {
            "" => None,
            "Z" | "z" => Some(0),
            _ => Some(parse_offset(tail)?),
        };
        if offset.is_some_and(|o| !(MIN_OFFSET..=MAX_OFFSET).contains(&o)) {
            return None;
        }

        let days = days_from_civil(i64::from(year), i64::from(month), i64::from(day));
        let wall = days * SECS_PER_DAY
            + i64::from(hour) * 3600
            + i64::from(minute) * 60
            + i64::from(second);
        Some(Self { wall, frac, offset })
    }

    /// A UTC reading of `unix_secs`; the fallback for files without any
    /// recorded capture time (their mtime).
    pub fn from_unix(unix_secs: i64) -> Option<Self> {
        in_range(unix_secs).then(|| Self { wall: unix_secs, frac: String::new(), offset: Some(0) })
    }

    /// XMP / ISO-8601 text: `2024-06-01T12:00:00+02:00`, no offset suffix
    /// when naive.
    pub fn to_xmp(&self) -> String {
        let days = self.wall.div_euclid(SECS_PER_DAY);
        let secs = self.wall.rem_euclid(SECS_PER_DAY);
        let (y, m, d) = civil_from_days(days);
        let mut out = String::with_capacity(32);
        let _ = write!(
            out,
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
            secs / 3600,
            secs % 3600 / 60,
            secs % 60
        );
        if !self.frac.is_empty() {
            out.push('.');
            out.push_str(&self.frac);
        }
        if let Some(off) = self.offset {
            let sign = if off < 0 { '-' } else { '+' };
            let _ = write!(out, "{sign}{:02}:{:02}", off.abs() / 60, off.abs() % 60);
        }
        out
    }

    /// UTC offset in minutes, `None` when the reading is naive.
    pub fn offset_minutes(&self) -> Option<i32> {
        self.offset
    }

    /// The absolute moment, seconds since the Unix epoch (naive = UTC).
    pub fn instant(&self) -> i64 {
        self.wall - i64::from(self.offset.unwrap_or(0)) * 60
    }

    /// Move the wall clock by `delta_secs`; the offset stays.
    pub fn shifted(&self, delta_secs: i64) -> Option<Self> {
        let wall = self.wall.checked_add(delta_secs).filter(|w| in_range(*w))?;
        Some(Self { wall, ..self.clone() })
    }

    /// Timezone fix, wall clock right / offset wrong: relabel with `offset`
    /// minutes and leave the clock reading alone. The instant moves.
    pub fn with_offset_keeping_wall_clock(&self, offset: i32) -> Option<Self> {
        valid_offset(offset)?;
        Some(Self { offset: Some(offset), ..self.clone() })
    }

    /// Timezone fix, instant right / zone wrong: show the same moment in
    /// `offset` minutes. A naive reading is taken as UTC.
    pub fn converted_to_offset(&self, offset: i32) -> Option<Self> {
        valid_offset(offset)?;
        let wall = self.instant() + i64::from(offset) * 60;
        in_range(wall).then(|| Self { wall, frac: self.frac.clone(), offset: Some(offset) })
    }
}

fn valid_offset(offset: i32) -> Option<()> {
    (MIN_OFFSET..=MAX_OFFSET).contains(&offset).then_some(())
}

fn in_range(wall: i64) -> bool {
    const MIN: i64 = -62_135_596_800; // 0001-01-01T00:00:00
    const MAX: i64 = 253_402_300_799; // 9999-12-31T23:59:59
    (MIN..=MAX).contains(&wall)
}

fn digits(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// `+HH:MM`, `-HHMM`.
fn parse_offset(s: &str) -> Option<i32> {
    let sign = match s.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let body = &s[1..];
    let (h, m) = match body.len() {
        5 if body.as_bytes()[2] == b':' => (&body[0..2], &body[3..5]),
        4 => (&body[0..2], &body[2..4]),
        _ => return None,
    };
    let (h, m) = (digits(h)?, digits(m)?);
    (m < 60).then_some(sign * (h * 60 + m) as i32)
}

fn is_leap(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}
