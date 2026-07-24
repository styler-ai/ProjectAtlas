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
import statistics
import subprocess
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any

import psutil

from mcp_composition import FIXTURES, McpClient, remove_tree


ROOT = Path(__file__).resolve().parents[3]
DEFAULT_PREREGISTRATION = ROOT / "docs/benchmarks/v0.4-system-scale-preregistration.json"
DEFAULT_WORK = ROOT / "target/benchmarks/system-scale/current"
DEFAULT_OUTPUT = ROOT / "docs/benchmarks/v0.4-system-scale-raw.json"
DEFAULT_CORPUS_CACHE = ROOT / "target/benchmarks/system-scale/corpus-cache"
MEASURE_INTERVAL_SECONDS = 0.02
TOON_INTEGER = r"^\s+{key}: (\d+)$"


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
    except psutil.Error:
        return []


def process_tree_rss(root_pid: int) -> int:
    total = 0
    for process in process_tree(root_pid):
        try:
            total += process.memory_info().rss
        except psutil.Error:
            continue
    return total


def process_tree_io(root_pid: int) -> dict[str, int]:
    totals = {
        "read_count": 0,
        "write_count": 0,
        "read_bytes": 0,
        "write_bytes": 0,
    }
    for process in process_tree(root_pid):
        try:
            counters = process.io_counters()
        except psutil.Error:
            continue
        for key in totals:
            totals[key] += int(getattr(counters, key))
    return totals


class ProcessTreeSampler:
    def __init__(self, root_pid: int) -> None:
        self.root_pid = root_pid
        self.stop_event = threading.Event()
        self.thread = threading.Thread(target=self._sample_until_stopped, daemon=True)
        self.peak_rss_bytes = 0
        self.peak_processes = 0
        self.cpu_seconds: dict[int, float] = {}
        self.read_bytes: dict[int, int] = {}
        self.write_bytes: dict[int, int] = {}

    def start(self) -> None:
        self.thread.start()

    def stop(self) -> dict[str, Any]:
        self.stop_event.set()
        self.thread.join(timeout=5)
        return {
            "sampler": f"psutil-{psutil.__version__}",
            "interval_seconds": MEASURE_INTERVAL_SECONDS,
            "peak_rss_bytes": self.peak_rss_bytes,
            "peak_processes": self.peak_processes,
            "peak_worker_processes": max(0, self.peak_processes - 1),
            "cpu_seconds": round(sum(self.cpu_seconds.values()), 6),
            "read_bytes": sum(self.read_bytes.values()),
            "write_bytes": sum(self.write_bytes.values()),
        }

    def _sample_until_stopped(self) -> None:
        while not self.stop_event.is_set():
            processes = process_tree(self.root_pid)
            rss = 0
            for process in processes:
                try:
                    rss += process.memory_info().rss
                    cpu = process.cpu_times()
                    self.cpu_seconds[process.pid] = max(
                        self.cpu_seconds.get(process.pid, 0.0), cpu.user + cpu.system
                    )
                    io = process.io_counters()
                    self.read_bytes[process.pid] = max(
                        self.read_bytes.get(process.pid, 0), io.read_bytes
                    )
                    self.write_bytes[process.pid] = max(
                        self.write_bytes.get(process.pid, 0), io.write_bytes
                    )
                except (psutil.AccessDenied, psutil.NoSuchProcess):
                    continue
            self.peak_rss_bytes = max(self.peak_rss_bytes, rss)
            self.peak_processes = max(self.peak_processes, len(processes))
            self.stop_event.wait(MEASURE_INTERVAL_SECONDS)


def terminate_process_tree(process: subprocess.Popen[bytes]) -> None:
    members = process_tree(process.pid)
    for member in reversed(members[1:]):
        try:
            member.kill()
        except psutil.Error:
            continue
    if process.poll() is None:
        process.kill()


def run_measured(
    arguments: list[str],
    *,
    cwd: Path,
    env: dict[str, str],
    timeout_seconds: float,
) -> dict[str, Any]:
    started = time.perf_counter()
    process = subprocess.Popen(
        arguments,
        cwd=cwd,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    sampler = ProcessTreeSampler(process.pid)
    sampler.start()
    timed_out = False
    try:
        stdout, stderr = process.communicate(timeout=timeout_seconds)
    except subprocess.TimeoutExpired:
        timed_out = True
        terminate_process_tree(process)
        stdout, stderr = process.communicate(timeout=5)
    elapsed = time.perf_counter() - started
    metrics = sampler.stop()
    logical_cpus = os.cpu_count() or 1
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
            "stdout_bytes": len(stdout),
            "stderr_bytes": len(stderr),
            "stdout": stdout.decode("utf-8", errors="replace"),
            "stderr": stderr.decode("utf-8", errors="replace"),
        }
    )
    return metrics


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
    warmup_seconds: float,
    edit: Any,
    expected_error: str | None = None,
) -> tuple[dict[str, Any], dict[str, Any]]:
    arguments = [
        str(runtime),
        "--require-version",
        "0.4.0",
        "--format",
        "json",
        "watch",
        "--poll-seconds",
        "1",
        "--max-cycles",
        "2",
        ".",
    ]
    started = time.perf_counter()
    process = subprocess.Popen(
        arguments,
        cwd=cwd,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    sampler = ProcessTreeSampler(process.pid)
    sampler.start()
    time.sleep(warmup_seconds)
    if process.poll() is not None:
        stdout, stderr = process.communicate()
        sampler.stop()
        raise RuntimeError(
            "watcher exited before the edit: "
            f"{stdout.decode(errors='replace')} {stderr.decode(errors='replace')}"
        )
    edit_started = time.perf_counter()
    edit()
    timed_out = False
    try:
        stdout, stderr = process.communicate(timeout=timeout_seconds)
    except subprocess.TimeoutExpired:
        timed_out = True
        terminate_process_tree(process)
        stdout, stderr = process.communicate(timeout=5)
    elapsed = time.perf_counter() - started
    metrics = sampler.stop()
    logical_cpus = os.cpu_count() or 1
    metrics.update(
        {
            "arguments": arguments[1:],
            "returncode": process.returncode,
            "timed_out": timed_out,
            "wall_seconds": round(elapsed, 6),
            "watcher_warmup_seconds": round(warmup_seconds, 6),
            "edit_to_complete_seconds": round(
                time.perf_counter() - edit_started, 6
            ),
            "one_core_cpu_percent": round(
                metrics["cpu_seconds"] / elapsed * 100 if elapsed else 0.0, 3
            ),
            "host_cpu_percent": round(
                metrics["cpu_seconds"] / elapsed / logical_cpus * 100
                if elapsed
                else 0.0,
                3,
            ),
            "stdout_bytes": len(stdout),
            "stderr_bytes": len(stderr),
            "stderr": stderr.decode("utf-8", errors="replace"),
        }
    )
    combined_output = (stdout + stderr).decode("utf-8", errors="replace")
    if process.returncode != 0 and (
        expected_error is None or expected_error not in combined_output
    ):
        raise RuntimeError(
            f"live watch failed ({process.returncode}): {metrics['stderr']}"
        )
    if expected_error is not None:
        if process.returncode == 0:
            raise RuntimeError(f"live watch did not return expected error: {expected_error}")
        metrics["expected_error"] = expected_error
        return metrics, {
            "status": "refresh_required",
            "diagnostic": combined_output.strip(),
        }
    return metrics, json.loads(stdout)


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
    database = root / ".projectatlas/projectatlas.db"
    paths = {
        "database_bytes": database,
        "wal_bytes": Path(f"{database}-wal"),
        "shm_bytes": Path(f"{database}-shm"),
    }
    result = {
        name: path.stat().st_size if path.exists() else 0 for name, path in paths.items()
    }
    result["total_bytes"] = sum(result.values())
    return result


def toon_integer(text: str, key: str) -> int | None:
    match = re.search(TOON_INTEGER.format(key=re.escape(key)), text, re.MULTILINE)
    return int(match.group(1)) if match else None


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


def mcp_queries(
    runtime: Path,
    root: Path,
    env: dict[str, str],
    query: dict[str, Any],
) -> dict[str, Any]:
    client = McpClient(runtime, root, env)
    sampler = ProcessTreeSampler(client.process.pid)
    sampler.start()
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
                    key: max(0, io_after[key] - io_before[key]) for key in io_before
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
        call("atlas_overview", {}, "first_exact_verification")
        rss_before = process_tree_rss(client.process.pid)
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
                call(tool, arguments, f"healthy_epoch_{repetition + 1}")
        rss_after = process_tree_rss(client.process.pid)
    finally:
        client.close()
        process_metrics = sampler.stop()

    healthy_latencies = [
        row["elapsed_ms"] for row in calls if row["phase"].startswith("healthy_epoch_")
    ]
    return {
        "startup_ms": round(client.startup_ms, 3),
        "rss_before_repeated_bytes": rss_before,
        "rss_after_repeated_bytes": rss_after,
        "retained_rss_growth_bytes": rss_after - rss_before,
        "healthy_query_median_ms": round(statistics.median(healthy_latencies), 3),
        "healthy_query_p95_ms": round(percentile(healthy_latencies, 0.95), 3),
        "maximum_output_bytes": max(row["output_bytes"] for row in calls),
        "process": process_metrics,
        "calls": calls,
    }


def database_counts(database: Path) -> dict[str, int]:
    import sqlite3

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


def run_incremental(
    runtime: Path,
    root: Path,
    env: dict[str, str],
    timeout_seconds: float,
    watcher_warmup_seconds: float,
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
        warmup_seconds=watcher_warmup_seconds,
        edit=narrow_edit,
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
        warmup_seconds=watcher_warmup_seconds,
        edit=expanded_edit,
        expected_error="dependency-aware incremental closure exceeded its safe limit",
    )
    after_guidance = database_counts(database)
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
    facts = corpus_facts(root)
    scan_run, scan = measured_json(
        runtime,
        ["scan", "."],
        cwd=root,
        env=env,
        timeout_seconds=timeout_seconds,
    )
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
            max(2.0, scan_run["wall_seconds"] * 1.5 + 1.0),
        )
        if incremental
        else None
    )
    result = {
        "scale": scale,
        "variant": variant,
        "root": str(root),
        "corpus": facts,
        "scan": {"process": scan_run, "report": scan},
        "settings": {"process": settings_run, "report": settings},
        "unchanged_refresh": {"process": unchanged_run, "report": unchanged},
        "incremental": incremental_result,
        "persistent": persistent_sizes(root),
        "queries": mcp_queries(runtime, root, env, query),
    }
    result["checks"] = evaluate_case(result, preregistration)
    return result


def evaluate_case(
    result: dict[str, Any], preregistration: dict[str, Any]
) -> list[dict[str, Any]]:
    all_limits = preregistration["thresholds"]["all"]
    limits = preregistration["thresholds"][result["scale"]]
    report = result["scan"]["report"]
    indexed_files = result["settings"]["report"]["index"]["files"]
    corpus_limits = preregistration["corpora"][result["scale"]]
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
            "full scan peak RSS bytes",
            result["scan"]["process"]["peak_rss_bytes"],
            "<=",
            limits["maximum_peak_rss_bytes"],
            result["scan"]["process"]["peak_rss_bytes"]
            <= limits["maximum_peak_rss_bytes"],
        ),
        (
            "worker processes",
            result["scan"]["process"]["peak_worker_processes"],
            "<=",
            all_limits["maximum_worker_processes"],
            result["scan"]["process"]["peak_worker_processes"]
            <= all_limits["maximum_worker_processes"],
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
            "healthy query p95 milliseconds",
            result["queries"]["healthy_query_p95_ms"],
            "<=",
            limits["maximum_query_p95_milliseconds"],
            result["queries"]["healthy_query_p95_ms"]
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
            "persistent bytes",
            result["persistent"]["total_bytes"],
            "<=",
            limits["maximum_persistent_bytes"],
            result["persistent"]["total_bytes"]
            <= limits["maximum_persistent_bytes"],
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
    ]
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

    healthy_calls = [
        call
        for call in result["queries"]["calls"]
        if call["phase"].startswith("healthy_epoch_")
    ]
    bounded_reads = [
        call["process_io"]["read_bytes"]
        for call in healthy_calls
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

    fallback_calls = [
        call
        for call in healthy_calls
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
        call for call in healthy_calls if call["tool"] == "atlas_symbol_relations"
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
        guidance = incremental["expanded"]["guidance"]
        rebuild = incremental["expanded"]["rebuild"]
        caller_files = preregistration["corpora"]["medium"]["caller_files"]
        checks.extend(
            [
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
                    "narrow publication generation",
                    narrow["counts"]["generation"],
                    "==",
                    before["generation"] + 1,
                    narrow["counts"]["generation"] == before["generation"] + 1,
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
                    "expanded closure returns refresh guidance",
                    guidance["report"]["status"],
                    "==",
                    "refresh_required",
                    guidance["report"]["status"] == "refresh_required",
                ),
                (
                    "expanded guidance preserves generation",
                    guidance["counts"]["generation"],
                    "==",
                    narrow["counts"]["generation"],
                    guidance["counts"] == narrow["counts"],
                ),
                (
                    "expanded guidance writes no database bytes",
                    guidance["process"]["write_bytes"],
                    "==",
                    0,
                    guidance["process"]["write_bytes"] == 0,
                ),
                (
                    "explicit rebuild generation",
                    rebuild["counts"]["generation"],
                    "==",
                    narrow["counts"]["generation"] + 1,
                    rebuild["counts"]["generation"] == narrow["counts"]["generation"] + 1,
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


def concurrent_isolation(
    runtime: Path,
    roots: list[Path],
    env: dict[str, str],
    timeout_seconds: float,
) -> dict[str, Any]:
    started = time.perf_counter()
    with ThreadPoolExecutor(max_workers=len(roots)) as pool:
        futures = [
            pool.submit(
                measured_json,
                runtime,
                ["watch", "--once", "."],
                cwd=root,
                env=env,
                timeout_seconds=timeout_seconds,
            )
            for root in roots
        ]
        results = [future.result()[0] for future in futures]
    return {
        "wall_seconds": round(time.perf_counter() - started, 6),
        "roots": [str(root) for root in roots],
        "runs": results,
        "passed": all(result["returncode"] == 0 for result in results),
    }


def cancellation_quiescence(
    runtime: Path,
    source_root: Path,
    work_root: Path,
    env: dict[str, str],
    threshold_seconds: float,
) -> dict[str, Any]:
    database = work_root / "cancel/projectatlas.db"
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
            ".",
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
        if len(known) > 1:
            break
        time.sleep(MEASURE_INTERVAL_SECONDS)
    if process.poll() is not None:
        return {
            "passed": False,
            "reason": "scan completed before cancellation could be requested",
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
            return {
                "passed": True,
                "quiescence_seconds": round(elapsed, 6),
                "known_processes": len(known),
                "survivors": [],
            }
        time.sleep(MEASURE_INTERVAL_SECONDS)
    for pid in survivors:
        try:
            psutil.Process(pid).kill()
        except psutil.Error:
            continue
    return {
        "passed": False,
        "quiescence_seconds": round(time.perf_counter() - requested, 6),
        "known_processes": len(known),
        "survivors": survivors,
    }


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
    runtime = args.runtime.resolve(strict=True)
    preregistration = json.loads(args.preregistration.read_text(encoding="utf-8"))
    work_root = args.work_root.resolve()
    allowed = (ROOT / "target/benchmarks/system-scale").resolve()
    if work_root == allowed or allowed not in work_root.parents:
        raise SystemExit(f"--work-root must be a child of {allowed}")
    corpus_cache = args.corpus_cache.resolve()
    if corpus_cache != allowed and allowed not in corpus_cache.parents:
        raise SystemExit(f"--corpus-cache must be {allowed} or one of its children")
    if corpus_cache == work_root or work_root in corpus_cache.parents:
        raise SystemExit("--corpus-cache must not be inside --work-root")
    if work_root.exists():
        remove_tree(
            Path(f"\\\\?\\{work_root}") if os.name == "nt" else work_root
        )
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
        concurrent_isolation(runtime, [small["clean"], medium], env, timeout)
        if args.only == "all"
        else None
    )
    cancellation = (
        cancellation_quiescence(
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
        "preregistration": str(args.preregistration.resolve()),
        "mode": args.only,
        "publication_eligible": args.only == "all",
        "candidate": {
            "version": subprocess.check_output([runtime, "--version"], text=True).strip(),
            "runtime": str(runtime),
            "runtime_sha256": hashlib.sha256(runtime.read_bytes()).hexdigest(),
            "runtime_bytes": runtime.stat().st_size,
            "git_head": subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
            ).strip(),
        },
        "environment": {
            "platform": platform.platform(),
            "python": platform.python_version(),
            "psutil": psutil.__version__,
            "logical_cpus": os.cpu_count(),
            "telemetry": "disabled",
        },
        "cases": cases,
        "concurrent_isolation": concurrency,
        "cancellation": cancellation,
    }
    result["passed"] = (
        all(check["passed"] for case in cases for check in case["checks"])
        and (concurrency is None or concurrency["passed"])
        and (cancellation is None or cancellation["passed"])
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(result, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    print(args.output)


if __name__ == "__main__":
    main()
