#!/usr/bin/env python3
"""ADR 0004 R14 — generate slot-kind enums, screen ids, and design tokens.

Inputs:
  docs/spec/ui/vocabulary.toml
  design/tokens/{gallery,contacts,music,health}.toml
  apps/*/ui-spec/screens.toml  (per-app UI specs; contacts is first)

Outputs (reproducible; --check fails on drift):
  core/localcore-ui/src/{kinds,tokens,screens}.rs
  docs/spec/ui/generated/Kinds.swift
  shells/shell-kit-swift/Sources/ShellKitSwift/Generated/Kinds.swift
  apps/{gallery,contacts,music}/.../Generated/{Tokens,Screens}.swift
  design/tokens/generated/{app}.css

A `dark` key is a sourced companion (Phase 3.5). Do not invent one.
Exception: a `dark` key on Gallery *surfaces* is authored (D4);
accents remain sourced from each app's AccentColor.colorset.
"""

from __future__ import annotations

import argparse
import sys
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
VOCAB = REPO / "docs/spec/ui/vocabulary.toml"
TOKENS_DIR = REPO / "design/tokens"
APPS = ("gallery", "contacts", "music", "health")

SWIFT_KINDS = REPO / "docs/spec/ui/generated/Kinds.swift"
SHELL_KIT_SWIFT_KINDS = (
    REPO
    / "shells/shell-kit-swift/Sources/ShellKitSwift/Generated/Kinds.swift"
)
RUST_KINDS = REPO / "core/localcore-ui/src/kinds.rs"
RUST_TOKENS = REPO / "core/localcore-ui/src/tokens.rs"
RUST_SCREENS = REPO / "core/localcore-ui/src/screens.rs"

SCREEN_SWIFT = {
    "contacts": REPO / "apps/contacts/LocalContacts/Generated/Screens.swift",
    "music": REPO / "apps/music/LocalMusic/Generated/Screens.swift",
}

TOKEN_SWIFT = {
    "gallery": REPO / "apps/gallery/LocalGallery/Generated/Tokens.swift",
    "contacts": REPO / "apps/contacts/LocalContacts/Generated/Tokens.swift",
    "music": REPO / "apps/music/LocalMusic/Generated/Tokens.swift",
}


def pascal(name: str) -> str:
    return "".join(p.title() for p in name.replace("_", "-").split("-"))


def camel(name: str) -> str:
    ident = pascal(name)
    return ident[0].lower() + ident[1:]


def snake(name: str) -> str:
    return name.replace("-", "_")


# WCAG AA normal text. Compare the raw ratio; never round 4.497 up to pass.
MIN_ACCENT_FG_CONTRAST = 4.5
ACCENT_FG_WHITE = "#FFFFFF"
ACCENT_FG_BLACK = "#000000"

# libadwaita 1.9 surface roles, only for apps that have the source tokens.
SURFACE_CSS_ROLES: tuple[tuple[str, str], ...] = (
    ("window-bg-color", "bg"),
    ("view-bg-color", "bg"),
    ("headerbar-bg-color", "bg"),
    ("card-bg-color", "bg_card"),
    ("dialog-bg-color", "bg_card"),
    ("popover-bg-color", "bg_card"),
    ("window-fg-color", "ink"),
    ("view-fg-color", "ink"),
    ("card-fg-color", "ink"),
    ("destructive-bg-color", "destructive"),
)


def hex_to_rgb(value: str) -> tuple[int, int, int]:
    h = value.removeprefix("#")
    if len(h) != 6:
        raise ValueError(f"expected #RRGGBB, got {value!r}")
    return int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16)


def color_hex(spec: dict, key: str) -> str | None:
    value = spec.get(key)
    if not value:
        return None
    hex_to_rgb(value)
    return value


def relative_luminance(value: str) -> float:
    """WCAG 2.x relative luminance for an sRGB #RRGGBB colour."""

    def channel(component: int) -> float:
        srgb = component / 255.0
        # WCAG 2.2 uses the sRGB breakpoint 0.04045 (2.0/2.1 listed 0.03928).
        if srgb <= 0.04045:
            return srgb / 12.92
        return ((srgb + 0.055) / 1.055) ** 2.4

    red, green, blue = hex_to_rgb(value)
    return (
        0.2126 * channel(red)
        + 0.7152 * channel(green)
        + 0.0722 * channel(blue)
    )


def contrast_ratio(foreground: str, background: str) -> float:
    lighter, darker = sorted(
        (relative_luminance(foreground), relative_luminance(background)),
        reverse=True,
    )
    return (lighter + 0.05) / (darker + 0.05)


def accent_fg_candidates(ink: str | None) -> list[tuple[str, str]]:
    """White, black, and the scheme's ink when the app has one (Gallery)."""
    out = [("white", ACCENT_FG_WHITE), ("black", ACCENT_FG_BLACK)]
    if ink:
        out.append(("ink", ink))
    return out


def choose_accent_fg(
    accent: str, ink: str | None = None
) -> tuple[str, str, float]:
    """Highest-contrast accent text. Does not adjust the sourced accent.

    Returns ``(label, hex, ratio)``. The caller must fail the generator
    when ``ratio < MIN_ACCENT_FG_CONTRAST`` (no rounding up).
    """
    best_label = ACCENT_FG_WHITE
    best_hex = ACCENT_FG_WHITE
    best_ratio = -1.0
    for label, candidate in accent_fg_candidates(ink):
        ratio = contrast_ratio(candidate, accent)
        if ratio > best_ratio:
            best_label, best_hex, best_ratio = label, candidate, ratio
    return best_label, best_hex, best_ratio


def require_accent_fg(
    app: str, scheme: str, accent: str, ink: str | None
) -> str:
    """Return the chosen ``--accent-fg-color`` hex, or exit below 4.5:1."""
    label, chosen, ratio = choose_accent_fg(accent, ink)
    if ratio < MIN_ACCENT_FG_CONTRAST:
        raise SystemExit(
            f"error: {app} {scheme} accent {accent}: best --accent-fg-color "
            f"{chosen} ({label}) is {ratio:.3f}:1, below 4.5:1. "
            "Sourced accents are never adjusted."
        )
    return chosen


def scheme_ink(colors: dict, scheme: str) -> str | None:
    spec = colors.get("ink") or {}
    return color_hex(spec, scheme)


def border_color_css(colors: dict, metrics: dict, scheme: str) -> str | None:
    spec = colors.get("separator_ink") or {}
    ink = color_hex(spec, scheme)
    opacity = metrics.get("separator_opacity")
    if not ink or opacity is None:
        return None
    red, green, blue = hex_to_rgb(ink)
    return f"rgba({red}, {green}, {blue}, {opacity})"


def swift_rgb(value: str) -> str:
    r, g, b = hex_to_rgb(value)
    return (
        f"Color(red: {r / 255:.3f}, green: {g / 255:.3f}, blue: {b / 255:.3f})"
    )


def rust_enum(name: str, variants: list[str]) -> str:
    arms = "\n".join(f"    {pascal(v)}," for v in variants)
    matches = "\n".join(
        f'            Self::{pascal(v)} => "{v}",' for v in variants
    )
    all_inner = ", ".join(f"Self::{pascal(v)}" for v in variants)
    all_one = f"    pub const ALL: &'static [Self] = &[{all_inner}];"
    if len(all_one) <= 100:
        all_decl = all_one
    else:
        all_decl = (
            "    pub const ALL: &'static [Self] = &[\n        "
            + ",\n        ".join(f"Self::{pascal(v)}" for v in variants)
            + ",\n    ];"
        )
    return f"""#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum {name} {{
{arms}
}}

impl {name} {{
{all_decl}

    pub fn as_str(self) -> &'static str {{
        match self {{
{matches}
        }}
    }}
}}
"""


def swift_enum(name: str, variants: list[str]) -> str:
    cases = "\n".join(f'    case {camel(v)} = "{v}"' for v in variants)
    return f"""public enum {name}: String, Sendable, CaseIterable, Hashable {{
{cases}
}}
"""


def generate_kinds(vocab: dict) -> tuple[str, str]:
    tables = [
        ("ScreenKind", vocab["screen_kinds"]),
        ("ItemKind", vocab["item_kinds"]),
        ("Affordance", vocab["affordances"]),
        ("NavIntent", vocab["nav_intents"]),
        ("ActionRole", vocab["action_roles"]),
        ("StatusSeverity", vocab["status_severities"]),
    ]
    rust = (
        "//! Generated by `scripts/gen_r14.py`. Do not edit.\n"
        "//! ADR 0004 R4 closed vocabulary.\n\n"
    )
    rust += "\n".join(rust_enum(n, vs) for n, vs in tables)
    swift = (
        "// Generated by scripts/gen_r14.py. Do not edit.\n"
        "// ADR 0004 R4 closed vocabulary.\n\n"
    )
    swift += "\n".join(swift_enum(n, vs) for n, vs in tables)
    if not swift.endswith("\n"):
        swift += "\n"
    return rust, swift


def generate_tokens_rs(tables: dict[str, dict]) -> str:
    lines = [
        "//! Generated by `scripts/gen_r14.py`. Do not edit.",
        "//! ADR 0004 R11 token tables. Light hex plus dark companions.",
        "//! Accents are sourced; Gallery surface dark keys are authored (D4).",
        "",
    ]
    for app, table in tables.items():
        colors = table.get("colors", {})
        lines.append(f"pub mod {app} {{")
        for role, spec in colors.items():
            const = snake(role).upper()
            light = color_hex(spec, "light")
            dark = color_hex(spec, "dark")
            if light:
                lines.append(f'    pub const {const}: &str = "{light}";')
            if dark:
                lines.append(f'    pub const {const}_DARK: &str = "{dark}";')
        for name, value in table.get("metrics", {}).items():
            const = snake(name).upper()
            if isinstance(value, float):
                lines.append(f"    pub const {const}: f64 = {value};")
            else:
                lines.append(f"    pub const {const}: f64 = {value}.0;")
        lines.append("}")
        lines.append("")
    return "\n".join(lines)


def generate_tokens_swift(app: str, table: dict) -> str:
    colors = table.get("colors", {})
    metrics = table.get("metrics", {})
    lines = [
        f"// Generated by scripts/gen_r14.py from design/tokens/{app}.toml. "
        "Do not edit.",
        "import SwiftUI",
        "",
        f"enum {pascal(app)}Tokens {{",
    ]
    for role, spec in colors.items():
        ident = camel(role)
        light = color_hex(spec, "light")
        dark = color_hex(spec, "dark")
        if light:
            lines.append(f"    static let {ident} = {swift_rgb(light)}")
        if dark:
            lines.append(f"    static let {ident}Dark = {swift_rgb(dark)}")
    for name, value in metrics.items():
        ident = camel(name)
        if isinstance(value, float):
            lines.append(f"    static let {ident}: Double = {value}")
        else:
            lines.append(f"    static let {ident}: CGFloat = {value}")
    lines.append("}")
    lines.append("")
    return "\n".join(lines)


def _metric_css(name: str, value: object) -> str:
    ident = snake(name).replace("_", "-")
    if isinstance(value, float):
        return f"--{ident}: {value};"
    return f"--{ident}: {value}px;"


def _accent_css_vars(accent: str, fg: str) -> list[str]:
    return [
        f"--accent-bg-color: {accent};",
        f"--accent-fg-color: {fg};",
        # Leftover alias of --accent-bg-color (same hex). The kit maps
        # accent_bg_color from --accent-bg-color; do not treat --accent
        # as the API. Do not emit --accent-color.
        f"--accent: {accent};",
    ]


def _surface_css_vars(colors: dict, metrics: dict, scheme: str) -> list[str]:
    out: list[str] = []
    for css_name, role in SURFACE_CSS_ROLES:
        spec = colors.get(role) or {}
        value = color_hex(spec, scheme)
        if value:
            out.append(f"--{css_name}: {value};")
    border = border_color_css(colors, metrics, scheme)
    if border:
        out.append(f"--border-color: {border};")
    return out


def generate_tokens_css(app: str, table: dict) -> str | None:
    colors = table.get("colors", {})
    metrics = table.get("metrics", {})
    if not colors and not metrics:
        return None

    accent_spec = colors.get("accent") or {}
    light_accent = color_hex(accent_spec, "light")
    dark_accent = color_hex(accent_spec, "dark")
    light_ink = scheme_ink(colors, "light")
    dark_ink = scheme_ink(colors, "dark")

    lines = [
        f"/* Generated by scripts/gen_r14.py from design/tokens/{app}.toml. "
        "Do not edit. */",
    ]
    if light_accent or dark_accent:
        lines.append(
            "/* libadwaita 1.9 (GTK ≥ 4.20). --accent is a leftover alias of "
            "--accent-bg-color (the accent hex). The kit maps accent_bg_color "
            "from --accent-bg-color; do not treat --accent as the API. Do not "
            "emit --accent-color; libadwaita derives a readable text accent. */"
        )

    root: list[str] = []
    dark: list[str] = []
    if light_accent:
        light_fg = require_accent_fg(app, "light", light_accent, light_ink)
        root.extend(_accent_css_vars(light_accent, light_fg))
    if dark_accent:
        dark_fg = require_accent_fg(app, "dark", dark_accent, dark_ink)
        dark.extend(_accent_css_vars(dark_accent, dark_fg))

    root.extend(_surface_css_vars(colors, metrics, "light"))
    dark.extend(_surface_css_vars(colors, metrics, "dark"))

    for name, value in metrics.items():
        root.append(_metric_css(name, value))

    lines.append(":root {")
    lines.extend(f"  {decl}" for decl in root)
    lines.append("}")
    if dark:
        lines.append("@media (prefers-color-scheme: dark) {")
        lines.append("  :root {")
        lines.extend(f"    {decl}" for decl in dark)
        lines.append("  }")
        lines.append("}")
    lines.append("")
    return "\n".join(lines)


def load_ui_specs(vocab: dict) -> dict[str, list[dict]]:
    """Return {app: [screen, ...]} for every screens.toml that exists."""
    found: dict[str, list[dict]] = {}
    for path in sorted((REPO / "apps").glob("*/ui-spec/screens.toml")):
        spec = tomllib.loads(path.read_text())
        app = spec.get("app") or path.parents[1].name
        screens = spec.get("screens", [])
        if not screens:
            raise SystemExit(f"error: {path} has no [[screens]]")
        seen: set[str] = set()
        for screen in screens:
            sid = screen.get("id")
            kind = screen.get("kind")
            if not sid or not kind:
                raise SystemExit(f"error: {path} screen missing id or kind")
            if sid in seen:
                raise SystemExit(f"error: duplicate screen id {sid} in {path}")
            seen.add(sid)
            if kind not in vocab["screen_kinds"]:
                raise SystemExit(f"error: {sid} has unknown kind {kind!r}")
            for section in screen.get("sections", []):
                item = section.get("item")
                if item and item not in vocab["item_kinds"]:
                    raise SystemExit(
                        f"error: {sid} section uses unknown item {item!r}"
                    )
            for aff in screen.get("affordances", []):
                if aff not in vocab["affordances"]:
                    raise SystemExit(
                        f"error: {sid} uses unknown affordance {aff!r}"
                    )
            item_nav = screen.get("item_nav") or {}
            intent = item_nav.get("intent")
            if intent and intent not in vocab["nav_intents"]:
                raise SystemExit(
                    f"error: {sid} item_nav uses unknown intent {intent!r}"
                )
        found[app] = screens
    return found


def generate_screens_rs(specs: dict[str, list[dict]]) -> str:
    lines = [
        "//! Generated by `scripts/gen_r14.py`. Do not edit.",
        "//! Per-app screen identifiers (ADR 0004 R14).",
        "",
        "use crate::ScreenKind;",
        "",
    ]
    for app, screens in specs.items():
        enum_name = f"{pascal(app)}Screen"
        lines.append(f"#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]")
        lines.append(f"pub enum {enum_name} {{")
        for screen in screens:
            lines.append(f"    {pascal(screen['id'])},")
        lines.append("}")
        lines.append("")
        all_inner = ", ".join(f"Self::{pascal(s['id'])}" for s in screens)
        all_one = f"    pub const ALL: &'static [Self] = &[{all_inner}];"
        if len(all_one) <= 100:
            all_decl = all_one
        else:
            all_decl = (
                "    pub const ALL: &'static [Self] = &[\n        "
                + ",\n        ".join(f"Self::{pascal(s['id'])}" for s in screens)
                + ",\n    ];"
            )
        as_str = "\n".join(
            f'            Self::{pascal(s["id"])} => "{s["id"]}",' for s in screens
        )
        kind_arms = "\n".join(
            f"            Self::{pascal(s['id'])} => ScreenKind::{pascal(s['kind'])},"
            for s in screens
        )
        lines.append(f"impl {enum_name} {{")
        lines.append(all_decl)
        lines.append("")
        lines.append("    pub fn as_str(self) -> &'static str {")
        lines.append("        match self {")
        lines.append(as_str)
        lines.append("        }")
        lines.append("    }")
        lines.append("")
        lines.append("    pub fn kind(self) -> ScreenKind {")
        lines.append("        match self {")
        lines.append(kind_arms)
        lines.append("        }")
        lines.append("    }")
        lines.append("}")
        lines.append("")
    return "\n".join(lines)


def generate_screens_swift(app: str, screens: list[dict]) -> str:
    enum_name = f"{pascal(app)}Screen"
    cases = "\n".join(f'    case {camel(s["id"])} = "{s["id"]}"' for s in screens)
    kind_arms = "\n".join(
        f"        case .{camel(s['id'])}: return .{camel(s['kind'])}"
        for s in screens
    )
    return (
        f"// Generated by scripts/gen_r14.py from apps/{app}/ui-spec/"
        "screens.toml. Do not edit.\n\n"
        f"public enum {enum_name}: String, Sendable, CaseIterable {{\n"
        f"{cases}\n\n"
        "    public var kind: ScreenKind {\n"
        "        switch self {\n"
        f"{kind_arms}\n"
        "        }\n"
        "    }\n"
        "}\n"
    )


def write(path: Path, body: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(body)


def generate_all() -> dict[Path, str]:
    vocab = tomllib.loads(VOCAB.read_text())
    rust_kinds, swift_kinds = generate_kinds(vocab)
    tables = {
        app: tomllib.loads((TOKENS_DIR / f"{app}.toml").read_text())
        for app in APPS
    }
    specs = load_ui_specs(vocab)
    out: dict[Path, str] = {
        RUST_KINDS: rust_kinds,
        RUST_TOKENS: generate_tokens_rs(tables),
        RUST_SCREENS: generate_screens_rs(specs),
        SWIFT_KINDS: swift_kinds,
        SHELL_KIT_SWIFT_KINDS: swift_kinds,
    }
    for app, table in tables.items():
        if app in TOKEN_SWIFT:
            out[TOKEN_SWIFT[app]] = generate_tokens_swift(app, table)
        css = generate_tokens_css(app, table)
        if css is not None:
            out[TOKENS_DIR / "generated" / f"{app}.css"] = css
    for app, screens in specs.items():
        if app in SCREEN_SWIFT:
            out[SCREEN_SWIFT[app]] = generate_screens_swift(app, screens)
    return out


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument(
        "--check",
        action="store_true",
        help="succeed only when generated files already match",
    )
    args = parser.parse_args(argv)
    planned = generate_all()
    if args.check:
        drifted = []
        for path, body in planned.items():
            if not path.is_file() or path.read_text() != body:
                drifted.append(str(path.relative_to(REPO)))
        if drifted:
            print("error: R14 generated sources drifted:", file=sys.stderr)
            for p in drifted:
                print(f"  {p}", file=sys.stderr)
            print("run: python3 scripts/gen_r14.py", file=sys.stderr)
            return 1
        print("R14 generated sources match")
        return 0
    for path, body in planned.items():
        write(path, body)
        print(f"wrote {path.relative_to(REPO)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
