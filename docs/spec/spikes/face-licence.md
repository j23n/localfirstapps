# Spike: Face licence

**Status:** licensing answered; x86-64 evidence run and production switch rejected, reviewed 2026-09-16
**Question:** Does a redistributable face embedder exist at acceptable quality?  
**Outcome:** **A permissive candidate exists, but the measured evidence is
insufficient to select it.** Keep the current pack split.

## Answer

OpenCV Zoo **SFace** with **YuNet** is a license-compatible candidate.

| | Current | Candidate |
|---|---|---|
| Embedder | insightface `buffalo_sc` (MBF, 512-D) | OpenCV Zoo **SFace** (`face_recognition_sface_2021dec.onnx`, 128-D) |
| Embedder licence | research / non-commercial | **Apache-2.0** (weights and conversion) |
| Detector | SCRFD (same NC pack) | **YuNet** (MIT / Apache-2.0) |
| LFW (published) | InsightFace family, strong | **99.40%** (OpenCV Zoo `tools/eval`) |
| Runtime | ONNX / `ort` | ONNX / `ort` — covered by the current reviewed ADR 0002 R13 build-time exception |

The Phase 5B harness hash-pinned YuNet/SFace at OpenCV Zoo revision
`47534e27c9851bb1128ccc0102f1145e27f23f98` and ran two committed face
fixtures plus a deterministic 100-image, 20-identity LFW subset. Full machine
evidence is in `docs/spec/evidence/gallery-phase5b-2026-09-16.json`.

- YuNet's rows were already in image-left eye, image-right eye, nose,
  image-left mouth, image-right mouth order. The required permutation is
  `[0,1,2,3,4]`, correcting the earlier unmeasured opposite-eye assumption.
  Direct crops differed from OpenCV SFace's reference alignment by at most one
  channel value on both fixtures.
- On 4,950 LFW pairs, current SCRFD/w600k scored AUC **0.9588** and best pair
  F1 **0.9501**; YuNet/SFace scored AUC **0.9678** and F1 **0.9691**. At each
  run's measured threshold, single-link clustering had zero false joins and
  missed **19** versus **12** same-identity pairs.
- Single-threaded x86-64 mean latency was **47.21 ms/image** current versus
  **44.74 ms/image** candidate. P95 was **64.14 ms** versus **79.73 ms**.
  Process peak RSS was **144.36 MiB** versus **184.34 MiB**.

This is useful positive candidate evidence, not a representative
personal-library or target-device result. Only x86-64 execution was available;
there is no identical-model/input arm64 result and no iPhone or Comet runtime
or peak-memory measurement. The production switch is therefore explicitly
**rejected for this revision**. The existing weights and thresholds are
unchanged.

## Rejected

- **AuraFace** (fal, Apache-2.0, ResNet100, LFW ~99.65%). Heavier; the Hugging Face pack may still ship InsightFace detection weights that stay non-commercial. Only the recognition ONNX would be usable, and YuNet already covers detection.
- **FaceX MobileFaceNet** (Apache-2.0, self-trained). Licence is clean; published train accuracy is too weak to trust for clustering without a dedicated eval.

## Consequences

- One distributable pack remains the goal. Do not retire
  `PACK_VARIANT=full|tagging`; Phase 5B did not pass the selection gate.
- A swap would change `face_pack_key`: re-detect, re-embed and re-cluster,
  with ADR 0005 R19 migration handling.
- A future selection must add separate M3 survival coverage, recalibrate
  SFace thresholds, run the same model/input on x86-64 and arm64, and measure
  actual iPhone and Comet cost before retiring `PACK_VARIANT`.
