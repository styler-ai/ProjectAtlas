"""Protect acceptance arithmetic and fail-closed completeness without a campaign."""

from contextlib import contextmanager
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
import zipfile

from indexing_acceptance import Campaign, DENSE_MODULES, archive_runtime_digest, digest, _stream_digest


@contextmanager
def legacy_hashlib():
    """Exercise native-host compatibility even when this test runs on newer Python."""
    missing = object()
    original = getattr(hashlib, "file_digest", missing)
    if original is not missing:
        del hashlib.file_digest
    try:
        yield
    finally:
        if original is not missing:
            hashlib.file_digest = original


class ArchiveIdentity(unittest.TestCase):
    def release_helper(self, name):
        path = Path(__file__).resolve().parents[3] / ".github/scripts" / (name + ".py")
        spec = importlib.util.spec_from_file_location(name.replace("-", "_"), path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def test_file_hashes_preserve_complete_bytes_on_legacy_python(self):
        functions = [digest] + [self.release_helper(name).file_sha256 for name in (
            "prepare-indexing-baseline", "prepare-published-predecessor")]
        payload = bytes(range(256)) * 9000 + b"final archive bytes"
        with tempfile.TemporaryDirectory() as task, legacy_hashlib():
            path = Path(task) / "archive"
            for contents in (b"", b"abc", payload, payload[:-1] + b"!"):
                path.write_bytes(contents)
                expected = hashlib.sha256(contents).hexdigest()
                for function in functions:
                    with self.subTest(function=function.__module__, length=len(contents)):
                        self.assertEqual(function(path), expected)
            self.assertNotEqual(hashlib.sha256(payload).hexdigest(), digest(path))

    def test_zip_and_tar_runtime_identity_preserves_full_member_bytes(self):
        payload = bytes(range(256)) * 9000 + b"native runtime tail"
        with tempfile.TemporaryDirectory() as task, legacy_hashlib():
            root = Path(task)
            archive = root / "runtime.zip"
            with zipfile.ZipFile(archive, "w") as package:
                package.writestr("projectatlas.exe", payload)
            self.assertEqual(archive_runtime_digest(archive), hashlib.sha256(payload).hexdigest())
            archive = root / "runtime.tar.gz"
            with tarfile.open(archive, "w:gz") as package:
                member = tarfile.TarInfo("projectatlas/projectatlas")
                member.size = len(payload)
                package.addfile(member, io.BytesIO(payload))
            self.assertEqual(archive_runtime_digest(archive), hashlib.sha256(payload).hexdigest())

    def test_tar_link_cannot_stand_in_for_a_regular_runtime(self):
        with tempfile.TemporaryDirectory() as task, legacy_hashlib():
            archive = Path(task) / "runtime.tar.gz"
            with tarfile.open(archive, "w:gz") as package:
                member = tarfile.TarInfo("projectatlas/projectatlas")
                member.type = tarfile.SYMTYPE
                member.linkname = "other-runtime"
                package.addfile(member)
            with self.assertRaisesRegex(ValueError, "not a regular file"):
                archive_runtime_digest(archive)

    def test_stream_hash_handles_short_reads_without_unbounded_allocation(self):
        class ShortReads(io.BytesIO):
            def read(self, size=-1):
                if not 0 < size <= 1024 * 1024:
                    raise AssertionError("hashing must request bounded binary chunks")
                return super().read(min(size, 113))

        payload = bytes(range(256)) * 9000
        with ShortReads(payload) as source, legacy_hashlib():
            self.assertEqual(_stream_digest(source), hashlib.sha256(payload).hexdigest())


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

    def test_sparse_work_refuses_missing_duplicate_or_hidden_dense_recovery(self):
        with tempfile.TemporaryDirectory() as task:
            campaign = self.campaign(Path(task), .4)
            complete = {"candidates": DENSE_MODULES, "parsed": DENSE_MODULES,
                        "unchanged": 0, "symbols": DENSE_MODULES,
                        "relations": DENSE_MODULES - 1}
            campaign.sparse_work("candidate", "dense", {"last_symbols": complete}, 0)
            baseline = {**complete, "parsed": 1, "unchanged": DENSE_MODULES - 1}
            campaign.sparse_work("baseline", "dense", {"last_symbols": baseline}, 0)
            for parsed in (1, DENSE_MODULES - 1, DENSE_MODULES + 1):
                with self.assertRaisesRegex(AssertionError, "exact source parse/recovery work"):
                    campaign.sparse_work("candidate", "dense",
                                         {"last_symbols": {**complete, "parsed": parsed}}, 0)
            with self.assertRaisesRegex(AssertionError, "changed raw-graph output"):
                campaign.sparse_work("candidate", "dense",
                                     {"last_symbols": {**complete, "symbols": DENSE_MODULES + 1}}, 0)
            with self.assertRaisesRegex(AssertionError, "exact source parse/recovery work"):
                campaign.sparse_work("candidate", "large", {"last_symbols": {"parsed": 2}}, 0)

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
