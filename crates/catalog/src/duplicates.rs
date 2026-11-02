//! Near-duplicate grouping (TASK-04-019, PLAN §2 2026 additions "Near-
//! duplicate detection", §6 P2b).
//!
//! Grouping is pure: union-find over the Hamming distance of the per-file
//! perceptual hashes stored in `files.phash` (derived data — the catalog is
//! a rebuildable cache, PLAN §5). Nothing here deletes anything: the finder
//! only *suggests*; a delete routes through the OS-trash path on explicit
//! user confirmation (§2 File menu "Delete from disk").

use serde::Serialize;

use crate::Catalog;
use crate::Result;

use rusqlite::OptionalExtension;

/// Default Hamming-distance threshold over the 64-bit hash. Resize and
/// re-compress moves a mean hash by a couple of bits; distinct photos sit
/// far above this. Conservative on purpose: a false merge shows a user a
/// photo inside the wrong group, a missed merge just hides a duplicate.
pub const DEFAULT_DUPLICATE_THRESHOLD_BITS: u32 = 6;

/// One suggested near-duplicate group: the file ids keep/suggest-delete acts
/// on, with display names (and paths for the delete step).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DuplicateGroup {
    /// Ascending; the review UI keys off these.
    pub file_ids: Vec<i64>,
    pub paths: Vec<String>,
    pub filenames: Vec<String>,
    /// Content hashes backing `thumb://` URIs; `None` until the lazy hash
    /// backfill fills them in (PLAN §10 "hash lazily").
    pub hashes: Vec<Option<String>>,
}

/// Pure union-find grouping over `(file_id, hash)` pairs in any order.
/// Only groups with two or more members are returned; ids inside a group
/// are ascending and the group list is deterministic for a given input set.
///
/// Candidate generation is pigeonhole bucketing, not O(n²): the 64-bit hash
/// is split into 8 chunks of 8 bits, and two hashes within `threshold`
/// (which must stay below 8) must agree on at least `8 − threshold` whole
/// chunks — below threshold 7 that is two or more, so every candidate pair
/// shares one of the C(8, 2) chunk-pair keys. Only those pairs are
/// Hamming-compared. Exact-identical hashes are unioned once up front; the
/// union-find absorbs repeated candidate pairs. Transitivity keeps clusters
/// whole even when two members share no chunk pair directly.
pub fn group_by_phash(hashes: &[(i64, [u8; 8])], threshold: u32) -> Vec<DuplicateGroup> {
    assert!(threshold < 8, "the pigeonhole index requires threshold < 8");
    let n = hashes.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }

    fn union(parent: &mut [usize], a: usize, b: usize) {
        let (ra, rb) = (find(parent, a), find(parent, b));
        if ra != rb {
            // Attach the larger root under the smaller for determinism.
            let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
            parent[hi] = lo;
        }
    }

    // Exact duplicates first, compared once per distinct hash value.
    let mut seen: std::collections::HashMap<[u8; 8], usize> = Default::default();
    for (i, (_, h)) in hashes.iter().enumerate() {
        if let Some(&first) = seen.get(h) {
            union(&mut parent, first, i);
        } else {
            seen.insert(*h, i);
        }
    }

    // Chunk-pair keys (or single chunks when threshold 7 forces the wide
    // index): chunk positions → value → indices carrying that value.
    let shared_chunks = 8u32.saturating_sub(threshold).max(1) as usize;
    let key_positions: Vec<[usize; 2]> = if shared_chunks == 1 {
        (0..8).map(|p| [p, p]).collect()
    } else {
        let mut pairs = Vec::new();
        for a in 0..8 {
            for b in (a + 1)..8 {
                pairs.push([a, b]);
            }
        }
        pairs
    };

    let mut buckets: std::collections::HashMap<([u8; 2], [u8; 2]), Vec<usize>> = Default::default();
    for (i, (_, h)) in hashes.iter().enumerate() {
        for key in &key_positions {
            let k = ([key[0] as u8, key[1] as u8], [h[key[0]], h[key[1]]]);
            buckets.entry(k).or_default().push(i);
        }
    }

    // Candidate pairs share a key; the union-find and the compared-set
    // absorb the repeats that the overlapping keys produce.
    let mut compared: std::collections::HashSet<(usize, usize)> = Default::default();
    for members in buckets.values() {
        if members.len() < 2 {
            continue;
        }
        for (a, &i) in members.iter().enumerate() {
            for &j in &members[a + 1..] {
                if find(&mut parent, i) == find(&mut parent, j) {
                    continue;
                }
                if !compared.insert((i.min(j), i.max(j))) {
                    continue;
                }
                let d: u32 =
                    hashes[i].1.iter().zip(&hashes[j].1).map(|(a, b)| (a ^ b).count_ones()).sum();
                if d <= threshold {
                    union(&mut parent, i, j);
                }
            }
        }
    }

    let mut clusters: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..n {
        clusters.entry(find(&mut parent, i)).or_default().push(i);
    }

    let mut groups: Vec<Vec<usize>> = clusters.into_values().filter(|c| c.len() >= 2).collect();
    // Deterministic order: by the group's smallest member index.
    groups.sort_by_key(|c| c[0]);

    groups
        .into_iter()
        .map(|members| {
            let mut rows: Vec<i64> = members.into_iter().map(|i| hashes[i].0).collect();
            rows.sort_unstable();
            DuplicateGroup {
                file_ids: rows,
                paths: Vec::new(),
                filenames: Vec::new(),
                hashes: Vec::new(),
            }
        })
        .collect()
}

impl Catalog {
    /// A file's stored path, `None` when the row is gone.
    pub fn file_path(&self, file_id: i64) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| r.get(0))
            .optional()?)
    }

    /// Files still missing a perceptual hash: images only, oldest first, so
    /// a cancelled pass leaves the newest photos for the next one.
    pub fn phash_candidates(&self) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path FROM files WHERE media_type = 'image' AND phash IS NULL ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Store a computed hash. Derived data only — safe to overwrite on a
    /// re-run (e.g. after a re-scan or a hash-algorithm change).
    pub fn set_phash(&self, file_id: i64, hash: &[u8; 8]) -> Result<()> {
        self.conn.execute(
            "UPDATE files SET phash = ?1 WHERE id = ?2",
            rusqlite::params![hash, file_id],
        )?;
        Ok(())
    }

    /// Group the catalog's hashed images. `threshold` is the Hamming
    /// distance below-or-equal at which two hashes count as near-duplicates.
    pub fn duplicate_groups(&self, threshold: u32) -> Result<Vec<DuplicateGroup>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, filename, phash, hash FROM files
             WHERE media_type = 'image' AND phash IS NOT NULL",
        )?;
        type HashRow = (i64, String, String, [u8; 8], Option<String>);
        let hashes: Vec<HashRow> = stmt
            .query_map([], |r| {
                let blob: Vec<u8> = r.get(3)?;
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, blob, r.get(4)?))
            })?
            .filter_map(|r| {
                let (id, path, filename, blob, content_hash) = r.ok()?;
                let hash: [u8; 8] = blob.try_into().ok()?;
                Some((id, path, filename, hash, content_hash))
            })
            .collect();

        let pairs: Vec<(i64, [u8; 8])> = hashes.iter().map(|(id, _, _, h, _)| (*id, *h)).collect();
        let mut groups = group_by_phash(&pairs, threshold);
        // Fill the display fields from the id → (path, filename) map.
        let lookup: std::collections::HashMap<i64, (String, String, Option<String>)> = hashes
            .into_iter()
            .map(|(id, path, filename, _, content)| (id, (path, filename, content)))
            .collect();
        for group in &mut groups {
            group.paths = group
                .file_ids
                .iter()
                .map(|id| lookup.get(id).map(|(p, _, _)| p.clone()).unwrap_or_default())
                .collect();
            group.filenames = group
                .file_ids
                .iter()
                .map(|id| lookup.get(id).map(|(_, f, _)| f.clone()).unwrap_or_default())
                .collect();
            group.hashes = group
                .file_ids
                .iter()
                .map(|id| lookup.get(id).and_then(|(_, _, h)| h.clone()))
                .collect();
        }
        Ok(groups)
    }
}
