#!/usr/bin/env python3
"""Focused tests for the reverse-caller benchmark harness."""

from __future__ import annotations

import unittest

from reverse_caller import require_definitive_cancellation


class ReverseCallerHarnessTests(unittest.TestCase):
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
