# M3 — Face-pack re-key

Spike 0.6 / ADR 0006 R12–R13 replace insightface `buffalo_sc`
(SCRFD-500M + w600k_mbf) with OpenCV Zoo **YuNet** + **SFace**. The new
weights change [`ModelPack::face_pack_key`](../core/gallery-ml/src/pack.rs)
(`detector_hash + embedder_hash # preprocess + align`).
[`FaceEngine::with_models`](../core/gallery-ml/src/face/engine.rs) compares
that key to `meta.face_pack` and, on mismatch, calls
[`CacheDb::reset_face_results`](../core/gallery-ml/src/cache.rs).

This document is the ADR 0005 R19 record for that migration. The survival
fixture is `gallery-ml` integration test `m3_survival` (SQL + XMP; no ONNX).
The pack files themselves have **not** been swapped in this tree.

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

The SFace + YuNet ONNX files are not in the tree. Do **not** download
them as part of this fixture.

1. Teach `scripts/build_model_pack/` to fetch and hash-pin YuNet + SFace
   (OpenCV Zoo; Apache-2.0 / MIT). Today `face_models.py` still pulls
   `buffalo_sc.zip`.
2. Retire `buffalo_sc` and `PACK_VARIANT=full|tagging`. One pack; R12/R13
   no longer need a non-commercial split.
3. Recalibrate clustering thresholds (SFace is 128-d; current numbers
   are cosine bars for 512-d w600k_mbf).
4. Regenerate face goldens once the new weights are committed.

## Release-note obligation (ADR 0005 R19)

A migration that cannot preserve state MUST say so in release notes
**before** it ships, naming what is lost. When the pack swap lands, the
note must include:

- Face review, named clusters, and merge proposals in the app are reset.
- Names and boxes already written to `.xmp` files are kept.
- The library will be re-detected and re-clustered under the new models.
- Previously assigned in-app names are not automatically rebound.
