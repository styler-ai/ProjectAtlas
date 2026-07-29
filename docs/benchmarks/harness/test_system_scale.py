import hashlib
import inspect
import json
import os
import sqlite3
import stat
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest import mock

import system_scale
import mcp_composition


class SystemScaleHarnessTests(unittest.TestCase):
    def test_remove_tree_tolerates_entry_disappearing_during_permission_retry(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "tree"
            root.mkdir()
            missing = root / "vanished"

            def simulate_rmtree(_path: Path, onerror: object) -> None:
                onerror(
                    os.unlink,
                    str(missing),
                    (FileNotFoundError, FileNotFoundError(), None),
                )

            with mock.patch.object(
                mcp_composition.shutil,
                "rmtree",
                side_effect=simulate_rmtree,
            ):
                mcp_composition.remove_tree(
                    root, allowed_parent=Path(directory)
                )

    def test_remove_tree_retries_a_transient_nonempty_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "tree"
            root.mkdir()
            transient = OSError("directory is not empty")
            transient.winerror = 145
            with mock.patch.object(
                mcp_composition.shutil,
                "rmtree",
                side_effect=[transient, None],
            ) as rmtree:
                mcp_composition.remove_tree(
                    root, allowed_parent=Path(directory)
                )
            self.assertEqual(rmtree.call_count, 2)
            if os.name == "nt":
                self.assertTrue(
                    str(rmtree.call_args_list[0].args[0]).startswith("\\\\?\\")
                )

    def test_remove_tree_propagates_unrelated_errors_and_exhausted_retries(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "tree"
            root.mkdir()
            with mock.patch.object(
                mcp_composition.shutil,
                "rmtree",
                side_effect=PermissionError("denied"),
            ):
                with self.assertRaises(PermissionError):
                    mcp_composition.remove_tree(
                        root, allowed_parent=Path(directory)
                    )
            transient = OSError("directory is not empty")
            transient.winerror = 145
            with mock.patch.object(
                mcp_composition.shutil,
                "rmtree",
                side_effect=transient,
            ) as rmtree:
                with self.assertRaises(OSError):
                    mcp_composition.remove_tree(
                        root, allowed_parent=Path(directory)
                    )
            self.assertEqual(rmtree.call_count, 3)

    @unittest.skipUnless(os.name == "nt", "Windows long-path behavior")
    def test_remove_tree_handles_long_paths_without_following_reparse_points(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            root = parent / "tree"
            current = root
            while len(str(current)) <= 280:
                current /= "forty-character-segment-0123456789abcdef"
            extended_current = Path(f"\\\\?\\{current}")
            extended_current.mkdir(parents=True)
            (extended_current / "payload.txt").write_text(
                "owned", encoding="utf-8"
            )
            mcp_composition.remove_tree(root, allowed_parent=parent)
            self.assertFalse(root.exists())

            root.mkdir()
            metadata = mock.Mock(
                st_mode=stat.S_IFDIR,
                st_file_attributes=stat.FILE_ATTRIBUTE_REPARSE_POINT,
            )
            with (
                mock.patch.object(Path, "lstat", return_value=metadata),
                mock.patch.object(mcp_composition.shutil, "rmtree") as rmtree,
                self.assertRaises(ValueError),
            ):
                mcp_composition.remove_tree(root, allowed_parent=parent)
            rmtree.assert_not_called()

    @unittest.skipUnless(os.name == "nt", "Windows extended-path behavior")
    def test_remove_tree_preserves_extended_paths_and_converts_unc_paths(self) -> None:
        metadata = mock.Mock(st_mode=stat.S_IFDIR, st_file_attributes=0)
        cases = (
            (
                Path(r"\\?\C:\benchmark\child"),
                Path(r"\\?\C:\benchmark"),
                r"\\?\C:\benchmark\child",
            ),
            (
                Path(r"\\server\share\benchmark\child"),
                Path(r"\\server\share\benchmark"),
                r"\\?\UNC\server\share\benchmark\child",
            ),
        )
        for path, parent, expected in cases:
            with (
                self.subTest(path=path),
                mock.patch.object(Path, "lstat", return_value=metadata),
                mock.patch.object(mcp_composition.shutil, "rmtree") as rmtree,
            ):
                mcp_composition.remove_tree(path, allowed_parent=parent)
                self.assertEqual(str(rmtree.call_args.args[0]), expected)

    @staticmethod
    def process_io_fixture(
        *, incremental: bool = False
    ) -> tuple[dict[str, object], dict[str, object]]:
        result: dict[str, object] = {
            "scale": "medium",
            "scan": {
                "pre_run_database_bytes": 10,
                "post_run_database_bytes": 1_000_000,
                "report": {"text_index": {"bytes": 100}},
                "process": {
                    "process_read_transfer_bytes": 1_100,
                    "process_write_transfer_bytes": 1_100,
                },
            },
            "incremental": None,
        }
        preregistration: dict[str, object] = {
            "thresholds": {
                "all": {
                    "maximum_expanded_guidance_process_read_transfer_bytes": (
                        64 * 1024 * 1024
                    ),
                    "maximum_expanded_guidance_process_write_transfer_bytes": (
                        2 * 1024 * 1024
                    ),
                },
                "medium": {
                    "maximum_full_process_read_transfer_bytes": 2_000,
                    "maximum_full_process_write_transfer_bytes": 2_000,
                },
            }
        }
        if incremental:
            result["incremental"] = {
                "expanded": {
                    "guidance": {
                        "process": {
                            "process_read_transfer_bytes": 1,
                            "process_write_transfer_bytes": 2 * 1024 * 1024 + 1,
                        }
                    },
                    "rebuild": {
                        "pre_run_database_bytes": 10,
                        "report": {"text_index": {"bytes": 100}},
                        "process": {
                            "process_read_transfer_bytes": 1_100,
                            "process_write_transfer_bytes": 1_100,
                        },
                    },
                }
            }
        return result, preregistration

    def test_windows_process_write_transfers_exclude_captured_output(self) -> None:
        with mock.patch.object(system_scale.platform, "system", return_value="Windows"):
            self.assertEqual(
                system_scale.measured_process_write_transfer_bytes(693, 693, 0), 0
            )
            with self.assertRaises(RuntimeError):
                system_scale.measured_process_write_transfer_bytes(600, 693, 0)
        with mock.patch.object(system_scale.platform, "system", return_value="Linux"):
            self.assertEqual(
                system_scale.measured_process_write_transfer_bytes(693, 693, 0), 693
            )

    def test_publication_identity_rejects_every_unlocked_boundary(self) -> None:
        preregistration = {
            "status": "draft",
            "candidate": {
                "required_version": "0.4.0",
                "runtime_sha256": "expected-runtime",
                "mcp_tools_sha256": "expected-tools",
                "skill_sha256": "expected-skill",
                "skill_bytes": 100,
            },
        }
        errors = system_scale.publication_identity_errors(
            preregistration,
            runtime_sha256="other-runtime",
            mcp_tools_sha256="other-tools",
            skill_sha256="other-skill",
            skill_bytes=200,
            runtime_info={
                "project": "Other",
                "version": "0.3.26",
                "capabilities": [],
                "text_format": "JSON",
                "mcp_tools": [],
            },
            dirty_paths=[
                "docs/benchmarks/v0.4-system-scale-preregistration.json",
                "docs/benchmarks/harness/system_scale.py",
            ],
            measurement_errors=["measurement input lock is invalid"],
        )
        self.assertEqual(len(errors), 12)

    def test_candidate_file_identity_rejects_escape_and_missing_artifacts(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            root = parent / "candidate"
            skill = root / "plugins/projectatlas/skills/projectatlas/SKILL.md"
            skill.parent.mkdir(parents=True)
            skill.write_bytes(b"skill\n")
            self.assertEqual(
                system_scale.candidate_file_identity(
                    "plugins/projectatlas/skills/projectatlas/SKILL.md",
                    root=root,
                ),
                {
                    "path": "plugins/projectatlas/skills/projectatlas/SKILL.md",
                    "sha256": hashlib.sha256(b"skill\n").hexdigest(),
                    "bytes": 6,
                },
            )
            (parent / "outside.md").write_bytes(b"outside\n")
            with self.assertRaisesRegex(ValueError, "escapes"):
                system_scale.candidate_file_identity(
                    "../outside.md",
                    root=root,
                )
            with self.assertRaisesRegex(ValueError, "regular file"):
                system_scale.candidate_file_identity("missing.md", root=root)

    def test_publication_identity_allows_clean_content_bound_preregistration(
        self,
    ) -> None:
        preregistration = {
            "status": "locked_for_final_measurement",
            "candidate": {
                "required_version": "0.4.0",
                "runtime_sha256": "runtime",
                "mcp_tools_sha256": "tools",
                "skill_sha256": "skill",
                "skill_bytes": 100,
            },
        }
        errors = system_scale.publication_identity_errors(
            preregistration,
            runtime_sha256="runtime",
            mcp_tools_sha256="tools",
            skill_sha256="skill",
            skill_bytes=100,
            runtime_info={
                "project": "ProjectAtlas",
                "version": "0.4.0",
                "capabilities": ["mcp", "sqlite", "toon"],
                "text_format": "TOON",
                "mcp_tools": [f"tool-{index}" for index in range(40)],
            },
            dirty_paths=[],
            measurement_errors=[],
        )
        self.assertEqual(errors, [])

    def test_termination_recovery_requires_reopen_integrity_and_cleanup(
        self,
    ) -> None:
        process = {"returncode": 0, "timed_out": False}
        checkpoint = {"busy": 0}
        profile = {"quick_check": "ok"}
        storage = {"wal_bytes": 0, "staging_bytes": 0, "stage_directories": 0}
        self.assertTrue(
            system_scale.termination_recovery_is_complete(
                process, checkpoint, profile, storage
            )
        )
        failures = (
            (
                {"returncode": 1, "timed_out": False},
                checkpoint,
                profile,
                storage,
            ),
            (
                {"returncode": 0, "timed_out": True},
                checkpoint,
                profile,
                storage,
            ),
            (process, {"busy": 1}, profile, storage),
            (process, checkpoint, {"quick_check": "malformed"}, storage),
            (process, checkpoint, profile, {**storage, "wal_bytes": 1}),
            (process, checkpoint, profile, {**storage, "staging_bytes": 1}),
            (process, checkpoint, profile, {**storage, "stage_directories": 1}),
        )
        for (
            reopen_process,
            recovery_checkpoint,
            recovery_profile,
            final_storage,
        ) in failures:
            with self.subTest(
                reopen_process=reopen_process,
                recovery_checkpoint=recovery_checkpoint,
                recovery_profile=recovery_profile,
                final_storage=final_storage,
            ):
                self.assertFalse(
                    system_scale.termination_recovery_is_complete(
                        reopen_process,
                        recovery_checkpoint,
                        recovery_profile,
                        final_storage,
                    )
                )

    def test_measurement_input_lock_fails_closed_on_path_or_digest_drift(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "-q"], cwd=root, check=True)
            subprocess.run(
                ["git", "config", "user.email", "benchmark@example.invalid"],
                cwd=root,
                check=True,
            )
            subprocess.run(
                ["git", "config", "user.name", "Benchmark Test"],
                cwd=root,
                check=True,
            )
            subprocess.run(
                ["git", "config", "core.autocrlf", "false"], cwd=root, check=True
            )
            required = (
                "docs/benchmarks/harness/measure.py",
                "docs/benchmarks/fixtures/mcp-composition",
            )
            path = root / required[0]
            path.parent.mkdir(parents=True)
            path.write_bytes(b"locked\n")
            fixture = root / required[1] / "clean/src/lib.rs"
            fixture.parent.mkdir(parents=True)
            fixture.write_bytes(b"fixture\n")
            subprocess.run(["git", "add", "."], cwd=root, check=True)
            subprocess.run(
                ["git", "commit", "-q", "-m", "lock input"], cwd=root, check=True
            )
            locked_head = subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=root, text=True
            ).strip()
            preregistration = {
                "measurement_inputs": {
                    relative: system_scale.committed_git_object_sha256(
                        relative, root=root
                    )
                    for relative in required
                }
            }
            self.assertEqual(
                system_scale.measurement_input_errors(
                    preregistration, required, root=root
                ),
                [],
            )
            path.write_bytes(b"locked\r\n")
            self.assertEqual(
                system_scale.measurement_input_errors(
                    preregistration, required, root=root
                ),
                [],
            )
            path.write_bytes(b"changed\n")
            subprocess.run(["git", "add", "."], cwd=root, check=True)
            subprocess.run(
                ["git", "commit", "-q", "-m", "change input"], cwd=root, check=True
            )
            self.assertEqual(
                system_scale.measurement_input_errors(
                    preregistration, required, root=root
                ),
                [f"measurement input changed after lock: {required[0]}"],
            )
            self.assertEqual(
                system_scale.measurement_input_errors(
                    preregistration, required, root=root, revision=locked_head
                ),
                [],
            )
            path.write_bytes(b"locked\n")
            added_fixture = root / required[1] / "dirty/src/added.rs"
            added_fixture.parent.mkdir(parents=True)
            added_fixture.write_bytes(b"added\n")
            subprocess.run(["git", "add", "."], cwd=root, check=True)
            subprocess.run(
                ["git", "commit", "-q", "-m", "change fixture tree"],
                cwd=root,
                check=True,
            )
            self.assertEqual(
                system_scale.measurement_input_errors(
                    preregistration, required, root=root
                ),
                [f"measurement input changed after lock: {required[1]}"],
            )
            self.assertEqual(
                system_scale.measurement_input_errors(
                    {"measurement_inputs": {}}, required, root=root
                ),
                ["measurement input lock does not match the required path set"],
            )

    def test_candidate_source_identity_is_descriptive_checkout_provenance(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            preregistration = root / "docs/benchmarks/preregistration.json"
            with (
                mock.patch.object(system_scale, "ROOT", root),
                mock.patch.object(
                    system_scale.subprocess,
                    "check_output",
                    return_value=("a" * 40) + "\n",
                ) as check_output,
            ):
                identity = system_scale.candidate_source_identity(preregistration)
        self.assertEqual(
            identity,
            {
                "checkout_head": "a" * 40,
                "preregistration_path": "docs/benchmarks/preregistration.json",
            },
        )
        check_output.assert_called_once_with(
            ["git", "rev-parse", "HEAD"], cwd=root, text=True
        )

    def test_database_profile_uses_real_sqlite(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            database = Path(temporary) / "projectatlas.db"
            connection = sqlite3.connect(database)
            connection.executescript(
                """
                CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
                INSERT INTO metadata VALUES('project_root', '/fixture');
                """
            )
            connection.commit()
            connection.close()

            profile = system_scale.database_profile(database)
            self.assertEqual(profile["quick_check"], "ok")
            self.assertEqual(profile["page_bytes"], database.stat().st_size)
            self.assertFalse(profile["sqlite_stat1_present"])

    def test_post_run_database_inflation_cannot_improve_source_input_ratio(
        self,
    ) -> None:
        result, preregistration = self.process_io_fixture()
        evaluation = system_scale.evaluate_process_io_contract(
            result, preregistration
        )
        self.assertEqual(evaluation["full_source_input_read_ratio"], 10)
        self.assertLess(
            evaluation["full_output_efficiency_read_ratio"],
            evaluation["full_source_input_read_ratio"],
        )
        result["scan"]["post_run_database_bytes"] = 10_000_000
        inflated = system_scale.evaluate_process_io_contract(
            result, preregistration
        )
        self.assertEqual(inflated["full_source_input_read_ratio"], 10)
        self.assertLess(
            inflated["full_output_efficiency_read_ratio"],
            evaluation["full_output_efficiency_read_ratio"],
        )

    def test_absolute_transfer_cap_rejects_even_when_ratio_passes(self) -> None:
        result, preregistration = self.process_io_fixture()
        result["scan"]["report"]["text_index"]["bytes"] = 10_000
        preregistration["thresholds"]["medium"][
            "maximum_full_process_read_transfer_bytes"
        ] = 1_000
        evaluation = system_scale.evaluate_process_io_contract(
            result, preregistration
        )
        self.assertLess(evaluation["full_output_efficiency_read_ratio"], 40)
        self.assertFalse(evaluation["full_read_transfer_within_absolute_cap"])

    def test_incremental_io_uses_guidance_cap_and_pre_rebuild_database(
        self,
    ) -> None:
        result, preregistration = self.process_io_fixture(incremental=True)
        evaluation = system_scale.evaluate_process_io_contract(
            result, preregistration
        )
        self.assertFalse(
            evaluation["expanded_guidance_write_within_absolute_cap"]
        )
        self.assertEqual(evaluation["rebuild_input_efficiency_read_ratio"], 10)
        result["incremental"]["expanded"]["rebuild"][
            "pre_run_database_bytes"
        ] = 1_000
        changed = system_scale.evaluate_process_io_contract(
            result, preregistration
        )
        self.assertEqual(
            changed["rebuild_input_efficiency_read_ratio"], 1
        )

    def test_non_windows_final_measurement_is_explicitly_ineligible(self) -> None:
        with mock.patch.object(system_scale.platform, "system", return_value="Linux"):
            final = system_scale.final_measurement_eligibility("all")
            smoke = system_scale.final_measurement_eligibility("medium")
        self.assertFalse(final["final_platform_eligible"])
        self.assertEqual(final["disposition"], "failed_ineligible_platform")
        self.assertEqual(smoke["disposition"], "skipped_nonfinal_smoke")
        self.assertEqual(
            final["kind"], "sampled-nonterminal-process-tree-transfer-counters"
        )

    def test_stalled_mcp_request_is_bounded_and_kills_server(self) -> None:
        class FakeInput:
            closed = False

            def write(self, _value: str) -> None:
                pass

            def flush(self) -> None:
                pass

        class FakeOutput:
            def __init__(self) -> None:
                self.stopped = threading.Event()

            def readline(self) -> str:
                self.stopped.wait()
                return ""

        class FakeProcess:
            def __init__(self) -> None:
                self.stdin = FakeInput()
                self.stdout = FakeOutput()
                self.killed = False

            def poll(self) -> int | None:
                return -9 if self.killed else None

            def kill(self) -> None:
                self.killed = True
                self.stdout.stopped.set()

            def wait(self, timeout: float) -> int:
                if not self.killed:
                    raise subprocess.TimeoutExpired("fake-mcp", timeout)
                return -9

        process = FakeProcess()
        started = time.perf_counter()
        def terminate(fake_process: FakeProcess, _job: object) -> None:
            fake_process.kill()
            fake_process.wait(5)

        with (
            mock.patch.object(
                mcp_composition,
                "spawn_owned_process",
                return_value=(process, None),
            ),
            mock.patch.object(
                mcp_composition,
                "terminate_owned_process",
                side_effect=terminate,
            ),
        ):
            with self.assertRaisesRegex(TimeoutError, "initialize"):
                mcp_composition.McpClient(
                    Path("fake-runtime"),
                    Path("."),
                    {},
                    request_timeout_seconds=0.02,
                )
        self.assertLess(time.perf_counter() - started, 1)
        self.assertTrue(process.killed)
        self.assertFalse(
            any(
                thread.name == "projectatlas-mcp-response-1"
                for thread in threading.enumerate()
            )
        )

    def test_mcp_default_timeout_is_the_preregistered_sixty_seconds(self) -> None:
        default = inspect.signature(
            mcp_composition.McpClient.__init__
        ).parameters["request_timeout_seconds"].default
        self.assertEqual(mcp_composition.MCP_REQUEST_TIMEOUT_SECONDS, 60)
        self.assertEqual(default, 60)

    def test_initialized_notification_failure_terminates_server(self) -> None:
        class FailingInput:
            closed = False

            def __init__(self) -> None:
                self.writes = 0

            def write(self, _value: str) -> None:
                self.writes += 1
                if self.writes == 2:
                    raise BrokenPipeError("notification pipe closed")

            def flush(self) -> None:
                pass

            def close(self) -> None:
                self.closed = True

        class InitializeOutput:
            closed = False

            def __init__(self) -> None:
                self.read = False

            def readline(self) -> str:
                if self.read:
                    return ""
                self.read = True
                return '{"jsonrpc":"2.0","id":1,"result":{}}\n'

            def close(self) -> None:
                self.closed = True

        class FakeProcess:
            def __init__(self) -> None:
                self.stdin = FailingInput()
                self.stdout = InitializeOutput()
                self.killed = False

            def poll(self) -> int | None:
                return -9 if self.killed else None

            def wait(self, timeout: float) -> int:
                if not self.killed:
                    raise subprocess.TimeoutExpired("fake-mcp", timeout)
                return -9

        process = FakeProcess()

        def terminate(fake_process: FakeProcess, _job: object) -> None:
            fake_process.killed = True

        with (
            mock.patch.object(
                mcp_composition,
                "spawn_owned_process",
                return_value=(process, None),
            ),
            mock.patch.object(
                mcp_composition,
                "terminate_owned_process",
                side_effect=terminate,
            ) as cleanup,
            self.assertRaisesRegex(
                BrokenPipeError, "notification pipe closed"
            ),
        ):
            mcp_composition.McpClient(Path("fake"), Path("."), {})
        self.assertTrue(process.killed)
        self.assertEqual(cleanup.call_count, 1)

    def test_request_protocol_failures_terminate_server(self) -> None:
        expected_errors = {
            "malformed": json.JSONDecodeError,
            "eof": RuntimeError,
            "write": BrokenPipeError,
        }
        for mode in ("malformed", "eof", "write"):
            with self.subTest(mode=mode):
                class FakeInput:
                    def __init__(self) -> None:
                        self.closed = False
                        self.writes = 0

                    def write(self, _value: str) -> None:
                        self.writes += 1
                        if mode == "write" and self.writes == 3:
                            raise BrokenPipeError("request pipe closed")

                    def flush(self) -> None:
                        pass

                    def close(self) -> None:
                        self.closed = True

                class FakeOutput:
                    def __init__(self) -> None:
                        self.closed = False
                        self.lines = [
                            '{"jsonrpc":"2.0","id":1,"result":{}}\n',
                            "not-json\n" if mode == "malformed" else "",
                        ]

                    def readline(self) -> str:
                        return self.lines.pop(0) if self.lines else ""

                    def close(self) -> None:
                        self.closed = True

                class FakeProcess:
                    def __init__(self) -> None:
                        self.stdin = FakeInput()
                        self.stdout = FakeOutput()
                        self.killed = False

                    def poll(self) -> int | None:
                        return -9 if self.killed else None

                    def wait(self, timeout: float) -> int:
                        if not self.killed:
                            raise subprocess.TimeoutExpired("fake-mcp", timeout)
                        return -9

                process = FakeProcess()

                def terminate(fake_process: FakeProcess, _job: object) -> None:
                    fake_process.killed = True

                with (
                    mock.patch.object(
                        mcp_composition,
                        "spawn_owned_process",
                        return_value=(process, None),
                    ),
                    mock.patch.object(
                        mcp_composition,
                        "terminate_owned_process",
                        side_effect=terminate,
                    ) as cleanup,
                ):
                    client = mcp_composition.McpClient(
                        Path("fake"), Path("."), {}
                    )
                    with self.assertRaises(expected_errors[mode]):
                        client.request("tools/list", {})
                self.assertTrue(process.killed)
                self.assertEqual(cleanup.call_count, 1)
                self.assertFalse(
                    any(
                        thread.name == "projectatlas-mcp-response-2"
                        for thread in threading.enumerate()
                    )
                )

    def test_mcp_close_is_idempotent(self) -> None:
        class FakeProcess:
            stdin = None
            stdout = None

            @staticmethod
            def wait(timeout: float) -> int:
                return 0

        client = mcp_composition.McpClient.__new__(
            mcp_composition.McpClient
        )
        client.process = FakeProcess()
        client.job = None
        client.closed = False
        with mock.patch.object(
            mcp_composition, "terminate_owned_process"
        ) as cleanup:
            client.close()
            client.close()
        self.assertEqual(cleanup.call_count, 1)

    def test_watch_resume_failure_reaps_the_owned_process(self) -> None:
        class FailingJob:
            @staticmethod
            def resume() -> None:
                raise OSError("resume failed")

        process = object()
        job = FailingJob()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with (
                mock.patch.object(
                    system_scale,
                    "spawn_owned_process",
                    return_value=(process, job),
                ),
                mock.patch.object(
                    system_scale, "terminate_owned_process"
                ) as cleanup,
                self.assertRaisesRegex(OSError, "resume failed"),
            ):
                system_scale.measured_watch_edit(
                    Path("fake-runtime"),
                    cwd=root,
                    env={},
                    timeout_seconds=1,
                    edit=lambda: None,
                    readiness_file=root / "ready.rs",
                    writer_probe_database=root / "projectatlas.db",
                )
        cleanup.assert_called_once_with(process, job)

    def test_measurement_timeout_stops_every_sampler(self) -> None:
        class FakeProcess:
            pid = 123

            @staticmethod
            def communicate(timeout: float) -> tuple[bytes, bytes]:
                raise subprocess.TimeoutExpired("measured", timeout)

        class FakeSampler:
            def __init__(self) -> None:
                self.started = False
                self.stopped = False

            def start(self) -> None:
                self.started = True

            def stop(self) -> dict[str, object]:
                self.stopped = True
                return {}

        process_sampler = FakeSampler()
        writer_sampler = FakeSampler()
        with (
            mock.patch.object(
                system_scale,
                "ProcessTreeSampler",
                return_value=process_sampler,
            ),
            mock.patch.object(
                system_scale,
                "SQLiteWriterAvailabilitySampler",
                return_value=writer_sampler,
            ),
            mock.patch.object(system_scale, "terminate_process_tree"),
            self.assertRaises(subprocess.TimeoutExpired),
        ):
            system_scale.collect_measured_process(
                FakeProcess(),
                ["measured"],
                cwd=Path("."),
                timeout_seconds=0.01,
                started=time.perf_counter(),
                writer_probe_database=Path("projectatlas.db"),
            )
        self.assertTrue(process_sampler.started)
        self.assertTrue(process_sampler.stopped)
        self.assertTrue(writer_sampler.started)
        self.assertTrue(writer_sampler.stopped)

    def test_exited_watch_is_reaped_before_pipe_drain(self) -> None:
        events: list[str] = []

        class FakeProcess:
            pid = 123

            @staticmethod
            def poll() -> int:
                return 1

            @staticmethod
            def communicate(timeout: float) -> tuple[bytes, bytes]:
                events.append(f"communicate:{timeout}")
                return b"", b"failed"

        def terminate(_process: object, _job: object) -> None:
            events.append("terminate")

        with (
            mock.patch.object(
                system_scale,
                "terminate_owned_process",
                side_effect=terminate,
            ),
            self.assertRaisesRegex(RuntimeError, "failed"),
        ):
            system_scale.wait_for_indexed_marker(
                FakeProcess(),
                object(),
                Path("projectatlas.db"),
                "src/lib.rs",
                "ready",
                1,
            )
        self.assertEqual(events, ["terminate", "communicate:5"])

    def test_watch_baseline_waits_for_stable_exact_accounting(self) -> None:
        first = {
            "active_processes": 1,
            "write_bytes": 8192,
            "other_bytes": 4096,
        }
        settled = {
            "active_processes": 1,
            "write_bytes": 8192,
            "other_bytes": 8192,
        }

        class FakeJob:
            def __init__(self) -> None:
                self.calls = 0

            def accounting(self) -> dict[str, int]:
                self.calls += 1
                return first if self.calls == 1 else settled

        job = FakeJob()
        baseline = system_scale.wait_for_idle_watch_baseline(job)
        self.assertEqual(baseline, settled)
        self.assertGreaterEqual(job.calls, 3)

    def test_mcp_sampler_start_failure_closes_client(self) -> None:
        class FakeClient:
            process = mock.Mock(pid=123)

            def __init__(self) -> None:
                self.closed = False

            def close(self) -> None:
                self.closed = True

        client = FakeClient()
        sampler = mock.Mock()
        sampler.start.side_effect = RuntimeError("sampler failed")
        with (
            mock.patch.object(system_scale, "McpClient", return_value=client),
            mock.patch.object(
                system_scale, "ProcessTreeSampler", return_value=sampler
            ),
            self.assertRaisesRegex(RuntimeError, "sampler failed"),
        ):
            system_scale.mcp_queries(Path("runtime"), Path("."), {}, {}, 1)
        self.assertTrue(client.closed)
        sampler.stop.assert_not_called()

    def test_post_cancellation_read_accepts_current_or_fail_closed_state(self) -> None:
        self.assertTrue(
            system_scale.post_cancellation_read_is_safe("overview:\n  files: 1\n")
        )
        self.assertTrue(
            system_scale.post_cancellation_read_is_safe(
                "error:\n  kind: refresh_required\n  message: source changed\n"
            )
        )
        self.assertFalse(
            system_scale.post_cancellation_read_is_safe(
                "error:\n  kind: database_error\n  message: unavailable\n"
            )
        )

    def test_cancellation_preflight_failure_closes_client(self) -> None:
        class FakeClient:
            process = mock.Mock(pid=123)

            def __init__(self) -> None:
                self.closed = False

            def close(self) -> None:
                self.closed = True

        client = FakeClient()
        with tempfile.TemporaryDirectory() as temporary:
            with (
                mock.patch.object(
                    system_scale, "database_counts", return_value={}
                ),
                mock.patch.object(system_scale, "McpClient", return_value=client),
                mock.patch.object(
                    system_scale,
                    "process_tree_state",
                    side_effect=RuntimeError("state failed"),
                ),
                mock.patch.object(Path, "write_text", return_value=0),
                self.assertRaisesRegex(RuntimeError, "state failed"),
            ):
                system_scale.cooperative_cancellation_reopen(
                    Path("runtime"),
                    Path(temporary),
                    {},
                    threshold_seconds=1,
                    request_timeout_seconds=1,
                )
        self.assertTrue(client.closed)

    def test_initialize_error_reaps_live_mcp_server_and_descendant(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            temporary_path = Path(temporary)
            server_pid_path = temporary_path / "server.pid"
            child_pid_path = temporary_path / "child.pid"
            server = """
import json
import os
import subprocess
import sys
import time
from pathlib import Path

server_pid_path, child_pid_path = map(Path, sys.argv[1:3])
server_pid_path.write_text(str(os.getpid()), encoding="utf-8")
child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
child_pid_path.write_text(str(child.pid), encoding="utf-8")
request = json.loads(sys.stdin.readline())
print(json.dumps({
    "jsonrpc": "2.0",
    "id": request["id"],
    "error": {"code": -32603, "message": "initialize rejected"},
}), flush=True)
time.sleep(60)
"""
            real_spawn = mcp_composition.spawn_owned_process

            def spawn_server(
                _arguments: list[str], **kwargs: object
            ) -> tuple[subprocess.Popen[object], object]:
                return real_spawn(
                    [
                        sys.executable,
                        "-c",
                        server,
                        str(server_pid_path),
                        str(child_pid_path),
                    ],
                    **kwargs,
                )

            with (
                mock.patch.object(
                    mcp_composition,
                    "spawn_owned_process",
                    side_effect=spawn_server,
                ),
                self.assertRaisesRegex(RuntimeError, "initialize rejected"),
            ):
                mcp_composition.McpClient(
                    Path("fake-runtime"),
                    temporary_path,
                    dict(os.environ),
                    request_timeout_seconds=5,
                )
            server_pid = int(server_pid_path.read_text(encoding="utf-8"))
            child_pid = int(child_pid_path.read_text(encoding="utf-8"))
            self.assertFalse(system_scale.psutil.pid_exists(server_pid))
            self.assertFalse(system_scale.psutil.pid_exists(child_pid))

    def test_storage_probe_counts_only_owned_direct_stage_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            atlas = root / ".projectatlas"
            stage = atlas / "graph-stage-test"
            nested = stage / "nested"
            nested.mkdir(parents=True)
            (atlas / "projectatlas.db").write_bytes(b"a" * 5)
            (stage / "projectatlas.db").write_bytes(b"b" * 7)
            (nested / "projectatlas.db").write_bytes(b"c" * 11)
            state = system_scale.storage_state(root)
            self.assertEqual(state["database_bytes"], 5)
            self.assertEqual(state["staging_bytes"], 7)
            self.assertEqual(state["stage_directories"], 1)

    def test_storage_probe_tolerates_disappearing_sqlite_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            atlas = root / ".projectatlas"
            stage = atlas / "graph-stage-test"
            stage.mkdir(parents=True)
            wal = atlas / "projectatlas.db-wal"
            stage_database = stage / "projectatlas.db"
            wal.touch()
            stage_database.touch()
            disappearing = {wal, stage_database}
            real_stat = Path.stat

            def stat(path: Path, *args: object, **kwargs: object) -> os.stat_result:
                if path in disappearing:
                    raise FileNotFoundError(path)
                return real_stat(path, *args, **kwargs)

            with mock.patch.object(Path, "stat", new=stat):
                state = system_scale.storage_state(root)

            self.assertEqual(state["wal_bytes"], 0)
            self.assertEqual(state["staging_bytes"], 0)
            self.assertEqual(state["stage_directories"], 1)

    def test_failed_result_is_persisted_before_nonzero_exit(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "result.json"
            with self.assertRaisesRegex(SystemExit, "1"):
                system_scale.write_result(
                    {
                        "passed": False,
                        "publication_eligible": False,
                        "path": str(system_scale.ROOT / "private"),
                    },
                    output,
                )
            result = json.loads(output.read_text(encoding="utf-8"))
            self.assertFalse(result["passed"])
            self.assertEqual(
                result["path"], str(Path("{REPO_ROOT}") / "private")
            )
            self.assertNotIn(
                str(system_scale.ROOT), output.read_text(encoding="utf-8")
            )

    def test_main_persists_stalled_mcp_failure(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "result.json"
            argv = [
                "system_scale.py",
                "--runtime",
                str(Path(temporary) / "runtime"),
                "--output",
                str(output),
                "--only",
                "small",
            ]
            with (
                mock.patch.object(sys, "argv", argv),
                mock.patch.object(
                    system_scale,
                    "run_benchmark",
                    side_effect=TimeoutError("stalled MCP"),
                ),
                self.assertRaisesRegex(SystemExit, "1"),
            ):
                system_scale.main()
            result = json.loads(output.read_text(encoding="utf-8"))
            self.assertFalse(result["publication_eligible"])
            self.assertEqual(result["failure"]["type"], "TimeoutError")

    def test_main_persists_invalid_benchmark_paths(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            temporary_path = Path(temporary)
            runtime = temporary_path / "runtime.exe"
            runtime.touch()
            preregistration = temporary_path / "preregistration.json"
            preregistration.write_text("{}\n", encoding="utf-8")
            for option in ("--work-root", "--corpus-cache"):
                output = temporary_path / f"{option[2:]}.json"
                argv = [
                    "system_scale.py",
                    "--runtime",
                    str(runtime),
                    "--preregistration",
                    str(preregistration),
                    "--output",
                    str(output),
                    "--only",
                    "small",
                    option,
                    str(temporary_path / option[2:]),
                ]
                with (
                    self.subTest(option=option),
                    mock.patch.object(sys, "argv", argv),
                    self.assertRaisesRegex(SystemExit, "1"),
                ):
                    system_scale.main()
                result = json.loads(output.read_text(encoding="utf-8"))
                self.assertFalse(result["passed"])
                self.assertEqual(result["failure"]["type"], "ValueError")

    def test_watch_once_applies_the_shared_worker_budget(self) -> None:
        with mock.patch.object(
            system_scale, "run_measured", return_value={"passed": True}
        ) as measured:
            result = system_scale.run_watch_once(
                Path("projectatlas.exe"),
                Path("fixture"),
                {},
                30,
                max_workers=8,
            )
        self.assertEqual(result, {"passed": True})
        self.assertEqual(
            measured.call_args.args[0][-5:],
            ["watch", "--once", "--max-workers", "8", "."],
        )

    def test_concurrent_worker_allocation_fails_closed_on_small_hosts(self) -> None:
        thresholds = {
            "maximum_worker_processes": 20,
            "maximum_worker_processes_per_logical_cpu": 1,
        }
        self.assertEqual(
            system_scale.concurrent_worker_allocation(16, 2, thresholds),
            (16, 8, 16),
        )
        host_budget, _, configured = system_scale.concurrent_worker_allocation(
            1, 2, thresholds
        )
        self.assertGreater(configured, host_budget)

    def test_parser_worker_report_is_a_supplemental_budget_gate(self) -> None:
        runs = [
            {
                "returncode": 0,
                "stdout": json.dumps({"last_symbols": {"max_workers": 8}}),
            },
            {"returncode": 1, "stdout": ""},
        ]
        self.assertTrue(
            system_scale.reported_parser_workers_within_budget(runs, 8)
        )
        runs[0]["stdout"] = json.dumps({"last_symbols": {"max_workers": 9}})
        self.assertFalse(
            system_scale.reported_parser_workers_within_budget(runs, 8)
        )

    def test_concurrent_resource_envelope_sums_process_peaks(self) -> None:
        run = {
            "peak_rss_bytes": 10,
            "peak_worker_processes": 2,
            "worker_process_bound": 3,
            "peak_threads": 3,
            "cpu_seconds": 0.25,
            "process_read_transfer_bytes": 4,
            "process_write_transfer_bytes": 5,
            "terminal_io_complete": True,
        }
        aggregate = system_scale.aggregate_process_metrics(
            [run, {**run, "terminal_io_complete": False}]
        )
        self.assertEqual(aggregate["peak_rss_bytes"], 20)
        self.assertEqual(aggregate["peak_worker_processes"], 4)
        self.assertEqual(aggregate["worker_process_bound"], 6)
        self.assertEqual(aggregate["peak_threads"], 6)
        self.assertEqual(aggregate["cpu_seconds"], 0.5)
        self.assertFalse(aggregate["terminal_io_complete"])

    @unittest.skipUnless(
        sys.platform == "win32", "Windows Job accounting is the final boundary"
    )
    def test_terminal_windows_job_counts_every_short_lived_child(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            direct_interpreter = str(
                Path(sys.base_prefix) / Path(sys.executable).name
            )
            for run in range(20):
                output = root / f"child-{run}.bin"
                child = (
                    "from pathlib import Path; "
                    f"Path({str(output)!r}).write_bytes(b'x' * 65536)"
                )
                parent = (
                    "import subprocess; "
                    f"subprocess.run([{direct_interpreter!r}, "
                    f"'-c', {child!r}], check=True)"
                )
                measured = system_scale.run_measured(
                    [direct_interpreter, "-c", parent],
                    cwd=root,
                    env=dict(os.environ),
                    timeout_seconds=10,
                )
                self.assertTrue(measured["terminal_io_complete"])
                self.assertEqual(measured["exact_total_processes"], 2)
                self.assertEqual(measured["worker_process_bound"], 1)
                self.assertEqual(
                    measured["worker_process_bound_method"],
                    "cumulative-owned-processes-conservative-upper-bound",
                )
                self.assertEqual(
                    measured["exact_terminal_active_processes"], 0
                )
                self.assertGreater(
                    measured["process_read_transfer_bytes"]
                    + measured["process_write_transfer_bytes"],
                    0,
                )
                self.assertGreater(measured["cpu_seconds"], 0)

    def test_writer_sampler_reports_an_intrusive_conservative_bound(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            database = Path(temporary) / "projectatlas.db"
            blocker = sqlite3.connect(database)
            blocker.execute("CREATE TABLE proof(value INTEGER)")
            blocker.commit()
            blocker.execute("BEGIN IMMEDIATE")
            sampler = system_scale.SQLiteWriterAvailabilitySampler(database)
            sampler.start()
            deadline = time.monotonic() + 2
            while sampler.busy_observations == 0 and time.monotonic() < deadline:
                time.sleep(0.005)
            blocker.rollback()
            blocker.close()
            report = sampler.stop()
            self.assertGreater(report["busy_observations"], 0)
            self.assertTrue(report["intrusive"])
            self.assertGreaterEqual(
                report["maximum_busy_upper_bound_seconds"],
                report["maximum_probe_gap_seconds"],
            )


if __name__ == "__main__":
    unittest.main()
