#!/usr/bin/env python3
"""Focused tests for the reverse-caller benchmark harness."""

from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import reverse_caller
from reverse_caller import (
    ROOT,
    compact_process_evidence,
    compact_query_plan_evidence,
    compact_review_evidence,
    process_environment,
    require_clean_process_exit,
    require_definitive_cancellation,
    require_successful_ping,
    run_summary,
    run_summary_evidence,
)


class ReverseCallerHarnessTests(unittest.TestCase):
    def test_timed_environment_cannot_enable_trace_observer(self) -> None:
        with patch.dict(
            os.environ,
            {
                "PROJECTATLAS_REVERSE_CALLER_TRACE": "ambient-trace.json",
                "PROJECTATLAS_REVERSE_CALLER_ALLOCATIONS": "ambient-allocations.json",
            },
            clear=False,
        ):
            timed = process_environment()
            replay = process_environment(trace_path=Path("replay-trace.json"))

        self.assertNotIn("PROJECTATLAS_REVERSE_CALLER_TRACE", timed)
        self.assertNotIn("PROJECTATLAS_REVERSE_CALLER_ALLOCATIONS", timed)
        self.assertEqual(
            replay["PROJECTATLAS_REVERSE_CALLER_TRACE"],
            "replay-trace.json",
        )
        self.assertNotIn("PROJECTATLAS_REVERSE_CALLER_ALLOCATIONS", replay)

    def test_evidence_replay_is_marked_untimed_and_separate(self) -> None:
        process = {
            "returncode": 0,
            "wall_ms": 1.0,
            "cpu_ms": 1.0,
            "peak_rss_bytes": 1,
            "stdout_bytes": 0,
            "stdout_sha256": "a" * 64,
            "stderr_bytes": 0,
            "stderr_sha256": "b" * 64,
            "allocation_metrics": {"allocation_calls": 1},
            "trace_observed": False,
            "timed": True,
        }
        compact_process = compact_process_evidence(process)
        compact_plan = compact_query_plan_evidence(
            {
                "events": [],
                "by_family": {},
                "engine": {"sqlite_version": "3"},
                "plan_provenance": "projectatlas-db::AtlasStore::connection",
                "timed": False,
            }
        )

        self.assertTrue(compact_process["timed"])
        self.assertFalse(compact_process["trace_observed"])
        self.assertFalse(compact_plan["timed"])
        self.assertEqual(
            compact_plan["plan_provenance"],
            "projectatlas-db::AtlasStore::connection",
        )

    def test_timed_and_evidence_invocations_have_disjoint_observers(self) -> None:
        target = {
            "path": "src/empty.rs",
            "language": "rust",
            "caller_suffix": ".rs",
            "symbol_count": 0,
            "caller_count": 0,
        }
        summary = json.dumps({"file_path": target["path"]}).encode()
        calls: list[dict[str, object]] = []

        def fake_process(
            command: list[str],
            cwd: Path,
            *,
            trace_path: Path | None = None,
            allocation_path: Path | None = None,
        ) -> dict[str, object]:
            del command, cwd
            calls.append(
                {"trace_path": trace_path, "allocation_path": allocation_path}
            )
            if allocation_path is not None:
                allocation_path.write_text(
                    json.dumps({"allocation_calls": 0, "allocation_bytes": 0}),
                    encoding="utf-8",
                )
            if trace_path is not None:
                trace_path.write_text(
                    json.dumps(
                        {
                            "queries": [],
                            "engine": {"sqlite_version": "3"},
                            "plan_provenance": (
                                "projectatlas-db::AtlasStore::connection"
                            ),
                        }
                    ),
                    encoding="utf-8",
                )
            return {
                "returncode": 0,
                "wall_ms": 1.0,
                "cpu_ms": 1.0,
                "peak_rss_bytes": 1,
                "stdout": summary,
                "stderr": b"",
            }

        with tempfile.TemporaryDirectory(dir=ROOT / ".tmp") as directory:
            root = Path(directory)
            with patch.object(reverse_caller, "run_process", side_effect=fake_process):
                timed = run_summary(
                    Path("projectatlas"), root, target, "small", 1, "baseline", 0
                )
                evidence = run_summary_evidence(
                    Path("projectatlas"),
                    root,
                    target,
                    "small",
                    1,
                    "baseline",
                    0,
                    {"file_path": target["path"]},
                )

        self.assertIsNone(calls[0]["trace_path"])
        self.assertIsNotNone(calls[0]["allocation_path"])
        self.assertIsNotNone(calls[1]["trace_path"])
        self.assertIsNone(calls[1]["allocation_path"])
        self.assertTrue(timed["timed"])
        self.assertFalse(timed["trace_observed"])
        self.assertFalse(evidence["timed"])

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
            "schema": "projectatlas.reverse-caller-performance.v6",
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
