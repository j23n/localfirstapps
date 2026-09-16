# UI vocabulary and tokens

ADR 0004 R4 lives in `vocabulary.toml` (closed lists). Per-app token
tables live in [`design/tokens/`](../../../design/tokens/) (sourced
dark accents as of 3.5). Semantic screen inventories live under
`apps/<app>/ui-spec/`, including
[`contacts`](../../../apps/contacts/ui-spec/screens.toml),
[`music`](../../../apps/music/ui-spec/screens.toml), and the curated static
[`health`](../../../apps/health/ui-spec/screens.toml) reference. A missing
*kind* fails the kit build; an unbound *screen* is a gap (Milestone C).

```
python3 scripts/gen_r14.py          # kinds, screen ids, tokens
python3 scripts/gen_r14.py --check  # CI: regenerating produces no diff
```
