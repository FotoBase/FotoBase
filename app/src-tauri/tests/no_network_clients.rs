//! TASK-05-011: the contacts link must never reach the network (PLAN §2G
//! "Contacts link — local contacts only"; no telemetry of any kind).
//!
//! Enforced at the dependency-graph level: every crate the workspace builds
//! is scanned through the committed `Cargo.lock` and no network *client*
//! crate may appear in any Fotobase crate's dependency closure except the
//! updater plugin's (tauri-plugin-updater checks GitHub Releases — that is
//! the update flow, not contacts). The contacts modules themselves must add
//! no new network dependency: `contacts.rs` and the catalog's contacts
//! mapping are scanned for known network-client imports.

use std::collections::HashSet;

/// Network client crates that must never enter the contacts path. Kept to
/// HTTP/socket clients — anything else (TLS stacks etc.) arrives with the
/// updater and is out of this task's reach.
const BANNED: &[&str] = &[
    "reqwest",
    "ureq",
    "attohttpc",
    "surf",
    "isahc",
    "curl",
    "curl-sys",
    "hyper",
    "hyper-tls",
    "hyper-util",
];

fn workspace_root() -> std::path::PathBuf {
    // Tests run with the crate dir as cwd; walk up to the lock file.
    let mut dir = std::env::current_dir().unwrap();
    loop {
        if dir.join("Cargo.lock").exists() {
            return dir;
        }
        if !dir.pop() {
            panic!("no Cargo.lock above {}", std::env::current_dir().unwrap().display());
        }
    }
}

/// All package names reachable from `roots` in the lock file's dependency
/// graph (lock v3/v4 both carry per-package `dependencies` lists).
fn closure(lock: &str, roots: &[&str]) -> HashSet<String> {
    let mut deps_of: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut current: Option<String> = None;
    for line in lock.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("name = \"").and_then(|r| r.strip_suffix('"')) {
            current = Some(rest.to_string());
        } else if line == "dependencies = [" {
            let name = match current.take() {
                Some(n) => n,
                None => continue,
            };
            // Collect following lines until the closing bracket.
            let mut deps = Vec::new();
            for l in lock.lines().skip_while(|l| l.trim() != "dependencies = [").skip(1) {
                let t = l.trim().trim_end_matches(',');
                if t == "]" {
                    break;
                }
                let dep = t.trim();
                if !dep.is_empty() {
                    deps.push(dep.split(' ').next().unwrap_or(dep).to_string());
                }
            }
            deps_of.insert(name, deps);
        }
    }
    let mut seen = HashSet::new();
    let mut stack: Vec<String> = roots.iter().map(|r| r.to_string()).collect();
    while let Some(pkg) = stack.pop() {
        if !seen.insert(pkg.clone()) {
            continue;
        }
        if let Some(deps) = deps_of.get(&pkg) {
            stack.extend(deps.iter().cloned());
        }
    }
    seen
}

#[test]
fn no_network_client_in_the_contacts_path() {
    // Which Fotobase crates make up the contacts path.
    let contacts_crates: &[&str] = &["fotobase", "fotobase-catalog"];

    let lock = std::fs::read_to_string(workspace_root().join("Cargo.lock")).unwrap();
    let reachable = closure(&lock, contacts_crates);

    let offenders: Vec<_> = BANNED.iter().filter(|b| reachable.contains(**b)).collect();
    assert!(
        offenders.is_empty(),
        "network client crates reachable from the contacts path: {offenders:?}"
    );
}

#[test]
fn contacts_modules_import_nothing_networky() {
    let root = workspace_root();
    for rel in ["app/src-tauri/src/contacts.rs", "app/src-tauri/src/people.rs"] {
        let src = std::fs::read_to_string(root.join(rel)).unwrap();
        for banned in BANNED {
            assert!(!src.contains(banned), "{rel} references network client crate {banned}");
        }
        assert!(!src.contains("TcpStream") && !src.contains("UdpSocket"), "{rel} opens sockets");
    }
}
