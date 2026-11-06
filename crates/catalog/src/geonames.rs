//! Embedded GeoNames cities dataset and offline reverse geocoding
//! (TASK-06-008, PLAN §4 "GeoNames cities dataset (embedded)", §6 P3
//! Places). The dataset is bundled inside the binary — the reverse
//! geocoding must work fully offline — and carries only name, latitude,
//! longitude and country per city (GeoNames, CC BY 4.0).
//!
//! `reverse_geocode` is a nearest-city lookup with a distance threshold:
//! a photo resolves to a city name only when a bundled city is within
//! [`MAX_DISTANCE_KM`], which keeps "city-level accuracy" honest — open
//! ocean or remote wilderness resolves to nothing rather than to a
//! wrong place.

use crate::Catalog;
use rusqlite::OptionalExtension;

/// The bundled dataset: `name<TAB>lat<TAB>lon<TAB>country<TAB>place_id`,
/// sorted by name for binary search. ~34k cities ≥ 15k population,
/// ~1 MB embedded.
pub const CITIES: &str = include_str!("../resources/geonames/cities.txt");

/// Maximum city-to-photo distance for a "city-level accuracy" hit,
/// in kilometres. Beyond this the photo's place is simply unknown.
pub const MAX_DISTANCE_KM: f64 = 50.0;

/// One city row of the embedded dataset.
#[derive(Debug, Clone, PartialEq)]
pub struct City {
    pub name: &'static str,
    pub latitude: f64,
    pub longitude: f64,
    pub country: &'static str,
    pub id: i64,
}

/// A reverse-geocode hit: the nearest city within the threshold.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaceHit {
    pub name: &'static str,
    pub country: &'static str,
    pub latitude: f64,
    pub longitude: f64,
    pub distance_km: f64,
}

fn parse() -> &'static Cities {
    use std::sync::OnceLock;
    static CITIES_PARSED: OnceLock<Cities> = OnceLock::new();
    CITIES_PARSED.get_or_init(|| {
        let mut rows: Vec<City> = CITIES
            .lines()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                let name = fields.next()?;
                let latitude = fields.next()?.parse().ok()?;
                let longitude = fields.next()?.parse().ok()?;
                let country = fields.next()?;
                let id = fields.next()?.parse().ok()?;
                Some(City { name, latitude, longitude, country, id })
            })
            .collect();
        // Latitude-sorted for a binary-search window: a 50 km radius is
        // ±0.45° of latitude, so only a few hundred of the ~34k rows are
        // ever haversine-checked (the 50k bulk pass stays well under a
        // second — PLAN §9).
        rows.sort_unstable_by(|a, b| a.latitude.total_cmp(&b.latitude));
        let lat_min = rows.first().map_or(0.0, |c| c.latitude);
        let lat_max = rows.last().map_or(0.0, |c| c.latitude);
        Cities { rows, lat_min, lat_max }
    })
}

/// The parsed dataset: latitude-sorted rows plus the span, for
/// `partition_point` windowing in [`reverse_geocode`].
struct Cities {
    rows: Vec<City>,
    lat_min: f64,
    lat_max: f64,
}

/// All bundled cities (lazily parsed once, latitude-sorted).
pub fn cities() -> &'static [City] {
    &parse().rows
}

/// Great-circle distance in kilometres (haversine; the dataset is small
/// and lookups are rare relative to scans, so no spatial index needed —
/// the whole pass over ~34k rows takes well under a millisecond).
fn distance_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_KM: f64 = 6371.0;
    let (lat1, lon1, lat2, lon2) =
        (lat1.to_radians(), lon1.to_radians(), lat2.to_radians(), lon2.to_radians());
    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_KM * a.sqrt().asin()
}

/// Half the latitude window: 50 km ≈ 0.45° of latitude, padded. Degrees
/// of longitude shrink by cos(latitude), so a longitude prefilter
/// (`LON_WINDOW_DEG * cos(lat)`) prunes most of the window's rows before
/// the haversine.
const LAT_WINDOW_DEG: f64 = 0.5;
const LON_WINDOW_DEG: f64 = 0.6;

/// Nearest city to `(latitude, longitude)` within [`MAX_DISTANCE_KM`],
/// or `None` when no bundled city is close enough.
pub fn reverse_geocode(latitude: f64, longitude: f64) -> Option<PlaceHit> {
    let cities = parse();
    if latitude < cities.lat_min - LAT_WINDOW_DEG || latitude > cities.lat_max + LAT_WINDOW_DEG {
        return None;
    }
    // Binary-search the latitude window, prune by the longitude window
    // (cos-shrunk), then haversine-check the survivors.
    let lo = cities.rows.partition_point(|c| c.latitude < latitude - LAT_WINDOW_DEG);
    let hi = cities.rows.partition_point(|c| c.latitude <= latitude + LAT_WINDOW_DEG);
    let lon_window = LON_WINDOW_DEG / latitude.cos().max(0.01);
    let mut best: Option<(f64, &City)> = None;
    for city in &cities.rows[lo..hi] {
        if (city.longitude - longitude).abs() > lon_window {
            continue;
        }
        let d = distance_km(latitude, longitude, city.latitude, city.longitude);
        if d <= MAX_DISTANCE_KM && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, city));
        }
    }
    let (d, city) = best?;
    Some(PlaceHit {
        name: city.name,
        country: city.country,
        latitude: city.latitude,
        longitude: city.longitude,
        distance_km: d,
    })
}

/// The geo cell of a coordinate pair: a 0.1° grid cell, so photos in the
/// same neighbourhood share one `places` row. Returned as a stable,
/// human-readable string key.
fn geo_cell(latitude: f64, longitude: f64) -> String {
    format!("{:.1}:{:.1}", (latitude * 10.0).round() / 10.0, (longitude * 10.0).round() / 10.0)
}

/// Reverse geocode every photo with a complete GPS mirror that has no
/// place yet, and drop the place of photos whose GPS disappeared
/// (the mirror follows the sidecar, the source of truth — PLAN §5).
/// `progress(done, total)` fires per photo for the job-queue event.
///
/// Returns the number of photos whose place *resolved* (an unresolvable
/// coordinate pair is processed but not counted).
pub fn reverse_geocode_all(
    cat: &mut Catalog,
    progress: &dyn Fn(usize, usize),
) -> Result<usize, crate::CatalogError> {
    // Photos needing work: GPS set but no place, or place set but no GPS.
    let pending: Vec<i64> = {
        let conn = cat.conn();
        let mut stmt = conn.prepare(
            "SELECT f.id FROM files f JOIN metadata m ON m.file_id = f.id \
             WHERE (m.latitude IS NOT NULL AND m.longitude IS NOT NULL AND f.place_id IS NULL) \
                OR ((m.latitude IS NULL OR m.longitude IS NULL) AND f.place_id IS NOT NULL) \
             ORDER BY f.id",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.flatten().collect()
    };
    let total = pending.len();
    let mut changed = 0usize;
    for (done, file_id) in pending.into_iter().enumerate() {
        let gps: Option<(f64, f64)> = cat
            .conn()
            .query_row(
                "SELECT latitude, longitude FROM metadata WHERE file_id = ?1",
                [file_id],
                |r| {
                    let lat: Option<f64> = r.get(0)?;
                    let lon: Option<f64> = r.get(1)?;
                    Ok(lat.zip(lon))
                },
            )
            .optional()?
            .flatten();
        let place = gps.and_then(|(lat, lon)| reverse_geocode(lat, lon));
        let tx = cat.conn_mut().unchecked_transaction()?;
        match place {
            Some(hit) => {
                changed += 1;
                let cell = geo_cell(hit.latitude, hit.longitude);
                tx.execute(
                    "INSERT INTO places (geo_cell, name, country, latitude, longitude)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(geo_cell) DO NOTHING",
                    rusqlite::params![cell, hit.name, hit.country, hit.latitude, hit.longitude],
                )?;
                let place_id: i64 =
                    tx.query_row("SELECT id FROM places WHERE geo_cell = ?1", [&cell], |r| {
                        r.get(0)
                    })?;
                tx.execute(
                    "UPDATE files SET place_id = ?1 WHERE id = ?2",
                    rusqlite::params![place_id, file_id],
                )?;
            }
            None => {
                tx.execute("UPDATE files SET place_id = NULL WHERE id = ?1", [file_id])?;
            }
        }
        tx.commit()?;
        progress(done + 1, total);
    }
    Ok(changed)
}

impl Catalog {
    /// See [`reverse_geocode_all`] — the inherent-method form the job
    /// queue and tests call.
    pub fn reverse_geocode_all(&mut self, progress: &dyn Fn(usize, usize)) -> crate::Result<usize> {
        reverse_geocode_all(self, progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geo_cell_is_stable_per_tenth_degree() {
        assert_eq!(geo_cell(38.7167, -9.1394), geo_cell(38.72, -9.14));
        assert_ne!(geo_cell(38.7167, -9.1394), geo_cell(38.8167, -9.1394));
    }
}
