# task: throttled scanning for network folders

**Feature:** [FEAT-02-02-network-drives](FEAT-02-02-network-drives.md) · **Epic:** [EPIC-02 — Media & platform hardening](../EPIC-02.md) · **Status:** todo

## Description
Make network-folder scans non-destructive to UX: IO limits, pause on disconnect, clean resume, lazy hashing (PLAN §6 P1.5 acceptance: mid-scan unplug pauses cleanly, resumes on reconnect, no UI hang).

## Functional specs
- Unplugging the network mid-scan pauses the scan cleanly — no error storm, no UI hang; reconnect resumes with no duplicated work.
- Network scans run at capped IO so browsing stays responsive.
- Hashes are computed lazily (only when needed, e.g. dedup or re-association), never during bulk scan.

## Technical specs
- `scanner` crate: per-folder-source IO budget (configurable ops/sec or bytes/sec); scan loop yields to the job queue (PLAN §3 rule 4).
- Disconnect detection: IO errors on the share flip the scan to paused; a liveness probe (cheap statfs) polls for reconnect; resume continues from the last completed directory entry, not from scratch.
- `files.hash` stays NULL until lazily filled; catalog queries must already treat hash as optional.
- Errors during a paused scan are aggregated, not surfaced per-file.

## Testing criteria (TDD)
- Integration (with 02-010 harness): kill the share mid-scan → scan state becomes `paused` within seconds, UI thread never blocks (assert via main-thread heartbeat).
- Integration: reconnect → scan completes; file count and scanned set match a never-disconnected control run.
- Unit: hash is not computed during scan of a network folder (spy on the hashing function).

## Acceptance criteria
- [ ] 02-009: mid-scan disconnect pauses cleanly; UI stays responsive.
- [ ] 02-009: reconnect resumes the scan with no duplicated work.
- [ ] 02-009: hashing is lazy for network folders.
- [ ] 02-009: IO throttling keeps interactive browsing responsive during a scan.
