//! Fotobase domain types and edit recipe model (PLAN §3).

pub mod apply;
pub mod buffer;
pub mod recipe;

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase-core";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_identity() {
        assert_eq!(super::CRATE_NAME, "fotobase-core");
    }
}
