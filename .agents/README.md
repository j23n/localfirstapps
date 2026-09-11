# Agent instructions

`CONVENTIONS.md` is retired (Phase 0.2). Cross-app rules live in the
family spec. ADR 0007 is the sole home for convention.

| Subject | Document |
|---|---|
| Vocabulary, settings, README, bundle ids, logging, testing | `docs/spec/adr/0007-product-conventions.md` |
| Layers and workspaces | `docs/spec/adr/0001-layering.md` |
| Identity, atomic I/O, scanning | `docs/spec/adr/0002-localcore.md` |
| App cores and the shell boundary | `docs/spec/adr/0003-app-core.md` |
| Slots, shells, design tokens | `docs/spec/adr/0004-ui-spec-and-shells.md` |
| Files, tiers, conflicts | `docs/spec/adr/0005-files-sync-and-state.md` |
| Capabilities and packs | `docs/spec/adr/0006-derived-data.md` |
| Health ingestion | `docs/spec/adr/0008-health-ingestion.md` |

Work-item routing (not path routing): [`ROUTING.md`](ROUTING.md).

Do not add a second vocabulary table. Do not put domain state in a shell.
