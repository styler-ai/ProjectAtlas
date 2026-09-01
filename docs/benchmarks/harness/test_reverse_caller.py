#!/usr/bin/env python3
"""Focused tests for the reverse-caller benchmark harness."""

from __future__ import annotations

import unittest

from reverse_caller import (
    require_clean_process_exit,
    require_definitive_cancellation,
    require_successful_ping,
)


class ReverseCallerHarnessTests(unittest.TestCase):
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
