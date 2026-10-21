//! Bounded priority job queue: visible items first, background last,
//! throttling hook, cancellation (PLAN §3 rule 4).

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase-jobs";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_identity() {
        assert_eq!(super::CRATE_NAME, "fotobase-jobs");
    }
}
