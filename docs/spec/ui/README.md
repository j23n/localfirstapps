# UI vocabulary and tokens

ADR 0004 R4 lives in `vocabulary.toml` (closed lists). Per-app token
tables live in [`design/tokens/`](../../../design/tokens/).

```
python3 scripts/gen_r14.py          # Rust, Swift, CSS
python3 scripts/gen_r14.py --check  # CI: regenerating produces no diff
```

Screen-identifier codegen (R14 row 2) waits for per-app UI specs.
Those land with the dummy spec in Phase 3.3. Kinds and tokens are 3.2.
