//! EXIF/IPTC/XMP read/write via exiv2 bindings (PLAN §3).
//!
//! All metadata writes must follow `docs/sidecar-merge-on-write.md`:
//! read-modify-write that preserves unknown nodes verbatim.

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase-metadata";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_identity() {
        assert_eq!(super::CRATE_NAME, "fotobase-metadata");
    }
}
