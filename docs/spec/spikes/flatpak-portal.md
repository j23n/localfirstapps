# Spike: Flatpak portal

**Status:** current packaging recorded; measurements not run, reviewed 2026-09-14
**Question:** Four measurements — (a) inotify through `xdg-document-portal`, (b) stat throughput over 20k entries vs a native path, (c) `rename()` of a temp file inside an exported directory, (d) whether the Comet image ships a portal backend.  
**Outcome:** **Flatpak suitability is unmeasured.**

## Answer

Linux delivery is a native GTK binary over the host filesystem. There is no Flatpak manifest, no `xdg-document-portal`, and no portal document grant.

The four measurements were not taken. The current native deployment uses a
real path and file-save sharing, so Phase 3 needed no portal work. This is
neither evidence of Flatpak support nor evidence for an architectural ban.

Comet remains the same binary with `--comet`, also on a native path.

## Consequences

- The current native path does not introduce a sandbox proxy.
- ADR 0002 R6 requires portal stat, watch, read, and atomic-rename
  measurements before a Flatpak build claims conformance.
- Phase 3 shipped no Flatpak manifest or portal document access.
- Portal behaviour remains an open deployment hypothesis.
