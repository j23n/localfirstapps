# Spike: Face licence

**Status:** answered, 2026-09-11  
**Question:** Does a redistributable face embedder exist at acceptable quality?  
**Outcome:** **1 — a permissive embedder is found.** ADR 0006 R12 and R13 stand unchanged.

## Answer

Yes. Ship **OpenCV Zoo SFace** as the face embedder, with **YuNet** as the detector.

| | Current | Chosen |
|---|---|---|
| Embedder | insightface `buffalo_sc` (MBF, 512-D) | OpenCV Zoo **SFace** (`face_recognition_sface_2021dec.onnx`, 128-D) |
| Embedder licence | research / non-commercial | **Apache-2.0** (weights and conversion) |
| Detector | SCRFD (same NC pack) | **YuNet** (MIT / Apache-2.0) |
| LFW (published) | InsightFace family, strong | **99.40%** (OpenCV Zoo `tools/eval`) |
| Runtime | ONNX / `ort` | ONNX / `ort` — same ADR 0002 R13 allowlist entry |

SFace is the standard commercial-safe recogniser paired with YuNet (LocalAI and OpenBiometrics both treat that pair as the default for this reason). Quality is a step down from the large InsightFace packs. That is accepted for personal-library clustering.

## Rejected

- **AuraFace** (fal, Apache-2.0, ResNet100, LFW ~99.65%). Heavier; the Hugging Face pack may still ship InsightFace detection weights that stay non-commercial. Only the recognition ONNX would be usable, and YuNet already covers detection.
- **FaceX MobileFaceNet** (Apache-2.0, self-trained). Licence is clean; published train accuracy is too weak to trust for clustering without a dedicated eval.

## Consequences

- One pack. `PACK_VARIANT=full|tagging` is retired — it existed only to hide the non-commercial embedder.
- Swapping models changes `face_pack_key`. **M3 is real**, not conditional: re-detect, re-embed, re-cluster; user-assigned names are orphaned. ADR 0005 R19 applies.
- No user-installed face pack. No capability drop. No amendment to R12 or R13.
