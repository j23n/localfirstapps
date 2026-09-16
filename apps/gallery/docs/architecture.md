# Architecture and trust boundaries

Current layout and what is allowed to cross each edge. Decisions:
[ADR 0001](adr/0001-xmp-ownership.md),
[ADR 0002](adr/0002-scan-freshness.md),
[ADR 0003](adr/0003-validation-only-release.md).

## Pieces

```
LocalGallery/          iOS shell: bookmarks, UI, widgets
linux/                 GTK4 / libadwaita shell (outside the Cargo workspace)
core/                  Cargo workspace — engines and XMP
  gallery-model        photo / folder / snapshot types, stable ids
  gallery-vfs          Vfs + atomic write
  gallery-meta         sidecar + embedded metadata read; sidecar write
  gallery-scan         tree walk and scan outcome
  gallery-index        search + tag buckets
  gallery-memories     memory selection and 7-day horizon
  gallery-ml           tagging, faces, HEIC software decode, cache DB
  gallery-session      shared Scan Photos order, pack roots, Places, mute
                       (Places lookup is localcore-geo; haversine cache here)
  gallery-ffi          UniFFI surface (iOS only)
scripts/               build_core.sh, prepare_pack.sh, model-pack builder
```

iOS talks to the core through UniFFI (`GalleryCore`). Linux links the
crates in-process. Neither shell reaches past `gallery-ffi` /
`gallery-session` into engine internals for product policy.

## Trust boundaries

| Boundary | Inside | Outside |
|---|---|---|
| Library folder | User-selected tree (security-scoped bookmark on iOS; a path on Linux) | The rest of the filesystem. Sidecar writes and move/delete/create stay under that root. |
| Image bytes | Never rewritten by the core | Sidecars (`.xmp`), caches, exports the user asked for |
| `gallery-cache.sqlite` | Work queues, embeddings, face clusters | Not portable truth; wipe is safe |
| Provider-backed folder (iOS) | Folder grant may come from the host; only entries with local readable bytes enter the scanner. | No provider probe, download request, materialisation, progress, or remote badge |
| Place names | Bundled gazetteer + admin-0 polygons (`localcore-geo`) | Coordinates stay on the device. Country is point-in-polygon, not nearest city. |
| Model pack | Local ONNX + labels, hash-verified | Optional. Missing pack disables tagging and faces only. |
| Synced person log | Schema-defined tier-2 operations under `.gallery/log/<dev>/` | Domain replay only; not diagnostics |
| Local diagnostics | Opt-in in-app ring buffer (`LogStore`), outside the library | No MetricKit, automatic export, or upload |
| Widgets (iOS) | App Group snapshots | Deep links back into the app |

## Analysis decode

- **iOS tagging/faces:** ImageIO (hardware) for HEIC; JPEG/PNG stay on
  the pinned Rust preprocess path.
- **`cargo test` and Linux:** software HEVC (`heif-oxide` + `rust_h265`)
  for HEIC; same JPEG/PNG crates. A host ImageIO decoder is never
  installed on those builds.
- Display pixels (grid, viewer) are a separate path. Tagging does not
  read Freedesktop or QuickLook thumbs.

## FFI rules (iOS)

Coarse-grained calls, typed error enums, long work on core-owned
threads with start/cancel/progress. UniFFI proc-macro mode, namespace
`GalleryCore`. A rustdoc on an exported item must not contain the
two-character sequence slash-star: UniFFI copies docs into nested
Swift block comments.

## Determinism

Decision convergence across devices uses pinned pack hashes, recorded
decisions, byte-idempotent writes, and hysteresis. The current epsilon has
not been measured as a cross-ISA bound. Results key on **content hash**, not
path.
