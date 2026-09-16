# Design tokens

One table per app (`gallery.toml`, `contacts.toml`, `music.toml`,
`health.toml`). ADR 0004 R11: tokens are data; a colour literal in a
shell file that is not generated is a defect.

Accents (`light` / `dark`) are **sourced** from each app's iOS
`AccentColor.colorset`. Do not invent an accent. Health has no catalog
and therefore no accent.

A `dark` key is otherwise a **sourced** companion (Phase 3.5). Do not
invent a palette. **Exception (D4):** Gallery dark *surfaces* (`bg`,
`bg_card`, `bg_grouped`, `ink`, `ink2`, `ink3`, `destructive`) are
**authored**, not sourced — there is no iOS dark catalog for those
roles. Contacts and Music still have no invented surfaces; shells use
platform semantic colours (ADR 0004 R12).

```
python3 scripts/gen_r14.py          # write generated sources
python3 scripts/gen_r14.py --check  # CI: regenerating produces no diff
```

Generated GTK CSS uses libadwaita 1.9 variables (GTK ≥ 4.20):

- `--accent-bg-color` from the app's accent for each scheme.
- `--accent-fg-color`: per scheme, the highest-contrast choice among
  `#FFFFFF`, `#000000`, and that scheme's `ink` when the app has ink
  (only Gallery). WCAG 2.x relative luminance (sRGB). The generator
  **fails** if the best candidate is below 4.5:1 — it does not round
  4.497 up to pass, and it never adjusts a sourced accent.
- `--accent` is a leftover alias of `--accent-bg-color` (same hex). The
  kit maps `accent_bg_color` from `--accent-bg-color`; do not treat
  `--accent` as the API.
- Do **not** emit `--accent-color`; libadwaita derives a readable
  text accent.
- Gallery also maps authored surfaces onto libadwaita roles
  (`--window-bg-color`, `--view-bg-color`, `--headerbar-bg-color`,
  `--card-bg-color`, `--dialog-bg-color`, `--popover-bg-color`,
  `--window-fg-color` / `--view-fg-color` / `--card-fg-color`,
  `--destructive-bg-color`, and `--border-color` from `separator_ink`
  × `separator_opacity`). Contacts and Music emit accent only and
  keep system surfaces.

R14 vocabulary lives in `docs/spec/ui/vocabulary.toml`. Per-app screen
identifiers come from `apps/<app>/ui-spec/screens.toml` (contacts is
the first).

Generated (do not edit): `core/localcore-ui/src/{kinds,tokens,screens}.rs`,
`docs/spec/ui/generated/Kinds.swift`, each app's `Generated/` Swift,
and `design/tokens/generated/*.css`.
