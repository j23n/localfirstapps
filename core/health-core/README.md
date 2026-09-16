# health-core

Headless owner of the localhealth archive formats. The Go `archive` CLI remains
the writer until the Phase 6 cutover; this crate is the Rust projection and
portable-v1 surface.

- Log and blobs are reused from `localcore-log` / `localcore-blob`.
- `derived/archive.db` is disposable and rebuilt from complete log lines.
- Sample rows come from `observation`/`episode` events or streamed NDJSON
  sample blobs. Apple `export.xml` is not parsed (ADR 0008).
- Semantic table dumps, not SQLite bytes, are the parity contract.
