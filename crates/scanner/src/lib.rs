//! Folder walk, `notify` watcher, import (PLAN §3).

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase-scanner";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_identity() {
        assert_eq!(super::CRATE_NAME, "fotobase-scanner");
    }
}
