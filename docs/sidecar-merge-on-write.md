# Sidecar merge-on-write & move/rename re-association — design doc

**Task:** 00-010 · **Status:** approved 2026-10-20 · **Gates:** EPIC-03 (all sidecar-writing tasks) · **Plan refs:** §5 (authority split, conflict rule, rebuild), §10 (sidecar-conflict risk), §11 Q2/Q5/Q6

This is the highest data-loss-risk area of the app. Everything here is normative for the `metadata` crate (03-001) and the sync engine (03-002).

---

## 1. Scope & principles

1. **Original files are read-only** except explicit "Save" with the opt-in embed setting (§11 Q2: sidecars only by default).
2. **Sidecars are the source of truth for user data**; the catalog is a rebuildable cache. Never write derived data (thumbnails, FTS, embeddings, scan state) back to sidecars.
3. **We are a guest on the filesystem.** digiKam, Syncthing, and the user's editor may touch the same sidecars at any time. Every write must assume the file changed since we last read it.
4. **Lossless by construction:** a Fotobase write may never destroy XMP content it does not understand.

## 2. Sidecar file & the `fb:` namespace

- Sidecar path: `<original-name>.xmp` next to the original (`IMG_1234.jpg.xmp`). UTF-8, XMP packet (RDF/XML), written by exiv2 via our own bindings (rexiv2 fork, 03-001).
- Custom namespace `https://fotobase.github.io/ns/1.0/` with prefix `fb:`, declared once per packet. (Namespace URI deliberately NOT `fotobase.app` — that domain is third-party-owned per ADR-0010; the org's GitHub Pages domain is stable and ours.) Fields (all in the sidecar unless noted):

| Field | XMP location | Type | Decided |
|---|---|---|---|
| Albums | `fb:Albums` | bag of album IDs (UUID strings) | §5 |
| Album definitions | `albums.json` next to the catalog file (name, date, place, description, cover) | one JSON document, merge-on-write (backup, `.tmp` + rename, unknown fields preserved) | 03-008 |
| Edit recipe | `fb:Recipe` | versioned JSON string | §5 |
| Stack membership | `fb:StackId` (UUID), `fb:StackRole` (`top`/`member`) | per member photo | §11 Q5 |
| Hidden (photo) | `fb:Hidden` | boolean | §11 Q6 |
| Hidden (folder) | `.fotobase` folder metadata file, one JSON flag `hidden` | per folder | §11 Q6 |

Standard fields stay standard so other apps interoperate: `xmp:Rating`, `dc:subject`, `lr:hierarchicalSubject`, `dc:description`, `dc:creator`, `dc:rights`, `exif:GPS*`, `exif:DateTimeOriginal`, `mwg-rs:Regions` (§5 table).

### Capture time (`exif:DateTimeOriginal`)

- The date/time adjustment tools (shift, timezone fix, multi-camera merge; 03-019) write the adjusted capture time to `exif:DateTimeOriginal` as ISO-8601 (`2024-06-01T10:00:00+02:00`; no offset suffix when the source had none). `xmp:CreateDate` and `photoshop:DateCreated` are updated only when the packet already carries them, so no app sees two capture dates; they are never invented.
- Base for an adjustment: the sidecar's value, else the catalog's `files.taken_at`, else the file `mtime` (UTC). `files.taken_at` mirrors the sidecar value on write-through and on rebuild; a sidecar without the field never clears the cached value.
- One sidecar write per photo per batch (§3 coalescing); a photo that cannot be adjusted (unreadable date, out of range, I/O error) is reported and skipped, never half-written.

### Info-pane metadata editing (GPS, artist, copyright, date)

- The info-pane editor (03-020) writes one photo's edited fields in a single merge-on-write (§3): the fields it did not change are not touched. Capture time follows the rules above; a date typed without a UTC offset keeps the photo's current offset (sidecar, else catalog).
- GPS: `exif:GPSLatitude` / `exif:GPSLongitude` in the XMP `GPSCoordinate` form `DDD,MM.mmmmmmmH` (`52,31.986N`; seven minute decimals, trailing zeros dropped), the form exiv2, digiKam and darktable read. An attribute-form value another tool wrote is replaced, never duplicated. Clearing removes both properties. Input accepts decimal degrees or degrees/minutes/seconds, with a sign or a hemisphere letter; out-of-range or malformed input is rejected before anything is written.
- Artist: `dc:creator`, an ordered array (`rdf:Seq`) holding the one name entered; several creators written by another tool read joined with ", " and are replaced by an edit. Copyright: `dc:rights`, a language alternative whose `x-default` entry is set; other languages stay. A blank value removes the property.
- The catalog mirrors all four (`files.taken_at`, `metadata.latitude/longitude/artist/copyright`) after the sidecar write succeeds, and re-derives them from sidecars on rebuild.

### Folder-level `.fotobase` file

- One JSON file per folder: `{ "version": 1, "hidden": false }`. Written with the same merge-on-write discipline (§3). Absent file = not hidden.
- Folder hide never touches member sidecars; the scanner inherits the flag at scan time. Photo hide always writes the photo's own sidecar.
- Hide, unhide and their undo go through the catalog trash (03-012, `fotobase_catalog::trash`): the trash table is a catalog-only record of what to reverse, and an undo calls the same `fb:Hidden` / `.fotobase` write path. A hide stays in effect after its trash entry expires — only the undo is lost. Read-side: `Sidecar::hidden` / `FolderMeta` (`fotobase_metadata`); an absent `.fotobase` means "not hidden", a malformed one is never overwritten and is skipped (logged) by scans.

## 3. Merge-on-write algorithm (read-modify-write)

Every user-data mutation maps to exactly one sidecar write, executed as:

1. **Acquire lock** (§5). Open and *parse the full XMP tree as it exists on disk right now* — never reuse a cached tree as the write base.
2. **Modify only known nodes:** set/delete the specific nodes for the field(s) being changed (e.g. replace the `xmp:Rating` value node). All other nodes — including unknown namespaces, digiKam's `digiKam:TagsList`, Lightroom hierarchies, XML comments, unknown attributes — are preserved verbatim in tree order.
3. **Serialize** with exiv2 (it preserves the packet structure and unknown fields it parsed).
4. **Backup previous version:** before replacing, copy the current on-disk sidecar to `<sidecar>.fb-bak` (single previous-version slot, overwrite older backups; ".originals"-style per §5). Backup failures are non-fatal but logged and surfaced in maintenance.
5. **Atomic replace:** write to `<sidecar>.tmp` in the same directory, fsync, then rename over the target. Never write in place.
6. **Update catalog** (derived index) in the same transaction boundary as the write-through (03-002): sidecar write succeeds first, catalog second — catalog is the cache, so a crash between the two self-heals on next read (conflict rule: sidecar wins).

**Coalescing:** bulk operations (tag 500 photos) coalesce per-file (one write per sidecar) but may batch catalog updates. Never write a sidecar more than once per user action per file.

**Embed-on-Save (opt-in, §11 Q2):** Save copies the same field set into the original's embedded XMP using the identical merge-on-write steps (target = original file, same backup discipline). Default remains sidecars only.

## 4. Move / rename re-association

When a file is renamed or moved outside Fotobase, the scanner must re-associate it with its existing sidecar and user data instead of treating it as a new photo.

1. **Identity key:** `(size, mtime, content hash)`. Size+mtime are the cheap probe; the content hash is authoritative (computed lazily and cached in the catalog).
2. **Scan-time matching:** on scan, a new-looking path is first probed by (size, mtime) against the missing-file set; candidates are confirmed by hash match. A confirmed match re-associates: the catalog row keeps its user-data provenance, and the sidecar follows the original.
3. **Sidecar follows the original:** if the sidecar did not move along (user moved only the original), Fotobase renames/moves the old sidecar to the new location after association — with the merge-on-write backup discipline applied to the destination if one already exists (then: merge two packets, ours-wins only on fields we own, unknown fields from both preserved).
4. **Ambiguity:** if two missing files match the same hash (true duplicates), associate to the one whose sidecar/user data is richer; log the ambiguity; never silently drop either sidecar (keep the loser as `.fb-bak` next to the winner).
5. **Inside-app moves** (batch rename/move, 03-011, `fotobase_scanner::batch`) use the same sidecar-follows-original rule directly, no matching needed. Per photo the order is original → sidecar (+ its `.fb-bak`) → catalog row; a target whose original *or* `.xmp` already exists is a collision (refused or auto-suffixed, never merged or overwritten). A failure aborts the batch and rolls the already-moved photos back. A hard kill mid-photo leaves the original at the new path and the sidecar/row at the old one — the next scan handles that through steps 2–3 above.

## 5. Locking & concurrency protocol

Threats: digiKam writing the same sidecar; Syncthing syncing a sidecar mid-write; two Fotobase windows/processes; our own scanner reading while a write lands.

1. **Advisory lock file** `<sidecar>.fb-lock` (O_CREAT|O_EXCL) held for the duration of read→backup→tmp-rename. Stale locks (PID dead / older than 60 s) are breakable. Best-effort: other apps won't honor it, so it protects only Fotobase-vs-Fotobase.
2. **Against other apps (digiKam etc.):** protection comes from the algorithm, not the lock — always re-read before write (§3 step 1), write atomically (§3 step 5), keep one backup (§3 step 4). Worst case with a lost race: the other app's write is in `.fb-bak`, recoverable from the maintenance dialog.
3. **Against Syncthing:** atomic rename means sync sees either the old or the new packet, never a torn file. Do not write into a directory with a Syncthing `.stfolder`-marked temp-file conflict present — if `<sidecar>.sync-conflict-*` exists, treat the directory as contended: log, back off, retry on next scan.
4. **Scanner vs writer:** the scanner never writes sidecars; it only reads. A sidecar that changes mid-read yields a re-parse on the next cycle (mtime check).
5. **Retry policy:** on any write error (lock contention, IO), retry 3× with exponential backoff, then surface to the user as a failed-batch entry; never block the UI on a single sidecar.

## 6. Test matrix (becomes the EPIC-03 test plan)

| # | Test | Layer |
|---|---|---|
| M1 | Write `xmp:Rating` to a sidecar containing unknown digiKam/Lightroom namespaces → unknown fields byte-preserved | Unit (03-001) |
| M2 | Every write produces `<sidecar>.fb-bak` = previous version | Unit |
| M3 | Write during simulated external modification (file changed between our read and write) → our write is based on the fresh content; no unknown-field loss | Integration |
| M4 | Crash between sidecar write and catalog update → next read reconciles, sidecar wins | Integration (03-002) |
| M5 | Rename original outside app → rescan → same photo (stars/tags intact), sidecar renamed alongside | Integration (03-003) |
| M6 | Move original without sidecar → rescan → sidecar relocated and re-associated | Integration |
| M7 | Two byte-identical originals both missing → ambiguous association keeps both sidecars, richer one wins | Unit |
| M8 | Syncthing-style `.sync-conflict-*` present → write backs off and retries next scan | Unit |
| M9 | Stale lock (> 60 s, dead PID) is broken; live lock is waited on then retried | Unit |
| M10 | Folder `.fotobase` hidden=true → members hidden after scan; file absent → not hidden | Integration (03-009) |
| M11 | Stack fields `fb:StackId`/`fb:StackRole` survive a digiKam round-trip (digiKam preserves unknown ns) | Interop (03-004) |
| M12 | Sidecars round-trip through darktable & digiKam: tags, ratings, captions, GPS, face regions intact | Interop (03-004) |

## 7. Decisions deferred to implementation (not blocking)

- exiv2 API specifics for node-level preservation (validate M1 empirically; if exiv2 cannot preserve comments, document the lossy edge).
- Backup retention count (default 1 slot; maintenance dialog may expose N).
- Hash function for identity (xxhash preferred for speed; any stable choice acceptable since the catalog caches it).
