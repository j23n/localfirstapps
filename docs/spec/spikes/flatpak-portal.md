# Spike: Flatpak portal

**Status:** answered, 2026-09-11  
**Question:** Four measurements — (a) inotify through `xdg-document-portal`, (b) stat throughput over 20k entries vs a native path, (c) `rename()` of a temp file inside an exported directory, (d) whether the Comet image ships a portal backend.  
**Outcome:** **do not use Flatpak or portals at all.**

## Answer

Linux delivery is a native GTK binary over the host filesystem. There is no Flatpak manifest, no `xdg-document-portal`, and no portal document grant.

The four measurements are therefore not taken: they describe a deployment this project will not ship. Folder access is a real path. Share is a file save. Inotify and `stat` run against the tree the synchroniser writes, which is the deployment ADR 0002 R6 already assumes.

Comet remains the same binary with `--comet`, also on a native path.

## Consequences

- ADR 0002 R6 is not reopened. Cheap `stat` holds because the deployment does not introduce a sandbox proxy.
- ADR 0007 R15 Linux rows lose the portal: folder grant is a path; share is file save.
- Phase 3 does not ship a Flatpak manifest or portal document access.
- The portal-over-Syncthing risk is retired, not deferred.
