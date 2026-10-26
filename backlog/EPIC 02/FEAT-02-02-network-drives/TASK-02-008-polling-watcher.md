# task: polling watcher for SMB/NFS paths

**Feature:** [FEAT-02-02-network-drives](FEAT-02-02-network-drives.md) · **Epic:** [EPIC-02 — Media & platform hardening](../EPIC-02.md) · **Status:** in review (2026-10-26)

## Description
Add a polling watcher for SMB/NFS-watched folders, because `notify` (inotify/FSEvents) never fires over network filesystems (PLAN §6 P1.5). Polls mtime+size with an adaptive interval; feeds the existing scanner pipeline.

## Functional specs
- Folders detected as network paths get the polling watcher automatically; local folders keep `notify`.
- New/changed/deleted files on the share are picked up without manual refresh.
- Interval adapts to share responsiveness (back off when slow, tighten when idle-free); heavy polling never saturates the share or the UI.

## Technical specs
- `scanner` crate: watcher trait abstraction with two impls (notify-based, polling); selection at watch-registration time via mount-filesystem check (statfs/mntent).
- Poll compares directory listing + mtime+size per file (cheap, no hash — hashing stays lazy per 02-009).
- Adaptive interval: min/max bounds (e.g. 5s–5min), scaled by measured probe latency and change activity.
- Reuse the existing change-event queue so downstream scan logic is unchanged.

## Testing criteria (TDD)
- Unit: given a fake FS listing with one changed mtime, the poller emits exactly one change event.
- Unit: interval doubles when probes exceed a latency threshold and halves back when fast.
- Integration: an SMB mount (test harness from 02-010) sees an added file appear in the catalog within one poll cycle.

## Acceptance criteria
- [x] 02-008: network-watched folders use polling, not `notify`.
- [x] 02-008: add/change/delete on the share is reflected in the library without restart.
- [x] 02-008: poll interval adapts to share responsiveness within configured bounds.
