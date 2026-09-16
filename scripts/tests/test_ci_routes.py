#!/usr/bin/env python3

from __future__ import annotations

import json
import unittest
from pathlib import Path
from unittest.mock import patch

from scripts.ci_routes import route_paths, routes_for_event

CASES = Path(__file__).with_name("ci_routes_cases.json")


class RouteFixtureTests(unittest.TestCase):
    def test_fixtures(self) -> None:
        cases = json.loads(CASES.read_text(encoding="utf-8"))
        for case in cases:
            with self.subTest(case=case["name"]):
                if case.get("event") == "initial-push":
                    routes, paths, reason = routes_for_event(
                        "push",
                        {"before": "0" * 40, "after": "1" * 40},
                        "",
                    )
                    self.assertEqual(paths, [])
                    self.assertEqual(reason, "initial-push")
                else:
                    routes = route_paths(case["paths"])
                self.assertEqual(routes.as_dict(), case["expected"])

    def test_mixed_docs_and_code_uses_code_route(self) -> None:
        routes = route_paths(
            ["docs/README.md", "apps/health/internal/log/store.go"]
        )
        self.assertFalse(routes.rust)
        self.assertTrue(routes.apps)
        self.assertFalse(routes.bindings)
        self.assertTrue(routes.conformance)
        self.assertFalse(routes.docker)

    def test_empty_change_set_selects_no_suite(self) -> None:
        self.assertEqual(
            route_paths([]).as_dict(),
            {check: False for check in route_paths([]).as_dict()},
        )

    @patch(
        "scripts.ci_routes._changed_paths",
        return_value=["apps/music/LocalMusic/LibraryView.swift"],
    )
    def test_pull_request_compares_base_to_head(self, changed_paths) -> None:
        routes, paths, reason = routes_for_event(
            "pull_request",
            {
                "pull_request": {
                    "base": {"sha": "a" * 40},
                    "head": {"sha": "b" * 40},
                }
            },
            "c" * 40,
        )
        changed_paths.assert_called_once_with("a" * 40, "b" * 40)
        self.assertEqual(paths, ["apps/music/LocalMusic/LibraryView.swift"])
        self.assertEqual(reason, "pull-request-base")
        self.assertTrue(routes.apps)

    @patch(
        "scripts.ci_routes._changed_paths",
        return_value=["core/localcore-vfs/src/lib.rs"],
    )
    def test_push_compares_before_to_after(self, changed_paths) -> None:
        routes, paths, reason = routes_for_event(
            "push",
            {"before": "a" * 40, "after": "b" * 40},
            "c" * 40,
        )
        changed_paths.assert_called_once_with("a" * 40, "b" * 40)
        self.assertEqual(paths, ["core/localcore-vfs/src/lib.rs"])
        self.assertEqual(reason, "push-before")
        self.assertTrue(routes.rust)
        self.assertTrue(routes.apps)

    @patch("scripts.ci_routes._changed_paths", return_value=None)
    def test_unavailable_comparison_is_conservative(self, _changed_paths) -> None:
        routes, paths, reason = routes_for_event(
            "push",
            {"before": "a" * 40, "after": "b" * 40},
            "",
        )
        self.assertEqual(routes.as_dict(), route_paths(["unknown"]).as_dict())
        self.assertEqual(paths, [])
        self.assertEqual(reason, "push-comparison-unavailable")


if __name__ == "__main__":
    unittest.main()
