# Design tokens

One table per app (`gallery.toml`, `contacts.toml`, `music.toml`,
`health.toml`). ADR 0004 R11: tokens are data; a colour literal in a
shell file that is not generated is a defect.

```
python3 scripts/gen_r14.py          # write generated sources
python3 scripts/gen_r14.py --check  # CI: regenerating produces no diff
```

Light values for gallery surfaces come from the existing `Design.swift`
literals. Accent light/dark for gallery, contacts, and music come from
each app's `AccentColor.colorset`. **Dark surfaces and ink are not
authored** — do not invent them. Add a `dark = "#RRGGBB"` key when you
have a value.

R14 vocabulary lives in `docs/spec/ui/vocabulary.toml` (closed lists
from ADR 0004 R4). The same script emits those enums.

Generated (do not edit): `core/localcore-ui/src/{kinds,tokens}.rs`,
`docs/spec/ui/generated/Kinds.swift`, each app's
`Generated/Tokens.swift`, and `design/tokens/generated/*.css`.
