# Design tokens

One table per app (`gallery.toml`, `contacts.toml`, `music.toml`,
`health.toml`). ADR 0004 R11: tokens are data; a colour literal in a
shell file that is not generated is a defect.

**Light-only until Phase 3.5.** `scripts/gen_r14.py` refuses a `dark`
key. Dark companions are tracked on the implementation plan; do not
invent a palette here.

```
python3 scripts/gen_r14.py          # write generated sources
python3 scripts/gen_r14.py --check  # CI: regenerating produces no diff
```

Gallery light surfaces come from the old `Design.swift` literals.
Accents are the light component of each app's `AccentColor.colorset`.

R14 vocabulary lives in `docs/spec/ui/vocabulary.toml`. Per-app screen
identifiers come from `apps/<app>/ui-spec/screens.toml` (contacts is
the first).

Generated (do not edit): `core/localcore-ui/src/{kinds,tokens,screens}.rs`,
`docs/spec/ui/generated/Kinds.swift`, each app's `Generated/` Swift,
and `design/tokens/generated/*.css`.
