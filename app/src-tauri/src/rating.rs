//! Star / 1–5 rating commands' logic (TASK-03-005, ADR-0002, PLAN §5).
//!
//! Star-only is the default: a star is `xmp:Rating` = 5, unstar removes the
//! node. With ratings enabled 1–5 are selectable and "starred" means any
//! rating. Both write the same field through the sidecar sync engine, so the
//! sidecar write always precedes the catalog update.

use fotobase_catalog::{Catalog, SyncError};

/// Rating value of a star in star-only mode.
pub const STAR: u8 = 5;

/// Lowest rating the starred filter accepts: 5 in star-only mode (a 3 from
/// another tool is not a star), 1 when ratings are enabled.
pub fn starred_threshold(ratings_enabled: bool) -> u8 {
    if ratings_enabled {
        1
    } else {
        STAR
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RatingError {
    #[error("only star (5) or unstar (0) is allowed while ratings are disabled, got {0}")]
    RatingsDisabled(u8),
    #[error(transparent)]
    Sync(#[from] SyncError),
}

/// Apply `rating` (0 = unstar) to one photo: write-through to its sidecar,
/// then the catalog mirror. Originals are never touched.
pub fn apply_rating(
    cat: &mut Catalog,
    file_id: i64,
    rating: u8,
    ratings_enabled: bool,
) -> Result<(), RatingError> {
    if !ratings_enabled && rating != 0 && rating != STAR {
        return Err(RatingError::RatingsDisabled(rating));
    }
    if rating == 0 {
        cat.clear_rating(file_id)?;
    } else {
        cat.set_rating(file_id, rating)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fotobase_metadata::Sidecar;

    fn catalog_with_photo(dir: &std::path::Path) -> (Catalog, i64, std::path::PathBuf) {
        let mut cat = Catalog::open(dir.join("catalog.db")).unwrap();
        let original = dir.join("a.jpg");
        std::fs::write(&original, b"bytes").unwrap();
        let folder = cat.seed_folder(dir.to_str().unwrap()).unwrap();
        cat.conn()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
                 VALUES (?1, ?2, 'a.jpg', 5, 0.0, 'image')",
                rusqlite::params![folder, original.to_str().unwrap()],
            )
            .unwrap();
        let id = cat.conn().last_insert_rowid();
        (cat, id, original)
    }

    #[test]
    fn threshold_is_5_for_star_only_and_1_with_ratings() {
        assert_eq!(starred_threshold(false), 5);
        assert_eq!(starred_threshold(true), 1);
    }

    #[test]
    fn star_toggle_writes_5_then_unstar_clears() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cat, id, original) = catalog_with_photo(dir.path());
        let sc = fotobase_catalog::sidecar_path(original.to_str().unwrap());

        apply_rating(&mut cat, id, STAR, false).unwrap();
        assert_eq!(Sidecar::read(&sc).unwrap().rating(), Some(5));

        apply_rating(&mut cat, id, 0, false).unwrap();
        assert_eq!(Sidecar::read(&sc).unwrap().rating(), None);
    }

    #[test]
    fn star_only_mode_rejects_1_to_4_without_touching_the_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cat, id, original) = catalog_with_photo(dir.path());
        let err = apply_rating(&mut cat, id, 3, false).unwrap_err();
        assert!(matches!(err, RatingError::RatingsDisabled(3)));
        assert!(!fotobase_catalog::sidecar_path(original.to_str().unwrap()).exists());
    }

    #[test]
    fn ratings_mode_accepts_1_to_5() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cat, id, original) = catalog_with_photo(dir.path());
        apply_rating(&mut cat, id, 3, true).unwrap();
        let sc = fotobase_catalog::sidecar_path(original.to_str().unwrap());
        assert_eq!(Sidecar::read(&sc).unwrap().rating(), Some(3));
    }
}
