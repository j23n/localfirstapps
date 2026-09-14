# Spike: Face licence

**Status:** licensing answered; product validation open, reviewed 2026-09-14  
**Question:** Does a redistributable face embedder exist at acceptable quality?  
**Outcome:** **A permissive candidate exists.** Quality, alignment and device
cost are not yet established for this product.

## Answer

OpenCV Zoo **SFace** with **YuNet** is a license-compatible candidate.

| | Current | Chosen |
|---|---|---|
| Embedder | insightface `buffalo_sc` (MBF, 512-D) | OpenCV Zoo **SFace** (`face_recognition_sface_2021dec.onnx`, 128-D) |
| Embedder licence | research / non-commercial | **Apache-2.0** (weights and conversion) |
| Detector | SCRFD (same NC pack) | **YuNet** (MIT / Apache-2.0) |
| LFW (published) | InsightFace family, strong | **99.40%** (OpenCV Zoo `tools/eval`) |
| Runtime | ONNX / `ort` | ONNX / `ort` — same ADR 0002 R13 allowlist entry |

Published LFW accuracy is not evidence for this app's crop alignment,
personal-library clustering thresholds, migration UX, or target-device
runtime. Those measurements are required before selection.

## Rejected

- **AuraFace** (fal, Apache-2.0, ResNet100, LFW ~99.65%). Heavier; the Hugging Face pack may still ship InsightFace detection weights that stay non-commercial. Only the recognition ONNX would be usable, and YuNet already covers detection.
- **FaceX MobileFaceNet** (Apache-2.0, self-trained). Licence is clean; published train accuracy is too weak to trust for clustering without a dedicated eval.

## Consequences

- One distributable pack remains the goal. Do not retire
  `PACK_VARIANT=full|tagging` until the replacement passes product evidence.
- A swap would change `face_pack_key`: re-detect, re-embed and re-cluster,
  with ADR 0005 R19 migration handling.
- Phase 5 must test representative clusters, crop/landmark alignment,
  thresholds and target-device performance before choosing the pack.
