#!/usr/bin/env python3
"""Run the preregistered ProjectAtlas v0.4 system-scale matrix."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
import re
import shutil
import sqlite3
import statistics
import subprocess
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any, Callable

import psutil

from mcp_composition import (
    FIXTURES,
    McpClient,
    WindowsJob,
    clear_git_repository_environment,
    remove_tree,
    spawn_owned_process,
    terminate_owned_process,
)


ROOT = Path(__file__).resolve().parents[3]
DEFAULT_PREREGISTRATION = ROOT / "docs/benchmarks/v0.4-system-scale-preregistration.json"
DEFAULT_WORK = ROOT / "target/benchmarks/system-scale/current"
DEFAULT_OUTPUT = ROOT / "docs/benchmarks/v0.4-system-scale-raw.json"
DEFAULT_CORPUS_CACHE = ROOT / "target/benchmarks/system-scale/corpus-cache"
MEASURE_INTERVAL_SECONDS = 0.02
WRITER_PROBE_INTERVAL_SECONDS = 0.005
WATCH_BASELINE_STABLE_SECONDS = 0.1
WATCH_BASELINE_TIMEOUT_SECONDS = 5.0
TOON_INTEGER = r"^\s+{key}: (\d+)$"
TOON_SCALAR = r"^\s+{key}: (.+)$"
GRAPH_STAGE_PREFIX = "graph-stage-"
POWERSHELL = shutil.which("pwsh")
PATH_PLACEHOLDERS = {
    "{REPO_ROOT}": "candidate repository root",
    "{USER_HOME}": "current operating-system user home",
    "{POWERSHELL}": "PowerShell executable",
}
SYSTEM_SCALE_MEASUREMENT_INPUTS = (
    "docs/benchmarks/harness/system_scale.py",
    "docs/benchmarks/harness/mcp_composition.py",
    "docs/benchmarks/harness/requirements.txt",
    "docs/benchmarks/fixtures/mcp-composition",
)


def committed_git_object_sha256(
    relative: str,
    *,
    root: Path | None = None,
    revision: str = "HEAD",
) -> str | None:
    """Return the SHA-256 of one canonical committed Git blob or tree."""

    root = ROOT if root is None else root
    for object_type in ("blob", "tree"):
        process = subprocess.run(
            ["git", "cat-file", object_type, f"{revision}:{relative}"],
            cwd=root,
            check=False,
            capture_output=True,
            timeout=120,
        )
        if process.returncode == 0:
            return hashlib.sha256(process.stdout).hexdigest()
    return None


def measurement_input_errors(
    preregistration: dict[str, Any],
    required_paths: tuple[str, ...],
    *,
    root: Path | None = None,
    revision: str = "HEAD",
) -> list[str]:
    """Validate one closed content lock over canonical committed harness inputs."""

    root = ROOT if root is None else root
    locked = preregistration.get("measurement_inputs")
    if not isinstance(locked, dict) or set(locked) != set(required_paths):
        return ["measurement input lock does not match the required path set"]
    if revision != "HEAD" and not re.fullmatch(r"[0-9a-f]{40}", revision):
        return ["measurement input revision is malformed"]
    errors = []
    for relative in required_paths:
        expected = locked.get(relative)
        if (
            not isinstance(expected, str)
            or len(expected) != 64
            or any(character not in "0123456789abcdef" for character in expected)
        ):
            errors.append(f"measurement input digest is malformed: {relative}")
            continue
        path = (root / relative).resolve()
        try:
            path.relative_to(root.resolve())
        except ValueError:
            errors.append(f"measurement input escapes the repository: {relative}")
            continue
        actual = committed_git_object_sha256(relative, root=root, revision=revision)
        if actual is None:
            errors.append(f"measurement input is missing from {revision}: {relative}")
            continue
        if actual != expected:
            errors.append(f"measurement input changed after lock: {relative}")
    return errors


def candidate_file_identity(
    relative: str, *, root: Path = ROOT
) -> dict[str, str | int]:
    path = (root / relative).resolve()
    try:
        canonical_relative = path.relative_to(root.resolve()).as_posix()
    except ValueError as error:
        raise ValueError("candidate artifact path escapes the repository") from error
    if not path.is_file():
        raise ValueError("candidate artifact path is not a regular file")
    payload = path.read_bytes()
    return {
        "path": canonical_relative,
        "sha256": hashlib.sha256(payload).hexdigest(),
        "bytes": len(payload),
    }


def redact_local_paths(value: Any) -> Any:
    if isinstance(value, dict):
        return {key: redact_local_paths(item) for key, item in value.items()}
    if isinstance(value, list):
        return [redact_local_paths(item) for item in value]
    if not isinstance(value, str):
        return value
    replacements = [(ROOT, "{REPO_ROOT}"), (Path.home(), "{USER_HOME}")]
    if POWERSHELL:
        replacements.append((Path(POWERSHELL), "{POWERSHELL}"))
    redacted = value
    for path, placeholder in sorted(
        replacements, key=lambda item: len(str(item[0])), reverse=True
    ):
        parts = re.split(r"[\\/]+", str(path))
        pattern = r"[\\/]+".join(re.escape(part) for part in parts)
        redacted = re.sub(pattern, placeholder, redacted, flags=re.IGNORECASE)
    return redacted


def command(*args: str, cwd: Path, env: dict[str, str] | None = None) -> None:
    subprocess.run(
        args,
        cwd=cwd,
        env=env,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def process_tree(root_pid: int) -> list[psutil.Process]:
    try:
        root = psutil.Process(root_pid)
        return [root, *root.children(recursive=True)]
    except (psutil.NoSuchProcess, psutil.ZombieProcess):
        return []


def process_tree_rss(root_pid: int) -> int:
    total = 0
    processes = process_tree(root_pid)
    if not processes:
        raise RuntimeError(f"process tree {root_pid} could not be observed")
    for process in processes:
        try:
            total += process.memory_info().rss
        except (psutil.NoSuchProcess, psutil.ZombieProcess):
            continue
    return total


def process_tree_io(root_pid: int) -> dict[str, int]:
    totals = {
        "read_count": 0,
        "write_count": 0,
        "read_bytes": 0,
        "write_bytes": 0,
    }
    processes = process_tree(root_pid)
    if not processes:
        raise RuntimeError(f"process tree {root_pid} could not be observed")
    for process in processes:
        try:
            counters = process.io_counters()
        except (psutil.NoSuchProcess, psutil.ZombieProcess):
            continue
        for key in totals:
            totals[key] += int(getattr(counters, key))
    return totals


def process_tree_state(root_pid: int) -> dict[str, Any]:
    pids: list[int] = []
    threads = 0
    processes = process_tree(root_pid)
    if not processes:
        raise RuntimeError(f"process tree {root_pid} could not be observed")
    for process in processes:
        try:
            pids.append(process.pid)
            threads += process.num_threads()
        except (psutil.NoSuchProcess, psutil.ZombieProcess):
            continue
    return {"pids": sorted(pids), "processes": len(pids), "threads": threads}


def file_size_or_zero(path: Path) -> int:
    try:
        return path.stat().st_size
    except FileNotFoundError:
        return 0


def storage_state(root: Path) -> dict[str, int]:
    atlas_root = root / ".projectatlas"
    database = atlas_root / "projectatlas.db"
    paths = {
        "database_bytes": database,
        "wal_bytes": Path(f"{database}-wal"),
        "shm_bytes": Path(f"{database}-shm"),
    }
    result = {name: file_size_or_zero(path) for name, path in paths.items()}
    stages = [
        path
        for path in atlas_root.glob(f"{GRAPH_STAGE_PREFIX}*")
        if path.is_dir()
    ]
    stage_bytes = 0
    for stage in stages:
        for name in ("projectatlas.db", "projectatlas.db-wal", "projectatlas.db-shm"):
            stage_bytes += file_size_or_zero(stage / name)
    result["staging_bytes"] = stage_bytes
    result["stage_directories"] = len(stages)
    return result


def measured_process_write_transfer_bytes(
    process_write_bytes: int, stdout_bytes: int, stderr_bytes: int
) -> int:
    """Exclude captured pipe output from Windows process I/O transfer counts."""
    if platform.system() == "Windows":
        captured_output_bytes = stdout_bytes + stderr_bytes
        if process_write_bytes < captured_output_bytes:
            raise RuntimeError(
                "Windows process I/O observation ended before captured output completed"
            )
        return process_write_bytes - captured_output_bytes
    return process_write_bytes


def process_io_measurement_contract() -> dict[str, Any]:
    host = platform.system()
    if host == "Windows":
        return {
            "backend": "windows-job-basic-and-io-accounting-plus-psutil-peaks",
            "kind": "exact-terminal-owned-process-tree-accounting",
            "required_final_platform": "Windows",
            "final_platform_eligible": True,
            "ineligible_reason": None,
        }
    return {
        "backend": f"psutil-{psutil.__version__}",
        "kind": "sampled-nonterminal-process-tree-transfer-counters",
        "required_final_platform": "Windows",
        "final_platform_eligible": False,
        "ineligible_reason": (
            f"{host} does not provide the complete terminal process counters "
            "required by this preregistration"
        ),
    }


def post_cancellation_read_is_safe(output: str) -> bool:
    """Accept a current overview or the required fail-closed freshness response."""
    return output.startswith("overview:") or (
        output.startswith("error:") and "\n  kind: refresh_required\n" in output
    )


def final_measurement_eligibility(mode: str) -> dict[str, Any]:
    contract = process_io_measurement_contract()
    requested = mode == "all"
    if not requested:
        disposition = "skipped_nonfinal_smoke"
    elif contract["final_platform_eligible"]:
        disposition = "eligible"
    else:
        disposition = "failed_ineligible_platform"
    return {**contract, "requested": requested, "disposition": disposition}


class ProcessTreeSampler:
    def __init__(
        self,
        root_pid: int,
        storage_root: Path | None = None,
        subtract_initial_work: bool = False,
    ) -> None:
        self.root_pid = root_pid
        self.storage_root = storage_root
        self.subtract_initial_work = subtract_initial_work
        self.stop_event = threading.Event()
        self.thread = threading.Thread(target=self._sample_until_stopped, daemon=True)
        self.peak_rss_bytes = 0
        self.peak_processes = 0
        self.peak_threads = 0
        self.peak_storage = {
            "database_bytes": 0,
            "wal_bytes": 0,
            "shm_bytes": 0,
            "staging_bytes": 0,
            "stage_directories": 0,
        }
        self.cpu_seconds: dict[int, float] = {}
        self.read_bytes: dict[int, int] = {}
        self.write_bytes: dict[int, int] = {}
        self.initial_cpu_seconds: dict[int, float] = {}
        self.initial_read_bytes: dict[int, int] = {}
        self.initial_write_bytes: dict[int, int] = {}
        self.observed_pids: set[int] = set()
        self.error: Exception | None = None

    def start(self) -> None:
        if self.subtract_initial_work and os.name != "nt":
            processes = process_tree(self.root_pid)
            if not processes:
                raise RuntimeError(
                    f"process tree {self.root_pid} exited before baseline observation"
                )
            for process in processes:
                try:
                    cpu = process.cpu_times()
                    io = process.io_counters()
                    self.observed_pids.add(process.pid)
                    self.initial_cpu_seconds[process.pid] = cpu.user + cpu.system
                    self.initial_read_bytes[process.pid] = io.read_bytes
                    self.initial_write_bytes[process.pid] = io.write_bytes
                except (psutil.NoSuchProcess, psutil.ZombieProcess):
                    continue
        self._sample()
        self.thread.start()

    def stop(self) -> dict[str, Any]:
        self.stop_event.set()
        self.thread.join(timeout=5)
        if self.thread.is_alive():
            raise RuntimeError("process sampler did not stop")
        if self.error is not None:
            raise RuntimeError("process sampler failed") from self.error
        if self.root_pid not in self.observed_pids:
            raise RuntimeError("process sampler never observed its root process")
        return {
            "sampler": f"psutil-{psutil.__version__}",
            "interval_seconds": MEASURE_INTERVAL_SECONDS,
            "sampled_peak_metrics": [
                "rss_bytes",
                "processes",
                "threads",
                "storage",
            ],
            "peak_rss_bytes": self.peak_rss_bytes,
            "peak_processes": self.peak_processes,
            "peak_worker_processes": max(0, self.peak_processes - 1),
            "peak_threads": self.peak_threads,
            "cpu_seconds": round(sum(self.cpu_seconds.values()), 6),
            "read_bytes": sum(self.read_bytes.values()),
            "write_bytes": sum(self.write_bytes.values()),
            "observed_pids": sorted(self.observed_pids),
            "peak_storage": self.peak_storage,
        }

    def _sample_until_stopped(self) -> None:
        while not self.stop_event.is_set():
            self._sample()
            self.stop_event.wait(MEASURE_INTERVAL_SECONDS)

    def _sample(self) -> None:
        try:
            processes = process_tree(self.root_pid)
        except psutil.Error as error:
            self.error = error
            self.stop_event.set()
            return
        rss = 0
        threads = 0
        for process in processes:
            try:
                self.observed_pids.add(process.pid)
                rss += process.memory_info().rss
                threads += process.num_threads()
                if os.name != "nt":
                    cpu = process.cpu_times()
                    self.cpu_seconds[process.pid] = max(
                        self.cpu_seconds.get(process.pid, 0.0),
                        max(
                            0.0,
                            cpu.user
                            + cpu.system
                            - self.initial_cpu_seconds.get(process.pid, 0.0),
                        ),
                    )
                    io = process.io_counters()
                    self.read_bytes[process.pid] = max(
                        self.read_bytes.get(process.pid, 0),
                        max(
                            0,
                            io.read_bytes - self.initial_read_bytes.get(process.pid, 0),
                        ),
                    )
                    self.write_bytes[process.pid] = max(
                        self.write_bytes.get(process.pid, 0),
                        max(
                            0,
                            io.write_bytes - self.initial_write_bytes.get(process.pid, 0),
                        ),
                    )
            except (psutil.NoSuchProcess, psutil.ZombieProcess):
                continue
            except psutil.Error as error:
                self.error = error
                self.stop_event.set()
                return
        self.peak_rss_bytes = max(self.peak_rss_bytes, rss)
        self.peak_processes = max(self.peak_processes, len(processes))
        self.peak_threads = max(self.peak_threads, threads)
        if self.storage_root is not None:
            try:
                storage = storage_state(self.storage_root)
            except OSError as error:
                self.error = error
                self.stop_event.set()
                return
            for key, value in storage.items():
                self.peak_storage[key] = max(self.peak_storage[key], value)


class SQLiteWriterAvailabilitySampler:
    def __init__(self, database: Path) -> None:
        self.database = database
        self.stop_event = threading.Event()
        self.thread = threading.Thread(target=self._sample_until_stopped, daemon=True)
        self.attempts = 0
        self.busy_observations = 0
        self.maximum_busy_upper_bound_seconds = 0.0
        self.maximum_probe_gap_seconds = 0.0
        self.busy_window_started: float | None = None
        self.last_attempt_ended: float | None = None
        self.error: Exception | None = None

    def start(self) -> None:
        self.thread.start()

    def stop(self) -> dict[str, Any]:
        self.stop_event.set()
        self.thread.join(timeout=5)
        if self.thread.is_alive():
            raise RuntimeError("SQLite writer sampler did not stop")
        if self.error is not None:
            raise RuntimeError("SQLite writer sampler failed") from self.error
        if self.busy_window_started is not None:
            self.maximum_busy_upper_bound_seconds = max(
                self.maximum_busy_upper_bound_seconds,
                time.perf_counter() - self.busy_window_started,
            )
        return {
            "method": "timeout-zero-begin-immediate-sampled-bound",
            "intrusive": True,
            "interval_seconds": WRITER_PROBE_INTERVAL_SECONDS,
            "attempts": self.attempts,
            "busy_observations": self.busy_observations,
            "maximum_probe_gap_seconds": round(
                self.maximum_probe_gap_seconds, 6
            ),
            "maximum_busy_upper_bound_seconds": round(
                max(
                    self.maximum_busy_upper_bound_seconds,
                    self.maximum_probe_gap_seconds,
                ),
                6,
            ),
        }

    def _sample_until_stopped(self) -> None:
        while not self.stop_event.is_set():
            attempt_started = time.perf_counter()
            if self.last_attempt_ended is not None:
                self.maximum_probe_gap_seconds = max(
                    self.maximum_probe_gap_seconds,
                    attempt_started - self.last_attempt_ended,
                )
            busy = False
            if self.database.exists():
                try:
                    connection = sqlite3.connect(self.database, timeout=0)
                    try:
                        connection.execute("PRAGMA busy_timeout=0")
                        connection.execute("BEGIN IMMEDIATE")
                        connection.rollback()
                        self.attempts += 1
                    finally:
                        connection.close()
                except sqlite3.OperationalError as error:
                    if "locked" not in str(error).lower() and "busy" not in str(error).lower():
                        self.error = error
                        self.stop_event.set()
                        return
                    self.attempts += 1
                    self.busy_observations += 1
                    busy = True
            attempt_ended = time.perf_counter()
            if busy:
                self.busy_window_started = (
                    self.busy_window_started
                    or self.last_attempt_ended
                    or attempt_started
                )
            elif self.busy_window_started is not None:
                self.maximum_busy_upper_bound_seconds = max(
                    self.maximum_busy_upper_bound_seconds,
                    attempt_ended - self.busy_window_started,
                )
                self.busy_window_started = None
            self.last_attempt_ended = attempt_ended
            self.stop_event.wait(WRITER_PROBE_INTERVAL_SECONDS)


def terminate_process_tree(process: subprocess.Popen[bytes]) -> None:
    members = process_tree(process.pid)
    for member in reversed(members[1:]):
        try:
            member.kill()
        except psutil.Error:
            continue
    if process.poll() is None:
        process.kill()


def collect_measured_process(
    process: subprocess.Popen[bytes],
    arguments: list[str],
    *,
    cwd: Path,
    timeout_seconds: float,
    started: float,
    writer_probe_database: Path | None = None,
    subtract_initial_work: bool = False,
    start_action: Callable[[], None] | None = None,
    job: WindowsJob | None = None,
    resume_suspended: bool = False,
    exact_baseline: dict[str, int] | None = None,
) -> dict[str, Any]:
    sampler = ProcessTreeSampler(process.pid, cwd, subtract_initial_work)
    writer_sampler = (
        SQLiteWriterAvailabilitySampler(writer_probe_database)
        if writer_probe_database is not None
        else None
    )
    sampler_started = False
    writer_started = False
    action_error: Exception | None = None
    timed_out = False
    exact_accounting: dict[str, int] | None = None
    drain_error: BaseException | None = None
    sampler_error: Exception | None = None
    writer_error: Exception | None = None
    metrics: dict[str, Any] = {}
    try:
        sampler.start()
        sampler_started = True
        if writer_sampler is not None:
            writer_sampler.start()
            writer_started = True
        if resume_suspended:
            if job is None:
                raise RuntimeError("suspended measured process has no Windows Job")
            job.resume()
        if start_action is not None:
            try:
                start_action()
            except Exception as error:
                action_error = error
                if job is not None:
                    job.terminate()
                else:
                    terminate_process_tree(process)
        try:
            stdout, stderr = process.communicate(timeout=timeout_seconds)
        except subprocess.TimeoutExpired:
            timed_out = True
            if job is not None:
                job.terminate()
            else:
                terminate_process_tree(process)
            stdout, stderr = process.communicate(timeout=5)
        elapsed = time.perf_counter() - started
        if job is not None:
            try:
                drain_timeout = (
                    5 if timed_out else max(0, timeout_seconds - elapsed)
                )
                exact_accounting = job.wait_for_zero_active(drain_timeout)
            except BaseException as error:
                drain_error = error
                try:
                    job.terminate()
                    job.wait_for_zero_active(5)
                except BaseException as cleanup_error:
                    error.add_note(f"Windows Job cleanup failed: {cleanup_error}")
    finally:
        if sampler_started:
            try:
                metrics = sampler.stop()
            except Exception as error:
                sampler_error = error
        if writer_sampler is not None and writer_started:
            try:
                metrics["writer_availability"] = writer_sampler.stop()
            except Exception as error:
                writer_error = error
        if job is not None:
            job.close()
    if action_error is not None:
        raise RuntimeError("measured process start action failed") from action_error
    if drain_error is not None:
        raise RuntimeError(
            "owned Windows Job did not drain before terminal accounting"
        ) from drain_error
    if sampler_error is not None:
        raise sampler_error
    if writer_error is not None:
        raise writer_error
    if exact_accounting is not None:
        baseline = exact_baseline or {
            key: 0 for key in exact_accounting
        }
        metrics["cpu_seconds"] = round(
            (
                exact_accounting["user_time_100ns"]
                + exact_accounting["kernel_time_100ns"]
                - baseline["user_time_100ns"]
                - baseline["kernel_time_100ns"]
            )
            / 10_000_000,
            6,
        )
        for key in ("read_count", "write_count", "read_bytes", "write_bytes"):
            metrics[key] = max(0, exact_accounting[key] - baseline[key])
        metrics["exact_total_processes"] = (
            exact_accounting["total_processes"]
            - baseline["total_processes"]
            + baseline["active_processes"]
        )
        metrics["exact_terminal_active_processes"] = exact_accounting[
            "active_processes"
        ]
    else:
        metrics["exact_total_processes"] = None
        metrics["exact_terminal_active_processes"] = None
    exact_worker_bound = (
        max(0, metrics["exact_total_processes"] - 1)
        if metrics["exact_total_processes"] is not None
        else 0
    )
    metrics["worker_process_bound"] = max(
        metrics["peak_worker_processes"], exact_worker_bound
    )
    metrics["worker_process_bound_method"] = (
        "cumulative-owned-processes-conservative-upper-bound"
        if exact_accounting is not None
        else "sampled-peak"
    )
    metrics["terminal_io_complete"] = (
        exact_accounting is not None
        and exact_accounting["active_processes"] == 0
    )
    metrics["terminal_io_method"] = (
        "windows-job-basic-and-io-accounting"
        if exact_accounting is not None
        else "live-process-sampling"
    )
    metrics["process_io_metric"] = {
        **process_io_measurement_contract(),
        "terminal_counters_complete": metrics["terminal_io_complete"],
    }
    stdout_bytes = len(stdout)
    stderr_bytes = len(stderr)
    logical_cpus = os.cpu_count() or 1
    process_read_transfer_bytes = metrics.pop("read_bytes")
    process_write_transfer_bytes = measured_process_write_transfer_bytes(
        metrics.pop("write_bytes"), stdout_bytes, stderr_bytes
    )
    metrics.update(
        {
            "arguments": arguments[1:],
            "returncode": process.returncode,
            "timed_out": timed_out,
            "wall_seconds": round(elapsed, 6),
            "one_core_cpu_percent": round(
                metrics["cpu_seconds"] / elapsed * 100 if elapsed else 0.0, 3
            ),
            "host_cpu_percent": round(
                metrics["cpu_seconds"] / elapsed / logical_cpus * 100
                if elapsed
                else 0.0,
                3,
            ),
            "stdout_bytes": stdout_bytes,
            "stderr_bytes": stderr_bytes,
            "process_read_transfer_bytes": process_read_transfer_bytes,
            "process_write_transfer_bytes": process_write_transfer_bytes,
            "stdout": stdout.decode("utf-8", errors="replace"),
            "stderr": stderr.decode("utf-8", errors="replace"),
        }
    )
    return metrics


def run_measured(
    arguments: list[str],
    *,
    cwd: Path,
    env: dict[str, str],
    timeout_seconds: float,
    writer_probe_database: Path | None = None,
) -> dict[str, Any]:
    started = time.perf_counter()
    process, job = spawn_owned_process(
        arguments,
        cwd=cwd,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        return collect_measured_process(
            process,
            arguments,
            cwd=cwd,
            timeout_seconds=timeout_seconds,
            started=started,
            writer_probe_database=writer_probe_database,
            job=job,
            resume_suspended=job is not None,
        )
    except BaseException as error:
        try:
            terminate_owned_process(process, job)
        except BaseException as cleanup_error:
            error.add_note(f"measured process cleanup failed: {cleanup_error}")
        raise


def wait_for_indexed_marker(
    process: subprocess.Popen[bytes],
    job: WindowsJob | None,
    database: Path,
    path: str,
    marker: str,
    timeout_seconds: float,
) -> float:
    started = time.perf_counter()
    deadline = started + min(timeout_seconds, 30.0)
    while time.perf_counter() < deadline:
        if process.poll() is not None:
            terminate_owned_process(process, job)
            stdout, stderr = process.communicate(timeout=5)
            raise RuntimeError(
                "watch exited before publishing its readiness marker: "
                f"{stdout.decode(errors='replace')}{stderr.decode(errors='replace')}"
            )
        try:
            connection = sqlite3.connect(
                f"{database.resolve().as_uri()}?mode=ro", uri=True, timeout=0
            )
            try:
                row = connection.execute(
                    "SELECT content FROM file_texts WHERE path = ?1", (path,)
                ).fetchone()
            finally:
                connection.close()
        except sqlite3.OperationalError as error:
            if "locked" not in str(error).lower() and "busy" not in str(error).lower():
                raise
            row = None
        if row is not None and marker in str(row[0]):
            return time.perf_counter() - started
        time.sleep(MEASURE_INTERVAL_SECONDS)
    terminate_owned_process(process, job)
    stdout, stderr = process.communicate(timeout=5)
    raise RuntimeError(
        "watch did not publish the indexed readiness marker: "
        f"{stdout.decode(errors='replace')}{stderr.decode(errors='replace')}"
    )


def wait_for_idle_watch_baseline(job: WindowsJob) -> dict[str, int]:
    """Return accounting only after the ready watcher has stopped changing."""
    deadline = time.monotonic() + WATCH_BASELINE_TIMEOUT_SECONDS
    previous = job.accounting()
    stable_since = time.monotonic()
    while True:
        if previous["active_processes"] != 1:
            raise RuntimeError(
                "ready watcher did not retain exactly one active process"
            )
        time.sleep(MEASURE_INTERVAL_SECONDS)
        current = job.accounting()
        if current == previous:
            if time.monotonic() - stable_since >= WATCH_BASELINE_STABLE_SECONDS:
                return current
        else:
            previous = current
            stable_since = time.monotonic()
        if time.monotonic() >= deadline:
            raise TimeoutError("ready watcher accounting did not become idle")


def measured_json(
    runtime: Path,
    arguments: list[str],
    *,
    cwd: Path,
    env: dict[str, str],
    timeout_seconds: float,
) -> tuple[dict[str, Any], dict[str, Any]]:
    run = run_measured(
        [str(runtime), "--require-version", "0.4.0", "--format", "json", *arguments],
        cwd=cwd,
        env=env,
        timeout_seconds=timeout_seconds,
    )
    if run["returncode"] != 0:
        raise RuntimeError(
            f"{' '.join(arguments)} failed ({run['returncode']}): {run['stderr']}"
        )
    payload = json.loads(run.pop("stdout"))
    return run, payload


def measured_watch_edit(
    runtime: Path,
    *,
    cwd: Path,
    env: dict[str, str],
    timeout_seconds: float,
    edit: Callable[[], None],
    readiness_file: Path,
    writer_probe_database: Path,
    expected_refresh_reason: str | None = None,
) -> tuple[dict[str, Any], dict[str, Any]]:
    readiness_path = readiness_file.relative_to(cwd).as_posix()
    readiness_marker = f"projectatlas-watch-ready-{time.time_ns()}"
    with readiness_file.open("a", encoding="utf-8", newline="\n") as stream:
        stream.write(f"// {readiness_marker}\n")
    arguments = [
        str(runtime),
        "--require-version",
        "0.4.0",
        "--format",
        "json",
        "watch",
        ".",
        "--poll-seconds",
        "1",
        "--max-cycles",
        "2",
    ]
    process, job = spawn_owned_process(
        arguments,
        cwd=cwd,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        if job is not None:
            job.resume()
        readiness_seconds = wait_for_indexed_marker(
            process,
            job,
            writer_probe_database,
            readiness_path,
            readiness_marker,
            timeout_seconds,
        )
        readiness_generation = database_counts(writer_probe_database)["generation"]
        exact_baseline = (
            wait_for_idle_watch_baseline(job) if job is not None else None
        )
        edit_started = time.perf_counter()
        metrics = collect_measured_process(
            process,
            arguments,
            cwd=cwd,
            timeout_seconds=timeout_seconds,
            started=edit_started,
            writer_probe_database=writer_probe_database,
            subtract_initial_work=True,
            start_action=edit,
            job=job,
            exact_baseline=exact_baseline,
        )
    except BaseException as error:
        try:
            terminate_owned_process(process, job)
        except BaseException as cleanup_error:
            error.add_note(f"watch process cleanup failed: {cleanup_error}")
        raise
    metrics["readiness_seconds"] = round(readiness_seconds, 6)
    metrics["readiness_path"] = readiness_path
    metrics["readiness_generation"] = readiness_generation
    metrics["edit_to_complete_seconds"] = metrics["wall_seconds"]
    if expected_refresh_reason is not None:
        if metrics["returncode"] != 1:
            raise RuntimeError(
                "watch did not return the expected refresh guidance: "
                f"{metrics['stdout']}{metrics['stderr']}"
            )
        payload = json.loads(metrics["stderr"])
        error = payload.get("error", {})
        refresh = error.get("refresh_required", {})
        if (
            error.get("kind") != "refresh_required"
            or refresh.get("reason") != expected_refresh_reason
        ):
            raise RuntimeError(
                "watch returned different refresh guidance: "
                f"{metrics['stdout']}{metrics['stderr']}"
            )
        metrics.pop("stdout")
        metrics.pop("stderr")
        return metrics, refresh
    if metrics["returncode"] != 0:
        raise RuntimeError(f"watch failed ({metrics['returncode']}): {metrics['stderr']}")
    return metrics, json.loads(metrics.pop("stdout"))


def git_commit_fixture(path: Path) -> None:
    command("git", "init", "-q", cwd=path)
    command("git", "config", "user.name", "ProjectAtlas Benchmark", cwd=path)
    command(
        "git",
        "config",
        "user.email",
        "benchmark@projectatlas.invalid",
        cwd=path,
    )
    command("git", "add", ".", cwd=path)
    command("git", "commit", "-q", "-m", "benchmark fixture", cwd=path)


def prepare_small(work_root: Path) -> dict[str, Path]:
    prepared: dict[str, Path] = {}
    for name in ("clean", "dirty", "non-git"):
        source = FIXTURES / name
        destination = work_root / f"small-{name}"
        shutil.copytree(source, destination, ignore=shutil.ignore_patterns("current"))
        if name != "non-git":
            git_commit_fixture(destination)
        if name == "dirty":
            shutil.copy2(source / "current/pricing.rs", destination / "src/pricing.rs")
        prepared[name] = destination
    return prepared


def prepare_medium(destination: Path, caller_files: int) -> None:
    source = destination / "src"
    source.mkdir(parents=True)
    (destination / ".gitignore").write_text(
        "/.projectatlas/\n", encoding="utf-8", newline="\n"
    )
    (destination / "Cargo.toml").write_text(
        '[package]\nname = "projectatlas-scale-medium"\nversion = "0.0.0"\n'
        'edition = "2024"\npublish = false\n',
        encoding="utf-8",
        newline="\n",
    )
    modules = ["pub mod hub;"]
    for index in range(caller_files):
        name = f"caller_{index:04d}"
        modules.append(f"pub mod {name};")
        (source / f"{name}.rs").write_text(
            "use crate::hub::shared;\n"
            f"pub fn call_{index:04d}(value: u64) -> u64 {{ shared(value) }}\n",
            encoding="utf-8",
            newline="\n",
        )
    (source / "lib.rs").write_text(
        "\n".join(modules) + "\n", encoding="utf-8", newline="\n"
    )
    (source / "hub.rs").write_text(
        "pub fn shared(value: u64) -> u64 { value + 1 }\n",
        encoding="utf-8",
        newline="\n",
    )
    git_commit_fixture(destination)


def cached_corpus_repository(cache_root: Path, corpus: dict[str, Any]) -> Path:
    repository_name = corpus["repository"].rstrip("/").rsplit("/", 1)[-1]
    repository_name = re.sub(r"[^A-Za-z0-9._-]+", "-", repository_name.removesuffix(".git"))
    cache = (cache_root / f"{repository_name}-{corpus['commit']}.git").resolve()
    if not cache.exists():
        cache.mkdir(parents=True)
        command("git", "init", "--bare", "-q", cwd=cache)
    try:
        command("git", "cat-file", "-e", f"{corpus['commit']}^{{commit}}", cwd=cache)
    except subprocess.CalledProcessError:
        command(
            "git",
            "fetch",
            "-q",
            "--depth",
            "1",
            corpus["repository"],
            corpus["commit"],
            cwd=cache,
        )
        command("git", "cat-file", "-e", f"{corpus['commit']}^{{commit}}", cwd=cache)
    return cache


def prepare_huge(
    destination: Path, corpus: dict[str, Any], cache_root: Path
) -> None:
    cache = cached_corpus_repository(cache_root, corpus)
    destination.mkdir(parents=True)
    command("git", "init", "-q", cwd=destination)
    if os.name == "nt":
        command("git", "config", "core.longpaths", "true", cwd=destination)
    command(
        "git",
        "fetch",
        "-q",
        str(cache),
        corpus["commit"],
        cwd=destination,
    )
    checkout_env = os.environ.copy()
    checkout_env["GIT_LFS_SKIP_SMUDGE"] = "1"
    command(
        "git",
        "checkout",
        "-q",
        "--detach",
        corpus["commit"],
        cwd=destination,
        env=checkout_env,
    )
    actual = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=destination, text=True
    ).strip()
    if actual != corpus["commit"]:
        raise RuntimeError(f"huge corpus resolved to {actual}, expected {corpus['commit']}")
    status = subprocess.check_output(
        ["git", "status", "--porcelain"], cwd=destination, text=True
    )
    if status:
        raise RuntimeError(f"huge corpus checkout is incomplete:\n{status}")
    info_exclude = destination / ".git/info/exclude"
    with info_exclude.open("a", encoding="utf-8", newline="\n") as stream:
        stream.write("\n.projectatlas/\n")


def git_files(root: Path) -> list[Path]:
    if (root / ".git").exists():
        output = subprocess.check_output(
            ["git", "ls-files", "-co", "--exclude-standard", "-z"], cwd=root
        )
        return [
            root / value.decode("utf-8")
            for value in output.split(b"\0")
            if value
        ]
    files = []
    for current, directories, names in os.walk(root):
        directories[:] = [
            name
            for name in directories
            if name not in {".git", ".projectatlas", "node_modules", "target"}
        ]
        files.extend(Path(current) / name for name in names)
    return files


def read_file_bytes(path: Path) -> bytes:
    if os.name == "nt":
        with open(f"\\\\?\\{path.resolve()}", "rb") as stream:
            return stream.read()
    return path.read_bytes()


def clean_git_corpus_facts(root: Path) -> dict[str, Any] | None:
    status = subprocess.check_output(
        ["git", "status", "--porcelain", "--untracked-files=all"], cwd=root
    )
    if status:
        return None
    manifest = subprocess.check_output(
        ["git", "ls-tree", "-r", "-l", "-z", "HEAD"], cwd=root
    )
    files = 0
    total_bytes = 0
    for row in manifest.split(b"\0"):
        if not row:
            continue
        metadata, _path = row.split(b"\t", 1)
        _mode, kind, _object_id, size = metadata.split()
        if kind != b"blob":
            continue
        files += 1
        total_bytes += int(size)
    return {
        "files": files,
        "bytes": total_bytes,
        "identity_kind": "git-tree-manifest-sha256",
        "tree_sha256": hashlib.sha256(manifest).hexdigest(),
        "git_tree": subprocess.check_output(
            ["git", "rev-parse", "HEAD^{tree}"], cwd=root, text=True
        ).strip(),
    }


def corpus_facts(root: Path) -> dict[str, Any]:
    if (root / ".git").exists():
        clean_facts = clean_git_corpus_facts(root)
        if clean_facts is not None:
            return clean_facts
    files = git_files(root)
    digest = hashlib.sha256()
    total_bytes = 0
    for path in sorted(files):
        relative = path.relative_to(root).as_posix()
        content = read_file_bytes(path)
        total_bytes += len(content)
        digest.update(relative.encode("utf-8"))
        digest.update(b"\0")
        digest.update(hashlib.sha256(content).digest())
    return {
        "files": len(files),
        "bytes": total_bytes,
        "identity_kind": "live-content-sha256",
        "tree_sha256": digest.hexdigest(),
    }


def persistent_sizes(root: Path) -> dict[str, int]:
    result = storage_state(root)
    result["total_bytes"] = sum(
        result[key]
        for key in ("database_bytes", "wal_bytes", "shm_bytes", "staging_bytes")
    )
    return result


def io_transfer_ratio(
    process_transfer_bytes: int,
    admitted_source_bytes: int,
    pre_run_database_bytes: int,
) -> float:
    return process_transfer_bytes / max(
        1, admitted_source_bytes + pre_run_database_bytes
    )


def toon_integer(text: str, key: str) -> int | None:
    match = re.search(TOON_INTEGER.format(key=re.escape(key)), text, re.MULTILINE)
    return int(match.group(1)) if match else None


def toon_scalar(text: str, key: str) -> str | None:
    match = re.search(TOON_SCALAR.format(key=re.escape(key)), text, re.MULTILINE)
    if match is None:
        return None
    value = match.group(1).strip()
    if value.startswith('"'):
        return str(json.loads(value))
    return value


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


def mcp_queries(
    runtime: Path,
    root: Path,
    env: dict[str, str],
    query: dict[str, Any],
    request_timeout_seconds: float,
) -> dict[str, Any]:
    client = McpClient(
        runtime,
        root,
        env,
        request_timeout_seconds=request_timeout_seconds,
    )
    sampler = ProcessTreeSampler(client.process.pid, root)
    sampler_started = False
    calls: list[dict[str, Any]] = []

    def call(tool: str, arguments: dict[str, Any], phase: str) -> str:
        io_before = process_tree_io(client.process.pid)
        text, elapsed_ms = client.call(tool, arguments)
        io_after = process_tree_io(client.process.pid)
        calls.append(
            {
                "tool": tool,
                "phase": phase,
                "arguments": arguments,
                "elapsed_ms": round(elapsed_ms, 3),
                "output_bytes": len(text.encode("utf-8")),
                "process_io": {
                    "read_operations": max(
                        0, io_after["read_count"] - io_before["read_count"]
                    ),
                    "write_operations": max(
                        0, io_after["write_count"] - io_before["write_count"]
                    ),
                    "read_transfer_bytes": max(
                        0, io_after["read_bytes"] - io_before["read_bytes"]
                    ),
                    "write_transfer_bytes": max(
                        0, io_after["write_bytes"] - io_before["write_bytes"]
                    ),
                },
                "work": {
                    key: toon_integer(text, key)
                    for key in (
                        "returned_rows",
                        "inspected_edges",
                        "active_nodes",
                        "visited_nodes",
                        "database_requested_rows",
                        "database_returned_rows",
                        "database_decoded_bytes",
                        "hydrated_entities",
                        "hydrated_purpose_paths",
                        "searched_files",
                        "searched_bytes",
                        "retained_bytes",
                        "rendered_output_bytes",
                    )
                },
            }
        )
        return text

    try:
        sampler.start()
        sampler_started = True
        call("atlas_overview", {}, "first_exact_verification")
        publication_before = database_publication_state(
            root / ".projectatlas/projectatlas.db"
        )
        rss_before = process_tree_rss(client.process.pid)
        state_before = process_tree_state(client.process.pid)
        requests = [
            ("atlas_overview", {}),
            (
                "atlas_file_summary",
                {"file": query["target_file"], "limit": 25},
            ),
            (
                "atlas_symbol_relations",
                {
                    "file": query["target_file"],
                    "view": "detailed",
                    "direction": "inbound",
                    "compact": True,
                    "limit": 20,
                    "output_bytes": 65536,
                },
            ),
            (
                "atlas_search",
                {
                    "pattern": query["literal"],
                    "file_pattern": query["file_pattern"],
                    "context_lines": 0,
                    "limit": 20,
                },
            ),
            (
                "atlas_search",
                {
                    "pattern": query["regex"],
                    "regex": True,
                    "file_pattern": query["file_pattern"],
                    "context_lines": 0,
                    "limit": 20,
                },
            ),
        ]
        for repetition in range(5):
            for tool, arguments in requests:
                call(tool, arguments, f"repeated_query_{repetition + 1}")
        rss_after = process_tree_rss(client.process.pid)
        state_after = process_tree_state(client.process.pid)
        publication_after = database_publication_state(
            root / ".projectatlas/projectatlas.db"
        )
    finally:
        try:
            if sampler_started:
                process_metrics = sampler.stop()
        finally:
            client.close()

    repeated_latencies = [
        row["elapsed_ms"]
        for row in calls
        if row["phase"].startswith("repeated_query_")
    ]
    return {
        "startup_ms": round(client.startup_ms, 3),
        "rss_before_repeated_bytes": rss_before,
        "rss_after_repeated_bytes": rss_after,
        "retained_rss_growth_bytes": rss_after - rss_before,
        "state_before_repeated": state_before,
        "state_after_repeated": state_after,
        "retained_thread_growth": state_after["threads"] - state_before["threads"],
        "retained_child_process_growth": (
            state_after["processes"] - state_before["processes"]
        ),
        "stable_publication": {
            "handshake": "first-successful-atlas_overview",
            "before": publication_before,
            "after": publication_after,
            "stable": publication_after == publication_before,
        },
        "repeated_query_median_ms": round(statistics.median(repeated_latencies), 3),
        "repeated_query_p95_ms": round(percentile(repeated_latencies, 0.95), 3),
        "maximum_output_bytes": max(row["output_bytes"] for row in calls),
        "process": process_metrics,
        "calls": calls,
    }


def database_publication_state(database: Path) -> dict[str, Any]:
    connection = sqlite3.connect(
        f"{database.resolve().as_uri()}?mode=ro", uri=True
    )
    try:
        connection.execute("PRAGMA query_only=ON")
        values = dict(
            connection.execute(
                "SELECT key, value FROM metadata WHERE key IN "
                "('index_publication_state', 'index_publication_fingerprint', "
                "'index_publication_generation', 'purpose.authored_revision')"
            )
        )
        return {
            "state": values.get("index_publication_state"),
            "contract_fingerprint": values.get("index_publication_fingerprint"),
            "generation": int(values.get("index_publication_generation", "0")),
            "authored_purpose_revision": int(
                values.get("purpose.authored_revision", "0")
            ),
        }
    finally:
        connection.close()


def database_writer_available(database: Path) -> bool:
    connection = sqlite3.connect(database, timeout=0)
    try:
        connection.execute("PRAGMA busy_timeout=0")
        connection.execute("BEGIN IMMEDIATE")
        connection.rollback()
        return True
    except sqlite3.OperationalError as error:
        if "locked" in str(error).lower() or "busy" in str(error).lower():
            return False
        raise
    finally:
        connection.close()


def database_counts(database: Path) -> dict[str, int]:
    connection = sqlite3.connect(database)
    try:
        tables = (
            "nodes",
            "symbols",
            "symbol_relations",
            "graph_entities",
            "graph_relations",
            "graph_relation_occurrences",
            "graph_resolution_keys",
            "graph_relation_dependencies",
            "graph_coverage",
        )
        counts = {
            table: int(connection.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0])
            for table in tables
        }
        counts["generation"] = int(
            connection.execute(
                "SELECT value FROM metadata WHERE key = 'index_publication_generation'"
            ).fetchone()[0]
        )
        counts["relation_resolution"] = dict(
            connection.execute(
                "SELECT resolution_status, COUNT(*) "
                "FROM graph_relations GROUP BY resolution_status"
            )
        )
        return counts
    finally:
        connection.close()


def database_profile(database: Path) -> dict[str, Any]:
    connection = sqlite3.connect(f"file:{database.as_posix()}?mode=ro", uri=True)
    try:
        connection.execute("PRAGMA query_only=ON")
        page_size = int(connection.execute("PRAGMA page_size").fetchone()[0])
        page_count = int(connection.execute("PRAGMA page_count").fetchone()[0])
        freelist_pages = int(connection.execute("PRAGMA freelist_count").fetchone()[0])
        quick_check = str(connection.execute("PRAGMA quick_check(1)").fetchone()[0])
        stat1_present = (
            connection.execute(
                "SELECT COUNT(*) FROM sqlite_schema "
                "WHERE type = 'table' AND name = 'sqlite_stat1'"
            ).fetchone()[0]
            == 1
        )
        project_root = connection.execute(
            "SELECT value FROM metadata WHERE key = 'project_root'"
        ).fetchone()
        return {
            "page_size": page_size,
            "page_count": page_count,
            "page_bytes": page_size * page_count,
            "freelist_pages": freelist_pages,
            "freelist_ratio": (
                round(freelist_pages / page_count, 6) if page_count else 0.0
            ),
            "quick_check": quick_check,
            "sqlite_stat1_present": stat1_present,
            "project_root": project_root[0] if project_root else None,
        }
    finally:
        connection.close()


def run_incremental(
    runtime: Path,
    root: Path,
    env: dict[str, str],
    timeout_seconds: float,
) -> dict[str, Any]:
    database = root / ".projectatlas/projectatlas.db"
    before = database_counts(database)
    leaf = root / "src/caller_0000.rs"

    def narrow_edit() -> None:
        with leaf.open("a", encoding="utf-8", newline="\n") as stream:
            stream.write("// narrow edit\n")

    narrow_run, narrow = measured_watch_edit(
        runtime,
        cwd=root,
        env=env,
        timeout_seconds=timeout_seconds,
        edit=narrow_edit,
        readiness_file=root / "src/caller_0001.rs",
        writer_probe_database=database,
    )
    after_narrow = database_counts(database)
    hub = root / "src/hub.rs"

    def expanded_edit() -> None:
        hub.write_text(
            "pub fn shared_v2(value: u64) -> u64 { value + 1 }\n",
            encoding="utf-8",
            newline="\n",
        )

    expanded_run, expanded = measured_watch_edit(
        runtime,
        cwd=root,
        env=env,
        timeout_seconds=timeout_seconds,
        edit=expanded_edit,
        readiness_file=root / "src/caller_0002.rs",
        writer_probe_database=database,
        expected_refresh_reason="dependency_closure_limit",
    )
    after_guidance = database_counts(database)
    pre_rebuild_database_bytes = database.stat().st_size
    rebuild_run, rebuild = measured_json(
        runtime,
        ["scan", "."],
        cwd=root,
        env=env,
        timeout_seconds=timeout_seconds,
    )
    after_rebuild = database_counts(database)
    return {
        "before": before,
        "narrow": {
            "process": narrow_run,
            "report": narrow,
            "counts": after_narrow,
        },
        "expanded": {
            "guidance": {
                "process": expanded_run,
                "report": expanded,
                "counts": after_guidance,
            },
            "rebuild": {
                "pre_run_database_bytes": pre_rebuild_database_bytes,
                "process": rebuild_run,
                "report": rebuild,
                "counts": after_rebuild,
            },
        },
    }


def run_case(
    runtime: Path,
    root: Path,
    env: dict[str, str],
    *,
    scale: str,
    variant: str,
    preregistration: dict[str, Any],
    query: dict[str, Any],
    incremental: bool = False,
) -> dict[str, Any]:
    timeout_seconds = preregistration["thresholds"]["all"]["command_timeout_seconds"]
    mcp_request_timeout_seconds = preregistration["thresholds"]["all"][
        "mcp_request_timeout_seconds"
    ]
    facts = corpus_facts(root)
    pre_scan_database_bytes = storage_state(root)["database_bytes"]
    scan_run, scan = measured_json(
        runtime,
        ["scan", "."],
        cwd=root,
        env=env,
        timeout_seconds=timeout_seconds,
    )
    post_scan_database_bytes = storage_state(root)["database_bytes"]
    settings_run, settings = measured_json(
        runtime,
        ["settings"],
        cwd=root,
        env=env,
        timeout_seconds=timeout_seconds,
    )
    unchanged_run, unchanged = measured_json(
        runtime,
        ["watch", "--once", "."],
        cwd=root,
        env=env,
        timeout_seconds=timeout_seconds,
    )
    incremental_result = (
        run_incremental(
            runtime,
            root,
            env,
            timeout_seconds,
        )
        if incremental
        else None
    )
    database = root / ".projectatlas/projectatlas.db"
    result = {
        "scale": scale,
        "variant": variant,
        "root": str(root),
        "corpus": facts,
        "scan": {
            "pre_run_database_bytes": pre_scan_database_bytes,
            "post_run_database_bytes": post_scan_database_bytes,
            "process": scan_run,
            "report": scan,
        },
        "settings": {"process": settings_run, "report": settings},
        "unchanged_refresh": {"process": unchanged_run, "report": unchanged},
        "incremental": incremental_result,
        "persistent": persistent_sizes(root),
        "database_profile": database_profile(database),
        "queries": mcp_queries(
            runtime,
            root,
            env,
            query,
            request_timeout_seconds=mcp_request_timeout_seconds,
        ),
    }
    result["checks"] = evaluate_case(result, preregistration)
    return result


def evaluate_process_io_contract(
    result: dict[str, Any], preregistration: dict[str, Any]
) -> dict[str, Any]:
    """Calculate the evaluator's preregistered process-I/O operands and gates."""
    all_limits = preregistration["thresholds"]["all"]
    limits = preregistration["thresholds"][result["scale"]]
    scan = result["scan"]
    process = scan["process"]
    source_bytes = scan["report"]["text_index"]["bytes"]
    pre_scan_database_bytes = scan["pre_run_database_bytes"]
    post_scan_database_bytes = scan["post_run_database_bytes"]
    evaluation = {
        "full_source_input_read_ratio": io_transfer_ratio(
            process["process_read_transfer_bytes"],
            source_bytes,
            pre_scan_database_bytes,
        ),
        "full_source_input_write_ratio": io_transfer_ratio(
            process["process_write_transfer_bytes"],
            source_bytes,
            pre_scan_database_bytes,
        ),
        "full_output_efficiency_read_ratio": io_transfer_ratio(
            process["process_read_transfer_bytes"],
            source_bytes,
            post_scan_database_bytes,
        ),
        "full_output_efficiency_write_ratio": io_transfer_ratio(
            process["process_write_transfer_bytes"],
            source_bytes,
            post_scan_database_bytes,
        ),
        "full_read_transfer_within_absolute_cap": (
            process["process_read_transfer_bytes"]
            <= limits["maximum_full_process_read_transfer_bytes"]
        ),
        "full_write_transfer_within_absolute_cap": (
            process["process_write_transfer_bytes"]
            <= limits["maximum_full_process_write_transfer_bytes"]
        ),
    }
    if result.get("incremental") is not None:
        guidance = result["incremental"]["expanded"]["guidance"]["process"]
        rebuild = result["incremental"]["expanded"]["rebuild"]
        rebuild_source_bytes = rebuild["report"]["text_index"]["bytes"]
        rebuild_database_bytes = rebuild["pre_run_database_bytes"]
        evaluation.update(
            {
                "expanded_guidance_read_within_absolute_cap": (
                    guidance["process_read_transfer_bytes"]
                    <= all_limits[
                        "maximum_expanded_guidance_process_read_transfer_bytes"
                    ]
                ),
                "expanded_guidance_write_within_absolute_cap": (
                    guidance["process_write_transfer_bytes"]
                    <= all_limits[
                        "maximum_expanded_guidance_process_write_transfer_bytes"
                    ]
                ),
                "rebuild_input_efficiency_read_ratio": io_transfer_ratio(
                    rebuild["process"]["process_read_transfer_bytes"],
                    rebuild_source_bytes,
                    rebuild_database_bytes,
                ),
                "rebuild_input_efficiency_write_ratio": io_transfer_ratio(
                    rebuild["process"]["process_write_transfer_bytes"],
                    rebuild_source_bytes,
                    rebuild_database_bytes,
                ),
            }
        )
    return evaluation


def evaluate_case(
    result: dict[str, Any], preregistration: dict[str, Any]
) -> list[dict[str, Any]]:
    all_limits = preregistration["thresholds"]["all"]
    limits = preregistration["thresholds"][result["scale"]]
    report = result["scan"]["report"]
    scan_process = result["scan"]["process"]
    unchanged_process = result["unchanged_refresh"]["process"]
    indexed_files = result["settings"]["report"]["index"]["files"]
    settings = result["settings"]["report"]
    database_settings = settings["database"]
    operating_profile = database_settings["operating_profile"]
    telemetry = settings["telemetry"]
    profile = result["database_profile"]
    corpus_limits = preregistration["corpora"][result["scale"]]
    logical_cpus = os.cpu_count() or 1
    worker_budget = min(
        all_limits["maximum_worker_processes"],
        max(
            1,
            math.ceil(
                logical_cpus
                * all_limits["maximum_worker_processes_per_logical_cpu"]
            ),
        ),
    )
    thread_budget = min(
        all_limits["maximum_process_tree_threads"],
        max(
            1,
            math.ceil(
                logical_cpus
                * all_limits["maximum_process_tree_threads_per_logical_cpu"]
            ),
        ),
    )
    process_io = evaluate_process_io_contract(result, preregistration)
    full_read_ratio = process_io["full_source_input_read_ratio"]
    full_write_ratio = process_io["full_source_input_write_ratio"]
    full_output_efficiency_read_ratio = process_io[
        "full_output_efficiency_read_ratio"
    ]
    full_output_efficiency_write_ratio = process_io[
        "full_output_efficiency_write_ratio"
    ]
    checks = [
        (
            "minimum indexed files",
            indexed_files,
            ">=",
            corpus_limits["minimum_indexed_files"],
            indexed_files >= corpus_limits["minimum_indexed_files"],
        ),
        (
            "full scan wall seconds",
            result["scan"]["process"]["wall_seconds"],
            "<=",
            limits["maximum_full_scan_seconds"],
            result["scan"]["process"]["wall_seconds"]
            <= limits["maximum_full_scan_seconds"],
        ),
        (
            "MCP startup milliseconds",
            result["queries"]["startup_ms"],
            "<=",
            all_limits["maximum_mcp_startup_milliseconds"],
            result["queries"]["startup_ms"]
            <= all_limits["maximum_mcp_startup_milliseconds"],
        ),
        (
            "full scan peak RSS bytes",
            result["scan"]["process"]["peak_rss_bytes"],
            "<=",
            limits["maximum_peak_rss_bytes"],
            result["scan"]["process"]["peak_rss_bytes"]
            <= limits["maximum_peak_rss_bytes"],
        ),
        (
            "worker process bound",
            result["scan"]["process"]["worker_process_bound"],
            "<=",
            worker_budget,
            result["scan"]["process"]["worker_process_bound"]
            <= worker_budget,
        ),
        (
            "reported parser workers",
            report["symbols"]["max_workers"],
            "<=",
            worker_budget,
            report["symbols"]["max_workers"] <= worker_budget,
        ),
        (
            "process-tree threads",
            scan_process["peak_threads"],
            "<=",
            thread_budget,
            scan_process["peak_threads"] <= thread_budget,
        ),
        (
            "unchanged refresh wall seconds",
            result["unchanged_refresh"]["process"]["wall_seconds"],
            "<=",
            limits["maximum_unchanged_refresh_seconds"],
            result["unchanged_refresh"]["process"]["wall_seconds"]
            <= limits["maximum_unchanged_refresh_seconds"],
        ),
        (
            "repeated query p95 milliseconds",
            result["queries"]["repeated_query_p95_ms"],
            "<=",
            limits["maximum_query_p95_milliseconds"],
            result["queries"]["repeated_query_p95_ms"]
            <= limits["maximum_query_p95_milliseconds"],
        ),
        (
            "bounded output bytes",
            result["queries"]["maximum_output_bytes"],
            "<=",
            all_limits["maximum_bounded_output_bytes"],
            result["queries"]["maximum_output_bytes"]
            <= all_limits["maximum_bounded_output_bytes"],
        ),
        (
            "retained RSS growth bytes",
            result["queries"]["retained_rss_growth_bytes"],
            "<=",
            all_limits["maximum_retained_rss_growth_bytes"],
            result["queries"]["retained_rss_growth_bytes"]
            <= all_limits["maximum_retained_rss_growth_bytes"],
        ),
        (
            "retained thread growth",
            result["queries"]["retained_thread_growth"],
            "<=",
            all_limits["maximum_retained_thread_growth"],
            result["queries"]["retained_thread_growth"]
            <= all_limits["maximum_retained_thread_growth"],
        ),
        (
            "retained child process growth",
            result["queries"]["retained_child_process_growth"],
            "==",
            0,
            result["queries"]["retained_child_process_growth"] == 0,
        ),
        (
            "persistent bytes",
            result["persistent"]["total_bytes"],
            "<=",
            limits["maximum_persistent_bytes"],
            result["persistent"]["total_bytes"]
            <= limits["maximum_persistent_bytes"],
        ),
        (
            "post-scan database bytes",
            result["scan"]["post_run_database_bytes"],
            "<=",
            limits["maximum_database_bytes"],
            result["scan"]["post_run_database_bytes"]
            <= limits["maximum_database_bytes"],
        ),
        (
            "final database bytes",
            result["persistent"]["database_bytes"],
            "<=",
            limits["maximum_database_bytes"],
            result["persistent"]["database_bytes"]
            <= limits["maximum_database_bytes"],
        ),
        (
            "complete terminal scan I/O observation",
            scan_process["terminal_io_complete"],
            "==",
            True,
            scan_process["terminal_io_complete"],
        ),
        (
            "complete terminal unchanged-refresh I/O observation",
            unchanged_process["terminal_io_complete"],
            "==",
            True,
            unchanged_process["terminal_io_complete"],
        ),
        (
            "full scan source/input read-transfer amplification ratio (reported)",
            round(full_read_ratio, 6),
            "reported",
            None,
            True,
        ),
        (
            "full scan output-efficiency read-transfer ratio",
            round(full_output_efficiency_read_ratio, 6),
            "<=",
            all_limits["maximum_database_adjusted_read_transfer_ratio"],
            full_output_efficiency_read_ratio
            <= all_limits["maximum_database_adjusted_read_transfer_ratio"],
        ),
        (
            "full scan process read-transfer bytes",
            scan_process["process_read_transfer_bytes"],
            "<=",
            limits["maximum_full_process_read_transfer_bytes"],
            process_io["full_read_transfer_within_absolute_cap"],
        ),
        (
            "full scan source/input write-transfer amplification ratio (reported)",
            round(full_write_ratio, 6),
            "reported",
            None,
            True,
        ),
        (
            "full scan output-efficiency write-transfer ratio",
            round(full_output_efficiency_write_ratio, 6),
            "<=",
            all_limits["maximum_database_adjusted_write_transfer_ratio"],
            full_output_efficiency_write_ratio
            <= all_limits["maximum_database_adjusted_write_transfer_ratio"],
        ),
        (
            "full scan process write-transfer bytes",
            scan_process["process_write_transfer_bytes"],
            "<=",
            limits["maximum_full_process_write_transfer_bytes"],
            process_io["full_write_transfer_within_absolute_cap"],
        ),
        (
            "unchanged refresh process read-transfer bytes",
            unchanged_process["process_read_transfer_bytes"],
            "<=",
            limits["maximum_unchanged_refresh_process_read_transfer_bytes"],
            unchanged_process["process_read_transfer_bytes"]
            <= limits["maximum_unchanged_refresh_process_read_transfer_bytes"],
        ),
        (
            "unchanged refresh process write-transfer bytes",
            unchanged_process["process_write_transfer_bytes"],
            "==",
            0,
            unchanged_process["process_write_transfer_bytes"] == 0,
        ),
        (
            "peak WAL bytes versus scale database cap",
            max(
                scan_process["peak_storage"]["wal_bytes"],
                unchanged_process["peak_storage"]["wal_bytes"],
            ),
            "<=",
            limits["maximum_database_bytes"],
            max(
                scan_process["peak_storage"]["wal_bytes"],
                unchanged_process["peak_storage"]["wal_bytes"],
            )
            <= limits["maximum_database_bytes"],
        ),
        (
            "peak staging bytes versus scale database cap",
            max(
                scan_process["peak_storage"]["staging_bytes"],
                unchanged_process["peak_storage"]["staging_bytes"],
            ),
            "<=",
            limits["maximum_database_bytes"],
            max(
                scan_process["peak_storage"]["staging_bytes"],
                unchanged_process["peak_storage"]["staging_bytes"],
            )
            <= limits["maximum_database_bytes"],
        ),
        (
            "final WAL bytes",
            result["persistent"]["wal_bytes"],
            "==",
            0,
            result["persistent"]["wal_bytes"] == 0,
        ),
        (
            "final graph stages",
            result["persistent"]["stage_directories"],
            "==",
            0,
            result["persistent"]["stage_directories"] == 0,
        ),
        (
            "symbol parse timeouts",
            report["symbols"]["timed_out"],
            "==",
            0,
            report["symbols"]["timed_out"] == 0,
        ),
        (
            "filesystem profile",
            result["settings"]["report"]["database"]["filesystem"],
            "==",
            "supported_local",
            result["settings"]["report"]["database"]["filesystem"]
            == "supported_local",
        ),
        (
            "journal mode",
            result["settings"]["report"]["database"]["operating_profile"][
                "observed_journal_mode"
            ],
            "==",
            "wal",
            result["settings"]["report"]["database"]["operating_profile"][
                "observed_journal_mode"
            ]
            == "wal",
        ),
        (
            "schema compatibility",
            database_settings["schema"]["compatibility"],
            "==",
            "current",
            database_settings["schema"]["compatibility"] == "current"
            and database_settings["schema"]["runtime_version"]
            == database_settings["schema"]["stored_version"],
        ),
        (
            "synchronous mode",
            operating_profile["observed_synchronous_mode"],
            "==",
            "full",
            operating_profile["observed_synchronous_mode"] == "full",
        ),
        (
            "normal busy timeout milliseconds",
            telemetry["normal_busy_timeout_ms"],
            "==",
            5000,
            telemetry["normal_busy_timeout_ms"] == 5000
            and telemetry["connection_busy_timeout_ms"] == 5000,
        ),
        (
            "telemetry busy timeout milliseconds",
            telemetry["telemetry_busy_timeout_ms"],
            "==",
            25,
            telemetry["telemetry_busy_timeout_ms"] == 25,
        ),
        (
            "WAL autocheckpoint pages",
            telemetry["wal_autocheckpoint_pages"],
            "==",
            1000,
            telemetry["wal_autocheckpoint_pages"] == 1000,
        ),
        (
            "telemetry-disabled checkpoint state",
            {
                "raw_rows": telemetry["raw_rows"],
                "writes_since_checkpoint": telemetry["writes_since_checkpoint"],
                "checkpoint_state": telemetry["checkpoint_state"],
            },
            "==",
            {
                "raw_rows": 0,
                "writes_since_checkpoint": 0,
                "checkpoint_state": "not_due",
            },
            telemetry["raw_rows"] == 0
            and telemetry["writes_since_checkpoint"] == 0
            and telemetry["checkpoint_state"] == "not_due",
        ),
        (
            "SQLite statistics policy",
            {
                "policy": telemetry["statistics_policy"],
                "state": telemetry["statistics_state"],
                "sqlite_stat1_present": profile["sqlite_stat1_present"],
            },
            "==",
            {
                "policy": "not_configured",
                "state": "not_initialized",
                "sqlite_stat1_present": False,
            },
            telemetry["statistics_policy"] == "not_configured"
            and telemetry["statistics_state"] == "not_initialized"
            and not profile["sqlite_stat1_present"],
        ),
        (
            "database quick check",
            profile["quick_check"],
            "==",
            "ok",
            profile["quick_check"] == "ok",
        ),
        (
            "database page bytes",
            profile["page_bytes"],
            "==",
            result["persistent"]["database_bytes"],
            profile["page_bytes"] == result["persistent"]["database_bytes"],
        ),
        (
            "database freelist ratio",
            profile["freelist_ratio"],
            "<=",
            all_limits["maximum_freelist_ratio"],
            profile["freelist_ratio"] <= all_limits["maximum_freelist_ratio"],
        ),
    ]
    if "minimum_scan_one_core_cpu_percent" in limits:
        checks.append(
            (
                "scan one-core CPU percent",
                scan_process["one_core_cpu_percent"],
                ">=",
                limits["minimum_scan_one_core_cpu_percent"],
                scan_process["one_core_cpu_percent"]
                >= limits["minimum_scan_one_core_cpu_percent"],
            )
        )
    if "maximum_indexed_files" in corpus_limits:
        maximum = corpus_limits["maximum_indexed_files"]
        checks.append(
            (
                "maximum indexed files",
                indexed_files,
                "<=",
                maximum,
                indexed_files <= maximum,
            )
        )
    if "minimum_tracked_bytes" in corpus_limits:
        minimum = corpus_limits["minimum_tracked_bytes"]
        checks.append(
            (
                "minimum tracked bytes",
                result["corpus"]["bytes"],
                ">=",
                minimum,
                result["corpus"]["bytes"] >= minimum,
            )
        )

    repeated_calls = [
        call
        for call in result["queries"]["calls"]
        if call["phase"].startswith("repeated_query_")
    ]
    publication = result["queries"]["stable_publication"]
    checks.extend(
        [
            (
                "stable publication starts complete",
                publication["before"],
                "complete",
                "stable complete publication with fingerprint",
                publication["before"]["state"] == "complete"
                and publication["before"]["generation"] > 0
                and bool(publication["before"]["contract_fingerprint"]),
            ),
            (
                "publication remains stable across repeated queries",
                publication["after"],
                "==",
                publication["before"],
                publication["stable"],
            ),
        ]
    )
    bounded_reads = [
        call["process_io"]["read_transfer_bytes"]
        for call in repeated_calls
        if call["tool"] != "atlas_search"
    ]
    maximum_bounded_read = max(bounded_reads, default=0)
    checks.append(
        (
            "bounded query process read bytes",
            maximum_bounded_read,
            "<=",
            all_limits["maximum_bounded_query_process_read_bytes"],
            maximum_bounded_read
            <= all_limits["maximum_bounded_query_process_read_bytes"],
        )
    )
    for key in ("active_nodes", "visited_nodes"):
        values = [
            call["work"][key]
            for call in repeated_calls
            if call["work"][key] is not None
        ]
        checks.append(
            (
                f"bounded query {key.replace('_', ' ')}",
                max(values, default=0),
                "<=",
                all_limits["maximum_relation_database_requested_rows"],
                all(
                    value <= all_limits["maximum_relation_database_requested_rows"]
                    for value in values
                ),
            )
        )

    fallback_calls = [
        call
        for call in repeated_calls
        if call["tool"] == "atlas_search" and call["arguments"].get("regex") is True
    ]
    fallback_bytes = [call["work"]["searched_bytes"] for call in fallback_calls]
    checks.extend(
        [
            (
                "fallback search runs",
                len(fallback_calls),
                "==",
                5,
                len(fallback_calls) == 5,
            ),
            (
                "fallback search reports selected text bytes",
                sum(value is not None for value in fallback_bytes),
                "==",
                len(fallback_calls),
                all(value is not None for value in fallback_bytes),
            ),
            (
                "fallback selected text bytes",
                max((value or 0 for value in fallback_bytes), default=0),
                "<=",
                all_limits["maximum_fallback_selected_text_bytes"],
                all(
                    value is not None
                    and value <= all_limits["maximum_fallback_selected_text_bytes"]
                    for value in fallback_bytes
                ),
            ),
            (
                "fallback search seconds",
                round(
                    max((call["elapsed_ms"] for call in fallback_calls), default=0)
                    / 1_000,
                    6,
                ),
                "<=",
                all_limits["maximum_fallback_search_seconds"],
                all(
                    call["elapsed_ms"] / 1_000
                    <= all_limits["maximum_fallback_search_seconds"]
                    for call in fallback_calls
                ),
            ),
        ]
    )

    relation_calls = [
        call for call in repeated_calls if call["tool"] == "atlas_symbol_relations"
    ]
    checks.append(
        (
            "relation query runs",
            len(relation_calls),
            "==",
            5,
            len(relation_calls) == 5,
        )
    )
    for name, key, threshold_key in (
        (
            "relation requested rows",
            "database_requested_rows",
            "maximum_relation_database_requested_rows",
        ),
        (
            "relation returned rows",
            "database_returned_rows",
            "maximum_relation_database_returned_rows",
        ),
        (
            "relation decoded bytes",
            "database_decoded_bytes",
            "maximum_relation_database_decoded_bytes",
        ),
        (
            "relation hydrated entities",
            "hydrated_entities",
            "maximum_relation_hydrated_entities",
        ),
        (
            "relation hydrated purpose paths",
            "hydrated_purpose_paths",
            "maximum_relation_hydrated_purpose_paths",
        ),
    ):
        values = [call["work"][key] for call in relation_calls]
        maximum = max((value or 0 for value in values), default=0)
        threshold = all_limits[threshold_key]
        checks.append(
            (
                name,
                maximum,
                "<=",
                threshold,
                all(value is not None and value <= threshold for value in values),
            )
        )

    if result["incremental"] is not None:
        incremental = result["incremental"]
        before = incremental["before"]
        narrow = incremental["narrow"]
        expanded = incremental["expanded"]
        guidance = expanded["guidance"]
        rebuild = incremental["expanded"]["rebuild"]
        caller_files = preregistration["corpora"]["medium"]["caller_files"]
        guidance_report = guidance["report"]
        guidance_changed = guidance_report.get("changed")
        guidance_sample_paths = guidance_report.get("sample_paths")
        rebuild_read_ratio = process_io["rebuild_input_efficiency_read_ratio"]
        rebuild_write_ratio = process_io["rebuild_input_efficiency_write_ratio"]
        checks.extend(
            [
                (
                    "complete narrow terminal I/O observation",
                    narrow["process"]["terminal_io_complete"],
                    "==",
                    True,
                    narrow["process"]["terminal_io_complete"],
                ),
                (
                    "narrow text candidates",
                    narrow["report"]["text_index"]["candidates"],
                    "==",
                    1,
                    narrow["report"]["text_index"]["candidates"] == 1,
                ),
                (
                    "narrow parsed source files",
                    narrow["report"]["last_symbols"]["parsed"],
                    "==",
                    1,
                    narrow["report"]["last_symbols"]["parsed"] == 1,
                ),
                (
                    "narrow watcher readiness publication",
                    narrow["process"]["readiness_generation"],
                    "==",
                    before["generation"] + 1,
                    narrow["process"]["readiness_generation"]
                    == before["generation"] + 1,
                ),
                (
                    "narrow publication generation",
                    narrow["counts"]["generation"],
                    "==",
                    narrow["process"]["readiness_generation"] + 1,
                    narrow["counts"]["generation"]
                    == narrow["process"]["readiness_generation"] + 1,
                ),
                (
                    "narrow watcher backend",
                    narrow["report"]["mode"],
                    "==",
                    "notify",
                    narrow["report"]["mode"] == "notify",
                ),
                (
                    "narrow relation resolution",
                    narrow["counts"]["relation_resolution"],
                    "==",
                    before["relation_resolution"],
                    narrow["counts"]["relation_resolution"]
                    == before["relation_resolution"],
                ),
                (
                    "narrow edit-to-refresh seconds",
                    narrow["process"]["edit_to_complete_seconds"],
                    "<=",
                    limits["maximum_unchanged_refresh_seconds"],
                    narrow["process"]["edit_to_complete_seconds"]
                    <= limits["maximum_unchanged_refresh_seconds"],
                ),
                (
                    "narrow refresh process read-transfer bytes",
                    narrow["process"]["process_read_transfer_bytes"],
                    "<=",
                    all_limits["maximum_narrow_refresh_process_read_transfer_bytes"],
                    narrow["process"]["process_read_transfer_bytes"]
                    <= all_limits[
                        "maximum_narrow_refresh_process_read_transfer_bytes"
                    ],
                ),
                (
                    "narrow refresh process write-transfer bytes",
                    narrow["process"]["process_write_transfer_bytes"],
                    "<=",
                    all_limits["maximum_narrow_refresh_process_write_transfer_bytes"],
                    narrow["process"]["process_write_transfer_bytes"]
                    <= all_limits[
                        "maximum_narrow_refresh_process_write_transfer_bytes"
                    ],
                ),
                (
                    "publication writer probe attempts",
                    narrow["process"]["writer_availability"]["attempts"],
                    ">",
                    0,
                    narrow["process"]["writer_availability"]["attempts"] > 0,
                ),
                (
                    "publication writer-unavailable upper bound seconds",
                    narrow["process"]["writer_availability"][
                        "maximum_busy_upper_bound_seconds"
                    ],
                    "<=",
                    all_limits[
                        "maximum_publication_writer_unavailable_seconds"
                    ],
                    narrow["process"]["writer_availability"][
                        "maximum_busy_upper_bound_seconds"
                    ]
                    <= all_limits[
                        "maximum_publication_writer_unavailable_seconds"
                    ],
                ),
                (
                    "expanded watcher readiness publication",
                    guidance["process"]["readiness_generation"],
                    "==",
                    narrow["counts"]["generation"] + 1,
                    guidance["process"]["readiness_generation"]
                    == narrow["counts"]["generation"] + 1,
                ),
                (
                    "complete expanded terminal I/O observation",
                    guidance["process"]["terminal_io_complete"],
                    "==",
                    True,
                    guidance["process"]["terminal_io_complete"],
                ),
                (
                    "expanded closure guidance scope",
                    guidance_report.get("scope"),
                    "==",
                    "full",
                    guidance_report.get("scope") == "full",
                ),
                (
                    "expanded closure changed graph footprint",
                    guidance_changed,
                    ">",
                    caller_files,
                    isinstance(guidance_changed, int)
                    and guidance_changed > caller_files,
                ),
                (
                    "expanded closure modified graph footprint",
                    guidance_report.get("modified"),
                    "==",
                    guidance_changed,
                    isinstance(guidance_changed, int)
                    and guidance_report.get("modified") == guidance_changed,
                ),
                (
                    "expanded closure added graph footprint",
                    guidance_report.get("added"),
                    "==",
                    0,
                    guidance_report.get("added") == 0,
                ),
                (
                    "expanded closure removed graph footprint",
                    guidance_report.get("removed"),
                    "==",
                    0,
                    guidance_report.get("removed") == 0,
                ),
                (
                    "expanded closure sample path count",
                    (
                        len(guidance_sample_paths)
                        if isinstance(guidance_sample_paths, list)
                        else None
                    ),
                    "between",
                    f"1..{caller_files}",
                    isinstance(guidance_sample_paths, list)
                    and 0 < len(guidance_sample_paths) <= caller_files,
                ),
                (
                    "expanded closure sample identifies fixture source",
                    guidance_sample_paths,
                    "contains",
                    "src/hub.rs or src/caller_*.rs",
                    isinstance(guidance_sample_paths, list)
                    and any(
                        path == "src/hub.rs" or path.startswith("src/caller_")
                        for path in guidance_sample_paths
                        if isinstance(path, str)
                    ),
                ),
                (
                    "expanded closure returns refresh guidance",
                    guidance["report"]["status"],
                    "==",
                    "refresh_required",
                    guidance["report"]["status"] == "refresh_required",
                ),
                (
                    "expanded closure guidance reason",
                    guidance["report"]["reason"],
                    "==",
                    "dependency_closure_limit",
                    guidance["report"]["reason"] == "dependency_closure_limit",
                ),
                (
                    "expanded guidance preserves generation",
                    guidance["counts"]["generation"],
                    "==",
                    guidance["process"]["readiness_generation"],
                    guidance["counts"]["generation"]
                    == guidance["process"]["readiness_generation"],
                ),
                (
                    "expanded guidance wall seconds",
                    guidance["process"]["edit_to_complete_seconds"],
                    "<=",
                    limits["maximum_full_scan_seconds"],
                    guidance["process"]["edit_to_complete_seconds"]
                    <= limits["maximum_full_scan_seconds"],
                ),
                (
                    "expanded guidance process read-transfer bytes",
                    guidance["process"]["process_read_transfer_bytes"],
                    "<=",
                    all_limits[
                        "maximum_expanded_guidance_process_read_transfer_bytes"
                    ],
                    process_io["expanded_guidance_read_within_absolute_cap"],
                ),
                (
                    "expanded guidance process write-transfer bytes",
                    guidance["process"]["process_write_transfer_bytes"],
                    "<=",
                    all_limits[
                        "maximum_expanded_guidance_process_write_transfer_bytes"
                    ],
                    process_io["expanded_guidance_write_within_absolute_cap"],
                ),
                (
                    "expanded guidance writer probe attempts",
                    guidance["process"]["writer_availability"]["attempts"],
                    ">",
                    0,
                    guidance["process"]["writer_availability"]["attempts"] > 0,
                ),
                (
                    "expanded guidance writer-unavailable seconds",
                    guidance["process"]["writer_availability"][
                        "maximum_busy_upper_bound_seconds"
                    ],
                    "<=",
                    all_limits[
                        "maximum_publication_writer_unavailable_seconds"
                    ],
                    guidance["process"]["writer_availability"][
                        "maximum_busy_upper_bound_seconds"
                    ]
                    <= all_limits[
                        "maximum_publication_writer_unavailable_seconds"
                    ],
                ),
                (
                    "complete rebuild terminal I/O observation",
                    rebuild["process"]["terminal_io_complete"],
                    "==",
                    True,
                    rebuild["process"]["terminal_io_complete"],
                ),
                (
                    "explicit rebuild generation",
                    rebuild["counts"]["generation"],
                    "==",
                    guidance["counts"]["generation"] + 1,
                    rebuild["counts"]["generation"]
                    == guidance["counts"]["generation"] + 1,
                ),
                (
                    "explicit rebuild resolved relations",
                    rebuild["counts"]["relation_resolution"].get("resolved", 0),
                    "==",
                    caller_files,
                    rebuild["counts"]["relation_resolution"].get("resolved", 0)
                    == caller_files,
                ),
                (
                    "explicit rebuild unresolved relations",
                    rebuild["counts"]["relation_resolution"].get("unresolved", 0),
                    "==",
                    caller_files,
                    rebuild["counts"]["relation_resolution"].get("unresolved", 0)
                    == caller_files,
                ),
                (
                    "explicit rebuild wall seconds",
                    rebuild["process"]["wall_seconds"],
                    "<=",
                    limits["maximum_full_scan_seconds"],
                    rebuild["process"]["wall_seconds"]
                    <= limits["maximum_full_scan_seconds"],
                ),
                (
                    "explicit rebuild input-efficiency read-transfer ratio",
                    round(rebuild_read_ratio, 6),
                    "<=",
                    all_limits["maximum_database_adjusted_read_transfer_ratio"],
                    rebuild_read_ratio
                    <= all_limits["maximum_database_adjusted_read_transfer_ratio"],
                ),
                (
                    "explicit rebuild process read-transfer bytes",
                    rebuild["process"]["process_read_transfer_bytes"],
                    "<=",
                    limits["maximum_full_process_read_transfer_bytes"],
                    rebuild["process"]["process_read_transfer_bytes"]
                    <= limits["maximum_full_process_read_transfer_bytes"],
                ),
                (
                    "explicit rebuild input-efficiency write-transfer ratio",
                    round(rebuild_write_ratio, 6),
                    "<=",
                    all_limits["maximum_database_adjusted_write_transfer_ratio"],
                    rebuild_write_ratio
                    <= all_limits["maximum_database_adjusted_write_transfer_ratio"],
                ),
                (
                    "explicit rebuild process write-transfer bytes",
                    rebuild["process"]["process_write_transfer_bytes"],
                    "<=",
                    limits["maximum_full_process_write_transfer_bytes"],
                    rebuild["process"]["process_write_transfer_bytes"]
                    <= limits["maximum_full_process_write_transfer_bytes"],
                ),
                (
                    "incremental and rebuild leave no graph stage",
                    result["persistent"]["stage_directories"],
                    "==",
                    0,
                    result["persistent"]["stage_directories"] == 0,
                ),
            ]
        )
    return [
        {
            "name": name,
            "observed": observed,
            "operator": operator,
            "threshold": threshold,
            "passed": passed,
        }
        for name, observed, operator, threshold, passed in checks
    ]


def run_watch_once(
    runtime: Path,
    root: Path,
    env: dict[str, str],
    timeout_seconds: float,
    max_workers: int | None = None,
) -> dict[str, Any]:
    worker_arguments = (
        ["--max-workers", str(max_workers)] if max_workers is not None else []
    )
    return run_measured(
        [
            str(runtime),
            "--require-version",
            "0.4.0",
            "--format",
            "json",
            "watch",
            "--once",
            *worker_arguments,
            ".",
        ],
        cwd=root,
        env=env,
        timeout_seconds=timeout_seconds,
    )


def concurrent_worker_allocation(
    logical_cpus: int,
    process_count: int,
    thresholds: dict[str, Any],
) -> tuple[int, int, int]:
    """Split the host index-worker budget across measured CLI processes."""
    worker_budget = min(
        thresholds["maximum_worker_processes"],
        max(
            1,
            math.ceil(
                logical_cpus
                * thresholds["maximum_worker_processes_per_logical_cpu"]
            ),
        ),
    )
    workers_per_process = max(1, worker_budget // process_count)
    return (
        worker_budget,
        workers_per_process,
        workers_per_process * process_count,
    )


def reported_parser_workers_within_budget(
    runs: list[dict[str, Any]], workers_per_process: int
) -> bool:
    """Check successful watch reports without treating them as scan evidence."""
    return all(
        run["returncode"] != 0
        or json.loads(run["stdout"])["last_symbols"]["max_workers"]
        <= workers_per_process
        for run in runs
    )


def aggregate_process_metrics(runs: list[dict[str, Any]]) -> dict[str, Any]:
    """Return conservative host-wide upper bounds from concurrent process runs."""
    return {
        "method": "sum-of-per-process-peaks-conservative-upper-bound",
        "peak_rss_bytes": sum(run["peak_rss_bytes"] for run in runs),
        "peak_worker_processes": sum(
            run["peak_worker_processes"] for run in runs
        ),
        "worker_process_bound": sum(
            run["worker_process_bound"] for run in runs
        ),
        "peak_threads": sum(run["peak_threads"] for run in runs),
        "cpu_seconds": round(sum(run["cpu_seconds"] for run in runs), 6),
        "process_read_transfer_bytes": sum(
            run["process_read_transfer_bytes"] for run in runs
        ),
        "process_write_transfer_bytes": sum(
            run["process_write_transfer_bytes"] for run in runs
        ),
        "terminal_io_complete": all(
            run["terminal_io_complete"] for run in runs
        ),
    }


def concurrent_isolation(
    runtime: Path,
    work_root: Path,
    env: dict[str, str],
    timeout_seconds: float,
    caller_files: int,
    thresholds: dict[str, Any],
) -> dict[str, Any]:
    roots = [work_root / "concurrent-a", work_root / "concurrent-b"]
    logical_cpus = os.cpu_count() or 1
    (
        worker_budget,
        workers_per_process,
        configured_worker_budget,
    ) = concurrent_worker_allocation(logical_cpus, len(roots), thresholds)
    for root in roots:
        prepare_medium(root, caller_files)
        measured_json(
            runtime,
            ["scan", "."],
            cwd=root,
            env=env,
            timeout_seconds=timeout_seconds,
        )
    databases = [root / ".projectatlas/projectatlas.db" for root in roots]
    before = [database_counts(database) for database in databases]
    markers = ["concurrent-root-a", "concurrent-root-b"]
    for root, marker in zip(roots, markers, strict=True):
        with (root / "src/caller_0000.rs").open(
            "a", encoding="utf-8", newline="\n"
        ) as stream:
            stream.write(f"// {marker}\n")

    started = time.perf_counter()
    with ThreadPoolExecutor(max_workers=len(roots)) as pool:
        results = list(
            pool.map(
                lambda root: run_watch_once(
                    runtime,
                    root,
                    env,
                    timeout_seconds,
                    max_workers=workers_per_process,
                ),
                roots,
            )
        )
    cross_root_resources = aggregate_process_metrics(results)
    cross_root: list[dict[str, Any]] = []
    for index, (root, database, own_marker, foreign_marker) in enumerate(
        zip(roots, databases, markers, reversed(markers), strict=True)
    ):
        after = database_counts(database)
        profile = database_profile(database)
        with sqlite3.connect(database) as connection:
            content = str(
                connection.execute(
                    "SELECT content FROM file_texts WHERE path = 'src/caller_0000.rs'"
                ).fetchone()[0]
            )
        expected_root = root.resolve().as_posix()
        observed_root = str(profile["project_root"]).replace("\\", "/")
        root_matches = (
            observed_root.casefold() == expected_root.casefold()
            if os.name == "nt"
            else observed_root == expected_root
        )
        cross_root.append(
            {
                "root": str(root),
                "before": before[index],
                "after": after,
                "profile": profile,
                "own_marker_present": own_marker in content,
                "foreign_marker_absent": foreign_marker not in content,
                "root_matches": root_matches,
                "stage_directories": storage_state(root)["stage_directories"],
            }
        )

    same_root = roots[0]
    same_database = databases[0]
    same_before = database_counts(same_database)
    with (same_root / "src/caller_0001.rs").open(
        "a", encoding="utf-8", newline="\n"
    ) as stream:
        stream.write("// same-root-race\n")
    with ThreadPoolExecutor(max_workers=2) as pool:
        same_runs = list(
            pool.map(
                lambda _: run_watch_once(
                    runtime,
                    same_root,
                    env,
                    timeout_seconds,
                    max_workers=workers_per_process,
                ),
                range(2),
            )
        )
    same_root_resources = aggregate_process_metrics(same_runs)
    same_after = database_counts(same_database)
    accepted_same_results = all(
        not run["timed_out"]
        and (
            run["returncode"] == 0
        or (
            run["returncode"] != 0
            and any(
                marker in (run["stdout"] + run["stderr"]).lower()
                for marker in ("busy", "locked", "refresh_required")
            )
        )
        )
        for run in same_runs
    )
    cross_root_passed = all(
        run["returncode"] == 0 and not run["timed_out"] for run in results
    ) and all(
        row["after"]["generation"] == row["before"]["generation"] + 1
        and row["profile"]["quick_check"] == "ok"
        and row["own_marker_present"]
        and row["foreign_marker_absent"]
        and row["root_matches"]
        and row["stage_directories"] == 0
        for row in cross_root
    )
    concurrent_runs = [*results, *same_runs]
    worker_argument = str(workers_per_process)
    worker_arguments_passed = all(
        "--max-workers" in run["arguments"]
        and run["arguments"][run["arguments"].index("--max-workers") + 1]
        == worker_argument
        for run in concurrent_runs
    )
    configured_worker_budget_passed = configured_worker_budget <= worker_budget
    reported_parser_worker_budget_passed = reported_parser_workers_within_budget(
        concurrent_runs, workers_per_process
    )
    resource_envelope_passed = all(
        resources["terminal_io_complete"]
        and resources["worker_process_bound"] <= worker_budget
        and resources["peak_rss_bytes"]
        <= thresholds["maximum_concurrent_peak_rss_bytes"]
        for resources in (cross_root_resources, same_root_resources)
    ) and all(
        (
            worker_arguments_passed,
            configured_worker_budget_passed,
            reported_parser_worker_budget_passed,
        )
    )
    same_root_passed = (
        accepted_same_results
        and any(run["returncode"] == 0 for run in same_runs)
        and same_after["generation"] == same_before["generation"] + 1
        and database_profile(same_database)["quick_check"] == "ok"
        and storage_state(same_root)["stage_directories"] == 0
    )
    return {
        "wall_seconds": round(time.perf_counter() - started, 6),
        "roots": [str(root) for root in roots],
        "runs": results,
        "cross_root_resources": cross_root_resources,
        "cross_root": cross_root,
        "same_root": {
            "before": same_before,
            "after": same_after,
            "runs": same_runs,
            "resources": same_root_resources,
            "passed": same_root_passed,
        },
        "resource_envelope": {
            "logical_cpus": logical_cpus,
            "worker_budget": worker_budget,
            "workers_per_process": workers_per_process,
            "configured_worker_budget": configured_worker_budget,
            "worker_arguments_passed": worker_arguments_passed,
            "configured_worker_budget_passed": configured_worker_budget_passed,
            "reported_parser_worker_budget_passed": (
                reported_parser_worker_budget_passed
            ),
            "peak_rss_budget": thresholds["maximum_concurrent_peak_rss_bytes"],
            "passed": resource_envelope_passed,
        },
        "passed": cross_root_passed
        and same_root_passed
        and resource_envelope_passed,
    }


def publication_contention(
    runtime: Path,
    root: Path,
    env: dict[str, str],
    timeout_seconds: float,
    maximum_failure_seconds: float,
) -> dict[str, Any]:
    database = root / ".projectatlas/projectatlas.db"
    before = database_counts(database)
    with (root / "src/caller_0002.rs").open(
        "a", encoding="utf-8", newline="\n"
    ) as stream:
        stream.write("// publication-contention\n")
    blocker = sqlite3.connect(database)
    try:
        blocker.execute("BEGIN IMMEDIATE")
        failed = run_watch_once(runtime, root, env, maximum_failure_seconds)
        after_failure = database_counts(database)
    finally:
        blocker.rollback()
        blocker.close()
    retry = run_watch_once(runtime, root, env, timeout_seconds)
    after_retry = database_counts(database)
    diagnostic = (failed["stdout"] + failed["stderr"]).lower()
    typed_busy = any(
        marker in diagnostic
        for marker in ("database is locked", "database is busy", "writer acquisition")
    )
    passed = (
        failed["returncode"] != 0
        and not failed["timed_out"]
        and failed["wall_seconds"] <= maximum_failure_seconds
        and typed_busy
        and after_failure == before
        and retry["returncode"] == 0
        and after_retry["generation"] == before["generation"] + 1
        and database_profile(database)["quick_check"] == "ok"
        and storage_state(root)["stage_directories"] == 0
    )
    return {
        "before": before,
        "blocked_run": failed,
        "after_blocked_run": after_failure,
        "typed_busy": typed_busy,
        "retry": retry,
        "after_retry": after_retry,
        "passed": passed,
    }


def cooperative_cancellation_reopen(
    runtime: Path,
    root: Path,
    env: dict[str, str],
    threshold_seconds: float,
    request_timeout_seconds: float,
) -> dict[str, Any]:
    database = root / ".projectatlas/projectatlas.db"
    before = database_counts(database)
    pending = root / "pending-cancellation"
    pending.mkdir()
    source = (
        "pub fn pending_contract() { let value = 1_u64; let _ = value; }\n"
        * 128
    )
    for index in range(512):
        (pending / f"work-{index:04}.rs").write_text(
            source, encoding="utf-8", newline="\n"
        )
    rpc_timeout_seconds = min(request_timeout_seconds, threshold_seconds)
    client = McpClient(
        runtime,
        root,
        env,
        request_timeout_seconds=rpc_timeout_seconds,
    )
    known = {client.process.pid}
    started_text = ""
    status_text = ""
    cancel_text = ""
    terminal_state = None
    cancellation_seconds = None
    active_work_observed = False
    same_client_read = ""
    state_after_cancel: dict[str, Any] | None = None
    writer_released = False
    after = before

    def call_before(
        mcp_client: McpClient,
        tool: str,
        arguments: dict[str, Any],
        deadline: float,
    ) -> tuple[str, float]:
        remaining = deadline - time.perf_counter()
        if remaining <= 0:
            raise TimeoutError(
                f"MCP cancellation flow exceeded its deadline before {tool}"
            )
        mcp_client.request_timeout_seconds = min(
            request_timeout_seconds, remaining
        )
        return mcp_client.call(tool, arguments)

    try:
        state_before = process_tree_state(client.process.pid)
        started_text, _ = client.call(
            "atlas_scan",
            {
                "project_path": str(root),
                "path": str(root),
                "background": True,
                "max_workers": 1,
            },
        )
        task_id = toon_scalar(started_text, "task_id")
        if task_id is None:
            raise RuntimeError(f"background scan omitted task id: {started_text}")
        active_deadline = time.perf_counter() + threshold_seconds
        while time.perf_counter() <= active_deadline:
            status_text, _ = call_before(
                client,
                "atlas_task_status",
                {"task_id": task_id},
                active_deadline,
            )
            known.update(member.pid for member in process_tree(client.process.pid))
            state = toon_scalar(status_text, "state")
            if state == "running":
                active_work_observed = True
                break
            if state in {"canceled", "failed", "succeeded"}:
                break
            time.sleep(0.025)
        cancel_started = time.perf_counter()
        deadline = cancel_started + threshold_seconds
        cancel_text, _ = call_before(
            client,
            "atlas_task_cancel",
            {"task_id": task_id},
            deadline,
        )
        while time.perf_counter() <= deadline:
            status_text, _ = call_before(
                client,
                "atlas_task_status",
                {"task_id": task_id},
                deadline,
            )
            known.update(member.pid for member in process_tree(client.process.pid))
            match = re.search(
                r"^\s+state: (pending|running|canceled|failed|succeeded)$",
                status_text,
                re.MULTILINE,
            )
            terminal_state = match.group(1) if match else None
            if terminal_state in {"canceled", "failed", "succeeded"}:
                cancellation_seconds = time.perf_counter() - cancel_started
                break
            time.sleep(0.025)
        quiescence_deadline = cancel_started + threshold_seconds
        while time.perf_counter() <= quiescence_deadline:
            known.update(member.pid for member in process_tree(client.process.pid))
            state_after_cancel = process_tree_state(client.process.pid)
            children = [
                pid
                for pid in state_after_cancel["pids"]
                if pid != client.process.pid
            ]
            writer_released = database_writer_available(database)
            if not children and writer_released:
                same_client_read, _ = call_before(
                    client,
                    "atlas_overview",
                    {},
                    quiescence_deadline,
                )
                after = database_counts(database)
                break
            time.sleep(0.025)
    finally:
        client.close()
    _, cli_settings = measured_json(
        runtime,
        ["settings"],
        cwd=root,
        env=env,
        timeout_seconds=threshold_seconds,
    )
    reopened = McpClient(
        runtime,
        root,
        env,
        request_timeout_seconds=request_timeout_seconds,
    )
    try:
        reopened_settings, _ = reopened.call("atlas_settings", {})
    finally:
        reopened.close()
    final = database_counts(database)
    survivors = []
    for pid in known:
        try:
            process = psutil.Process(pid)
            if process.is_running() and process.status() != psutil.STATUS_ZOMBIE:
                survivors.append(pid)
        except psutil.Error:
            continue
    passed = (
        active_work_observed
        and terminal_state == "canceled"
        and cancellation_seconds is not None
        and cancellation_seconds <= threshold_seconds
        and "cancellation_requested" in cancel_text
        and post_cancellation_read_is_safe(same_client_read)
        and state_after_cancel is not None
        and state_after_cancel["processes"] == 1
        and state_after_cancel["threads"] <= state_before["threads"] + 2
        and writer_released
        and after == before
        and final == before
        and cli_settings["database"]["publication"]["generation"]
        == before["generation"]
        and toon_integer(reopened_settings, "generation") == before["generation"]
        and database_profile(database)["quick_check"] == "ok"
        and storage_state(root)["stage_directories"] == 0
        and not survivors
    )
    return {
        "task_start": started_text,
        "last_status": status_text,
        "cancel": cancel_text,
        "active_work_observed": active_work_observed,
        "terminal_state": terminal_state,
        "cancellation_seconds": (
            round(cancellation_seconds, 6)
            if cancellation_seconds is not None
            else None
        ),
        "before": before,
        "after": after,
        "state_before": state_before,
        "state_after_cancel": state_after_cancel,
        "same_client_read": same_client_read,
        "writer_released_before_server_close": writer_released,
        "final": final,
        "survivors": survivors,
        "passed": passed,
    }


def termination_recovery_is_complete(
    reopen_process: dict[str, Any],
    checkpoint: dict[str, int],
    recovery_profile: dict[str, Any],
    final_storage: dict[str, int],
) -> bool:
    return (
        reopen_process.get("returncode") == 0
        and not reopen_process.get("timed_out", False)
        and checkpoint.get("busy") == 0
        and recovery_profile.get("quick_check") == "ok"
        and final_storage.get("wal_bytes") == 0
        and final_storage.get("staging_bytes") == 0
        and final_storage.get("stage_directories") == 0
    )


def forced_termination_quiescence(
    runtime: Path,
    source_root: Path,
    work_root: Path,
    env: dict[str, str],
    threshold_seconds: float,
) -> dict[str, Any]:
    recovery_root = work_root / "default-core-parent-termination"
    database = recovery_root / ".projectatlas/projectatlas.db"
    database.parent.mkdir(parents=True, exist_ok=True)
    process = subprocess.Popen(
        [
            str(runtime),
            "--require-version",
            "0.4.0",
            "--format",
            "json",
            "--db",
            str(database),
            "scan",
            str(source_root),
        ],
        cwd=source_root,
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    known: set[int] = {process.pid}
    deadline = time.perf_counter() + 2
    while process.poll() is None and time.perf_counter() < deadline:
        known.update(member.pid for member in process_tree(process.pid))
        time.sleep(MEASURE_INTERVAL_SECONDS)
    if process.poll() is not None:
        return {
            "passed": False,
            "reason": "scan completed before cancellation could be requested",
            "returncode": process.returncode,
            "known_processes": len(known),
        }
    requested = time.perf_counter()
    process.terminate()
    try:
        process.wait(timeout=threshold_seconds)
    except subprocess.TimeoutExpired:
        terminate_process_tree(process)
        process.wait(timeout=5)
    while time.perf_counter() - requested <= threshold_seconds:
        survivors = []
        for pid in known:
            try:
                member = psutil.Process(pid)
                if member.is_running() and member.status() != psutil.STATUS_ZOMBIE:
                    survivors.append(pid)
            except psutil.Error:
                continue
        if not survivors:
            elapsed = time.perf_counter() - requested
            try:
                reopen_process, reopen_settings = measured_json(
                    runtime,
                    ["--db", str(database), "settings"],
                    cwd=source_root,
                    env=env,
                    timeout_seconds=threshold_seconds,
                )
                connection = sqlite3.connect(
                    database, timeout=threshold_seconds
                )
                try:
                    checkpoint_row = connection.execute(
                        "PRAGMA wal_checkpoint(TRUNCATE)"
                    ).fetchone()
                finally:
                    connection.close()
                if checkpoint_row is None:
                    raise RuntimeError(
                        "terminated database checkpoint returned no result"
                    )
                checkpoint = {
                    "busy": int(checkpoint_row[0]),
                    "log_frames": int(checkpoint_row[1]),
                    "checkpointed_frames": int(checkpoint_row[2]),
                }
                recovery_profile = database_profile(database)
                final_storage = persistent_sizes(recovery_root)
            except Exception as error:
                return {
                    "scope": "default-core in-process scan parent",
                    "passed": False,
                    "quiescence_seconds": round(elapsed, 6),
                    "known_processes": len(known),
                    "survivors": [],
                    "reason": f"terminated database recovery failed: {error}",
                }
            recovery_complete = termination_recovery_is_complete(
                reopen_process, checkpoint, recovery_profile, final_storage
            )
            passed = len(known) == 1 and recovery_complete
            return {
                "scope": "default-core in-process scan parent",
                "passed": passed,
                "quiescence_seconds": round(elapsed, 6),
                "known_processes": len(known),
                "survivors": [],
                "reopen": {
                    "process": reopen_process,
                    "settings": reopen_settings,
                },
                "checkpoint": checkpoint,
                "recovery_profile": recovery_profile,
                "final_storage": final_storage,
                "reason": (
                    None
                    if passed
                    else (
                        "default-core scan unexpectedly created child processes"
                        if len(known) != 1
                        else "terminated database did not reopen, checkpoint, and recover cleanly"
                    )
                ),
            }
        time.sleep(MEASURE_INTERVAL_SECONDS)
    for pid in survivors:
        try:
            psutil.Process(pid).kill()
        except psutil.Error:
            continue
    return {
        "scope": "default-core in-process scan parent",
        "passed": False,
        "quiescence_seconds": round(time.perf_counter() - requested, 6),
        "known_processes": len(known),
        "survivors": survivors,
    }


def publication_identity_errors(
    preregistration: dict[str, Any],
    *,
    runtime_sha256: str,
    mcp_tools_sha256: str,
    skill_sha256: str,
    skill_bytes: int,
    runtime_info: dict[str, Any],
    dirty_paths: list[str],
    measurement_errors: list[str],
) -> list[str]:
    candidate = preregistration["candidate"]
    errors = []
    if preregistration.get("status") != "locked_for_final_measurement":
        errors.append("preregistration is not locked for final measurement")
    if candidate.get("runtime_sha256") != runtime_sha256:
        errors.append("runtime SHA-256 does not match the preregistered candidate")
    if candidate.get("mcp_tools_sha256") != mcp_tools_sha256:
        errors.append("MCP tool inventory/schema digest does not match the candidate")
    if candidate.get("skill_sha256") != skill_sha256:
        errors.append("packaged skill SHA-256 does not match the candidate")
    if candidate.get("skill_bytes") != skill_bytes:
        errors.append("packaged skill size does not match the candidate")
    if runtime_info.get("project") != "ProjectAtlas":
        errors.append("runtime identity is not ProjectAtlas")
    if runtime_info.get("version") != candidate.get("required_version"):
        errors.append("runtime version does not match the preregistered candidate")
    capabilities = set(runtime_info.get("capabilities", []))
    if not {"mcp", "sqlite", "toon"}.issubset(capabilities):
        errors.append("runtime omitted required MCP, SQLite, or TOON capability")
    if runtime_info.get("text_format") != "TOON":
        errors.append("runtime text format is not TOON")
    if len(runtime_info.get("mcp_tools", [])) != 40:
        errors.append("runtime does not advertise the frozen 40-tool MCP surface")
    if dirty_paths:
        errors.append(
            "tracked benchmark source is dirty: " + ", ".join(dirty_paths)
        )
    errors.extend(measurement_errors)
    return errors


def candidate_source_identity(preregistration_path: Path) -> dict[str, str]:
    try:
        preregistration_relative = (
            preregistration_path.resolve().relative_to(ROOT.resolve()).as_posix()
        )
    except ValueError as error:
        raise ValueError(
            "preregistration must be inside the candidate checkout"
        ) from error
    return {
        "checkout_head": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip(),
        "preregistration_path": preregistration_relative,
    }


def validate_publication_identity(
    runtime: Path,
    preregistration: dict[str, Any],
    preregistration_path: Path,
) -> tuple[dict[str, Any], dict[str, str]]:
    candidate = preregistration["candidate"]
    runtime_sha256 = hashlib.sha256(runtime.read_bytes()).hexdigest()
    try:
        skill_identity = candidate_file_identity(str(candidate.get("skill_path", "")))
    except ValueError as error:
        raise RuntimeError(f"packaged skill identity is invalid: {error}") from error
    skill_sha256 = str(skill_identity["sha256"])
    skill_bytes = int(skill_identity["bytes"])
    runtime_process = subprocess.run(
        [
            str(runtime),
            "--require-version",
            str(preregistration["candidate"]["required_version"]),
            "--format",
            "json",
            "runtime-info",
        ],
        cwd=ROOT,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if runtime_process.returncode != 0:
        raise RuntimeError(
            "candidate runtime-info failed before fixture preparation: "
            + runtime_process.stderr
        )
    runtime_info = json.loads(runtime_process.stdout)
    identity_env = os.environ.copy()
    identity_env["PROJECTATLAS_NO_TELEMETRY"] = "1"
    mcp_client = McpClient(
        runtime,
        ROOT,
        identity_env,
        request_timeout_seconds=preregistration["thresholds"]["all"][
            "mcp_request_timeout_seconds"
        ],
    )
    try:
        mcp_tools, _ = mcp_client.tools()
    finally:
        mcp_client.close()
    mcp_tools_sha256 = hashlib.sha256(
        json.dumps(
            mcp_tools, separators=(",", ":"), ensure_ascii=False
        ).encode("utf-8")
    ).hexdigest()
    status_rows = subprocess.check_output(
        ["git", "status", "--porcelain", "--untracked-files=all"],
        cwd=ROOT,
        text=True,
    ).splitlines()
    dirty_paths = [
        row[3:].split(" -> ", 1)[-1].replace("\\", "/")
        for row in status_rows
        if len(row) > 3
    ]
    errors = publication_identity_errors(
        preregistration,
        runtime_sha256=runtime_sha256,
        mcp_tools_sha256=mcp_tools_sha256,
        skill_sha256=skill_sha256,
        skill_bytes=skill_bytes,
        runtime_info=runtime_info,
        dirty_paths=dirty_paths,
        measurement_errors=measurement_input_errors(
            preregistration,
            SYSTEM_SCALE_MEASUREMENT_INPUTS,
        ),
    )
    if errors:
        raise RuntimeError(
            "publication candidate identity rejected before fixture preparation: "
            + "; ".join(errors)
        )
    return (
        {
            "runtime_sha256": runtime_sha256,
            "mcp_tools_sha256": mcp_tools_sha256,
            "skill_path": skill_identity["path"],
            "skill_sha256": skill_sha256,
            "skill_bytes": skill_bytes,
            "runtime_info": runtime_info,
        },
        candidate_source_identity(preregistration_path),
    )


def write_result(result: dict[str, Any], output: Path) -> None:
    """Persist every result, then fail the command when any gate rejected it."""
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps(redact_local_paths(result), indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    print(output)
    if not result["passed"]:
        raise SystemExit(1)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runtime", type=Path, required=True)
    parser.add_argument("--preregistration", type=Path, default=DEFAULT_PREREGISTRATION)
    parser.add_argument("--work-root", type=Path, default=DEFAULT_WORK)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--corpus-cache", type=Path, default=DEFAULT_CORPUS_CACHE)
    parser.add_argument(
        "--only",
        choices=("small", "medium", "huge", "all"),
        default="all",
        help="Use small or medium only for harness smoke; publication requires all.",
    )
    args = parser.parse_args()
    try:
        run_benchmark(args)
    except Exception as error:
        write_result(
            {
                "schema_version": 1,
                "preregistration": str(args.preregistration.resolve()),
                "mode": args.only,
                "final_measurement_eligibility": final_measurement_eligibility(
                    args.only
                ),
                "publication_eligible": False,
                "passed": False,
                "failure": {
                    "type": type(error).__name__,
                    "message": str(error),
                },
            },
            args.output,
        )


def run_benchmark(args: argparse.Namespace) -> None:
    clear_git_repository_environment()
    runtime = args.runtime.resolve(strict=True)
    preregistration_path = args.preregistration.resolve(strict=True)
    preregistration = json.loads(preregistration_path.read_text(encoding="utf-8"))
    measurement_eligibility = final_measurement_eligibility(args.only)
    if (
        measurement_eligibility["requested"]
        and not measurement_eligibility["final_platform_eligible"]
    ):
        raise RuntimeError(measurement_eligibility["ineligible_reason"])
    if args.only == "all":
        publication_identity, source_identity = validate_publication_identity(
            runtime, preregistration, preregistration_path
        )
    else:
        publication_identity, source_identity = None, None
    work_root = args.work_root.resolve()
    allowed = (ROOT / "target/benchmarks/system-scale").resolve()
    if work_root == allowed or allowed not in work_root.parents:
        raise ValueError(f"--work-root must be a child of {allowed}")
    corpus_cache = args.corpus_cache.resolve()
    if corpus_cache != allowed and allowed not in corpus_cache.parents:
        raise ValueError(
            f"--corpus-cache must be {allowed} or one of its children"
        )
    if corpus_cache == work_root or work_root in corpus_cache.parents:
        raise ValueError("--corpus-cache must not be inside --work-root")
    if work_root.exists():
        remove_tree(work_root, allowed_parent=allowed)
    work_root.mkdir(parents=True)
    env = os.environ.copy()
    env["PROJECTATLAS_NO_TELEMETRY"] = "1"

    cases = []
    small = prepare_small(work_root)
    if args.only in {"small", "all"}:
        small_queries = {
            "clean": {
                "target_file": "src/storage.rs",
                "literal": "save_order",
                "regex": r"pub\s+fn",
                "file_pattern": "src/**/*.rs",
            },
            "dirty": {
                "target_file": "src/pricing.rs",
                "literal": "calculate_total",
                "regex": r"pub\s+fn",
                "file_pattern": "src/**/*.rs",
            },
            "non-git": {
                "target_file": "src/config.rs",
                "literal": "load_timeout_millis",
                "regex": r"pub\s+fn",
                "file_pattern": "src/**/*.rs",
            },
        }
        for variant, root in small.items():
            cases.append(
                run_case(
                    runtime,
                    root,
                    env,
                    scale="small",
                    variant=variant,
                    preregistration=preregistration,
                    query=small_queries[variant],
                )
            )

    medium = work_root / "medium"
    prepare_medium(medium, preregistration["corpora"]["medium"]["caller_files"])
    if args.only in {"medium", "all"}:
        cases.append(
            run_case(
                runtime,
                medium,
                env,
                scale="medium",
                variant="high-degree",
                preregistration=preregistration,
                query={
                    "target_file": "src/hub.rs",
                    "literal": "shared",
                    "regex": r"pub\s+fn",
                    "file_pattern": "src/**/*.rs",
                },
                incremental=True,
            )
        )

    huge = work_root / "huge-vscode"
    if args.only in {"huge", "all"}:
        prepare_huge(huge, preregistration["corpora"]["huge"], corpus_cache)
        cases.append(
            run_case(
                runtime,
                huge,
                env,
                scale="huge",
                variant="vscode-1.130.0",
                preregistration=preregistration,
                query={
                    "target_file": preregistration["corpora"]["huge"]["target_file"],
                    "literal": "createDecorator",
                    "regex": r"class\s+[A-Za-z_]+",
                    "file_pattern": "src/**/*.ts",
                },
            )
        )

    timeout = preregistration["thresholds"]["all"]["command_timeout_seconds"]
    concurrency = (
        concurrent_isolation(
            runtime,
            work_root,
            env,
            timeout,
            preregistration["corpora"]["medium"]["caller_files"],
            preregistration["thresholds"]["all"],
        )
        if args.only == "all"
        else None
    )
    contention = (
        publication_contention(
            runtime,
            work_root / "concurrent-b",
            env,
            timeout,
            preregistration["thresholds"]["all"][
                "maximum_contention_failure_seconds"
            ],
        )
        if args.only == "all"
        else None
    )
    cooperative_cancellation = (
        cooperative_cancellation_reopen(
            runtime,
            huge,
            env,
            preregistration["thresholds"]["all"][
                "maximum_cancellation_quiescence_seconds"
            ],
            preregistration["thresholds"]["all"][
                "mcp_request_timeout_seconds"
            ],
        )
        if args.only == "all"
        else None
    )
    default_core_parent_termination = (
        forced_termination_quiescence(
            runtime,
            huge,
            work_root,
            env,
            preregistration["thresholds"]["all"][
                "maximum_cancellation_quiescence_seconds"
            ],
        )
        if args.only == "all"
        else None
    )
    result = {
        "schema_version": 1,
        "preregistration": str(preregistration_path),
        "effective_preregistration": preregistration,
        "mode": args.only,
        "final_measurement_eligibility": measurement_eligibility,
        "publication_eligible": False,
        "candidate": {
            "version": subprocess.check_output([runtime, "--version"], text=True).strip(),
            "runtime": str(runtime),
            "runtime_sha256": hashlib.sha256(runtime.read_bytes()).hexdigest(),
            "runtime_bytes": runtime.stat().st_size,
            "publication_identity": publication_identity,
        },
        "candidate_source_identity": source_identity,
        "path_placeholders": PATH_PLACEHOLDERS,
        "environment": {
            "platform": platform.platform(),
            "python": platform.python_version(),
            "psutil": psutil.__version__,
            "logical_cpus": os.cpu_count(),
            "telemetry": "disabled",
        },
        "cases": cases,
        "concurrent_isolation": concurrency,
        "publication_contention": contention,
        "cooperative_cancellation": cooperative_cancellation,
        "default_core_parent_termination": default_core_parent_termination,
    }
    result["passed"] = (
        all(check["passed"] for case in cases for check in case["checks"])
        and (
            args.only != "all"
            or measurement_eligibility["final_platform_eligible"]
        )
        and (concurrency is None or concurrency["passed"])
        and (contention is None or contention["passed"])
        and (
            cooperative_cancellation is None
            or cooperative_cancellation["passed"]
        )
        and (
            default_core_parent_termination is None
            or default_core_parent_termination["passed"]
        )
    )
    result["publication_eligible"] = (
        args.only == "all"
        and measurement_eligibility["final_platform_eligible"]
        and result["passed"]
    )
    write_result(result, args.output)


if __name__ == "__main__":
    main()
