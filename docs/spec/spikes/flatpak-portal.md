# Spike: Flatpak portal

**Status:** deployment decision recorded; measurements not run, reviewed 2026-09-14  
**Question:** Four measurements — (a) inotify through `xdg-document-portal`, (b) stat throughput over 20k entries vs a native path, (c) `rename()` of a temp file inside an exported directory, (d) whether the Comet image ships a portal backend.  
**Outcome:** **native-first; Flatpak is currently unsupported.**

## Answer

Linux delivery is a native GTK binary over the host filesystem. There is no Flatpak manifest, no `xdg-document-portal`, and no portal document grant.

The four measurements were not taken. The current native deployment uses a
real path and file-save sharing, so Phase 3 needs no portal work. This is not
evidence for a permanent architectural ban if distribution requirements
change.

Comet remains the same binary with `--comet`, also on a native path.

## Consequences

- ADR 0002 R6 is not reopened. Cheap `stat` holds because the deployment does not introduce a sandbox proxy.
- ADR 0007 R15 Linux rows lose the portal: folder grant is a path; share is file save.
- Phase 3 does not ship a Flatpak manifest or portal document access.
- Portal behavior remains unmeasured. Revisit it only if Flatpak becomes a
  product requirement.
