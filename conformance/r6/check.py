#!/usr/bin/env python3
"""ADR 0003 R6 — display-record surface over gallery-ffi.

Enumerates `uniffi::Record` / `#[derive(uniffi::Record)]` and other
exported types in `apps/gallery/core/gallery-ffi/src/**/*.rs`. A Record
is green only when every field is a display-ready scalar for exactly
one ADR 0004 R4 slot kind (see docs/spec/adr/0003-r6-surface.md).

The current surface is domain records. That is the honest state until
a later rewrite. CI pins this exact red so a second ScanPhoto cannot
hide behind the known ones.

Usage (from the monorepo root):

    python3 conformance/r6/check.py                  # exit 1 while red
    python3 conformance/r6/check.py --expect-violations
    python3 conformance/r6/check.py --self-test

Exit 0 when the surface is clean, or when --expect-violations matches
exactly. Exit 1 on a real mismatch. Exit 2 on usage / IO errors.
"""

from __future__ import annotations

import argparse
import io
import re
import sys
import tempfile
from contextlib import redirect_stderr, redirect_stdout
from dataclasses import dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
DEFAULT_SRC = Path("apps/gallery/core/gallery-ffi/src")
DEFAULT_EXPECTED = HERE / "expected.txt"

PRIMITIVES = frozenset(
    {
        "String",
        "bool",
        "char",
        "f32",
        "f64",
        "i8",
        "i16",
        "i32",
        "i64",
        "i128",
        "isize",
        "u8",
        "u16",
        "u32",
        "u64",
        "u128",
        "usize",
    }
)

# Canonical + documented aliases for one ADR 0004 item kind.
# `id` may appear on any display record so the shell can key the row.
SLOT_KINDS: dict[str, dict[str, frozenset[str]]] = {
    "text-row": {
        "required": frozenset({"title"}),
        "optional": frozenset(
            {
                "id",
                "subtitle",
                "trailing",
                "trailing_value",
                "leading_symbol",
                "symbol",
                "disposition",
            }
        ),
        "need_one_of": frozenset(),
    },
    "media-item": {
        "required": frozenset(),
        "optional": frozenset(
            {
                "id",
                "thumbnail",
                "thumbnail_ref",
                "thumbnail_id",
                "label",
                "badge",
            }
        ),
        "need_one_of": frozenset({"thumbnail", "thumbnail_ref", "thumbnail_id"}),
    },
    "field-row": {
        "required": frozenset({"label", "value"}),
        "optional": frozenset({"id", "editable", "editability"}),
        "need_one_of": frozenset(),
    },
    "toggle-row": {
        "required": frozenset({"label"}),
        "optional": frozenset({"id", "on", "on_off", "state"}),
        "need_one_of": frozenset({"on", "on_off", "state"}),
    },
    "action-row": {
        "required": frozenset({"label", "role", "enabled"}),
        "optional": frozenset({"id"}),
        "need_one_of": frozenset(),
    },
    "nav-row": {
        "required": frozenset({"label", "destination"}),
        "optional": frozenset({"id", "trailing", "trailing_value"}),
        "need_one_of": frozenset(),
    },
    "progress-row": {
        "required": frozenset({"label"}),
        "optional": frozenset(
            {"id", "fraction", "determinate", "indeterminate", "cancel"}
        ),
        "need_one_of": frozenset({"fraction", "determinate", "indeterminate"}),
    },
    "status-row": {
        "required": frozenset({"message", "severity"}),
        "optional": frozenset({"id"}),
        "need_one_of": frozenset(),
    },
}

ITEM_RE = re.compile(
    r"^(?:pub(?:\([^)]*\))?\s+)?(?P<kind>struct|enum|trait|fn)\s+(?P<name>\w+)"
)
IMPL_RE = re.compile(r"^impl(?:\s*<[^>]+>)?\s+(?P<name>\w+)\s*[{]")
FIELD_RE = re.compile(r"^pub(?:\([^)]*\))?\s+(?P<name>\w+)\s*:\s*(?P<typ>.+?)\s*,?\s*$")


@dataclass(frozen=True)
class Field:
    name: str
    typ: str


@dataclass(frozen=True)
class Record:
    name: str
    path: str
    line: int
    fields: tuple[Field, ...]


@dataclass(frozen=True)
class Export:
    kind: str  # record / enum / error / object / trait / fn
    name: str
    path: str
    line: int


@dataclass(frozen=True)
class Violation:
    name: str
    path: str
    line: int
    reason: str


def _attr_kinds(attrs: list[str]) -> set[str]:
    blob = " ".join(attrs)
    kinds: set[str] = set()
    if "uniffi::Record" in blob:
        kinds.add("record")
    if "uniffi::Enum" in blob:
        kinds.add("enum")
    if "uniffi::Error" in blob:
        kinds.add("error")
    if "uniffi::Object" in blob:
        kinds.add("object")
    if "uniffi::export" in blob:
        kinds.add("export")
        if "with_foreign" in blob:
            kinds.add("foreign")
    return kinds


def _brackets_balanced(text: str) -> bool:
    depth = 0
    for ch in text:
        if ch == "[":
            depth += 1
        elif ch == "]":
            depth -= 1
    return depth == 0


def _angles_balanced(text: str) -> bool:
    depth = 0
    for ch in text:
        if ch == "<":
            depth += 1
        elif ch == ">":
            depth -= 1
    return depth == 0


def peel_option(typ: str) -> str:
    typ = typ.strip()
    if typ.startswith("Option<") and typ.endswith(">") and _angles_balanced(typ):
        return typ[len("Option<") : -1].strip()
    return typ


def is_vec(typ: str) -> bool:
    typ = typ.strip()
    return typ.startswith("Vec<") and typ.endswith(">") and _angles_balanced(typ)


def peel_vec(typ: str) -> str:
    typ = typ.strip()
    if is_vec(typ):
        return typ[len("Vec<") : -1].strip()
    return typ


def innermost_named(typ: str) -> str:
    """Strip Option/Vec wrappers; return the named type at the core."""
    current = typ.strip().rstrip(",")
    while True:
        stripped = peel_option(current)
        if stripped != current:
            current = stripped
            continue
        stripped = peel_vec(current)
        if stripped != current:
            current = stripped
            continue
        break
    return current


def slot_kind_for(field_names: set[str]) -> str | None:
    hits: list[str] = []
    for kind, spec in SLOT_KINDS.items():
        allowed = spec["required"] | spec["optional"]
        if not spec["required"] <= field_names:
            continue
        need = spec["need_one_of"]
        if need and not (field_names & need):
            continue
        if not field_names <= allowed:
            continue
        hits.append(kind)
    if len(hits) == 1:
        return hits[0]
    return None


def _parse_struct_fields(lines: list[str], start: int) -> tuple[tuple[Field, ...], int]:
    """Collect `pub name: Type` fields. `start` is the struct's line index."""
    text = "\n".join(lines[start:])
    brace = text.find("{")
    if brace < 0:
        return (), start
    depth = 0
    body: list[str] = []
    i = brace
    while i < len(text):
        ch = text[i]
        if ch == "{":
            depth += 1
            if depth == 1:
                i += 1
                continue
        elif ch == "}":
            depth -= 1
            if depth == 0:
                break
        if depth >= 1:
            body.append(ch)
        i += 1
    consumed = text[: i + 1].count("\n")
    fields: list[Field] = []
    pending: list[str] = []
    for raw in "".join(body).splitlines():
        stripped = raw.strip()
        if not stripped or stripped.startswith("//") or stripped.startswith("#["):
            continue
        pending.append(stripped)
        joined = " ".join(pending)
        if not _angles_balanced(joined):
            continue
        pending = []
        m = FIELD_RE.match(joined)
        if m:
            fields.append(Field(m.group("name"), m.group("typ").rstrip(",")))
    return tuple(fields), start + consumed


def parse_ffi_source(rel: str, text: str) -> tuple[list[Record], list[Export]]:
    """Walk one Rust file. Attributes attach across doc comments and blanks."""
    lines = text.splitlines()
    records: list[Record] = []
    exports: list[Export] = []
    pending: list[str] = []
    buf: str | None = None
    i = 0
    while i < len(lines):
        raw = lines[i]
        stripped = raw.strip()
        if buf is not None:
            buf = f"{buf} {stripped}"
            if _brackets_balanced(buf):
                pending.append(buf)
                buf = None
            i += 1
            continue
        if stripped.startswith("#["):
            if _brackets_balanced(stripped):
                pending.append(stripped)
            else:
                buf = stripped
            i += 1
            continue
        if (
            not stripped
            or stripped.startswith("//")
            or stripped.startswith("///")
            or stripped.startswith("//!")
        ):
            i += 1
            continue

        kinds = _attr_kinds(pending)
        pending = []
        item = ITEM_RE.match(stripped)
        if item:
            name = item.group("name")
            kind = item.group("kind")
            line_no = i + 1
            if kind == "struct" and "record" in kinds:
                fields, end = _parse_struct_fields(lines, i)
                records.append(Record(name, rel, line_no, fields))
                exports.append(Export("record", name, rel, line_no))
                i = end + 1
                continue
            export_kind: str | None = None
            if kind == "enum" and "enum" in kinds:
                export_kind = "enum"
            elif kind == "enum" and "error" in kinds:
                export_kind = "error"
            elif kind == "struct" and "object" in kinds:
                export_kind = "object"
            elif kind == "trait" and "export" in kinds:
                export_kind = "trait"
            elif kind == "fn" and "export" in kinds:
                export_kind = "fn"
            if export_kind:
                exports.append(Export(export_kind, name, rel, line_no))
            i += 1
            continue

        impl = IMPL_RE.match(stripped)
        if impl and "export" in kinds:
            exports.append(Export("impl", impl.group("name"), rel, i + 1))
        i += 1
    return records, exports


def iter_rust_files(src: Path) -> list[Path]:
    return sorted(p for p in src.rglob("*.rs") if p.is_file())


def load_surface(src: Path, root: Path) -> tuple[list[Record], list[Export]]:
    records: list[Record] = []
    exports: list[Export] = []
    for path in iter_rust_files(src):
        rel = str(path.relative_to(root))
        recs, exps = parse_ffi_source(rel, path.read_text(encoding="utf-8"))
        records.extend(recs)
        exports.extend(exps)
    records.sort(key=lambda r: r.name)
    exports.sort(key=lambda e: (e.kind, e.name, e.path, e.line))
    return records, exports


def classify(
    records: list[Record], exports: list[Export]
) -> tuple[list[Violation], dict[str, str]]:
    """Return (violations, name -> slot kind for clean records)."""
    enums = {e.name for e in exports if e.kind == "enum"}
    record_names = {r.name for r in records}
    clean: dict[str, str] = {}
    violations: list[Violation] = []
    for rec in records:
        nested: set[str] = set()
        lists = False
        unknown: set[str] = set()
        display_ready = True
        for field in rec.fields:
            core = innermost_named(field.typ)
            exposed = peel_option(field.typ)
            if is_vec(exposed) or is_vec(peel_option(exposed)):
                lists = True
                display_ready = False
                if core in record_names:
                    nested.add(core)
                continue
            if core in record_names:
                nested.add(core)
                display_ready = False
                continue
            if core in PRIMITIVES or core in enums:
                continue
            unknown.add(core)
            display_ready = False
        names = {f.name for f in rec.fields}
        slot = slot_kind_for(names) if display_ready else None
        if slot:
            clean[rec.name] = slot
            continue
        if nested:
            reason = "carries " + ", ".join(sorted(nested))
        elif lists:
            reason = "carries a list"
        elif unknown:
            reason = "carries " + ", ".join(sorted(unknown))
        else:
            reason = "not a display record for one ADR 0004 slot kind"
        violations.append(Violation(rec.name, rec.path, rec.line, reason))
    return violations, clean


def load_expected(path: Path) -> list[str]:
    names: list[str] = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        names.append(line)
    if len(names) != len(set(names)):
        raise ValueError(f"duplicate names in {path}")
    return names


def render(
    src: Path,
    records: list[Record],
    exports: list[Export],
    violations: list[Violation],
    clean: dict[str, str],
) -> str:
    status = "RED" if violations else "GREEN"
    by_kind: dict[str, int] = {}
    for exp in exports:
        by_kind[exp.kind] = by_kind.get(exp.kind, 0) + 1
    counts = ", ".join(
        f"{by_kind[k]} {k}{'s' if by_kind[k] != 1 else ''}"
        for k in ("record", "enum", "error", "object", "trait", "fn", "impl")
        if k in by_kind
    )
    lines = [
        f"ADR 0003 R6 — gallery-ffi display-record surface  [{status}]",
        f"source: {src}",
        f"records: {len(records)}",
        f"exported: {counts or '(none)'}",
    ]
    if violations:
        lines.append("")
        lines.append(f"violations ({len(violations)}):")
        for v in violations:
            lines.append(f"  {v.name}  ({v.path}:{v.line})  {v.reason}")
        lines.append("")
        lines.append(
            "A domain Record on the FFI is the material ADR 0001 R4 forbids. "
            "expected.txt pins this inventory so a new Record cannot hide. "
            "Gallery is not rewritten here."
        )
    else:
        lines.append("no Record fails the display-record taxonomy.")
    if clean:
        lines.append("")
        lines.append(f"R6-clean records ({len(clean)}):")
        for name in sorted(clean):
            lines.append(f"  {name}  ({clean[name]})")
    return "\n".join(lines) + "\n"


def run_self_test() -> int:
    sample = """
/// Docs mention uniffi::Record but this is not an attribute.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TextRow {
    pub title: String,
    pub subtitle: Option<String>,
}

#[derive(
    Debug,
    uniffi::Record,
)]
pub struct ScanPhoto {
    pub id: String,
    pub path: String,
    pub hierarchical_tags: Vec<ScanTag>,
    pub locality: ScanLocality,
}

#[derive(uniffi::Record)]
pub struct ScanTag {
    pub full_path: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ScanLocality {
    Local,
    Remote { downloaded: bool },
}

#[derive(uniffi::Record)]
pub struct MediaCard {
    pub thumbnail: String,
    pub label: Option<String>,
}

#[derive(uniffi::Object)]
pub struct LibraryIndex {}

#[uniffi::export]
pub fn core_version() -> String { String::new() }

#[uniffi::export(with_foreign)]
pub trait ProviderProbe: Send + Sync {
    fn probe(&self, paths: Vec<String>) -> Vec<String>;
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum ScanError {
    Failed { detail: String },
}
"""
    records, exports = parse_ffi_source("fake.rs", sample)
    names = [r.name for r in records]
    assert names == ["TextRow", "ScanPhoto", "ScanTag", "MediaCard"], names
    by_kind = {}
    for e in exports:
        by_kind.setdefault(e.kind, []).append(e.name)
    assert by_kind["record"] == names
    assert by_kind["enum"] == ["ScanLocality"]
    assert by_kind["object"] == ["LibraryIndex"]
    assert by_kind["fn"] == ["core_version"]
    assert by_kind["trait"] == ["ProviderProbe"]
    assert by_kind["error"] == ["ScanError"]

    photo = next(r for r in records if r.name == "ScanPhoto")
    assert [f.name for f in photo.fields] == [
        "id",
        "path",
        "hierarchical_tags",
        "locality",
    ]

    violations, clean = classify(records, exports)
    assert clean == {"TextRow": "text-row", "MediaCard": "media-item"}, clean
    vnames = [v.name for v in violations]
    assert vnames == ["ScanPhoto", "ScanTag"], vnames
    reasons = {v.name: v.reason for v in violations}
    assert reasons["ScanPhoto"] == "carries ScanTag", reasons
    assert reasons["ScanTag"] == "not a display record for one ADR 0004 slot kind"

    # A comment must not invent a Record, and Object is not a Record.
    quiet = parse_ffi_source(
        "c.rs",
        "// #[derive(uniffi::Record)]\n// pub struct Ghost {}\n",
    )
    assert quiet == ([], [])

    # Option wrapping is display-ready; Vec is not; nested Record is not.
    assert peel_option("Option<String>") == "String"
    assert is_vec("Vec<ScanTag>")
    assert innermost_named("Option<Vec<ScanPhoto>>") == "ScanPhoto"
    assert slot_kind_for({"title", "subtitle"}) == "text-row"
    assert slot_kind_for({"label", "value"}) == "field-row"
    assert slot_kind_for({"message", "severity"}) == "status-row"
    assert slot_kind_for({"title", "path"}) is None
    assert slot_kind_for({"label"}) is None  # toggle/progress need a state field

    expected = ["ScanPhoto", "ScanTag"]
    assert [v.name for v in violations] == expected

    # CLI: --expect-violations pins the set; a new Record cannot hide.
    rust = """
#[derive(uniffi::Record)]
pub struct ScanPhoto {
    pub id: String,
    pub path: String,
}
"""
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        src = root / "apps/gallery/core/gallery-ffi/src"
        src.mkdir(parents=True)
        (src / "lib.rs").write_text(rust)
        expected_file = root / "expected.txt"
        expected_file.write_text("ScanPhoto\n")
        sink = io.StringIO()
        with redirect_stdout(sink), redirect_stderr(sink):
            matched = main(
                [
                    "--root",
                    str(root),
                    "--expected-file",
                    str(expected_file),
                    "--expect-violations",
                ]
            )
            mismatch = main(
                [
                    "--root",
                    str(root),
                    "--expect-violations",
                    "ScanPhoto",
                    "GhostRecord",
                ]
            )
            default_red = main(["--root", str(root)])
        assert matched == 0, sink.getvalue()
        assert mismatch == 1, sink.getvalue()
        assert default_red == 1, sink.getvalue()

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
        "--src",
        type=Path,
        default=None,
        help="gallery-ffi src directory (default: <root>/apps/gallery/core/gallery-ffi/src)",
    )
    parser.add_argument(
        "--expect-violations",
        metavar="TYPE",
        nargs="*",
        default=None,
        help="succeed only when the violating Record set is exactly these "
        "names, or exactly expected.txt when none are given",
    )
    parser.add_argument(
        "--expected-file",
        type=Path,
        default=None,
        help="override conformance/r6/expected.txt",
    )
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)

    if args.self_test:
        return run_self_test()

    root = args.root.resolve()
    src = (args.src if args.src is not None else root / DEFAULT_SRC).resolve()
    if not src.is_dir():
        print(f"error: gallery-ffi src not found: {src}", file=sys.stderr)
        return 2

    records, exports = load_surface(src, root)
    if not records and not exports:
        print(f"error: no uniffi types in {src}", file=sys.stderr)
        return 2

    violations, clean = classify(records, exports)
    names = [v.name for v in violations]
    print(render(src, records, exports, violations, clean), end="")

    if args.expect_violations is not None:
        if args.expect_violations:
            expected = list(args.expect_violations)
        else:
            expected_path = (
                args.expected_file.resolve()
                if args.expected_file is not None
                else DEFAULT_EXPECTED
            )
            if not expected_path.is_file():
                print(f"error: expected file not found: {expected_path}", file=sys.stderr)
                return 2
            try:
                expected = load_expected(expected_path)
            except ValueError as exc:
                print(f"error: {exc}", file=sys.stderr)
                return 2
        if names == expected:
            print(f"expected red: {', '.join(expected)}  (matched)")
            return 0
        print(
            "error: findings "
            f"{names or '[]'} != expected {expected or '[]'}",
            file=sys.stderr,
        )
        return 1

    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main())
