//! The devices this one has paired with.
//!
//! Only paired devices may pull or push metadata. The set (and the last
//! address each peer announced) can be persisted so pairing survives a
//! restart; like every other cache it never holds user data, only who to
//! talk to.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use iroh::{EndpointAddr, EndpointId};
use iroh_tickets::{endpoint::EndpointTicket, Ticket};
use serde::{Deserialize, Serialize};

use crate::SyncError;

#[derive(Serialize, Deserialize)]
struct Entry {
    id: String,
    /// Last address the peer announced, as an iroh endpoint ticket.
    addr: Option<String>,
}

/// Paired devices, optionally backed by a JSON file.
#[derive(Debug, Default)]
pub struct PairedDevices {
    peers: BTreeMap<EndpointId, Option<EndpointAddr>>,
    path: Option<PathBuf>,
}

impl PairedDevices {
    /// In-memory only.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load `path` (an empty set when it does not exist yet) and write every
    /// later change back to it.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SyncError> {
        let path = path.as_ref().to_path_buf();
        let mut peers = BTreeMap::new();
        match std::fs::read(&path) {
            Ok(bytes) => {
                let entries: Vec<Entry> = serde_json::from_slice(&bytes).map_err(store_err)?;
                for entry in entries {
                    let id: EndpointId = entry.id.parse().map_err(store_err)?;
                    let addr = entry
                        .addr
                        .and_then(|s| s.parse::<EndpointTicket>().ok())
                        .map(EndpointAddr::from);
                    peers.insert(id, addr);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(store_err(e)),
        }
        Ok(Self { peers, path: Some(path) })
    }

    pub fn contains(&self, id: &EndpointId) -> bool {
        self.peers.contains_key(id)
    }

    pub fn ids(&self) -> Vec<EndpointId> {
        self.peers.keys().copied().collect()
    }

    /// How to reach `id`: the last address it announced, or just its id.
    pub fn addr(&self, id: &EndpointId) -> EndpointAddr {
        match self.peers.get(id) {
            Some(Some(addr)) => addr.clone(),
            _ => EndpointAddr::new(*id),
        }
    }

    /// Pair with `id`, or refresh its address. A refresh without an address
    /// keeps the one already known.
    pub fn insert(&mut self, id: EndpointId, addr: Option<EndpointAddr>) -> Result<(), SyncError> {
        let slot = self.peers.entry(id).or_insert(None);
        if addr.is_some() {
            *slot = addr;
        }
        self.save()
    }

    pub fn remove(&mut self, id: &EndpointId) -> Result<bool, SyncError> {
        let removed = self.peers.remove(id).is_some();
        self.save()?;
        Ok(removed)
    }

    fn save(&self) -> Result<(), SyncError> {
        let Some(path) = &self.path else { return Ok(()) };
        let entries: Vec<Entry> = self
            .peers
            .iter()
            .map(|(id, addr)| Entry {
                id: id.to_string(),
                addr: addr.clone().map(|a| EndpointTicket::new(a).encode_string()),
            })
            .collect();
        let bytes = serde_json::to_vec_pretty(&entries).map_err(store_err)?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes).map_err(store_err)?;
        std::fs::rename(&tmp, path).map_err(store_err)
    }
}

fn store_err(e: impl std::fmt::Display) -> SyncError {
    SyncError::Store(e.to_string())
}
