#!/usr/bin/env python3
"""Reproduce the ProjectAtlas v0.4 MCP composition evaluation."""

from __future__ import annotations

import argparse
import copy
import ctypes
import errno
import hashlib
import json
import os
import platform
import queue
import re
import signal
import shutil
import stat
import statistics
import subprocess
import tempfile
import threading
import time
from ctypes import wintypes
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[3]
FIXTURES = ROOT / "docs/benchmarks/fixtures/mcp-composition"
REQUESTS = ROOT / "docs/benchmarks/v0.4-mcp-composition-requests.json"
DEFAULT_WORK = ROOT / "target/benchmarks/mcp-composition/current"
DEFAULT_OUTPUT = ROOT / "docs/benchmarks/v0.4-mcp-composition-raw.json"
CONTINUATION = re.compile(r'^\s+(?:continuation|cursor): "(.*)"$', re.MULTILINE)
PURPOSES = {
    "clean": {
        ".": "Own the clean Git navigation benchmark crate.",
        "src": "Own clean order-submission benchmark source.",
        ".gitignore": "Exclude generated Cargo and ProjectAtlas state.",
        "Cargo.toml": "Define the clean benchmark crate.",
        "src/api.rs": "Expose public order submission orchestration.",
        "src/lib.rs": "Define the clean benchmark crate boundary and order types.",
        "src/service.rs": "Validate order quantities and construct accepted orders.",
        "src/states.rs": "Exercise every static relation-resolution state.",
        "src/storage.rs": "Reject unassigned order identifiers before durable writes.",
    },
    "dirty": {
        ".": "Own the dirty-worktree navigation benchmark crate.",
        "src": "Own dirty checkout-policy benchmark source.",
        ".gitignore": "Exclude generated Cargo and ProjectAtlas state.",
        "Cargo.toml": "Define the dirty-worktree benchmark crate.",
        "src/checkout.rs": "Expose the public checkout-total entrypoint.",
        "src/lib.rs": "Define the dirty benchmark crate boundary and line-item type.",
        "src/pricing.rs": "Own checkout subtotal and discount policy.",
        "src/states.rs": "Exercise every static relation-resolution state.",
    },
    "non-git": {
        ".": "Own the non-Git navigation benchmark crate.",
        "src": "Own non-Git health-route benchmark source.",
        "Cargo.toml": "Define the non-Git benchmark crate.",
        "src/config.rs": "Own the health-response timeout configuration.",
        "src/handler.rs": "Build the health response from current configuration.",
        "src/lib.rs": "Define the non-Git benchmark crate boundary.",
        "src/router.rs": "Expose path dispatch for the public health route.",
        "src/states.rs": "Exercise every static relation-resolution state.",
    },
}
ORACLES = {
    "clean": {
        "Q1": ["save_order", "order id must be assigned"],
        "Q2": ["submit_order", "status: resolved"],
        "Q3": ["src/storage.rs", "src/api.rs"],
        "Q4": [
            "status: resolved",
            "status: ambiguous",
            "status: unresolved",
            "status: external",
        ],
        "Q5": ["generation:"],
        "Q6": ["submit_order", "start_line:", "end_line:"],
    },
    "dirty": {
        "Q1": ["apply_discount", "10_000", "1_000"],
        "Q2": ["calculate_total", "checkout_total", "status: resolved"],
        "Q3": ["src/pricing.rs", "src/checkout.rs"],
        "Q4": [
            "status: resolved",
            "status: ambiguous",
            "status: unresolved",
            "status: external",
        ],
        "Q5": ["generation:"],
        "Q6": ["calculate_total", "start_line:", "end_line:"],
    },
    "non-git": {
        "Q1": ["load_timeout_millis", "250"],
        "Q2": ["health_response", "dispatch", "status: resolved"],
        "Q3": ["src/config.rs", "src/handler.rs"],
        "Q4": [
            "status: resolved",
            "status: ambiguous",
            "status: unresolved",
            "status: external",
        ],
        "Q5": ["generation:"],
        "Q6": ["health_response", "start_line:", "end_line:"],
    },
}
SELECTOR_ORACLES = {
    "clean": {
        "Q1": ["src/storage.rs", "save_order"],
        "Q2": ["src/api.rs", "submit_order"],
        "Q3": ["src/storage.rs", "src/api.rs"],
        "Q4": ["src/states.rs", "inspect_states"],
        "Q5": ["src/storage.rs", "save_order"],
        "Q6": ["src/api.rs", "submit_order"],
    },
    "dirty": {
        "Q1": ["src/pricing.rs", "apply_discount"],
        "Q2": ["src/checkout.rs", "checkout_total"],
        "Q3": ["src/pricing.rs", "src/checkout.rs"],
        "Q4": ["src/states.rs", "inspect_states"],
        "Q5": ["src/pricing.rs", "calculate_total"],
        "Q6": ["src/pricing.rs", "calculate_total"],
    },
    "non-git": {
        "Q1": ["src/config.rs", "load_timeout_millis"],
        "Q2": ["src/router.rs", "dispatch"],
        "Q3": ["src/config.rs", "src/handler.rs"],
        "Q4": ["src/states.rs", "inspect_states"],
        "Q5": ["src/config.rs", "load_timeout_millis"],
        "Q6": ["src/handler.rs", "health_response"],
    },
}
TRUST_ORACLES = {
    "Q1": ["index_status: available", "start_line:", "end_line:"],
    "Q2": ["generation:", "status: resolved", "coverage[", "next_call:"],
    "Q3": ["index_status: available", "connections[", "next_call:"],
    "Q4": ["generation:", "coverage[", "next_call:"],
    "Q5": ["generation:", "coverage[", "status: resolved"],
    "Q6": ["generation:", "status: resolved", "start_line:", "end_line:"],
}
ARM_C_SCHEMA = {
    "name": "atlas_relation_slice",
    "description": (
        "Return one compact detailed relation page and the exact source slice "
        "selected by its first reusable local next call."
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "project_path": {"type": ["string", "null"]},
            "file": {"type": "string"},
            "symbol": {"type": "string"},
            "symbol_kind": {"type": ["string", "null"]},
            "symbol_parent": {"type": ["string", "null"]},
            "symbol_signature": {"type": ["string", "null"]},
            "direction": {"enum": ["inbound", "outbound", "both"]},
            "relation": {"type": ["string", "null"]},
            "depth": {"type": "integer", "minimum": 1},
            "include_occurrences": {"type": "boolean"},
            "limit": {"type": "integer", "minimum": 1},
            "output_bytes": {"type": "integer", "minimum": 2048},
        },
        "required": ["file", "symbol"],
        "additionalProperties": False,
    },
}
MCP_REQUEST_TIMEOUT_SECONDS = 60.0
OWNED_PROCESS_CLEANUP_SECONDS = 5.0
WINDOWS_CREATE_SUSPENDED = 0x00000004


class WindowsIoCounters(ctypes.Structure):
    _fields_ = [
        ("read_operations", ctypes.c_ulonglong),
        ("write_operations", ctypes.c_ulonglong),
        ("other_operations", ctypes.c_ulonglong),
        ("read_bytes", ctypes.c_ulonglong),
        ("write_bytes", ctypes.c_ulonglong),
        ("other_bytes", ctypes.c_ulonglong),
    ]


class WindowsJobBasicAccounting(ctypes.Structure):
    _fields_ = [
        ("total_user_time", ctypes.c_longlong),
        ("total_kernel_time", ctypes.c_longlong),
        ("this_period_user_time", ctypes.c_longlong),
        ("this_period_kernel_time", ctypes.c_longlong),
        ("total_page_fault_count", wintypes.DWORD),
        ("total_processes", wintypes.DWORD),
        ("active_processes", wintypes.DWORD),
        ("total_terminated_processes", wintypes.DWORD),
    ]


class WindowsJobBasicAndIoAccounting(ctypes.Structure):
    _fields_ = [
        ("basic", WindowsJobBasicAccounting),
        ("io", WindowsIoCounters),
    ]


class WindowsJobBasicLimit(ctypes.Structure):
    _fields_ = [
        ("per_process_user_time_limit", ctypes.c_longlong),
        ("per_job_user_time_limit", ctypes.c_longlong),
        ("limit_flags", wintypes.DWORD),
        ("minimum_working_set_size", ctypes.c_size_t),
        ("maximum_working_set_size", ctypes.c_size_t),
        ("active_process_limit", wintypes.DWORD),
        ("affinity", ctypes.c_size_t),
        ("priority_class", wintypes.DWORD),
        ("scheduling_class", wintypes.DWORD),
    ]


class WindowsJobExtendedLimit(ctypes.Structure):
    _fields_ = [
        ("basic_limit_information", WindowsJobBasicLimit),
        ("io_info", WindowsIoCounters),
        ("process_memory_limit", ctypes.c_size_t),
        ("job_memory_limit", ctypes.c_size_t),
        ("peak_process_memory_used", ctypes.c_size_t),
        ("peak_job_memory_used", ctypes.c_size_t),
    ]


class WindowsThreadEntry(ctypes.Structure):
    _fields_ = [
        ("size", wintypes.DWORD),
        ("usage", wintypes.DWORD),
        ("thread_id", wintypes.DWORD),
        ("owner_process_id", wintypes.DWORD),
        ("base_priority", wintypes.LONG),
        ("delta_priority", wintypes.LONG),
        ("flags", wintypes.DWORD),
    ]


class WindowsJob:
    """Own one suspended-before-assignment Windows process tree."""

    _JOB_OBJECT_EXTENDED_LIMIT_INFORMATION = 9
    _JOB_OBJECT_BASIC_AND_IO_ACCOUNTING_INFORMATION = 8
    _JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000
    _PROCESS_TERMINATE = 0x0001
    _PROCESS_SET_QUOTA = 0x0100
    _THREAD_SUSPEND_RESUME = 0x0002
    _TH32CS_SNAPTHREAD = 0x00000004
    _ERROR_NO_MORE_FILES = 18

    def __init__(self) -> None:
        if os.name != "nt":
            raise RuntimeError("Windows Job ownership is only available on Windows")
        self.kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        self._configure_signatures()
        self.handle = self.kernel32.CreateJobObjectW(None, None)
        if not self.handle:
            raise ctypes.WinError(ctypes.get_last_error())
        self.thread_handle: int | None = None
        limits = WindowsJobExtendedLimit()
        limits.basic_limit_information.limit_flags = (
            self._JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        )
        if not self.kernel32.SetInformationJobObject(
            self.handle,
            self._JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
            ctypes.byref(limits),
            ctypes.sizeof(limits),
        ):
            error = ctypes.WinError(ctypes.get_last_error())
            self.close()
            raise error

    def _configure_signatures(self) -> None:
        self.kernel32.CreateJobObjectW.argtypes = [wintypes.LPVOID, wintypes.LPCWSTR]
        self.kernel32.CreateJobObjectW.restype = wintypes.HANDLE
        self.kernel32.SetInformationJobObject.argtypes = [
            wintypes.HANDLE,
            ctypes.c_int,
            wintypes.LPVOID,
            wintypes.DWORD,
        ]
        self.kernel32.SetInformationJobObject.restype = wintypes.BOOL
        self.kernel32.AssignProcessToJobObject.argtypes = [
            wintypes.HANDLE,
            wintypes.HANDLE,
        ]
        self.kernel32.AssignProcessToJobObject.restype = wintypes.BOOL
        self.kernel32.QueryInformationJobObject.argtypes = [
            wintypes.HANDLE,
            ctypes.c_int,
            wintypes.LPVOID,
            wintypes.DWORD,
            wintypes.LPVOID,
        ]
        self.kernel32.QueryInformationJobObject.restype = wintypes.BOOL
        self.kernel32.TerminateJobObject.argtypes = [wintypes.HANDLE, wintypes.UINT]
        self.kernel32.TerminateJobObject.restype = wintypes.BOOL
        self.kernel32.OpenProcess.argtypes = [
            wintypes.DWORD,
            wintypes.BOOL,
            wintypes.DWORD,
        ]
        self.kernel32.OpenProcess.restype = wintypes.HANDLE
        self.kernel32.CreateToolhelp32Snapshot.argtypes = [
            wintypes.DWORD,
            wintypes.DWORD,
        ]
        self.kernel32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
        self.kernel32.Thread32First.argtypes = [
            wintypes.HANDLE,
            ctypes.POINTER(WindowsThreadEntry),
        ]
        self.kernel32.Thread32First.restype = wintypes.BOOL
        self.kernel32.Thread32Next.argtypes = [
            wintypes.HANDLE,
            ctypes.POINTER(WindowsThreadEntry),
        ]
        self.kernel32.Thread32Next.restype = wintypes.BOOL
        self.kernel32.OpenThread.argtypes = [
            wintypes.DWORD,
            wintypes.BOOL,
            wintypes.DWORD,
        ]
        self.kernel32.OpenThread.restype = wintypes.HANDLE
        self.kernel32.ResumeThread.argtypes = [wintypes.HANDLE]
        self.kernel32.ResumeThread.restype = wintypes.DWORD
        self.kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
        self.kernel32.CloseHandle.restype = wintypes.BOOL

    def assign_suspended(self, process_id: int) -> None:
        process_handle = self.kernel32.OpenProcess(
            self._PROCESS_TERMINATE | self._PROCESS_SET_QUOTA,
            False,
            process_id,
        )
        if not process_handle:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            if not self.kernel32.AssignProcessToJobObject(
                self.handle, process_handle
            ):
                raise ctypes.WinError(ctypes.get_last_error())
        finally:
            self.kernel32.CloseHandle(process_handle)
        self.thread_handle = self._open_only_thread(process_id)

    def _open_only_thread(self, process_id: int) -> int:
        snapshot = self.kernel32.CreateToolhelp32Snapshot(
            self._TH32CS_SNAPTHREAD, 0
        )
        if snapshot == ctypes.c_void_p(-1).value:
            raise ctypes.WinError(ctypes.get_last_error())
        thread_ids: list[int] = []
        try:
            entry = WindowsThreadEntry()
            entry.size = ctypes.sizeof(entry)
            found = self.kernel32.Thread32First(snapshot, ctypes.byref(entry))
            while found:
                if entry.owner_process_id == process_id:
                    thread_ids.append(int(entry.thread_id))
                entry.size = ctypes.sizeof(entry)
                found = self.kernel32.Thread32Next(snapshot, ctypes.byref(entry))
            error = ctypes.get_last_error()
            if error not in (0, self._ERROR_NO_MORE_FILES):
                raise ctypes.WinError(error)
        finally:
            self.kernel32.CloseHandle(snapshot)
        if len(thread_ids) != 1:
            raise RuntimeError(
                f"suspended process {process_id} exposed {len(thread_ids)} threads"
            )
        thread_handle = self.kernel32.OpenThread(
            self._THREAD_SUSPEND_RESUME, False, thread_ids[0]
        )
        if not thread_handle:
            raise ctypes.WinError(ctypes.get_last_error())
        return int(thread_handle)

    def resume(self) -> None:
        if self.thread_handle is None:
            return
        previous_count = self.kernel32.ResumeThread(self.thread_handle)
        if previous_count != 1:
            error = (
                ctypes.WinError(ctypes.get_last_error())
                if previous_count == 0xFFFFFFFF
                else RuntimeError(
                    f"primary thread suspend count was {previous_count}, expected 1"
                )
            )
            self.kernel32.CloseHandle(self.thread_handle)
            self.thread_handle = None
            raise error
        self.kernel32.CloseHandle(self.thread_handle)
        self.thread_handle = None

    def accounting(self) -> dict[str, int]:
        counters = WindowsJobBasicAndIoAccounting()
        if not self.kernel32.QueryInformationJobObject(
            self.handle,
            self._JOB_OBJECT_BASIC_AND_IO_ACCOUNTING_INFORMATION,
            ctypes.byref(counters),
            ctypes.sizeof(counters),
            None,
        ):
            raise ctypes.WinError(ctypes.get_last_error())
        return {
            "user_time_100ns": int(counters.basic.total_user_time),
            "kernel_time_100ns": int(counters.basic.total_kernel_time),
            "total_processes": int(counters.basic.total_processes),
            "active_processes": int(counters.basic.active_processes),
            "terminated_processes": int(
                counters.basic.total_terminated_processes
            ),
            "read_count": int(counters.io.read_operations),
            "write_count": int(counters.io.write_operations),
            "other_count": int(counters.io.other_operations),
            "read_bytes": int(counters.io.read_bytes),
            "write_bytes": int(counters.io.write_bytes),
            "other_bytes": int(counters.io.other_bytes),
        }

    def wait_for_zero_active(self, timeout_seconds: float) -> dict[str, int]:
        deadline = time.monotonic() + timeout_seconds
        while True:
            accounting = self.accounting()
            if accounting["active_processes"] == 0:
                return accounting
            if time.monotonic() >= deadline:
                raise TimeoutError("Windows Job retained active processes")
            time.sleep(0.005)

    def terminate(self) -> None:
        if self.handle and not self.kernel32.TerminateJobObject(self.handle, 1):
            error = ctypes.get_last_error()
            if error != 6:
                raise ctypes.WinError(error)

    def close(self) -> None:
        if self.thread_handle is not None:
            self.kernel32.CloseHandle(self.thread_handle)
            self.thread_handle = None
        if getattr(self, "handle", None):
            self.kernel32.CloseHandle(self.handle)
            self.handle = None


def spawn_owned_process(
    arguments: list[str], **kwargs: Any
) -> tuple[subprocess.Popen[Any], WindowsJob | None]:
    """Spawn a process in a private tree that can be killed and reaped as a unit."""
    if os.name != "nt":
        return subprocess.Popen(
            arguments, start_new_session=True, **kwargs
        ), None
    job = WindowsJob()
    process: subprocess.Popen[Any] | None = None
    try:
        creationflags = int(kwargs.pop("creationflags", 0))
        process = subprocess.Popen(
            arguments,
            creationflags=creationflags | WINDOWS_CREATE_SUSPENDED,
            **kwargs,
        )
        job.assign_suspended(process.pid)
        return process, job
    except BaseException as error:
        try:
            job.terminate()
        except BaseException as cleanup_error:
            error.add_note(f"Windows Job termination failed: {cleanup_error}")
        if process is not None:
            try:
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=OWNED_PROCESS_CLEANUP_SECONDS)
            except BaseException as cleanup_error:
                error.add_note(f"root process reap failed: {cleanup_error}")
        job.close()
        raise


def terminate_owned_process(
    process: subprocess.Popen[Any],
    job: WindowsJob | None,
    timeout_seconds: float = OWNED_PROCESS_CLEANUP_SECONDS,
) -> None:
    """Kill the complete owned process tree and boundedly reap its root."""
    cleanup_error: BaseException | None = None
    try:
        if job is not None and job.handle:
            job.terminate()
            job.wait_for_zero_active(timeout_seconds)
        elif job is None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
    except BaseException as error:
        cleanup_error = error
    finally:
        if job is not None:
            job.close()
    try:
        process.wait(timeout=timeout_seconds)
    except BaseException as reap_error:
        try:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=timeout_seconds)
        except BaseException as fallback_error:
            reap_error.add_note(f"forced root process reap failed: {fallback_error}")
        if cleanup_error is None:
            cleanup_error = reap_error
        else:
            cleanup_error.add_note(f"root process reap failed: {reap_error}")
    if cleanup_error is not None:
        raise cleanup_error


def clear_git_repository_environment() -> None:
    variables = subprocess.check_output(
        ["git", "rev-parse", "--local-env-vars"], cwd=ROOT, text=True
    ).splitlines()
    for variable in variables:
        os.environ.pop(variable, None)


def command(*args: str, cwd: Path, env: dict[str, str] | None = None) -> None:
    subprocess.run(
        args,
        cwd=cwd,
        env=env,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def self_test_git_environment_isolation() -> None:
    clear_git_repository_environment()
    with tempfile.TemporaryDirectory(prefix="projectatlas-git-environment-") as root:
        sentinel = Path(root) / "sentinel"
        fixture = Path(root) / "fixture"
        sentinel.mkdir()
        fixture.mkdir()
        command("git", "init", "-q", cwd=sentinel)
        command("git", "config", "user.name", "ProjectAtlas Benchmark", cwd=sentinel)
        command(
            "git",
            "config",
            "user.email",
            "benchmark@projectatlas.invalid",
            cwd=sentinel,
        )
        (sentinel / "sentinel.txt").write_text(
            "preserve\n", encoding="utf-8", newline="\n"
        )
        command("git", "add", ".", cwd=sentinel)
        command("git", "commit", "-q", "-m", "sentinel", cwd=sentinel)
        before = (
            subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=sentinel, text=True
            ),
            (sentinel / ".git/config").read_bytes(),
            subprocess.check_output(
                ["git", "status", "--porcelain"], cwd=sentinel, text=True
            ),
        )
        os.environ["GIT_DIR"] = str(sentinel / ".git")
        clear_git_repository_environment()
        command("git", "init", "-q", cwd=fixture)
        after = (
            subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=sentinel, text=True
            ),
            (sentinel / ".git/config").read_bytes(),
            subprocess.check_output(
                ["git", "status", "--porcelain"], cwd=sentinel, text=True
            ),
        )
        if not (fixture / ".git").is_dir() or before != after:
            raise RuntimeError("Git fixture isolation changed the sentinel repository")


def remove_tree(path: Path, *, allowed_parent: Path) -> None:
    absolute = Path(os.path.abspath(path))
    allowed = Path(os.path.abspath(allowed_parent))
    if absolute == allowed or allowed not in absolute.parents:
        raise ValueError(f"cleanup target must be a child of {allowed}")
    try:
        metadata = absolute.lstat()
    except FileNotFoundError:
        return
    if stat.S_ISLNK(metadata.st_mode) or (
        os.name == "nt"
        and metadata.st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT
    ):
        raise ValueError(f"refusing to recursively remove linked path {absolute}")

    remove_path = absolute
    if os.name == "nt":
        path_text = str(absolute)
        if not path_text.startswith("\\\\?\\"):
            path_text = (
                f"\\\\?\\UNC\\{path_text[2:]}"
                if path_text.startswith("\\\\")
                else f"\\\\?\\{path_text}"
            )
        remove_path = Path(path_text)

    def retry(function: Any, target: str, _: Any) -> None:
        try:
            os.chmod(target, stat.S_IWRITE)
            function(target)
        except FileNotFoundError:
            pass

    for attempt in range(3):
        try:
            shutil.rmtree(remove_path, onerror=retry)
            return
        except FileNotFoundError:
            return
        except OSError as error:
            directory_not_empty = (
                error.errno == errno.ENOTEMPTY
                or getattr(error, "winerror", None) == 145
            )
            if not directory_not_empty or attempt == 2:
                raise


def prepare_fixture(name: str, destination: Path, runtime: Path, env: dict[str, str]) -> None:
    source = FIXTURES / name
    shutil.copytree(source, destination, ignore=shutil.ignore_patterns("current"))
    if name != "non-git":
        command("git", "init", "-q", cwd=destination)
        command("git", "config", "user.name", "ProjectAtlas Benchmark", cwd=destination)
        command("git", "config", "user.email", "benchmark@projectatlas.invalid", cwd=destination)
        command("git", "add", ".", cwd=destination)
        command("git", "commit", "-q", "-m", "benchmark fixture", cwd=destination)
    if name == "dirty":
        shutil.copy2(source / "current/pricing.rs", destination / "src/pricing.rs")
    command(str(runtime), "init", "--force-rescan", cwd=destination, env=env)
    for path, purpose in PURPOSES[name].items():
        command(str(runtime), "purpose", "set", path, purpose, cwd=destination, env=env)
    if name == "clean":
        status = subprocess.check_output(
            ["git", "status", "--porcelain"], cwd=destination, text=True
        )
        if status:
            raise RuntimeError(f"clean fixture is dirty after preparation: {status}")
    elif name == "dirty":
        status = subprocess.check_output(
            ["git", "status", "--porcelain"], cwd=destination, text=True
        )
        if status.strip() != "M src/pricing.rs":
            raise RuntimeError(f"dirty fixture state drifted: {status}")
    elif (destination / ".git").exists():
        raise RuntimeError("non-Git fixture unexpectedly contains .git")


class McpClient:
    def __init__(
        self,
        runtime: Path,
        fixture: Path,
        env: dict[str, str],
        request_timeout_seconds: float = MCP_REQUEST_TIMEOUT_SECONDS,
    ) -> None:
        if request_timeout_seconds <= 0:
            raise ValueError("MCP request timeout must be positive")
        self.request_timeout_seconds = request_timeout_seconds
        self.process, self.job = spawn_owned_process(
            [str(runtime), "--require-version", "0.4.0", "mcp"],
            cwd=fixture,
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            encoding="utf-8",
        )
        self.closed = False
        self.next_id = 1
        try:
            if self.job is not None:
                self.job.resume()
            started = time.perf_counter()
            self.request(
                "initialize",
                {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "mcp-composition-benchmark",
                        "version": "1",
                    },
                },
            )
            self.notify("notifications/initialized", {})
            self.startup_ms = (time.perf_counter() - started) * 1000
        except BaseException as error:
            self._terminate_after(error)
            raise

    def notify(self, method: str, params: dict[str, Any]) -> None:
        try:
            self._write({"jsonrpc": "2.0", "method": method, "params": params})
        except BaseException as error:
            self._terminate_after(error)
            raise

    def request(self, method: str, params: dict[str, Any]) -> tuple[dict[str, Any], float]:
        request_id = self.next_id
        self.next_id += 1
        started = time.perf_counter()
        try:
            self._write(
                {
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "method": method,
                    "params": params,
                }
            )
        except BaseException as error:
            self._terminate_after(error)
            raise
        assert self.process.stdout is not None
        result: queue.Queue[dict[str, Any] | Exception] = queue.Queue(maxsize=1)

        def read_response() -> None:
            try:
                while line := self.process.stdout.readline():
                    response = json.loads(line)
                    if response.get("id") == request_id:
                        result.put(response)
                        return
                result.put(RuntimeError(f"MCP process ended before response {request_id}"))
            except Exception as error:
                result.put(error)

        reader = threading.Thread(
            target=read_response,
            name=f"projectatlas-mcp-response-{request_id}",
            daemon=True,
        )
        reader.start()
        try:
            response = result.get(timeout=self.request_timeout_seconds)
        except queue.Empty as error:
            timeout_error = TimeoutError(
                f"MCP request {method!r} exceeded {self.request_timeout_seconds:.3f} seconds"
            )
            self._terminate_after(timeout_error)
            self._join_reader(reader, timeout_error)
            raise timeout_error from error
        self._join_reader(reader)
        if isinstance(response, Exception):
            self._terminate_after(response)
            raise response
        elapsed_ms = (time.perf_counter() - started) * 1000
        if response.get("error") is not None:
            raise RuntimeError(json.dumps(response["error"], sort_keys=True))
        return response, elapsed_ms

    def call(self, name: str, arguments: dict[str, Any]) -> tuple[str, float]:
        response, elapsed_ms = self.request(
            "tools/call", {"name": name, "arguments": arguments}
        )
        return str(response["result"]["content"][0]["text"]), elapsed_ms

    def tools(self) -> tuple[list[dict[str, Any]], float]:
        response, elapsed_ms = self.request("tools/list", {})
        return list(response["result"]["tools"]), elapsed_ms

    def close(self) -> None:
        if self.closed:
            return
        if self.process.stdin is not None and not self.process.stdin.closed:
            try:
                self.process.stdin.close()
            except BrokenPipeError:
                pass
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            pass
        self._terminate()

    def _terminate(self) -> None:
        if self.closed:
            return
        self.closed = True
        try:
            terminate_owned_process(self.process, self.job)
        finally:
            for stream in (self.process.stdin, self.process.stdout):
                if stream is not None and not stream.closed:
                    try:
                        stream.close()
                    except OSError:
                        pass

    def _terminate_after(self, primary_error: BaseException) -> None:
        try:
            self._terminate()
        except BaseException as cleanup_error:
            primary_error.add_note(f"MCP process cleanup failed: {cleanup_error}")

    def _join_reader(
        self,
        reader: threading.Thread,
        primary_error: BaseException | None = None,
    ) -> None:
        reader.join(OWNED_PROCESS_CLEANUP_SECONDS)
        if reader.is_alive():
            error = RuntimeError(f"MCP response reader {reader.name!r} did not stop")
            if primary_error is None:
                self._terminate_after(error)
                raise error
            primary_error.add_note(str(error))

    def _write(self, payload: dict[str, Any]) -> None:
        assert self.process.stdin is not None
        self.process.stdin.write(json.dumps(payload, separators=(",", ":")) + "\n")
        self.process.stdin.flush()


def run_arm(
    runtime: Path,
    fixture_name: str,
    fixture: Path,
    calls: list[dict[str, Any]],
    compact: bool,
    env: dict[str, str],
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    client = McpClient(runtime, fixture, env)
    rows: list[dict[str, Any]] = []
    continuation: str | None = None
    try:
        tools, discovery_ms = client.tools()
        discovery_json = json.dumps(tools, separators=(",", ":"), ensure_ascii=False)
        for requested in calls:
            call = copy.deepcopy(requested)
            arguments = call["arguments"]
            if compact and call["name"] == "atlas_symbol_relations" and arguments.get("view") == "detailed":
                arguments["compact"] = True
            if arguments.get("cursor") == "__previous_continuation__":
                if continuation is None:
                    raise RuntimeError(f"{fixture_name} {call['step']} has no continuation")
                arguments["cursor"] = continuation
            text, elapsed_ms = client.call(call["name"], arguments)
            match = CONTINUATION.search(text)
            continuation = json.loads(f'"{match.group(1)}"') if match else None
            rows.append(
                {
                    "fixture": fixture_name,
                    "arm": "compact" if compact else "full",
                    "question": call["question"],
                    "step": call["step"],
                    "name": call["name"],
                    "arguments": arguments,
                    "response_bytes": len(text.encode("utf-8")),
                    "elapsed_ms": round(elapsed_ms, 4),
                    "response_sha256": hashlib.sha256(text.encode("utf-8")).hexdigest(),
                    "response_text": text,
                }
            )
        correctness = {}
        for question, needles in ORACLES[fixture_name].items():
            question_rows = [row for row in rows if row["question"] == question]
            question_text = "\n".join(row["response_text"] for row in question_rows)
            missing = [needle for needle in needles if needle not in question_text]
            missing_selectors = [
                needle
                for needle in SELECTOR_ORACLES[fixture_name][question]
                if needle not in question_text
            ]
            missing_trust = [
                needle for needle in TRUST_ORACLES[question] if needle not in question_text
            ]
            rubric = {
                "correct": not missing,
                "selector_correct": not missing_selectors,
                "trust_correct": not missing_trust,
                "missing": missing,
                "missing_selectors": missing_selectors,
                "missing_trust": missing_trust,
                "backtracked": False,
            }
            rubric["disposition"] = (
                "pass"
                if rubric["correct"]
                and rubric["selector_correct"]
                and rubric["trust_correct"]
                else "fail"
            )
            correctness[question] = rubric
            for row in question_rows:
                row["route_rubric"] = copy.deepcopy(rubric)
        summary = summarize_arm(
            fixture_name,
            "compact" if compact else "full",
            client.startup_ms,
            discovery_json,
            discovery_ms,
            rows,
            correctness,
        )
        return summary, rows
    finally:
        client.close()


def summarize_arm(
    fixture: str,
    arm: str,
    startup_ms: float,
    discovery_json: str,
    discovery_ms: float,
    rows: list[dict[str, Any]],
    correctness: dict[str, Any],
) -> dict[str, Any]:
    relations = [row for row in rows if row["name"] == "atlas_symbol_relations"]
    question_bytes = {
        question: sum(row["response_bytes"] for row in rows if row["question"] == question)
        for question in ORACLES[fixture]
    }
    question_ms = {
        question: sum(row["elapsed_ms"] for row in rows if row["question"] == question)
        for question in ORACLES[fixture]
    }
    return {
        "fixture": fixture,
        "arm": arm,
        "correct": all(value["disposition"] == "pass" for value in correctness.values()),
        "correctness": correctness,
        "startup_ms": round(startup_ms, 4),
        "discovery_bytes": len(discovery_json.encode("utf-8")),
        "discovery_elapsed_ms": round(discovery_ms, 4),
        "calls": len(rows),
        "emitted_bytes": sum(row["response_bytes"] for row in rows),
        "route_elapsed_ms": round(sum(row["elapsed_ms"] for row in rows), 4),
        "median_relation_bytes": statistics.median(
            row["response_bytes"] for row in relations
        ),
        "median_relation_elapsed_ms": round(
            statistics.median(row["elapsed_ms"] for row in relations), 4
        ),
        "median_question_bytes": statistics.median(question_bytes.values()),
        "median_question_elapsed_ms": round(statistics.median(question_ms.values()), 4),
        "question_bytes": question_bytes,
    }


def run_freshness(runtime: Path, fixture: Path, env: dict[str, str]) -> dict[str, Any]:
    source = fixture / "src/pricing.rs"
    original = source.read_text(encoding="utf-8")
    client = McpClient(runtime, fixture, env)
    arguments = {
        "file": "src/pricing.rs",
        "symbol": "calculate_total",
        "view": "detailed",
        "compact": True,
        "direction": "outbound",
        "depth": 2,
        "include_occurrences": True,
        "limit": 10,
        "output_bytes": 65536,
    }
    try:
        before, _ = client.call("atlas_symbol_relations", arguments)
        before_generation = int(re.search(r"^  generation: (\d+)$", before, re.MULTILINE).group(1))
        source.write_text(original.replace("10_000", "12_000"), encoding="utf-8")
        time.sleep(0.5)
        after, _ = client.call("atlas_symbol_relations", arguments)
        after_generation = int(re.search(r"^  generation: (\d+)$", after, re.MULTILINE).group(1))
        sliced, _ = client.call(
            "atlas_slice", {"file": "src/pricing.rs", "start_line": 10, "end_line": 16}
        )
        return {
            "before_generation": before_generation,
            "after_generation": after_generation,
            "generation_advanced": after_generation > before_generation,
            "relation_still_resolved": "status: resolved" in after,
            "current_slice_contains_edit": "12_000" in sliced,
            "after_relation_bytes": len(after.encode("utf-8")),
            "slice_bytes": len(sliced.encode("utf-8")),
        }
    finally:
        source.write_text(original, encoding="utf-8")
        client.close()


def run_bounds(runtime: Path, fixture: Path, env: dict[str, str]) -> list[dict[str, Any]]:
    rows = []
    for compact, limit in ((False, 4096), (True, 4096), (True, 2048)):
        client = McpClient(runtime, fixture, env)
        try:
            text, elapsed_ms = client.call(
                "atlas_symbol_relations",
                {
                    "file": "src/storage.rs",
                    "symbol": "save_order",
                    "view": "detailed",
                    "compact": compact,
                    "direction": "inbound",
                    "include_occurrences": True,
                    "limit": 20,
                    "output_bytes": limit,
                },
            )
            rows.append(
                {
                    "arm": "compact" if compact else "full",
                    "limit_bytes": limit,
                    "response_bytes": len(text.encode("utf-8")),
                    "elapsed_ms": round(elapsed_ms, 4),
                    "response_text": text,
                }
            )
        finally:
            client.close()
    return rows


def arm_c_analysis(summaries: list[dict[str, Any]], raw: list[dict[str, Any]]) -> dict[str, Any]:
    schema_json = json.dumps(ARM_C_SCHEMA, separators=(",", ":"), ensure_ascii=False)
    fixtures = []
    for fixture in ORACLES:
        q6 = [
            row
            for row in raw
            if row["fixture"] == fixture
            and row["arm"] == "compact"
            and row["question"] == "Q6"
        ]
        current_bytes = sum(row["response_bytes"] for row in q6)
        relation_lower_bound_bytes = next(
            row["response_bytes"]
            for row in q6
            if row["name"] == "atlas_symbol_relations"
        )
        fixtures.append(
            {
                "fixture": fixture,
                "applicable_question": "Q6",
                "current_calls": len(q6),
                "candidate_calls": 1,
                "current_bytes": current_bytes,
                "candidate_payload_lower_bound_bytes": relation_lower_bound_bytes,
                "maximum_possible_reduction_percent": round(
                    100.0
                    * (current_bytes - relation_lower_bound_bytes)
                    / current_bytes,
                    1,
                ),
            }
        )
    return {
        "design": ARM_C_SCHEMA,
        "schema_bytes": len(schema_json.encode("utf-8")),
        "achievable_selection_rule": (
            "remove at least one call from every applicable multi-call question and "
            "reduce those route bytes by at least 20 percent without losing trust"
        ),
        "median_question_calls_before": 1.5,
        "median_question_calls_after": 1.0,
        "fixtures": fixtures,
        "disposition": (
            "rejected: even granting an impossible zero-byte exact-source payload, "
            "the compact relation response leaves less than 20 percent achievable "
            "body-byte reduction, and the tool adds discovery schema"
        ),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--work-root", type=Path, default=DEFAULT_WORK)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test_git_environment_isolation()
        print("MCP composition harness self-test passed")
        return
    if args.runtime is None:
        parser.error("--runtime is required unless --self-test is used")
    clear_git_repository_environment()
    runtime = args.runtime.resolve(strict=True)
    work_root = args.work_root.resolve()
    allowed = (ROOT / "target/benchmarks/mcp-composition").resolve()
    if work_root == allowed or allowed not in work_root.parents:
        raise SystemExit(f"--work-root must be a child of {allowed}")
    if work_root.exists():
        remove_tree(work_root, allowed_parent=allowed)
    work_root.mkdir(parents=True)
    env = os.environ.copy()
    env["PROJECTATLAS_NO_TELEMETRY"] = "1"
    requests = json.loads(REQUESTS.read_text(encoding="utf-8"))
    prepared = {}
    for name in ORACLES:
        destination = work_root / name
        prepare_fixture(name, destination, runtime, env)
        prepared[name] = destination

    summaries = []
    raw = []
    for name, calls in requests["fixtures"].items():
        fixture_name = name.replace("non_git", "non-git")
        for compact in (False, True):
            summary, rows = run_arm(
                runtime, fixture_name, prepared[fixture_name], calls, compact, env
            )
            summaries.append(summary)
            raw.extend(rows)

    result = {
        "schema_version": 1,
        "candidate": {
            "version": subprocess.check_output([runtime, "--version"], text=True).strip(),
            "runtime": str(runtime),
            "runtime_sha256": hashlib.sha256(runtime.read_bytes()).hexdigest(),
            "runtime_bytes": runtime.stat().st_size,
            "git_base": subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
            ).strip(),
        },
        "environment": {
            "platform": platform.platform(),
            "python": platform.python_version(),
            "telemetry": "disabled",
        },
        "requests": str(REQUESTS.relative_to(ROOT)).replace("\\", "/"),
        "fixture_source": str(FIXTURES.relative_to(ROOT)).replace("\\", "/"),
        "summaries": summaries,
        "raw_calls": raw,
        "freshness": run_freshness(runtime, prepared["dirty"], env),
        "bounded_output": run_bounds(runtime, prepared["clean"], env),
    }
    result["arm_c"] = arm_c_analysis(summaries, raw)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(result, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    print(args.output)


if __name__ == "__main__":
    main()
