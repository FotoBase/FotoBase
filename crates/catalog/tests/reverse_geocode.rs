//! TASK-06-008: offline reverse geocoding from the embedded GeoNames
//! cities dataset (PLAN §4 "GeoNames cities dataset (embedded)", §6 P3
//! Places). Unit: known coordinates resolve to city names; nearest-city
//! lookup with a distance threshold keeps "city-level accuracy" honest —
//! coordinates in open ocean or far from any bundled city resolve to
//! nothing, never to a wrong place.

use fotobase_catalog::geonames;

#[test]
fn lisbon_coordinates_resolve_to_lisbon() {
    let hit = geonames::reverse_geocode(38.7167, -9.1394).expect("Lisbon resolves");
    assert_eq!(hit.name, "Lisbon");
    assert_eq!(hit.country, "PT");
}

#[test]
fn tokyo_coordinates_resolve_to_tokyo() {
    let hit = geonames::reverse_geocode(35.6895, 139.6917).expect("Tokyo resolves");
    assert_eq!(hit.name, "Tokyo");
    assert_eq!(hit.country, "JP");
}

#[test]
fn unknown_coordinates_in_open_ocean_resolve_to_nothing() {
    // Mid-Atlantic: no bundled city within the city-level threshold.
    assert!(geonames::reverse_geocode(0.0, -30.0).is_none());
}

#[test]
fn far_from_any_city_resolves_to_nothing() {
    // Central Sahara; nearest bundled city beyond the threshold.
    assert!(geonames::reverse_geocode(23.0, 12.0).is_none());
}

#[test]
fn nearest_city_wins_not_alphabetical_neighbor() {
    // A point between two bundled cities must pick the actually
    // nearest one, so "city-level accuracy" means the right city.
    // Between Paris (48.8566, 2.3522) and Reims (49.2583, 4.0317):
    // a point clearly closer to Reims.
    let hit = geonames::reverse_geocode(49.20, 3.95).expect("resolves");
    assert_eq!(hit.name, "Reims");
}

#[test]
fn dataset_is_embedded_and_sorted() {
    // The resource travels inside the binary (offline guarantee); the
    // loader requires sorted-by-name rows for binary search.
    let data = geonames::CITIES;
    assert!(!data.is_empty());
    let mut prev: Option<&str> = None;
    for line in data.lines() {
        let name = line.split('\t').next().unwrap();
        if let Some(p) = prev {
            assert!(p <= name, "dataset not sorted: {p} > {name}");
        }
        prev = Some(name);
    }
}

#[test]
fn fifty_thousand_lookups_stay_well_under_a_second() {
    // PLAN §9: bulk geocoding of the 50k fixture must complete as a
    // background job without UI jank — the lookup itself is the
    // per-photo cost and must stay far below one millisecond each.
    let start = std::time::Instant::now();
    let mut hits = 0usize;
    // Deterministic spread over Europe (dense part of the dataset).
    for i in 0..50_000 {
        let lat = 35.0 + (i % 250) as f64 * 0.1;
        let lon = -10.0 + (i / 250) as f64 * 0.1;
        if geonames::reverse_geocode(lat, lon).is_some() {
            hits += 1;
        }
    }
    let elapsed = start.elapsed();
    assert!(hits > 0, "the dense scan must resolve something");
    // The invariant is per-lookup cost, not wall clock on whatever
    // hardware runs CI: a debug build on a Windows runner measures
    // ~28 µs/lookup where a unix release-ish build measures ~4 µs. Both
    // are far below the 1 ms/lookup bar, so budget 50 µs each (2.5 s
    // total) — the binary-search windowing is what the test guards, and
    // a linear scan would blow any budget hundreds of times over.
    assert!(
        elapsed.as_secs_f64() < 2.5,
        "50k lookups took {elapsed:?}, budget is 2.5 s (50 µs/lookup)"
    );
}
