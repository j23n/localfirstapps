#!/usr/bin/env python3
"""ADR 0004 R11 — no colour literals in GTK shell CSS or Rust.

Scans `shells/**/*.css` and `shells/**/*.rs`. Generated token CSS
(`design/tokens/generated/**`) is excluded: those hex values are the
emitted form shells consume.

Fails on `#RRGGBB` / `#RGB` literals and `@define-color … #…`.
Does not inspect Swift; `Color(red:)` stays
`shells/shell-kit-swift/scripts/check.py`.

Usage (from the monorepo root):

    python3 conformance/colour/check.py
    python3 conformance/colour/check.py --self-test
"""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]

HEX_LITERAL = re.compile(r"#(?:[0-9A-Fa-f]{6}|[0-9A-Fa-f]{3})(?![0-9A-Fa-f])")
DEFINE_COLOR_HEX = re.compile(r"@define-color\s+\S+\s+#")
CFG_TEST_RE = re.compile(r"#\s*\[\s*cfg\s*\([^)]*\btest\b[^)]*\)\s*\]")

SKIP_DIR_NAMES = frozenset(
    {
        ".git",
        ".build",
        ".worktrees",
        "target",
        "vendor",
        "node_modules",
    }
)
GENERATED_PREFIX = "design/tokens/generated"


@dataclass(frozen=True)
class Finding:
    path: str
    line: int
    token: str
    text: str


def is_generated(path: Path, root: Path) -> bool:
    parts = path.relative_to(root).parts
    marker = tuple(GENERATED_PREFIX.split("/"))
    for index in range(len(parts) - len(marker) + 1):
        if parts[index : index + len(marker)] == marker:
            return True
    return False


def is_skipped(path: Path, root: Path) -> bool:
    if is_generated(path, root):
        return True
    rel = path.relative_to(root)
    return any(part in SKIP_DIR_NAMES for part in rel.parts)


def _matching_brace(code: str, opening: int) -> int:
    depth = 0
    for index in range(opening, len(code)):
        if code[index] == "{":
            depth += 1
        elif code[index] == "}":
            depth -= 1
            if depth == 0:
                return index + 1
    return len(code)


def _mask_span(chars: list[str], start: int, end: int) -> None:
    for index in range(start, min(end, len(chars))):
        if chars[index] != "\n":
            chars[index] = " "


def strip_rust_test_items(code: str) -> str:
    """Blank items guarded by cfg(test); production checks ignore test code."""
    chars = list(code)
    cursor = 0
    while True:
        match = CFG_TEST_RE.search(code, cursor)
        if match is None:
            break
        brace = code.find("{", match.end())
        semicolon = code.find(";", match.end())
        if semicolon >= 0 and (brace < 0 or semicolon < brace):
            end = semicolon + 1
        elif brace >= 0:
            end = _matching_brace(code, brace)
        else:
            end = len(code)
        _mask_span(chars, match.start(), end)
        cursor = end
    return "".join(chars)


def production_text(path: Path) -> str:
    text = path.read_text(encoding="utf-8", errors="replace")
    if path.suffix == ".rs":
        return strip_rust_test_items(text)
    return text


def iter_sources(root: Path) -> list[Path]:
    shells = root / "shells"
    if not shells.is_dir():
        return []
    out: list[Path] = []
    for path in shells.rglob("*"):
        if not path.is_file():
            continue
        if path.suffix not in {".css", ".rs"}:
            continue
        if is_skipped(path, root):
            continue
        out.append(path)
    return sorted(out)


def scan_text(rel: str, text: str) -> list[Finding]:
    found: list[Finding] = []
    for i, line in enumerate(text.splitlines(), start=1):
        if DEFINE_COLOR_HEX.search(line):
            found.append(Finding(rel, i, "@define-color hex", line.strip()))
        for match in HEX_LITERAL.finditer(line):
            found.append(Finding(rel, i, match.group(0), line.strip()))
    return found


def findings_for(root: Path) -> list[Finding]:
    found: list[Finding] = []
    for path in iter_sources(root):
        rel = str(path.relative_to(root))
        found.extend(scan_text(rel, production_text(path)))
    return found


def render(found: list[Finding]) -> str:
    if not found:
        return "colour check: green (no #RGB / #RRGGBB literals in shells)\n"
    lines = [f"colour check: red ({len(found)} hit(s))\n"]
    for finding in found:
        lines.append(
            f"  {finding.path}:{finding.line}: {finding.token}: {finding.text}\n"
        )
    return "".join(lines)


def run_self_test() -> int:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        app = root / "shells" / "app" / "src"
        app.mkdir(parents=True)
        (app / "bad.rs").write_text('const ACCENT: &str = "#336BC7";\n', encoding="utf-8")
        (app / "short.css").write_text("label { color: #abc; }\n", encoding="utf-8")
        (app / "define.css").write_text(
            "@define-color accent_bg_color #00ff00;\n", encoding="utf-8"
        )
        (app / "ok.css").write_text(
            "@define-color accent_bg_color var(--accent-bg-color);\n",
            encoding="utf-8",
        )
        (app / "ok.rs").write_text(
            'fn load() { let _ = "var(--accent-bg-color)"; }\n'
            "#[cfg(test)]\n"
            "mod tests {\n"
            '    fn pin_generated() { let _ = "#FFFFFF"; }\n'
            "}\n",
            encoding="utf-8",
        )
        (app / "attrs.rs").write_text("#[derive(Clone)]\nstruct Row;\n", encoding="utf-8")
        generated = root / "design" / "tokens" / "generated"
        generated.mkdir(parents=True)
        generated_css = generated / "contacts.css"
        generated_css.write_text(
            ":root { --accent-bg-color: #336BC7; }\n", encoding="utf-8"
        )
        # A generated path must never be walked even if it appears under shells/.
        nested = root / "shells" / "design" / "tokens" / "generated"
        nested.mkdir(parents=True)
        (nested / "leak.css").write_text("x { color: #123456; }\n", encoding="utf-8")

        assert is_generated(generated_css, root)
        assert not is_generated(app / "ok.css", root)

        found = findings_for(root)
        tokens = [(f.path, f.token) for f in found]
        assert (
            "shells/app/src/bad.rs",
            "#336BC7",
        ) in tokens, tokens
        assert ("shells/app/src/short.css", "#abc") in tokens, tokens
        assert ("shells/app/src/define.css", "@define-color hex") in tokens, tokens
        assert ("shells/app/src/define.css", "#00ff00") in tokens, tokens
        assert all("ok.css" not in path for path, _ in tokens), tokens
        assert all("ok.rs" not in path for path, _ in tokens), tokens
        assert all("attrs.rs" not in path for path, _ in tokens), tokens
        assert all("generated" not in path for path, _ in tokens), tokens
        assert all(f.token != "Color(red:)" for f in found)

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
