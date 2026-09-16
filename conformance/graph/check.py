#!/usr/bin/env python3
"""ADR 0002 R13 — dependency tripwire over the core lockfiles.

Walks the resolved graph, not source text, looking for curated families of
networking, UI, and platform crates. A source grep would have certified a
Nominatim client as clean; this tripwire did not. Passing it is not proof that
no dependency can open a socket.

Both `core/Cargo.lock` and `apps/gallery/core/Cargo.lock` are scanned
when they exist, and findings are unioned by package name. Preferring
only `core/` would hide a networking crate that lives only in the
extracted workspace. `--lockfile` remains a single-file override.

Usage (from the monorepo root):

    python3 conformance/graph/check.py
    python3 conformance/graph/check.py --expect-violations gallery-geo
    python3 conformance/graph/check.py --self-test

Exit 0 when the graph is clean, or when --expect-violations matches
exactly. Exit 1 on a real mismatch. Exit 2 on usage / IO errors.

Traversal: from each workspace package, follow third-party edges.
Reviewed build-time exception roots are not entered (ort's ureq stays
invisible as a finding), but their expected policy hits are checked exactly.
Sibling workspace packages are not entered (a clean crate does not
inherit a sibling's finding). The finding is the workspace crate
that introduced the edge.
"""

from __future__ import annotations

import argparse
import json
import re
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
    allow_entries: tuple["AllowEntry", ...]
    forbidden: dict[str, str]  # crate name -> class (network/ui/platform)

    @property
    def allow_entry_count(self) -> int:
        return len(self.allow_entries)


@dataclass(frozen=True)
class AllowEntry:
    crates: tuple[str, ...]
    expected_policy_hits: frozenset[str]
    scope: str
    why: str
    offline: str
    reviewed: str


class PolicyError(ValueError):
    """The checked-in policy or allowlist is invalid or has drifted."""


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


CRATE_NAME = re.compile(r"^[A-Za-z0-9_-]+$")
ALLOW_KEYS = {
    "crates",
    "expected_policy_hits",
    "scope",
    "why",
    "offline",
    "reviewed",
}
POLICY_CLASSES = ("network", "ui", "platform")


def _nonempty_string(table: dict, key: str, label: str) -> str:
    value = table.get(key)
    if not isinstance(value, str) or not value.strip():
        raise PolicyError(f"{label}.{key} must be a non-empty string")
    return value.strip()


def _crate_list(table: dict, key: str, label: str) -> tuple[str, ...]:
    value = table.get(key)
    if not isinstance(value, list) or not value:
        raise PolicyError(f"{label}.{key} must be a non-empty array of crate names")
    if any(
        not isinstance(crate, str) or not CRATE_NAME.fullmatch(crate)
        for crate in value
    ):
        raise PolicyError(f"{label}.{key} contains a malformed crate name")
    duplicates = sorted({crate for crate in value if value.count(crate) > 1})
    if duplicates:
        raise PolicyError(
            f"{label}.{key} contains duplicate crates: {', '.join(duplicates)}"
        )
    return tuple(value)


def _policy_from_docs(allow_doc: dict, policy_doc: dict) -> Policy:
    unknown_roots = sorted(set(allow_doc) - {"allow"})
    if unknown_roots:
        raise PolicyError(
            "allowlist has unknown top-level keys: " + ", ".join(unknown_roots)
        )

    raw_entries = allow_doc.get("allow", [])
    if not isinstance(raw_entries, list):
        raise PolicyError("allowlist.allow must be an array of tables")

    entries: list[AllowEntry] = []
    owners: dict[str, int] = {}
    for index, raw in enumerate(raw_entries):
        label = f"allow[{index}]"
        if not isinstance(raw, dict):
            raise PolicyError(f"{label} must be a table")
        missing = sorted(ALLOW_KEYS - set(raw))
        unknown = sorted(set(raw) - ALLOW_KEYS)
        if missing:
            raise PolicyError(f"{label} is missing fields: {', '.join(missing)}")
        if unknown:
            raise PolicyError(f"{label} has unknown fields: {', '.join(unknown)}")

        crates = _crate_list(raw, "crates", label)
        expected = _crate_list(raw, "expected_policy_hits", label)
        scope = _nonempty_string(raw, "scope", label)
        if scope != "build-time":
            raise PolicyError(
                f"{label}.scope must be 'build-time', got {scope!r}"
            )

        for crate in crates:
            if crate in owners:
                raise PolicyError(
                    f"{label}.crates duplicates {crate!r} from "
                    f"allow[{owners[crate]}]"
                )
            owners[crate] = index

        entries.append(
            AllowEntry(
                crates=crates,
                expected_policy_hits=frozenset(expected),
                scope=scope,
                why=_nonempty_string(raw, "why", label),
                offline=_nonempty_string(raw, "offline", label),
                reviewed=_nonempty_string(raw, "reviewed", label),
            )
        )

    unknown_classes = sorted(set(policy_doc) - set(POLICY_CLASSES))
    missing_classes = sorted(set(POLICY_CLASSES) - set(policy_doc))
    if unknown_classes:
        raise PolicyError(
            "policy has unknown top-level keys: " + ", ".join(unknown_classes)
        )
    if missing_classes:
        raise PolicyError(
            "policy is missing classes: " + ", ".join(missing_classes)
        )

    forbidden: dict[str, str] = {}
    for klass in POLICY_CLASSES:
        raw = policy_doc[klass]
        if not isinstance(raw, dict) or set(raw) != {"crates"}:
            raise PolicyError(f"policy.{klass} must contain only a crates array")
        for crate in _crate_list(raw, "crates", f"policy.{klass}"):
            previous = forbidden.get(crate)
            if previous is not None:
                raise PolicyError(
                    f"policy crate {crate!r} is duplicated in {previous} and {klass}"
                )
            forbidden[crate] = klass

    for index, entry in enumerate(entries):
        unknown_hits = sorted(entry.expected_policy_hits - forbidden.keys())
        if unknown_hits:
            raise PolicyError(
                f"allow[{index}].expected_policy_hits names crates absent from "
                f"policy.toml: {', '.join(unknown_hits)}"
            )

    return Policy(
        allow=frozenset(owners),
        allow_entries=tuple(entries),
        forbidden=forbidden,
    )


def load_policy(allow_path: Path, policy_path: Path) -> Policy:
    allow_doc = tomllib.loads(allow_path.read_text())
    policy_doc = tomllib.loads(policy_path.read_text())
    return _policy_from_docs(allow_doc, policy_doc)


LOCKFILE_RELS = ("core/Cargo.lock", "apps/gallery/core/Cargo.lock")


def find_lockfiles(root: Path) -> list[Path]:
    """Every default lockfile that exists.

    Both paths are scanned. Returning only the first hit would hide a
    networking crate that lives only in the other workspace.
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


def _reachable_third_party(packages: list[Package]) -> set[str]:
    by_name = _packages_by_name(packages)
    workspace = [pkg for pkg in packages if pkg.workspace]
    workspace_names = {pkg.name for pkg in workspace}
    reachable: set[str] = set()

    def walk(start: str, crate: str, stack: set[str]) -> None:
        if crate in workspace_names and crate != start:
            return
        if crate in stack:
            return
        if crate not in workspace_names:
            reachable.add(crate)
        stack.add(crate)
        try:
            for pkg in by_name.get(crate, []):
                for dep in pkg.deps:
                    walk(start, dep, stack)
        finally:
            stack.remove(crate)

    for pkg in workspace:
        walk(pkg.name, pkg.name, set())
    return reachable


def _forbidden_below(
    roots: tuple[str, ...], packages: list[Package], forbidden: dict[str, str]
) -> set[str]:
    by_name = _packages_by_name(packages)
    hits: set[str] = set()
    seen: set[str] = set()
    todo = list(roots)
    while todo:
        crate = todo.pop()
        if crate in seen:
            continue
        seen.add(crate)
        if crate in forbidden:
            hits.add(crate)
        for pkg in by_name.get(crate, []):
            todo.extend(pkg.deps)
    return hits


def validate_allowlist_usage(
    policy: Policy, package_groups: list[list[Package]]
) -> None:
    """Reject stale exceptions and undocumented changes below their roots."""
    reachable: set[str] = set()
    for packages in package_groups:
        reachable.update(_reachable_third_party(packages))

    errors: list[str] = []
    for index, entry in enumerate(policy.allow_entries):
        unused_roots = sorted(set(entry.crates) - reachable)
        if unused_roots:
            errors.append(
                f"allow[{index}] has unused crates: {', '.join(unused_roots)}"
            )

        actual_hits: set[str] = set()
        for packages in package_groups:
            actual_hits.update(
                _forbidden_below(entry.crates, packages, policy.forbidden)
            )
        undocumented = sorted(actual_hits - entry.expected_policy_hits)
        stale = sorted(entry.expected_policy_hits - actual_hits)
        if undocumented:
            errors.append(
                f"allow[{index}] has undocumented policy hits: "
                + ", ".join(undocumented)
            )
        if stale:
            errors.append(
                f"allow[{index}] has unused expected_policy_hits: "
                + ", ".join(stale)
            )

    if errors:
        raise PolicyError("; ".join(errors))


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
    lines.append(f"ADR 0002 R13 — dependency tripwire  [{status}]")
    if len(lockfiles) == 1:
        lines.append(f"lockfile: {lockfiles[0]}")
    else:
        lines.append("lockfiles:")
        for lockfile in lockfiles:
            lines.append(f"  {lockfile}")
    allow = ", ".join(sorted(policy.allow)) or "(empty)"
    lines.append(
        f"reviewed build-time exceptions ({policy.allow_entry_count} "
        f"{'entry' if policy.allow_entry_count == 1 else 'entries'}): {allow}"
    )
    for entry in policy.allow_entries:
        crates = ", ".join(entry.crates)
        expected = ", ".join(sorted(entry.expected_policy_hits))
        lines.append(f"  {crates}: {entry.why}")
        lines.append(f"    reviewed: {entry.reviewed}")
        lines.append(f"    expected policy hits: {expected}")
        lines.append(f"    offline: {entry.offline}")
    if found:
        lines.append("")
        lines.append("un-excepted policy hits:")
        for f in found:
            lines.append(f"  {f.package}")
            lines.append(f"    via: {f.via} ({f.klass})")
        lines.append("")
        lines.append(
            "This tripwire found a curated dependency family that requires "
            "review; it does not inspect runtime socket behavior."
        )
    else:
        lines.append(
            "no known networking, UI, or platform crate outside the reviewed "
            "exceptions; this tripwire is not proof that dependencies cannot "
            "open sockets."
        )
    return "\n".join(lines) + "\n"


def run_self_test() -> int:
    ort_entry = AllowEntry(
        crates=("ort", "ort-sys"),
        expected_policy_hits=frozenset({"ureq"}),
        scope="build-time",
        why="test fixture",
        offline="ORT_LIB_LOCATION=/fixture",
        reviewed="ADR 0002 R13",
    )
    policy = Policy(
        allow=frozenset({"ort", "ort-sys"}),
        allow_entries=(ort_entry,),
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
 "ort-sys",
 "ureq",
]
[[package]]
name = "ort-sys"
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

    # Allowlist conformance is exact. Zero exceptions is the default; every
    # checked-in exception must remain used and must document the precise
    # curated policy hits hidden below its roots.
    validate_allowlist_usage(policy, [ort])
    zero_policy = Policy(
        allow=frozenset(),
        allow_entries=(),
        forbidden=policy.forbidden,
    )
    validate_allowlist_usage(zero_policy, [geo])

    def expect_policy_error(call, text: str) -> None:
        try:
            call()
        except PolicyError as exc:
            assert text in str(exc), exc
        else:
            raise AssertionError(f"expected PolicyError containing {text!r}")

    drifted_ort = parse_cargo_lock(
        """
[[package]]
name = "gallery-ml"
version = "0.1.0"
dependencies = ["ort"]
[[package]]
name = "ort"
version = "2.0.0-rc.13"
source = "registry+https://github.com/rust-lang/crates.io-index"
dependencies = ["ort-sys", "ureq", "reqwest"]
[[package]]
name = "ort-sys"
version = "2.0.0-rc.13"
source = "registry+https://github.com/rust-lang/crates.io-index"
dependencies = ["ureq"]
[[package]]
name = "ureq"
version = "3.3.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
[[package]]
name = "reqwest"
version = "0.12.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"""
    )
    drift_policy = Policy(
        allow=policy.allow,
        allow_entries=policy.allow_entries,
        forbidden={**policy.forbidden, "reqwest": "network"},
    )
    expect_policy_error(
        lambda: validate_allowlist_usage(drift_policy, [drifted_ort]),
        "undocumented policy hits: reqwest",
    )

    ort_without_network = parse_cargo_lock(
        """
[[package]]
name = "gallery-ml"
version = "0.1.0"
dependencies = ["ort"]
[[package]]
name = "ort"
version = "2.0.0-rc.13"
source = "registry+https://github.com/rust-lang/crates.io-index"
dependencies = ["ort-sys"]
[[package]]
name = "ort-sys"
version = "2.0.0-rc.13"
source = "registry+https://github.com/rust-lang/crates.io-index"
"""
    )
    expect_policy_error(
        lambda: validate_allowlist_usage(policy, [ort_without_network]),
        "unused expected_policy_hits: ureq",
    )
    expect_policy_error(
        lambda: validate_allowlist_usage(policy, [geo]),
        "unused crates: ort, ort-sys",
    )

    raw_entry = {
        "crates": ["ort", "ort-sys"],
        "expected_policy_hits": ["ureq"],
        "scope": "build-time",
        "why": "test fixture",
        "offline": "ORT_LIB_LOCATION=/fixture",
        "reviewed": "ADR 0002 R13",
    }
    raw_policy = {
        "network": {"crates": ["ureq"]},
        "ui": {"crates": ["gtk"]},
        "platform": {"crates": ["metal"]},
    }
    parsed_zero = _policy_from_docs({}, raw_policy)
    assert parsed_zero.allow_entries == ()
    expect_policy_error(
        lambda: _policy_from_docs(
            {"allow": [raw_entry, dict(raw_entry)]}, raw_policy
        ),
        "duplicates 'ort'",
    )
    malformed = dict(raw_entry)
    del malformed["offline"]
    expect_policy_error(
        lambda: _policy_from_docs({"allow": [malformed]}, raw_policy),
        "missing fields: offline",
    )
    unreviewed = dict(raw_entry)
    del unreviewed["reviewed"]
    expect_policy_error(
        lambda: _policy_from_docs({"allow": [unreviewed]}, raw_policy),
        "missing fields: reviewed",
    )

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

    try:
        policy = load_policy(HERE / "allowlist.toml", HERE / "policy.toml")
    except PolicyError as exc:
        print(f"error: graph policy: {exc}", file=sys.stderr)
        return 1
    except (OSError, tomllib.TOMLDecodeError) as exc:
        print(f"error: cannot load graph policy: {exc}", file=sys.stderr)
        return 2

    groups: list[list[Finding]] = []
    package_groups: list[list[Package]] = []
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
        package_groups.append(packages)
        groups.append(findings_for(packages, policy))
    if not scanned:
        print(
            "error: no packages in any scanned lockfile: "
            + ", ".join(str(p) for p in lockfiles),
            file=sys.stderr,
        )
        return 2
    try:
        validate_allowlist_usage(policy, package_groups)
    except PolicyError as exc:
        print(f"error: graph policy: {exc}", file=sys.stderr)
        return 1

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
