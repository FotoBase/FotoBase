//! `basemap://` URI scheme resolution (TASK-06-001, PLAN §4 "MapLibre GL JS
//! + Protomaps PMTiles", ADR-0008).
//!
//! The map panel gets its tiles from a local PMTiles archive through the
//! custom scheme handler — no tile server, no network access, ever. The
//! bundled low-zoom world basemap ships inside the installer; a larger
//! regional archive the user opted into downloading replaces it at runtime
//! via [`BasemapPaths::set_override`].
//!
//! Like `thumb.rs`, the pure, app-free core (URI parsing, archive
//! resolution) lives here so it can be tested without a Tauri runtime;
//! the registration in `lib.rs` is a thin adapter.
//!
//! Requests are Web-Mercator XYZ addresses, optionally with a `.mvt`
//! extension (`basemap://z/x/y` or `basemap://localhost/z/x/y.mvt` — the
//! Windows WebView2 spelling maps custom schemes to
//! `http://<scheme>.localhost/`; both are accepted, mirroring `thumb.rs`).
//! Tiles are served raw MVT (`application/vnd.mapbox-vector-tile`), gzip
//! decoded once — MapLibre's `pmtiles://` protocol plugin would decompress
//! per fetch, and the scheme handler is the single reader.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use pmtiles::{AsyncPmTilesReader, MmapBackend};
use tauri::http::Uri;

/// The scheme name the frontend addresses tiles with.
pub const BASEMAP_PROTOCOL: &str = "basemap";

/// Content type served for every tile: raw Mapbox Vector Tile.
pub const MVT_CONTENT_TYPE: &str = "application/vnd.mapbox-vector-tile";

/// A resolved `basemap://` response: an HTTP-shaped reply the Tauri scheme
/// handler turns into a response without any IPC involvement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasemapReply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

/// Empty 404 for an unknown tile — the map just skips it, never an error
/// dialog (same degrade-don't-error rule as the thumbnail scheme).
fn not_found() -> BasemapReply {
    BasemapReply { status: 404, content_type: MVT_CONTENT_TYPE, body: Vec::new() }
}

/// A parsed `basemap://` request: the Web-Mercator tile address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BasemapRequest {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

/// Parse a scheme request URI into a tile address.
///
/// Accepts both WebView spellings of a custom scheme (mirroring `thumb.rs`):
/// - `basemap://z/x/y` (macOS/Linux: the address is the authority — `http`
///   splits it as host `z` + path `/x/y`),
/// - `http://basemap.localhost/z/x/y` (Windows WebView2),
/// - `basemap://localhost/z/x/y` (defensive).
///
/// A trailing `.mvt` is accepted and ignored; other extensions are
/// rejected — only raw tiles are served. A query string is ignored.
/// Out-of-range coordinates (`x` or `y` outside `[0, 2^z)`, `z` above 30)
/// are `None`, not an error: the map never builds those, so anything else
/// is a stale or malformed request.
pub fn request_from_uri(uri: &Uri) -> Option<BasemapRequest> {
    let host = uri.host().unwrap_or("");
    let path = uri.path().trim_start_matches('/');
    // The direct form carries the address in the authority plus path; the
    // localhost forms carry it entirely in the path.
    let address = if host == "basemap.localhost" || host == "localhost" {
        std::borrow::Cow::Borrowed(path)
    } else if host.is_empty() {
        return None;
    } else {
        std::borrow::Cow::Owned(format!("{host}/{path}"))
    };
    address_from_str(&address)
}

/// Parse the `z/x/y` (optionally `z/x/y.ext`) address.
fn address_from_str(address: &str) -> Option<BasemapRequest> {
    let address = address.split('?').next().unwrap_or(address);
    let address = match address.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("mvt") => stem,
        Some(_) => return None,
        None => address,
    };
    let mut parts = address.splitn(3, '/');
    let z: u8 = parts.next()?.parse().ok()?;
    let x: u32 = parts.next()?.parse().ok()?;
    let y: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || z > 30 {
        return None;
    }
    let span = 1u64 << z;
    if u64::from(x) >= span || u64::from(y) >= span {
        return None;
    }
    Some(BasemapRequest { z, x, y })
}

/// Where the archive is: the bundled world basemap (resource, always
/// present) and the user-consented regional download (catalog-adjacent,
/// replaces the bundle per tile when present). Resolved once at startup;
/// the override is a later opt-in download landing in place.
#[derive(Debug, Clone)]
pub struct BasemapPaths {
    /// The installer's bundled low-zoom world basemap.
    pub bundled: PathBuf,
    /// The user's opt-in high-zoom regional archive; `None` until one is
    /// downloaded (ADR-0008: downloads are explicit, never silent).
    pub regional: Option<PathBuf>,
}

impl BasemapPaths {
    /// Paths for a bundled resource with no regional download yet.
    pub fn bundled_only(bundled: PathBuf) -> Self {
        Self { bundled, regional: None }
    }

    /// The archive a tile request resolves against: the regional download
    /// when present (ADR-0008), else the bundled world basemap. The tile
    /// address is part of the resolution seam (a future bundle split can
    /// route by zoom) but bundles are whole-file today.
    pub fn archive_for(&self, _req: &BasemapRequest) -> &Path {
        match &self.regional {
            Some(regional) => regional,
            None => &self.bundled,
        }
    }
}

/// The process-wide, swappable archive location. The scheme handler reads
/// it per request, so a regional download that lands while the map is open
/// is picked up without a restart.
static BASEMAP_PATHS: RwLock<Option<BasemapPaths>> = RwLock::new(None);

/// Install (or replace) the archive paths. Startup calls this once; the
/// regional-download flow calls it again when a bundle lands.
pub fn set_paths(paths: BasemapPaths) {
    *BASEMAP_PATHS.write().expect("basemap paths poisoned") = Some(paths);
}

/// The installed paths, if startup ran. `None` means the scheme handler
/// answers 404 for everything (browser dev / test harness without a bundle).
pub fn paths() -> Option<BasemapPaths> {
    BASEMAP_PATHS.read().expect("basemap paths poisoned").clone()
}

/// Resolve one tile request against the archive: mmap the file, look the
/// address up in the PMTiles directory, gzip-decode the MVT. A missing
/// tile (outside the bundle's zoom range or coverage) is a 404 — the map
/// renders neighboring content and never errors. An unreadable archive is
/// also a 404: the basemap is decorative, an error dialog would be worse.
pub async fn resolve(req: &BasemapRequest, path: &Path) -> BasemapReply {
    let Ok(backend) = MmapBackend::try_from(path).await else {
        return not_found();
    };
    let Ok(reader) = AsyncPmTilesReader::try_from_source(backend).await else {
        return not_found();
    };
    let Ok(coord) = pmtiles::TileCoord::new(req.z, req.x, req.y) else {
        return not_found();
    };
    match reader.get_tile_decompressed(coord).await {
        Ok(Some(tile)) => {
            BasemapReply { status: 200, content_type: MVT_CONTENT_TYPE, body: tile.to_vec() }
        }
        _ => not_found(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Option<BasemapRequest> {
        request_from_uri(&s.parse::<Uri>().expect("test uri"))
    }

    #[test]
    fn parses_direct_form() {
        assert_eq!(uri("basemap://0/0/0"), Some(BasemapRequest { z: 0, x: 0, y: 0 }));
        assert_eq!(uri("basemap://4/8/5"), Some(BasemapRequest { z: 4, x: 8, y: 5 }));
        assert_eq!(uri("basemap://4/8/5?r=1"), Some(BasemapRequest { z: 4, x: 8, y: 5 }));
    }

    #[test]
    fn parses_webview2_localhost_form() {
        assert_eq!(
            uri("http://basemap.localhost/4/8/5"),
            Some(BasemapRequest { z: 4, x: 8, y: 5 })
        );
        assert_eq!(uri("basemap://localhost/2/1/1"), Some(BasemapRequest { z: 2, x: 1, y: 1 }));
    }

    #[test]
    fn accepts_mvt_extension_only() {
        assert_eq!(uri("basemap://4/8/5.mvt"), Some(BasemapRequest { z: 4, x: 8, y: 5 }));
        assert_eq!(uri("basemap://4/8/5.png"), None);
        assert_eq!(uri("basemap://4/8/5.txt"), None);
    }

    #[test]
    fn rejects_malformed_addresses() {
        assert_eq!(uri("basemap://4/8"), None);
        assert_eq!(uri("basemap://4/8/5/9"), None);
        assert_eq!(uri("basemap://x/8/5"), None);
        assert_eq!(uri("basemap://-1/0/0"), None);
        // Out-of-range coordinates for the zoom level.
        assert_eq!(uri("basemap://2/4/0"), None);
        assert_eq!(uri("basemap://2/0/4"), None);
        // z=30 is the last representable zoom; anything above is rejected.
        assert_eq!(uri("basemap://31/0/0"), None);
        assert!(uri("basemap://30/0/0").is_some());
    }

    #[test]
    fn rejects_unrelated_authorities() {
        // A non-basemap authority with a path is another scheme's request.
        assert_eq!(uri("http://thumb.localhost/abc"), None);
    }

    #[test]
    fn paths_prefer_the_regional_download() {
        let bundled = BasemapPaths::bundled_only(PathBuf::from("/bundled/world.pmtiles"));
        assert_eq!(
            bundled.archive_for(&BasemapRequest { z: 0, x: 0, y: 0 }),
            Path::new("/bundled/world.pmtiles")
        );

        let with_regional = BasemapPaths {
            bundled: PathBuf::from("/bundled/world.pmtiles"),
            regional: Some(PathBuf::from("/data/europe.pmtiles")),
        };
        assert_eq!(
            with_regional.archive_for(&BasemapRequest { z: 10, x: 512, y: 341 }),
            Path::new("/data/europe.pmtiles")
        );
    }

    #[test]
    fn paths_swap_is_visible_to_later_requests() {
        set_paths(BasemapPaths::bundled_only(PathBuf::from("/a.pmtiles")));
        assert_eq!(paths().unwrap().bundled, PathBuf::from("/a.pmtiles"));
        set_paths(BasemapPaths {
            bundled: PathBuf::from("/a.pmtiles"),
            regional: Some(PathBuf::from("/b.pmtiles")),
        });
        assert_eq!(paths().unwrap().regional, Some(PathBuf::from("/b.pmtiles")));
    }

    #[tokio::test]
    async fn missing_archive_is_404_never_error() {
        let reply =
            resolve(&BasemapRequest { z: 0, x: 0, y: 0 }, Path::new("/nonexistent.pmtiles")).await;
        assert_eq!(reply.status, 404);
        assert!(reply.body.is_empty());
    }

    #[tokio::test]
    async fn resolves_tiles_from_a_real_archive() {
        // The committed world bundle, or a locally regenerated one; both are
        // valid PMTiles v3 MVT archives. Skipped when neither exists (the
        // bundle is regenerated by tools/basemap/build.sh).
        let bundle =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/basemap/world.pmtiles");
        if !bundle.exists() {
            eprintln!("skipping: no bundled basemap at {}", bundle.display());
            return;
        }
        // z0 has exactly one tile and the world bundle always covers it.
        let reply = resolve(&BasemapRequest { z: 0, x: 0, y: 0 }, &bundle).await;
        assert_eq!(reply.status, 200, "z0 tile must resolve from the bundle");
        assert_eq!(reply.content_type, MVT_CONTENT_TYPE);
        assert!(!reply.body.is_empty());
        // Raw MVT starts with its layer count varint — gzip bytes would mean
        // the decode step did not run (MapLibre cannot read gzipped MVT).
        assert_ne!(reply.body[0], 0x1f, "tile still gzipped");
        // A tile outside the bundle's zoom range (z>4) is a clean 404.
        let far = resolve(&BasemapRequest { z: 10, x: 512, y: 341 }, &bundle).await;
        assert_eq!(far.status, 404);
    }
}
