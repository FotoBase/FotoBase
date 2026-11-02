//! TASK-04-019: near-duplicate grouping in the catalog.
//!
//! The per-file perceptual hash lives in `files.phash` — derived data
//! (PLAN §5 Authority), recomputable, never written to sidecars. Grouping is
//! union-find over the Hamming distance of 64-bit hashes; every group holds
//! 2+ members.

use fotobase_catalog::{group_by_phash, Catalog, DEFAULT_DUPLICATE_THRESHOLD_BITS};

/// A 64-bit hash with `ones` high bits set, for deterministic Hamming tests.
fn hash_with_high_bits(ones: u32) -> [u8; 8] {
    let mut h = [0u8; 8];
    for i in 0..ones.min(64) {
        h[(i / 8) as usize] |= 1 << (i % 8);
    }
    h
}

fn hamming(a: &[u8; 8], b: &[u8; 8]) -> u32 {
    a.iter().zip(b).map(|(x, y)| (x ^ y).count_ones()).sum()
}

#[test]
fn variants_of_one_photo_group_together() {
    let base = hash_with_high_bits(4);
    // Two near-variants within the default threshold, one far outlier.
    let mut v1 = base;
    v1[0] ^= 0b1; // 1 bit away
    let far = [0xFF; 8];

    let hashes = vec![(1, base), (2, v1), (3, far)];
    let groups = group_by_phash(&hashes, DEFAULT_DUPLICATE_THRESHOLD_BITS);
    assert_eq!(groups.len(), 1, "only the pair groups");
    assert_eq!(groups[0].file_ids, vec![1, 2]);
}

#[test]
fn transitive_similarity_merges_the_whole_cluster() {
    // a ~ b (1 bit), b ~ c (1 bit) — a is 2 bits from c, all under threshold.
    let a = hash_with_high_bits(2);
    let mut b = a;
    b[0] ^= 0b1;
    let mut c = b;
    c[0] ^= 0b10;

    let groups = group_by_phash(&[(1, a), (2, b), (3, c)], DEFAULT_DUPLICATE_THRESHOLD_BITS);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].file_ids, vec![1, 2, 3]);
}

#[test]
fn threshold_zero_only_exact_matches_group() {
    let a = hash_with_high_bits(8);
    let mut b = a;
    b[7] ^= 0x80; // exactly 1 bit apart
    let same = a;
    let groups = group_by_phash(&[(1, a), (2, b), (3, same)], 0);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].file_ids, vec![1, 3]);
}

#[test]
fn singletons_produce_no_groups() {
    let a = hash_with_high_bits(1);
    let b = hash_with_high_bits(40);
    assert!(group_by_phash(&[(1, a), (2, b)], DEFAULT_DUPLICATE_THRESHOLD_BITS).is_empty());
}

#[test]
fn groups_are_deterministic_and_sorted() {
    let base = hash_with_high_bits(4);
    let mut v = base;
    v[0] ^= 0b1;
    let mut rev = vec![(3, v), (1, base), (2, base)];
    let first = group_by_phash(&rev, DEFAULT_DUPLICATE_THRESHOLD_BITS);
    rev.reverse();
    let second = group_by_phash(&rev, DEFAULT_DUPLICATE_THRESHOLD_BITS);
    assert_eq!(first[0].file_ids, second[0].file_ids, "input order must not matter");
    assert_eq!(first[0].file_ids, vec![1, 2, 3], "ids ascending inside the group");
}

// --- catalog round-trip ------------------------------------------------

fn catalog_with_photos(n: usize) -> (tempfile::TempDir, Catalog) {
    let tmp = tempfile::tempdir().unwrap();
    let photos = tmp.path().join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();
    let folder = cat.seed_folder(photos.to_str().unwrap()).unwrap();
    for i in 0..n {
        let name = format!("p{i}.jpg");
        std::fs::write(photos.join(&name), b"x").unwrap();
        cat.conn()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
                 VALUES (?1, ?2, ?3, 1, 0.0, 'image')",
                rusqlite::params![folder, photos.join(&name).to_str().unwrap(), name],
            )
            .unwrap();
    }
    (tmp, cat)
}

#[test]
fn phash_roundtrip_and_grouping_from_the_db() {
    let (_tmp, cat) = catalog_with_photos(3);

    // Nothing hashed yet: no groups, and all three files are candidates.
    assert!(cat.duplicate_groups(DEFAULT_DUPLICATE_THRESHOLD_BITS).unwrap().is_empty());
    assert_eq!(cat.phash_candidates().unwrap().len(), 3);

    let files = cat.list_files(usize::MAX).unwrap();
    let mut ids: Vec<i64> = files.iter().map(|f| f.id).collect();
    ids.sort_unstable();
    let h = hash_with_high_bits(4);
    cat.set_phash(ids[0], &h).unwrap();
    cat.set_phash(ids[1], &h).unwrap();
    let mut other = hash_with_high_bits(40);
    other[7] ^= 0xFF;
    cat.set_phash(ids[2], &other).unwrap();

    assert!(cat.phash_candidates().unwrap().is_empty(), "all hashed");

    let groups = cat.duplicate_groups(DEFAULT_DUPLICATE_THRESHOLD_BITS).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].file_ids, vec![ids[0], ids[1]]);
    assert_eq!(groups[0].filenames, vec!["p0.jpg", "p1.jpg"]);
}

#[test]
fn videos_are_never_hash_candidates() {
    let (_tmp, cat) = catalog_with_photos(0);
    let folder = cat.list_folders().unwrap()[0].id;
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
             VALUES (?1, ?2, 'v.mp4', 1, 0.0, 'video')",
            rusqlite::params![folder, "v.mp4"],
        )
        .unwrap();
    assert!(cat.phash_candidates().unwrap().is_empty());
}

#[test]
fn hamming_helper_is_sane() {
    assert_eq!(hamming(&hash_with_high_bits(64), &[0; 8]), 64);
    assert_eq!(hamming(&hash_with_high_bits(4), &hash_with_high_bits(4)), 0);
}

/// Perf (TASK-04-019, PLAN §9): grouping a 50k-photo catalog must stay
/// interactive — the finder runs as a background job while the UI scrolls.
/// Realistic shape: mostly distinct hashes, sparse planted duplicate
/// clusters (61 clusters of 5 spread through the corpus).
#[test]
fn grouping_50k_hashes_stays_fast() {
    // Deterministic spread: widely-varied bytes, no two alike.
    let h_for = |i: i64| -> [u8; 8] {
        let mut h = [0u8; 8];
        let mut x = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15u64);
        for b in h.iter_mut() {
            x = x.wrapping_mul(6364136223846793005u64).wrapping_add(1442695040888963407u64);
            *b = (x >> 33) as u8;
        }
        h
    };
    let mut hashes: Vec<(i64, [u8; 8])> = (0i64..50_000).map(|i| (i, h_for(i))).collect();
    // Planted clusters: 5 near-identical variants sharing a base hash.
    let mut expected_groups = 0usize;
    for c in 0..50i64 {
        let base = h_for(100_000 + c);
        for v in 0..5 {
            let mut h = base;
            if v > 0 {
                h[v as usize] ^= 0b11; // ≤4 bits apart, one cluster
            }
            hashes[(990 * c + v) as usize].1 = h;
        }
        expected_groups += 1;
    }
    let start = std::time::Instant::now();
    let groups = group_by_phash(&hashes, DEFAULT_DUPLICATE_THRESHOLD_BITS);
    let elapsed = start.elapsed();
    assert_eq!(groups.len(), expected_groups, "every planted cluster recalled");
    assert!(
        elapsed.as_millis() < 5_000,
        "50k grouping took {elapsed:?} — too slow for the background job"
    );
}
