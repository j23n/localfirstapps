# contacts-core

Headless contacts app core (Phase 3.1). `cargo test` is the gate.

- Parse / write vCard 3.0 (`X-LOCALCONTACTS-ID`, canonical CRLF + fold).
- Folder index through `localcore-walk` + `localcore-vfs`. Conflict
  copies are never cards.
- R8–R11 merge: fixtures under `fixtures/r8/`. Copies are deleted only
  in `apply_merge`.
- Display rows, typed conflict disposition, edit draft, and
  save/delete/resolve actions live here. Tier-1 vCards win if the
  best-effort Tier-2 log append fails.
- UniFFI surface is `contacts-ffi` (display-record inventory green;
  serialized vCard reads remain explicit R6 debt).

vCards on disk are the authority. No UserDefaults, no dual-write log,
no queue. A later enqueue MUST NFC the primary key first.
