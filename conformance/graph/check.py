#!/usr/bin/env python3
"""ADR 0002 R13 — dependency-graph check over the core lockfiles.

Walks the resolved graph, not source text. A source grep would have
certified gallery-geo's Nominatim client as clean; this check does not.

Both `core/Cargo.lock` and `apps/gallery/core/Cargo.lock` are scanned
when they exist, and findings are unioned by package name. Preferring
only `core/` would hide `gallery-geo → ureq` the moment the extracted
workspace exists. `--lockfile` remains a single-file override.

Usage (from the monorepo root):

    python3 conformance/graph/check.py
    python3 conformance/graph/check.py --expect-violations gallery-geo
    python3 conformance/graph/check.py --self-test

Exit 0 when the graph is clean, or when --expect-violations matches
exactly. Exit 1 on a real mismatch. Exit 2 on usage / IO errors.

Traversal: from each workspace package, follow third-party edges.
Allowlisted packages are not entered (ort's ureq stays invisible).
Sibling workspace packages are not entered (gallery-ffi does not
inherit gallery-geo's finding). The finding is the workspace crate
that introduced the edge.
"""

from __future__ import annotations

import argparse
import json
import sys
import tomllib
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]


@dataclass(frozen=True)
class Package:
    name: str
    version: str
    source: str | None
    deps: tuple[str, ...]  # dependency names (version stripped)

    @property
    def workspace(self) -> bool:
        return self.source is None


@dataclass
class Policy:
    allow: frozenset[str]
    allow_entries: list[dict]
    forbidden: dict[str, str]  # crate name -> class (network/ui/platform)

    @property
    def allow_entry_count(self) -> int:
        return len(self.allow_entries)


@dataclass
class Finding:
    package: str
    via: str
    klass: str


def _unquote(value: str) -> str:
    value = value.strip()
    if value.startswith('"') and value.endswith('"'):
        return value[1:-1]
    return value


def parse_cargo_lock(text: str) -> list[Package]:
    """Parse Cargo.lock package stanzas. Subset of TOML, line-oriented."""
    packages: list[Package] = []
    in_package = False
    in_deps = False
    name = version = source = None
    deps: list[str] = []

    def flush() -> None:
        nonlocal in_package, in_deps, name, version, source, deps
        if in_package and name is not None and version is not None:
            packages.append(
                Package(name=name, version=version, source=source, deps=tuple(deps))
            )
        in_package = False
        in_deps = False
        name = version = source = None
        deps = []

    for raw in text.splitlines():
        stripped = raw.strip()
        if stripped == "[[package]]":
            flush()
            in_package = True
            continue
        if not in_package:
            continue
        if in_deps:
            if stripped == "]":
                in_deps = False
            elif stripped:
                item = stripped.strip(",")
                if item:
                    deps.append(_unquote(item).split()[0])
            continue
        if stripped.startswith("name = "):
            name = _unquote(stripped.split("=", 1)[1])
        elif stripped.startswith("version = "):
            version = _unquote(stripped.split("=", 1)[1])
        elif stripped.startswith("source = "):
            source = _unquote(stripped.split("=", 1)[1])
        elif stripped.startswith("dependencies = ["):
            rest = stripped[len("dependencies = [") :].strip()
            if rest.endswith("]"):
                inner = rest[:-1].strip()
                if inner:
                    for part in inner.split(","):
                        part = part.strip()
                        if part:
                            deps.append(_unquote(part).split()[0])
            elif rest:
                deps.append(_unquote(rest.strip(",")).split()[0])
                in_deps = True
            else:
                in_deps = True
    flush()
    return packages


def load_policy(allow_path: Path, policy_path: Path) -> Policy:
    allow_doc = tomllib.loads(allow_path.read_text())
    policy_doc = tomllib.loads(policy_path.read_text())
    entries = list(allow_doc.get("allow") or [])
    allow: set[str] = set()
    for entry in entries:
        allow.update(entry.get("crates") or [])
    forbidden: dict[str, str] = {}
    for klass in ("network", "ui", "platform"):
        for crate in policy_doc.get(klass, {}).get("crates") or []:
            forbidden[crate] = klass
    return Policy(allow=frozenset(allow), allow_entries=entries, forbidden=forbidden)


LOCKFILE_RELS = ("core/Cargo.lock", "apps/gallery/core/Cargo.lock")


def find_lockfiles(root: Path) -> list[Path]:
    """Every default lockfile that exists.

    Both paths are scanned until gallery-geo is deleted. Returning only
    the first hit would hide that crate once `core/Cargo.lock` exists.
    """
    found = [root / rel for rel in LOCKFILE_RELS if (root / rel).is_file()]
    if not found:
        raise FileNotFoundError(
            "no core Cargo.lock (looked at core/ and apps/gallery/core/)"
        )
    return found


def union_findings(groups: list[list[Finding]]) -> list[Finding]:
    """Merge findings from several lockfiles, one row per workspace crate."""
    by_package: dict[str, Finding] = {}
    for group in groups:
        for finding in group:
            by_package.setdefault(finding.package, finding)
    return [by_package[name] for name in sorted(by_package)]


def _packages_by_name(packages: list[Package]) -> dict[str, list[Package]]:
    by_name: dict[str, list[Package]] = defaultdict(list)
    for pkg in packages:
        by_name[pkg.name].append(pkg)
    return by_name


def findings_for(packages: list[Package], policy: Policy) -> list[Finding]:
    by_name = _packages_by_name(packages)
    workspace = [p for p in packages if p.workspace]
    workspace_names = {p.name for p in workspace}
    out: list[Finding] = []
    seen: set[str] = set()

    def walk(start: str, crate: str, stack: set[str]) -> Finding | None:
        if crate in policy.allow:
            return None
        if crate in policy.forbidden:
            return Finding(package=start, via=crate, klass=policy.forbidden[crate])
        if crate in workspace_names and crate != start:
            return None
        if crate in stack:
            return None
        stack.add(crate)
        try:
            for pkg in by_name.get(crate, []):
                for dep in pkg.deps:
                    hit = walk(start, dep, stack)
                    if hit:
                        return hit
        finally:
            stack.remove(crate)
        return None

    for pkg in sorted(workspace, key=lambda p: p.name):
        if pkg.name in seen:
            continue
        hit = walk(pkg.name, pkg.name, set())
        if hit:
            out.append(hit)
            seen.add(pkg.name)
    return out


def render(lockfiles: list[Path], policy: Policy, found: list[Finding]) -> str:
    lines: list[str] = []
    status = "RED" if found else "GREEN"
    lines.append(f"ADR 0002 R13 — dependency graph  [{status}]")
    if len(lockfiles) == 1:
        lines.append(f"lockfile: {lockfiles[0]}")
    else:
        lines.append("lockfiles:")
        for lockfile in lockfiles:
            lines.append(f"  {lockfile}")
    allow = ", ".join(sorted(policy.allow)) or "(empty)"
    lines.append(f"allowlist ({policy.allow_entry_count} "
                 f"{'entry' if policy.allow_entry_count == 1 else 'entries'}): {allow}")
    if policy.allow_entry_count != 1:
        lines.append(
            "warning: ADR 0002 R13 permits one allowlist entry without an "
            "amendment; this file has "
            f"{policy.allow_entry_count}."
        )
    for entry in policy.allow_entries:
        why = entry.get("why", "")
        offline = entry.get("offline", "")
        crates = ", ".join(entry.get("crates") or [])
        if why:
            lines.append(f"  {crates}: {why}")
        if offline:
            lines.append(f"  offline: {offline}")
    if found:
        lines.append("")
        lines.append("un-allowlisted entries:")
        for f in found:
            lines.append(f"  {f.package}")
            lines.append(f"    via: {f.via} ({f.klass})")
        lines.append("")
        lines.append(
            "A source grep would have missed a client that lives in a "
            "dependency. Phase 2 removes gallery-geo once localcore-geo exists."
        )
    else:
        lines.append("no networking, UI, or platform crate outside the allowlist.")
    return "\n".join(lines) + "\n"


def run_self_test() -> int:
    policy = Policy(
        allow=frozenset({"ort", "ort-sys"}),
        allow_entries=[{"crates": ["ort", "ort-sys"]}],
        forbidden={"ureq": "network", "gtk": "ui"},
    )

    geo = parse_cargo_lock(
        """
version = 4
[[package]]
name = "gallery-geo"
version = "0.1.0"
dependencies = [
 "ureq 2.12.1",
]
[[package]]
name = "ureq"
version = "2.12.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "00"
"""
    )
    got = {f.package for f in findings_for(geo, policy)}
    assert got == {"gallery-geo"}, got

    ort = parse_cargo_lock(
        """
[[package]]
name = "gallery-ml"
version = "0.1.0"
dependencies = [
 "ort",
]
[[package]]
name = "ort"
version = "2.0.0-rc.13"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "00"
dependencies = [
 "ureq",
]
[[package]]
name = "ureq"
version = "3.3.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "00"
"""
    )
    got = {f.package for f in findings_for(ort, policy)}
    assert got == set(), got

    both = parse_cargo_lock(
        """
[[package]]
name = "gallery-ffi"
version = "0.1.0"
dependencies = [
 "gallery-geo",
 "gallery-ml",
]
[[package]]
name = "gallery-geo"
version = "0.1.0"
dependencies = [
 "ureq",
]
[[package]]
name = "gallery-ml"
version = "0.1.0"
dependencies = [
 "ort",
]
[[package]]
name = "ort"
version = "2.0.0-rc.13"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "00"
dependencies = [
 "ureq",
]
[[package]]
name = "ureq"
version = "2.12.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "00"
"""
    )
    got = {f.package for f in findings_for(both, policy)}
    assert got == {"gallery-geo"}, got

    gtk = parse_cargo_lock(
        """
[[package]]
name = "oops"
version = "0.1.0"
dependencies = [
 "gtk",
]
[[package]]
name = "gtk"
version = "0.8.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "00"
"""
    )
    found = findings_for(gtk, policy)
    assert [(f.package, f.klass) for f in found] == [("oops", "ui")], found

    # A clean lockfile must not hide a finding that lives only in the other.
    # This is why both default lockfiles are walked rather than preferring
    # core/ once that workspace exists.
    merged = union_findings([findings_for(ort, policy), findings_for(geo, policy)])
    assert [f.package for f in merged] == ["gallery-geo"], merged
    assert union_findings([[], findings_for(geo, policy)])[0].package == "gallery-geo"
    assert union_findings([findings_for(geo, policy), findings_for(geo, policy)]) == [
        Finding(package="gallery-geo", via="ureq", klass="network")
    ]

    print("self-test: ok")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument(
        "--root",
        type=Path,
        default=REPO,
        help="monorepo root (default: two levels above this script)",
    )
    parser.add_argument("--lockfile", type=Path, default=None)
    parser.add_argument(
        "--expect-violations",
        metavar="CRATE",
        nargs="*",
        default=None,
        help="succeed only when the finding set is exactly these workspace crates",
    )
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)

    if args.self_test:
        return run_self_test()

    root = args.root.resolve()
    if args.lockfile:
        lockfile = args.lockfile.resolve()
        if not lockfile.is_file():
            print(f"error: lockfile not found: {lockfile}", file=sys.stderr)
            return 2
        lockfiles = [lockfile]
    else:
        try:
            lockfiles = find_lockfiles(root)
        except FileNotFoundError as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 2

    policy = load_policy(HERE / "allowlist.toml", HERE / "policy.toml")
    groups: list[list[Finding]] = []
    scanned: list[Path] = []
    for lockfile in lockfiles:
        packages = parse_cargo_lock(lockfile.read_text())
        if not packages:
            if args.lockfile:
                print(f"error: no packages in {lockfile}", file=sys.stderr)
                return 2
            # Empty extracted workspace (members = []) is not a finding
            # source. Skip it so it cannot hide the gallery lockfile.
            continue
        scanned.append(lockfile)
        groups.append(findings_for(packages, policy))
    if not scanned:
        print(
            "error: no packages in any scanned lockfile: "
            + ", ".join(str(p) for p in lockfiles),
            file=sys.stderr,
        )
        return 2
    found = union_findings(groups)
    names = [f.package for f in found]

    if args.json:
        print(
            json.dumps(
                {
                    "status": "red" if found else "green",
                    "lockfiles": [str(p) for p in lockfiles],
                    "allowlist": sorted(policy.allow),
                    "findings": [
                        {"package": f.package, "via": f.via, "class": f.klass}
                        for f in found
                    ],
                },
                indent=2,
            )
        )
    else:
        print(render(lockfiles, policy, found), end="")

    if args.expect_violations is not None:
        expected = list(args.expect_violations)
        if names == expected:
            if not args.json:
                print(f"expected red: {', '.join(expected)}  (matched)")
            return 0
        print(
            "error: findings "
            f"{names or '[]'} != expected {expected or '[]'}",
            file=sys.stderr,
        )
        return 1

    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
