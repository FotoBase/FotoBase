//! Unnamed people queue, app half (TASK-05-005, PLAN §2G "Unnamed people
//! queue", §6 P3 People). Review payloads over the catalog's queue query —
//! the catalog owns the rules (which clusters qualify, what a bulk
//! confirm/reject touches), this layer only serializes them for IPC. Face
//! thumbnails come from the photo's content-hash `thumb://` URI like the
//! grid: no image bytes over IPC (§3 rule 3).

use fotobase_catalog::{Catalog, CatalogError, QueueCluster as CatalogQueue};
use serde::Serialize;

/// One reviewable cluster: representative face box + the photo's content
/// hash backing the `thumb://` URI.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QueueClusterInfo {
    pub cluster_id: i64,
    pub face_count: i64,
    pub face_id: i64,
    pub file_id: i64,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub hash: Option<String>,
}

impl From<CatalogQueue> for QueueClusterInfo {
    fn from(q: CatalogQueue) -> Self {
        let r = q.representative;
        Self {
            cluster_id: q.cluster_id,
            face_count: q.face_count,
            face_id: r.face_id,
            file_id: r.file_id,
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
            hash: r.hash,
        }
    }
}

/// The unnamed clusters, queue-ordered (cluster id ascending).
pub fn queue(cat: &Catalog) -> Result<Vec<QueueClusterInfo>, CatalogError> {
    Ok(cat.unnamed_queue()?.into_iter().map(Into::into).collect())
}

/// One bulk action: confirm the cluster as `name` (stamps every actionable
/// face and writes the named MWG region into each sidecar) or reject it
/// (suppress the suggestion per face). Returns how many faces were touched.
/// One bulk action on a queue cluster. The frontend sends either
/// `"reject"` or `{ confirm: { name } }` (serde's external tagging).
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BulkAction {
    Confirm(ConfirmArg),
    Reject,
}

/// The name a cluster is confirmed as.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct ConfirmArg {
    pub name: String,
}

pub fn bulk(
    cat: &mut Catalog,
    cluster_id: i64,
    action: BulkAction,
) -> Result<usize, fotobase_catalog::sync::SyncError> {
    match action {
        BulkAction::Confirm(arg) => Ok(cat.bulk_confirm_cluster(cluster_id, &arg.name)?.1),
        BulkAction::Reject => Ok(cat.bulk_reject_cluster(cluster_id)?),
    }
}
