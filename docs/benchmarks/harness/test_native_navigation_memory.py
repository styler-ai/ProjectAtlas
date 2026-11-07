"""Protect immutable-input admission, missing-process evidence and owned cleanup."""

import hashlib
import io
import json
import os
from pathlib import Path
import sys
import tarfile
import tempfile
import time
import unittest
from unittest.mock import patch

import native_navigation_memory as diagnosis


class OriginalInputs(unittest.TestCase):
    def setUp(self):
        self.run = {"id": 17, "event": "workflow_dispatch", "path": ".github/workflows/release.yml",
                    "head_repository": {"full_name": "owner/repo"}, "status": "completed", "head_sha": "a" * 40}
        self.report = {"schema_version": 1, "source_revision": "a" * 40, "workflow_run_id": "17",
                       "candidate_origin": "installed release workflow", "machine": "x86_64", "platform": "Linux-native",
                       "telemetry": "enabled", "runtimes": {name: {"version": "0.5.0"} for name in ("baseline", "candidate")},
                       "samples": [{"case": "detailed", "runtime": name, "output_bytes": 7119}
                                   for name in ("baseline", "candidate") for _ in range(9)]}

    def test_same_repository_completed_manual_native_input_and_failure_are_admitted(self):
        self.run["conclusion"] = "failure"
        diagnosis.original_report(self.run, self.report, "owner/repo", 17)

    def test_foreign_partial_wrong_revision_and_disabled_telemetry_are_refused(self):
        for key, value in (("event", "pull_request"), ("path", ".github/workflows/ci.yml"),
                           ("head_repository", {"full_name": "fork/repo"}), ("status", "in_progress"),
                           ("head_sha", "HEAD:untrusted")):
            with self.subTest(key=key):
                run = {**self.run, key: value}
                with self.assertRaises(ValueError):
                    diagnosis.original_report(run, self.report, "owner/repo", 17)
        for key, value in (("source_revision", "b" * 40), ("workflow_run_id", "18"),
                           ("telemetry", "disabled"), ("candidate_origin", "local draft"),
                           ("samples", self.report["samples"][:-1])):
            with self.subTest(key=key):
                report = {**self.report, key: value}
                with self.assertRaises(ValueError):
                    diagnosis.original_report(self.run, report, "owner/repo", 17)

    def test_digest_mismatch_is_not_executed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            file = root / "bytes"
            file.write_bytes(b"wrong")
            with self.assertRaises(ValueError):
                diagnosis.verify_file(file, hashlib.sha256(b"expected").hexdigest())

    @unittest.skipUnless(hasattr(tarfile, "data_filter"), "requires safe tar data filtering")
    def test_regular_package_is_preserved_and_indirect_runtime_is_refused(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for indirect in (False, True):
                package = root / (str(indirect) + ".tar.gz")
                with tarfile.open(package, "w:gz") as archive:
                    member = tarfile.TarInfo("projectatlas/projectatlas")
                    if indirect:
                        member.type = tarfile.SYMTYPE
                        member.linkname = "outside"
                        archive.addfile(member)
                    else:
                        member.size = 7
                        archive.addfile(member, io.BytesIO(b"runtime"))
                destination = root / ("runtime-" + str(indirect))
                if indirect:
                    with self.assertRaises(ValueError):
                        diagnosis.runtime_from_archive(package, destination, hashlib.sha256(b"runtime").hexdigest())
                    self.assertFalse(destination.exists())
                else:
                    self.assertEqual(diagnosis.runtime_from_archive(package, destination, hashlib.sha256(b"runtime").hexdigest()).read_bytes(), b"runtime")

    @unittest.skipUnless(sys.platform == "linux" and hasattr(tarfile, "data_filter"), "requires Linux safe tar links")
    def test_safe_packaged_alias_survives_and_escape_is_refused_before_extraction(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for escape in (False, True):
                package = root / (str(escape) + ".tar.gz")
                with tarfile.open(package, "w:gz") as archive:
                    executable = tarfile.TarInfo("projectatlas/projectatlas")
                    executable.size = 7
                    archive.addfile(executable, io.BytesIO(b"runtime"))
                    alias = tarfile.TarInfo("projectatlas/atlas")
                    alias.type = tarfile.SYMTYPE
                    alias.linkname = "../../outside" if escape else "projectatlas"
                    archive.addfile(alias)
                destination = root / ("package-" + str(escape))
                if escape:
                    with self.assertRaises(ValueError):
                        diagnosis.runtime_from_archive(package, destination, hashlib.sha256(b"runtime").hexdigest())
                    self.assertFalse(destination.exists())
                else:
                    diagnosis.runtime_from_archive(package, destination, hashlib.sha256(b"runtime").hexdigest())
                    self.assertEqual((destination / "projectatlas/atlas").read_bytes(), b"runtime")


class MemoryEvidence(unittest.TestCase):
    def test_units_are_exact_and_zombie_status_is_unavailable(self):
        self.assertIsNone(diagnosis.memory_status("Threads:\t1\n"))
        self.assertIsNone(diagnosis.memory_status("VmRSS: 0 kB\nThreads: 1\n"))
        self.assertEqual(diagnosis.memory_status("VmRSS: 23 kB\nVmHWM: 31 kB\nThreads: 4\n"),
                         {"VmRSS": 23 * 1024, "VmHWM": 31 * 1024, "Threads": 4})

    def test_empty_mapping_or_rollup_does_not_create_zero_evidence(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            status = "VmRSS: 4 kB\nThreads: 1\n"
            smaps = "1000-2000 r-xp 00000000 00:00 1 /runtime\nRss: 4 kB\n"
            for values, reason in ((("Threads: 1\n", smaps), "resident status unavailable"),
                                   ((status, ""), "mapping data unavailable"),
                                   ((status, smaps, ""), "rollup data unavailable")):
                with patch.object(Path, "read_text", side_effect=values):
                    result = diagnosis.snapshot(17, Path("/runtime"), root, "exited")
                self.assertFalse(result["available"])
                self.assertEqual(result["reason"], reason)
                self.assertEqual(list(root.iterdir()), [])

    @unittest.skipUnless(sys.platform == "linux", "requires Linux owned process cleanup")
    def test_timeout_preserves_collected_evidence_and_bounded_partial_streams(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            program = "import sys,time;print('partial-output',flush=True);print('partial-error',file=sys.stderr,flush=True);time.sleep(60)"
            with patch.object(diagnosis, "state", return_value={"generation": 1}):
                for hold in (False, True):
                    destination = root / str(hold)
                    with self.assertRaises(TimeoutError):
                        diagnosis.observe([sys.executable, "-c", program], root, os.environ.copy(), destination, hold, time.monotonic() + .25)
                    result = json.loads((destination / "observation.json").read_text())
                    self.assertEqual(result["state"], "failed")
                    self.assertIn("TimeoutError", result["error"])
                    self.assertEqual((destination / "stdout").read_bytes(), b"partial-output\n")
                    self.assertEqual((destination / "stderr").read_bytes(), b"partial-error\n")
                    self.assertFalse(result.get("state_preserved", False))
                    if not hold:
                        self.assertTrue(result["timeline"])
                        self.assertTrue(result["snapshots"])
                for name, program in (("invalid", "print('not JSON')"),
                                      ("overflow", "import sys;sys.stdout.write('x'*200000);sys.stdout.flush()")):
                    destination = root / name
                    with self.assertRaises(ValueError):
                        diagnosis.observe([sys.executable, "-c", program], root, os.environ.copy(), destination, False, time.monotonic() + 5)
                    result = json.loads((destination / "observation.json").read_text())
                    self.assertEqual(result["state"], "failed")
                    self.assertLessEqual((destination / "stdout").stat().st_size, 65536)
                    self.assertEqual(result["stdout_truncated"], name == "overflow")

    @unittest.skipUnless(sys.platform == "linux", "requires Linux pipe and /proc semantics")
    def test_real_pipe_hold_preserves_complete_output_and_timeout_reaps_child(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            payload = {"symbol_relations": {"returned": 1}, "name": "item_0_0", "padding": "x" * 6000}
            program = "import sys,time;sys.stdout.write(" + repr(json.dumps(payload)) + ");sys.stdout.flush();time.sleep(.03)"
            with patch.object(diagnosis, "state", return_value={"generation": 1}):
                result = diagnosis.observe([sys.executable, "-c", program], root, os.environ.copy(), root / "hold", True, time.monotonic() + 5)
                self.assertTrue(result["state_preserved"])
                self.assertTrue(result["snapshots"][0]["available"])
                self.assertEqual(json.loads((root / "hold/stdout").read_bytes()), payload)
                children = []
                spawn = diagnosis.spawn_owned_process
                def owned(*args, **kwargs):
                    process, job = spawn(*args, **kwargs)
                    children.append(process)
                    return process, job
                with patch.object(diagnosis, "spawn_owned_process", side_effect=owned):
                    with self.assertRaises(TimeoutError):
                        diagnosis.observe([sys.executable, "-c", "import time;time.sleep(60)"], root, os.environ.copy(), root / "timeout", False, time.monotonic() + .1)
                self.assertIsNotNone(children[0].poll())


if __name__ == "__main__":
    unittest.main()
