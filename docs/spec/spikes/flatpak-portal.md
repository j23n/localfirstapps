# Spike: Flatpak portal

**Status:** reproducible harness and native controls run; live portal unavailable, reviewed 2026-09-16
**Question:** Four measurements — (a) inotify through `xdg-document-portal`, (b) stat throughput over 20k entries vs a native path, (c) `rename()` of a temp file inside an exported directory, (d) whether the Comet image ships a portal backend.  
**Outcome:** **Reject a Flatpak support claim in this revision.** Native
filesystem feasibility passed, but no live portal/session evidence exists.

## Answer

Linux delivery remains a native GTK binary over the host filesystem. The
Contacts and Music shells now make the folder workflow concrete, so Phase 5B
added `scripts/flatpak_portal_experiment.py`. It accepts a folder returned by
a portal-backed chooser and labels a run as portal evidence only when a
document-portal D-Bus owner, documents mount, and path under that mount are
all observed.

The available x86-64 environment had no D-Bus session address, display, portal
owner, documents mount, or folder grant. Persisted portal access and Comet
backend availability could not be tested and are not claimed.

The same harness ran native controls: a separate writer process was visible
to both rescan and inotify; atomic replace succeeded while the conflict loser
survived until an explicit delete; and a 20,000-entry stat walk completed in
**450.92 ms** (**44,354 entries/s**). These show that the proposed rescan,
Syncthing visibility, and preservation-first conflict workflow work on the
native host path. They do not establish that `xdg-document-portal` preserves
those semantics or that a grant survives a desktop-session restart. Recorded
machine evidence is in
`docs/spec/evidence/gallery-phase5b-2026-09-16.json`.

Comet remains the same binary with `--comet`, also on a native path.

## Consequences

- The current native path does not introduce a sandbox proxy.
- ADR 0002 R6 requires portal stat, watch, read, and atomic-rename
  measurements before a Flatpak build claims conformance.
- Phase 3 shipped no Flatpak manifest or portal document access.
- Flatpak support is rejected for this revision, rather than reported as
  silently unmeasured. A real GTK chooser grant must be fed to the harness,
  rerun after a desktop-session restart, and repeated on the target Comet
  image before that decision can change.
