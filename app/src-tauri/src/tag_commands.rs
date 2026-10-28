//! Tag commands' logic (TASK-03-006, PLAN §2B/§5). The catalog's sync engine
//! owns the write-through (sidecar first, catalog mirror second); these
//! helpers only add the app-facing shapes: per-photo tag lists and the
//! library-wide rename.

use fotobase_catalog::{Catalog, SyncError};

/// One tag with its usage count (`list_tags` row).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TagCount {
    pub name: String,
    pub count: i64,
}

pub fn tags(cat: &Catalog, file_id: i64) -> Result<Vec<String>, SyncError> {
    cat.file_tags(file_id).map_err(SyncError::from)
}

pub fn add(cat: &mut Catalog, file_id: i64, tag: &str) -> Result<Vec<String>, SyncError> {
    cat.add_tag(file_id, tag)
}

pub fn remove(cat: &mut Catalog, file_id: i64, tag: &str) -> Result<Vec<String>, SyncError> {
    cat.remove_tag(file_id, tag)
}

pub fn rename(cat: &mut Catalog, old: &str, new: &str) -> Result<usize, SyncError> {
    cat.rename_tag(old, new)
}

pub fn all(cat: &Catalog) -> Result<Vec<TagCount>, SyncError> {
    Ok(cat
        .all_tags()
        .map_err(SyncError::from)?
        .into_iter()
        .map(|(name, count)| TagCount { name, count })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fotobase_metadata::Sidecar;

    fn catalog_with_photo(dir: &std::path::Path) -> (i64, Catalog, std::path::PathBuf) {
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
        (cat.conn().last_insert_rowid(), cat, original)
    }

    #[test]
    fn add_remove_round_trip_through_the_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let (id, mut cat, original) = catalog_with_photo(dir.path());

        add(&mut cat, id, "sea").unwrap();
        let sc =
            Sidecar::read(&fotobase_catalog::sidecar_path(original.to_str().unwrap())).unwrap();
        assert_eq!(sc.tags(), vec!["sea"]);

        let remaining = remove(&mut cat, id, "sea").unwrap();
        assert!(remaining.is_empty());
        assert!(tags(&cat, id).unwrap().is_empty());
    }

    #[test]
    fn rename_reports_the_number_of_touched_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        let (id, mut cat, _) = catalog_with_photo(dir.path());
        add(&mut cat, id, "sea").unwrap();

        assert_eq!(rename(&mut cat, "sea", "ocean").unwrap(), 1);
        assert_eq!(all(&cat).unwrap(), vec![TagCount { name: "ocean".into(), count: 1 }]);
    }

    #[test]
    fn empty_tag_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (id, mut cat, _) = catalog_with_photo(dir.path());
        assert!(add(&mut cat, id, "   ").is_err());
    }
}
