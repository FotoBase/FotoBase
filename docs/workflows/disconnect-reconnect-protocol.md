# Disconnect/reconnect manual test protocol (network shares)

**Task:** 02-010 · **Status:** approved 2026-10-27 · **Plan refs:** §6 P1.5 acceptance · **Companion harness:** `tools/smb_harness.py`

Defines the manual step-by-step protocol for validating the P1.5 NAS acceptance bar — *watched SMB share: unplug the network mid-scan → scan pauses cleanly, resumes on reconnect, no UI hang* — on real hardware, and the pass/fail observations for each step. Use this when the automated harness is unavailable (macOS runners without containerized SMB, physical NAS devices, Wi-Fi disconnect simulation) or when validating the automated harness itself against a real share.

The automated path (containerized Samba + the scripted cycle run 3× consecutively) is `.github/workflows/smb-harness.yml`; this document is the fallback the task spec requires and the source of expected observations for both paths.

---

## 1. Environment

- Any dev machine or physical NAS with an SMB share the app can mount. macOS: Finder/mount_smbfs; Windows: mapped drive; Linux: `mount -t cifs`.
- Fotobase dev build (`cargo tauri dev` or the installed debug build).
- A way to interrupt the network mid-scan without closing the app: turn off Wi-Fi, unplug the Ethernet cable, or power off the NAS. All three are valid "unplugs"; note which was used in the run record.

## 2. Steps

| # | Step | Expected observation | Pass/fail |
|---|------|----------------------|-----------|
| 1 | Add the share as a watched folder; start a scan of a tree ≥ 200 files | Scan progress advances; UI stays responsive while scanning | |
| 2 | Mid-scan (while progress is still advancing), disconnect the network | Within seconds: scan state becomes **paused** (not failed, not running); no error dialog storm — at most one aggregate notice; UI remains responsive throughout | |
| 3 | With the network still down, browse the library | Already-scanned items render; no hang, no beachball/spinner freeze; catalog shows no mass deletions of share files | |
| 4 | Verify nothing was deleted while paused | File count for the share in the catalog is unchanged from before the disconnect (paused scans are non-destructive) | |
| 5 | Reconnect the network (or power the NAS back on) | The scan **resumes automatically** (or after the next watcher poll cycle) and continues from where it stopped | |
| 6 | Wait for the scan to complete | Final file count equals the number of files on the share; no missing entries | |
| 7 | Check for duplicated work | No duplicate rows for the same path in the catalog; the resumed pass did not re-add files already committed before the pause | |
| 8 | Repeat steps 1–7 two more times (3 consecutive full cycles) | All three cycles pass with the same observations | |

## 3. Recording a run

Record one line per cycle in the PR or task issue:

```
cycle 1: OS <macOS 15 / Win 11 / Ubuntu 22.04> · unplug method <wifi-off> · paused <s> after disconnect ·
resumed <yes/no auto> · final count <N/N> · duplicates <0> · UI hang <none> · PASS/FAIL
```

A cycle passes only if **all** of these hold:

- scan paused cleanly within seconds of the unplug (state = paused, not error);
- UI stayed responsive during and after the disconnect (no hang);
- nothing was deleted while paused;
- resume continued without duplicating work (final count exact, no duplicate rows);
- the cycle completed end-to-end.

Three consecutive passing cycles = the acceptance bar (§6 P1.5) is met for that platform.

## 4. Cross-references

- Automated harness: `tools/smb_harness.py` (`mount/populate/disconnect/reconnect/state/cycle`), integration test `crates/scanner/tests/smb_harness.rs`, CI job `.github/workflows/smb-harness.yml`.
- Scanner pause/resume semantics: `crates/scanner/src/network.rs` (02-009).
- Polling watcher for network paths: `crates/scanner/src/polling.rs` (02-008, env-gated test `FB_SMB_TEST_ROOT`).
