//! TASK-04-002: versioning + forward migration of edit recipes
//! (docs/edit-recipe-spec-v1.md §4, PLAN §5).

use fotobase_core::recipe::{migrate, RECIPE_SPEC, RECIPE_VERSION};

#[test]
fn a_v1_recipe_migrates_to_the_current_version_with_equivalent_output() {
    let raw = r#"{"spec":"fotobase.recipe","version":1,
        "ops":[{"type":"sepia","params":{"amount":0.5}},
               {"type":"crop","params":{"x":0.1,"y":0.1,"w":0.5,"h":0.5}}]}"#;
    let (migrated, warnings) = migrate(raw).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(migrated.version, RECIPE_VERSION);
    assert_eq!(migrated.spec, RECIPE_SPEC);
    assert_eq!(migrated.ops.len(), 2);
    assert_eq!(migrated.ops[0].op_type, "sepia");
    assert_eq!(migrated.ops[0].params.as_ref().unwrap()["amount"], serde_json::json!(0.5));
    assert_eq!(migrated.ops[1].op_type, "crop");
    // Migrated output applies equivalently: parse(migrated JSON) == migrated.
    assert_eq!(fotobase_core::recipe::parse(&migrated.to_json()).unwrap().0, migrated);
}

#[test]
fn a_migrated_recipe_serializes_at_the_current_version() {
    let (migrated, _) = migrate(r#"{"spec":"fotobase.recipe","version":1,"ops":[]}"#).unwrap();
    let json = migrated.to_json();
    assert!(json.contains(&format!(r#""version":{RECIPE_VERSION}"#)), "{json}");
    assert!(json.contains(r#""spec":"fotobase.recipe""#), "{json}");
}

#[test]
fn migration_skips_unknown_ops_with_a_warning_but_never_fails() {
    let raw = r#"{"spec":"fotobase.recipe","version":1,
        "ops":[{"type":"sepia"},{"type":"future_op","params":{"x":1}}]}"#;
    let (migrated, warnings) = migrate(raw).unwrap();
    assert_eq!(migrated.ops.len(), 1);
    assert_eq!(migrated.ops[0].op_type, "sepia");
    assert!(warnings.iter().any(|w| w.contains("future_op")), "{warnings:?}");
}

#[test]
fn a_recipe_from_a_newer_spec_version_is_rejected_not_migrated() {
    let err = migrate(r#"{"spec":"fotobase.recipe","version":99,"ops":[]}"#).unwrap_err();
    assert!(err.to_string().contains("99"), "{err}");
}

#[test]
fn a_recipe_with_a_wrong_spec_discriminator_is_rejected() {
    assert!(migrate(r#"{"spec":"other.app","version":1,"ops":[]}"#).is_err());
    assert!(migrate("not json").is_err());
}

#[test]
fn an_empty_recipe_migrates_and_round_trips() {
    let (migrated, warnings) =
        migrate(r#"{"spec":"fotobase.recipe","version":1,"ops":[]}"#).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(fotobase_core::recipe::parse(&migrated.to_json()).unwrap().0, migrated);
}
