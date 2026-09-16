#!/usr/bin/env python3
"""ADR 0007 R16 — keep human-review questions complete and in sync.

`checklist.toml` is the machine-readable source of truth. The pull-request
template carries the same stable ids and exact prompts so a future reusable
workflow can run this checkout-local command without inspecting GitHub event
payloads.

Usage (from the monorepo root):

    python3 conformance/r16/check.py
    python3 conformance/r16/check.py --self-test
"""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
DEFAULT_CHECKLIST = Path("conformance/r16/checklist.toml")

REQUIRED_REFERENCES = frozenset(
    {
        "ADR 0001 R4",
        "ADR 0003 R5",
        "ADR 0003 R6",
        "ADR 0004 R8",
        "ADR 0004 R9",
        "ADR 0007 R7",
        "ADR 0007 R8",
        "ADR 0007 R9",
    }
)
MARKER_RE = re.compile(r"<!--\s*r16:([a-z0-9-]+)\s*-->")
ID_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")


class ChecklistError(ValueError):
    pass


@dataclass(frozen=True)
class Question:
    id: str
    title: str
    requirements: tuple[str, ...]
    prompt: str


@dataclass(frozen=True)
class Checklist:
    template: str
    questions: tuple[Question, ...]


def _required_string(value: Any, field: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ChecklistError(f"{field} must be a non-empty string")
    return value


def load_checklist(path: Path) -> Checklist:
    try:
        doc = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise ChecklistError(f"cannot read {path}: {exc}") from exc

    if doc.get("version") != 1:
        raise ChecklistError(f"{path}: version must be 1")
    template = _required_string(doc.get("template"), "template")
    raw_questions = doc.get("question")
    if not isinstance(raw_questions, list) or not raw_questions:
        raise ChecklistError(f"{path}: at least one [[question]] is required")

    questions: list[Question] = []
    for index, raw in enumerate(raw_questions, start=1):
        if not isinstance(raw, dict):
            raise ChecklistError(f"question {index} must be a table")
        question_id = _required_string(raw.get("id"), f"question {index}.id")
        if not ID_RE.fullmatch(question_id):
            raise ChecklistError(f"question {index}.id is not a stable kebab-case id")
        requirements = raw.get("requirements")
        if (
            not isinstance(requirements, list)
            or not requirements
            or not all(isinstance(item, str) and item.strip() for item in requirements)
        ):
            raise ChecklistError(
                f"question {question_id}.requirements must be non-empty strings"
            )
        questions.append(
            Question(
                id=question_id,
                title=_required_string(raw.get("title"), f"question {question_id}.title"),
                requirements=tuple(requirements),
                prompt=_required_string(
                    raw.get("prompt"), f"question {question_id}.prompt"
                ),
            )
        )

    ids = [question.id for question in questions]
    if len(ids) != len(set(ids)):
        raise ChecklistError(f"{path}: duplicate question ids")
    prompts = [question.prompt for question in questions]
    if len(prompts) != len(set(prompts)):
        raise ChecklistError(f"{path}: duplicate question prompts")
    return Checklist(template=template, questions=tuple(questions))


def validate_coverage(
    checklist: Checklist, required: frozenset[str] = REQUIRED_REFERENCES
) -> list[str]:
    covered = {
        requirement
        for question in checklist.questions
        for requirement in question.requirements
    }
    errors: list[str] = []
    missing = sorted(required - covered)
    if missing:
        errors.append("checklist misses required references: " + ", ".join(missing))
    return errors


def validate_template(checklist: Checklist, text: str) -> list[str]:
    errors: list[str] = []
    expected_ids = [question.id for question in checklist.questions]
    marker_matches = list(MARKER_RE.finditer(text))
    marker_ids = [match.group(1) for match in marker_matches]

    if marker_ids != expected_ids:
        errors.append(
            "template marker order/set differs: "
            f"found {marker_ids!r}, expected {expected_ids!r}"
        )

    marker_by_id = {match.group(1): match for match in marker_matches}
    for question in checklist.questions:
        marker = marker_by_id.get(question.id)
        if marker is None:
            continue
        marker_index = marker_matches.index(marker)
        next_start = (
            marker_matches[marker_index + 1].start()
            if marker_index + 1 < len(marker_matches)
            else len(text)
        )
        section = text[marker.end() : next_start]
        count = text.count(question.prompt)
        if count != 1:
            errors.append(
                f"{question.id}: exact prompt occurs {count} times (expected once)"
            )
        elif question.prompt not in section:
            errors.append(f"{question.id}: prompt is outside its marked section")
        if not re.search(r"(?m)^Answer:\s*$", section):
            errors.append(f"{question.id}: marked section has no prose Answer field")
    return errors


def check(root: Path, checklist_path: Path) -> list[str]:
    checklist = load_checklist(checklist_path)
    errors = validate_coverage(checklist)
    template_path = root / checklist.template
    try:
        template = template_path.read_text(encoding="utf-8")
    except OSError as exc:
        return errors + [f"cannot read {template_path}: {exc}"]
    return errors + validate_template(checklist, template)


def run_self_test() -> int:
    sample = Checklist(
        template=".github/pull_request_template.md",
        questions=(
            Question("first", "First", ("A",), "Where is the policy?"),
            Question("second", "Second", ("B",), "Which fixture preserves it?"),
        ),
    )
    valid = """\
<!-- r16:first -->
Where is the policy?

Answer:

<!-- r16:second -->
Which fixture preserves it?

Answer:
"""
    assert validate_coverage(sample, frozenset({"A", "B"})) == []
    assert validate_template(sample, valid) == []
    assert validate_coverage(sample, frozenset({"A", "B", "C"}))
    assert validate_template(sample, valid.replace("Where is the policy?", "Why?"))
    assert validate_template(sample, valid + "\n<!-- r16:extra -->\nAnswer:\n")
    assert validate_template(sample, valid.replace("Answer:", "Response:", 1))

    with tempfile.TemporaryDirectory() as tmp:
        bad = Path(tmp) / "checklist.toml"
        bad.write_text(
            """\
version = 1
template = "template.md"
[[question]]
id = "same"
title = "One"
requirements = ["A"]
prompt = "One?"
[[question]]
id = "same"
title = "Two"
requirements = ["B"]
prompt = "Two?"
""",
            encoding="utf-8",
        )
        try:
            load_checklist(bad)
        except ChecklistError:
            pass
        else:
            raise AssertionError("duplicate ids were accepted")

    print("self-test: ok")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument(
        "--root",
        type=Path,
        default=REPO,
        help="monorepo root (default: two levels above this script)",
    )
    parser.add_argument(
        "--checklist",
        type=Path,
        default=None,
        help="checklist path (default: <root>/conformance/r16/checklist.toml)",
    )
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)

    if args.self_test:
        return run_self_test()

    root = args.root.resolve()
    checklist_path = (
        args.checklist.resolve()
        if args.checklist is not None
        else root / DEFAULT_CHECKLIST
    )
    try:
        errors = check(root, checklist_path)
    except ChecklistError as exc:
        print(f"R16 checklist: RED\n  {exc}")
        return 2

    if errors:
        print(f"R16 checklist: RED ({len(errors)} error(s))")
        for error in errors:
            print(f"  {error}")
        return 1
    print("R16 checklist: GREEN (machine-readable questions match PR template)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
