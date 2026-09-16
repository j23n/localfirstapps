#!/usr/bin/env python3
"""Semantic debt guard for provider state, FFI payloads, and Vfs authority.

This check deliberately complements syntax/record checks:

* Gallery production code must not acquire new provider-placeholder,
  download-state, or QuickLook semantics.
* Contacts UniFFI must not acquire new whole-vCard escape hatches.
* Music UniFFI must not acquire serialized playlist/domain escape hatches.
* Production decisions must not use best-effort `Vfs.exists`; authoritative
  code uses `try_exists` and propagates errors.

Known debt is pinned by exact category/path/symbol/count entries in
`baseline.toml`. Removing debt makes the baseline stale; adding an occurrence
or a new path/symbol fails.

Usage (from the monorepo root):

    python3 conformance/semantic/check.py
    python3 conformance/semantic/check.py --self-test
"""

from __future__ import annotations

import argparse
import os
import re
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
DEFAULT_BASELINE = Path("conformance/semantic/baseline.toml")

GALLERY_KIND = "gallery-provider-semantics"
CONTACTS_KIND = "contacts-serialized-domain-ffi"
MUSIC_KIND = "music-serialized-domain-ffi"
VFS_KIND = "authoritative-vfs-exists"
KINDS = frozenset({GALLERY_KIND, CONTACTS_KIND, MUSIC_KIND, VFS_KIND})

GALLERY_PATTERNS: tuple[tuple[str, re.Pattern[str]], ...] = (
    ("PhotoLocality", re.compile(r"\bPhotoLocality\b")),
    ("DownloadStatus", re.compile(r"\bDownloadStatus\b")),
    ("ScanLocality", re.compile(r"\bScanLocality\b")),
    ("isRemotePlaceholder", re.compile(r"\bisRemotePlaceholder\b")),
    ("remote(downloaded:)", re.compile(r"\bremote\s*\(\s*downloaded\s*:")),
    ("Remote{downloaded}", re.compile(r"\bRemote\s*\{\s*downloaded\b")),
    ("downloadStatus", re.compile(r"\bdownloadStatus\b")),
    ("download_status", re.compile(r"\bdownload_status\b")),
    ("folderPlaceholderPhotos", re.compile(r"\bfolderPlaceholderPhotos\b")),
    (
        "folderPlaceholderTimeZoneOffsets",
        re.compile(r"\bfolderPlaceholderTimeZoneOffsets\b"),
    ),
    ("folder_placeholder_photos", re.compile(r"\bfolder_placeholder_photos\b")),
    (
        "folder_placeholder_time_zone_offsets",
        re.compile(r"\bfolder_placeholder_time_zone_offsets\b"),
    ),
    ("QuickLookThumbnailing", re.compile(r"\bQuickLookThumbnailing\b")),
    ("QLThumbnailGenerator", re.compile(r"\bQLThumbnailGenerator\b")),
    ("useQuickLook", re.compile(r"\buseQuickLook\b")),
    ("decodeQuickLook", re.compile(r"\bdecodeQuickLook\b")),
    ("isMaterializing", re.compile(r"\bisMaterializing\b")),
    ("materializeError", re.compile(r"\bmaterializeError\b")),
)

CONTACTS_ESCAPE_RE = re.compile(r"\bpub\s+fn\s+(vcard_text|save_vcard)\b")
MUSIC_ESCAPE_RE = re.compile(
    r"\bpub\s+fn\s+"
    r"(playlist_text|save_playlist|playlist_json|track_json|library_json)\b"
)
VFS_TYPED_RE = re.compile(
    r"\b(?P<name>[A-Za-z_]\w*)\s*:\s*"
    r"(?:&\s*(?:mut\s+)?(?:dyn\s+)?)?"
    r"(?:(?:Arc|Box)\s*<\s*(?:dyn\s+)?)?"
    r"(?:(?:[A-Za-z_]\w*)::)*(?:Vfs|StdVfs|MemVfs)\b"
)
VFS_LOCAL_RE = re.compile(
    r"\blet\s+(?:mut\s+)?(?P<name>[A-Za-z_]\w*)\s*="
    r"[^;\n]*(?:StdVfs|MemVfs)::new\b"
)
VFS_BOUND_RE = re.compile(
    r"\b(?P<type>[A-Z]\w*)\s*:\s*[^,>{}]*\bVfs\b"
)
VFS_NAMED_CALL_RE = re.compile(
    r"(?:(?P<self>\bself)\s*\.\s*)?"
    r"(?P<name>[A-Za-z_]\w*)\s*\.\s*exists\s*\("
)
VFS_CONSTRUCTED_CALL_RE = re.compile(
    r"\b(?P<type>(?:(?:[A-Za-z_]\w*)::)*(?:StdVfs|MemVfs))"
    r"::new\s*\([^)]*\)\s*\.\s*exists\s*\("
)
CFG_TEST_RE = re.compile(r"#\s*\[\s*cfg\s*\([^)]*\btest\b[^)]*\)\s*\]")

SKIP_DIRS = frozenset(
    {
        ".git",
        ".build",
        ".worktrees",
        "target",
        "vendor",
        "node_modules",
        "tests",
        "fixtures",
        "benches",
        "swift-shim",
    }
)
GENERATED_FILES = frozenset({"GalleryCore.swift"})


class BaselineError(ValueError):
    pass


@dataclass(frozen=True, order=True)
class Finding:
    kind: str
    path: str
    symbol: str
    occurrences: int
    lines: tuple[int, ...]

    @property
    def key(self) -> tuple[str, str, str]:
        return (self.kind, self.path, self.symbol)


@dataclass(frozen=True)
class Allow:
    kind: str
    path: str
    symbol: str
    occurrences: int
    rationale: str
    target_wave: str

    @property
    def key(self) -> tuple[str, str, str]:
        return (self.kind, self.path, self.symbol)


def _mask_span(chars: list[str], start: int, end: int) -> None:
    for index in range(start, end):
        if chars[index] != "\n":
            chars[index] = " "


def mask_non_code(text: str) -> str:
    """Blank C/Rust/Swift comments and string literals, preserving positions."""
    chars = list(text)
    length = len(text)
    index = 0
    while index < length:
        if text.startswith("//", index):
            end = text.find("\n", index + 2)
            end = length if end < 0 else end
            _mask_span(chars, index, end)
            index = end
            continue
        if text.startswith("/*", index):
            start = index
            index += 2
            depth = 1
            while index < length and depth:
                if text.startswith("/*", index):
                    depth += 1
                    index += 2
                elif text.startswith("*/", index):
                    depth -= 1
                    index += 2
                else:
                    index += 1
            _mask_span(chars, start, index)
            continue
        if text.startswith('"""', index):
            start = index
            end = text.find('"""', index + 3)
            index = length if end < 0 else end + 3
            _mask_span(chars, start, index)
            continue
        raw = re.match(r'r(#+)?"', text[index:])
        if raw:
            start = index
            hashes = raw.group(1) or ""
            close = '"' + hashes
            content_start = index + len(raw.group(0))
            end = text.find(close, content_start)
            index = length if end < 0 else end + len(close)
            _mask_span(chars, start, index)
            continue
        if text[index] == '"':
            start = index
            index += 1
            while index < length:
                if text[index] == "\\":
                    index += 2
                    continue
                if text[index] == '"':
                    index += 1
                    break
                index += 1
            _mask_span(chars, start, min(index, length))
            continue
        index += 1
    return "".join(chars)


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


def production_code(path: Path) -> str:
    text = path.read_text(encoding="utf-8", errors="replace")
    code = mask_non_code(text)
    if path.suffix == ".rs":
        code = strip_rust_test_items(code)
    return code


def line_numbers(code: str, matches: Iterable[re.Match[str]]) -> tuple[int, ...]:
    return tuple(code.count("\n", 0, match.start()) + 1 for match in matches)


def gallery_findings(path: str, code: str) -> list[Finding]:
    found: list[Finding] = []
    for symbol, pattern in GALLERY_PATTERNS:
        matches = list(pattern.finditer(code))
        if matches:
            found.append(
                Finding(GALLERY_KIND, path, symbol, len(matches), line_numbers(code, matches))
            )
    return found


def contacts_findings(path: str, code: str) -> list[Finding]:
    grouped: dict[str, list[re.Match[str]]] = {}
    for match in CONTACTS_ESCAPE_RE.finditer(code):
        grouped.setdefault(match.group(1), []).append(match)
    return [
        Finding(CONTACTS_KIND, path, symbol, len(matches), line_numbers(code, matches))
        for symbol, matches in sorted(grouped.items())
    ]


def music_findings(path: str, code: str) -> list[Finding]:
    grouped: dict[str, list[re.Match[str]]] = {}
    for match in MUSIC_ESCAPE_RE.finditer(code):
        grouped.setdefault(match.group(1), []).append(match)
    return [
        Finding(MUSIC_KIND, path, symbol, len(matches), line_numbers(code, matches))
        for symbol, matches in sorted(grouped.items())
    ]


def vfs_findings(path: str, code: str) -> list[Finding]:
    names = {match.group("name") for match in VFS_TYPED_RE.finditer(code)}
    names.update(match.group("name") for match in VFS_LOCAL_RE.finditer(code))
    for bound in VFS_BOUND_RE.finditer(code):
        typ = re.escape(bound.group("type"))
        parameter = re.compile(
            rf"\b(?P<name>[A-Za-z_]\w*)\s*:\s*&\s*(?:mut\s+)?{typ}\b"
        )
        names.update(match.group("name") for match in parameter.finditer(code))
    for match in VFS_NAMED_CALL_RE.finditer(code):
        name = match.group("name")
        if name.lower() == "vfs" or name.lower().endswith("_vfs"):
            names.add(name)

    found: list[Finding] = []
    for name in sorted(names):
        pattern = re.compile(
            rf"(?P<receiver>\bself\s*\.\s*)?\b{re.escape(name)}\s*\.\s*exists\s*\("
        )
        by_symbol: dict[str, list[re.Match[str]]] = {}
        for match in pattern.finditer(code):
            symbol = f"self.{name}.exists" if match.group("receiver") else f"{name}.exists"
            by_symbol.setdefault(symbol, []).append(match)
        for symbol, matches in sorted(by_symbol.items()):
            found.append(
                Finding(VFS_KIND, path, symbol, len(matches), line_numbers(code, matches))
            )
    constructed: dict[str, list[re.Match[str]]] = {}
    for match in VFS_CONSTRUCTED_CALL_RE.finditer(code):
        symbol = f"{match.group('type')}::new().exists"
        constructed.setdefault(symbol, []).append(match)
    for symbol, matches in sorted(constructed.items()):
        found.append(
            Finding(VFS_KIND, path, symbol, len(matches), line_numbers(code, matches))
        )
    return found


def is_skipped(path: Path, root: Path) -> bool:
    rel = path.relative_to(root)
    if path.name in GENERATED_FILES:
        return True
    for part in rel.parts:
        lower = part.lower()
        if lower in SKIP_DIRS or lower.endswith("tests"):
            return True
    return False


def gallery_source_files(root: Path) -> list[Path]:
    files: list[Path] = []
    swift = root / "apps/gallery/LocalGallery"
    if swift.is_dir():
        files.extend(swift.rglob("*.swift"))
    for base in (root / "apps/gallery/core", root / "apps/gallery/linux/src"):
        if not base.is_dir():
            continue
        for path in base.rglob("*.rs"):
            if "src" in path.relative_to(base).parts or base.name == "src":
                files.append(path)
    return sorted({path for path in files if path.is_file() and not is_skipped(path, root)})


def rust_production_files(root: Path) -> list[Path]:
    files: list[Path] = []
    for current, dirnames, filenames in os.walk(root):
        dirnames[:] = [
            dirname
            for dirname in dirnames
            if dirname.lower() not in SKIP_DIRS
            and not dirname.lower().endswith("tests")
        ]
        current_path = Path(current)
        if "src" not in current_path.relative_to(root).parts:
            continue
        for filename in filenames:
            if filename.endswith(".rs"):
                files.append(current_path / filename)
    return sorted(files)


def findings_for(root: Path) -> list[Finding]:
    by_key: dict[tuple[str, str, str], Finding] = {}

    for path in gallery_source_files(root):
        rel = path.relative_to(root).as_posix()
        for finding in gallery_findings(rel, production_code(path)):
            by_key[finding.key] = finding

    contacts_src = root / "core/contacts-ffi/src"
    if contacts_src.is_dir():
        for path in sorted(contacts_src.rglob("*.rs")):
            if is_skipped(path, root):
                continue
            rel = path.relative_to(root).as_posix()
            for finding in contacts_findings(rel, production_code(path)):
                by_key[finding.key] = finding

    music_src = root / "core/music-ffi/src"
    if music_src.is_dir():
        for path in sorted(music_src.rglob("*.rs")):
            if is_skipped(path, root):
                continue
            rel = path.relative_to(root).as_posix()
            for finding in music_findings(rel, production_code(path)):
                by_key[finding.key] = finding

    for path in rust_production_files(root):
        rel = path.relative_to(root).as_posix()
        for finding in vfs_findings(rel, production_code(path)):
            by_key[finding.key] = finding

    return sorted(by_key.values())


def _string(raw: dict[str, Any], field: str, index: int) -> str:
    value = raw.get(field)
    if not isinstance(value, str) or not value.strip():
        raise BaselineError(f"allow {index}.{field} must be a non-empty string")
    return value


def load_baseline(path: Path) -> list[Allow]:
    try:
        doc = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise BaselineError(f"cannot read {path}: {exc}") from exc
    if doc.get("version") != 1:
        raise BaselineError(f"{path}: version must be 1")
    rows = doc.get("allow", [])
    if not isinstance(rows, list):
        raise BaselineError(f"{path}: allow must be an array of tables")

    allowed: list[Allow] = []
    for index, raw in enumerate(rows, start=1):
        if not isinstance(raw, dict):
            raise BaselineError(f"allow {index} must be a table")
        kind = _string(raw, "kind", index)
        if kind not in KINDS:
            raise BaselineError(f"allow {index}.kind is unknown: {kind}")
        rel = _string(raw, "path", index)
        rel_path = Path(rel)
        if rel_path.is_absolute() or ".." in rel_path.parts or rel_path.as_posix() != rel:
            raise BaselineError(f"allow {index}.path must be a normalized relative path")
        occurrences = raw.get("occurrences")
        if not isinstance(occurrences, int) or isinstance(occurrences, bool) or occurrences < 1:
            raise BaselineError(f"allow {index}.occurrences must be a positive integer")
        allowed.append(
            Allow(
                kind=kind,
                path=rel,
                symbol=_string(raw, "symbol", index),
                occurrences=occurrences,
                rationale=_string(raw, "rationale", index),
                target_wave=_string(raw, "target_wave", index),
            )
        )
    keys = [entry.key for entry in allowed]
    if len(keys) != len(set(keys)):
        raise BaselineError(f"{path}: duplicate exact kind/path/symbol entries")
    return allowed


def compare(found: list[Finding], allowed: list[Allow]) -> list[str]:
    actual = {finding.key: finding for finding in found}
    baseline = {entry.key: entry for entry in allowed}
    errors: list[str] = []
    for key in sorted(actual.keys() - baseline.keys()):
        finding = actual[key]
        errors.append(
            "unbaselined: "
            f"{finding.kind} {finding.path}::{finding.symbol} "
            f"({finding.occurrences} occurrence(s), lines {list(finding.lines)})"
        )
    for key in sorted(baseline.keys() - actual.keys()):
        entry = baseline[key]
        errors.append(
            f"stale baseline: {entry.kind} {entry.path}::{entry.symbol} is gone"
        )
    for key in sorted(actual.keys() & baseline.keys()):
        finding = actual[key]
        entry = baseline[key]
        if finding.occurrences != entry.occurrences:
            errors.append(
                "occurrence drift: "
                f"{finding.kind} {finding.path}::{finding.symbol} has "
                f"{finding.occurrences}, baseline has {entry.occurrences} "
                f"(lines {list(finding.lines)})"
            )
    return errors


def _toml_string(value: str) -> str:
    escaped = value.replace("\\", "\\\\").replace('"', '\\"')
    return f'"{escaped}"'


def baseline_metadata(kind: str) -> tuple[str, str]:
    if kind == GALLERY_KIND:
        return (
            "Known provider-era Gallery behavior retained after service retirement.",
            "Phase 5 gallery vertical",
        )
    if kind == CONTACTS_KIND:
        return (
            "Known whole-vCard Contacts FFI escape hatch; records are syntax-green only.",
            "Contacts R6 boundary cleanup",
        )
    if kind == MUSIC_KIND:
        return (
            "Known serialized Music domain payload on the FFI.",
            "Music R6 boundary cleanup",
        )
    return (
        "Known authoritative decision uses best-effort Vfs.exists and can hide IO errors.",
        "Phase 5 gallery vertical",
    )


def render_baseline(found: list[Finding]) -> str:
    lines = [
        "# Generated draft; replace generic text if a finding needs narrower rationale.",
        "version = 1",
    ]
    for finding in found:
        rationale, target = baseline_metadata(finding.kind)
        lines.extend(
            [
                "",
                "[[allow]]",
                f"kind = {_toml_string(finding.kind)}",
                f"path = {_toml_string(finding.path)}",
                f"symbol = {_toml_string(finding.symbol)}",
                f"occurrences = {finding.occurrences}",
                f"rationale = {_toml_string(rationale)}",
                f"target_wave = {_toml_string(target)}",
            ]
        )
    return "\n".join(lines) + "\n"


def run_self_test() -> int:
    swift = """\
// PhotoLocality useQuickLook
let prose = "DownloadStatus QLThumbnailGenerator"
enum PhotoLocality {}
func load(useQuickLook: Bool) {
    _ = photo.locality.isRemotePlaceholder
    _ = QLThumbnailGenerator.shared
}
"""
    swift_findings = {
        finding.symbol: finding.occurrences
        for finding in gallery_findings("Gallery.swift", mask_non_code(swift))
    }
    assert swift_findings == {
        "PhotoLocality": 1,
        "QLThumbnailGenerator": 1,
        "isRemotePlaceholder": 1,
        "useQuickLook": 1,
    }, swift_findings

    rust = """\
fn read(vfs: &dyn Vfs) -> bool { vfs.exists("/user/data") }
fn generic<T: Vfs>(storage: &T) -> bool { storage.exists("/other") }
fn host(path: &Path) -> bool { path.exists() }
#[cfg(test)]
mod tests {
    fn assertion(vfs: &dyn Vfs) { assert!(vfs.exists("/fixture")); }
}
"""
    rust_code = strip_rust_test_items(mask_non_code(rust))
    got_vfs = vfs_findings("src/lib.rs", rust_code)
    assert [(f.symbol, f.occurrences) for f in got_vfs] == [
        ("storage.exists", 1),
        ("vfs.exists", 1),
    ], got_vfs

    contacts = """\
#[uniffi::export]
impl ContactsSession {
    pub fn vcard_text(&self, id: String) -> String { id }
    pub fn save_vcard(&self, text: String) -> String { text }
    // pub fn vcard_copy(&self) {}
}
"""
    got_contacts = contacts_findings("contacts.rs", mask_non_code(contacts))
    assert [finding.symbol for finding in got_contacts] == [
        "save_vcard",
        "vcard_text",
    ], got_contacts

    music = """\
pub fn playlist_text() -> String { String::new() }
pub fn save_playlist() {}
// pub fn track_json() {}
"""
    got_music = music_findings("music.rs", mask_non_code(music))
    assert [finding.symbol for finding in got_music] == [
        "playlist_text",
        "save_playlist",
    ], got_music

    allowed = [
        Allow(f.kind, f.path, f.symbol, f.occurrences, "known", "wave")
        for f in got_vfs + got_contacts + got_music
    ]
    assert compare(got_vfs + got_contacts + got_music, allowed) == []
    drifted = [
        Finding(
            got_vfs[0].kind,
            got_vfs[0].path,
            got_vfs[0].symbol,
            2,
            (1, 2),
        )
    ] + got_contacts + got_music
    assert any("occurrence drift" in error for error in compare(drifted, allowed))

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        empty_baseline = root / "empty.toml"
        empty_baseline.write_text("version = 1\n", encoding="utf-8")
        assert load_baseline(empty_baseline) == []

        gallery = root / "apps/gallery/LocalGallery"
        gallery.mkdir(parents=True)
        (gallery / "Real.swift").write_text("enum PhotoLocality {}\n", encoding="utf-8")
        (gallery / "GalleryCore.swift").write_text(
            "enum DownloadStatus {}\n", encoding="utf-8"
        )
        contact_src = root / "core/contacts-ffi/src"
        contact_src.mkdir(parents=True)
        (contact_src / "lib.rs").write_text(
            "pub fn vcard_text() -> String { String::new() }\n", encoding="utf-8"
        )
        fixture = root / "core/thing/tests"
        fixture.mkdir(parents=True)
        (fixture / "ignored.rs").write_text(
            "fn x(vfs: &dyn Vfs) { vfs.exists(\"x\"); }\n", encoding="utf-8"
        )
        got = findings_for(root)
        keys = {finding.key for finding in got}
        assert (GALLERY_KIND, "apps/gallery/LocalGallery/Real.swift", "PhotoLocality") in keys
        assert not any("GalleryCore.swift" in finding.path for finding in got)
        assert not any("ignored.rs" in finding.path for finding in got)

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
    parser.add_argument(
        "--baseline",
        type=Path,
        default=None,
        help="baseline path (default: <root>/conformance/semantic/baseline.toml)",
    )
    parser.add_argument(
        "--print-baseline",
        action="store_true",
        help="print a TOML draft for the current production findings",
    )
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)

    if args.self_test:
        return run_self_test()

    root = args.root.resolve()
    found = findings_for(root)
    if args.print_baseline:
        print(render_baseline(found), end="")
        return 0

    baseline_path = (
        args.baseline.resolve()
        if args.baseline is not None
        else root / DEFAULT_BASELINE
    )
    try:
        allowed = load_baseline(baseline_path)
    except BaselineError as exc:
        print(f"semantic conformance: RED\n  {exc}")
        return 2

    errors = compare(found, allowed)
    if errors:
        print(f"semantic conformance: RED ({len(errors)} mismatch(es))")
        for error in errors:
            print(f"  {error}")
        return 1
    print(
        "semantic conformance: GREEN "
        f"({len(found)} known-debt path/symbol finding(s) exactly baselined)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
