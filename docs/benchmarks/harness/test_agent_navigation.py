#!/usr/bin/env python3
"""Focused tests for the repeated Codex source-navigation harness."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from agent_navigation import (
    aggregate_runs,
    append_checkpoint,
    build_command,
    parse_trace,
    projectatlas_mcp_contract,
    schedule,
    write_result,
)


MARKER = "BENCHMARK_SELF_AUDIT:"


def audit() -> dict[str, object]:
    return {
        "productive": {
            "folders": ["src"],
            "files": ["src/lib.rs"],
            "relations": ["lib::entry -> service::run"],
        },
        "wrong": {"folders": [], "files": [], "relations": []},
        "backtracks": 1,
        "broad_reads": 0,
        "full_reads": 1,
    }


class AgentNavigationHarnessTests(unittest.TestCase):
    def test_parse_trace_captures_usage_tools_arguments_and_audit(self) -> None:
        events = [
            {"type": "thread.started", "thread_id": "thread"},
            {
                "type": "item.started",
                "item": {
                    "id": "command",
                    "type": "command_execution",
                    "command": "rg --files",
                    "status": "in_progress",
                },
            },
            {
                "type": "item.completed",
                "item": {
                    "id": "command",
                    "type": "command_execution",
                    "command": "rg --files",
                    "status": "completed",
                    "aggregated_output": "src/lib.rs\n",
                },
            },
            {
                "type": "item.completed",
                "item": {
                    "id": "mcp",
                    "type": "mcp_tool_call",
                    "server": "projectatlas",
                    "tool": "atlas_file_summary",
                    "arguments": {"file": "src/lib.rs"},
                    "status": "completed",
                    "result": {"content": "summary"},
                },
            },
            {
                "type": "item.completed",
                "item": {
                    "id": "answer",
                    "type": "agent_message",
                    "text": (
                        f"The entry point is src/lib.rs.\n{MARKER}{json.dumps(audit())}"
                    ),
                },
            },
            {
                "type": "turn.completed",
                "usage": {
                    "input_tokens": 120,
                    "cached_input_tokens": 20,
                    "output_tokens": 30,
                    "reasoning_output_tokens": 10,
                },
            },
        ]
        parsed = parse_trace(
            "\n".join(json.dumps(event) for event in events) + "\n", MARKER
        )
        self.assertEqual(
            parsed["tool_calls_by_type"],
            {"command_execution": 1, "mcp_tool_call": 1},
        )
        self.assertEqual(parsed["mcp_calls"][0]["arguments"], {"file": "src/lib.rs"})
        self.assertEqual(parsed["provider_usage"]["cached_input_tokens"], 20)
        self.assertEqual(parsed["self_audit"]["backtracks"], 1)
        self.assertGreater(parsed["tool_emitted_bytes"], 0)
        self.assertEqual(parsed["invalid_lines"], [])
        self.assertTrue(projectatlas_mcp_contract(parsed, "v0.4")["passed"])
        self.assertFalse(projectatlas_mcp_contract(parsed, "plain")["passed"])

    def test_projectatlas_arm_rejects_only_failed_mcp_calls(self) -> None:
        trace = {
            "mcp_calls": [
                {
                    "server": "projectatlas",
                    "tool": "atlas_session_brief",
                    "status": "failed",
                    "error": {"message": "user cancelled MCP tool call"},
                }
            ]
        }
        contract = projectatlas_mcp_contract(trace, "v0.4")
        self.assertFalse(contract["passed"])
        self.assertEqual(contract["successful_calls"], 0)

    def test_parse_trace_retains_invalid_jsonl(self) -> None:
        parsed = parse_trace('{"type":"turn.started"}\nnot-json\n', MARKER)
        self.assertEqual(parsed["event_count"], 1)
        self.assertEqual(parsed["invalid_lines"][0]["line"], 2)
        self.assertIsNone(parsed["self_audit"])

    def test_aggregate_retains_failed_run_and_absolute_values(self) -> None:
        completed = {
            "run_id": "r01-medium-v0.4",
            "case": "medium",
            "arm": "v0.4",
            "execution_status": "completed",
            "excluded": False,
            "measurement": {
                "wall_seconds": 2.0,
                "cpu_seconds": 1.0,
                "peak_rss_bytes": 10,
                "process_read_transfer_bytes": 20,
                "process_write_transfer_bytes": 30,
            },
            "navigation_context": {
                "gross_navigation_bytes": 40,
                "net_navigation_bytes": 50,
                "gross_navigation_tokens": 10,
                "net_navigation_tokens": 13,
            },
            "trace": {
                "provider_usage": {"input_tokens": 100},
                "tool_calls_by_type": {"command_execution": 2},
            },
            "correctness": {"passed": True},
        }
        failed = {
            "run_id": "r02-medium-v0.4",
            "case": "medium",
            "arm": "v0.4",
            "execution_status": "failed",
            "excluded": False,
            "failure": {"type": "TimeoutError", "message": "bounded timeout"},
        }
        result = aggregate_runs([completed, failed])
        group = result["groups"]["medium/v0.4"]
        self.assertEqual(result["all_run_ids"], [completed["run_id"], failed["run_id"]])
        self.assertEqual(group["scheduled"], 2)
        self.assertEqual(group["failed"], 1)
        self.assertEqual(group["distributions"]["wall_seconds"]["values"], [2.0])
        self.assertEqual(group["distributions"]["wall_seconds"]["maximum"], 2.0)
        self.assertEqual(
            group["distributions"]["wall_seconds"]["observed_tail"], "maximum"
        )
        self.assertEqual(group["tool_calls_by_type"], {"command_execution": 2})

    def test_aggregate_reports_baseline_savings_without_causal_token_claim(
        self,
    ) -> None:
        def row(arm: str, wall: float, tokens: int) -> dict[str, object]:
            return {
                "run_id": f"r01-medium-{arm}",
                "case": "medium",
                "arm": arm,
                "execution_status": "completed",
                "excluded": False,
                "measurement": {"wall_seconds": wall},
                "navigation_context": {
                    "gross_navigation_bytes": tokens * 4,
                    "net_navigation_bytes": tokens * 4,
                    "gross_navigation_tokens": tokens,
                    "net_navigation_tokens": tokens,
                },
                "trace": {
                    "provider_usage": {"input_tokens": tokens},
                    "tool_calls_by_type": {},
                },
                "correctness": {"passed": True},
            }

        result = aggregate_runs([row("v0.4", 1.0, 50), row("v0.3.26", 2.0, 100)])
        comparison = result["comparisons"]["medium/v0.4-vs-v0.3.26"]
        self.assertEqual(
            comparison["lower_is_better_percent_savings"]["wall_seconds"][
                "median_percent_saving"
            ],
            50.0,
        )
        self.assertFalse(
            comparison["provider_usage_descriptive_only"]["input_tokens"][
                "causal_attribution"
            ]
        )

    def test_command_construction_is_equal_except_projectatlas_capability(self) -> None:
        candidate = {
            "codex": {
                "executable": "${PROJECTATLAS_TEST_EXECUTABLE}",
                "model": "gpt-test",
                "reasoning_effort": "high",
                "sandbox": "read-only",
                "approval_policy": "never",
                "mcp_approval": {
                    "default_mode": "prompt",
                    "read_only_tools": ["atlas_session_brief"],
                },
                "config": {"web_search": '"disabled"'},
            },
            "arms": {
                "v0.4": {
                    "runtime": "${PROJECTATLAS_TEST_EXECUTABLE}",
                    "skill_path": str(Path(__file__).resolve()),
                    "mcp_args": [
                        "--db",
                        "{db}",
                        "--config",
                        "{config}",
                        "mcp",
                    ],
                    "mcp_env": {"PROJECTATLAS_NO_TELEMETRY": "1"},
                },
                "plain": {},
            },
        }
        fixture = Path("C:/fixture")
        executable = str(Path(__file__).resolve())
        with patch.dict("os.environ", {"PROJECTATLAS_TEST_EXECUTABLE": executable}):
            projectatlas, projectatlas_prompt = build_command(
                candidate, "v0.4", fixture, "Find the implementation."
            )
            plain, plain_prompt = build_command(
                candidate, "plain", fixture, "Find the implementation."
            )
        self.assertEqual(projectatlas[0], executable)
        self.assertEqual(plain[0], executable)
        for flag in (
            "--json",
            "--ephemeral",
            "--ignore-user-config",
            "--ignore-rules",
            "--skip-git-repo-check",
            "--sandbox",
            "read-only",
            "--model",
            "gpt-test",
        ):
            self.assertIn(flag, projectatlas)
            self.assertIn(flag, plain)
        self.assertIn("mcp_servers.projectatlas.required=true", projectatlas)
        self.assertIn(
            'mcp_servers.projectatlas.default_tools_approval_mode="prompt"',
            projectatlas,
        )
        self.assertIn(
            "mcp_servers.projectatlas.tools.atlas_session_brief."
            'approval_mode="approve"',
            projectatlas,
        )
        self.assertFalse(
            any("tools.atlas_purpose_set" in value for value in projectatlas)
        )
        self.assertFalse(any("mcp_servers.projectatlas" in value for value in plain))
        self.assertIn(
            str(Path(candidate["arms"]["v0.4"]["skill_path"]).resolve()),
            projectatlas_prompt,
        )
        self.assertTrue(plain_prompt.startswith("Find the implementation."))
        self.assertIn("Control arm:", plain_prompt)
        self.assertIn("must not be invoked", plain_prompt)

    def test_schedule_rotates_arm_order_without_dropping_trials(self) -> None:
        rows = schedule(3)
        self.assertEqual(len(rows), 45)
        first_arm_by_repeat = [
            next(row["arm"] for row in rows if row["repeat"] == repeat)
            for repeat in (1, 2, 3)
        ]
        self.assertEqual(first_arm_by_repeat, ["v0.4", "v0.3.26", "plain"])
        self.assertEqual(len({row["run_id"] for row in rows}), len(rows))

    def test_result_write_is_atomic_and_checkpoint_is_append_only(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "result.json"
            journal = root / "result.json.journal.jsonl"
            append_checkpoint({"run_id": "one"}, journal)
            append_checkpoint(
                {"run_id": "two", "path": str(Path.home() / "private")},
                journal,
            )
            write_result(
                {"complete": True, "path": str(Path.home() / "private")},
                output,
            )
            self.assertEqual(
                [
                    json.loads(line)["run_id"]
                    for line in journal.read_text().splitlines()
                ],
                ["one", "two"],
            )
            saved = json.loads(output.read_text())
            expected_private_path = str(Path("{USER_HOME}") / "private")
            self.assertEqual(saved["path"], expected_private_path)
            self.assertEqual(
                json.loads(journal.read_text().splitlines()[1])["path"],
                expected_private_path,
            )
            self.assertFalse((root / "result.json.tmp").exists())


if __name__ == "__main__":
    unittest.main()
