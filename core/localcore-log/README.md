# localcore-log

Append-only NDJSON event log, ported from health `internal/log` + `internal/event`.

Health Go remains the writer until Phase 6. The copy under
`tests/fixtures/m0/log` is not an independent source of truth.
`tests/health_golden.rs` is the port contract: every file under
`apps/health/testdata/m0/log` must stay byte-identical to that copy,
and `read_all` must return the same `id` / `ts` / `dev` / `type` / `body`
from both trees.

I/O goes through `localcore-vfs` (`append_on` / `read_all_on`). Type
tokens are open (`valid_type`: `[a-z][a-z0-9_]*`). `known_type` lists
the health and gallery helpers this crate documents; it is not a
monorepo enum and does not gate append.

## On-disk contract

- Layout: `log/<dev>/YYYY-MM.ndjson`
- Field order: `id`, `ts`, `dev`, `type`, `body`
- `ts` is UTC with 9 fractional digits (`2006-01-02T15:04:05.000000000Z`)
- Marshal matches Go `json.Encoder` with `SetEscapeHTML(false)` — `<`, `>`, `&` are not `\u00xx`-escaped
