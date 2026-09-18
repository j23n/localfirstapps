# Mutations, storage, and backup

What the apps write, where it lives, and what to copy if you care
about the library.

## Library folder (source of truth)

Photos and videos stay ordinary files. The apps also:

| Action | What happens |
|---|---|
| Scan Photos / tagging / faces / Places | Writes or updates `photo.jpg.xmp` next to the file. Image bytes are not rewritten. |
| Name / rename / merge people | Updates People keywords and MWG regions in that sidecar. |
| Move photo | Moves the image and its companions (sidecar, Live Photo pair, etc.) into another folder in the library. |
| Delete photo | Deletes the image and its companions. |
| Create folder | Creates a directory in the library tree. |

These are explicit user or Scan Photos actions, not background
rewrites of the originals. Confirmations apply to delete and move.

Current tagging writer treats `Objects/*` and `Scenes/*` as a replace
set for those roots; People, Places, Landmarks, and bare keywords are
left alone. The standing policy is narrower
([ADR 0001](adr/0001-xmp-ownership.md)): retract only sentinel-owned
claims. Unrelated XML is preserved on the DOM path.

Places: GPS is resolved offline; the resulting `Places/…` path is
written to the sidecar.

## Caches (recomputable)

| Store | Typical location | Contents |
|---|---|---|
| `gallery-cache.sqlite` | iOS Application Support; Linux XDG data | ML queues, embeddings, face clusters |
| Library snapshot JSON | App cache | Last tree + optional sidecar manifest (v20) |
| Memories cache | App cache | Generated rail; evicted if the library snapshot version mismatches |
| Sidecar parse cache (iOS) | App cache | Parsed XMP from locally read sidecars |
| Thumbnails | iOS disk cache; Linux Freedesktop `thumbnails/large` and `x-large` | Display only |
| Geo cache | App support / XDG | Place-lookup results by coordinates |
| Widget snapshots (iOS) | App Group | Pre-rendered tiles and deep-link ids |

Deleting caches does not delete photos or sidecars. The next scan or
analysis rebuilds them.

## Backup

Copy the **library folder**, including every `.xmp`. That is the
portable gallery. Application Support / XDG caches are optional; they
only save time.

If you sync with Syncthing (or SyncTrain on iOS), include sidecars.
Exclude `gallery-cache.sqlite` if you sync the same folder the app
uses as a library — the cache is per-device.

## Person state (tier 2)

Hidden / featured / me / cover-photo / contact-link decisions live in
the synced event log after folder attach:

`{library}/.gallery/log/<dev>/YYYY-MM.ndjson`

That log is the authority. The five person keys
(`hiddenPeople`, `pinnedPeople`, `featuredPhotoByPerson`,
`mePersonPath`, `personContactLinks`) are not written back to
UserDefaults. On attach, this device may migrate a leftover
UserDefaults snapshot into the log (one-shot: a `person_migrated`
marker written last) and then drop those keys.

The log syncs with the library. The device id (`galleryDeviceId` in
UserDefaults) is the ADR 0005 R5 per-device exception and must not
sync. Memory chrome (`hiddenMemories`, `seenMemoryIDs`,
`surfacedClusters`, `birthdayMemoriesEnabled`, `memoriesGeneratedDay`)
is still a UserDefaults snapshot and is not in this log.

This is a schema-defined domain event log, not a diagnostic log. It syncs
because replayed person decisions must follow the folder.

A pre-M2 UserDefaults dump lives at
`core/localcore-log/tests/fixtures/m2/userdefaults-person-state.json`.

## Local diagnostics

Diagnostic capture is opt-in and per-device. The in-app ring buffer and any
explicitly exported diagnostic file stay outside the library folder; they do
not sync or upload automatically. Clearing diagnostics does not change the
person event log or any projected domain state.

## Provider-backed folders (iOS)

Provider-backed folders are accepted only for entries whose bytes are already
local. The app does not request downloads, materialise placeholders, track
provider progress, or preserve searchability by fetching sidecars in bulk. A
non-resident entry is absent from the projection until the provider makes its
bytes local outside the app.
