# feat: network drives (NAS)

**Epic:** [EPIC-02 — Media & platform hardening](../EPIC-02.md) · **Status:** planned

## Acceptance criteria

- Watched SMB/NFS share: mid-scan network unplug pauses scanning cleanly and resumes on reconnect with no UI hang and no duplicated work.
- Polling interval adapts to share responsiveness; IO is throttled so the UI stays responsive during network scans; hashing is lazy.
- Disconnect/reconnect harness (CI virtual SMB or documented manual protocol) passes repeatedly.

- [x] `02-008` (M) Polling watcher for SMB/NFS paths (adaptive interval, mtime+size) — `notify` never fires over network FS.
- [x] `02-009` (M) Throttled scanning for network folders: IO limits, pause-on-disconnect, clean resume, no UI hang; hash lazily.
- [ ] `02-010` (S) Disconnect/reconnect test harness with virtual SMB share in CI (or documented manual protocol).

## Tasks

- [x] [TASK-02-008-polling-watcher.md](TASK-02-008-polling-watcher.md) — polling watcher for SMB/NFS paths
- [ ] [TASK-02-009-throttled-scanning.md](TASK-02-009-throttled-scanning.md) — throttled scanning for network folders
- [ ] [TASK-02-010-disconnect-test-harness.md](TASK-02-010-disconnect-test-harness.md) — disconnect/reconnect test harness
