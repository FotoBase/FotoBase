//! TASK-03-023: portable export of user data (PLAN §5 "Portability
//! guarantee"): albums + tags + recipes to JSON/CSV at any time. The JSON is
//! validated against the published schemas in `docs/`, and a parser test
//! round-trips it back into equivalent structures.

use std::path::{Path, PathBuf};

use fotobase_catalog::portable::{ExportFormat, ExportSections, PortableExport, EXPORT_SPEC};
use fotobase_catalog::{sidecar_path, Catalog, CatalogError};
use fotobase_metadata::FB_NS;
use serde_json::Value;

fn docs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Serves the in-repo recipe schema for its published `$id`, so the export
/// schema's `$ref` resolves offline.
struct RecipeSchema(Value);

impl jsonschema::Retrieve for RecipeSchema {
    fn retrieve(
        &self,
        uri: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        if uri.as_str() == "https://fotobase.github.io/ns/recipe-v1.schema.json" {
            Ok(self.0.clone())
        } else {
            Err(format!("unexpected schema reference: {uri}").into())
        }
    }
}

fn export_validator() -> jsonschema::Validator {
    let recipe = read_json(&docs().join("recipe-v1.schema.json"));
    let export = read_json(&docs().join("portable-export-v1.schema.json"));
    jsonschema::options().with_retriever(RecipeSchema(recipe)).build(&export).unwrap()
}

fn recipe_validator() -> jsonschema::Validator {
    jsonschema::validator_for(&read_json(&docs().join("recipe-v1.schema.json"))).unwrap()
}

fn assert_valid(validator: &jsonschema::Validator, doc: &Value) {
    let errors: Vec<String> = validator.iter_errors(doc).map(|e| e.to_string()).collect();
    assert!(errors.is_empty(), "schema errors: {errors:#?}\n{doc:#}");
}

struct Lib {
    dir: tempfile::TempDir,
    cat: Catalog,
}

impl Lib {
    fn photo(&mut self, name: &str) -> i64 {
        let original = self.dir.path().join(name);
        std::fs::write(&original, format!("fake image bytes of {name}")).unwrap();
        let folder_id = self.cat.seed_folder(self.dir.path().to_str().unwrap()).unwrap();
        self.cat
            .conn()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
                 VALUES (?1, ?2, ?3, 10, 0.0, 'image')",
                rusqlite::params![folder_id, original.to_str().unwrap(), name],
            )
            .unwrap();
        self.cat.conn().last_insert_rowid()
    }

    fn path(&self, name: &str) -> String {
        self.dir.path().join(name).to_str().unwrap().to_string()
    }

    /// Write a sidecar carrying `fb:Recipe` (must precede other writes: it
    /// replaces the sidecar) and let the catalog pick it up.
    fn recipe(&mut self, id: i64, name: &str, json: &str) {
        let xmp = format!(
            r#"<?xml version="1.0"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:fb="{FB_NS}"><fb:Recipe>{json}</fb:Recipe></rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#
        );
        std::fs::write(sidecar_path(&self.path(name)), xmp).unwrap();
        self.cat.sync_file(id).unwrap();
    }
}

const SEPIA: &str = r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"crop","params":{"x":0.1,"y":0.1,"w":0.8,"h":0.8}},{"type":"sepia"}]}"#;
/// Packets written before the `spec` discriminator was pinned.
const LEGACY: &str = r#"{"version":1,"ops":[{"type":"grayscale"}]}"#;

/// Fixture library: two photos with recipes (one legacy), tags, two albums
/// (one with a CSV-hostile name), an empty album, a hidden photo, and two
/// recipes that cannot be exported as conforming v1 recipes.
fn fixture() -> Lib {
    let dir = tempfile::tempdir().unwrap();
    let cat = Catalog::open(dir.path().join("catalog.db")).unwrap();
    let mut l = Lib { dir, cat };

    let a = l.photo("a.jpg");
    let b = l.photo("b.jpg");
    let c = l.photo("c.jpg");
    let d = l.photo("d.jpg");
    let e = l.photo("e.jpg");
    l.recipe(a, "a.jpg", SEPIA);
    l.recipe(b, "b.jpg", LEGACY);
    l.recipe(d, "d.jpg", "not json at all");
    l.recipe(e, "e.jpg", r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"warp"}]}"#);

    l.cat.add_tag(a, "sea").unwrap();
    l.cat.add_tag(a, "family").unwrap();
    l.cat.add_tag(b, "sea").unwrap();
    l.cat.add_tag(c, r#"say "cheese""#).unwrap();

    let summer = l.cat.create_album("Summer", "2024-07-14", Some("Lisbon"), None).unwrap();
    let trip = l
        .cat
        .create_album("Trip, \"2024\"", "2024-03-01", None, Some("line one\nline two"))
        .unwrap();
    l.cat.create_album("Empty", "2023-01-01", None, None).unwrap();
    l.cat.add_to_album(summer.id, &[b, a]).unwrap();
    l.cat.add_to_album(trip.id, &[c]).unwrap();
    l.cat.hide_photos(&[c]).unwrap();
    l
}

const ALL: ExportSections = ExportSections { albums: true, tags: true, recipes: true };

#[test]
fn json_export_validates_against_the_published_schema() {
    let l = fixture();
    let out = tempfile::tempdir().unwrap();

    let report = l.cat.export_user_data(out.path(), ALL, ExportFormat::Json).unwrap();

    assert_eq!(report.files.len(), 1, "{report:?}");
    let doc = read_json(&report.files[0]);
    assert_valid(&export_validator(), &doc);
    let recipes = recipe_validator();
    for entry in doc["recipes"].as_array().unwrap() {
        assert_valid(&recipes, &entry["recipe"]);
    }
    assert_eq!(doc["spec"], EXPORT_SPEC);
    assert_eq!(doc["version"], 1);
}

#[test]
fn json_export_round_trips_into_equivalent_structures() {
    let l = fixture();
    let out = tempfile::tempdir().unwrap();
    let report = l.cat.export_user_data(out.path(), ALL, ExportFormat::Json).unwrap();

    let parsed =
        PortableExport::from_json(&std::fs::read_to_string(&report.files[0]).unwrap()).unwrap();

    let mut expected = l.cat.portable_export(ALL).unwrap();
    expected.exported_at.clone_from(&parsed.exported_at);
    assert_eq!(parsed, expected);
    let albums = parsed.albums.unwrap();
    let names: Vec<&str> = albums.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["Summer", "Trip, \"2024\"", "Empty"], "album-list order");
    assert_eq!(albums[0].members, [l.path("b.jpg"), l.path("a.jpg")], "added order");
    assert_eq!(albums[0].place.as_deref(), Some("Lisbon"));
    assert_eq!(albums[0].id, l.cat.list_albums().unwrap()[0].uuid, "stable fb:Albums id");
    assert!(albums[2].members.is_empty());

    let tags = parsed.tags.unwrap();
    let sea = tags.iter().find(|t| t.name == "sea").unwrap();
    assert_eq!(sea.files, [l.path("a.jpg"), l.path("b.jpg")]);

    let recipes = parsed.recipes.unwrap();
    let files: Vec<&str> = recipes.iter().map(|r| r.file.as_str()).collect();
    assert_eq!(files, [l.path("a.jpg"), l.path("b.jpg")]);
    assert_eq!(recipes[0].recipe, serde_json::from_str::<Value>(SEPIA).unwrap());
}

#[test]
fn a_legacy_recipe_without_the_spec_discriminator_gains_it() {
    let l = fixture();

    let export = l.cat.portable_export(ALL).unwrap();

    let b = &export.recipes.unwrap()[1];
    assert_eq!(b.recipe["spec"], "fotobase.recipe");
    assert_eq!(b.recipe["ops"][0]["type"], "grayscale");
}

#[test]
fn recipes_that_are_not_conforming_v1_are_kept_raw_not_dropped() {
    let l = fixture();

    let export = l.cat.portable_export(ALL).unwrap();

    let bad: Vec<(&str, &str)> =
        export.unreadable_recipes.iter().map(|u| (u.file.as_str(), u.raw.as_str())).collect();
    let (d, e) = (l.path("d.jpg"), l.path("e.jpg"));
    assert_eq!(
        bad,
        [
            (d.as_str(), "not json at all"),
            (e.as_str(), r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"warp"}]}"#),
        ]
    );
    assert!(export.unreadable_recipes.iter().all(|u| !u.reason.is_empty()));
}

#[test]
fn hidden_photos_are_still_exported() {
    let l = fixture();

    let export = l.cat.portable_export(ALL).unwrap();

    let trip = &export.albums.unwrap()[1];
    assert_eq!(trip.members, [l.path("c.jpg")]);
    assert!(export.tags.unwrap().iter().any(|t| t.name == r#"say "cheese""#));
}

#[test]
fn only_the_picked_sections_are_exported() {
    let l = fixture();
    let out = tempfile::tempdir().unwrap();
    let tags_only = ExportSections { albums: false, tags: true, recipes: false };

    let report = l.cat.export_user_data(out.path(), tags_only, ExportFormat::Json).unwrap();

    let doc = read_json(&report.files[0]);
    assert!(doc.get("albums").is_none() && doc.get("recipes").is_none(), "{doc:#}");
    assert!(doc.get("unreadable_recipes").is_none());
    assert!(doc["tags"].is_array());
    assert_valid(&export_validator(), &doc);
    assert_eq!((report.albums, report.tags, report.recipes), (0, 3, 0));
}

#[test]
fn exporting_nothing_is_refused() {
    let l = fixture();
    let out = tempfile::tempdir().unwrap();
    let none = ExportSections { albums: false, tags: false, recipes: false };

    let err = l.cat.export_user_data(out.path(), none, ExportFormat::Csv).unwrap_err();

    assert!(matches!(err, CatalogError::Export(_)), "{err:?}");
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
}

#[test]
fn a_missing_destination_folder_is_an_error() {
    let l = fixture();
    let out = tempfile::tempdir().unwrap();

    let err =
        l.cat.export_user_data(&out.path().join("nope"), ALL, ExportFormat::Json).unwrap_err();

    assert!(matches!(err, CatalogError::Export(_)), "{err:?}");
}

fn read_csv(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn csv_export_writes_one_quoted_table_per_section() {
    let l = fixture();
    let out = tempfile::tempdir().unwrap();

    let report = l.cat.export_user_data(out.path(), ALL, ExportFormat::Csv).unwrap();

    let names: Vec<String> =
        report.files.iter().map(|p| p.file_name().unwrap().to_str().unwrap().to_string()).collect();
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(names[0].ends_with("-albums.csv"), "{names:?}");
    assert!(names[1].ends_with("-tags.csv"), "{names:?}");
    assert!(names[2].ends_with("-recipes.csv"), "{names:?}");

    let albums = read_csv(&report.files[0]);
    let mut lines = albums.split("\r\n");
    assert_eq!(
        lines.next().unwrap(),
        "album_id,album_name,album_date,place,description,position,filename,path"
    );
    let a = l.path("a.jpg");
    assert!(albums.contains(&format!(",Summer,2024-07-14,Lisbon,,2,a.jpg,{a}\r\n")), "{albums}");
    assert!(
        albums.contains(",\"Trip, \"\"2024\"\"\",2024-03-01,,\"line one\nline two\",1,c.jpg,"),
        "RFC 4180 quoting: {albums}"
    );
    assert!(albums.contains(",Empty,2023-01-01,,,,,\r\n"), "empty album keeps a row: {albums}");

    let tags = read_csv(&report.files[1]);
    assert!(tags.starts_with("tag,filename,path\r\n"), "{tags}");
    assert!(tags.contains(&format!("sea,a.jpg,{a}\r\n")), "{tags}");
    assert!(tags.contains("\"say \"\"cheese\"\"\",c.jpg,"), "{tags}");

    let recipes = read_csv(&report.files[2]);
    assert!(recipes.starts_with("filename,path,recipe\r\n"), "{recipes}");
    assert_eq!(recipes.matches("\r\n").count(), 3, "header + 2 conforming recipes: {recipes}");
    let row = recipes.split("\r\n").nth(1).unwrap();
    let cell = row.split_once(",\"").unwrap().1.strip_suffix('"').unwrap().replace("\"\"", "\"");
    let recipe: Value = serde_json::from_str(&cell).unwrap();
    assert_valid(&recipe_validator(), &recipe);
}

#[test]
fn export_is_read_only_for_sidecars_and_catalog() {
    let l = fixture();
    let out = tempfile::tempdir().unwrap();
    let sidecar = sidecar_path(&l.path("a.jpg"));
    let before = std::fs::read(&sidecar).unwrap();
    let mut catalog_before = l.cat.portable_export(ALL).unwrap();

    l.cat.export_user_data(out.path(), ALL, ExportFormat::Json).unwrap();
    l.cat.export_user_data(out.path(), ALL, ExportFormat::Csv).unwrap();

    assert_eq!(std::fs::read(&sidecar).unwrap(), before);
    let after = l.cat.portable_export(ALL).unwrap();
    catalog_before.exported_at.clone_from(&after.exported_at);
    assert_eq!(after, catalog_before);
}

#[test]
fn a_second_export_never_overwrites_the_first() {
    let l = fixture();
    let out = tempfile::tempdir().unwrap();

    let first = l.cat.export_user_data(out.path(), ALL, ExportFormat::Json).unwrap();
    let second = l.cat.export_user_data(out.path(), ALL, ExportFormat::Json).unwrap();

    assert_ne!(first.files, second.files);
    assert!(first.files[0].exists() && second.files[0].exists());
    let leftovers: Vec<_> = std::fs::read_dir(out.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn the_documented_example_validates_and_parses() {
    let example = docs().join("portable-export-v1.example.json");

    let doc = read_json(&example);

    assert_valid(&export_validator(), &doc);
    PortableExport::from_json(&std::fs::read_to_string(&example).unwrap()).unwrap();
}
