"""Protect acceptance arithmetic and fail-closed completeness without a campaign."""

from pathlib import Path
import tempfile
import unittest

from indexing_acceptance import Campaign


class AcceptanceTargets(unittest.TestCase):
    def campaign(self, root: Path, candidate_ratio: float) -> Campaign:
        runtime = root / "runtime"
        runtime.write_bytes(b"measurement arithmetic fixture")
        campaign = Campaign(root / "result.json", runtime, runtime, "0.5.1-rc1")
        cases = {"initial": 3, "full": 5, "rebuild": 3, "cold_noop": 6,
                 "sparse": 3, "summary": 9, "detailed": 9, "compatible_open": 9,
                 "large_sparse": 3, "dense_sparse": 3, "semantic_upgrade": 3}
        for case, count in cases.items():
            for runtime in campaign.runtimes:
                for _ in range(count):
                    baseline = .04 if case == "cold_noop" else 1.0
                    ratio = candidate_ratio if runtime == "candidate" else 1.0
                    campaign.result["samples"].append({
                        "runtime": runtime, "case": case,
                        "wall_seconds": baseline * ratio,
                        "sampled_peak_rss_bytes": 100,
                        "source_bytes_per_second": 1000 / ratio,
                    })
        campaign.result["warm_mcp"] = [
            {"runtime": runtime, "wall_seconds": .04 * (candidate_ratio if runtime == "candidate" else 1)}
            for runtime in campaign.runtimes for _ in range(9)
        ]
        return campaign

    def test_equal_runtimes_cannot_pass_required_speedups(self):
        with tempfile.TemporaryDirectory() as task:
            campaign = self.campaign(Path(task), 1.0)
            with self.assertRaisesRegex(AssertionError, "initial latency target"):
                campaign.evaluate()
            self.assertFalse(campaign.result["passed"])
            self.assertIn("compatible_open", campaign.result["summary"])
            self.assertTrue(campaign.output.is_file())

    def test_complete_improved_samples_pass_and_missing_samples_refuse(self):
        with tempfile.TemporaryDirectory() as task:
            campaign = self.campaign(Path(task), .4)
            campaign.evaluate()
            self.assertTrue(campaign.result["passed"])
            campaign.result["samples"].pop()
            with self.assertRaisesRegex(AssertionError, "exact sample count"):
                campaign.evaluate()
            self.assertFalse(campaign.result["passed"])

    def test_full_work_refuses_reuse_and_missing_parse(self):
        with tempfile.TemporaryDirectory() as task:
            campaign = self.campaign(Path(task), .4)
            for report in ({"parsed": 0, "unchanged": 9}, {"parsed": 8, "unchanged": 0}):
                with self.assertRaisesRegex(AssertionError, "not a cached no-op"):
                    campaign.full_work(report, 8, "full")
            complete = {"parsed": 9, "unchanged": 0, "symbols": 97, "relations": 96,
                        "too_large": 0, "binary_or_non_utf8": 0, "timed_out": 0}
            campaign.full_work({"symbols": complete}, 8, "full")
            campaign.full_work(complete, 8, "rebuild")
            with self.assertRaisesRegex(AssertionError, "without parser omissions"):
                campaign.full_work({**complete, "symbols": 96}, 8, "full")

    def test_warm_regression_and_missing_samples_cannot_hide_behind_full_speedups(self):
        with tempfile.TemporaryDirectory() as task:
            campaign = self.campaign(Path(task), .4)
            for sample in campaign.result["warm_mcp"]:
                if sample["runtime"] == "candidate":
                    sample["wall_seconds"] = .08
            with self.assertRaisesRegex(AssertionError, "warm MCP exact reconciliation latency target"):
                campaign.evaluate()
            self.assertFalse(campaign.result["passed"])
            campaign.result["warm_mcp"].pop()
            with self.assertRaisesRegex(AssertionError, "warm MCP exact sample count"):
                campaign.evaluate()


if __name__ == "__main__":
    unittest.main()
