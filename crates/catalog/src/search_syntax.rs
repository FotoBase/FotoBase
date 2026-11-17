//! The search bar's field-filter grammar (TASK-07-005, PLAN §2 2026
//! additions "Advanced search syntax: `person:anna camera:X100V
//! before:2023`"). A whitespace tokenizer that honors double quotes splits
//! the query; a token whose `field:value` prefix names a known field
//! becomes a structured filter, everything else — including unknown fields
//! (`bogus:x`), empty values (`person:`) and malformed dates
//! (`before:notadate`) — stays literal free text. Degrade, never error:
//! a user typo must not blank the grid, only narrow it less.
//!
//! Pure and unit-tested: string in, [`ParsedQuery`] out; the SQL predicates
//! are built in [`crate::query`] from the normalized bounds.

/// The known filter fields. Anything else before the first `:` is text.
pub const KNOWN_FIELDS: [&str; 4] = ["person", "camera", "before", "after"];

/// One parsed query: the filters found plus the free text that remains.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ParsedQuery {
    /// Remaining free text (FTS + semantic blend input). Words, in order.
    pub text: String,
    /// `person:` — matched against `persons.name` case-insensitively.
    pub person: Option<String>,
    /// `camera:` — substring of the catalog's EXIF Make/Model mirror.
    pub camera: Option<String>,
    /// `before:` — normalized to an exclusive `YYYY-MM-DD` upper bound.
    pub before: Option<String>,
    /// `after:` — normalized to an inclusive `YYYY-MM-DD` lower bound.
    pub after: Option<String>,
}

/// Repeat a filter (`person:a person:b`)? The later value wins: the
/// grammar is a filter bag, not a boolean expression, and the last value
/// is the one the user was looking at when they hit Enter.
fn set_slot(parsed: &mut ParsedQuery, field: &str, value: String) {
    match field {
        "person" => parsed.person = Some(value),
        "camera" => parsed.camera = Some(value),
        "before" => parsed.before = Some(value),
        "after" => parsed.after = Some(value),
        _ => unreachable!("caller checked KNOWN_FIELDS"),
    }
}

/// A bare `YYYY` bound means the whole year: `before:2023` is strictly
/// before 2023-01-01, `after:2023` is 2023-01-01 onward (inclusive — the
/// symmetric reading of "before").
fn normalize_bound(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    if bytes.len() == 4 && bytes.iter().all(u8::is_ascii_digit) {
        return Some(format!("{value}-01-01"));
    }
    if bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes.iter().enumerate().all(|(i, &b)| (i == 4 || i == 7) || b.is_ascii_digit())
    {
        let month: u32 = value[5..7].parse().ok()?;
        let day: u32 = value[8..10].parse().ok()?;
        if (1..=12).contains(&month) && (1..=31).contains(&day) {
            return Some(value.to_string());
        }
    }
    None
}

/// Is this token a known field filter? `person:anna` is; `bogus:x`,
/// `person:` (empty value) and `before:notadate` are not — they stay text.
fn as_filter(token: &str) -> Option<(String, String)> {
    let (field, value) = token.split_once(':')?;
    let field = field.to_ascii_lowercase();
    if !KNOWN_FIELDS.contains(&field.as_str()) || value.is_empty() {
        return None;
    }
    // A quoted value inside an unquoted token (`person:"Ben`) with a
    // closing quote on the same token keeps its content; an unterminated
    // quote is malformed and stays text.
    let value = if value.starts_with('"') {
        value.strip_suffix('"')?;
        &value[1..value.len() - 1]
    } else {
        value
    };
    if field == "before" || field == "after" {
        return normalize_bound(value).map(|bound| (field, bound));
    }
    Some((field, value.to_string()))
}

/// Parse a search bar string into filters + free text (TASK-07-005).
/// Quoted values keep their spaces (`person:"mary ann"`); a quoted token
/// that is not a filter stays text with the quotes stripped.
pub fn parse_field_filters(input: &str) -> ParsedQuery {
    let mut parsed = ParsedQuery::default();
    let mut words: Vec<&str> = Vec::new();
    let mut i = 0; // byte offset of the unconsumed tail
    loop {
        while matches!(input[i..].chars().next(), Some(c) if c.is_whitespace()) {
            i += c_len(&input[i..]);
        }
        if i >= input.len() {
            break;
        }
        // A `field:"quoted value"` token may span spaces: the closing
        // quote, not whitespace, ends it.
        let filter_quoted = KNOWN_FIELDS.iter().find_map(|field| {
            let head = input.get(i..i + field.len() + 1)?;
            if !head[..field.len()].eq_ignore_ascii_case(field)
                || !head[field.len()..].starts_with(':')
            {
                return None;
            }
            let value = input[i + field.len() + 1..].strip_prefix('"')?;
            let close = value.find('"')?;
            Some((
                i + field.len() + 2 + close + 1,
                field.to_ascii_lowercase(),
                value[..close].to_string(),
            ))
        });
        if let Some((end, field, value)) = filter_quoted {
            set_slot(&mut parsed, &field, value);
            i = end;
            continue;
        }
        let (token, end) = if input[i..].starts_with('"') {
            let close = input[i + 1..].find('"').map_or(input.len(), |c| i + 1 + c);
            (&input[i + 1..close], (close + 1).min(input.len()))
        } else {
            let end = input[i..].find(char::is_whitespace).map_or(input.len(), |w| i + w);
            (&input[i..end], end)
        };
        i = end;
        match as_filter(token) {
            Some((field, value)) => set_slot(&mut parsed, &field, value),
            None => words.extend(token.split_whitespace()),
        }
    }
    parsed.text = words.join(" ");
    parsed
}

fn c_len(s: &str) -> usize {
    s.chars().next().map_or(0, char::len_utf8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TDD unit criterion: the spec's example query parses into the three
    /// filters plus the free-text remainder.
    #[test]
    fn parses_the_spec_example() {
        let parsed = parse_field_filters("person:anna camera:X100V before:2023 sunset");
        assert_eq!(
            parsed,
            ParsedQuery {
                text: "sunset".into(),
                person: Some("anna".into()),
                camera: Some("X100V".into()),
                before: Some("2023-01-01".into()),
                after: None,
            }
        );
    }

    #[test]
    fn quoted_values_keep_spaces_and_unknown_fields_stay_text() {
        let parsed = parse_field_filters(r#"person:"mary ann" bogus:x person:"#);
        // `mary ann` is quoted text inside the person slot? No — the quotes
        // bind to the value, so this is one person filter…
        assert_eq!(parsed.person.as_deref(), Some("mary ann"));
        // …while the unknown field and the empty value degrade to text.
        assert_eq!(parsed.text, "bogus:x person:");
        // A quoted non-filter loses its quotes but keeps its words.
        assert_eq!(parse_field_filters(r#""sunset beach""#).text, "sunset beach");
        // A quoted date filter still parses.
        assert_eq!(
            parse_field_filters(r#"after:"2022-06-01""#).after.as_deref(),
            Some("2022-06-01")
        );
    }

    /// Malformed dates degrade to text, valid bounds normalize: a bare
    /// year expands to January 1st, `before` exclusive, `after` inclusive.
    #[test]
    fn date_bounds_normalize_or_degrade() {
        assert_eq!(parse_field_filters("before:2023").before.as_deref(), Some("2023-01-01"));
        assert_eq!(parse_field_filters("after:2023-12-31").after.as_deref(), Some("2023-12-31"));
        let parsed = parse_field_filters("before:notadate after:2023-13-01 before:23");
        assert_eq!(parsed.text, "before:notadate after:2023-13-01 before:23");
        assert_eq!(parsed.before, None);
    }

    #[test]
    fn empty_input_is_an_empty_query() {
        assert_eq!(parse_field_filters(""), ParsedQuery::default());
        assert_eq!(parse_field_filters("   "), ParsedQuery::default());
        assert_eq!(parse_field_filters("just text").text, "just text");
    }

    /// A later filter of the same kind replaces the earlier one.
    #[test]
    fn repeated_filters_take_the_last_value() {
        assert_eq!(parse_field_filters("person:a person:b").person.as_deref(), Some("b"));
    }
}
