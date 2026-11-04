//! TASK-05-011 integration: the contacts link's app half. A mock provider
//! stands in for the OS contacts store (the real backends are platform
//! APIs); the flow checked here is the one the UI drives — search the local
//! store, link a person, the mapping persists and the People Manager shows
//! the resolved contact name/email. The contact data is never copied into
//! the catalog: only the provider reference is stored.

use fotobase_catalog::{sidecar_path, Catalog, FaceRecord, PersonRow};
use fotobase_lib::contacts::{Contact, ContactsProvider};
use fotobase_lib::people;

/// A stand-in local contacts store: deterministic, offline, and recording
/// every reference resolution so the test can assert the shell actually
/// goes through the provider instead of copying contact data.
struct MockContacts {
    store: Vec<Contact>,
}

impl ContactsProvider for MockContacts {
    fn search(&self, query: &str) -> Result<Vec<Contact>, String> {
        let q = query.to_lowercase();
        Ok(self.store.iter().filter(|c| c.name.to_lowercase().contains(&q)).cloned().collect())
    }

    fn get(&self, id: &str) -> Result<Option<Contact>, String> {
        Ok(self.store.iter().find(|c| c.id == id).cloned())
    }
}

fn provider() -> MockContacts {
    MockContacts {
        store: vec![
            Contact {
                id: "os-1".into(),
                name: "Alice Example".into(),
                email: Some("alice@example.local".into()),
            },
            Contact { id: "os-2".into(), name: "Bob Example".into(), email: None },
        ],
    }
}

fn fixture() -> (tempfile::TempDir, Catalog) {
    let lib = tempfile::tempdir().unwrap();
    let cat = Catalog::open(tempfile::tempdir().unwrap().path().join("catalog.db")).unwrap();
    (lib, cat)
}

fn named_person(cat: &mut Catalog, dir: &std::path::Path, name: &str) -> PersonRow {
    let original = dir.join(format!("{name}.jpg"));
    std::fs::write(&original, "fake image bytes").unwrap();
    let folder_id = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, media_type, width, height)
             VALUES (?1, ?2, ?3, 10, 0.0, 'image', 1000, 1000)",
            rusqlite::params![folder_id, original.to_str().unwrap(), format!("{name}.jpg")],
        )
        .unwrap();
    cat.record_face(
        cat.conn().last_insert_rowid(),
        &FaceRecord {
            x: 0.1,
            y: 0.2,
            width: 0.2,
            height: 0.25,
            landmarks: [0.0; 10],
            confidence: 0.9,
        },
        Some(&[0.5; 128]),
    )
    .unwrap();
    cat.conn().execute("INSERT INTO persons (name) VALUES (?1)", [name]).unwrap();
    let id = cat.conn().last_insert_rowid();
    cat.conn().execute("UPDATE faces SET person_id = ?1", [id]).unwrap();
    PersonRow { id, name: name.to_string() }
}

#[test]
fn search_link_and_show_through_the_provider() {
    let (lib, mut cat) = fixture();
    let alice = named_person(&mut cat, lib.path(), "Alice");
    let contacts = provider();

    // The UI search only ever touches the local provider.
    let hits = people::search(&contacts, "alice").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "os-1");
    assert_eq!(hits[0].email.as_deref(), Some("alice@example.local"));

    // Link by reference: the catalog stores the id, nothing else.
    let (_, linked) = people::link(&mut cat, &contacts, alice.id, "os-1").unwrap();
    assert_eq!(linked.as_ref().unwrap().name, "Alice Example");
    let stored: String = cat
        .conn()
        .query_row("SELECT contact_id FROM person_contacts WHERE person_id = ?1", [alice.id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(stored, "os-1");
    // The contact's data is NOT in the catalog.
    let leaks: i64 = cat
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM person_contacts WHERE contact_id LIKE '%alice%' \
             OR contact_id LIKE '%Example%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(leaks, 0);

    // The People Manager entry shows the resolved contact name/email.
    let entries = people::entries_with_contacts(&cat, &contacts).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].entry.contact_id.as_deref(), Some("os-1"));
    let shown = entries[0].contact.as_ref().unwrap();
    assert_eq!(shown.name, "Alice Example");
    assert_eq!(shown.email.as_deref(), Some("alice@example.local"));
}

#[test]
fn unlink_clears_and_unknown_reference_is_refused() {
    let (lib, mut cat) = fixture();
    let alice = named_person(&mut cat, lib.path(), "Alice");
    let contacts = provider();

    people::link(&mut cat, &contacts, alice.id, "os-2").unwrap();
    let renamed = people::unlink(&mut cat, alice.id).unwrap();
    assert_eq!(renamed.name, "Alice");
    let entries = people::entries_with_contacts(&cat, &contacts).unwrap();
    assert!(entries[0].contact.is_none());
    assert_eq!(entries[0].entry.contact_id, None);

    // A reference the store does not know is refused — no dangling mapping.
    assert!(people::link(&mut cat, &contacts, alice.id, "os-404").is_err());
    let links: i64 =
        cat.conn().query_row("SELECT COUNT(*) FROM person_contacts", [], |r| r.get(0)).unwrap();
    assert_eq!(links, 0);
}

#[test]
fn linking_touches_no_sidecar() {
    let (lib, mut cat) = fixture();
    let alice = named_person(&mut cat, lib.path(), "Alice");
    let contacts = provider();
    people::link(&mut cat, &contacts, alice.id, "os-1").unwrap();

    // The contact mapping is app-local state, not photo user data — no
    // sidecar write may happen for it (docs/sidecar-merge-on-write.md).
    let original: String =
        cat.conn().query_row("SELECT path FROM files LIMIT 1", [], |r| r.get(0)).unwrap();
    assert!(!sidecar_path(&original).exists());
}
