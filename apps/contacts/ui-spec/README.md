# contacts UI spec

Semantic screens for localcontacts (ADR 0004). Both shells and
`contacts-core` tests read this. It does not declare geometry.

```
python3 scripts/gen_r14.py   # emits ContactsScreen in Rust + Swift
```

Milestone C core loop: `folder-picker`, `contact-list`,
`contact-detail`, `contact-edit`, `settings` (partial),
`sync-conflict-group`. Later: `tag-management`, `logs`.
`apple-conflict` is the iOS CN merge sheet (ADR 0007 R15).
`sync-conflict-group` is the Syncthing sheet (ADR 0005 R8–R11):
iOS `SyncConflictGroupSheet`, GTK conflict sheet.
