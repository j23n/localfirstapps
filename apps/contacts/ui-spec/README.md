# contacts UI spec

Semantic screens for localcontacts (ADR 0004). Both shells and
`contacts-core` tests read this. It does not declare geometry.

```
python3 scripts/gen_r14.py   # emits ContactsScreen in Rust + Swift
```

`sync-conflict-group` is the Milestone C Syncthing sheet (ADR 0005
R8–R11). iOS binds it as `SyncConflictGroupSheet`; GTK as the
contacts-gtk conflict sheet. `apple-conflict` is the existing CN
merge sheet (ADR 0007 R15, iOS host).
