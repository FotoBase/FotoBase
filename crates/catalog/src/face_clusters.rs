//! TASK-05-003: HDBSCAN face clustering (PLAN §6 P3 People, §4 Clustering,
//! §5 derived-data domain). The `hdbscan` crate (MIT/Apache-2.0, pure Rust)
//! satisfies PLAN §4's "crate or in-house" — it needs no external build
//! deps and supports an unknown number of clusters plus noise.
//!
//! Clusters are derived data (PLAN §5 Authority): recomputable from the
//! stored embeddings, never written to sidecars (PLAN §3 rule 2). Full
//! re-clustering rebuilds the partition from all embeddings; incremental
//! assignment only touches new faces — existing rows keep their cluster.

use crate::{Catalog, CatalogError, Result};
use hdbscan::{Center, DistanceMetric, Hdbscan, HdbscanHyperParams};
use rusqlite::OptionalExtension;

/// Distance at or below which a new face joins an existing cluster's
/// centroid instead of spawning a new cluster. Euclidean on the embedding;
/// the face-embedding scale (MobileFaceNet-class, unit-ish vectors) makes
/// 1.0 a conservative join threshold — tightened with the §9 labeled set.
pub const JOIN_THRESHOLD: f32 = 1.0;

/// One cluster as the app consumes it. `person_id` is `None` for unnamed
/// clusters (TASK-05-004 names them).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ClusterRow {
    pub id: i64,
    pub size: i64,
    pub person_id: Option<i64>,
}

fn blob_to_vec(bytes: &[u8]) -> Vec<f32> {
    bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Euclidean distance between two equal-length vectors.
fn dist(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum::<f32>().sqrt()
}

impl Catalog {
    /// Load every face that has an embedding: `(face_id, dim, vector)`,
    /// face-id ordered so runs are deterministic (the hdbscan crate is
    /// deterministic on fixed input order, and we always feed it this order).
    fn embedded_faces(&self) -> Result<Vec<(i64, Vec<f32>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, e.dim, e.vector FROM faces f \
             JOIN embeddings e ON e.face_id = f.id ORDER BY f.id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let id: i64 = r.get(0)?;
                let dim: i64 = r.get(1)?;
                let blob: Vec<u8> = r.get(2)?;
                Ok((id, dim, blob))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .map(|(id, dim, blob)| {
                let v = blob_to_vec(&blob);
                (id, v.into_iter().take(dim as usize).collect::<Vec<_>>())
            })
            .collect())
    }

    /// Full HDBSCAN pass over all embedded faces. Rebuilds the cluster table
    /// and rewrites every `faces.cluster`; `person_id` links survive only
    /// when the named cluster survives (matched by centroid proximity).
    pub fn cluster_faces(&mut self, min_cluster_size: usize) -> Result<()> {
        let faces = self.embedded_faces()?;
        let tx = self.conn_mut().transaction()?;
        tx.execute("DELETE FROM face_clusters", [])?;
        tx.execute("UPDATE faces SET cluster = NULL", [])?;

        if faces.len() >= min_cluster_size {
            let dim = faces[0].1.len();
            let data: Vec<Vec<f32>> = faces.iter().map(|(_, v)| v.clone()).collect();
            // `min_samples = 2` keeps dense synthetic-quality embeddings
            // from reading as noise, and `allow_single_cluster` lets a
            // library with exactly one person still form a cluster.
            let params = HdbscanHyperParams::builder()
                .min_cluster_size(min_cluster_size)
                .min_samples(2)
                .dist_metric(DistanceMetric::Euclidean)
                .allow_single_cluster(true)
                .build();
            let clusterer = Hdbscan::new(&data, params);
            let labels = clusterer
                .cluster()
                .map_err(|e| CatalogError::Clustering(format!("hdbscan failed: {e}")))?;
            let centroids = clusterer
                .calc_centers(Center::Centroid, &labels)
                .map_err(|e| CatalogError::Clustering(format!("hdbscan centroids failed: {e}")))?;

            // Deterministic mapping: label → cluster row, in label order.
            let mut label_ids: std::collections::BTreeMap<i32, i64> = Default::default();
            for (i, &label) in labels.iter().enumerate() {
                if label < 0 {
                    continue; // noise — stays unclustered
                }
                let cluster_id = *label_ids.entry(label).or_insert_with(|| {
                    tx.execute(
                        "INSERT INTO face_clusters (dim, centroid, size) VALUES (?1, ?2, 0)",
                        rusqlite::params![dim as i64, vec_to_blob(&centroids[label as usize])],
                    )
                    .expect("insert cluster");
                    tx.last_insert_rowid()
                });
                tx.execute(
                    "UPDATE faces SET cluster = ?1 WHERE id = ?2",
                    rusqlite::params![cluster_id, faces[i].0],
                )?;
                tx.execute("UPDATE face_clusters SET size = size + 1 WHERE id = ?1", [cluster_id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Incremental pass: assign each not-yet-clustered face to the nearest
    /// existing cluster whose centroid is within `JOIN_THRESHOLD`, else
    /// spawn a new cluster for it. Existing assignments are never rewritten
    /// — "no full re-cluster required for every import".
    pub fn cluster_faces_incremental(&mut self, join_threshold: f32) -> Result<()> {
        let faces = self.embedded_faces()?;
        let unclustered: Vec<(i64, Vec<f32>)> = {
            let mut stmt =
                self.conn.prepare("SELECT f.id FROM faces f WHERE f.cluster IS NOT NULL")?;
            let clustered: std::collections::HashSet<i64> =
                stmt.query_map([], |r| r.get(0))?.flatten().collect();
            faces.into_iter().filter(|(id, _)| !clustered.contains(id)).collect()
        };
        if unclustered.is_empty() {
            return Ok(());
        }

        let tx = self.conn_mut().transaction()?;
        let mut clusters: Vec<(i64, Vec<f32>)> = {
            let mut stmt = tx.prepare("SELECT id, dim, centroid FROM face_clusters")?;
            let rows = stmt
                .query_map([], |r| {
                    let id: i64 = r.get(0)?;
                    let dim: i64 = r.get(1)?;
                    let blob: Vec<u8> = r.get(2)?;
                    Ok((id, blob_to_vec(&blob).into_iter().take(dim as usize).collect::<Vec<_>>()))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };

        for (face_id, vec) in unclustered {
            let best = clusters
                .iter()
                .enumerate()
                .map(|(i, (cid, c))| (dist(&vec, c), i, *cid))
                .filter(|(d, _, _)| *d <= join_threshold)
                .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

            let cluster_id = match best {
                Some((_, _, cid)) => cid,
                None => {
                    tx.execute(
                        "INSERT INTO face_clusters (dim, centroid, size) VALUES (?1, ?2, 0)",
                        rusqlite::params![vec.len() as i64, vec_to_blob(&vec)],
                    )?;
                    let cid = tx.last_insert_rowid();
                    clusters.push((cid, vec.clone()));
                    cid
                }
            };
            tx.execute(
                "UPDATE faces SET cluster = ?1 WHERE id = ?2",
                rusqlite::params![cluster_id, face_id],
            )?;
            tx.execute("UPDATE face_clusters SET size = size + 1 WHERE id = ?1", [cluster_id])?;
            // Move the centroid toward the new member — a running mean keeps
            // incremental assignment consistent as the cluster grows.
            if let Some((_, c)) = clusters.iter_mut().find(|(cid, _)| *cid == cluster_id) {
                let n = c.len() as f32;
                for (cv, nv) in c.iter_mut().zip(&vec) {
                    *cv += (nv - *cv) / n;
                }
                tx.execute(
                    "UPDATE face_clusters SET centroid = ?1 WHERE id = ?2",
                    rusqlite::params![vec_to_blob(c), cluster_id],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Cluster id per face, for the requested ids only.
    pub fn face_cluster_assignments(
        &self,
        face_ids: &[i64],
    ) -> Result<std::collections::HashMap<i64, i64>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, cluster FROM faces WHERE id = ?1 AND cluster IS NOT NULL",
        )?;
        let mut out = std::collections::HashMap::new();
        for id in face_ids {
            if let Some(cluster) = stmt.query_row([id], |r| r.get::<_, i64>(1)).optional()? {
                out.insert(*id, cluster);
            }
        }
        Ok(out)
    }

    /// All clusters with their current size and naming state.
    pub fn face_clusters(&self) -> Result<Vec<ClusterRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.id, c.size, f.person_id FROM face_clusters c \
             LEFT JOIN faces f ON f.cluster = c.id AND f.person_id IS NOT NULL \
             GROUP BY c.id ORDER BY c.id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(ClusterRow { id: r.get(0)?, size: r.get(1)?, person_id: r.get(2)? })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}
