#!/usr/bin/env python3
"""Disconnect/reconnect test harness for network shares (TASK-02-010, PLAN §6 P1.5).

Runs a containerized Samba share that a test can mount, populate with a
deterministic fixture tree, disconnect (smbd killed — the SMB session
stops answering mid-op, exactly like an unplugged NAS), and reconnect.
The scripted cycle (mount → populate → scan → disconnect → paused →
reconnect → complete) is the harness 02-008/02-009 integration tests and
the CI job (`.github/workflows/smb-harness.yml`) drive.

Usage (stdlib only, no Python dependencies):

    python3 tools/smb_harness.py mount      # start container + mount share
    python3 tools/smb_harness.py populate   # fixture tree with known mtimes/sizes
    python3 tools/smb_harness.py disconnect # kill smbd (share session drops)
    python3 tools/smb_harness.py reconnect  # restart smbd (share answers again)
    python3 tools/smb_harness.py state      # catalog scan state summary
    python3 tools/smb_harness.py unmount    # unmount + remove container
    python3 tools/smb_harness.py cycle      # full scripted cycle, 3x by default

State is recorded in .fb-smb-harness.json (host cwd) so the subcommands can
run as separate CI steps; mountpoints use /tmp. The container is named
fb-smb-harness and uses host port 1445 — override with FB_SMB_IMAGE,
FB_SMB_PORT, FB_SMB_USER, FB_SMB_PASS, FB_SMB_CONTAINER.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import time
from pathlib import Path

CONTAINER_DEFAULT = "fb-smb-harness"
IMAGE_DEFAULT = "ghcr.io/servercontainers/samba:latest"
PORT_DEFAULT = "1445"
USER_DEFAULT = "fotobase"
PASS_DEFAULT = "fotobase"
SHARE = "photos"
STATE_FILE = Path(".fb-smb-harness.json")
MOUNT_BASE = Path("/tmp")

CONTAINER = os.environ.get("FB_SMB_CONTAINER", CONTAINER_DEFAULT)
IMAGE = os.environ.get("FB_SMB_IMAGE", IMAGE_DEFAULT)
PORT = os.environ.get("FB_SMB_PORT", PORT_DEFAULT)
USER = os.environ.get("FB_SMB_USER", USER_DEFAULT)
PASS = os.environ.get("FB_SMB_PASS", PASS_DEFAULT)

# Deterministic fixture: tens of files, known sizes, mtimes staggered by a
# fixed offset so (mtime, size) poll keys are stable across runs.
FIXTURE_DIRS = 5
FILES_PER_DIR = 10
MTIME_BASE = 1700000000  # fixed epoch — never "now", so mtimes are known


def run(cmd: list[str], *, check: bool = True, capture: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, check=check, capture_output=capture, text=True)


def docker(*args: str, check: bool = True) -> subprocess.CompletedProcess:
    return run(["docker", *args], check=check)


def state_file_write(**data: object) -> None:
    STATE_FILE.write_text(json.dumps(data, indent=2))


def state_file_read() -> dict:
    if STATE_FILE.exists():
        return json.loads(STATE_FILE.read_text())
    return {}


def _macos_mountpoint() -> Path:
    return MOUNT_BASE / "fb-smb-harness"


def _linux_mountpoint() -> Path:
    return MOUNT_BASE / "fb-smb-harness"


def mountpoint() -> Path:
    return _macos_mountpoint() if platform.system() == "Darwin" else _linux_mountpoint()


def cmd_mount(args: argparse.Namespace) -> int:
    """Start the Samba container and mount the share on the host."""
    if container_running():
        print(f"container {CONTAINER} already running")
    else:
        docker(
            "run",
            "-d",
            "--rm",
            "--name",
            CONTAINER,
            "-p",
            f"{PORT}:445",
            "-e",
            "AVAHI_DISABLE=1",
            "-e",
            "SMB1_DISABLE=1",
            "-e",
            f"ACCOUNT_{USER}={PASS}",
            "-e",
            f"SAMBA_VOLUME_CONFIG_{SHARE}=[{SHARE}]; path=/share; valid users={USER}; "
            "read only=no; guest ok=no; browseable=yes",
            IMAGE,
        )
        # The share path must exist inside the container before the first
        # tree connect; the image starts smbd without creating it, and the
        # readiness probe below tree-connects to the share.
        docker("exec", CONTAINER, "sh", "-c", f"mkdir -p /share && chown {USER} /share")
        ready = False
        for _ in range(60):
            out = docker(
                "exec",
                CONTAINER,
                "smbclient",
                f"//localhost/{SHARE}",
                "-U",
                f"{USER}%{PASS}",
                "-c",
                "ls",
                check=False,
            )
            if out.returncode == 0:
                ready = True
                break
            time.sleep(1)
        if not ready:
            print("samba container did not become ready", file=sys.stderr)
            return 1

    mp = mountpoint()
    mp.parent.mkdir(parents=True, exist_ok=True)
    # The mount target itself must exist (mount.cifs/mount_smbfs chdir into
    # it); on a fresh CI runner nothing created it yet.
    mp.mkdir(parents=True, exist_ok=True)
    system = platform.system()
    for attempt in range(3):
        try:
            if system == "Darwin":
                run(["/sbin/mount_smbfs", f"//{USER}:{PASS}@127.0.0.1:{PORT}/{SHARE}", str(mp)])
            elif system == "Linux":
                run(
                    [
                        "sudo",
                        "mount",
                        "-t",
                        "cifs",
                        # mount.cifs does not accept //host:port/share — the port
                        # goes into the port= option.
                        f"//127.0.0.1/{SHARE}",
                        str(mp),
                        "-o",
                        # uid/gid/file_mode/dir_mode so the runner user
                        # (not root, who performed the mount) can write.
                        f"username={USER},password={PASS},vers=3.0,soft,_netdev,port={PORT},uid={os.getuid()},gid={os.getgid()},file_mode=0666,dir_mode=0777",
                    ]
                )
            else:
                print(f"unsupported platform for mount: {system}", file=sys.stderr)
                return 1
            break
        except subprocess.CalledProcessError as e:
            if attempt == 2:
                print(f"mount failed: {e.stderr.strip()}", file=sys.stderr)
                return 1
            time.sleep(2)
    state_file_write(mounted=True, mountpoint=str(mp), port=PORT, container=CONTAINER)
    print(f"mounted at {mp}")
    return 0


def container_running() -> bool:
    out = docker("ps", "--filter", f"name=^{CONTAINER}$", "--format", "{{.Status}}", check=False)
    return out.returncode == 0 and out.stdout.strip() != ""


def _is_mounted(mp: Path) -> bool:
    """Is `mp` an actual mount (not a plain local directory)?"""
    if platform.system() == "Darwin":
        out = run(["/sbin/mount"])
        # macOS reports the resolved /private/tmp path in mount output.
        return any(f" on {mp} (" in line or f" on {os.path.realpath(mp)} (" in line for line in out.stdout.splitlines())
    return mp.is_mount()


def cmd_populate(args: argparse.Namespace) -> int:
    """Write the deterministic fixture tree onto the mounted share."""
    mp = mountpoint()
    if not _is_mounted(mp):
        print(f"{mp} is not a mounted share — run `smb_harness.py mount` first", file=sys.stderr)
        return 1
    for d in range(FIXTURE_DIRS):
        ddir = mp / f"dir{d:02}"
        ddir.mkdir(parents=True, exist_ok=True)
        for f in range(FILES_PER_DIR):
            fpath = ddir / f"img{f:03}.jpg"
            payload = bytes([0xFF]) * (16 + f)  # known size: 16+f bytes
            fpath.write_bytes(payload)
            mtime = MTIME_BASE + d * 1000 + f
            os.utime(fpath, (mtime, mtime))
    state_file_write(**{**state_file_read(), "populated": True})
    print(f"populated {FIXTURE_DIRS} dirs x {FILES_PER_DIR} files at {mp}")
    return 0


def fixture_digest(root: Path) -> str:
    """Deterministic digest of the fixture tree (path + size + mtime)."""
    h = hashlib.sha256()
    for p in sorted(root.rglob("*")):
        if p.is_file():
            st = p.stat()
            h.update(f"{p.relative_to(root)}\x00{st.st_size}\x00{int(st.st_mtime)}\x00".encode())
    return h.hexdigest()


def cmd_disconnect(args: argparse.Namespace) -> int:
    """Simulate an unplug: kill smbd inside the container.

    Killing the daemon tears down the SMB session, so the client's next
    uncached IO fails fast (ECONNRESET-class) instead of hanging on a
    wedged socket. `docker pause` is deliberately NOT used: macOS's SMB
    client serves already-listed directories from its cache while the
    server is frozen, which makes a disconnect invisible to the scanner.
    """
    # runsv supervises smbd and respawns it instantly — kill the supervisor
    # first or the "unplug" heals itself in under a second.
    out = docker(
        "exec",
        CONTAINER,
        "sh",
        "-c",
        "pkill -f 'runsv [s]amba' || true; sleep 0.3; pkill -9 smbd || true",
        check=False,
    )
    if out.returncode != 0:
        print(f"cannot disconnect: {out.stderr.strip() or out.stdout.strip()}", file=sys.stderr)
        return 1
    state_file_write(**{**state_file_read(), "disconnected": True})
    print(f"disconnected (smbd killed in {CONTAINER})")
    return 0


def cmd_reconnect(args: argparse.Namespace) -> int:
    """Simulate a replug: start smbd again and wait for it to answer."""
    out = docker("exec", CONTAINER, "sh", "-c", "pgrep -f 'smbd --foreground' >/dev/null || smbd -D", check=False)
    if out.returncode != 0:
        print(f"cannot reconnect: {out.stderr.strip() or out.stdout.strip()}", file=sys.stderr)
        return 1
    # Wait for smbd to answer again so a resume right after this succeeds.
    for _ in range(30):
        probe = docker(
            "exec",
            CONTAINER,
            "smbclient",
            f"//localhost/{SHARE}",
            "-U",
            f"{USER}%{PASS}",
            "-c",
            "ls",
            check=False,
        )
        if probe.returncode == 0:
            state_file_write(**{**state_file_read(), "disconnected": False})
            print(f"reconnected (smbd answering in {CONTAINER})")
            return 0
        time.sleep(1)
    print("smbd did not come back within 30s", file=sys.stderr)
    return 1


def cmd_state(args: argparse.Namespace) -> int:
    """Report the current harness + catalog scan state as JSON."""
    report = {
        "container_running": container_running(),
        "mountpoint": str(mountpoint()),
        "mounted": mountpoint().exists(),
        **state_file_read(),
    }
    if args.catalog and Path(args.catalog).exists():
        import sqlite3

        conn = sqlite3.connect(args.catalog)
        try:
            report["catalog"] = {
                "files": conn.execute("SELECT COUNT(*) FROM files").fetchone()[0],
                "folders": conn.execute("SELECT COUNT(*) FROM folders").fetchone()[0],
            }
        finally:
            conn.close()
    print(json.dumps(report, indent=2))
    return 0


def cmd_unmount(args: argparse.Namespace) -> int:
    """Unmount the share and remove the container."""
    mp = mountpoint()
    system = platform.system()
    if system == "Darwin":
        run(["/sbin/umount", str(mp)], check=False)
    elif system == "Linux":
        run(["sudo", "umount", str(mp)], check=False)
    docker("unpause", CONTAINER, check=False)
    docker("rm", "-f", CONTAINER, check=False)
    STATE_FILE.unlink(missing_ok=True)
    print("unmounted and container removed")
    return 0


def _scan_catalog(root: Path, catalog_path: Path) -> dict:
    """Run one throttled network scan of the share into a fresh catalog.

    Uses the fotobase-scanner test binary via `cargo test` — the same code
    path the app uses, driven from the harness so the scripted cycle is
    end-to-end (mount → scan → paused → resume → complete).
    """
    env = dict(os.environ)
    env["FB_SMB_TEST_ROOT"] = str(root)
    env["FB_SMB_HARNESS_CATALOG"] = str(catalog_path)
    out = run(
        ["cargo", "test", "-p", "fotobase-scanner", "--test", "smb_harness", "--", "--nocapture"],
        capture=False,
    )
    return {"cargo_exit": out.returncode}


def cmd_cycle(args: argparse.Namespace) -> int:
    """Full scripted cycle, repeated (TDD: passes 3x consecutively in one job)."""
    results = []
    for i in range(args.repeat):
        print(f"=== cycle {i + 1}/{args.repeat} ===")
        cmd_unmount(args)
        cmd_mount(args)
        cmd_populate(args)
        before = fixture_digest(mountpoint())
        # Disconnect mid-"scan": the scanner test binary runs the full
        # pause/resume scenario against the real mount.
        scan = _scan_catalog(mountpoint(), Path(f"/tmp/fb-smb-catalog-{i}.db"))
        after = fixture_digest(mountpoint())
        ok = scan["cargo_exit"] == 0 and before == after
        results.append(ok)
        print(f"cycle {i + 1}: {'PASS' if ok else 'FAIL'} (fixture digest {'stable' if before == after else 'CHANGED'})")
        if not ok:
            return 1
    print(f"all {args.repeat} cycles passed")
    return 0


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="cmd", required=True)
    sub.add_parser("mount", help="start container + mount share")
    sub.add_parser("populate", help="write deterministic fixture tree")
    sub.add_parser("disconnect", help="pause container (simulate unplug)")
    sub.add_parser("reconnect", help="unpause container (simulate replug)")
    sub.add_parser("state", help="report harness/catalog state as JSON")
    sub.add_parser("cycle", help="full scripted cycle, N times")
    sub.add_parser("unmount", help="unmount + remove container")
    sub.choices["state"].add_argument("--catalog", default="", help="optional catalog.db path for scan state")
    sub.choices["cycle"].add_argument("--repeat", type=int, default=3, help="number of cycles (default 3)")
    args = parser.parse_args()
    sys.exit({"mount": cmd_mount, "populate": cmd_populate, "disconnect": cmd_disconnect,
              "reconnect": cmd_reconnect, "state": cmd_state, "unmount": cmd_unmount,
              "cycle": cmd_cycle}[args.cmd](args))


if __name__ == "__main__":
    main()
