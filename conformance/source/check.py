#!/usr/bin/env python3
"""Milestone A — production Swift/Go must not link retired host APIs.

Decision B (Phase 1): no FileProvider / NSFileProvider* / ubiquitousItem*
/ MetricKit / MXMetric* in production Swift or Go.

Excluded on purpose:
- Generated UniFFI (`GalleryCore.swift`) — `VfsProviderAttrs` stays until
  Phase 2 lifts it off `Vfs`.
- `apps/gallery/linux/swift-shim/` — copy of the same generated surface.
- `apps/health/reference/web-ui/` — preserved brief, not the product.
- `vendor/` — third-party Go.

Usage (from the monorepo root):

    python3 conformance/source/check.py
    python3 conformance/source/check.py --self-test
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]

PATTERNS: tuple[tuple[str, re.Pattern[str]], ...] = (
    ("import FileProvider", re.compile(r"\bimport\s+FileProvider\b")),
    ("NSFileProvider", re.compile(r"\bNSFileProvider")),
    ("ubiquitousItem", re.compile(r"\bubiquitousItem")),
    ("import MetricKit", re.compile(r"\bimport\s+MetricKit\b")),
    ("MXMetric", re.compile(r"\bMXMetric")),
)

SKIP_DIR_NAMES = frozenset(
    {
        "vendor",
        "reference",
        "swift-shim",
        ".git",
        "target",
        ".build",
        "node_modules",
    }
)

SKIP_FILE_NAMES = frozenset({"GalleryCore.swift"})


@dataclass(frozen=True)
class Finding:
    path: str
    line: int
    token: str
    text: str


def is_skipped(path: Path, root: Path) -> bool:
    rel = path.relative_to(root)
    if path.name in SKIP_FILE_NAMES:
        return True
    return any(part in SKIP_DIR_NAMES for part in rel.parts)


def iter_sources(root: Path) -> list[Path]:
    apps = root / "apps"
    out: list[Path] = []
    for app in ("gallery", "contacts", "music", "health"):
        base = apps / app
        if not base.is_dir():
            continue
        for path in base.rglob("*"):
            if not path.is_file():
                continue
            if path.suffix not in {".swift", ".go"}:
                continue
            if is_skipped(path, root):
                continue
            out.append(path)
    return sorted(out)


def scan_text(rel: str, text: str) -> list[Finding]:
    found: list[Finding] = []
    for i, line in enumerate(text.splitlines(), start=1):
        for token, pat in PATTERNS:
            if pat.search(line):
                found.append(Finding(rel, i, token, line.strip()))
    return found


def findings_for(root: Path) -> list[Finding]:
    found: list[Finding] = []
    for path in iter_sources(root):
        rel = str(path.relative_to(root))
        found.extend(scan_text(rel, path.read_text(errors="replace")))
    return found


def render(found: list[Finding]) -> str:
    if not found:
        return "source check: green (no FileProvider / MetricKit APIs)\n"
    lines = [f"source check: red ({len(found)} hit(s))\n"]
    for f in found:
        lines.append(f"  {f.path}:{f.line}: {f.token}: {f.text}\n")
    return "".join(lines)


def run_self_test() -> int:
    hits = scan_text(
        "fake.swift",
        "\n".join(
            [
                "import FileProvider",
                "let x = NSFileProviderManager.default",
                "keys.append(.ubiquitousItemDownloadingStatusKey)",
                "import MetricKit",
                "class S: NSObject, MXMetricManagerSubscriber {}",
                "let isFileProvider = false",
                "FileProviderDetector.probe(url)",
            ]
        ),
    )
    tokens = [h.token for h in hits]
    assert tokens == [
        "import FileProvider",
        "NSFileProvider",
        "ubiquitousItem",
        "import MetricKit",
        "MXMetric",
    ], tokens

    skipped = is_skipped(REPO / "apps/gallery/LocalGallery/GalleryCore.swift", REPO)
    assert skipped
    skipped_ref = is_skipped(
        REPO / "apps/health/reference/web-ui/serve.go", REPO
    )
    assert skipped_ref
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
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)

    if args.self_test:
        return run_self_test()

    root = args.root.resolve()
    found = findings_for(root)
    print(render(found), end="")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
