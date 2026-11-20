//! TDD tests for TASK-08-007: plugin registry lists registered plugins with
//! their declared capabilities (PLAN §6 P4).

use fotobase_export::mock::{MockBehavior, MockEndpoint, MockPlugin};
use fotobase_export::{ExportError, PluginCapabilities, Registry};

fn plugin(id: &str, name: &str, capabilities: PluginCapabilities) -> MockPlugin {
    MockPlugin::new(MockEndpoint::default(), MockBehavior::default()).with_info(
        id,
        name,
        capabilities,
    )
}

#[test]
fn lists_registered_plugins_with_declared_capabilities() {
    let mut registry = Registry::new();
    registry
        .register(Box::new(plugin(
            "s3",
            "S3-compatible",
            PluginCapabilities { albums: false, size_choice: true },
        )))
        .unwrap();
    registry
        .register(Box::new(plugin(
            "immich",
            "Immich",
            PluginCapabilities { albums: true, size_choice: true },
        )))
        .unwrap();

    let listed = registry.list();
    let ids: Vec<_> = listed.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["immich", "s3"], "listing is sorted by id");
    assert_eq!(listed[0].name, "Immich");
    assert!(listed[0].capabilities.albums && listed[0].capabilities.size_choice);
    assert!(!listed[1].capabilities.albums && listed[1].capabilities.size_choice);
}

#[test]
fn duplicate_plugin_id_is_rejected() {
    let mut registry = Registry::new();
    registry.register(Box::new(plugin("s3", "S3", PluginCapabilities::default()))).unwrap();
    let err = registry
        .register(Box::new(plugin("s3", "Other S3", PluginCapabilities::default())))
        .unwrap_err();
    assert!(matches!(err, ExportError::DuplicatePlugin(id) if id == "s3"));
    assert_eq!(registry.list().len(), 1);
}

#[test]
fn lookup_by_id() {
    let mut registry = Registry::new();
    registry
        .register(Box::new(plugin("local", "Local folder", PluginCapabilities::default())))
        .unwrap();
    assert!(registry.get_mut("local").is_some());
    assert!(registry.get_mut("missing").is_none());
}
