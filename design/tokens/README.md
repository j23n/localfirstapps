# Design tokens

One table per app (`gallery.toml`, `contacts.toml`, `music.toml`,
`health.toml`). ADR 0004 R11: tokens are data; a colour literal in a
shell file that is not generated is a defect.

A `dark` key is a **sourced** companion (Phase 3.5). Accents come from
each app's iOS `AccentColor.colorset`. Do not invent a palette.
Gallery surfaces/ink stay light-only (those values were in
`Design.swift`; no sourced dark exists). Health has no catalog.

```
python3 scripts/gen_r14.py          # write generated sources
python3 scripts/gen_r14.py --check  # CI: regenerating produces no diff
```

Accents are the light and dark components of each app's
`AccentColor.colorset`. Gallery light surfaces come from the old
`Design.swift` literals.

R14 vocabulary lives in `docs/spec/ui/vocabulary.toml`. Per-app screen
identifiers come from `apps/<app>/ui-spec/screens.toml` (contacts is
the first).

Generated (do not edit): `core/localcore-ui/src/{kinds,tokens,screens}.rs`,
`docs/spec/ui/generated/Kinds.swift`, each app's `Generated/` Swift,
and `design/tokens/generated/*.css`.
