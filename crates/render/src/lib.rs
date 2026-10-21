//! Decode (libvips, LibRaw, libheif), recipe ops, thumbnails (PLAN §3).

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase-render";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_identity() {
        assert_eq!(super::CRATE_NAME, "fotobase-render");
    }
}
