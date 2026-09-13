# contacts-core

Headless contacts app core (Phase 3.1). `cargo test` is the gate.

- Parse / write vCard 3.0 (`X-LOCALCONTACTS-ID`, canonical CRLF + fold).
- Folder index through `localcore-walk` + `localcore-vfs`. Conflict
  copies are never cards.
- R8–R11 merge: fixtures under `fixtures/r8/`. Copies are deleted only
  in `apply_merge`.
- UniFFI surface is `contacts-ffi` (R6-clean: `TextRow` / `FieldRow`).

vCards on disk are the authority. No UserDefaults, no dual-write log,
no queue. A later enqueue MUST NFC the primary key first.
