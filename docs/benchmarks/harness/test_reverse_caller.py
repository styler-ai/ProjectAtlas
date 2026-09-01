#!/usr/bin/env python3
"""Focused tests for the reverse-caller benchmark harness."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from reverse_caller import (
    ROOT,
    compact_review_evidence,
    require_clean_process_exit,
    require_definitive_cancellation,
    require_successful_ping,
)


class ReverseCallerHarnessTests(unittest.TestCase):
    def test_compact_evidence_drops_raw_process_streams(self) -> None:
        process = {
            "returncode": 0,
            "wall_ms": 1.0,
            "cpu_ms": 1.0,
            "peak_rss_bytes": 1,
            "stdout_bytes": 3,
            "stdout_sha256": "a" * 64,
            "stdout_base64": "cmF3",
            "stdout_text": "raw",
            "stderr_bytes": 0,
            "stderr_sha256": "b" * 64,
            "stderr_base64": "",
            "stderr_text": "",
        }
        fixture = {
            "targets": [],
            "aggregate": {"wall_ms": 1.0},
            "runs": [process],
            "query_observations": {"events": [], "by_family": {}},
        }
        result = {
            "schema": "projectatlas.reverse-caller-performance.v5",
            "issue": 342,
            "repository_revision": "c" * 40,
            "repeats": 1,
            "baseline": {"binary_sha256": "d" * 64, "fixtures": {"small": fixture}},
            "candidate": {
                "binary_sha256": "e" * 64,
                "candidate_patch": {"path": "patch", "sha256": "f" * 64},
                "fixtures": {"small": fixture},
            },
            "candidate_patch_preflight": {"status": "passed"},
            "failure_cases": {},
            "multi_binding_alias": {"baseline": {}, "candidate": {}},
            "cancellation": {"baseline": {}, "candidate": {}},
            "decision": {"semantic_findings": []},
        }
        with tempfile.TemporaryDirectory(dir=ROOT / ".tmp") as directory:
            raw = Path(directory) / "raw.json"
            raw.write_bytes(b"raw")
            compact = compact_review_evidence(result, raw)

        serialized = json.dumps(compact)
        self.assertEqual(compact["artifact_kind"], "compact-review-evidence")
        self.assertEqual(compact["reproduction"]["raw_trace_bytes"], 3)
        self.assertNotIn("stdout_base64", serialized)
        self.assertNotIn("stderr_base64", serialized)
        self.assertNotIn("stdout_text", serialized)
        self.assertNotIn("stderr_text", serialized)

    def test_ping_requires_successful_json_rpc_result(self) -> None:
        require_successful_ping(
            {"jsonrpc": "2.0", "id": 3, "result": {}}
        )
        with self.assertRaises(AssertionError):
            require_successful_ping(
                {"jsonrpc": "2.0", "id": 3, "error": {"code": -1}}
            )

    def test_mcp_process_requires_clean_exit(self) -> None:
        require_clean_process_exit(0)
        with self.assertRaises(AssertionError):
            require_clean_process_exit(1)

    def test_cancellation_requires_authoritative_operation_result(self) -> None:
        base = {
            "request_cancellation_observed": True,
            "work_cancellation_observed": True,
        }
        with self.subTest("bridge-drop diagnostic"):
            with self.assertRaises(AssertionError):
                require_definitive_cancellation(
                    {
                        **base,
                        "outcome": "request-context-canceled",
                        "result_was_canceled": None,
                    }
                )
        with self.subTest("missing result flag"):
            with self.assertRaises(AssertionError):
                require_definitive_cancellation(
                    {**base, "outcome": "canceled", "result_was_canceled": None}
                )
        with self.subTest("authoritative cancellation"):
            require_definitive_cancellation(
                {**base, "outcome": "canceled", "result_was_canceled": True}
            )


if __name__ == "__main__":
    unittest.main()
