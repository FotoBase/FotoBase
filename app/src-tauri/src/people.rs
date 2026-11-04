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

// ---- TASK-05-006: the People Manager (PLAN §2 Tools menu, §6 P3 People) ---

/// Every named person and every unnamed cluster, each with a face count and
/// a representative face (the photo's content-hash `thumb://` URI — no
/// image bytes over IPC, §3 rule 3).
pub fn entries(cat: &Catalog) -> Result<Vec<fotobase_catalog::PeopleEntry>, CatalogError> {
    cat.people_entries()
}

/// Rename a person: every confirmed face's sidecar MWG region is rewritten
/// (sidecar-first per docs/sidecar-merge-on-write.md), then the persons
/// row. Returns the renamed person.
pub fn rename(
    cat: &mut Catalog,
    person_id: i64,
    name: &str,
) -> Result<fotobase_catalog::PersonRow, fotobase_catalog::sync::SyncError> {
    cat.rename_person(person_id, name)
}

/// The renamed person, serialized for IPC.
pub type RenamedPerson = fotobase_catalog::PersonRow;

// ---- TASK-05-011: contacts link (PLAN §2G "Contacts link — local contacts
// only") — the catalog holds the reference, this layer resolves it against
// the OS contacts store. No network: the provider trait is the only access
// to contact data and has no network client behind it (test-enforced).

/// One contact resolved from the local OS contacts store for display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkedContact {
    pub name: String,
    pub email: Option<String>,
}

/// The provider-backed face of [`entries`]: catalog rows plus the resolved
/// contact for linked people (a contact deleted on the OS side resolves to
/// `None` — the mapping survives, the display shows nothing).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EntryWithContact {
    #[serde(flatten)]
    pub entry: fotobase_catalog::PeopleEntry,
    pub contact: Option<LinkedContact>,
}

/// People Manager entries with locally resolved contacts.
pub fn entries_with_contacts(
    cat: &Catalog,
    contacts: &dyn crate::contacts::ContactsProvider,
) -> Result<Vec<EntryWithContact>, CatalogError> {
    Ok(cat
        .people_entries()?
        .into_iter()
        .map(|entry| {
            let contact = match entry.contact_id.as_deref() {
                Some(id) => contacts
                    .get(id)
                    .ok()
                    .flatten()
                    .map(|c| LinkedContact { name: c.name, email: c.email }),
                None => None,
            };
            EntryWithContact { entry, contact }
        })
        .collect())
}

/// Link a person to a local contact by reference. The contact must exist in
/// the local store — the reference is validated against the provider before
/// it lands in the catalog; nothing is copied from the store.
pub fn link(
    cat: &mut Catalog,
    contacts: &dyn crate::contacts::ContactsProvider,
    person_id: i64,
    contact_id: &str,
) -> Result<(fotobase_catalog::PersonRow, Option<LinkedContact>), CatalogError> {
    let contact = contacts
        .get(contact_id)
        .map_err(CatalogError::Corrupted)?
        .ok_or_else(|| CatalogError::InvalidQuery(format!("no contact {contact_id:?}")))?;
    cat.link_contact(person_id, contact.id.as_str())?;
    Ok((
        fotobase_catalog::PersonRow { id: person_id, name: person_name(cat, person_id) },
        Some(LinkedContact { name: contact.name, email: contact.email }),
    ))
}

/// Remove a person's contact link. The reference-only mapping is dropped;
/// the contact itself is untouched.
pub fn unlink(
    cat: &mut Catalog,
    person_id: i64,
) -> Result<fotobase_catalog::PersonRow, CatalogError> {
    cat.unlink_contact(person_id)?;
    Ok(fotobase_catalog::PersonRow { id: person_id, name: person_name(cat, person_id) })
}

/// Search the local contacts store (name prefix/substring, provider's
/// choice). Strictly local, whatever the platform backend does.
pub fn search(
    contacts: &dyn crate::contacts::ContactsProvider,
    query: &str,
) -> Result<Vec<crate::contacts::Contact>, String> {
    contacts.search(query)
}

fn person_name(cat: &Catalog, person_id: i64) -> String {
    cat.conn()
        .query_row("SELECT name FROM persons WHERE id = ?1", [person_id], |r| r.get(0))
        .unwrap_or_default()
}
