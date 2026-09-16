#!/usr/bin/env python3
"""Phase 1 accent-fg contrast table (GTK design pass). No extra deps."""

from __future__ import annotations

import sys
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS))

import gen_r14 as gen  # noqa: E402

# Phase 1 expected table (docs/GTK-DESIGN-PLAN.md). Ink hex is the candidate
# documented in that table — Gallery dark lists light ink #1C1A16 (7.00).
# Authored Gallery dark ink #F2EDE5 is ~2.13 on the dark accent and is not
# selected; the generator still chooses black.
TABLE = (
    # app, scheme, accent, white, black, ink_hex, ink_ratio, chosen
    ("Contacts", "light", "#336BC7", 5.16, 4.07, None, None, "white"),
    ("Contacts", "dark", "#4D85DE", 3.67, 5.73, None, None, "black"),
    ("Music", "light", "#C0392B", 5.44, 3.86, None, None, "white"),
    ("Music", "dark", "#D14738", 4.497, 4.67, None, None, "black"),
    ("Gallery", "light", "#C48A3E", 2.98, 7.05, "#1C1A16", 5.83, "black"),
    ("Gallery", "dark", "#D4994D", 2.48, 8.46, "#1C1A16", 7.00, "black"),
)

CHOSEN_HEX = {"white": "#FFFFFF", "black": "#000000"}


def _assert_ratio(actual: float, expected: float, *, music_dark_white: bool) -> None:
    if music_dark_white:
        if abs(actual - 4.497) >= 0.001:
            raise AssertionError(f"Music dark white is {actual:.6f}, expected ~4.497")
        if actual >= gen.MIN_ACCENT_FG_CONTRAST:
            raise AssertionError(
                f"Music dark white {actual:.6f} must stay below 4.5 "
                "(do not round 4.497 up to pass)"
            )
        # Documents the trap: two-decimal rounding would falsely report 4.50.
        if round(actual, 2) != 4.5:
            raise AssertionError("expected 4.497 to round to 4.50 at 2 d.p.")
        return
    if round(actual, 2) != expected:
        raise AssertionError(f"ratio {actual:.6f} rounded to {round(actual, 2)}, expected {expected}")


def main() -> int:
    rows: list[tuple[str, ...]] = []
    for app, scheme, accent, exp_w, exp_k, ink, exp_ink, chosen in TABLE:
        white = gen.contrast_ratio("#FFFFFF", accent)
        black = gen.contrast_ratio("#000000", accent)
        ink_ratio = gen.contrast_ratio(ink, accent) if ink else None
        music_dark_white = app == "Music" and scheme == "dark"
        _assert_ratio(white, exp_w, music_dark_white=music_dark_white)
        _assert_ratio(black, exp_k, music_dark_white=False)
        if ink is not None:
            assert exp_ink is not None
            _assert_ratio(ink_ratio or 0.0, exp_ink, music_dark_white=False)

        label, hex_, ratio = gen.choose_accent_fg(accent, ink)
        if label != chosen:
            raise AssertionError(f"{app} {scheme}: chose {label}, expected {chosen}")
        if hex_ != CHOSEN_HEX[chosen]:
            raise AssertionError(f"{app} {scheme}: chose {hex_}, expected {CHOSEN_HEX[chosen]}")
        if ratio < gen.MIN_ACCENT_FG_CONTRAST:
            raise AssertionError(f"{app} {scheme}: best {ratio:.3f} is below 4.5")
        if chosen == "white" and white < gen.MIN_ACCENT_FG_CONTRAST:
            raise AssertionError(f"{app} {scheme}: selected failing white")

        rows.append(
            (
                app,
                scheme,
                accent,
                f"{white:.3f}" if music_dark_white else f"{white:.2f}",
                f"{black:.2f}",
                f"{ink_ratio:.2f} ({ink})" if ink_ratio is not None and ink else "—",
                chosen,
            )
        )

    # Live tokens: Gallery dark ink is the authored companion, still not chosen.
    tables = {
        app: gen.tomllib.loads((gen.TOKENS_DIR / f"{app}.toml").read_text())
        for app in ("contacts", "music", "gallery")
    }
    live_expect = {
        ("contacts", "light"): "#FFFFFF",
        ("contacts", "dark"): "#000000",
        ("music", "light"): "#FFFFFF",
        ("music", "dark"): "#000000",
        ("gallery", "light"): "#000000",
        ("gallery", "dark"): "#000000",
    }
    for app, table in tables.items():
        colors = table.get("colors", {})
        accent_spec = colors.get("accent") or {}
        for scheme in ("light", "dark"):
            accent = gen.color_hex(accent_spec, scheme)
            if not accent:
                raise AssertionError(f"{app} missing {scheme} accent")
            ink = gen.scheme_ink(colors, scheme)
            fg = gen.require_accent_fg(app, scheme, accent, ink)
            if fg != live_expect[(app, scheme)]:
                raise AssertionError(
                    f"{app} {scheme}: generator chose {fg}, expected {live_expect[(app, scheme)]}"
                )

    gallery_dark_ink = gen.scheme_ink(tables["gallery"].get("colors", {}), "dark")
    if gallery_dark_ink != "#F2EDE5":
        raise AssertionError(f"Gallery dark ink is {gallery_dark_ink!r}, expected #F2EDE5")
    authored_ink_on_dark_accent = gen.contrast_ratio("#F2EDE5", "#D4994D")
    if authored_ink_on_dark_accent >= 4.5:
        raise AssertionError(
            "authored Gallery dark ink unexpectedly passes on the dark accent"
        )

    contacts_css = gen.generate_tokens_css("contacts", tables["contacts"]) or ""
    music_css = gen.generate_tokens_css("music", tables["music"]) or ""
    gallery_css = gen.generate_tokens_css("gallery", tables["gallery"]) or ""
    health = gen.tomllib.loads((gen.TOKENS_DIR / "health.toml").read_text())
    health_css = gen.generate_tokens_css("health", health) or ""

    if "--accent-color:" in contacts_css or "--accent-color:" in music_css or "--accent-color:" in gallery_css:
        raise AssertionError("generated CSS must not emit --accent-color")
    if "--accent:" not in contacts_css or "--accent-bg-color:" not in contacts_css:
        raise AssertionError("contacts CSS missing --accent alias or --accent-bg-color")
    if "--window-bg-color" in contacts_css or "--window-bg-color" in music_css:
        raise AssertionError("Contacts/Music must not emit invented surface CSS")
    if "--window-bg-color: #1A1815" not in gallery_css:
        raise AssertionError("Gallery dark CSS missing authored --window-bg-color")
    if "--accent:" in health_css or "--accent-bg-color" in health_css:
        raise AssertionError("Health CSS must not invent an accent")

    print(
        "| App | Scheme | Accent | White | Black | Ink | Chosen |\n"
        "|---|---|---|---|---|---|---|"
    )
    for row in rows:
        print("| " + " | ".join(row) + " |")
    print(
        f"Gallery dark authored ink #F2EDE5 on #D4994D = "
        f"{authored_ink_on_dark_accent:.2f} (not selected)"
    )
    print("accent-fg contrast table: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
