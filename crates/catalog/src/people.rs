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
    /// Name a cluster: create the person and stamp every face in the
    /// cluster with it. Blank and already-taken names are refused.
    pub fn name_cluster(&mut self, cluster_id: i64, name: &str) -> Result<PersonRow> {
        let name = name.trim();
        if name.is_empty() {
            return Err(CatalogError::InvalidQuery("person name is blank".into()));
        }
        let tx = self.conn_mut().transaction()?;
        let exists: bool = tx
            .query_row("SELECT 1 FROM face_clusters WHERE id = ?1", [cluster_id], |r| r.get(0))
            .optional()?
            .unwrap_or(false);
        if !exists {
            return Err(CatalogError::InvalidQuery(format!("no cluster {cluster_id}")));
        }
        // Same name on the same cluster is a no-op, not an error.
        if let Some(existing) = tx
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
        let person_id = match tx.execute("INSERT INTO persons (name) VALUES (?1)", [name]) {
            Ok(_) => tx.last_insert_rowid(),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Err(CatalogError::InvalidQuery(format!(
                    "person name {name:?} is already in use"
                )));
            }
            Err(e) => return Err(e.into()),
        };
        tx.execute(
            "UPDATE faces SET person_id = ?1 WHERE cluster = ?2",
            rusqlite::params![person_id, cluster_id],
        )?;
        tx.commit()?;
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

        self.mutate_user_data(file_id, |sc| {
            sc.add_face_regions(
                &[FaceRegion {
                    name: name.clone(),
                    x: x as f64,
                    y: y as f64,
                    w: w as f64,
                    h: h as f64,
                }],
                None,
            );
        })?;

        self.conn().execute(
            "UPDATE faces SET person_id = ?1 WHERE id = ?2",
            rusqlite::params![person_id, face_id],
        )?;
        Ok(PersonRow { id: person_id, name })
    }
}
