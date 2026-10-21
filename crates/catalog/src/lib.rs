//! SQLite catalog, FTS5 + sqlite-vec (PLAN §3). The catalog is a rebuildable
//! cache; XMP sidecars remain the source of truth.

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase-catalog";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_identity() {
        assert_eq!(super::CRATE_NAME, "fotobase-catalog");
    }
}
