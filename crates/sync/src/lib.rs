//! Device sync, layer 2 of PLAN §7 F1: user metadata as a CRDT document.
//!
//! Stars, tags, captions, albums, confirmed face names and edit recipes of
//! every photo live in one [Loro](https://loro.dev) document per library,
//! so concurrent edits from several devices merge without loss. Originals
//! never enter the document — metadata only.
//!
//! The XMP sidecar stays the on-disk truth for single-machine state
//! (PLAN §3): [`PhotoMeta::from_sidecar`] reads the syncable fields out of a
//! sidecar, [`MetadataDoc::update_photo`] records local changes, and
//! [`PhotoMeta::apply_to_sidecar`] writes merged state back through the
//! merge-on-write [`Sidecar`] API (docs/sidecar-merge-on-write.md).
//!
//! Layout: each photo owns a handful of *root* containers named after its
//! sync key. Root containers are identified by name, so two devices touching
//! the same photo for the first time write into the same container instead
//! of racing to create a child container (where Loro would keep only one).
//! Scalars (rating, caption, recipe) are last-writer-wins registers ordered
//! by Lamport time then peer id — deterministic on every replica. Tags,
//! albums and faces are add/remove sets keyed per member, so concurrent
//! additions on different devices all survive.

use std::collections::BTreeSet;

use fotobase_metadata::{FaceRegion, Sidecar};

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase-sync";

/// Errors from encoding, decoding or addressing the sync document.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("invalid photo sync key {0:?}")]
    InvalidKey(String),
    #[error("could not encode sync document: {0}")]
    Encode(String),
    #[error("could not import sync data: {0}")]
    Import(String),
}

/// The syncable metadata of one photo (PLAN §7 F1): everything else in a
/// sidecar — GPS, capture time, stacks, hidden flag, unknown nodes — stays
/// per device.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhotoMeta {
    pub rating: Option<u8>,
    pub caption: Option<String>,
    pub tags: BTreeSet<String>,
    /// Album membership UUIDs (`fb:Albums`).
    pub albums: BTreeSet<String>,
    /// Confirmed, named faces only — build with [`syncable_faces`].
    pub faces: Vec<FaceRegion>,
    /// Raw `fb:Recipe` JSON, uninterpreted.
    pub recipe: Option<String>,
}

/// A face as the catalog knows it: detected, optionally named, optionally
/// confirmed by the user.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedFace {
    pub region: FaceRegion,
    pub confirmed: bool,
}

/// Keep only faces that may leave the device: confirmed and named.
/// Unnamed clusters and unconfirmed suggestions are recomputed per device.
pub fn syncable_faces(faces: impl IntoIterator<Item = DetectedFace>) -> Vec<FaceRegion> {
    faces
        .into_iter()
        .filter(|f| f.confirmed && !f.region.name.trim().is_empty())
        .map(|f| f.region)
        .collect()
}

fn named(faces: Vec<FaceRegion>) -> Vec<FaceRegion> {
    faces.into_iter().filter(|f| !f.name.trim().is_empty()).collect()
}

/// Set of `items` in `current`'s order, then the new members sorted: a
/// rewrite keeps the sidecar's existing order stable.
fn ordered(current: &[String], target: &BTreeSet<String>) -> Vec<String> {
    let mut out: Vec<String> = current.iter().filter(|t| target.contains(*t)).cloned().collect();
    let mut seen: BTreeSet<&String> = out.iter().collect();
    let fresh: Vec<String> = target.iter().filter(|t| seen.insert(t)).cloned().collect();
    out.extend(fresh);
    out
}

fn same_recipe(a: &str, b: &str) -> bool {
    match (
        serde_json::from_str::<serde_json::Value>(a),
        serde_json::from_str::<serde_json::Value>(b),
    ) {
        (Ok(x), Ok(y)) => x == y,
        _ => a.trim() == b.trim(),
    }
}

impl PhotoMeta {
    /// Read the syncable fields from a sidecar. Sidecars hold confirmed
    /// faces only; blank-named regions are dropped all the same.
    pub fn from_sidecar(sidecar: &Sidecar) -> Self {
        Self {
            rating: sidecar.rating(),
            caption: sidecar.caption(),
            tags: sidecar.tags().into_iter().collect(),
            albums: sidecar.albums().into_iter().collect(),
            faces: named(sidecar.face_regions()),
            recipe: sidecar.recipe(),
        }
    }

    /// Bring a sidecar's syncable fields in line with `self` through the
    /// merge-on-write setters; unchanged fields are not touched. Returns
    /// whether anything changed (the caller then writes the sidecar).
    pub fn apply_to_sidecar(&self, sidecar: &mut Sidecar) -> bool {
        let mut changed = false;

        if sidecar.rating() != self.rating {
            match self.rating {
                Some(r) => sidecar.set_rating(r),
                None => sidecar.clear_rating(),
            }
            changed = true;
        }
        if sidecar.caption() != self.caption {
            match &self.caption {
                Some(c) => sidecar.set_caption(c),
                None => sidecar.clear_caption(),
            }
            changed = true;
        }
        let tags = sidecar.tags();
        if tags.iter().cloned().collect::<BTreeSet<_>>() != self.tags {
            sidecar.set_tags(ordered(&tags, &self.tags));
            changed = true;
        }
        let albums = sidecar.albums();
        if albums.iter().cloned().collect::<BTreeSet<_>>() != self.albums {
            sidecar.set_albums(ordered(&albums, &self.albums));
            changed = true;
        }
        let recipe_differs = match (sidecar.recipe(), &self.recipe) {
            (Some(a), Some(b)) => !same_recipe(&a, b),
            (None, None) => false,
            _ => true,
        };
        if recipe_differs {
            match &self.recipe {
                Some(r) => sidecar.set_recipe(r),
                None => sidecar.clear_recipe(),
            }
            changed = true;
        }
        changed |= self.apply_faces(sidecar);
        changed
    }

    fn apply_faces(&self, sidecar: &mut Sidecar) -> bool {
        let current = sidecar.face_regions();
        let (current_named, blank): (Vec<_>, Vec<_>) =
            current.into_iter().partition(|f| !f.name.trim().is_empty());
        let missing: Vec<FaceRegion> = self
            .faces
            .iter()
            .filter(|t| !current_named.iter().any(|c| c.same_face(t)))
            .cloned()
            .collect();
        let stale = current_named.iter().any(|c| !self.faces.iter().any(|t| t.same_face(c)));
        if !stale {
            // Additions only: merge in place, every existing region verbatim.
            return !missing.is_empty()
                && sidecar.add_face_regions(&missing, None).is_some_and(|n| n > 0);
        }
        // A face left or was renamed: rebuild the face set. Non-face
        // regions and unknown nodes stay verbatim; an unrecognised
        // `Regions` structure is never rewritten.
        if sidecar.remove_face_regions().is_none() {
            return false;
        }
        let mut keep = self.faces.clone();
        keep.extend(blank);
        if !keep.is_empty() {
            sidecar.add_face_regions(&keep, None);
        }
        true
    }
}

// Root container names. Every container of a photo is a root, named after
// its key, so first edits on two devices land in the same container.
const PHOTOS: &str = "photos";
const RATING: &str = "rating";
const CAPTION: &str = "caption";
const RECIPE: &str = "recipe";

fn scalars(key: &str) -> String {
    format!("p:{key}")
}
fn tags_of(key: &str) -> String {
    format!("t:{key}")
}
fn albums_of(key: &str) -> String {
    format!("a:{key}")
}
fn faces_of(key: &str) -> String {
    format!("f:{key}")
}

/// Identity of a face inside the faces set: the name as `same_face`
/// compares it plus the box at 4 decimals.
fn face_key(f: &FaceRegion) -> String {
    format!("{}|{:.4}|{:.4}|{:.4}|{:.4}", f.name.trim().to_lowercase(), f.x, f.y, f.w, f.h)
}

fn face_json(f: &FaceRegion) -> String {
    serde_json::json!({ "name": f.name, "x": f.x, "y": f.y, "w": f.w, "h": f.h }).to_string()
}

fn face_from_json(text: &str) -> Option<FaceRegion> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    Some(FaceRegion {
        name: v.get("name")?.as_str()?.to_string(),
        x: v.get("x")?.as_f64()?,
        y: v.get("y")?.as_f64()?,
        w: v.get("w")?.as_f64()?,
        h: v.get("h")?.as_f64()?,
    })
}

fn string_of(v: loro::ValueOrContainer) -> Option<String> {
    match v {
        loro::ValueOrContainer::Value(loro::LoroValue::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

fn check_key(key: &str) -> Result<(), SyncError> {
    // Loro root names reject '/' and NUL; the prefixes keep the rest valid.
    if key.trim().is_empty() || key.contains(['/', '\0']) {
        return Err(SyncError::InvalidKey(key.to_string()));
    }
    Ok(())
}

fn loro_err(e: impl std::fmt::Display) -> SyncError {
    SyncError::Encode(e.to_string())
}

/// One library's syncable metadata as a Loro CRDT document.
pub struct MetadataDoc {
    doc: loro::LoroDoc,
}

impl MetadataDoc {
    /// A fresh document with a random peer id.
    pub fn new() -> Self {
        Self { doc: loro::LoroDoc::new() }
    }

    /// A fresh document with a fixed peer id (tests, reproducible merges).
    pub fn with_peer(peer: u64) -> Self {
        let doc = loro::LoroDoc::new();
        doc.set_peer_id(peer).expect("fresh document accepts a peer id");
        Self { doc }
    }

    /// Record `meta` as the current state of `key`, writing only the fields
    /// and set members that differ from the document. Untouched fields emit
    /// no operations, so they never override a concurrent remote edit.
    /// Returns whether anything changed.
    pub fn update_photo(&mut self, key: &str, meta: &PhotoMeta) -> Result<bool, SyncError> {
        check_key(key)?;
        let before = self.photo(key);
        let old = before.clone().unwrap_or_default();
        let mut changed = false;

        if before.is_none() {
            self.doc.get_map(PHOTOS).insert(key, true).map_err(loro_err)?;
            changed = true;
        }
        let p = self.doc.get_map(scalars(key).as_str());
        if old.rating != meta.rating {
            match meta.rating {
                Some(r) => p.insert(RATING, i64::from(r)),
                None => p.delete(RATING),
            }
            .map_err(loro_err)?;
            changed = true;
        }
        for (field, was, now) in
            [(CAPTION, &old.caption, &meta.caption), (RECIPE, &old.recipe, &meta.recipe)]
        {
            if was != now {
                match now {
                    Some(v) => p.insert(field, v.as_str()),
                    None => p.delete(field),
                }
                .map_err(loro_err)?;
                changed = true;
            }
        }
        for (name, was, now) in
            [(tags_of(key), &old.tags, &meta.tags), (albums_of(key), &old.albums, &meta.albums)]
        {
            let m = self.doc.get_map(name.as_str());
            for gone in was.difference(now) {
                m.delete(gone).map_err(loro_err)?;
                changed = true;
            }
            for added in now.difference(was) {
                m.insert(added, true).map_err(loro_err)?;
                changed = true;
            }
        }
        let f = self.doc.get_map(faces_of(key).as_str());
        let was: BTreeSet<String> = old.faces.iter().map(face_key).collect();
        let now: BTreeSet<String> = meta.faces.iter().map(face_key).collect();
        for gone in was.difference(&now) {
            f.delete(gone).map_err(loro_err)?;
            changed = true;
        }
        for face in &meta.faces {
            let k = face_key(face);
            if !was.contains(&k) {
                f.insert(&k, face_json(face)).map_err(loro_err)?;
                changed = true;
            }
        }
        self.doc.commit();
        Ok(changed)
    }

    /// The merged state of `key`, `None` if the document never saw it.
    pub fn photo(&self, key: &str) -> Option<PhotoMeta> {
        check_key(key).ok()?;
        self.doc.get_map(PHOTOS).get(key)?;
        let p = self.doc.get_map(scalars(key).as_str());
        let rating = match p.get(RATING) {
            Some(loro::ValueOrContainer::Value(loro::LoroValue::I64(r))) => u8::try_from(r).ok(),
            _ => None,
        };
        let set = |name: String| -> BTreeSet<String> {
            self.doc.get_map(name.as_str()).keys().map(|k| k.to_string()).collect()
        };
        let faces_map = self.doc.get_map(faces_of(key).as_str());
        let mut face_keys: Vec<String> = faces_map.keys().map(|k| k.to_string()).collect();
        face_keys.sort_unstable();
        let faces = face_keys
            .iter()
            .filter_map(|k| faces_map.get(k).and_then(string_of))
            .filter_map(|j| face_from_json(&j))
            .collect();
        Some(PhotoMeta {
            rating,
            caption: p.get(CAPTION).and_then(string_of),
            tags: set(tags_of(key)),
            albums: set(albums_of(key)),
            faces,
            recipe: p.get(RECIPE).and_then(string_of),
        })
    }

    /// Every photo key the document knows, sorted.
    pub fn photos(&self) -> Vec<String> {
        let mut keys: Vec<String> =
            self.doc.get_map(PHOTOS).keys().map(|k| k.to_string()).collect();
        keys.sort_unstable();
        keys
    }

    /// The whole document, for a first sync or persistence.
    pub fn export_snapshot(&self) -> Result<Vec<u8>, SyncError> {
        self.doc.export(loro::ExportMode::Snapshot).map_err(loro_err)
    }

    /// Merge a snapshot or update blob from another replica.
    pub fn import(&mut self, bytes: &[u8]) -> Result<(), SyncError> {
        self.doc.import(bytes).map(|_| ()).map_err(|e| SyncError::Import(e.to_string()))
    }
}

impl Default for MetadataDoc {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_identity() {
        assert_eq!(super::CRATE_NAME, "fotobase-sync");
    }
}
