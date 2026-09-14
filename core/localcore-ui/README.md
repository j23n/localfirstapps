# localcore-ui

ADR 0004 R4 closed vocabulary, R14 screen identifiers, and R11 token
tables, as Rust data. No UI toolkit (ADR 0001 R9).

```
python3 scripts/gen_r14.py          # rewrite kinds.rs / tokens.rs / screens.rs
python3 scripts/gen_r14.py --check  # CI: no drift
```

Inputs: `docs/spec/ui/vocabulary.toml`, `design/tokens/*.toml`,
`apps/*/ui-spec/screens.toml`. A `dark` key is a sourced companion.
