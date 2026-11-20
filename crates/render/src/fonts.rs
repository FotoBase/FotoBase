//! System-font lookup for the recipe `text` op (TASK-08-003). The engine
//! (`fotobase_core::text`) bundles the generic families `sans`, `serif` and
//! `mono`; any other family name is resolved here against the fonts the OS
//! has installed. Unknown families fall back to the bundled default inside
//! the engine, so a recipe made on another machine still renders.

use std::sync::OnceLock;

use fotobase_core::text::{set_system_font_resolver, SystemFont};

/// Lazily scanned: reading the system font directories costs tens of
/// milliseconds, so it only happens when a recipe asks for a non-bundled
/// family.
fn database() -> &'static fontdb::Database {
    static DB: OnceLock<fontdb::Database> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db
    })
}

/// Look up an installed font family by name (regular weight and style).
pub fn find_system_font(family: &str) -> Option<SystemFont> {
    let db = database();
    let id = db.query(&fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        ..fontdb::Query::default()
    })?;
    db.with_face_data(id, |data, collection_index| SystemFont {
        data: data.to_vec(),
        collection_index,
    })
}

/// Let recipes name installed fonts. Call once at app start.
pub fn install_system_fonts() {
    set_system_font_resolver(Box::new(find_system_font));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_family_is_not_found() {
        assert!(find_system_font("No Such Font 9000 Fotobase").is_none());
    }
}
