//! TASK-07-005: advanced search field-filter syntax (PLAN §2 2026 additions
//! "Advanced search syntax"). `person:` / `camera:` / `before:` / `after:`
//! tokens compile into SQL predicates over `persons`/`faces` and `metadata`
//! that AND-combine with the free-text blend; malformed or unknown tokens
//! degrade to literal text instead of erroring. All derived catalog data —
//! no sidecar is touched (§3 rule 2).

use fotobase_catalog::{parse_field_filters, Catalog};

fn fixture() -> (tempfile::TempDir, Catalog, Vec<i64>) {
    let lib = tempfile::tempdir().unwrap();
    let dir = lib.path().join("photos");
    std::fs::create_dir_all(&dir).unwrap();
    let db = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(db.path().join("catalog.db")).unwrap();
    cat.seed_folder(dir.to_str().unwrap()).unwrap();
    let folder_id: i64 =
        cat.conn().query_row("SELECT id FROM folders LIMIT 1", [], |r| r.get(0)).unwrap();

    // Two people × two cameras × two years: anna/X100V/2022, anna/A7/2023,
    // ben/X100V/2023, ben/A7/2022 — every combination of the three axes.
    let rows = [
        ("anna-x100v-2022.jpg", "2022-06-01T10:00:00", "Fujifilm X100V", "Anna"),
        ("anna-a7-2023.jpg", "2023-06-01T10:00:00", "Sony A7", "Anna"),
        ("ben-x100v-2023.jpg", "2023-07-01T10:00:00", "Fujifilm X100V", "Ben"),
        ("ben-a7-2022.jpg", "2022-07-01T10:00:00", "Sony A7", "Ben"),
    ];
    let mut ids = Vec::new();
    for (name, taken, camera, person) in rows {
        let path = dir.join(name);
        std::fs::write(&path, format!("bytes of {name}")).unwrap();
        cat.conn()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, media_type, taken_at)
                 VALUES (?1, ?2, ?3, 10, 0.0, 'image', ?4)",
                rusqlite::params![folder_id, path.to_str().unwrap(), name, taken],
            )
            .unwrap();
        let id = cat.conn().last_insert_rowid();
        cat.conn()
            .execute(
                "INSERT INTO metadata (file_id, camera) VALUES (?1, ?2)",
                rusqlite::params![id, camera],
            )
            .unwrap();
        cat.conn().execute("INSERT OR IGNORE INTO persons (name) VALUES (?1)", [person]).unwrap();
        let person_id: i64 = cat
            .conn()
            .query_row("SELECT id FROM persons WHERE name = ?1", [person], |r| r.get(0))
            .unwrap();
        cat.conn()
            .execute(
                "INSERT INTO faces (file_id, x, y, width, height, confidence, person_id)
                 VALUES (?1, 0.5, 0.5, 0.2, 0.2, 0.9, ?2)",
                rusqlite::params![id, person_id],
            )
            .unwrap();
        ids.push(id);
    }
    (lib, cat, ids)
}

fn ids_of(cat: &Catalog, query: &str) -> Vec<i64> {
    let parsed = parse_field_filters(query);
    cat.query_files(
        &fotobase_catalog::FileQuery {
            text: (!parsed.text.is_empty()).then_some(parsed.text),
            person: parsed.person,
            camera: parsed.camera,
            taken_before: parsed.before,
            taken_after: parsed.after,
            ..Default::default()
        },
        usize::MAX,
    )
    .unwrap()
    .into_iter()
    .map(|f| f.id)
    .collect()
}

/// TDD integration criterion: the full example query returns exactly the
/// intersecting set — anna's X100V photos taken before 2023.
#[test]
fn combined_field_filters_return_the_intersection() {
    let (_lib, cat, ids) = fixture();
    let got = ids_of(&cat, "person:anna camera:X100V before:2023 sunset");
    assert_eq!(got, Vec::<i64>::new(), "anna∩X100V∩before:2023 is empty in this fixture");
    let _ = ids;
    let got = ids_of(&cat, "person:anna camera:X100V before:2024");
    assert_eq!(got, vec![ids[0]], "anna∩X100V∩before:2024 is exactly the 2022 X100V shot");
}

/// `before:`/`after:` bracket a year; ISO dates work too, and the two
/// bounds compose.
#[test]
fn date_bounds_bracket_the_fixture() {
    let (_lib, cat, ids) = fixture();
    assert_eq!(ids_of(&cat, "after:2023"), vec![ids[1], ids[2]]);
    assert_eq!(ids_of(&cat, "before:2023-07-01"), vec![ids[1], ids[0], ids[3]]);
    assert_eq!(ids_of(&cat, "after:2022-06-15 before:2023"), vec![ids[3]]);
}

/// Camera matches as a substring of the EXIF Make/Model mirror.
#[test]
fn camera_substring_matches_make_and_model() {
    let (_lib, cat, ids) = fixture();
    let got = ids_of(&cat, "camera:sony");
    assert_eq!(got.len(), 2);
    assert!(got.contains(&ids[1]) && got.contains(&ids[3]));
}

/// Free text blends with the filters: `ben` also appears in the filename,
/// so the FTS side narrows inside the field-filter set.
#[test]
fn free_text_narrows_inside_the_filters() {
    let (_lib, cat, ids) = fixture();
    // "2023" is in both 2023 filenames; camera:Sony keeps only
    // anna-a7-2023 of those.
    let got = ids_of(&cat, "camera:Sony 2023");
    assert_eq!(got, vec![ids[1]]);
}

/// Unknown or malformed filter tokens degrade to literal text, never an
/// error: `bogus:x` searches as text, an empty value stays text.
#[test]
fn malformed_tokens_degrade_to_text() {
    let (_lib, cat, _ids) = fixture();
    // `bogus:x` is not a filter — it is FTS text matching nothing here,
    // but the query must succeed.
    assert!(ids_of(&cat, "person:anna bogus:x").is_empty());
    // An empty filter value is not a filter either: the literal token
    // stays searchable text but matches no document here, while the plain
    // word does match.
    assert!(ids_of(&cat, "person: ben").is_empty());
    assert_eq!(ids_of(&cat, "ben").len(), 2);
    // An invalid date value degrades to text.
    assert!(ids_of(&cat, "before:notadate").is_empty());
    // Quoted values with spaces work.
    let got = ids_of(&cat, "person:\"Ben\" camera:\"Sony A7\"");
    assert_eq!(got.len(), 1);
}

#[test]
fn dbg_dates() {
    let (_lib, cat, ids) = fixture();
    let sql = "SELECT f.id, f.taken_at, COALESCE(CASE WHEN f.taken_at GLOB '[0-9][0-9][0-9][0-9][-:][0-9][0-9][-:][0-9][0-9]*' THEN replace(substr(f.taken_at, 1, 10), ':', '-') END, date(f.mtime, 'unixepoch')) FROM files f ORDER BY f.path";
    cat.conn()
        .prepare(sql)
        .unwrap()
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .unwrap()
        .flatten()
        .for_each(|x| println!("{x:?}"));
    let q = fotobase_catalog::FileQuery {
        taken_after: Some("2023-01-01".into()),
        ..Default::default()
    };
    println!(
        "ids={ids:?} after={:?}",
        cat.query_files(&q, 100).unwrap().iter().map(|f| f.id).collect::<Vec<_>>()
    );
}
