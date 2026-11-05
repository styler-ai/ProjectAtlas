"""Protect acceptance arithmetic and fail-closed completeness without a campaign."""

from contextlib import ExitStack, closing, contextmanager
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sqlite3
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

from indexing_acceptance import (
    Campaign, DENSE_MODULES, SOURCE_FILE, archive_runtime_digest, copy_database,
    database, database_reader, digest, state, _stream_digest,
)


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


class DatabaseInspection(unittest.TestCase):
    def project(self, root):
        database(root).parent.mkdir()
        connection = sqlite3.connect(database(root))
        try:
            connection.executescript("""
                CREATE TABLE project_identity(project_instance_id BLOB, active_generation INTEGER);
                INSERT INTO project_identity VALUES(zeroblob(16),1);
                CREATE TABLE metadata(key TEXT,value TEXT);
                INSERT INTO metadata VALUES('index_publication_state','complete');
                INSERT INTO metadata VALUES('index_publication_generation','1');
                CREATE TABLE nodes(id INTEGER,path TEXT);
                CREATE TABLE purposes(node_id INTEGER,purpose TEXT,status TEXT);
                CREATE TABLE symbols(id INTEGER);
                CREATE TABLE symbol_relations(id INTEGER);
                CREATE TABLE graph_entities(id INTEGER);
                CREATE TABLE graph_relations(id INTEGER);
            """)
            connection.execute("INSERT INTO nodes VALUES(1,?)", (SOURCE_FILE,))
            connection.execute("INSERT INTO purposes VALUES(1,'Keep source authority','approved')")
            connection.commit()
        finally:
            connection.close()

    def test_state_reader_closes_on_success_and_query_failure(self):
        original_connect = sqlite3.connect
        for fail_query in (False, True):
            with self.subTest(fail_query=fail_query), tempfile.TemporaryDirectory() as task:
                root = Path(task)
                self.project(root)
                if fail_query:
                    connection = original_connect(database(root))
                    try:
                        connection.execute("DROP TABLE graph_relations")
                        connection.commit()
                    finally:
                        connection.close()
                opened = []

                def observe_connection(*args, **kwargs):
                    connection = original_connect(*args, **kwargs)
                    opened.append(connection)
                    with self.assertRaises(sqlite3.OperationalError):
                        connection.execute("DELETE FROM metadata")
                    return connection

                try:
                    with patch("indexing_acceptance.sqlite3.connect", side_effect=observe_connection), \
                            patch("sys.stderr", new_callable=io.StringIO) as diagnostics:
                        if fail_query:
                            with self.assertRaises(sqlite3.OperationalError):
                                state(root)
                            details = json.loads(diagnostics.getvalue())["sqlite_inspection"]
                            self.assertEqual(details["stage"], "query")
                            self.assertTrue(details["files"]["database"]["exists"])
                        else:
                            result = state(root)
                            self.assertEqual(result["generation"], 1)
                            self.assertEqual(result["approved_purposes"], [(SOURCE_FILE, "Keep source authority", "approved")])
                    self.assertEqual(len(opened), 1)
                    with self.assertRaises(sqlite3.ProgrammingError):
                        opened[0].execute("SELECT 1")
                finally:
                    for connection in opened:
                        connection.close()

    def test_missing_database_is_not_created_and_reports_connect_stage(self):
        with tempfile.TemporaryDirectory() as task:
            root = Path(task)
            with patch("sys.stderr", new_callable=io.StringIO) as diagnostics:
                with self.assertRaises(sqlite3.OperationalError):
                    with database_reader(root):
                        self.fail("missing database was opened")
            self.assertFalse(database(root).exists())
            details = json.loads(diagnostics.getvalue())["sqlite_inspection"]
            self.assertEqual(details["stage"], "connect")
            self.assertEqual(details["sqlite_version"], sqlite3.sqlite_version)
            self.assertFalse(details["files"]["database"]["exists"])
            self.assertFalse(details["files"]["parent"]["exists"])

    def test_reader_observes_settled_wal_without_writing_database(self):
        with tempfile.TemporaryDirectory() as task:
            root = Path(task)
            self.project(root)
            path = database(root)
            writer = sqlite3.connect(path)
            try:
                self.assertEqual(writer.execute("PRAGMA journal_mode=WAL").fetchone(), ("wal",))
                writer.execute("UPDATE purposes SET purpose='Settled WAL authority'")
                writer.commit()
                self.assertEqual(writer.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchone(),
                                 (0, 0, 0))
            finally:
                writer.close()
            wal = Path(str(path) + "-wal")
            shm = Path(str(path) + "-shm")
            # Some native SQLite builds retain sidecars after the last close.
            # This fixture owns every connection: after a successful truncating
            # checkpoint and close, only an empty WAL may be removed safely.
            if wal.exists():
                self.assertEqual(wal.stat().st_size, 0, "fixture WAL still contains frames")
                wal.unlink()
            shm.unlink(missing_ok=True)
            self.assertEqual(path.read_bytes()[18:20], b"\x02\x02", "fixture left WAL mode")
            self.assertFalse(wal.exists())
            self.assertFalse(shm.exists())
            before = digest(path)
            with database_reader(root) as reader:
                self.assertEqual(reader.execute("SELECT purpose FROM purposes").fetchone(),
                                 ("Settled WAL authority",))
                with self.assertRaises(sqlite3.OperationalError):
                    reader.execute("UPDATE purposes SET purpose='Forbidden change'")
            self.assertEqual(digest(path), before)

    def test_reader_observes_live_wal_without_writing_database(self):
        with tempfile.TemporaryDirectory() as task:
            root = Path(task)
            self.project(root)
            path = database(root)
            wal = Path(str(path) + "-wal")
            writer = sqlite3.connect(path)
            try:
                self.assertEqual(writer.execute("PRAGMA journal_mode=WAL").fetchone(), ("wal",))
                writer.execute("UPDATE purposes SET purpose='Committed live WAL authority'")
                writer.commit()
                self.assertTrue(wal.exists())
                before, wal_before = digest(path), digest(wal)
                with database_reader(root) as reader:
                    self.assertEqual(reader.execute("SELECT purpose FROM purposes").fetchone(),
                                     ("Committed live WAL authority",))
                    with self.assertRaises(sqlite3.OperationalError):
                        reader.execute("CREATE TABLE forbidden(value TEXT)")
                self.assertEqual(digest(path), before)
                self.assertEqual(digest(wal), wal_before)
            finally:
                writer.close()

    def test_backup_closes_handles_and_preserves_committed_wal_content(self):
        original_connect = sqlite3.connect
        for fail_target in (False, True):
            with self.subTest(fail_target=fail_target), tempfile.TemporaryDirectory() as task:
                root = Path(task)
                self.project(root)
                source = original_connect(database(root))
                opened = []
                try:
                    source.execute("PRAGMA journal_mode=WAL")
                    source.execute("UPDATE purposes SET purpose='Committed WAL authority'")
                    source.commit()
                    target = root / ("missing/backup.db" if fail_target else "backup.db")

                    def observe_connection(*args, **kwargs):
                        connection = original_connect(*args, **kwargs)
                        opened.append(connection)
                        return connection

                    with patch("indexing_acceptance.sqlite3.connect", side_effect=observe_connection):
                        if fail_target:
                            with self.assertRaises(sqlite3.OperationalError):
                                copy_database(database(root), target)
                        else:
                            copy_database(database(root), target)
                    self.assertEqual(len(opened), 1 if fail_target else 2)
                    for connection in opened:
                        with self.assertRaises(sqlite3.ProgrammingError):
                            connection.execute("SELECT 1")
                    if not fail_target:
                        check = original_connect(target)
                        try:
                            self.assertEqual(check.execute("SELECT purpose FROM purposes").fetchone(),
                                             ("Committed WAL authority",))
                        finally:
                            check.close()
                finally:
                    for connection in opened:
                        connection.close()
                    source.close()


class WarmMeasurement(unittest.TestCase):
    @contextmanager
    def protocol(self, root, *, fail_construct=None, fail_call=None, change_state=None):
        """Observe real fixture state around controlled, serial MCP responses."""
        executable = root / "runtime"
        executable.write_bytes(b"warm measurement protocol fixture")
        campaign = Campaign(root / "result.json", executable, executable, "0.5.1-rc1")
        events, live, calls = [], set(), {"baseline": 0, "candidate": 0}
        owner = self

        def create(current):
            current.mkdir(parents=True)
            path = current / SOURCE_FILE
            path.parent.mkdir(parents=True)
            path.write_text("pub fn example(n: u64) -> u64 { n.saturating_add(1) }\n")
            DatabaseInspection().project(current)
            with closing(sqlite3.connect(database(current))) as connection, connection:
                connection.execute("CREATE TABLE file_texts(path TEXT, content TEXT)")
                connection.execute("INSERT INTO file_texts VALUES(?,?)", (SOURCE_FILE, path.read_text()))

        def run(runtime, current, arguments, case=None):
            owner.assertEqual(current.name, runtime)
            if case == "offline_same_metadata":
                owner.assertFalse(live, "offline controls must follow both client closures")
                events.append(("offline", runtime))
                with closing(sqlite3.connect(database(current))) as connection, connection:
                    connection.execute("UPDATE file_texts SET content=? WHERE path=?",
                                       ((current / SOURCE_FILE).read_text(), SOURCE_FILE))
            else:
                owner.assertEqual(arguments, ["init"])
                events.append(("init", runtime))
            return {}

        class Client:
            def __init__(self, executable, current, env, **options):
                self.runtime, self.current = current.name, current
                events.append(("construct", self.runtime))
                owner.assertEqual(options["required_version"], campaign.versions[self.runtime])
                if self.runtime == fail_construct:
                    raise RuntimeError("injected client construction failure")
                live.add(self.runtime)

            def call(self, name, arguments):
                owner.assertEqual(live, {"baseline", "candidate"},
                                  "both clients must remain live throughout paired sampling")
                owner.assertEqual(name, "atlas_watch_once")
                owner.assertEqual(arguments, {"project_path": str(self.current)})
                calls[self.runtime] += 1
                point = self.runtime, calls[self.runtime]
                events.append(("call", self.runtime))
                if point == fail_call:
                    raise RuntimeError("injected request failure")
                if point == change_state:
                    with closing(sqlite3.connect(database(self.current))) as connection, connection:
                        connection.execute("UPDATE purposes SET purpose='unexpected publication'")
                return "watch: unchanged", float(len(events))

            def close(self):
                owner.assertIn(self.runtime, live)
                live.remove(self.runtime)
                events.append(("close", self.runtime))

        with ExitStack() as patches:
            patches.enter_context(patch("indexing_acceptance.fixture", side_effect=create))
            patches.enter_context(patch.object(campaign, "run", side_effect=run))
            patches.enter_context(patch("indexing_acceptance.McpClient", Client))
            yield campaign, events, live

    def test_warm_calls_are_paired_and_controls_follow_both_client_closures(self):
        with tempfile.TemporaryDirectory() as task, self.protocol(Path(task)) as (campaign, events, live):
            campaign.warm(Path(task) / "warm")
            expected = [runtime for round_index in range(10)
                        for runtime in (("baseline", "candidate") if round_index % 2 == 0
                                        else ("candidate", "baseline"))]
            self.assertEqual([runtime for event, runtime in events if event == "call"], expected)
            first_call = next(index for index, event in enumerate(events) if event[0] == "call")
            self.assertEqual(events[:2], [("init", "baseline"), ("init", "candidate")])
            self.assertEqual(sum(event[0] == "construct" for event in events[:first_call]), 2)
            samples = campaign.result["warm_mcp"]
            self.assertEqual([row["runtime"] for row in samples], expected[2:])
            self.assertEqual([(row["round"], row["position"]) for row in samples],
                             [(round_index, position) for round_index in range(1, 10)
                              for position in range(2)])
            self.assertEqual(campaign.result["warm_mcp_order"], [
                {"runtime": runtime, "round": index // 2, "position": index % 2,
                 "warmup": index < 2} for index, runtime in enumerate(expected)])
            self.assertEqual(len(campaign.result["checks"]), 44)
            self.assertTrue(all(row["passed"] for row in campaign.result["checks"]))
            for runtime in ("baseline", "candidate"):
                for control in ("warm MCP refresh succeeds", "warm MCP preserves publication"):
                    self.assertEqual(sum(row["name"] == runtime + " " + control
                                         for row in campaign.result["checks"]), 10)
            self.assertEqual([event for event in events if event[0] == "offline"],
                             [("offline", "baseline"), ("offline", "candidate")])
            self.assertFalse(live)

    def test_second_client_construction_failure_closes_first_before_any_call(self):
        with tempfile.TemporaryDirectory() as task, self.protocol(
                Path(task), fail_construct="candidate") as (campaign, events, live):
            with self.assertRaisesRegex(RuntimeError, "injected client construction failure"):
                campaign.warm(Path(task) / "warm")
            self.assertFalse(live)
            self.assertNotIn("call", [event[0] for event in events])
            self.assertEqual(events[-1], ("close", "baseline"))

    def test_request_and_publication_failures_close_both_and_preserve_partial_evidence(self):
        cases = [(failure, (runtime, call_index)) for failure in ("request", "publication")
                 for runtime in ("baseline", "candidate") for call_index in range(1, 11)]
        for failure, point in cases:
            with self.subTest(failure=failure, point=point), tempfile.TemporaryDirectory() as task:
                options = {"fail_call" if failure == "request" else "change_state": point}
                with self.protocol(Path(task), **options) as (campaign, events, live):
                    with self.assertRaisesRegex(
                            (RuntimeError if failure == "request" else AssertionError),
                            ("injected request failure" if failure == "request" else
                             "warm MCP preserves publication")):
                        campaign.warm(Path(task) / "warm")
                    self.assertFalse(live)
                    self.assertEqual({runtime for event, runtime in events if event == "close"},
                                     {"baseline", "candidate"})
                    self.assertNotIn("offline", [event[0] for event in events])
                    attempts = [runtime for event, runtime in events if event == "call"]
                    self.assertEqual([row["runtime"] for row in campaign.result["warm_mcp_order"]],
                                     attempts)
                    self.assertEqual(len(campaign.result["warm_mcp"]),
                                     max(0, len(attempts) - 2 - (failure == "request")))
                    saved = json.loads(campaign.output.read_text())
                    self.assertEqual(saved["warm_mcp"], campaign.result["warm_mcp"])
                    self.assertEqual(saved["warm_mcp_order"], campaign.result["warm_mcp_order"])


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
