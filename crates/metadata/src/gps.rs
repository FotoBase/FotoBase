//! GPS coordinate entry for the metadata editor (TASK-03-020): decimal
//! degrees and degrees/minutes/seconds as a person types them, turned into
//! the signed decimal degrees the sidecar stores. Reading what other tools
//! wrote (`52,30.99N`, …) stays with the sidecar reader.

/// Which coordinate is being entered; sets the range and the hemisphere
/// letters that make sense.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpsAxis {
    Latitude,
    Longitude,
}

impl GpsAxis {
    fn max_degrees(self) -> u32 {
        match self {
            GpsAxis::Latitude => 90,
            GpsAxis::Longitude => 180,
        }
    }

    /// (positive, negative) hemisphere letters.
    fn hemispheres(self) -> (char, char) {
        match self {
            GpsAxis::Latitude => ('N', 'S'),
            GpsAxis::Longitude => ('E', 'W'),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GpsParseError {
    #[error("no coordinate entered")]
    Empty,
    #[error("not a coordinate: use decimal degrees (52.5331) or degrees, minutes, seconds (52°31'59.9\"N)")]
    Invalid,
    #[error("a sign and a hemisphere letter were both given; use one of them")]
    SignAndHemisphere,
    #[error("hemisphere {0} does not fit this coordinate")]
    WrongHemisphere(char),
    #[error("minutes and seconds must be below 60")]
    MinutesOrSecondsOutOfRange,
    #[error("out of range: at most ±{max}°")]
    OutOfRange { max: u32 },
}

/// Parse one coordinate. Accepts a leading `+`/`-` or a hemisphere letter
/// (before or after the number), and one to three numbers — degrees,
/// minutes, seconds — separated by spaces or the marks `° ' " ′ ″`. Only the
/// last number may carry a fraction.
pub fn parse_coordinate(text: &str, axis: GpsAxis) -> Result<f64, GpsParseError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(GpsParseError::Empty);
    }
    let (positive, negative) = axis.hemispheres();
    let is_hemisphere = |c: char| matches!(c.to_ascii_uppercase(), 'N' | 'S' | 'E' | 'W');

    let mut body = trimmed;
    let mut hemisphere = None;
    for edge in [body.chars().next(), body.chars().last()] {
        if let Some(c) = edge.filter(|c| is_hemisphere(*c)) {
            if hemisphere.is_some() {
                return Err(GpsParseError::Invalid);
            }
            hemisphere = Some(c.to_ascii_uppercase());
            body = if body.starts_with(c) {
                &body[c.len_utf8()..]
            } else {
                &body[..body.len() - c.len_utf8()]
            };
            if body.is_empty() {
                return Err(GpsParseError::Invalid);
            }
        }
    }
    if let Some(h) = hemisphere {
        if h != positive && h != negative {
            return Err(GpsParseError::WrongHemisphere(h));
        }
    }

    let (explicit_sign, body) = match body.trim_start().chars().next() {
        Some('-') => (Some(-1.0), &body.trim_start()[1..]),
        Some('+') => (Some(1.0), &body.trim_start()[1..]),
        _ => (None, body),
    };
    if explicit_sign.is_some() && hemisphere.is_some() {
        return Err(GpsParseError::SignAndHemisphere);
    }

    let numbers = numbers(body)?;
    let (deg, min, sec) = match numbers.as_slice() {
        [d] => (d.value, 0.0, 0.0),
        [d, m] if !d.fractional => (d.value, m.value, 0.0),
        [d, m, s] if !d.fractional && !m.fractional => (d.value, m.value, s.value),
        _ => return Err(GpsParseError::Invalid),
    };
    if min >= 60.0 || sec >= 60.0 {
        return Err(GpsParseError::MinutesOrSecondsOutOfRange);
    }

    let magnitude = deg + min / 60.0 + sec / 3600.0;
    let negative_sign = explicit_sign == Some(-1.0) || hemisphere == Some(negative);
    let value = if negative_sign { -magnitude } else { magnitude };
    let max = axis.max_degrees();
    if value.abs() > f64::from(max) {
        return Err(GpsParseError::OutOfRange { max });
    }
    Ok(value)
}

struct Number {
    value: f64,
    fractional: bool,
}

/// The numbers of `body`, separated by whitespace or degree/minute/second
/// marks. Anything else (letters, a comma, an exponent) is invalid.
fn numbers(body: &str) -> Result<Vec<Number>, GpsParseError> {
    fn flush(current: &mut String, out: &mut Vec<Number>) -> Result<(), GpsParseError> {
        if current.is_empty() {
            return Ok(());
        }
        let well_formed = current.matches('.').count() <= 1
            && current.chars().next().is_some_and(|c| c.is_ascii_digit())
            && current.chars().last().is_some_and(|c| c.is_ascii_digit());
        let value: f64 = current.parse().map_err(|_| GpsParseError::Invalid)?;
        if !well_formed || !value.is_finite() {
            return Err(GpsParseError::Invalid);
        }
        out.push(Number { value, fractional: current.contains('.') });
        current.clear();
        Ok(())
    }

    let mut out = Vec::new();
    let mut current = String::new();
    for c in body.chars() {
        match c {
            '0'..='9' | '.' => current.push(c),
            c if c.is_whitespace() => flush(&mut current, &mut out)?,
            '°' | 'º' | '\'' | '′' | '’' | '"' | '″' | '”' => {
                flush(&mut current, &mut out)?
            }
            _ => return Err(GpsParseError::Invalid),
        }
    }
    flush(&mut current, &mut out)?;
    if out.is_empty() {
        return Err(GpsParseError::Invalid);
    }
    Ok(out)
}
