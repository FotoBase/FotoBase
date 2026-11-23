//! Data-driven gallery templates (TASK-08-016).
//!
//! A [`Template`] is plain data: an index page, a per-photo fragment and a
//! set of static assets. Rendering is a tiny string substitution, not a
//! template engine: `{{name}}` inserts the HTML-escaped value, `{{{name}}}`
//! inserts it verbatim. An unknown placeholder is an error, so a broken
//! theme fails loudly instead of producing a half-rendered site. F5 themes
//! plug in by constructing a [`Template`] from their own files.
//!
//! Placeholders available in `index`: `title`, `description`, `count`,
//! `items` (raw, the rendered `item` fragments) and `data` (raw, the viewer
//! JSON, safe to embed in a `<script type="application/json">` element).
//!
//! Placeholders available in `item`: `index`, `image`, `thumb`,
//! `thumb_width`, `thumb_height`, `width`, `height`, `title`, `caption`,
//! `alt`. File references are relative to the site root.

use super::GalleryError;

/// A static file shipped verbatim with a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateAsset {
    /// Path below the site root (`/`-separated, no `..`).
    pub path: String,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub id: String,
    pub name: String,
    /// `index.html` source.
    pub index: String,
    /// Fragment rendered once per photo and substituted into `{{{items}}}`.
    pub item: String,
    pub assets: Vec<TemplateAsset>,
}

const ITEM: &str = include_str!("templates/shared/item.html");
const VIEWER_JS: &str = include_str!("templates/shared/viewer.js");
const LIGHTBOX_CSS: &str = include_str!("templates/shared/lightbox.css");

fn builtin(id: &str, name: &str, index: &str, style: &str) -> Template {
    let asset = |path: &str, contents: &str| TemplateAsset {
        path: path.to_owned(),
        contents: contents.to_owned(),
    };
    Template {
        id: id.to_owned(),
        name: name.to_owned(),
        index: index.to_owned(),
        item: ITEM.to_owned(),
        assets: vec![
            asset("style.css", style),
            asset("lightbox.css", LIGHTBOX_CSS),
            asset("viewer.js", VIEWER_JS),
        ],
    }
}

/// The templates that ship with Fotobase: `grid` (light thumbnail grid) and
/// `dark` (dark masonry wall).
pub fn builtin_templates() -> Vec<Template> {
    vec![
        builtin(
            "grid",
            "Grid",
            include_str!("templates/grid/index.html"),
            include_str!("templates/grid/style.css"),
        ),
        builtin(
            "dark",
            "Dark wall",
            include_str!("templates/dark/index.html"),
            include_str!("templates/dark/style.css"),
        ),
    ]
}

pub fn builtin_template(id: &str) -> Option<Template> {
    builtin_templates().into_iter().find(|t| t.id == id)
}

/// Escapes text for HTML element content and quoted attribute values.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Substitutes `{{name}}` (escaped) and `{{{name}}}` (raw) from `vars`.
pub fn render(source: &str, vars: &[(&str, &str)]) -> Result<String, GalleryError> {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(at) = rest.find("{{") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let (raw, close) = if after.starts_with('{') { (true, "}}}") } else { (false, "}}") };
        let body = if raw { &after[1..] } else { after };
        let end = body.find(close).ok_or_else(|| {
            GalleryError::Template(format!("unterminated placeholder near {:?}", clip(after)))
        })?;
        let name = body[..end].trim();
        let value = vars
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| *v)
            .ok_or_else(|| GalleryError::Template(format!("unknown placeholder `{name}`")))?;
        if raw {
            out.push_str(value);
        } else {
            out.push_str(&escape_html(value));
        }
        rest = &body[end + close.len()..];
    }
    out.push_str(rest);
    Ok(out)
}

fn clip(text: &str) -> String {
    text.chars().take(24).collect()
}

/// Rejects asset paths that could escape the destination.
pub(super) fn validate_asset_path(path: &str) -> Result<(), GalleryError> {
    let bad = path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path.split('/').any(|c| c.is_empty() || c == "." || c == "..");
    if bad {
        return Err(GalleryError::Template(format!("invalid asset path `{path}`")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_and_raw_placeholders() {
        let html = render(
            "<p>{{caption}}</p>{{{items}}}",
            &[("caption", "a <b> & \"c\""), ("items", "<li>x</li>")],
        )
        .unwrap();
        assert_eq!(html, "<p>a &lt;b&gt; &amp; &quot;c&quot;</p><li>x</li>");
    }

    #[test]
    fn whitespace_inside_braces_is_ignored() {
        assert_eq!(render("{{ title }}", &[("title", "T")]).unwrap(), "T");
    }

    #[test]
    fn unknown_and_unterminated_placeholders_are_errors() {
        assert!(matches!(render("{{nope}}", &[]), Err(GalleryError::Template(_))));
        assert!(matches!(render("{{title", &[("title", "")]), Err(GalleryError::Template(_))));
        assert!(matches!(render("{{{title}}", &[("title", "")]), Err(GalleryError::Template(_))));
    }

    #[test]
    fn braces_without_placeholder_pass_through() {
        assert_eq!(render("a { b } c", &[]).unwrap(), "a { b } c");
    }

    #[test]
    fn asset_paths_cannot_escape() {
        assert!(validate_asset_path("css/theme.css").is_ok());
        for bad in ["", "/etc/passwd", "../x", "a/../x", "a\\b", "C:x", "a//b"] {
            assert!(validate_asset_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn builtin_templates_only_use_known_placeholders() {
        let item_vars: Vec<(&str, &str)> = [
            "index",
            "image",
            "thumb",
            "thumb_width",
            "thumb_height",
            "width",
            "height",
            "title",
            "caption",
            "alt",
        ]
        .iter()
        .map(|k| (*k, ""))
        .collect();
        let index_vars =
            [("title", ""), ("description", ""), ("count", ""), ("items", ""), ("data", "")];
        for t in builtin_templates() {
            render(&t.item, &item_vars).unwrap();
            render(&t.index, &index_vars).unwrap();
        }
    }
}
