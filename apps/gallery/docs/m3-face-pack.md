# M3 — Face-pack re-key

Spike 0.6 / ADR 0006 R12–R13 identify OpenCV Zoo **YuNet** + **SFace**
as a licence-compatible candidate to replace insightface `buffalo_sc`
(SCRFD-500M + w600k_mbf). Phase 5B measured alignment, a deterministic
100-image LFW subset, and x86-64 cost, then rejected selection because no
arm64 or target-device run was available and peak RSS increased. If a future
evidence set supports the candidate, its weights will change
[`ModelPack::face_pack_key`](../core/gallery-ml/src/pack.rs)
(`detector_hash + embedder_hash # preprocess + align`).
[`FaceEngine::with_models`](../core/gallery-ml/src/face/engine.rs) compares
that key to `meta.face_pack` and, on mismatch, calls
[`CacheDb::reset_face_results`](../core/gallery-ml/src/cache.rs).

This document and `gallery-ml` integration test `m3_survival` (SQL + XMP; no
ONNX) are migration preflight. They prove the reset boundary, not that YuNet
+ SFace is suitable or selected. The pack files themselves have **not** been
swapped in this tree.

## What is lost

Derived data only. After the re-key the cache no longer holds:

- in-app cluster → name review (every `clusters` row, including `Named`)
- merge proposals
- cached detections and embeddings (`faces`, `face_scans`)

A cluster id the UI is still holding becomes `ClusterNotFound`. The
AUTOINCREMENT high-water mark is kept so the next cluster is a new id,
not a silent reuse of the old one.

Re-detect / re-embed / re-cluster rebuilds unlabeled groups. Previously
named people are **orphaned** in the cache: the app will not put Ada back
on a cluster until someone names it again.

## What survives

Tier-1 sidecar state. `People/<Name>` keywords, MWG-RS regions, and
`CoreFaceDecisions` stay on disk. `reset_face_results` does not open a
sidecar. A later naming that cannot see Ada must speak with
`Authority::Partial` so it does not retract her (standing decision: leak
a name rather than delete one because a model changed its mind).

## Remaining work

The SFace + YuNet ONNX files are not in the production pack or this migration
fixture. The evidence harness downloads hash-pinned temporary copies only.

1. Extend the recorded Phase 5B evidence with representative personal-library
   clustering, the identical model/input run on arm64, and actual iPhone and
   Comet runtime/peak-memory measurements.
2. If those measurements support selection, teach
   `scripts/build_model_pack/` to fetch and hash-pin YuNet + SFace (OpenCV
   Zoo; Apache-2.0 / MIT). Today `face_models.py` still pulls
   `buffalo_sc.zip`.
3. Add migration-specific M3 survival tests in the production-switch commit,
   then retire `PACK_VARIANT=full|tagging` only when the distributable pack
   actually includes the selected face models.
4. Recalibrate clustering thresholds (SFace is 128-d; current numbers
   are cosine bars for 512-d w600k_mbf).
5. Regenerate face goldens only after new weights are selected and committed.

## Release-note obligation (ADR 0005 R19)

A migration that cannot preserve state MUST say so in release notes
**before** it ships, naming what is lost. When the pack swap lands, the
note must include:

- Face review, named clusters, and merge proposals in the app are reset.
- Names and boxes already written to `.xmp` files are kept.
- The library will be re-detected and re-clustered under the new models.
- Previously assigned in-app names are not automatically rebound.
