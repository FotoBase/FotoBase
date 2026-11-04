//! TASK-05-004: name-a-cluster flow with suggestions (PLAN §2G Naming,
//! §6 P3 People). Naming a cluster turns it into a `persons` row — the
//! naming layer over derived cluster data (§5: persons are user data, the
//! cluster partition is not). Suggestions come from cosine similarity
//! between the named cluster's centroid and every still-unnamed cluster's
//! centroid; confirming a suggestion stamps the face with the person and
//! writes the name into the sidecar's `mwg-rs:Regions` via
//! `crates/metadata` (docs/sidecar-merge-on-write.md — sidecar first,
//! catalog mirror second).

use rusqlite::OptionalExtension;

use fotobase_metadata::FaceRegion;

use crate::sync::SyncResult;
use crate::{Catalog, CatalogError, Result};

/// The catalog stores detected faces in *pixel* coordinates; MWG regions
/// are normalized centre-point + size (docs/sidecar-merge-on-write.md §2).
/// Convert with the photo's dimensions — the `files` row first, an image
/// probe as fallback (the folder-ini precedent). Returns the region and
/// the pixel dimensions for the region's `AppliedToDimensions`.
fn region_for(
    cat: &Catalog,
    file_id: i64,
    name: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> SyncResult<(FaceRegion, (u32, u32))> {
    let (path, fw, fh): (String, Option<i64>, Option<i64>) = cat
        .conn()
        .query_row("SELECT path, width, height FROM files WHERE id = ?1", [file_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?
        .ok_or(crate::SyncError::UnknownFile(file_id))?;
    let dims = match (fw, fh) {
        (Some(w), Some(h)) if w > 0 && h > 0 => (w as u32, h as u32),
        _ => imagesize::size(&path)
            .ok()
            .map(|s| (s.width as u32, s.height as u32))
            .filter(|(w, h)| *w > 0 && *h > 0)
            .ok_or_else(|| {
                CatalogError::InvalidQuery(format!("image size of file {file_id} is unknown"))
            })?,
    };
    let (dw, dh) = (f64::from(dims.0), f64::from(dims.1));
    let region = FaceRegion {
        name: name.to_string(),
        x: (f64::from(x) + f64::from(w) / 2.0) / dw,
        y: (f64::from(y) + f64::from(h) / 2.0) / dh,
        w: f64::from(w) / dw,
        h: f64::from(h) / dh,
    };
    Ok((region, dims))
}

/// One person: the name a user gave to a cluster.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PersonRow {
    pub id: i64,
    pub name: String,
}

/// An unconfirmed face the named cluster's person probably also is.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Suggestion {
    pub face_id: i64,
    pub cluster_id: i64,
    /// The suggesting person's name (the one confirmed in the flow).
    pub name: String,
    /// Cosine similarity between the face's cluster centroid and the named
    /// cluster's centroid, in `[0, 1]`.
    pub similarity: f32,
}

fn blob_to_vec(bytes: &[u8]) -> Vec<f32> {
    bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let (na, nb) =
        (a.iter().map(|x| x * x).sum::<f32>().sqrt(), b.iter().map(|y| y * y).sum::<f32>().sqrt());
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    (dot / (na * nb)).clamp(-1.0, 1.0)
}

impl Catalog {
    /// Name a cluster: create the person, stamp every face in the cluster
    /// with it and write each photo's sidecar a named MWG face region
    /// (TASK-05-008, §5: a named face is user data — the sidecar is the
    /// source of truth, so the stamp is not done without it). Blank and
    /// already-taken names are refused; one transaction per face, so a
    /// sidecar failure leaves the remaining faces unstamped but never
    /// corrupts a written one.
    pub fn name_cluster(&mut self, cluster_id: i64, name: &str) -> SyncResult<PersonRow> {
        let name = name.trim();
        if name.is_empty() {
            return Err(CatalogError::InvalidQuery("person name is blank".into()).into());
        }
        let exists: bool = self
            .conn()
            .query_row("SELECT 1 FROM face_clusters WHERE id = ?1", [cluster_id], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?
            .is_some();
        if !exists {
            return Err(CatalogError::InvalidQuery(format!("no cluster {cluster_id}")).into());
        }
        // Same name on the same cluster is a no-op, not an error.
        if let Some(existing) = self
            .conn()
            .query_row(
                "SELECT p.id, p.name FROM persons p \
                 JOIN faces f ON f.person_id = p.id WHERE f.cluster = ?1 LIMIT 1",
                [cluster_id],
                |r| Ok(PersonRow { id: r.get(0)?, name: r.get(1)? }),
            )
            .optional()?
        {
            if existing.name.eq_ignore_ascii_case(name) {
                return Ok(existing);
            }
        }
        let person_id = match self.conn().execute("INSERT INTO persons (name) VALUES (?1)", [name])
        {
            Ok(_) => self.conn().last_insert_rowid(),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Err(CatalogError::InvalidQuery(format!(
                    "person name {name:?} is already in use"
                ))
                .into());
            }
            Err(e) => return Err(e.into()),
        };

        let faces = {
            let mut stmt = self.conn().prepare_cached(
                "SELECT id, file_id, x, y, width, height FROM faces \
                 WHERE cluster = ?1 ORDER BY id",
            )?;
            let faces: Vec<_> = stmt
                .query_map([cluster_id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, f32>(2)?,
                        r.get::<_, f32>(3)?,
                        r.get::<_, f32>(4)?,
                        r.get::<_, f32>(5)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            faces
        };
        for (face_id, file_id, x, y, w, h) in faces {
            let (region, dims) = region_for(self, file_id, name, x, y, w, h)?;
            self.mutate_user_data(file_id, |sc| {
                sc.add_face_regions(&[region], Some(dims));
            })?;
            self.conn().execute(
                "UPDATE faces SET person_id = ?1 WHERE id = ?2",
                rusqlite::params![person_id, face_id],
            )?;
        }
        Ok(PersonRow { id: person_id, name: name.to_string() })
    }

    /// The person a cluster is named as, if any.
    pub fn cluster_person(&self, cluster_id: i64) -> Result<Option<PersonRow>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT p.id, p.name FROM persons p \
             JOIN faces f ON f.person_id = p.id WHERE f.cluster = ?1 LIMIT 1",
        )?;
        Ok(stmt
            .query_row([cluster_id], |r| Ok(PersonRow { id: r.get(0)?, name: r.get(1)? }))
            .optional()?)
    }

    /// The person a face is confirmed as, if any.
    pub fn face_person(&self, face_id: i64) -> Result<Option<i64>> {
        Ok(self
            .conn()
            .query_row("SELECT person_id FROM faces WHERE id = ?1", [face_id], |r| r.get(0))
            .optional()?
            .flatten())
    }

    /// Faces whose cluster centroid is cosine-similar (≥ `threshold`) to
    /// the named cluster's — the "remaining matches" the name is offered
    /// to. Only faces without a confirmed person are suggested, so
    /// partially confirmed clusters still surface their rest.
    pub fn name_suggestions(&self, cluster_id: i64, threshold: f32) -> Result<Vec<Suggestion>> {
        let person = match self.cluster_person(cluster_id)? {
            Some(person) => person,
            None => {
                return Err(CatalogError::InvalidQuery(format!("cluster {cluster_id} unnamed")))
            }
        };
        let (dim, centroid): (i64, Vec<u8>) = self.conn().query_row(
            "SELECT dim, centroid FROM face_clusters WHERE id = ?1",
            [cluster_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let named: Vec<f32> = blob_to_vec(&centroid).into_iter().take(dim as usize).collect();

        let mut stmt = self.conn().prepare(
            "SELECT c.id, c.dim, c.centroid FROM face_clusters c \
             WHERE EXISTS (SELECT 1 FROM faces f WHERE f.cluster = c.id AND f.person_id IS NULL)",
        )?;
        let candidates = stmt
            .query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Vec<u8>>(2)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut out = Vec::new();
        for (cid, cdim, cblob) in candidates {
            let candidate: Vec<f32> = blob_to_vec(&cblob).into_iter().take(cdim as usize).collect();
            let similarity = cosine(&named, &candidate);
            if similarity < threshold {
                continue;
            }
            let mut faces = self.conn().prepare(
                "SELECT id FROM faces WHERE cluster = ?1 AND person_id IS NULL ORDER BY id",
            )?;
            for face_id in faces.query_map([cid], |r| r.get::<_, i64>(0))? {
                out.push(Suggestion {
                    face_id: face_id?,
                    cluster_id: cid,
                    name: person.name.clone(),
                    similarity,
                });
            }
        }
        out.sort_by(|a, b| {
            b.similarity.partial_cmp(&a.similarity).unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(out)
    }

    /// Confirm a suggestion: stamp the face with `person_id` and write the
    /// person's name into the photo's sidecar as a named MWG face region
    /// (§5 user data — survives a catalog rebuild). Sidecar first, catalog
    /// mirror second; a sidecar failure leaves the face unstamped.
    pub fn confirm_suggestion(&mut self, face_id: i64, person_id: i64) -> SyncResult<PersonRow> {
        let row: Option<(i64, f32, f32, f32, f32)> = self
            .conn()
            .query_row(
                "SELECT file_id, x, y, width, height FROM faces WHERE id = ?1",
                [face_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;
        let (file_id, x, y, w, h) = row.ok_or(crate::SyncError::UnknownFile(face_id))?;
        let name: String = self
            .conn()
            .query_row("SELECT name FROM persons WHERE id = ?1", [person_id], |r| r.get(0))
            .optional()?
            .ok_or(crate::SyncError::UnknownFile(person_id))?;
        let (region, dims) = region_for(self, file_id, &name, x, y, w, h)?;
        self.mutate_user_data(file_id, |sc| {
            sc.add_face_regions(&[region], Some(dims));
        })?;

        self.conn().execute(
            "UPDATE faces SET person_id = ?1 WHERE id = ?2",
            rusqlite::params![person_id, face_id],
        )?;
        Ok(PersonRow { id: person_id, name })
    }
}

/// TASK-05-005: the unnamed people queue (PLAN §2G "Unnamed people queue").
/// One reviewable entry: an unnamed cluster, the count of its still-
/// actionable faces and a representative face for the thumbnail.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct QueueCluster {
    pub cluster_id: i64,
    /// Faces that a bulk action would still touch: unnamed and not rejected.
    pub face_count: i64,
    pub representative: QueueFace,
}

/// The representative face: box (fractions of the photo) plus the photo's
/// content hash backing the `thumb://` URI (`None` until the hash backfill
/// fills it in — no image bytes ever cross IPC, §3 rule 3).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct QueueFace {
    pub face_id: i64,
    pub file_id: i64,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub hash: Option<String>,
}

impl Catalog {
    /// Unnamed clusters with at least one actionable face (unnamed, not
    /// rejected), cluster-id ordered for a stable queue. Fully rejected and
    /// already-named clusters do not re-enter.
    pub fn unnamed_queue(&self) -> Result<Vec<QueueCluster>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT c.id, COUNT(f.id), MIN(f.id) FROM face_clusters c \
             JOIN faces f ON f.cluster = c.id \
             WHERE f.person_id IS NULL AND f.rejected = 0 \
             GROUP BY c.id ORDER BY c.id",
        )?;
        let heads = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        heads
            .into_iter()
            .map(|(cluster_id, face_count, rep)| {
                let mut stmt = self.conn().prepare_cached(
                    "SELECT f.id, f.file_id, f.x, f.y, f.width, f.height, fi.hash \
                     FROM faces f JOIN files fi ON fi.id = f.file_id WHERE f.id = ?1",
                )?;
                let representative = stmt.query_row([rep], |r| {
                    Ok(QueueFace {
                        face_id: r.get(0)?,
                        file_id: r.get(1)?,
                        x: r.get(2)?,
                        y: r.get(3)?,
                        width: r.get(4)?,
                        height: r.get(5)?,
                        hash: r.get(6)?,
                    })
                })?;
                Ok(QueueCluster { cluster_id, face_count, representative })
            })
            .collect()
    }

    /// Per-face rejection state (TASK-05-005): the suggestion is suppressed,
    /// the face stays detected. A face with a confirmed person cannot be
    /// rejected — its name is already user data in the sidecar.
    pub fn reject_face(&self, face_id: i64) -> Result<()> {
        let named: Option<Option<i64>> = self
            .conn()
            .query_row("SELECT person_id FROM faces WHERE id = ?1", [face_id], |r| r.get(0))
            .optional()?;
        match named {
            None => Err(CatalogError::InvalidQuery(format!("no face {face_id}"))),
            Some(Some(_)) => {
                Err(CatalogError::InvalidQuery(format!("face {face_id} already has a person")))
            }
            Some(None) => {
                self.conn().execute("UPDATE faces SET rejected = 1 WHERE id = ?1", [face_id])?;
                Ok(())
            }
        }
    }

    /// Bulk reject: suppress the suggestion for every still-unnamed face of
    /// the cluster in one action. Returns how many faces were rejected.
    pub fn bulk_reject_cluster(&mut self, cluster_id: i64) -> Result<usize> {
        let exists: bool = self
            .conn()
            .query_row("SELECT 1 FROM face_clusters WHERE id = ?1", [cluster_id], |r| r.get(0))
            .optional()?
            .unwrap_or(false);
        if !exists {
            return Err(CatalogError::InvalidQuery(format!("no cluster {cluster_id}")));
        }
        Ok(self.conn().execute(
            "UPDATE faces SET rejected = 1 WHERE cluster = ?1 AND person_id IS NULL AND rejected = 0",
            [cluster_id],
        )?)
    }

    /// Bulk confirm: one action names the cluster (get-or-create the
    /// person), stamps every still-unnamed non-rejected face with it and
    /// writes the name into each photo's sidecar as a named MWG region
    /// (§5 user data — sidecar first, catalog mirror second; one
    /// transaction per face, so a sidecar failure leaves the remaining
    /// faces unstamped but never corrupts a written one). Returns the
    /// person and how many faces were confirmed.
    pub fn bulk_confirm_cluster(
        &mut self,
        cluster_id: i64,
        name: &str,
    ) -> SyncResult<(PersonRow, usize)> {
        let name = name.trim();
        if name.is_empty() {
            return Err(CatalogError::InvalidQuery("person name is blank".into()).into());
        }
        let exists: bool = self
            .conn()
            .query_row("SELECT 1 FROM face_clusters WHERE id = ?1", [cluster_id], |r| r.get(0))
            .optional()?
            .unwrap_or(false);
        if !exists {
            return Err(CatalogError::InvalidQuery(format!("no cluster {cluster_id}")).into());
        }
        // Get-or-create: a cluster that already carries a person keeps it —
        // a different name would silently rename confirmed faces.
        let person = match self.cluster_person(cluster_id)? {
            Some(person) => {
                if !person.name.eq_ignore_ascii_case(name) {
                    return Err(CatalogError::InvalidQuery(format!(
                        "cluster {cluster_id} is already named {:?}",
                        person.name
                    ))
                    .into());
                }
                person
            }
            None => {
                let person_id =
                    match self.conn().execute("INSERT INTO persons (name) VALUES (?1)", [name]) {
                        Ok(_) => self.conn().last_insert_rowid(),
                        Err(rusqlite::Error::SqliteFailure(e, _))
                            if e.code == rusqlite::ErrorCode::ConstraintViolation =>
                        {
                            return Err(CatalogError::InvalidQuery(format!(
                                "person name {name:?} is already in use"
                            ))
                            .into());
                        }
                        Err(e) => return Err(e.into()),
                    };
                PersonRow { id: person_id, name: name.to_string() }
            }
        };

        let faces = {
            let mut stmt = self.conn().prepare_cached(
                "SELECT id, file_id, x, y, width, height FROM faces \
                 WHERE cluster = ?1 AND person_id IS NULL AND rejected = 0 ORDER BY id",
            )?;
            let faces: Vec<_> = stmt
                .query_map([cluster_id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, f32>(2)?,
                        r.get::<_, f32>(3)?,
                        r.get::<_, f32>(4)?,
                        r.get::<_, f32>(5)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            faces
        };

        let mut confirmed = 0;
        for (face_id, file_id, x, y, w, h) in faces {
            let (region, dims) = region_for(self, file_id, name, x, y, w, h)?;
            self.mutate_user_data(file_id, |sc| {
                sc.add_face_regions(&[region], Some(dims));
            })?;
            self.conn().execute(
                "UPDATE faces SET person_id = ?1 WHERE id = ?2",
                rusqlite::params![person.id, face_id],
            )?;
            confirmed += 1;
        }
        Ok((person, confirmed))
    }
}

// ---- TASK-05-006: the People Manager (PLAN §2 Tools menu, §6 P3 People) ---

/// One People Manager entry: a named person (`person_id` set) or an unnamed
/// cluster (`cluster_id` set), with a face count and a representative face
/// for the `thumb://` URI. `representative` is `None` for a person with no
/// confirmed faces. `contact_id` is the local OS contact's reference
/// (TASK-05-011) — the contact data itself is resolved by the app shell
/// from the OS contacts store, never stored here.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PeopleEntry {
    pub person_id: Option<i64>,
    pub cluster_id: Option<i64>,
    pub name: Option<String>,
    pub face_count: i64,
    pub representative: Option<QueueFace>,
    #[serde(default)]
    pub contact_id: Option<String>,
}

impl Catalog {
    /// Every named person and every unnamed cluster, named entries first
    /// (name-ordered), then unnamed clusters (cluster-id ordered, the queue
    /// order). Fully rejected clusters stay listed — their faces are still
    /// unnamed — but the unnamed queue's entry point covers the actionable
    /// ones.
    pub fn people_entries(&self) -> Result<Vec<PeopleEntry>> {
        let mut entries = Vec::new();

        let mut named = self.conn().prepare_cached(
            "SELECT p.id, p.name, COUNT(f.id), MIN(f.id), \
             (SELECT pc.contact_id FROM person_contacts pc WHERE pc.person_id = p.id) \
             FROM persons p \
             LEFT JOIN faces f ON f.person_id = p.id \
             GROUP BY p.id ORDER BY p.name COLLATE NOCASE",
        )?;
        let heads = named
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (person_id, name, face_count, rep, contact_id) in heads {
            entries.push(PeopleEntry {
                person_id: Some(person_id),
                cluster_id: None,
                name: Some(name),
                face_count,
                representative: rep.and_then(|id| self.representative_face(id).ok().flatten()),
                contact_id,
            });
        }

        let mut unnamed = self.conn().prepare_cached(
            "SELECT c.id, \
             COALESCE(SUM(CASE WHEN f.rejected = 0 THEN 1 ELSE 0 END), 0), \
             MIN(CASE WHEN f.rejected = 0 THEN f.id END), MIN(f.id) \
             FROM face_clusters c \
             JOIN faces f ON f.cluster = c.id \
             WHERE f.person_id IS NULL \
             GROUP BY c.id ORDER BY c.id",
        )?;
        let heads = unnamed
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (cluster_id, face_count, actionable, any) in heads {
            let rep = actionable.or(Some(any));
            entries.push(PeopleEntry {
                person_id: None,
                cluster_id: Some(cluster_id),
                name: None,
                face_count,
                representative: rep.and_then(|id| self.representative_face(id).ok().flatten()),
                contact_id: None,
            });
        }
        Ok(entries)
    }

    /// The representative face row by face id; `None` when the face is gone.
    fn representative_face(&self, face_id: i64) -> Result<Option<QueueFace>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT f.id, f.file_id, f.x, f.y, f.width, f.height, fi.hash \
             FROM faces f JOIN files fi ON fi.id = f.file_id WHERE f.id = ?1",
        )?;
        Ok(stmt
            .query_row([face_id], |r| {
                Ok(QueueFace {
                    face_id: r.get(0)?,
                    file_id: r.get(1)?,
                    x: r.get(2)?,
                    y: r.get(3)?,
                    width: r.get(4)?,
                    height: r.get(5)?,
                    hash: r.get(6)?,
                })
            })
            .optional()?)
    }

    /// Rename a person: the name lands in every confirmed face's sidecar
    /// `mwg-rs:Regions` first (§5 — sidecar first, catalog mirror second,
    /// one transaction per face so a failure never corrupts a written
    /// sidecar), then the `persons` row is updated. Sidecars whose
    /// `Regions` structure we do not recognise are left verbatim — a
    /// Fotobase write never destroys XMP it does not understand. Refuses a
    /// blank name, a name another person already uses (case-insensitively)
    /// and an unknown person id before touching any sidecar.
    pub fn rename_person(&mut self, person_id: i64, new_name: &str) -> SyncResult<PersonRow> {
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Err(CatalogError::InvalidQuery("person name is blank".into()).into());
        }
        let exists: Option<String> = self
            .conn()
            .query_row("SELECT name FROM persons WHERE id = ?1", [person_id], |r| r.get(0))
            .optional()?;
        let old_name =
            exists.ok_or_else(|| CatalogError::InvalidQuery(format!("no person {person_id}")))?;
        if new_name.eq_ignore_ascii_case(&old_name) {
            return Ok(PersonRow { id: person_id, name: new_name.to_string() });
        }
        let taken: bool = self
            .conn()
            .query_row(
                "SELECT 1 FROM persons WHERE name = ?1 COLLATE NOCASE AND id != ?2",
                rusqlite::params![new_name, person_id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(false);
        if taken {
            return Err(CatalogError::InvalidQuery(format!(
                "person name {new_name:?} is already in use"
            ))
            .into());
        }

        let faces = {
            let mut stmt = self.conn().prepare_cached(
                "SELECT f.id, f.file_id, f.x, f.y, f.width, f.height FROM faces f \
                 WHERE f.person_id = ?1 ORDER BY f.id",
            )?;
            let faces: Vec<_> = stmt
                .query_map([person_id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, f32>(2)?,
                        r.get::<_, f32>(3)?,
                        r.get::<_, f32>(4)?,
                        r.get::<_, f32>(5)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            faces
        };
        for (_, file_id, ..) in &faces {
            self.mutate_user_data(*file_id, |sc| {
                sc.rename_face_regions(&old_name, new_name);
            })?;
        }

        self.conn().execute(
            "UPDATE persons SET name = ?1 WHERE id = ?2",
            rusqlite::params![new_name, person_id],
        )?;
        Ok(PersonRow { id: person_id, name: new_name.to_string() })
    }

    /// Link a person to a local OS contact (TASK-05-011, PLAN §2G "Contacts
    /// link — local contacts only"). Only the contact's *reference* is
    /// stored — the contact data itself lives in the OS contacts store and
    /// is resolved by the app shell on display; nothing is copied, nothing
    /// is written to sidecars (a contact mapping is app-local state, not
    /// photo user data). One contact maps to at most one person and vice
    /// versa; relinking replaces the previous reference. Refuses an unknown
    /// person, a blank reference and a contact another person already uses.
    pub fn link_contact(&mut self, person_id: i64, contact_id: &str) -> Result<PersonRow> {
        let contact_id = contact_id.trim();
        if contact_id.is_empty() {
            return Err(CatalogError::InvalidQuery("contact reference is blank".into()));
        }
        let exists: Option<i64> = self
            .conn()
            .query_row("SELECT id FROM persons WHERE id = ?1", [person_id], |r| r.get(0))
            .optional()?;
        if exists.is_none() {
            return Err(CatalogError::InvalidQuery(format!("no person {person_id}")));
        }
        let taken: Option<String> = self
            .conn()
            .query_row(
                "SELECT p.name FROM person_contacts pc \
                 JOIN persons p ON p.id = pc.person_id \
                 WHERE pc.contact_id = ?1 AND pc.person_id != ?2",
                rusqlite::params![contact_id, person_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(name) = taken {
            return Err(CatalogError::InvalidQuery(format!(
                "contact {contact_id:?} is already linked to {name:?}"
            )));
        }
        self.conn().execute(
            "INSERT INTO person_contacts (person_id, contact_id) VALUES (?1, ?2) \
             ON CONFLICT(person_id) DO UPDATE SET contact_id = excluded.contact_id",
            rusqlite::params![person_id, contact_id],
        )?;
        Ok(PersonRow { id: person_id, name: self.person_name(person_id) })
    }

    /// Remove a person's contact link. Unlinking an unlinked person is a
    /// no-op; an unknown person id is refused.
    pub fn unlink_contact(&mut self, person_id: i64) -> Result<()> {
        let exists: Option<i64> = self
            .conn()
            .query_row("SELECT id FROM persons WHERE id = ?1", [person_id], |r| r.get(0))
            .optional()?;
        if exists.is_none() {
            return Err(CatalogError::InvalidQuery(format!("no person {person_id}")));
        }
        self.conn().execute("DELETE FROM person_contacts WHERE person_id = ?1", [person_id])?;
        Ok(())
    }

    fn person_name(&self, person_id: i64) -> String {
        self.conn()
            .query_row("SELECT name FROM persons WHERE id = ?1", [person_id], |r| r.get(0))
            .unwrap_or_default()
    }
}
