#!/usr/bin/env python3
"""Reproducible folder-access experiment for Flatpak GTK shells.

Pass a folder returned by the shell's portal-backed chooser with
``--folder``. Without a desktop session the harness still runs native
filesystem controls, but labels them as controls rather than portal evidence.
"""

from __future__ import annotations

import argparse
import ctypes
import json
import os
import platform
import select
import shutil
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

IN_CREATE = 0x00000100
IN_CLOSE_WRITE = 0x00000008
IN_MOVED_TO = 0x00000080
IN_NONBLOCK = 0x00000800


def session_portal() -> dict[str, object]:
    address = bool(os.environ.get("DBUS_SESSION_BUS_ADDRESS"))
    display = bool(os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY"))
    command = shutil.which("gdbus")
    owner = False
    detail = "gdbus unavailable" if command is None else "no D-Bus session address"
    if command and address:
        probe = subprocess.run(
            [
                command,
                "call",
                "--session",
                "--dest",
                "org.freedesktop.DBus",
                "--object-path",
                "/org/freedesktop/DBus",
                "--method",
                "org.freedesktop.DBus.NameHasOwner",
                "org.freedesktop.portal.Documents",
            ],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=10,
            check=False,
        )
        owner = probe.returncode == 0 and "true" in probe.stdout.lower()
        detail = probe.stdout.strip()
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    mount = Path(runtime) / "doc" if runtime else None
    return {
        "dbus_session_address": address,
        "display_or_wayland": display,
        "documents_bus_owner": owner,
        "documents_mount": str(mount) if mount else None,
        "documents_mount_exists": bool(mount and mount.is_dir()),
        "probe_detail": detail,
    }


def under(path: Path, parent: Path | None) -> bool:
    if parent is None:
        return False
    try:
        path.resolve().relative_to(parent.resolve())
        return True
    except (OSError, ValueError):
        return False


def snapshot(folder: Path) -> list[tuple[str, int]]:
    rows = []
    for path in sorted(folder.rglob("*")):
        if path.is_file():
            rows.append((path.relative_to(folder).as_posix(), path.stat().st_size))
    return rows


def watch_external_create(folder: Path) -> dict[str, object]:
    marker = folder / f"syncthing-visible-{uuid.uuid4().hex}.xmp"
    libc = ctypes.CDLL(None, use_errno=True)
    init = getattr(libc, "inotify_init1", None)
    add = getattr(libc, "inotify_add_watch", None)
    if init is None or add is None:
        return {"supported": False, "reason": "inotify unavailable"}
    fd = init(IN_NONBLOCK)
    if fd < 0:
        return {"supported": False, "reason": f"inotify_init1 errno={ctypes.get_errno()}"}
    try:
        watch = add(fd, os.fsencode(folder), IN_CREATE | IN_CLOSE_WRITE | IN_MOVED_TO)
        if watch < 0:
            return {
                "supported": False,
                "reason": f"inotify_add_watch errno={ctypes.get_errno()}",
            }
        child = subprocess.Popen(
            [
                sys.executable,
                "-c",
                "import pathlib,sys,time; time.sleep(.1); "
                "pathlib.Path(sys.argv[1]).write_bytes(b'<x:xmpmeta/>')",
                str(marker),
            ]
        )
        ready, _, _ = select.select([fd], [], [], 5.0)
        event_bytes = os.read(fd, 65536) if ready else b""
        child.wait(timeout=5)
        visible = marker.exists() and marker.read_bytes() == b"<x:xmpmeta/>"
        return {
            "supported": bool(ready and event_bytes and visible),
            "event_received": bool(ready and event_bytes),
            "rescan_visible": visible,
            "writer_process": "separate child process",
        }
    finally:
        os.close(fd)
        marker.unlink(missing_ok=True)


def atomic_conflict_control(folder: Path) -> dict[str, object]:
    group = folder / f"conflict-{uuid.uuid4().hex}"
    group.mkdir()
    canonical = group / "photo.jpg.xmp"
    conflict = group / "photo.jpg.sync-conflict-20260916-120000-PHONE01.xmp"
    temporary = group / ".gallery-tmp-merged"
    canonical.write_bytes(b"canonical")
    conflict.write_bytes(b"conflict")
    temporary.write_bytes(b"merged-preserving-both")
    try:
        os.replace(temporary, canonical)
        renamed = canonical.read_bytes() == b"merged-preserving-both"
        before_choice = conflict.exists()
        conflict.unlink()
        deleted_after_choice = not conflict.exists()
        return {
            "atomic_replace": renamed,
            "loser_survived_until_explicit_delete": before_choice,
            "explicit_loser_delete": deleted_after_choice,
        }
    finally:
        shutil.rmtree(group, ignore_errors=True)


def stat_throughput(folder: Path, entries: int) -> dict[str, object]:
    root = folder / f"stat-{uuid.uuid4().hex}"
    root.mkdir()
    try:
        for index in range(entries):
            (root / f"{index:05}.xmp").touch()
        started = time.perf_counter()
        rows = snapshot(root)
        elapsed = time.perf_counter() - started
        return {
            "entries": entries,
            "elapsed_ms": elapsed * 1000,
            "entries_per_second": entries / elapsed if elapsed else None,
            "complete": len(rows) == entries,
        }
    finally:
        shutil.rmtree(root, ignore_errors=True)


def persistence(folder: Path, state_path: Path | None, real_portal: bool) -> dict[str, object]:
    if state_path is None:
        return {"tested": False, "reason": "--state was not supplied"}
    previous = None
    if state_path.exists():
        try:
            previous = json.loads(state_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            previous = None
    current = str(folder.resolve())
    state_path.parent.mkdir(parents=True, exist_ok=True)
    state_path.write_text(json.dumps({"folder": current}) + "\n", encoding="utf-8")
    if not real_portal:
        return {
            "tested": False,
            "reason": "path is not backed by a live document portal",
            "state_round_trip_control": previous is not None and previous.get("folder") == current,
        }
    return {
        "tested": previous is not None,
        "persisted": previous is not None
        and previous.get("folder") == current
        and folder.is_dir(),
        "reason": None if previous is not None else "first run recorded; rerun to test persistence",
    }


def run(args: argparse.Namespace) -> dict[str, object]:
    portal = session_portal()
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    mount = Path(runtime) / "doc" if runtime else None
    temporary = None
    if args.folder is None:
        temporary = tempfile.TemporaryDirectory(prefix="flatpak-portal-control-")
        folder = Path(temporary.name)
    else:
        folder = args.folder
    if not folder.is_dir():
        raise SystemExit(f"not a directory: {folder}")

    real_portal = bool(
        portal["documents_bus_owner"]
        and portal["documents_mount_exists"]
        and under(folder, mount)
    )
    probe_root = folder / f".portal-experiment-{uuid.uuid4().hex}"
    probe_root.mkdir()
    try:
        (probe_root / "before.txt").write_text("before", encoding="utf-8")
        before = snapshot(probe_root)
        (probe_root / "after.txt").write_text("after", encoding="utf-8")
        after = snapshot(probe_root)
        result = {
            "schema": 1,
            "execution": {
                "architecture": platform.machine(),
                "platform": platform.platform(),
                "python": platform.python_version(),
            },
            "portal_session": portal,
            "folder": {
                "caller_supplied": args.folder is not None,
                "under_documents_mount": under(folder, mount),
                "real_portal_grant_observed": real_portal,
            },
            "persisted_access": persistence(folder, args.state, real_portal),
            "rescan": {
                "initial_entries": len(before),
                "after_entries": len(after),
                "new_file_visible": ("after.txt", 5) in after,
            },
            "syncthing_visibility": watch_external_create(probe_root),
            "conflict_resolution": atomic_conflict_control(probe_root),
            "stat_walk": stat_throughput(probe_root, args.entries),
        }
        capabilities = [
            result["rescan"]["new_file_visible"],
            result["syncthing_visibility"]["supported"],
            result["conflict_resolution"]["atomic_replace"],
            result["conflict_resolution"]["loser_survived_until_explicit_delete"],
            result["conflict_resolution"]["explicit_loser_delete"],
            result["stat_walk"]["complete"],
        ]
        if real_portal and result["persisted_access"].get("persisted") and all(capabilities):
            outcome = "support"
            reason = "all portal grant, persistence, rescan, watch, rename, and delete probes passed"
        else:
            outcome = "reject-support-claim"
            reason = (
                "no live document-portal grant and persisted desktop session were observed"
                if not real_portal
                else "one or more required portal capabilities remain unproved"
            )
        result["decision"] = {
            "outcome": outcome,
            "reason": reason,
            "native_controls_are_portal_evidence": real_portal,
        }
        return result
    finally:
        shutil.rmtree(probe_root, ignore_errors=True)
        if temporary is not None:
            temporary.cleanup()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--folder", type=Path)
    parser.add_argument("--state", type=Path)
    parser.add_argument("--entries", type=int, default=20_000)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = run(args)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
