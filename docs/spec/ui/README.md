# UI vocabulary and tokens

ADR 0004 R4 lives in `vocabulary.toml` (closed lists). Per-app token
tables live in [`design/tokens/`](../../../design/tokens/) (light-only
until Phase 3.5). The contacts screen list is
[`apps/contacts/ui-spec/screens.toml`](../../../apps/contacts/ui-spec/screens.toml).

```
python3 scripts/gen_r14.py          # kinds, screen ids, tokens
python3 scripts/gen_r14.py --check  # CI: regenerating produces no diff
```
