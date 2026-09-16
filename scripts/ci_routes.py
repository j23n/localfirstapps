#!/usr/bin/env python3
"""Classify changed paths for the top-level CI workflow.

Unknown paths deliberately run every suite. Documentation paths are the only
class of changes that can select no suite.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

CHECKS = ("rust", "apps", "bindings", "conformance", "docker")
SHA_RE = re.compile(r"^[0-9a-fA-F]{40,64}$")


@dataclass(frozen=True)
class Routes:
    rust: bool = False
    apps: bool = False
    bindings: bool = False
    conformance: bool = False
    docker: bool = False

    @classmethod
    def all(cls) -> "Routes":
        return cls(True, True, True, True, True)

    def union(self, other: "Routes") -> "Routes":
        return Routes(
            *(getattr(self, check) or getattr(other, check) for check in CHECKS)
        )

    def as_dict(self) -> dict[str, bool]:
        return {check: getattr(self, check) for check in CHECKS}


def _is_documentation(path: str) -> bool:
    """Return true only for paths that cannot affect generated/runtime code."""
    if path.startswith("docs/spec/ui/"):
        return False
    if path.startswith(("docs/", ".agents/")):
        return True
    if path.endswith((".md", ".mdx", ".rst", ".adoc")):
        return True
    return path.endswith("/.ai-disclaimer.json")


def _is_bindings_path(path: str) -> bool:
    if path.startswith("apps/gallery/core/"):
        return True
    if path.startswith(("core/contacts-core/", "core/contacts-ffi/")):
        return True
    return path.startswith(
        (
            "apps/gallery/scripts/generate_bindings.sh",
            "apps/gallery/LocalGallery/GalleryCore",
            "apps/gallery/linux/swift-shim/",
            "apps/contacts/scripts/build_ffi.sh",
            "apps/contacts/scripts/generate_bindings.sh",
            "apps/contacts/LocalContacts/ContactsCore",
        )
    )


def route_path(raw_path: str) -> Routes:
    path = raw_path.removeprefix("./")
    if not path:
        return Routes()

    # Workflow and classifier changes exercise the complete orchestration.
    if path.startswith(".github/") or path in {
        "scripts/ci_routes.py",
        "scripts/tests/test_ci_routes.py",
        "scripts/tests/ci_routes_cases.json",
    }:
        return Routes.all()

    if path.startswith("docker/"):
        return Routes(docker=True)

    if _is_documentation(path):
        return Routes()

    # R14's specification and token inputs generate both Rust and Swift code.
    # Conformance's --check is the gate that detects an omitted regeneration.
    if path.startswith(("docs/spec/ui/", "design/")) or path == "scripts/gen_r14.py":
        return Routes(conformance=True)

    # Shared core changes can break both FFI compilation and app shells.
    if path.startswith("core/"):
        return Routes(
            rust=True, apps=True, bindings=True, conformance=True
        )

    if path.startswith("apps/gallery/core/"):
        return Routes(
            rust=True, apps=True, bindings=True, conformance=True
        )

    if path.startswith("apps/"):
        return Routes(
            apps=True,
            bindings=_is_bindings_path(path),
            conformance=True,
        )

    if path.startswith("shells/shell-kit-swift/"):
        return Routes(apps=True, conformance=True)

    if path.startswith(("shells/", "mac/")):
        return Routes(apps=True)

    if path.startswith("conformance/"):
        return Routes(conformance=True)

    # A new top-level area or shared file is assumed to affect everything.
    return Routes.all()


def route_paths(paths: Iterable[str], *, force_all: bool = False) -> Routes:
    if force_all:
        return Routes.all()
    routes = Routes()
    for path in paths:
        routes = routes.union(route_path(path))
    return routes


def _valid_sha(value: object) -> str | None:
    if not isinstance(value, str) or not SHA_RE.fullmatch(value):
        return None
    return value.lower()


def _has_commit(sha: str) -> bool:
    result = subprocess.run(
        ["git", "cat-file", "-e", f"{sha}^{{commit}}"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return result.returncode == 0


def _ensure_commit(sha: str) -> bool:
    if _has_commit(sha):
        return True
    subprocess.run(
        ["git", "fetch", "--no-tags", "--depth=1", "origin", sha],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return _has_commit(sha)


def _changed_paths(base: str, head: str) -> list[str] | None:
    if not _ensure_commit(base) or not _ensure_commit(head):
        return None
    result = subprocess.run(
        [
            "git",
            "diff",
            "--name-only",
            "--no-renames",
            "-z",
            base,
            head,
            "--",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    if result.returncode != 0:
        return None
    return [
        os.fsdecode(path)
        for path in result.stdout.split(b"\0")
        if path
    ]


def routes_for_event(
    event_name: str, event: dict[str, object], fallback_sha: str
) -> tuple[Routes, list[str], str]:
    """Return routes, changed paths, and a diagnostic comparison reason."""
    if event_name == "pull_request":
        pull_request = event.get("pull_request")
        if isinstance(pull_request, dict):
            base_data = pull_request.get("base")
            head_data = pull_request.get("head")
            if isinstance(base_data, dict) and isinstance(head_data, dict):
                base = _valid_sha(base_data.get("sha"))
                head = _valid_sha(head_data.get("sha"))
                if base and head:
                    paths = _changed_paths(base, head)
                    if paths is not None:
                        return route_paths(paths), paths, "pull-request-base"
        return Routes.all(), [], "pull-request-comparison-unavailable"

    if event_name == "push":
        before_value = event.get("before")
        if isinstance(before_value, str) and before_value and set(before_value) == {"0"}:
            return Routes.all(), [], "initial-push"
        base = _valid_sha(before_value)
        head = _valid_sha(event.get("after")) or _valid_sha(fallback_sha)
        if base and head:
            paths = _changed_paths(base, head)
            if paths is not None:
                return route_paths(paths), paths, "push-before"
        return Routes.all(), [], "push-comparison-unavailable"

    return Routes.all(), [], f"unsupported-event-{event_name or 'unknown'}"


def _write_github_output(path: Path, routes: Routes, reason: str) -> None:
    with path.open("a", encoding="utf-8") as output:
        for check, selected in routes.as_dict().items():
            output.write(f"{check}={'true' if selected else 'false'}\n")
        output.write(f"reason={reason}\n")


def _github_command(args: argparse.Namespace) -> int:
    try:
        event = json.loads(args.event_path.read_text(encoding="utf-8"))
        if not isinstance(event, dict):
            raise ValueError("event payload is not an object")
        routes, paths, reason = routes_for_event(
            args.event_name, event, args.sha
        )
    except (OSError, ValueError, json.JSONDecodeError) as error:
        routes, paths, reason = Routes.all(), [], "event-read-unavailable"
        print(f"warning: {error}; selecting every CI suite", file=sys.stderr)

    _write_github_output(args.output, routes, reason)
    print(f"CI route reason: {reason}")
    print(f"Changed paths: {len(paths)}")
    for check, selected in routes.as_dict().items():
        print(f"  {check}: {'run' if selected else 'skip'}")
    return 0


def _paths_command(args: argparse.Namespace) -> int:
    routes = route_paths(args.paths, force_all=args.all)
    print(json.dumps(routes.as_dict(), sort_keys=True))
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    github = subparsers.add_parser("github")
    github.add_argument("--event-name", required=True)
    github.add_argument("--event-path", type=Path, required=True)
    github.add_argument("--sha", default="")
    github.add_argument("--output", type=Path, required=True)
    github.set_defaults(func=_github_command)

    paths = subparsers.add_parser("paths")
    paths.add_argument("--all", action="store_true")
    paths.add_argument("paths", nargs="*")
    paths.set_defaults(func=_paths_command)

    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
