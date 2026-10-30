//! Catalog registry (TASK-03-024, PLAN §6 P2a): the small JSON list of
//! catalog locations that lives *outside* any catalog, under the app data
//! dir. Each entry points at a per-catalog directory that holds that
//! catalog's database, thumbnail cache, and (later) ML models — PLAN §1's
//! "Catalog" decision, so switching isolates caches, watchers, and state
//! completely.
//!
//! The default entry points at the data dir itself, so existing
//! single-catalog installs (`data_dir/catalog.db` + `data_dir/thumbnails`)
//! keep their exact layout with no migration step.
//!
//! Writes follow the repo's atomic-replace discipline (`.tmp` + rename, same
//! as settings.json and the sidecar writer): a crash mid-save can never
//! truncate the registry. A missing or corrupt file falls back to the
//! default registry — never an error dialog.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One registered catalog: a display name and the directory holding its
/// database, thumbnail cache, and models.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub name: String,
    pub dir: PathBuf,
}

/// The registry file's on-disk shape (`catalogs.json` under the data dir).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    pub catalogs: Vec<CatalogEntry>,
    /// Name of the catalog to reopen on launch (functional spec:
    /// "last-used catalog reopens on launch").
    pub last_used: Option<String>,
}

/// Why a registry operation was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("catalog name must not be empty")]
    EmptyName,
    #[error("a catalog named {0:?} already exists")]
    DuplicateName(String),
    #[error("no catalog named {0:?}")]
    UnknownCatalog(String),
    #[error("creating catalog directory failed: {0}")]
    Io(String),
}

/// The database file inside a catalog directory.
pub fn catalog_db_path(dir: &Path) -> PathBuf {
    dir.join("catalog.db")
}

/// The thumbnail cache directory inside a catalog directory.
pub fn catalog_cache_dir(dir: &Path) -> PathBuf {
    dir.join("thumbnails")
}

impl Registry {
    /// The registry every fresh install starts with: a single `Default`
    /// catalog rooted at the app data dir (backward-compatible layout).
    pub fn default_for(data_dir: &Path) -> Self {
        Registry {
            catalogs: vec![CatalogEntry {
                name: "Default".to_string(),
                dir: data_dir.to_path_buf(),
            }],
            last_used: Some("Default".to_string()),
        }
    }

    /// Load `catalogs.json`; a missing or corrupt file falls back to the
    /// defaults. A dangling `last_used` resolves to the first catalog.
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join("catalogs.json");
        let parsed = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Registry>(&bytes).ok())
            .filter(|r| !r.catalogs.is_empty());
        match parsed {
            Some(mut r) => {
                let known =
                    r.last_used.as_deref().is_some_and(|n| r.catalogs.iter().any(|c| c.name == n));
                if !known {
                    r.last_used = Some(r.catalogs[0].name.clone());
                }
                r
            }
            None => Registry::default_for(data_dir),
        }
    }

    /// Atomically persist to `data_dir/catalogs.json` (`.tmp` + rename).
    pub fn save(&self, data_dir: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self).expect("registry serializes");
        let tmp = data_dir.join("catalogs.json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, data_dir.join("catalogs.json"))
    }

    pub fn get(&self, name: &str) -> Option<&CatalogEntry> {
        self.catalogs.iter().find(|c| c.name == name)
    }

    /// Register a new catalog and create its directory. Names are unique
    /// case-insensitively (the switcher UI would otherwise show two entries
    /// that differ only in case).
    pub fn create(&mut self, name: &str, dir: &Path) -> Result<&CatalogEntry, RegistryError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(RegistryError::EmptyName);
        }
        if self.catalogs.iter().any(|c| c.name.eq_ignore_ascii_case(name)) {
            return Err(RegistryError::DuplicateName(name.to_string()));
        }
        std::fs::create_dir_all(dir).map_err(|e| RegistryError::Io(e.to_string()))?;
        self.catalogs.push(CatalogEntry { name: name.to_string(), dir: dir.to_path_buf() });
        Ok(self.catalogs.last().expect("just pushed"))
    }

    /// Point the registry at `name` for the next launch. The caller performs
    /// the actual switch (stop watchers, flush the queue, close the DB,
    /// reopen); this only records the selection.
    pub fn switch(&mut self, name: &str) -> Result<&CatalogEntry, RegistryError> {
        match self.catalogs.iter().position(|c| c.name == name) {
            Some(_) => {
                self.last_used = Some(name.to_string());
                Ok(self.get(name).expect("position found"))
            }
            None => Err(RegistryError::UnknownCatalog(name.to_string())),
        }
    }

    /// The entry to open at launch: last-used if it still exists, else the
    /// first registered catalog.
    pub fn entry_for_launch(&self) -> &CatalogEntry {
        let idx = self
            .last_used
            .as_deref()
            .and_then(|n| self.catalogs.iter().position(|c| c.name == n))
            .unwrap_or(0);
        &self.catalogs[idx]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_for_launch_prefers_last_used() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        let mut r = Registry::default_for(dir.path());
        r.create("Work", &work).unwrap();
        r.switch("Work").unwrap();
        assert_eq!(r.entry_for_launch().name, "Work");
    }

    #[test]
    fn entry_for_launch_falls_back_to_first() {
        let mut r = Registry {
            catalogs: vec![
                CatalogEntry { name: "A".into(), dir: PathBuf::from("/a") },
                CatalogEntry { name: "B".into(), dir: PathBuf::from("/b") },
            ],
            last_used: Some("Gone".into()),
        };
        // Dangling last_used still loads resolved via `load`; direct
        // construction keeps the dangling value, so `switch` errors but the
        // launch entry falls back to the first catalog.
        assert!(r.switch("Gone").is_err());
        assert_eq!(r.entry_for_launch().name, "A");
    }
}
