// No JS-callable commands: the app crate drives the share sheet from Rust,
// so there is nothing to put behind a capability.
fn main() {
    tauri_plugin::Builder::new(&[]).android_path("android").ios_path("ios").build();
}
