//! TASK-06-008 regression: the full native chain the reverse-geocode e2e
//! exercises — a scanned sidecar library (GPS in `exif:GPSLatitude`
//! attributes, plain decimal degrees) lands in the `metadata` mirror via
//! the scan's sidecar sync, then `reverse_geocode_all` fills `places` +
//! the FTS `place` column, so search "Lisbon" finds the photos.

use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use fotobase_catalog::Catalog;
use fotobase_scanner::scan_folder;

const GEOTAGGED: usize = 10;

fn sidecar(lat: f64, lon: f64) -> String {
    format!(
        r#"<?xpacket begin="﻿" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:exif="http://ns.adobe.com/exif/1.0/"
   exif:GPSLatitude="{lat}"
   exif:GPSLongitude="{lon}">
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>
"#
    )
}

fn make_geotagged_library(root: &Path) {
    fs::create_dir_all(root).unwrap();
    // Lisbon's own dataset coordinates with a < 0.5 km cluster: the
    // cities15000 dump contains Lisbon neighborhoods (Arroios, Areeiro,
    // Alvalade…) as separate cities, and a loose grid around the center
    // resolves to them, not to Lisbon.
    let (center_lat, center_lon, spread) = (38.72509_f64, -9.1498_f64, 0.001_f64);
    for i in 0..GEOTAGGED {
        let lat = center_lat + (i % 8) as f64 * spread;
        let lon = center_lon + (i / 8) as f64 * spread;
        let name = format!("geo_{i:03}.jpg");
        fs::write(root.join(&name), vec![0xff; 16]).unwrap();
        fs::write(root.join(format!("{name}.xmp")), sidecar(lat, lon)).unwrap();
    }
    // A plain photo without a sidecar: must stay placeless.
    fs::write(root.join("plain_000.jpg"), vec![0xff; 16]).unwrap();
}

#[test]
fn scanned_sidecar_gps_resolves_to_places_and_fts() {
    let tmp = tempfile::tempdir().unwrap();
    make_geotagged_library(tmp.path());
    let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();

    let report = scan_folder(&mut cat, tmp.path(), &AtomicBool::new(false)).unwrap();
    assert_eq!(report.added, GEOTAGGED + 1, "every photo scanned");

    // The scan's sidecar sync mirrored the GPS into `metadata`.
    let with_gps: i64 = cat
        .conn()
        .query_row("SELECT COUNT(*) FROM metadata WHERE latitude IS NOT NULL", [], |r| r.get(0))
        .unwrap();
    assert_eq!(with_gps as usize, GEOTAGGED, "sidecar GPS mirrored into metadata");
    let geocoded = cat.reverse_geocode_all(&|_, _| {}).unwrap();
    assert_eq!(geocoded, GEOTAGGED, "every geotagged photo resolves to Lisbon");

    let hits = cat.search_files("Lisbon", 100).unwrap();
    assert_eq!(hits.len(), GEOTAGGED, "FTS finds every Lisbon photo");
}
