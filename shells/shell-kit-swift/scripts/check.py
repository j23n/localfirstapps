#!/usr/bin/env python3
"""Static Linux checks for shell-kit-swift's ADR boundaries."""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path

PACKAGE = Path(__file__).resolve().parents[1]
REPO = PACKAGE.parents[1]
SOURCES = PACKAGE / "Sources" / "ShellKitSwift"
GENERATED = SOURCES / "Generated" / "Kinds.swift"
CANONICAL_KINDS = REPO / "docs/spec/ui/generated/Kinds.swift"
VOCABULARY = REPO / "docs/spec/ui/vocabulary.toml"
COVERAGE = SOURCES / "KindCoverage.swift"

VOCABULARY_TABLES = {
    "screen_kinds": "ScreenKind",
    "item_kinds": "ItemKind",
    "affordances": "Affordance",
    "nav_intents": "NavIntent",
    "action_roles": "ActionRole",
    "status_severities": "StatusSeverity",
}


def camel(raw: str) -> str:
    parts = raw.replace("_", "-").split("-")
    pascal = "".join(part.title() for part in parts)
    return pascal[0].lower() + pascal[1:]


def forbidden_source_errors(sources: dict[str, str]) -> list[str]:
    errors: list[str] = []
    for path, body in sources.items():
        for imported in re.findall(r"(?m)^\s*import\s+([A-Za-z0-9_]+)", body):
            if imported != "SwiftUI":
                errors.append(f"{path}: domain/non-UI import {imported}")
        if re.search(r"\bColor\s*\(\s*red\s*:", body):
            errors.append(f"{path}: hard-coded Color(red:) is forbidden")
    return errors


def vocabulary_errors(
    vocabulary: dict[str, list[str]],
    generated: str,
    coverage: str,
) -> list[str]:
    errors: list[str] = []
    for table, enum_name in VOCABULARY_TABLES.items():
        for raw in vocabulary[table]:
            identifier = camel(raw)
            declaration = (
                rf'(?m)^\s*case\s+{re.escape(identifier)}\s*=\s*'
                rf'"{re.escape(raw)}"\s*$'
            )
            if len(re.findall(declaration, generated)) != 1:
                errors.append(
                    f"Generated/Kinds.swift: {enum_name}.{identifier} "
                    f"does not exactly represent {raw!r}"
                )
            arm = rf"(?m)^\s*case\s+\.{re.escape(identifier)}\s*:\s*$"
            if len(re.findall(arm, coverage)) != 1:
                errors.append(
                    f"KindCoverage.swift: {enum_name}.{identifier} "
                    "must have exactly one exhaustive switch arm"
                )
    return errors


def check() -> list[str]:
    errors: list[str] = []
    manifest = (PACKAGE / "Package.swift").read_text(encoding="utf-8")
    if ".package(" in manifest:
        errors.append(
            "Package.swift: shell-kit-swift must not declare package "
            "dependencies"
        )

    source_bodies = {
        str(path.relative_to(PACKAGE)): path.read_text(encoding="utf-8")
        for path in sorted(SOURCES.rglob("*.swift"))
    }
    errors.extend(forbidden_source_errors(source_bodies))

    generated = GENERATED.read_text(encoding="utf-8")
    canonical = CANONICAL_KINDS.read_text(encoding="utf-8")
    if generated != canonical:
        errors.append(
            "Generated/Kinds.swift differs from the canonical generated "
            "Swift vocabulary; run python3 scripts/gen_r14.py"
        )

    vocabulary = tomllib.loads(VOCABULARY.read_text(encoding="utf-8"))
    coverage = COVERAGE.read_text(encoding="utf-8")
    errors.extend(vocabulary_errors(vocabulary, generated, coverage))

    consumers = {
        "Contacts": (
            REPO / "apps/contacts/LocalContacts/Views/SettingsView.swift",
            "ContactsTokens.cardRadius",
        ),
        "Music": (
            REPO / "apps/music/LocalMusic/Views/SettingsView.swift",
            "MusicTokens.cardRadius",
        ),
    }
    for app, (path, token_use) in consumers.items():
        body = path.read_text(encoding="utf-8")
        for required in (
            "import ShellKitSwift",
            "ShellSettings",
            token_use,
        ):
            if required not in body:
                errors.append(
                    f"{path.relative_to(REPO)}: {app} consumer missing "
                    f"{required}"
                )

    search_consumers = (
        REPO / "apps/contacts/LocalContacts/Views/ContactListView.swift",
        REPO / "apps/music/LocalMusic/Views/LibraryView.swift",
    )
    for path in search_consumers:
        body = path.read_text(encoding="utf-8")
        if "import ShellKitSwift" not in body or ".shellSearch(" not in body:
            errors.append(
                f"{path.relative_to(REPO)}: shared search binding missing"
            )

    contacts_settings = consumers["Contacts"][0].read_text(encoding="utf-8")
    if ".shellConfirmation(" not in contacts_settings:
        errors.append(
            "apps/contacts/LocalContacts/Views/SettingsView.swift: "
            "shared confirmation binding missing"
        )

    return errors


def self_test() -> None:
    bad = forbidden_source_errors(
        {
            "Bad.swift": (
                "import Contacts\n"
                "import SwiftUI\n"
                "let accent = Color(red: 1, green: 0, blue: 0)\n"
            )
        }
    )
    assert len(bad) == 2, bad

    vocabulary = {
        "screen_kinds": ["list"],
        "item_kinds": [],
        "affordances": [],
        "nav_intents": [],
        "action_roles": [],
        "status_severities": [],
    }
    generated = 'public enum ScreenKind {\n    case list = "list"\n}\n'
    assert vocabulary_errors(vocabulary, generated, "") == [
        "KindCoverage.swift: ScreenKind.list must have exactly one "
        "exhaustive switch arm"
    ]
    assert vocabulary_errors(
        vocabulary, generated, "switch kind {\ncase .list:\n.shared\n}\n"
    ) == []


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        print("shell-kit-swift static checker self-test passed")
        return 0

    errors = check()
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1
    print("shell-kit-swift static checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
