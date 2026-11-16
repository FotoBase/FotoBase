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

// ---- auto-tagging suggestions (TASK-07-004, PLAN §6 P3) ------------------

/// One (concept, prompt-embedding) pair.
pub type ConceptVector = (String, Vec<f32>);

/// The prompt-embedding cache: the vocabulary's prompts are embedded once
/// per process (a text-tower inference per concept is expensive); the
/// vectors are derived data, never persisted. Vocabulary is fixed per
/// release, so one slot suffices.
#[derive(Default)]
pub struct PromptCache(std::sync::Mutex<Option<Vec<ConceptVector>>>);

impl PromptCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// The cached (concept, vector) list, or `encode`'s result (which then
    /// becomes the cache content). An encode failure leaves the cache alone.
    pub fn get_or_encode(
        &self,
        mut encode: impl FnMut() -> Result<Vec<ConceptVector>, String>,
    ) -> Result<Vec<ConceptVector>, String> {
        if let Ok(guard) = self.0.lock() {
            if let Some(cached) = guard.as_ref() {
                return Ok(cached.clone());
            }
        }
        let prompts = encode()?;
        if let Ok(mut guard) = self.0.lock() {
            *guard = Some(prompts.clone());
        }
        Ok(prompts)
    }
}

/// One photo's stored suggestions (catalog only, no sidecar read).
pub fn suggestions(cat: &Catalog, file_id: i64) -> Result<Vec<String>, SyncError> {
    cat.file_suggestions(file_id).map_err(SyncError::from)
}

/// Confirm one suggestion: ordinary tag via the sidecar write-through.
pub fn confirm(cat: &mut Catalog, file_id: i64, tag: &str) -> Result<Vec<String>, SyncError> {
    cat.confirm_tag_suggestion(file_id, tag)
}

/// Reject one suggestion: catalog row only, sidecar untouched.
pub fn reject(cat: &mut Catalog, file_id: i64, tag: &str) -> Result<bool, SyncError> {
    cat.reject_tag_suggestion(file_id, tag)
}

/// Generate the auto-tag suggestions for one photo from its stored CLIP
/// embedding (TASK-07-004 technical spec): rank the vocabulary prompt
/// embeddings by cosine distance and store the hits as suggested rows.
/// Returns the stored suggestion list. Errors when the photo has no
/// embedding yet (the 07-001 pass has not reached it) or the text engine
/// fails — suggestion generation only runs after the model opt-in.
pub fn suggest(
    cat: &mut Catalog,
    file_id: i64,
    engine: &mut dyn crate::clip_jobs::TextEngine,
    cache: &PromptCache,
) -> Result<Vec<String>, String> {
    let image =
        cat.photo_embedding(file_id).map_err(|e| e.to_string())?.ok_or("no-embedding-yet")?;
    let vocab = fotobase_ai::Vocabulary::from_json(fotobase_ai::VOCABULARY_JSON)
        .map_err(|e| format!("vocabulary: {e}"))?;
    let prompts = cache.get_or_encode(|| {
        vocab
            .prompts()
            .into_iter()
            .map(|(concept, prompt)| {
                let v = engine.embed_text(&prompt)?;
                Ok((concept, v))
            })
            .collect()
    })?;
    let hits = fotobase_ai::extract_keywords(&image, &prompts, vocab.threshold);
    let keywords: Vec<String> = hits.iter().map(|h| h.concept.clone()).collect();
    cat.store_suggestions(file_id, &keywords).map_err(|e| e.to_string())?;
    suggestions(cat, file_id).map_err(|e| e.to_string())
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

    // ---- auto-tagging suggestions (TASK-07-004, PLAN §6 P3) --------------

    /// A text engine whose prompt embeddings cluster per concept: the
    /// concept's prompt embeds to a seed-offset unit vector, so the tests
    /// control which vocabulary concepts land near an image embedding.
    struct FakeEngine {
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    fn unit(seed: f32) -> Vec<f32> {
        let mut v: Vec<f32> =
            (0..fotobase_ai::CLIP_EMBEDDING_DIM).map(|i| (i as f32 + seed).sin()).collect();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        for x in &mut v {
            *x /= norm;
        }
        v
    }

    fn seed_for(concept: &str) -> f32 {
        concept.bytes().map(|b| b as f32).sum::<f32>() / 8.0
    }

    impl crate::clip_jobs::TextEngine for FakeEngine {
        fn embed_text(&mut self, text: &str) -> Result<Vec<f32>, String> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let concept = text
                .strip_prefix("a photo of ")
                .ok_or_else(|| format!("unexpected prompt {text}"))?;
            Ok(unit(seed_for(concept)))
        }
    }

    #[test]
    fn suggest_ranks_vocabulary_and_stores_derived_rows() {
        let dir = tempfile::tempdir().unwrap();
        let (id, mut cat, sc) = catalog_with_photo(dir.path());
        // The image embedding sits next to the "dog" prompt embedding.
        cat.store_photo_embedding(id, &unit(seed_for("dog") + 0.05)).unwrap();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut engine = FakeEngine { calls: calls.clone() };
        let cache = PromptCache::new();

        let out = suggest(&mut cat, id, &mut engine, &cache).unwrap();
        assert!(out.contains(&"dog".to_string()), "dog appears among suggestions: {out:?}");
        assert!(
            !fotobase_catalog::sidecar_path(sc.to_str().unwrap()).exists(),
            "generation must never create a sidecar"
        );
        assert!(cat.file_tags(id).unwrap().is_empty(), "suggestions are not tags yet");

        // The prompt cache: a second photo needs no re-encoding.
        let before = calls.load(std::sync::atomic::Ordering::Relaxed);
        let folder_id: i64 =
            cat.conn().query_row("SELECT id FROM folders LIMIT 1", [], |r| r.get(0)).unwrap();
        let path_b = dir.path().join("b.jpg");
        std::fs::write(&path_b, b"bytes").unwrap();
        cat.conn()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
                 VALUES (?1, ?2, 'b.jpg', 5, 0.0, 'image')",
                rusqlite::params![folder_id, path_b.to_str().unwrap()],
            )
            .unwrap();
        let id2 = cat.conn().last_insert_rowid();
        cat.store_photo_embedding(id2, &unit(seed_for("cat") + 0.05)).unwrap();
        let out2 = suggest(&mut cat, id2, &mut engine, &cache).unwrap();
        assert!(out2.contains(&"cat".to_string()));
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::Relaxed),
            before,
            "vocabulary prompts were cached"
        );
    }

    #[test]
    fn suggest_errors_without_a_stored_embedding() {
        let dir = tempfile::tempdir().unwrap();
        let (id, mut cat, _) = catalog_with_photo(dir.path());
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut engine = FakeEngine { calls };
        assert_eq!(
            suggest(&mut cat, id, &mut engine, &PromptCache::new()).unwrap_err(),
            "no-embedding-yet"
        );
    }

    #[test]
    fn confirm_moves_the_suggestion_into_dc_subject_reject_leaves_the_sidecar_alone() {
        let dir = tempfile::tempdir().unwrap();
        let (id, mut cat, sc) = catalog_with_photo(dir.path());
        store_suggestions_ok(&mut cat, id, &["dog", "beach"]);

        // Reject: no sidecar, suggestion gone.
        assert!(reject(&mut cat, id, "beach").unwrap());
        assert!(
            !fotobase_catalog::sidecar_path(sc.to_str().unwrap()).exists(),
            "rejection never writes a sidecar"
        );

        // Confirm: ordinary tag through the single write-through pathway.
        let tags = confirm(&mut cat, id, "dog").unwrap();
        assert!(tags.contains(&"dog".to_string()));
        let sidecar_tags =
            fotobase_metadata::Sidecar::read(&fotobase_catalog::sidecar_path(sc.to_str().unwrap()))
                .unwrap()
                .tags();
        assert!(sidecar_tags.contains(&"dog".to_string()), "dc:subject written");
        assert!(suggestions(&cat, id).unwrap().is_empty());
    }

    fn store_suggestions_ok(cat: &mut fotobase_catalog::Catalog, id: i64, tags: &[&str]) {
        let owned: Vec<String> = tags.iter().map(|t| t.to_string()).collect();
        cat.store_suggestions(id, &owned).unwrap();
    }
}
