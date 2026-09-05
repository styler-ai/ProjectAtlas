#!/usr/bin/env python3
"""Run the ProjectAtlas #342 reverse-caller performance decision matrix."""

from __future__ import annotations

import argparse
import base64
import hashlib
import io
import json
import os
import queue
import sqlite3
import statistics
import subprocess
import tarfile
import tempfile
import threading
import time
from pathlib import Path
from typing import Any

import psutil


ROOT = Path(__file__).resolve().parents[3]
RUN_TIMEOUT_SECONDS = 120
SUMMARY_LIMIT = 500
CALLERS_PER_SYMBOL_LIMIT = 20
FROZEN_HIGH_IMPORT_IMPROVEMENT = 0.20
FROZEN_REPRESENTATIVE_IMPROVEMENT = 0.20
FROZEN_SMALL_REGRESSION = 0.05
FROZEN_HIGH_SYMBOL_REGRESSION = 0.05
FROZEN_DUPLICATE_REGRESSION = 0.05

SHAPES = {
    "small": (4, 1),
    "high-symbol": (320, 1),
    "high-import": (1, 240),
    "duplicate-alias": (1, 1),
    "representative-large": (120, 120),
}
LANGUAGES = ("rust", "typescript", "python")
RELEASE_PROFILE_DIRECTORY = "release"
RELEASE_BINARY_STEM = "projectatlas"
RAW_OUTPUT_DIRECTORY = Path("target/benchmarks/reverse-caller")
RESULT_SCHEMA = "projectatlas.reverse-caller-performance.v8"


def sha256_bytes(value: bytes) -> str:
    """Return the stable digest used for binary and raw-output identity."""

    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    """Hash one immutable benchmark input."""

    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run_process(
    command: list[str],
    cwd: Path,
    *,
    trace_path: Path | None = None,
    allocation_path: Path | None = None,
) -> dict[str, Any]:
    """Run one bounded child process and retain its exact raw streams."""

    environment = process_environment(
        trace_path=trace_path,
        allocation_path=allocation_path,
    )
    started = time.perf_counter()
    with tempfile.TemporaryFile() as stdout_file, tempfile.TemporaryFile() as stderr_file:
        process = subprocess.Popen(
            command,
            cwd=cwd,
            env=environment,
            stdout=stdout_file,
            stderr=stderr_file,
        )
        observed = psutil.Process(process.pid)
        peak_rss = 0
        cpu_seconds = 0.0
        while process.poll() is None:
            if time.perf_counter() - started >= RUN_TIMEOUT_SECONDS:
                process.kill()
                process.wait()
                raise TimeoutError(
                    f"benchmark command exceeded {RUN_TIMEOUT_SECONDS}s"
                )
            try:
                peak_rss = max(peak_rss, observed.memory_info().rss)
                cpu = observed.cpu_times()
                cpu_seconds = cpu.user + cpu.system
            except psutil.Error:
                pass
            time.sleep(0.001)
        process.wait()
        stdout_file.seek(0)
        stderr_file.seek(0)
        stdout = stdout_file.read()
        stderr = stderr_file.read()
        try:
            peak_rss = max(peak_rss, observed.memory_info().rss)
        except psutil.Error:
            pass
    return {
        "returncode": process.returncode,
        "wall_ms": (time.perf_counter() - started) * 1000,
        "cpu_ms": cpu_seconds * 1000,
        "peak_rss_bytes": peak_rss,
        "stdout": stdout,
        "stderr": stderr,
    }


def process_environment(
    *,
    trace_path: Path | None = None,
    allocation_path: Path | None = None,
) -> dict[str, str]:
    """Build a child environment with explicit observer/allocation routing."""

    environment = os.environ.copy()
    environment.pop("PROJECTATLAS_REVERSE_CALLER_TRACE", None)
    environment.pop("PROJECTATLAS_REVERSE_CALLER_ALLOCATIONS", None)
    if trace_path is not None:
        environment["PROJECTATLAS_REVERSE_CALLER_TRACE"] = str(trace_path)
    if allocation_path is not None:
        environment["PROJECTATLAS_REVERSE_CALLER_ALLOCATIONS"] = str(allocation_path)
    return environment


def require_success(measured: dict[str, Any], label: str) -> None:
    """Stop the matrix when an installation or scan setup failed."""

    if measured["returncode"] != 0:
        stderr = measured["stderr"].decode("utf-8", errors="replace")
        raise RuntimeError(f"{label} failed with {measured['returncode']}: {stderr}")


def sanitize_text(value: bytes, fixture_root: Path) -> str:
    """Redact temporary fixture paths without changing comparison bytes."""

    text = value.decode("utf-8", errors="replace")
    return text.replace(str(fixture_root), "<fixture>").replace(
        fixture_root.as_posix(), "<fixture>"
    )


def serialize_process(measured: dict[str, Any], fixture_root: Path) -> dict[str, Any]:
    """Retain exact raw streams plus bounded process metrics."""

    stdout = measured["stdout"]
    stderr = measured["stderr"]
    return {
        "returncode": measured["returncode"],
        "wall_ms": measured["wall_ms"],
        "cpu_ms": measured["cpu_ms"],
        "peak_rss_bytes": measured["peak_rss_bytes"],
        "stdout_bytes": len(stdout),
        "stdout_sha256": sha256_bytes(stdout),
        "stdout_base64": base64.b64encode(stdout).decode("ascii"),
        "stdout_text": sanitize_text(stdout, fixture_root),
        "stderr_bytes": len(stderr),
        "stderr_sha256": sha256_bytes(stderr),
        "stderr_base64": base64.b64encode(stderr).decode("ascii"),
        "stderr_text": sanitize_text(stderr, fixture_root),
    }


def wait_for_cancellation_stage(
    trace_path: Path,
    process: subprocess.Popen[bytes],
    stage: str,
) -> dict[str, Any]:
    """Wait for one bridge-owned cancellation lifecycle stage."""

    deadline = time.monotonic() + RUN_TIMEOUT_SECONDS
    while True:
        if process.poll() is not None and not trace_path.is_file():
            raise RuntimeError(
                f"MCP server exited before the cancellation {stage} observation"
            )
        if time.monotonic() >= deadline:
            raise TimeoutError(f"MCP cancellation {stage} observation timed out")
        if not trace_path.is_file():
            time.sleep(0.005)
            continue
        try:
            observation = json.loads(trace_path.read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            time.sleep(0.005)
            continue
        if observation.get("stage") == stage:
            break
        if stage == "terminal" and observation.get("stage") == "started":
            time.sleep(0.005)
            continue
        raise AssertionError(
            f"MCP cancellation reached {observation.get('stage')!r}, expected {stage!r}"
        )
    return observation


def require_definitive_cancellation(observation: dict[str, Any]) -> None:
    """Require the operation result, not a bridge-drop diagnostic marker."""

    if (
        observation.get("request_cancellation_observed") is not True
        or observation.get("work_cancellation_observed") is not True
        or observation.get("outcome") != "canceled"
        or observation.get("result_was_canceled") is not True
    ):
        raise AssertionError(
            "MCP cancellation did not reach a definitive canceled terminal state: "
            f"{observation!r}"
        )


def require_successful_ping(response: dict[str, Any]) -> None:
    """Require the benchmark server's JSON-RPC ping success response."""

    if (
        response.get("jsonrpc") != "2.0"
        or response.get("id") != 3
        or "error" in response
        or response.get("result") != {}
    ):
        raise AssertionError(f"MCP ping response was not successful: {response!r}")


def require_clean_process_exit(returncode: int | None) -> None:
    """Require the benchmark MCP process to exit successfully."""

    if returncode != 0:
        raise AssertionError(f"MCP cancellation process exited with {returncode!r}")


def run_mcp_cancellation(
    binary: Path,
    root: Path,
    target: str,
) -> dict[str, Any]:
    """Cancel one real MCP summary request through the supported request boundary."""

    cancellation_trace = root.parent / f"{root.name}-cancellation-trace.json"
    environment = os.environ.copy()
    environment["PROJECTATLAS_REVERSE_CALLER_CANCELLATION_TRACE"] = str(
        cancellation_trace
    )
    process = subprocess.Popen(
        [str(binary), "mcp"],
        cwd=root,
        env=environment,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    stdout = bytearray()
    responses: dict[int, queue.Queue[dict[str, Any] | BaseException]] = {}
    response_lock = threading.Lock()

    def read_responses() -> None:
        assert process.stdout is not None
        try:
            for line in iter(process.stdout.readline, b""):
                stdout.extend(line)
                try:
                    response = json.loads(line)
                except json.JSONDecodeError:
                    continue
                request_id = response.get("id")
                if isinstance(request_id, int):
                    with response_lock:
                        response_queue = responses.get(request_id)
                    if response_queue is not None:
                        response_queue.put(response)
        except BaseException as error:
            with response_lock:
                response_queues = list(responses.values())
            for response_queue in response_queues:
                response_queue.put(error)

    reader = threading.Thread(
        target=read_responses,
        name="projectatlas-reverse-caller-mcp-reader",
        daemon=True,
    )
    reader.start()

    def request(request_id: int, method: str, params: dict[str, Any]) -> dict[str, Any]:
        response_queue: queue.Queue[dict[str, Any] | BaseException] = queue.Queue(maxsize=1)
        with response_lock:
            responses[request_id] = response_queue
        assert process.stdin is not None
        process.stdin.write(
            (json.dumps(
                {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params},
                separators=(",", ":"),
            ) + "\n").encode("utf-8")
        )
        process.stdin.flush()
        response: dict[str, Any] | BaseException | None = None
        try:
            response = response_queue.get(timeout=RUN_TIMEOUT_SECONDS)
        finally:
            with response_lock:
                responses.pop(request_id, None)
        if response is None:
            raise RuntimeError(f"MCP response {request_id} was empty")
        if isinstance(response, BaseException):
            raise response
        return response

    def notify(method: str, params: dict[str, Any]) -> None:
        assert process.stdin is not None
        process.stdin.write(
            (json.dumps(
                {"jsonrpc": "2.0", "method": method, "params": params},
                separators=(",", ":"),
            ) + "\n").encode("utf-8")
        )
        process.stdin.flush()

    stderr = b""
    cleanup_error: BaseException | None = None
    try:
        initialize = request(
            1,
            "initialize",
            {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "projectatlas-reverse-caller-benchmark", "version": "1"},
            },
        )
        if initialize.get("error") is not None:
            raise RuntimeError(f"MCP initialize failed: {initialize['error']}")
        notify("notifications/initialized", {})
        response_queue: queue.Queue[dict[str, Any] | BaseException] = queue.Queue(maxsize=1)
        with response_lock:
            responses[2] = response_queue
        assert process.stdin is not None
        process.stdin.write(
            (json.dumps(
                {
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/call",
                    "params": {
                        "name": "atlas_file_summary",
                        "arguments": {"file": target, "limit": SUMMARY_LIMIT},
                    },
                },
                separators=(",", ":"),
            ) + "\n").encode("utf-8")
        )
        process.stdin.flush()
        wait_for_cancellation_stage(cancellation_trace, process, "started")
        # MCP cancellation is a notification for the in-flight request, not process teardown.
        notify(
            "notifications/cancelled",
            {"requestId": 2, "reason": "reverse-caller benchmark cancellation"},
        )
        terminal_observation = wait_for_cancellation_stage(
            cancellation_trace, process, "terminal"
        )
        require_definitive_cancellation(terminal_observation)
        # A live ping proves the server stayed up while rmcp suppressed the
        # canceled request's response; a response for id=2 would be a failure.
        require_successful_ping(request(3, "ping", {}))
        with response_lock:
            responses.pop(2, None)
    except BaseException as error:
        cleanup_error = error
    finally:
        if process.stdin is not None and not process.stdin.closed:
            process.stdin.close()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
        reader.join(timeout=5)
        if reader.is_alive() and cleanup_error is None:
            cleanup_error = RuntimeError("MCP response reader did not stop")
        if process.stderr is not None:
            stderr = process.stderr.read()
        if cancellation_trace.is_file():
            cancellation_trace.unlink()
    if cleanup_error is not None:
        raise cleanup_error
    stream_responses = []
    for line in bytes(stdout).splitlines():
        if not line.strip():
            continue
        try:
            stream_responses.append(json.loads(line))
        except json.JSONDecodeError as error:
            raise AssertionError(
                f"MCP cancellation emitted a non-JSON stream line: {error}"
            ) from error
    if any(response.get("id") == 2 for response in stream_responses):
        raise AssertionError("canceled MCP summary emitted a response")
    require_clean_process_exit(process.returncode)
    response_text = json.dumps(
        {"canceled_request_response": None, "ping": "ok"},
        sort_keys=True,
    )
    return {
        "returncode": process.returncode,
        "stdout_bytes": len(stdout),
        "stdout_sha256": sha256_bytes(bytes(stdout)),
        "stdout_base64": base64.b64encode(bytes(stdout)).decode("ascii"),
        "stdout_text": sanitize_text(bytes(stdout), root),
        "stderr_bytes": len(stderr),
        "stderr_sha256": sha256_bytes(stderr),
        "stderr_base64": base64.b64encode(stderr).decode("ascii"),
        "stderr_text": sanitize_text(stderr, root),
        "terminal_observation": terminal_observation,
        "response_sha256": sha256_bytes(response_text.encode("utf-8")),
        "response_text": response_text,
        "response_is_error": False,
        "outcome": "canceled",
        "partial_success": False,
    }


def source_for(language: str, symbol_count: int) -> str:
    """Render one language-native declaration fixture."""

    if language == "rust":
        return "\n".join(
            f"pub fn target_{index}() {{}}" for index in range(symbol_count)
        ) + "\n"
    if language == "typescript":
        return "\n".join(
            f"export function target_{index}() {{}}" for index in range(symbol_count)
        ) + "\n"
    return "\n".join(
        f"def target_{index}():\n    pass" for index in range(symbol_count)
    ) + "\n"


def caller_source(language: str, module: str, caller: int, selected: int) -> str:
    """Render one caller with a language-specific import alias."""

    if language == "rust":
        return (
            f"use crate::{module} as {module}_alias;\n"
            f"fn caller_{caller}() {{ {module}_alias::target_{selected}(); }}\n"
        )
    if language == "typescript":
        return (
            f'import {{ target_{selected} as {language}_alias_{selected} }} from "./{module}";\n'
            f"function caller_{caller}() {{ {language}_alias_{selected}(); }}\n"
        )
    return (
        f"from package.{module} import target_{selected} as {language}_alias_{selected}\n"
        f"def caller_{caller}():\n    {language}_alias_{selected}()\n"
    )


def language_paths(language: str) -> tuple[Path, Path, str]:
    """Return target path, caller suffix, and module name for one language."""

    if language == "rust":
        return Path("src/rust_target.rs"), ".rs", "rust_target"
    if language == "typescript":
        return Path("src/ts_target.ts"), ".ts", "ts_target"
    return Path("src/package/py_target.py"), ".py", "py_target"


def write_fixture(root: Path, shape: str) -> list[dict[str, Any]]:
    """Create real Rust, TypeScript, Python, and alias-collision sources."""

    symbol_count, caller_count = SHAPES[shape]
    targets: list[dict[str, Any]] = []
    for language in LANGUAGES:
        target_path, caller_suffix, module = language_paths(language)
        target_file = root / target_path
        target_file.parent.mkdir(parents=True, exist_ok=True)
        target_file.write_text(source_for(language, symbol_count), encoding="utf-8")
        target = {
            "language": language,
            "path": target_path.as_posix(),
            "caller_suffix": caller_suffix,
            "module": module,
            "symbol_count": symbol_count,
            "caller_count": caller_count,
        }
        targets.append(target)
        for caller in range(caller_count):
            selected = caller % symbol_count
            caller_path = root / "src" / (
                f"{language}_caller_{caller:03d}{caller_suffix}"
            )
            caller_path.parent.mkdir(parents=True, exist_ok=True)
            caller_path.write_text(
                caller_source(language, module, caller, selected),
                encoding="utf-8",
            )
        if shape == "duplicate-alias":
            other_module = f"{language}_other"
            if language == "python":
                other_path = root / "src" / "package" / "py_other.py"
            else:
                other_path = root / "src" / f"{other_module}{caller_suffix}"
            other_path.parent.mkdir(parents=True, exist_ok=True)
            other_path.write_text(source_for(language, 1), encoding="utf-8")
            caller_path = root / "src" / f"{language}_caller_000{caller_suffix}"
            if language == "rust":
                duplicate = (
                    "use crate::rust_target as target_alias;\n"
                    "use crate::rust_other as target_alias;\n"
                    "fn caller_0() { target_alias::target_0(); }\n"
                )
            elif language == "typescript":
                duplicate = (
                    'import { target_0 as target_alias_0 } from "./ts_target";\n'
                    'import { target_0 as target_alias_0 } from "./typescript_other";\n'
                    "function caller_0() { target_alias_0(); }\n"
                )
            else:
                duplicate = (
                    "from package.py_target import target_0 as target_alias_0\n"
                    "from package.py_other import target_0 as target_alias_0\n"
                    "def caller_0():\n"
                    "    target_alias_0()\n"
                )
            caller_path.write_text(duplicate, encoding="utf-8")
    return targets


def write_cancellation_fixture(root: Path) -> dict[str, Any]:
    """Create one bounded high-symbol fixture that leaves time for MCP cancellation."""

    target = next(
        item for item in write_fixture(root, "high-symbol") if item["language"] == "rust"
    )
    target["symbol_count"] = 10_000
    (root / target["path"]).write_text(
        source_for("rust", target["symbol_count"]), encoding="utf-8"
    )
    return target


def expected_called_by(target: dict[str, Any], shape: str, symbol: int) -> list[str]:
    """Return the exact semantic caller list for one generated target symbol."""

    if shape == "duplicate-alias":
        return []
    if symbol >= target["symbol_count"] or target["caller_count"] == 0:
        return []
    callers = [
        f"src/{target['language']}_caller_{caller:03d}{target['caller_suffix']}::caller_{caller}"
        for caller in range(target["caller_count"])
        if caller % target["symbol_count"] == symbol
    ]
    return callers[:CALLERS_PER_SYMBOL_LIMIT]


def assert_semantics(payload: dict[str, Any], target: dict[str, Any], shape: str) -> None:
    """Assert the generated source reaches the public summary semantics."""

    if payload.get("file_path") != target["path"]:
        raise AssertionError(f"summary selected the wrong file: {payload!r}")
    symbols = {
        item["name"]: item
        for section in ("functions", "methods", "classes", "types")
        for item in payload.get(section, [])
    }
    effective_limit = payload.get("limit", SUMMARY_LIMIT)
    if target["symbol_count"] > effective_limit and not payload.get("truncated"):
        raise AssertionError("summary did not report symbol truncation")
    for section in ("functions", "methods", "classes", "types"):
        if len(payload.get(section, [])) > effective_limit:
            raise AssertionError(f"{section} exceeded summary limit")
    for symbol in range(min(target["symbol_count"], effective_limit)):
        name = f"target_{symbol}"
        if name not in symbols:
            raise AssertionError(f"summary omitted {name}: {payload!r}")
        expected = expected_called_by(target, shape, symbol)
        if symbols[name].get("called_by") != expected:
            raise AssertionError(
                f"unexpected callers for {target['language']}:{name}: "
                f"{symbols[name].get('called_by')!r} != {expected!r}"
            )
    if shape == "duplicate-alias" and any(
        symbols[name].get("called_by")
        for name in symbols
        if name == "target_0"
    ):
        raise AssertionError("ambiguous alias produced a caller")


def summary_command(target: dict[str, Any], limit: int) -> list[str]:
    """Build the public summary command used by timed and replay samples."""

    return [
        "--format",
        "json",
        "summary",
        target["path"],
        "--limit",
        str(limit),
    ]


def run_summary(
    binary: Path,
    root: Path,
    target: dict[str, Any],
    shape: str,
    limit: int,
    arm: str,
    ordinal: int,
) -> dict[str, Any]:
    """Run one observer-free timed public summary invocation."""

    allocation_path = root.parent / f"{root.name}-allocations-{arm}-{ordinal:04d}.json"
    trace_path = root.parent / f"{root.name}-trace-{arm}-{ordinal:04d}.json"
    for path in (trace_path, allocation_path):
        if path.exists():
            raise AssertionError(f"timed summary observer path already exists: {path}")
    measured = run_process(
        [str(binary), *summary_command(target, limit)],
        root,
    )
    result = serialize_process(measured, root)
    result["target"] = target["path"]
    result["limit"] = limit
    for path in (trace_path, allocation_path):
        if path.is_file():
            path.unlink()
            raise AssertionError("timed summary unexpectedly enabled instrumentation")
    result["allocation_metrics"] = None
    result["query_observations"] = []
    result["trace_observed"] = False
    result["timed"] = True
    if measured["returncode"] == 0:
        try:
            payload = json.loads(measured["stdout"])
        except json.JSONDecodeError as error:
            raise AssertionError(f"summary emitted invalid JSON: {error}") from error
        assert_semantics(payload, target, shape)
        result["decoded_summary"] = payload
    return result


def run_summary_evidence(
    binary: Path,
    root: Path,
    target: dict[str, Any],
    shape: str,
    limit: int,
    arm: str,
    ordinal: int,
    expected_summary: dict[str, Any],
) -> dict[str, Any]:
    """Replay one summary outside timing to capture production-owned evidence."""

    allocation_path = root.parent / f"{root.name}-allocations-{arm}-{ordinal:04d}.json"
    allocation_run = run_process(
        [str(binary), *summary_command(target, limit)],
        root,
        allocation_path=allocation_path,
    )
    if allocation_run["returncode"] != 0:
        raise AssertionError(
            f"{arm} {shape} allocation replay failed: "
            f"{allocation_run['stderr'].decode('utf-8', errors='replace')}"
        )
    if not allocation_path.is_file():
        raise AssertionError(f"{arm} {shape} allocation replay omitted its sidecar")
    allocation_metrics = json.loads(allocation_path.read_text(encoding="utf-8"))
    allocation_path.unlink()
    allocation_payload = json.loads(allocation_run["stdout"])
    assert_semantics(allocation_payload, target, shape)
    if allocation_payload != expected_summary:
        raise AssertionError(f"{arm} {shape} allocation replay changed the summary")

    trace_path = root.parent / f"{root.name}-trace-{arm}-{ordinal:04d}.json"
    measured = run_process(
        [str(binary), *summary_command(target, limit)],
        root,
        trace_path=trace_path,
    )
    if measured["returncode"] != 0:
        raise AssertionError(
            f"{arm} {shape} evidence replay failed: "
            f"{measured['stderr'].decode('utf-8', errors='replace')}"
        )
    if not trace_path.is_file():
        raise AssertionError(f"{arm} {shape} evidence replay omitted its trace")
    trace = json.loads(trace_path.read_text(encoding="utf-8"))
    trace_path.unlink()
    require_production_trace_evidence(trace, f"{arm} {shape}")
    if not isinstance(allocation_metrics, dict):
        raise AssertionError(f"{arm} {shape} evidence replay omitted allocations")
    payload = json.loads(measured["stdout"])
    assert_semantics(payload, target, shape)
    if payload != expected_summary:
        raise AssertionError(f"{arm} {shape} evidence replay changed the summary")
    return {
        "events": trace.get("queries", []),
        "engine": trace.get("engine"),
        "plan_provenance": trace.get("plan_provenance"),
        "allocation_metrics": allocation_metrics,
        "target": target["path"],
        "limit": limit,
        "trace_observed": True,
        "timed": False,
    }


def require_production_trace_evidence(trace: dict[str, Any], label: str) -> None:
    """Require plan evidence to identify the production engine and owner."""

    if trace.get("plan_provenance") != "projectatlas-db::AtlasStore::connection":
        raise AssertionError(f"{label} trace has no production plan provenance")
    engine = trace.get("engine")
    if not isinstance(engine, dict) or not isinstance(
        engine.get("sqlite_version"), str
    ):
        raise AssertionError(f"{label} trace has no SQLite engine identity")
    for event in trace.get("queries", []):
        query_plan = event.get("query_plan")
        if not isinstance(query_plan, list):
            raise AssertionError(f"{label} trace omitted a production query plan")
        outcome = event.get("outcome")
        if not isinstance(outcome, dict) or outcome.get("status") not in {
            "succeeded",
            "failed",
        }:
            raise AssertionError(f"{label} trace omitted a typed query outcome")
        plan_error = event.get("query_plan_error")
        if outcome["status"] == "succeeded":
            if not query_plan or plan_error is not None:
                raise AssertionError(f"{label} successful query omitted its production plan")
        else:
            if not isinstance(outcome.get("error"), str):
                raise AssertionError(f"{label} failed query omitted its database error")
            if not query_plan and not isinstance(plan_error, str):
                raise AssertionError(
                    f"{label} failed query omitted both its plan and replay error"
                )
            if query_plan and plan_error is not None:
                raise AssertionError(
                    f"{label} failed query retained both a plan and replay error"
                )


def multi_binding_alias_case(binary: Path, arm: str) -> dict[str, Any]:
    """Prove one Python import relation deduplicates repeated local aliases."""

    with tempfile.TemporaryDirectory(
        prefix=f"projectatlas-multi-binding-alias-{arm}-"
    ) as directory:
        root = Path(directory)
        target_path = Path("src/package/py_target.py")
        caller_path = root / "src/py_caller.py"
        (root / "src/package").mkdir(parents=True, exist_ok=True)
        (root / target_path).write_text(
            "def a():\n    pass\n\ndef b():\n    pass\n",
            encoding="utf-8",
        )
        caller_path.write_text(
            "from package.py_target import a as shared, b as shared\n"
            "def caller():\n"
            "    shared()\n",
            encoding="utf-8",
        )
        setup_fixture(binary, root)
        trace_path = root.parent / f"{root.name}-trace-multi-binding-{arm}.json"
        measured = run_process(
            [
                str(binary),
                "--format",
                "json",
                "summary",
                target_path.as_posix(),
                "--limit",
                "1",
            ],
            root,
            trace_path=trace_path,
        )
        result = serialize_process(measured, root)
        result["case"] = "python-multi-binding-same-local-alias"
        result["target"] = target_path.as_posix()
        result["limit"] = 1
        trace = {"queries": [], "allocations": None}
        if trace_path.is_file():
            trace = json.loads(trace_path.read_text(encoding="utf-8"))
            trace_path.unlink()
            require_production_trace_evidence(
                trace, f"{arm} Python multi-binding alias"
            )
        result["allocation_metrics"] = trace.get("allocations")
        result["query_observations"] = trace.get("queries", [])
        result["query_engine"] = trace.get("engine")
        result["plan_provenance"] = trace.get("plan_provenance")
        result["trace_observed"] = bool(result["query_observations"])
        result["timed"] = False
        if measured["returncode"] != 0:
            raise AssertionError(
                "Python multi-binding alias summary failed: "
                f"{measured['stderr'].decode('utf-8', errors='replace')}"
            )
        payload = json.loads(measured["stdout"])
        functions = {
            item["name"]: item for item in payload.get("functions", [])
        }
        if payload.get("truncated") is not True:
            raise AssertionError(
                "Python multi-binding alias case did not exercise summary truncation"
            )
        expected = ["src/py_caller.py::caller"]
        if functions.get("a", {}).get("called_by") != expected:
            raise AssertionError(
                "same-relation Python alias was treated as ambiguous: "
                f"{functions.get('a', {}).get('called_by')!r} != {expected!r}"
            )
        result["decoded_summary"] = payload
        return result


def setup_fixture(binary: Path, root: Path) -> dict[str, Any]:
    """Initialize and scan one isolated fixture with the real CLI."""

    setup: dict[str, Any] = {}
    for label, command in (
        ("init", ["init", "--no-scan"]),
        ("scan", ["scan"]),
    ):
        measured = run_process([str(binary), *command], root)
        require_success(measured, label)
        setup[label] = serialize_process(measured, root)
    return setup


def aggregate_queries(evidence_runs: list[dict[str, Any]]) -> dict[str, Any]:
    """Summarize untimed production query events without synthetic SQL."""

    observations = [
        observation
        for run in evidence_runs
        for observation in run["events"]
    ]
    by_family: dict[str, dict[str, Any]] = {}
    for observation in observations:
        family = observation["family"]
        bucket = by_family.setdefault(
            family,
            {"statements": 0, "rows": 0, "row_bytes": 0, "limits": [], "plans": []},
        )
        bucket["statements"] += 1
        bucket["rows"] += observation["rows"]
        bucket["row_bytes"] += observation["row_bytes"]
        bucket["limits"].append(observation["limit"])
        bucket["plans"].append(observation["query_plan"])
    for bucket in by_family.values():
        bucket["limits"] = sorted(set(bucket["limits"]))
        bucket["plans"] = sorted({json.dumps(plan) for plan in bucket["plans"]})
        bucket["plans"] = [json.loads(plan) for plan in bucket["plans"]]
    return {
        "events": observations,
        "by_family": by_family,
        "engine": evidence_runs[0].get("engine") if evidence_runs else None,
        "plan_provenance": (
            evidence_runs[0].get("plan_provenance") if evidence_runs else None
        ),
        "timed": False,
    }


def measure_shape(
    binary: Path,
    evidence_binary: Path,
    shape: str,
    repeats: int,
    arm: str,
) -> dict[str, Any]:
    """Measure every language target and one bounded truncation replay."""

    with tempfile.TemporaryDirectory(prefix=f"projectatlas-{shape}-") as directory:
        root = Path(directory)
        (root / "src").mkdir(parents=True, exist_ok=True)
        targets = write_fixture(root, shape)
        setup = setup_fixture(binary, root)
        runs: list[dict[str, Any]] = []
        evidence_runs: list[dict[str, Any]] = []
        ordinal = 0
        evidence_ordinal = 0
        for target in targets:
            limits = [1, SUMMARY_LIMIT] if target is targets[0] else [SUMMARY_LIMIT]
            for limit in limits:
                for repeat in range(repeats):
                    runs.append(
                        run_summary(
                            binary,
                            root,
                            target,
                            shape,
                            limit,
                            arm,
                            ordinal,
                        )
                    )
                    ordinal += 1
                evidence_runs.append(
                    run_summary_evidence(
                        evidence_binary,
                        root,
                        target,
                        shape,
                        limit,
                        arm,
                        evidence_ordinal,
                        runs[-1]["decoded_summary"],
                    )
                )
                evidence_ordinal += 1
        metrics = {
            key: statistics.median(run[key] for run in runs)
            for key in ("wall_ms", "cpu_ms", "peak_rss_bytes", "stdout_bytes")
        }
        allocation_runs = [
            run["allocation_metrics"]
            for run in evidence_runs
            if isinstance(run.get("allocation_metrics"), dict)
        ]
        if len(allocation_runs) != len(evidence_runs):
            raise AssertionError(
                f"{arm} {shape} did not retain allocation metrics for every evidence replay"
            )
        metrics["allocation_calls"] = statistics.median(
            run["allocation_calls"] for run in allocation_runs
        )
        metrics["allocation_bytes"] = statistics.median(
            run["allocation_bytes"] for run in allocation_runs
        )
        return {
            "targets": targets,
            "setup": setup,
            "runs": runs,
            "evidence_replays": evidence_runs,
            "aggregate": metrics,
            "allocation_measurement": "untimed-evidence-replay",
            "query_observations": aggregate_queries(evidence_runs),
        }


def mutate_corrupt_row(database: Path) -> None:
    """Introduce one admitted malformed relation for the fail-closed read case."""

    connection = sqlite3.connect(database)
    try:
        connection.execute(
            "UPDATE symbol_relations SET line = 'invalid' WHERE rowid = "
            "(SELECT rowid FROM symbol_relations "
            "WHERE kind = 'calls' AND path = 'src/rust_caller_000.rs' LIMIT 1)"
        )
        connection.commit()
    finally:
        connection.close()


def mutate_corrupt_import_row(database: Path, family: str) -> None:
    """Corrupt the import row selected by one production query family."""

    connection = sqlite3.connect(database)
    try:
        if family == "import-targets":
            assignment = "line = 'invalid'"
        elif family == "import-caller-path":
            assignment = "line = 'invalid', target_name = 'use crate::unrelated'"
        else:
            raise ValueError(family)
        cursor = connection.execute(
            f"UPDATE symbol_relations SET {assignment} WHERE rowid = "
            "(SELECT rowid FROM symbol_relations "
            "WHERE kind = 'imports' AND path = 'src/rust_caller_000.rs' LIMIT 1)"
        )
        if cursor.rowcount != 1:
            raise AssertionError(f"{family} corruption selected no import row")
        connection.commit()
    finally:
        connection.close()


def cancellation_case(binary: Path, arm: str) -> dict[str, Any]:
    """Compare one supported MCP summary cancellation against the other arm."""

    with tempfile.TemporaryDirectory(prefix=f"projectatlas-cancellation-{arm}-") as directory:
        root = Path(directory)
        (root / "src").mkdir(parents=True, exist_ok=True)
        target = write_cancellation_fixture(root)
        setup_fixture(binary, root)
        result = run_mcp_cancellation(binary, root, target["path"])
        result["case"] = "mcp-summary-cancellation"
        result["target"] = target["path"]
        return result


def failure_case(
    binary: Path, evidence_binary: Path, name: str, arm: str
) -> dict[str, Any]:
    """Capture ordinary behavior and an untimed query-evidence replay."""

    with tempfile.TemporaryDirectory(prefix=f"projectatlas-failure-{name}-") as directory:
        root = Path(directory)
        (root / "src").mkdir(parents=True, exist_ok=True)
        targets = write_fixture(root, "small")
        setup_fixture(binary, root)
        if name == "missing-summary":
            target_path = "src/missing.rs"
        elif name == "corrupt-relation":
            mutate_corrupt_row(root / ".projectatlas" / "projectatlas.db")
            target_path = targets[0]["path"]
        elif name in {"corrupt-import-target", "corrupt-import-caller-path"}:
            family = {
                "corrupt-import-target": "import-targets",
                "corrupt-import-caller-path": "import-caller-path",
            }[name]
            mutate_corrupt_import_row(root / ".projectatlas" / "projectatlas.db", family)
            target_path = targets[0]["path"]
        elif name == "stale-source":
            target_path = targets[0]["path"]
            (root / target_path).write_text(
                "pub fn target_0() {}\npub fn stale_added() {}\n",
                encoding="utf-8",
            )
        else:
            raise ValueError(name)
        command = summary_command({"path": target_path}, SUMMARY_LIMIT)
        measured = run_process([str(binary), *command], root)
        trace_path = root.parent / f"{root.name}-trace-failure-{arm}.json"
        evidence_measured = run_process(
            [str(evidence_binary), *command],
            root,
            trace_path=trace_path,
        )
        result = serialize_process(measured, root)
        evidence_result = serialize_process(evidence_measured, root)
        for field in ("returncode", "stdout_base64", "stderr_base64"):
            if result[field] != evidence_result[field]:
                raise AssertionError(f"{arm} {name} evidence replay {field} drift")
        result["case"] = name
        result["target"] = target_path
        if not trace_path.is_file():
            raise AssertionError(f"{arm} {name} evidence replay emitted no trace")
        trace = json.loads(trace_path.read_text(encoding="utf-8"))
        trace_path.unlink()
        require_production_trace_evidence(trace, f"{arm} {name}")
        result["allocation_metrics"] = trace.get("allocations")
        result["query_observations"] = trace.get("queries", [])
        result["query_engine"] = trace.get("engine")
        result["plan_provenance"] = trace.get("plan_provenance")
        result["trace_observed"] = True
        result["timed"] = False
        failed_family = {
            "corrupt-relation": "call-targets",
            "corrupt-import-target": "import-targets",
            "corrupt-import-caller-path": "import-caller-path",
        }.get(name)
        if failed_family is not None:
            failed_queries = [
                event
                for event in result["query_observations"]
                if event.get("family") == failed_family
            ]
            if len(failed_queries) != 1 or failed_queries[0].get("outcome", {}).get(
                "status"
            ) != "failed":
                raise AssertionError(
                    f"{arm} {name} trace omitted the failed {failed_family} query"
                )
            if measured["returncode"] == 0:
                raise AssertionError("malformed relation unexpectedly succeeded")
            if measured["stdout"]:
                raise AssertionError("malformed relation emitted partial JSON")
        if measured["returncode"] == 0:
            result["decoded_summary"] = json.loads(measured["stdout"])
        return result


def compare_raw_runs(
    baseline: dict[str, Any],
    candidate: dict[str, Any],
) -> list[str]:
    """Compare status, raw streams, semantics, and real query observations."""

    findings: list[str] = []
    if baseline["returncode"] != candidate["returncode"]:
        findings.append("summary returncode drift")
    for stream in ("stdout", "stderr"):
        if baseline[f"{stream}_base64"] != candidate[f"{stream}_base64"]:
            findings.append(f"{stream} bytes drift")
    if baseline.get("decoded_summary") != candidate.get("decoded_summary"):
        findings.append("decoded summary drift")
    if baseline.get("query_observations") != candidate.get("query_observations"):
        findings.append("production query observation drift")
    for field in ("query_engine", "plan_provenance"):
        if baseline.get(field) != candidate.get(field):
            findings.append(f"{field} drift")
    return findings


def compare_cancellation_runs(
    baseline: dict[str, Any],
    candidate: dict[str, Any],
) -> list[str]:
    """Compare cancellation outcome and complete MCP process streams."""

    findings: list[str] = []
    for field in (
        "returncode",
        "outcome",
        "response_is_error",
        "partial_success",
        "stdout_base64",
        "stderr_base64",
        "response_text",
        "terminal_observation",
    ):
        if baseline.get(field) != candidate.get(field):
            findings.append(f"cancellation {field} drift")
    for label, run in (("baseline", baseline), ("candidate", candidate)):
        if run.get("outcome") != "canceled":
            findings.append(f"{label} cancellation was not observed")
        if run.get("partial_success"):
            findings.append(f"{label} cancellation emitted a successful summary")
    return findings


def decision(
    baseline: dict[str, Any],
    candidate: dict[str, Any],
    semantic_findings: list[str],
) -> dict[str, Any]:
    """Apply the unchanged frozen rule to shape medians."""

    changes: dict[str, float] = {}
    for shape in SHAPES:
        base = baseline[shape]["aggregate"]["wall_ms"]
        trial = candidate[shape]["aggregate"]["wall_ms"]
        changes[shape] = (trial - base) / base
    performance_pass = (
        changes["high-import"] <= -FROZEN_HIGH_IMPORT_IMPROVEMENT
        and changes["representative-large"] <= -FROZEN_REPRESENTATIVE_IMPROVEMENT
        and changes["small"] <= FROZEN_SMALL_REGRESSION
        and changes["high-symbol"] <= FROZEN_HIGH_SYMBOL_REGRESSION
        and changes["duplicate-alias"] <= FROZEN_DUPLICATE_REGRESSION
    )
    accepted = performance_pass and not semantic_findings
    return {
        "candidate_accepted": accepted,
        "outcome": "adopt-candidate" if accepted else "retain-current-no-product-change",
        "wall_change_fraction": changes,
        "performance_pass": performance_pass,
        "semantic_findings": semantic_findings,
        "thresholds": {
            "high_import_improvement_fraction": FROZEN_HIGH_IMPORT_IMPROVEMENT,
            "representative_large_improvement_fraction": FROZEN_REPRESENTATIVE_IMPROVEMENT,
            "small_max_regression_fraction": FROZEN_SMALL_REGRESSION,
            "high_symbol_max_regression_fraction": FROZEN_HIGH_SYMBOL_REGRESSION,
            "duplicate_alias_max_regression_fraction": FROZEN_DUPLICATE_REGRESSION,
        },
    }


def compact_process_evidence(record: dict[str, Any]) -> dict[str, Any]:
    """Keep bounded process measurements without retaining raw streams."""

    fields = (
        "returncode",
        "wall_ms",
        "cpu_ms",
        "peak_rss_bytes",
        "stdout_bytes",
        "stdout_sha256",
        "stderr_bytes",
        "stderr_sha256",
        "allocation_metrics",
        "case",
        "target",
        "limit",
        "trace_observed",
        "timed",
        "query_engine",
        "plan_provenance",
    )
    return {field: record[field] for field in fields if field in record}


def compact_summary_evidence(summary: object) -> dict[str, Any] | None:
    """Keep only review-relevant fields from one decoded summary."""

    if not isinstance(summary, dict):
        return None
    result: dict[str, Any] = {
        field: summary[field]
        for field in (
            "file_path",
            "language",
            "source_status",
            "summary_status",
            "parser_kind",
            "symbol_count",
            "limit",
            "line_count",
            "truncated",
        )
        if field in summary
    }
    result["functions"] = [
        {
            field: function[field]
            for field in ("name", "kind", "line", "called_by")
            if field in function
        }
        for function in summary.get("functions", [])
        if isinstance(function, dict)
    ]
    return result


def compact_query_plan_evidence(observations: dict[str, Any]) -> dict[str, Any]:
    """Retain query-plan aggregates and one exact event per query family."""

    representatives: list[dict[str, Any]] = []
    seen_families: set[str] = set()
    for event in observations.get("events", []):
        family = event.get("family")
        if not isinstance(family, str) or family in seen_families:
            continue
        seen_families.add(family)
        representatives.append(
            {
                field: event[field]
                for field in (
                    "sequence",
                    "family",
                    "binding",
                    "limit",
                    "rows",
                    "row_bytes",
                    "outcome",
                    "sql",
                    "parameters",
                    "query_plan",
                    "query_plan_error",
                )
                if field in event
            }
        )
    return {
        "by_family": observations.get("by_family", {}),
        "representative_events": representatives,
        "engine": observations.get("engine"),
        "plan_provenance": observations.get("plan_provenance"),
        "timed": observations.get("timed") is True,
    }


def compact_fixture_evidence(fixture: dict[str, Any]) -> dict[str, Any]:
    """Project one fixture to bounded process and query-plan evidence."""

    return {
        "targets": fixture.get("targets", []),
        "aggregate": fixture.get("aggregate", {}),
        "allocation_measurement": fixture.get("allocation_measurement"),
        "runs": [compact_process_evidence(run) for run in fixture.get("runs", [])],
        "evidence_replays": [
            compact_process_evidence(run)
            for run in fixture.get("evidence_replays", [])
        ],
        "query_plan_evidence": compact_query_plan_evidence(
            fixture.get("query_observations", {})
        ),
    }


def compact_arm_evidence(arm: dict[str, Any]) -> dict[str, Any]:
    """Project one benchmark arm without raw setup or process streams."""

    result: dict[str, Any] = {
        "timed_binary_sha256": arm["timed_binary_sha256"],
        "evidence_binary_sha256": arm["evidence_binary_sha256"],
    }
    if "candidate_patch" in arm:
        result["candidate_patch"] = arm["candidate_patch"]
    result["fixtures"] = {
        name: compact_fixture_evidence(fixture)
        for name, fixture in arm.get("fixtures", {}).items()
    }
    return result


def compact_failure_evidence(record: dict[str, Any]) -> dict[str, Any]:
    """Keep bounded failure diagnostics and omit complete process streams."""

    result = compact_process_evidence(record)
    diagnostic = record.get("stderr_text")
    if isinstance(diagnostic, str) and diagnostic:
        result["diagnostic"] = diagnostic
    summary = compact_summary_evidence(record.get("decoded_summary"))
    if summary is not None:
        result["decoded_summary"] = summary
    result["query_plan_evidence"] = compact_query_plan_evidence(
        {
            "events": record.get("query_observations", []),
            "engine": record.get("query_engine"),
            "plan_provenance": record.get("plan_provenance"),
            "timed": record.get("timed"),
        }
    )
    return result


def compact_cancellation_evidence(record: dict[str, Any]) -> dict[str, Any]:
    """Keep the authoritative cancellation and post-cancel process proof."""

    result = compact_process_evidence(record)
    for field in (
        "terminal_observation",
        "response_sha256",
        "response_is_error",
        "outcome",
        "partial_success",
    ):
        if field in record:
            result[field] = record[field]
    return result


def compact_review_evidence(
    result: dict[str, Any], raw_output: Path
) -> dict[str, Any]:
    """Build the bounded tracked projection while retaining raw output elsewhere."""

    raw_output = raw_output.resolve()
    if not raw_output.is_file():
        raise ValueError(f"missing raw benchmark output: {raw_output}")
    compact: dict[str, Any] = {
        "schema": result["schema"],
        "artifact_kind": "compact-review-evidence",
        "issue": result["issue"],
        "repository_revision": result["repository_revision"],
        "repeats": result["repeats"],
        "build_profile": result["build_profile"],
        "measurement_split": result["measurement_split"],
        "harness_identity": {
            "path": "docs/benchmarks/harness/reverse_caller.py",
            "sha256": sha256_file(ROOT / "docs/benchmarks/harness/reverse_caller.py"),
        },
        "baseline": compact_arm_evidence(result["baseline"]),
        "candidate": compact_arm_evidence(result["candidate"]),
        "candidate_patch_preflight": result["candidate_patch_preflight"],
        "failure_cases": {
            name: {
                "baseline": compact_failure_evidence(case["baseline"]),
                "candidate": compact_failure_evidence(case["candidate"]),
            }
            for name, case in result.get("failure_cases", {}).items()
        },
        "multi_binding_alias": {
            arm: compact_failure_evidence(result["multi_binding_alias"][arm])
            for arm in ("baseline", "candidate")
        },
        "cancellation": {
            arm: compact_cancellation_evidence(result["cancellation"][arm])
            for arm in ("baseline", "candidate")
        },
        "decision": result["decision"],
        "reproduction": {
            "raw_trace_path": raw_output.relative_to(ROOT).as_posix(),
            "raw_trace_schema": result["schema"],
            "raw_trace_bytes": raw_output.stat().st_size,
            "raw_trace_sha256": sha256_file(raw_output),
            "raw_trace_ignored": True,
            "note": (
                "Full raw streams remain in ignored local output and are recoverable "
                "by rerunning the existing harness; this tracked file contains only "
                "bounded review evidence."
            ),
        },
    }
    return compact


def git_revision() -> str:
    """Read the exact source revision used by the matrix."""

    completed = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return completed.stdout.strip()


def validate_input(path: Path, label: str) -> Path:
    """Resolve one bounded local benchmark input."""

    resolved = path.resolve()
    if not resolved.is_file():
        raise ValueError(f"missing {label}: {path}")
    return resolved


def validate_release_binary(path: Path, label: str) -> Path:
    """Require the real release-profile ProjectAtlas executable."""

    resolved = validate_input(path, label)
    if (
        resolved.parent.name.casefold() != RELEASE_PROFILE_DIRECTORY
        or resolved.stem.casefold() != RELEASE_BINARY_STEM
    ):
        raise ValueError(
            f"{label} must be the release-profile {RELEASE_BINARY_STEM} executable"
        )
    return resolved


def canonical_raw_output_path(repository_revision: str) -> Path:
    """Return the revision-derived raw result path from the rerun contract."""

    return (
        ROOT / RAW_OUTPUT_DIRECTORY / f"{repository_revision}-v8-full-results.json"
    ).resolve()


def validate_raw_output(path: Path, repository_revision: str) -> Path:
    """Require the revision-derived raw output used by the rerun command."""

    resolved = path.resolve()
    expected = canonical_raw_output_path(repository_revision)
    if resolved != expected:
        raise ValueError(
            "--output must be the canonical revision-derived raw result path: "
            f"{expected.relative_to(ROOT).as_posix()}"
        )
    return resolved


def preflight_candidate_patch(candidate_patch: Path) -> dict[str, str]:
    """Check the candidate patch against an exact exported baseline source."""

    with tempfile.TemporaryDirectory(prefix="projectatlas-candidate-preflight-") as directory:
        source_root = Path(directory)
        archive_result = subprocess.run(
            ["git", "archive", "--format=tar", "HEAD"],
            cwd=ROOT,
            check=False,
            capture_output=True,
            timeout=RUN_TIMEOUT_SECONDS,
        )
        if archive_result.returncode != 0:
            stderr = archive_result.stderr.decode("utf-8", errors="replace")
            raise RuntimeError(
                f"baseline source export failed with {archive_result.returncode}: {stderr}"
            )
        with tarfile.open(fileobj=io.BytesIO(archive_result.stdout), mode="r:") as archive:
            root = source_root.resolve()
            for member in archive.getmembers():
                extracted = (source_root / member.name).resolve()
                if not extracted.is_relative_to(root):
                    raise RuntimeError(f"baseline archive escaped its root: {member.name}")
                archive.extract(member, source_root)
        check = subprocess.run(
            [
                "git",
                "-C",
                str(source_root),
                "apply",
                "--check",
                "--unidiff-zero",
                "--unsafe-paths",
                "--whitespace=nowarn",
                str(candidate_patch),
            ],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
            timeout=RUN_TIMEOUT_SECONDS,
        )
        if check.returncode != 0:
            raise RuntimeError(
                "candidate patch does not apply to the exported baseline: "
                f"{check.stderr.strip()}"
            )
        baseline_source = source_root / "crates/projectatlas-service/src/import_aliases.rs"
        return {
            "status": "passed",
            "repository_revision": git_revision(),
            "baseline_source_sha256": sha256_file(baseline_source),
        }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline-binary", type=Path, required=True)
    parser.add_argument("--baseline-evidence-binary", type=Path, required=True)
    parser.add_argument("--candidate-binary", type=Path, required=True)
    parser.add_argument("--candidate-evidence-binary", type=Path, required=True)
    parser.add_argument("--candidate-patch", type=Path, default=ROOT / "docs/benchmarks/reverse-caller-candidate.patch")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--compact-output",
        type=Path,
        help="also write bounded review evidence beside the retained raw output",
    )
    parser.add_argument("--repeats", type=int, default=3)
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error("--repeats must be positive")
    repository_revision = git_revision()
    try:
        baseline_binary = validate_release_binary(args.baseline_binary, "baseline binary")
        baseline_evidence_binary = validate_release_binary(
            args.baseline_evidence_binary, "baseline evidence binary"
        )
        candidate_binary = validate_release_binary(args.candidate_binary, "candidate binary")
        candidate_evidence_binary = validate_release_binary(
            args.candidate_evidence_binary, "candidate evidence binary"
        )
        output = validate_raw_output(args.output, repository_revision)
    except ValueError as error:
        parser.error(str(error))
    candidate_patch = validate_input(args.candidate_patch, "candidate patch")
    if not output.is_relative_to(ROOT):
        parser.error("--output must remain inside the repository")
    compact_output = args.compact_output.resolve() if args.compact_output else None
    if compact_output is not None and not compact_output.is_relative_to(ROOT):
        parser.error("--compact-output must remain inside the repository")
    if compact_output == output:
        parser.error("--compact-output must differ from --output")
    baseline_binary_sha256 = sha256_file(baseline_binary)
    baseline_evidence_binary_sha256 = sha256_file(baseline_evidence_binary)
    candidate_binary_sha256 = sha256_file(candidate_binary)
    candidate_evidence_binary_sha256 = sha256_file(candidate_evidence_binary)
    if baseline_binary_sha256 == candidate_binary_sha256:
        parser.error("baseline and candidate binaries must be distinct")
    if baseline_evidence_binary_sha256 == candidate_evidence_binary_sha256:
        parser.error("baseline and candidate evidence binaries must be distinct")
    if baseline_binary_sha256 == baseline_evidence_binary_sha256:
        parser.error("baseline timed and evidence binaries must be distinct")
    if candidate_binary_sha256 == candidate_evidence_binary_sha256:
        parser.error("candidate timed and evidence binaries must be distinct")
    if not candidate_patch.is_relative_to(ROOT):
        parser.error("--candidate-patch must remain inside the repository")
    candidate_patch_preflight = preflight_candidate_patch(candidate_patch)

    baseline: dict[str, Any] = {}
    candidate: dict[str, Any] = {}
    for shape in SHAPES:
        baseline[shape] = measure_shape(
            baseline_binary, baseline_evidence_binary, shape, args.repeats, "baseline"
        )
        candidate[shape] = measure_shape(
            candidate_binary, candidate_evidence_binary, shape, args.repeats, "candidate"
        )

    semantic_findings: list[str] = []
    for shape in SHAPES:
        baseline_runs = baseline[shape]["runs"]
        candidate_runs = candidate[shape]["runs"]
        if len(baseline_runs) != len(candidate_runs):
            semantic_findings.append(f"{shape}: run count drift")
            continue
        for index, (base_run, candidate_run) in enumerate(
            zip(baseline_runs, candidate_runs, strict=True)
        ):
            semantic_findings.extend(
                f"{shape}[{index}]: {finding}"
                for finding in compare_raw_runs(base_run, candidate_run)
            )
        if baseline[shape]["query_observations"] != candidate[shape]["query_observations"]:
            semantic_findings.append(f"{shape}: production query evidence drift")

    failures: dict[str, dict[str, Any]] = {}
    for case in (
        "missing-summary",
        "corrupt-relation",
        "corrupt-import-target",
        "corrupt-import-caller-path",
        "stale-source",
    ):
        failures[case] = {
            "baseline": failure_case(
                baseline_binary, baseline_evidence_binary, case, "baseline"
            ),
            "candidate": failure_case(
                candidate_binary, candidate_evidence_binary, case, "candidate"
            ),
        }
        semantic_findings.extend(
            f"{case}: {finding}"
            for finding in compare_raw_runs(
                failures[case]["baseline"], failures[case]["candidate"]
            )
        )

    multi_binding_alias = {
        "baseline": multi_binding_alias_case(
            baseline_evidence_binary, "baseline"
        ),
        "candidate": multi_binding_alias_case(
            candidate_evidence_binary, "candidate"
        ),
    }
    semantic_findings.extend(
        f"python-multi-binding-same-local-alias: {finding}"
        for finding in compare_raw_runs(
            multi_binding_alias["baseline"], multi_binding_alias["candidate"]
        )
    )

    cancellation = {
        "baseline": cancellation_case(baseline_evidence_binary, "baseline"),
        "candidate": cancellation_case(candidate_evidence_binary, "candidate"),
    }
    semantic_findings.extend(
        f"mcp-summary-cancellation: {finding}"
        for finding in compare_cancellation_runs(
            cancellation["baseline"], cancellation["candidate"]
        )
    )

    result = {
        "schema": RESULT_SCHEMA,
        "issue": 342,
        "repository_revision": repository_revision,
        "build_profile": "release",
        "measurement_split": {
            "timing": "ordinary-release-binary",
            "allocations": "untimed-benchmark-feature-allocation-only-replay",
            "query_plans": "untimed-benchmark-feature-trace-replay",
            "cancellation": "benchmark-feature-binary",
        },
        "repeats": args.repeats,
        "baseline": {
            "timed_binary_sha256": baseline_binary_sha256,
            "evidence_binary_sha256": baseline_evidence_binary_sha256,
            "fixtures": baseline,
        },
        "candidate": {
            "timed_binary_sha256": candidate_binary_sha256,
            "evidence_binary_sha256": candidate_evidence_binary_sha256,
            "candidate_patch": {
                "path": candidate_patch.relative_to(ROOT).as_posix(),
                "sha256": sha256_file(candidate_patch),
            },
            "fixtures": candidate,
        },
        "candidate_patch_preflight": candidate_patch_preflight,
        "failure_cases": failures,
        "multi_binding_alias": multi_binding_alias,
        "cancellation": cancellation,
        "decision": decision(baseline, candidate, semantic_findings),
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    if compact_output is not None:
        compact_output.parent.mkdir(parents=True, exist_ok=True)
        compact_output.write_text(
            json.dumps(compact_review_evidence(result, output), indent=2, sort_keys=True)
            + "\n",
            encoding="utf-8",
        )
    output_paths = {"output": output.relative_to(ROOT).as_posix()}
    if compact_output is not None:
        output_paths["compact_output"] = compact_output.relative_to(ROOT).as_posix()
    print(
        json.dumps(
            {
                **output_paths,
                "outcome": result["decision"]["outcome"],
                "semantic_findings": len(semantic_findings),
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
