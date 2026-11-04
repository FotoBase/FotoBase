//! TASK-05-011: contacts link (PLAN §2G "Contacts link — local contacts
//! only", §6 P3 People). The catalog stores only the *reference* to a local
//! OS contact — the contact data itself is never copied (§5: the sidecar
//! keeps photo user data; the contact mapping is app-local state). Tests
//! run against a mock contact id exactly like the app layers do.

use fotobase_catalog::Catalog;

fn catalog() -> Catalog {
    let dir = tempfile::tempdir().unwrap();
    let cat = Catalog::open(dir.path().join("catalog.db")).unwrap();
    // Leak the tempdir so the database file outlives the returned catalog.
    std::mem::forget(dir);
    cat
}

fn catalog_at(path: &std::path::Path) -> Catalog {
    Catalog::open(path).unwrap()
}

#[test]
fn link_persists_and_entries_show_the_reference() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("catalog.db");
    let mut cat = catalog_at(&db);
    let alice = person(&mut cat, "Alice");
    assert_eq!(cat.people_entries().unwrap()[0].contact_id, None);

    cat.link_contact(alice.id, "os-contact-42").unwrap();

    let entry = &cat.people_entries().unwrap()[0];
    assert_eq!(entry.person_id, Some(alice.id));
    assert_eq!(entry.contact_id.as_deref(), Some("os-contact-42"));

    // The mapping survives a reopen (catalog state, not in-memory).
    drop(cat);
    let cat = catalog_at(&db);
    let entry = &cat.people_entries().unwrap()[0];
    assert_eq!(entry.contact_id.as_deref(), Some("os-contact-42"));
}

fn person(cat: &mut Catalog, name: &str) -> fotobase_catalog::PersonRow {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("p.jpg");
    std::fs::write(&original, b"fake image bytes").unwrap();
    let folder_id = cat.seed_folder(dir.path().to_str().unwrap()).unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, media_type, width, height)
             VALUES (?1, ?2, 'p.jpg', 10, 0.0, 'image', 1000, 1000)",
            rusqlite::params![folder_id, original.to_str().unwrap()],
        )
        .unwrap();
    let file_id = cat.conn().last_insert_rowid();
    cat.record_face(
        file_id,
        &fotobase_catalog::FaceRecord {
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
    // A named person directly: the contacts link is independent of the
    // clustering flow.
    cat.conn().execute("INSERT INTO persons (name) VALUES (?1)", [name]).unwrap();
    let person_id = cat.conn().last_insert_rowid();
    cat.conn().execute("UPDATE faces SET person_id = ?1", [person_id]).unwrap();
    fotobase_catalog::PersonRow { id: person_id, name: name.to_string() }
}

#[test]
fn relink_replaces_and_unlink_clears() {
    let mut cat = catalog();
    let alice = person(&mut cat, "Alice");
    cat.link_contact(alice.id, "os-contact-1").unwrap();
    cat.link_contact(alice.id, "os-contact-2").unwrap();
    assert_eq!(cat.people_entries().unwrap()[0].contact_id.as_deref(), Some("os-contact-2"));

    cat.unlink_contact(alice.id).unwrap();
    assert_eq!(cat.people_entries().unwrap()[0].contact_id, None);
    // Unlinking an unlinked person is a no-op, not an error.
    cat.unlink_contact(alice.id).unwrap();
}

#[test]
fn unknown_person_and_blank_contact_are_refused() {
    let mut cat = catalog();
    assert!(cat.link_contact(999, "c1").is_err());
    let alice = person(&mut cat, "Alice");
    assert!(cat.link_contact(alice.id, "  ").is_err());
    assert!(cat.unlink_contact(999).is_err());
}

#[test]
fn one_contact_links_to_one_person() {
    let mut cat = catalog();
    let alice = person(&mut cat, "Alice");
    let bob = person(&mut cat, "Bob");
    cat.link_contact(alice.id, "os-contact-1").unwrap();
    assert!(cat.link_contact(bob.id, "os-contact-1").is_err());
}
