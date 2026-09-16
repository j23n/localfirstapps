#!/usr/bin/env python3
"""Emit path-dependency consumers before changing Cargo crate boundaries."""

from __future__ import annotations

import argparse
import json
import tomllib
from collections import defaultdict
from pathlib import Path


def dependency_tables(document: dict) -> list[tuple[str, dict]]:
    tables = []
    for name in ["dependencies", "build-dependencies", "dev-dependencies"]:
        tables.append((name, document.get(name, {})))
    for target, target_table in document.get("target", {}).items():
        for name in ["dependencies", "build-dependencies", "dev-dependencies"]:
            tables.append((f"target:{target}:{name}", target_table.get(name, {})))
    return tables


def packages(root: Path) -> dict[str, tuple[Path, dict]]:
    found = {}
    for manifest in sorted(root.rglob("Cargo.toml")):
        if any(part in {"target", "build"} for part in manifest.parts):
            continue
        try:
            document = tomllib.loads(manifest.read_text(encoding="utf-8"))
        except (OSError, tomllib.TOMLDecodeError):
            continue
        package = document.get("package")
        if package and package.get("name"):
            found[package["name"]] = (manifest, document)
    return found


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--prefix", default="gallery-")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    root = args.root.resolve()
    found = packages(root)
    consumers: dict[str, list[dict[str, str]]] = defaultdict(list)
    for consumer, (manifest, document) in found.items():
        for table, dependencies in dependency_tables(document):
            for alias, spec in dependencies.items():
                if not isinstance(spec, dict) or "path" not in spec:
                    continue
                dependency = spec.get("package", alias)
                consumers[dependency].append(
                    {
                        "consumer": consumer,
                        "kind": table,
                        "manifest": manifest.relative_to(root).as_posix(),
                    }
                )

    rows = []
    for name, (manifest, _) in sorted(found.items()):
        if not name.startswith(args.prefix):
            continue
        direct = sorted(
            consumers.get(name, []),
            key=lambda row: (row["consumer"], row["kind"], row["manifest"]),
        )
        production = [row for row in direct if not row["kind"].endswith("dev-dependencies")]
        rows.append(
            {
                "crate": name,
                "manifest": manifest.relative_to(root).as_posix(),
                "direct_consumers": direct,
                "production_consumer_count": len({row["consumer"] for row in production}),
                "production_consumers": sorted({row["consumer"] for row in production}),
            }
        )

    output = {
        "schema": 1,
        "method": "all repository Cargo.toml path dependencies; feature-unified registry edges excluded",
        "prefix": args.prefix,
        "crates": rows,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(output, sort_keys=True))


if __name__ == "__main__":
    main()
