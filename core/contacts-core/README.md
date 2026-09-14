# contacts-core

Headless contacts app core (Phase 3.1). `cargo test` is the gate.

- Parse / write vCard 3.0 (`X-LOCALCONTACTS-ID`, canonical CRLF + fold).
- Folder index through `localcore-walk` + `localcore-vfs`. Conflict
  copies are never cards.
- R8–R11 merge: fixtures under `fixtures/r8/`. Copies are deleted only
  in `apply_merge`.
- Display rows, edit draft, and logged save/delete/resolve live here
  so both shells call the same functions (Milestone C).
- UniFFI surface is `contacts-ffi` (R6-clean: copies those display rows).

vCards on disk are the authority. No UserDefaults, no dual-write log,
no queue. A later enqueue MUST NFC the primary key first.
