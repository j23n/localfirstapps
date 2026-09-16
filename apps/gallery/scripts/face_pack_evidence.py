#!/usr/bin/env python3
"""Reproducible YuNet/SFace construction and face-pack evidence harness.

The model files and LFW images are downloaded into caller-selected temporary
directories. Nothing fetched by this script is a production pack or a
redistributable repository fixture.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import os
import platform
import resource
import shutil
import statistics
import sys
import time
import urllib.request
from collections import Counter
from pathlib import Path

OPENCV_ZOO_REVISION = "47534e27c9851bb1128ccc0102f1145e27f23f98"
MODEL_BASE = (
    "https://media.githubusercontent.com/media/opencv/opencv_zoo/"
    f"{OPENCV_ZOO_REVISION}/models"
)
CANDIDATE_MODELS = {
    "face_detector.onnx": {
        "url": f"{MODEL_BASE}/face_detection_yunet/face_detection_yunet_2023mar.onnx",
        "sha256": "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4",
        "bytes": 232_589,
        "model": "YuNet 2023mar",
        "license": "MIT",
    },
    "face_embedder.onnx": {
        "url": f"{MODEL_BASE}/face_recognition_sface/face_recognition_sface_2021dec.onnx",
        "sha256": "0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79",
        "bytes": 38_696_353,
        "model": "SFace 2021dec",
        "license": "Apache-2.0",
    },
}

LFW_REVISION = "12a61458b56d0433d07269dc1d64368abf4f6b4d"
LFW_ROOT = f"https://huggingface.co/datasets/marcelohaps/lfw/resolve/{LFW_REVISION}"
LFW_METADATA_URL = f"{LFW_ROOT}/train/metadata.csv"
LFW_METADATA_SHA256 = "ebe9dce66e8c751647086731dc87b329b5cfd2de21d79ae570bb464b8a51709a"

CURRENT_HASHES = {
    "face_detector.onnx": "5e4447f50245bbd7966bd6c0fa52938c61474a04ec7def48753668a9d8b4ea3a",
    "face_embedder.onnx": "9cc6e4a75f0e2bf0b1aed94578f144d15175f357bdc05e815e5c4a02b319eb4f",
}

ARCFACE_TEMPLATE = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
]


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def fetch(url: str, destination: Path, expected_sha256: str) -> None:
    if destination.exists() and sha256_file(destination) == expected_sha256:
        return
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_suffix(destination.suffix + ".part")
    with urllib.request.urlopen(url, timeout=120) as response, temporary.open("wb") as out:
        shutil.copyfileobj(response, out)
    actual = sha256_file(temporary)
    if actual != expected_sha256:
        temporary.unlink(missing_ok=True)
        raise SystemExit(f"{url} hashes to {actual}, expected {expected_sha256}")
    temporary.replace(destination)


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def construct_candidate(args: argparse.Namespace) -> None:
    args.out.mkdir(parents=True, exist_ok=True)
    for filename, spec in CANDIDATE_MODELS.items():
        fetch(spec["url"], args.out / filename, spec["sha256"])
        if (args.out / filename).stat().st_size != spec["bytes"]:
            raise SystemExit(f"{filename} has the wrong byte length")
    manifest = {
        "purpose": "phase-5b-evidence-only-not-a-production-pack",
        "source_revision": OPENCV_ZOO_REVISION,
        "models": CANDIDATE_MODELS,
        "detector_contract": {
            "input_size": 640,
            "framing": "top-left letterbox",
            "landmark_order": [
                "image-left eye",
                "image-right eye",
                "nose",
                "image-left mouth",
                "image-right mouth",
            ],
            "yunet_row_permutation_to_arcface": [0, 1, 2, 3, 4],
        },
        "embedder_contract": {
            "input_size": 112,
            "embedding_dim": 128,
            "alignment": "SFace reference five-point similarity transform",
        },
    }
    write_json(args.out / "evidence-manifest.json", manifest)
    print(json.dumps(manifest, sort_keys=True))


def fetch_lfw(args: argparse.Namespace) -> None:
    metadata = args.out / "metadata.csv"
    fetch(LFW_METADATA_URL, metadata, LFW_METADATA_SHA256)
    with metadata.open(newline="", encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle))
    counts = Counter(row["identity"] for row in rows)
    eligible = [name for name, count in counts.items() if count >= args.per_identity]
    eligible.sort(key=lambda name: (hashlib.sha256(name.encode()).hexdigest(), name))
    chosen = set(eligible[: args.identities])
    selected: dict[str, list[dict[str, str]]] = {name: [] for name in chosen}
    for row in rows:
        bucket = selected.get(row["identity"])
        if bucket is not None and len(bucket) < args.per_identity:
            bucket.append(row)

    files = []
    for identity in sorted(selected):
        for row in selected[identity]:
            relative = Path("images") / identity / row["source_filename"]
            destination = args.out / relative
            url = f"{LFW_ROOT}/train/{row['file_name']}"
            if not destination.exists():
                destination.parent.mkdir(parents=True, exist_ok=True)
                with urllib.request.urlopen(url, timeout=60) as response:
                    payload = response.read()
                destination.write_bytes(payload)
            files.append(
                {
                    "identity": identity,
                    "path": relative.as_posix(),
                    "sha256": sha256_file(destination),
                    "source_path": row["file_name"],
                }
            )
    manifest = {
        "dataset": "Labeled Faces in the Wild, original 250x250 images",
        "source": "marcelohaps/lfw on Hugging Face",
        "source_revision": LFW_REVISION,
        "selection": (
            f"first {args.identities} identities after sorting eligible identity names "
            f"by SHA-256; first {args.per_identity} source-order images each"
        ),
        "identity_count": len(selected),
        "images_per_identity": args.per_identity,
        "image_count": len(files),
        "representative_for": (
            "unconstrained capture and repeated-identity clustering; not a demographic "
            "or personal-library representativeness claim"
        ),
        "files": files,
    }
    write_json(args.out / "manifest.json", manifest)
    print(json.dumps({k: v for k, v in manifest.items() if k != "files"}, sort_keys=True))


def imports():
    try:
        import cv2
        import numpy as np
    except ImportError as error:
        raise SystemExit(
            "measurement needs numpy and opencv-python-headless in an isolated venv"
        ) from error
    cv2.setNumThreads(1)
    return cv2, np


def check_models(directory: Path, expected: dict[str, str]) -> None:
    for filename, digest in expected.items():
        path = directory / filename
        if not path.exists():
            raise SystemExit(f"missing {path}")
        actual = sha256_file(path)
        if actual != digest:
            raise SystemExit(f"{path} hashes to {actual}, expected {digest}")


def letterbox(rgb, size: int = 640):
    cv2, np = imports()
    height, width = rgb.shape[:2]
    scale = min(size / width, size / height)
    resized = cv2.resize(rgb, (round(width * scale), round(height * scale)))
    canvas = np.zeros((size, size, 3), dtype=np.uint8)
    canvas[: resized.shape[0], : resized.shape[1]] = resized
    return canvas, scale


def umeyama(src, dst):
    _, np = imports()
    src = np.asarray(src, dtype=np.float64)
    dst = np.asarray(dst, dtype=np.float64)
    src_mean = src.mean(axis=0)
    dst_mean = dst.mean(axis=0)
    src_centered = src - src_mean
    dst_centered = dst - dst_mean
    covariance = dst_centered.T @ src_centered / len(src)
    variance = (src_centered * src_centered).sum() / len(src)
    trace = covariance[0, 0] + covariance[1, 1]
    skew = covariance[1, 0] - covariance[0, 1]
    scale = math.hypot(trace, skew) / variance
    angle = math.atan2(skew, trace)
    sine, cosine = math.sin(angle), math.cos(angle)
    linear = scale * np.array([[cosine, -sine], [sine, cosine]])
    translate = dst_mean - linear @ src_mean
    return np.column_stack([linear, translate])


def align_crop(bgr, landmarks):
    cv2, np = imports()
    matrix = umeyama(landmarks, ARCFACE_TEMPLATE)
    return cv2.warpAffine(
        bgr,
        matrix,
        (112, 112),
        flags=cv2.INTER_LINEAR,
        borderMode=cv2.BORDER_CONSTANT,
        borderValue=(0, 0, 0),
    )


class CurrentPipeline:
    name = "SCRFD-500M+w600k_mbf"

    def __init__(self, directory: Path):
        cv2, _ = imports()
        check_models(directory, CURRENT_HASHES)
        self.detector = cv2.dnn.readNetFromONNX(str(directory / "face_detector.onnx"))
        self.embedder = cv2.dnn.readNetFromONNX(str(directory / "face_embedder.onnx"))
        self.output_names = ["443", "468", "493", "446", "471", "496", "449", "474", "499"]

    def detect(self, bgr):
        _, np = imports()
        rgb = bgr[:, :, ::-1]
        canvas, scale = letterbox(rgb)
        blob = np.transpose((canvas.astype(np.float32) / 255.0 - 0.5) / (128.0 / 255.0), (2, 0, 1))[None]
        self.detector.setInput(blob, "input.1")
        outputs = self.detector.forward(self.output_names)
        candidates = []
        for level, stride in enumerate([8, 16, 32]):
            scores = outputs[level].reshape(-1)
            boxes = outputs[3 + level].reshape(-1, 4)
            points = outputs[6 + level].reshape(-1, 5, 2)
            cells = 640 // stride
            for index in np.flatnonzero(scores >= 0.5):
                cell = int(index) // 2
                center = np.array([cell % cells, cell // cells], dtype=np.float32) * stride
                distance = boxes[index] * stride
                box = np.array(
                    [
                        center[0] - distance[0],
                        center[1] - distance[1],
                        center[0] + distance[2],
                        center[1] + distance[3],
                    ]
                ) / scale
                landmarks = (points[index] * stride + center) / scale
                if min(box[2] - box[0], box[3] - box[1]) >= 24:
                    candidates.append((float(scores[index]), box, landmarks))
        candidates.sort(
            key=lambda item: (
                -item[0],
                float(item[1][0]),
                float(item[1][1]),
                float(item[1][2]),
                float(item[1][3]),
            )
        )
        return candidates[:1]

    def embed(self, bgr, landmarks):
        _, np = imports()
        crop = align_crop(bgr, landmarks)
        rgb = crop[:, :, ::-1]
        blob = np.transpose((rgb.astype(np.float32) / 255.0 - 0.5) / 0.5, (2, 0, 1))[None]
        self.embedder.setInput(blob, "input.1")
        vector = self.embedder.forward("516").reshape(-1).astype(np.float32)
        return vector / np.linalg.norm(vector), crop


class CandidatePipeline:
    name = "YuNet+SFace"

    def __init__(self, directory: Path):
        cv2, _ = imports()
        expected = {name: spec["sha256"] for name, spec in CANDIDATE_MODELS.items()}
        check_models(directory, expected)
        self.detector = cv2.FaceDetectorYN_create(
            str(directory / "face_detector.onnx"),
            "",
            (640, 640),
            score_threshold=0.5,
            nms_threshold=0.3,
            top_k=5000,
        )
        self.embedder = cv2.FaceRecognizerSF_create(str(directory / "face_embedder.onnx"), "")

    def detect(self, bgr):
        cv2, np = imports()
        rgb = bgr[:, :, ::-1]
        canvas_rgb, scale = letterbox(rgb)
        _, rows = self.detector.detect(canvas_rgb[:, :, ::-1])
        if rows is None:
            return []
        candidates = []
        for row in rows:
            x, y, width, height = row[:4]
            box = np.array([x, y, x + width, y + height], dtype=np.float32) / scale
            landmarks = row[4:14].reshape(5, 2).astype(np.float32) / scale
            if min(width, height) / scale >= 24:
                candidates.append((float(row[14]), box, landmarks))
        candidates.sort(
            key=lambda item: (
                -item[0],
                float(item[1][0]),
                float(item[1][1]),
                float(item[1][2]),
                float(item[1][3]),
            )
        )
        return candidates[:1]

    def embed(self, bgr, landmarks):
        _, np = imports()
        crop = align_crop(bgr, landmarks)
        vector = self.embedder.feature(crop).reshape(-1).astype(np.float32)
        return vector / np.linalg.norm(vector), crop

    def reference_crop(self, bgr, detection):
        _, np = imports()
        score, box, landmarks = detection
        row = np.concatenate(
            [
                np.array([box[0], box[1], box[2] - box[0], box[3] - box[1]]),
                landmarks.reshape(-1),
                np.array([score]),
            ]
        ).astype(np.float32)
        return self.embedder.alignCrop(bgr, row)


def load_pipeline(kind: str, directory: Path):
    return CurrentPipeline(directory) if kind == "current" else CandidatePipeline(directory)


def rss_mib() -> float:
    status = Path("/proc/self/status")
    if not status.exists():
        return 0.0
    for line in status.read_text().splitlines():
        if line.startswith("VmRSS:"):
            return int(line.split()[1]) / 1024.0
    return 0.0


def percentile(values, p: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = (len(ordered) - 1) * p
    lower = math.floor(index)
    upper = math.ceil(index)
    if lower == upper:
        return float(ordered[lower])
    return float(ordered[lower] * (upper - index) + ordered[upper] * (index - lower))


def pair_metrics(entries):
    _, np = imports()
    similarities = []
    labels = []
    for left in range(len(entries)):
        for right in range(left + 1, len(entries)):
            similarities.append(float(np.dot(entries[left]["embedding"], entries[right]["embedding"])))
            labels.append(entries[left]["identity"] == entries[right]["identity"])
    order = np.argsort(-np.asarray(similarities))
    y = np.asarray(labels, dtype=bool)[order]
    scores = np.asarray(similarities)[order]
    tp = np.cumsum(y)
    fp = np.cumsum(~y)
    positives = int(y.sum())
    precision = tp / np.maximum(tp + fp, 1)
    recall = tp / max(positives, 1)
    f1 = 2 * precision * recall / np.maximum(precision + recall, 1e-12)
    best = int(np.argmax(f1))
    threshold = float(scores[best])

    same = [score for score, label in zip(similarities, labels) if label]
    different = [score for score, label in zip(similarities, labels) if not label]
    wins = sum(a > b for a in same for b in different)
    ties = sum(a == b for a in same for b in different)
    auc = (wins + 0.5 * ties) / max(len(same) * len(different), 1)
    return {
        "pairs": len(similarities),
        "same_pairs": len(same),
        "different_pairs": len(different),
        "same_min": min(same) if same else None,
        "same_p05": percentile(same, 0.05),
        "different_p95": percentile(different, 0.95),
        "different_p99": percentile(different, 0.99),
        "different_max": max(different) if different else None,
        "auc": auc,
        "best_pair_f1": float(f1[best]),
        "best_pair_threshold": threshold,
    }


def cluster_metrics(entries, threshold: float):
    _, np = imports()
    parent = list(range(len(entries)))

    def root(index):
        while parent[index] != index:
            parent[index] = parent[parent[index]]
            index = parent[index]
        return index

    def union(left, right):
        a, b = root(left), root(right)
        if a != b:
            parent[max(a, b)] = min(a, b)

    for left in range(len(entries)):
        for right in range(left + 1, len(entries)):
            if float(np.dot(entries[left]["embedding"], entries[right]["embedding"])) >= threshold:
                union(left, right)
    tp = fp = fn = 0
    for left in range(len(entries)):
        for right in range(left + 1, len(entries)):
            predicted = root(left) == root(right)
            actual = entries[left]["identity"] == entries[right]["identity"]
            tp += predicted and actual
            fp += predicted and not actual
            fn += not predicted and actual
    precision = tp / max(tp + fp, 1)
    recall = tp / max(tp + fn, 1)
    groups = Counter(root(index) for index in range(len(entries)))
    return {
        "threshold": threshold,
        "clusters": len(groups),
        "largest_cluster": max(groups.values(), default=0),
        "pair_precision": precision,
        "pair_recall": recall,
        "pair_f1": 2 * precision * recall / max(precision + recall, 1e-12),
        "false_join_pairs": fp,
        "missed_same_identity_pairs": fn,
    }


def measure(args: argparse.Namespace) -> None:
    cv2, np = imports()
    manifest = json.loads((args.library / "manifest.json").read_text())
    pipeline = load_pipeline(args.model, args.models)
    loaded_rss = rss_mib()
    entries = []
    missed = []
    timings = []
    digest = hashlib.sha256()
    started = time.perf_counter()
    for row in manifest["files"]:
        path = args.library / row["path"]
        bgr = cv2.imread(str(path), cv2.IMREAD_COLOR)
        if bgr is None:
            raise SystemExit(f"could not decode {path}")
        one = time.perf_counter()
        detections = pipeline.detect(bgr)
        if not detections:
            missed.append(row["path"])
            continue
        vector, _ = pipeline.embed(bgr, detections[0][2])
        timings.append((time.perf_counter() - one) * 1000)
        vector = vector.astype("<f4", copy=False)
        digest.update(row["identity"].encode() + b"\0" + row["path"].encode() + b"\0")
        digest.update(vector.tobytes())
        entries.append(
            {
                "identity": row["identity"],
                "path": row["path"],
                "embedding": vector,
            }
        )
    elapsed = time.perf_counter() - started
    pairs = pair_metrics(entries)
    clusters = cluster_metrics(entries, pairs["best_pair_threshold"])
    peak_mib = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1024.0
    output = {
        "schema": 1,
        "model": args.model,
        "pipeline": pipeline.name,
        "model_hashes": {
            name: sha256_file(args.models / name)
            for name in ["face_detector.onnx", "face_embedder.onnx"]
        },
        "dataset_revision": manifest["source_revision"],
        "dataset_selection": manifest["selection"],
        "identity_count": manifest["identity_count"],
        "images_requested": len(manifest["files"]),
        "images_embedded": len(entries),
        "images_missed": len(missed),
        "missed_paths": missed,
        "quality": {"pairs": pairs, "single_link_clustering": clusters},
        "performance": {
            "elapsed_ms": elapsed * 1000,
            "mean_image_ms": statistics.mean(timings) if timings else None,
            "p95_image_ms": percentile(timings, 0.95),
            "model_loaded_rss_mib": loaded_rss,
            "process_peak_rss_mib": peak_mib,
            "measured_peak_delta_mib": max(0.0, peak_mib - loaded_rss),
        },
        "execution": {
            "architecture": platform.machine(),
            "platform": platform.platform(),
            "python": platform.python_version(),
            "opencv": cv2.__version__,
            "opencv_threads": 1,
        },
        "identical_input_embedding_sha256": digest.hexdigest(),
    }
    serializable = json.loads(
        json.dumps(output, default=lambda value: value.item() if hasattr(value, "item") else value)
    )
    write_json(args.out, serializable)
    print(json.dumps(serializable, sort_keys=True))


def alignment(args: argparse.Namespace) -> None:
    cv2, np = imports()
    pipeline = CandidatePipeline(args.models)
    rows = []
    for path in args.images:
        bgr = cv2.imread(str(path), cv2.IMREAD_COLOR)
        detections = pipeline.detect(bgr)
        if not detections:
            raise SystemExit(f"YuNet did not detect a face in {path}")
        detection = detections[0]
        vector, direct = pipeline.embed(bgr, detection[2])
        reference = pipeline.reference_crop(bgr, detection)
        swapped_landmarks = detection[2][[1, 0, 2, 4, 3]]
        swapped_vector, swapped = pipeline.embed(bgr, swapped_landmarks)
        difference = np.abs(direct.astype(np.int16) - reference.astype(np.int16))
        rows.append(
            {
                "image": path.name,
                "image_sha256": sha256_file(path),
                "bbox_xyxy": [float(value) for value in detection[1]],
                "landmarks_arcface_order": [
                    [float(point[0]), float(point[1])] for point in detection[2]
                ],
                "detector_score": detection[0],
                "direct_order_crop_sha256": sha256_bytes(direct.tobytes()),
                "swapped_order_crop_sha256": sha256_bytes(swapped.tobytes()),
                "reference_crop_sha256": sha256_bytes(reference.tobytes()),
                "direct_vs_reference_max_channel_delta": int(difference.max()),
                "direct_vs_reference_mean_channel_delta": float(difference.mean()),
                "direct_vs_swapped_embedding_cosine": float(np.dot(vector, swapped_vector)),
                "order_assertion": bool(
                    detection[2][0][0] < detection[2][1][0]
                    and detection[2][3][0] < detection[2][4][0]
                ),
            }
        )
    output = {
        "schema": 1,
        "candidate_revision": OPENCV_ZOO_REVISION,
        "model_hashes": {
            name: sha256_file(args.models / name)
            for name in ["face_detector.onnx", "face_embedder.onnx"]
        },
        "yunet_row_permutation_to_arcface": [0, 1, 2, 3, 4],
        "fixtures": rows,
        "execution_architecture": platform.machine(),
    }
    write_json(args.out, output)
    print(json.dumps(output, sort_keys=True))


def compare(args: argparse.Namespace) -> None:
    current = json.loads(args.current.read_text())
    candidate = json.loads(args.candidate.read_text())
    if current["dataset_revision"] != candidate["dataset_revision"]:
        raise SystemExit("runs used different dataset revisions")
    quality = {}
    for name, run in [("current", current), ("candidate", candidate)]:
        quality[name] = {
            "images_embedded": run["images_embedded"],
            "auc": run["quality"]["pairs"]["auc"],
            "best_pair_f1": run["quality"]["pairs"]["best_pair_f1"],
            "cluster_pair_f1": run["quality"]["single_link_clustering"]["pair_f1"],
            "false_join_pairs": run["quality"]["single_link_clustering"]["false_join_pairs"],
            "missed_same_identity_pairs": run["quality"]["single_link_clustering"][
                "missed_same_identity_pairs"
            ],
            "mean_image_ms": run["performance"]["mean_image_ms"],
            "process_peak_rss_mib": run["performance"]["process_peak_rss_mib"],
        }
    architectures = sorted(
        {current["execution"]["architecture"], candidate["execution"]["architecture"]}
    )
    output = {
        "schema": 1,
        "dataset_revision": current["dataset_revision"],
        "quality": quality,
        "execution_architectures": architectures,
        "selection_gate": {
            "sufficient": False,
            "reason": (
                "This harness measures a public repeated-identity library on the listed "
                "architectures. Consult a compare-isa report for an identical-model/input "
                "x86_64/arm64 comparison; target-device cost is also required."
            ),
        },
    }
    write_json(args.out, output)
    print(json.dumps(output, sort_keys=True))


def canonical_architecture(value: str) -> str:
    if value in {"arm64", "aarch64"}:
        return "arm64"
    return value


def compare_isa(args: argparse.Namespace) -> None:
    reports = [json.loads(path.read_text()) for path in args.reports]
    groups: dict[tuple[str, str, str], list[dict[str, object]]] = {}
    for report in reports:
        key = (
            report["model"],
            json.dumps(report["model_hashes"], sort_keys=True),
            report["dataset_revision"],
        )
        groups.setdefault(key, []).append(report)

    comparisons = []
    for (model, model_hashes, dataset_revision), runs in sorted(groups.items()):
        by_arch = {
            canonical_architecture(run["execution"]["architecture"]): run for run in runs
        }
        if "x86_64" not in by_arch or "arm64" not in by_arch:
            continue
        x86_hash = by_arch["x86_64"]["identical_input_embedding_sha256"]
        arm_hash = by_arch["arm64"]["identical_input_embedding_sha256"]
        comparisons.append(
            {
                "model": model,
                "model_hashes": json.loads(model_hashes),
                "dataset_revision": dataset_revision,
                "x86_64_embedding_sha256": x86_hash,
                "arm64_embedding_sha256": arm_hash,
                "byte_identical": x86_hash == arm_hash,
            }
        )

    architectures = sorted(
        {
            canonical_architecture(report["execution"]["architecture"])
            for report in reports
        }
    )
    output = {
        "schema": 1,
        "reports": [str(path) for path in args.reports],
        "execution_architectures": architectures,
        "identical_model_input_comparisons": comparisons,
        "outcome": "available" if comparisons else "unavailable",
        "reason": (
            None
            if comparisons
            else "no x86_64/arm64 reports with identical model hashes and dataset revision"
        ),
    }
    write_json(args.out, output)
    print(json.dumps(output, sort_keys=True))


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    sub = root.add_subparsers(dest="command", required=True)

    construct = sub.add_parser("construct-candidate")
    construct.add_argument("--out", type=Path, required=True)
    construct.set_defaults(function=construct_candidate)

    lfw = sub.add_parser("fetch-lfw")
    lfw.add_argument("--out", type=Path, required=True)
    lfw.add_argument("--identities", type=int, default=20)
    lfw.add_argument("--per-identity", type=int, default=5)
    lfw.set_defaults(function=fetch_lfw)

    run = sub.add_parser("measure")
    run.add_argument("--model", choices=["current", "candidate"], required=True)
    run.add_argument("--models", type=Path, required=True)
    run.add_argument("--library", type=Path, required=True)
    run.add_argument("--out", type=Path, required=True)
    run.set_defaults(function=measure)

    align = sub.add_parser("alignment")
    align.add_argument("--models", type=Path, required=True)
    align.add_argument("--images", type=Path, nargs="+", required=True)
    align.add_argument("--out", type=Path, required=True)
    align.set_defaults(function=alignment)

    comparison = sub.add_parser("compare")
    comparison.add_argument("--current", type=Path, required=True)
    comparison.add_argument("--candidate", type=Path, required=True)
    comparison.add_argument("--out", type=Path, required=True)
    comparison.set_defaults(function=compare)

    isa = sub.add_parser("compare-isa")
    isa.add_argument("--reports", type=Path, nargs="+", required=True)
    isa.add_argument("--out", type=Path, required=True)
    isa.set_defaults(function=compare_isa)
    return root


def main() -> None:
    args = parser().parse_args()
    args.function(args)


if __name__ == "__main__":
    main()
