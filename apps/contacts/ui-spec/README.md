# contacts UI spec

Semantic screen inventory for localcontacts (ADR 0004). The R14 generator
reads this file and emits identifiers; the shells do not assemble their views
from it. It does not declare geometry or prove that a screen is implemented.

```
python3 scripts/gen_r14.py   # emits ContactsScreen in Rust + Swift
```

Milestone C implements: `folder-picker`, `contact-list`,
`contact-detail`, `contact-edit`, `settings` (partial),
`sync-conflict-group`. Outstanding debt: `tag-management`, `logs`.
`apple-conflict` is the iOS CN merge sheet (ADR 0007 R15).
`sync-conflict-group` is the Syncthing sheet (ADR 0005 R8–R11):
iOS `SyncConflictGroupSheet`, GTK conflict sheet.
