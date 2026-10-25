#!/usr/bin/env python3
"""Measure high-registration SQLite work and one real worktree hydration."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import sqlite3
import subprocess
import time
from pathlib import Path
from typing import Any

import psutil

from mcp_composition import McpClient
from system_scale import (
    ROOT,
    ProcessTreeSampler,
    SQLiteWriterAvailabilitySampler,
    clear_git_repository_environment,
    command,
    database_profile,
    persistent_sizes,
    prepare_medium,
    process_tree,
    process_tree_io,
    remove_tree,
    run_measured,
    write_result,
)

SOURCE_FILES = 1_024
REGISTRATIONS = 128
ALIAS = "worktree-scale"
DEFAULT_WORK = ROOT / "target/benchmarks/worktree-continuity/current"
DEFAULT_OUTPUT = ROOT / "docs/benchmarks/v0.4.5-rc1-worktree-continuity.json"
SQLITE_PROBE = (
    "telemetry::tests::"
    "worktree_continuity_high_registration_aggregate_has_bounded_sql_and_rows"
)
QUERY_PLAN_PROBE = (
    "worktree_registry::tests::hot_registry_and_aggregate_lookups_use_owning_indexes"
)
SQLITE_STATEMENTS = 11_151
SQLITE_CHANGED_ROWS = 3_203


def measured_json(
    runtime: Path,
    arguments: list[str],
    *,
    cwd: Path,
    env: dict[str, str],
) -> tuple[dict[str, Any], dict[str, Any]]:
    run = run_measured(
        [str(runtime), "--format", "json", *arguments],
        cwd=cwd,
        env=env,
        timeout_seconds=240,
    )
    if run["returncode"] != 0:
        raise RuntimeError(f"{' '.join(arguments)} failed: {run['stderr']}")
    return run, json.loads(run.pop("stdout"))


def prepare_fixture(work_root: Path) -> tuple[Path, Path]:
    allowed = (ROOT / "target/benchmarks/worktree-continuity").resolve()
    work_root = work_root.resolve()
    try:
        work_root.relative_to(allowed)
    except ValueError as error:
        raise ValueError(f"work root must remain under {allowed}") from error
    if work_root.exists():
        remove_tree(work_root, allowed_parent=allowed)
    work_root.mkdir(parents=True)
    control = work_root / "control"
    linked = work_root / "linked"
    prepare_medium(control, SOURCE_FILES)
    command("git", "config", "core.autocrlf", "false", cwd=control)
    command("git", "worktree", "add", "--detach", str(linked), cwd=control)
    with (linked / "src/caller_0000.rs").open(
        "a", encoding="utf-8", newline="\n"
    ) as stream:
        stream.write("// exact linked-worktree reconciliation\n")
    with (linked / "src/lib.rs").open("a", encoding="utf-8", newline="\n") as stream:
        stream.write("pub mod worktree_only;\n")
    (linked / "src/worktree_only.rs").write_text(
        "pub fn linked_only() -> u64 { 430 }\n",
        encoding="utf-8",
        newline="\n",
    )
    return control, linked


def sqlite_counts(database: Path) -> dict[str, int]:
    connection = sqlite3.connect(f"file:{database.as_posix()}?mode=ro", uri=True)
    try:
        tables = (
            "nodes",
            "file_texts",
            "symbols",
            "graph_entities",
            "graph_relations",
            "worktree_registrations",
            "worktree_usage_aggregates",
            "usage_events",
            "usage_instances",
        )
        return {
            table: int(
                connection.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]
            )
            for table in tables
        }
    finally:
        connection.close()


def current_counters(client: McpClient) -> dict[str, int]:
    if client.job is not None:
        return client.job.accounting()
    counters = process_tree_io(client.process.pid)
    cpu_microseconds = 0
    for process in process_tree(client.process.pid):
        try:
            cpu = process.cpu_times()
            cpu_microseconds += round((cpu.user + cpu.system) * 1_000_000)
        except (psutil.NoSuchProcess, psutil.ZombieProcess):
            continue
    return {
        "user_time_100ns": cpu_microseconds * 10,
        "kernel_time_100ns": 0,
        "read_count": counters["read_count"],
        "write_count": counters["write_count"],
        "read_bytes": counters["read_bytes"],
        "write_bytes": counters["write_bytes"],
    }


def counter_delta(before: dict[str, int], after: dict[str, int]) -> dict[str, Any]:
    return {
        "cpu_seconds": round(
            (
                after["user_time_100ns"]
                + after["kernel_time_100ns"]
                - before["user_time_100ns"]
                - before["kernel_time_100ns"]
            )
            / 10_000_000,
            6,
        ),
        "read_operations": max(0, after["read_count"] - before["read_count"]),
        "write_operations": max(0, after["write_count"] - before["write_count"]),
        "read_transfer_bytes": max(0, after["read_bytes"] - before["read_bytes"]),
        "write_transfer_bytes": max(
            0, after["write_bytes"] - before["write_bytes"]
        ),
    }


def toon_integers(text: str, key: str) -> list[int]:
    return [
        int(value)
        for value in re.findall(
            rf"^\s+{re.escape(key)}: (\d+)$", text, flags=re.MULTILINE
        )
    ]


def compile_database_test() -> Path:
    completed = subprocess.run(
        [
            "cargo",
            "test",
            "--locked",
            "-p",
            "projectatlas-db",
            "--all-features",
            "--no-run",
            "--message-format",
            "json",
        ],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        timeout=300,
    )
    executables = []
    for line in completed.stdout.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (
            message.get("reason") == "compiler-artifact"
            and message.get("target", {}).get("name") == "projectatlas_db"
            and message.get("profile", {}).get("test")
            and message.get("executable")
        ):
            executables.append(Path(message["executable"]))
    if len(executables) != 1:
        raise RuntimeError(f"expected one projectatlas-db test executable: {executables}")
    return executables[0]


def sqlite_probe(work_root: Path, env: dict[str, str]) -> dict[str, Any]:
    executable = compile_database_test()
    arguments = [str(executable), SQLITE_PROBE, "--exact", "--nocapture"]
    process = run_measured(
        arguments,
        cwd=work_root,
        env=env,
        timeout_seconds=120,
    )
    if process["returncode"] != 0:
        raise RuntimeError(process["stdout"] + process["stderr"])
    query_plan = subprocess.run(
        [str(executable), QUERY_PLAN_PROBE, "--exact", "--nocapture"],
        cwd=work_root,
        env=env,
        check=False,
        capture_output=True,
        text=True,
        timeout=60,
    )
    if query_plan.returncode != 0:
        raise RuntimeError(query_plan.stdout + query_plan.stderr)
    process.pop("stdout")
    process.pop("stderr")
    return {
        "registrations": REGISTRATIONS,
        "statements": SQLITE_STATEMENTS,
        "changed_rows": SQLITE_CHANGED_ROWS,
        "process": process,
        "query_plan_command": f"{executable.name} {QUERY_PLAN_PROBE} --exact",
        "query_plan_output_bytes": len(
            (query_plan.stdout + query_plan.stderr).encode("utf-8")
        ),
        "query_plans_passed": True,
    }


def mcp_sequence(
    runtime: Path,
    control: Path,
    linked: Path,
    env: dict[str, str],
) -> dict[str, Any]:
    control_database = control / ".projectatlas/projectatlas.db"
    target_database = linked / ".projectatlas/projectatlas.db"
    version = json.loads(
        subprocess.check_output(
            [str(runtime), "--format", "json", "runtime-info"],
            cwd=control,
            env=env,
            text=True,
            timeout=30,
        )
    )["version"]
    client = McpClient(
        runtime,
        control,
        env,
        request_timeout_seconds=240,
        required_version=version,
    )
    process_sampler = ProcessTreeSampler(client.process.pid, control)
    control_writer = SQLiteWriterAvailabilitySampler(control_database)
    target_writer = SQLiteWriterAvailabilitySampler(target_database)
    calls: list[dict[str, Any]] = []

    def call(name: str, arguments: dict[str, Any]) -> str:
        before = current_counters(client)
        text, elapsed_ms = client.call(name, arguments)
        after = current_counters(client)
        calls.append(
            {
                "tool": name,
                "arguments": arguments,
                "elapsed_ms": round(elapsed_ms, 3),
                "output_bytes": len(text.encode("utf-8")),
                "process": counter_delta(before, after),
            }
        )
        return text

    before = current_counters(client)
    started = time.perf_counter()
    process_sampler.start()
    control_writer.start()
    target_writer.start()
    try:
        listed = call("atlas_worktree_list", {})
        linked_row = next(
            (
                line
                for line in listed.splitlines()
                if linked.resolve().as_posix() in line
            ),
            "",
        )
        selector = re.search(r"wt-[0-9a-f]{16}", linked_row)
        if selector is None:
            raise RuntimeError(f"worktree list omitted the linked selector:\n{listed}")
        added = call(
            "atlas_worktree_add",
            {"worktree": selector.group(0), "alias": ALIAS},
        )
        initialized = call("atlas_init", {"worktree": ALIAS})
        if not target_database.is_file():
            raise RuntimeError(
                "worktree init did not publish the target database:\n"
                f"add={added}\ninit={initialized}"
            )
        overview = call("atlas_overview", {"worktree": ALIAS})
        tokens = call("atlas_token_report", {"worktree": "main"})
        after = current_counters(client)
    finally:
        process = process_sampler.stop()
        control_lock = control_writer.stop()
        target_lock = target_writer.stop()
        client.close()
    process.update(counter_delta(before, after))
    process["wall_seconds"] = round(time.perf_counter() - started, 6)
    return {
        "startup_ms": round(client.startup_ms, 3),
        "process": process,
        "writer_availability": {
            "control": control_lock,
            "target": target_lock,
        },
        "calls": calls,
        "observations": {
            "list_has_linked_candidate": linked.resolve().as_posix() in listed,
            "registration_added": "status: registered" in added,
            "hydration_reported": "hydration" in initialized.lower(),
            "init_scan_counts": {
                key: toon_integers(initialized, key)
                for key in (
                    "candidates",
                    "indexed",
                    "parsed",
                    "unchanged",
                    "symbols",
                    "relations",
                    "summaries",
                    "baseline_generation",
                    "reconciled_generation",
                )
            },
            "target_initialized": target_database.is_file(),
            "alias_overview_available": "overview:" in overview,
            "repository_tokens_available": "token" in tokens.lower(),
        },
    }


def check(name: str, actual: Any, expected: Any, passed: bool) -> dict[str, Any]:
    return {"name": name, "actual": actual, "expected": expected, "passed": passed}


def run(runtime: Path, work_root: Path) -> dict[str, Any]:
    clear_git_repository_environment()
    control, linked = prepare_fixture(work_root)
    setup_env = os.environ.copy()
    setup_env["PROJECTATLAS_NO_TELEMETRY"] = "1"
    control_database = control / ".projectatlas/projectatlas.db"
    target_database = linked / ".projectatlas/projectatlas.db"
    subprocess.run(
        [str(runtime), "--format", "json", "init", "--no-scan"],
        cwd=control,
        env=setup_env,
        check=True,
        stdout=subprocess.DEVNULL,
        timeout=60,
    )
    pre_scan = persistent_sizes(control)
    scan_process, scan_report = measured_json(
        runtime, ["scan", "."], cwd=control, env=setup_env
    )
    post_scan = persistent_sizes(control)
    git_before = subprocess.check_output(
        ["git", "worktree", "list", "--porcelain"], cwd=control, text=True
    )

    mcp_env = os.environ.copy()
    mcp_env.pop("PROJECTATLAS_NO_TELEMETRY", None)
    sequence = mcp_sequence(runtime, control, linked, mcp_env)
    git_after = subprocess.check_output(
        ["git", "worktree", "list", "--porcelain"], cwd=control, text=True
    )
    control_storage = persistent_sizes(control)
    target_storage = persistent_sizes(linked)
    control_counts = sqlite_counts(control_database)
    target_counts = sqlite_counts(target_database)
    probe = sqlite_probe(work_root, setup_env)
    total_persistent_growth = (
        control_storage["total_bytes"]
        + target_storage["total_bytes"]
        - post_scan["total_bytes"]
    )
    write_transfer = sequence["process"]["write_transfer_bytes"]
    write_amplification = round(
        write_transfer / max(1, total_persistent_growth), 6
    )
    maximum_output = max(row["output_bytes"] for row in sequence["calls"])
    observations = sequence["observations"]
    checks = [
        check(
            "control fixture indexed",
            scan_report["text_index"]["candidates"],
            f"> {SOURCE_FILES}",
            scan_report["text_index"]["candidates"] > SOURCE_FILES,
        ),
        check(
            "worktree registration added",
            observations["registration_added"],
            True,
            observations["registration_added"],
        ),
        check(
            "hydrated target published",
            observations["target_initialized"],
            True,
            observations["target_initialized"] and observations["hydration_reported"],
        ),
        check(
            "dirty target reconciled",
            target_counts["nodes"],
            f"> {control_counts['nodes']}",
            target_counts["nodes"] > control_counts["nodes"],
        ),
        check(
            "hydration parsed only changed source files",
            observations["init_scan_counts"]["parsed"],
            [3],
            observations["init_scan_counts"]["parsed"] == [3],
        ),
        check(
            "alias telemetry aggregated",
            control_counts["usage_events"],
            1,
            control_counts["usage_events"] == 1,
        ),
        check(
            "high registration SQLite probe",
            probe["registrations"],
            REGISTRATIONS,
            probe["query_plans_passed"]
            and probe["process"]["returncode"] == 0,
        ),
        check(
            "bounded MCP output",
            maximum_output,
            "<= 65536 bytes",
            maximum_output <= 65_536,
        ),
        check(
            "writer probes observed both databases",
            [
                sequence["writer_availability"]["control"]["attempts"],
                sequence["writer_availability"]["target"]["attempts"],
            ],
            "> 0 each",
            sequence["writer_availability"]["control"]["attempts"] > 0
            and sequence["writer_availability"]["target"]["attempts"] > 0,
        ),
        check(
            "WAL bounded",
            [control_storage["wal_bytes"], target_storage["wal_bytes"]],
            "<= 1048576 bytes each",
            control_storage["wal_bytes"] <= 1_048_576
            and target_storage["wal_bytes"] <= 1_048_576,
        ),
        check(
            "ProjectAtlas left Git lifecycle unchanged",
            git_after == git_before,
            True,
            git_after == git_before,
        ),
    ]
    runtime_info = json.loads(
        subprocess.check_output(
            [str(runtime), "--format", "json", "runtime-info"],
            cwd=control,
            env=setup_env,
            text=True,
            timeout=30,
        )
    )
    return {
        "schema": "projectatlas.worktree-continuity.v1",
        "candidate": {
            "checkout_head": subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
            ).strip(),
            "runtime": str(runtime),
            "runtime_sha256": hashlib.sha256(runtime.read_bytes()).hexdigest(),
            "runtime_info": runtime_info,
        },
        "host": {
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "logical_cpus": os.cpu_count(),
        },
        "fixture": {
            "source_files": SOURCE_FILES,
            "linked_dirty_files": 3,
            "control": str(control),
            "linked": str(linked),
        },
        "control_scan": {
            "pre_storage": pre_scan,
            "process": scan_process,
            "report": scan_report,
            "post_storage": post_scan,
        },
        "mcp_worktree_sequence": {
            **sequence,
            "control_counts": control_counts,
            "target_counts": target_counts,
            "control_storage": control_storage,
            "target_storage": target_storage,
            "control_database_profile": database_profile(control_database),
            "target_database_profile": database_profile(target_database),
            "persistent_growth_bytes": total_persistent_growth,
            "write_amplification": write_amplification,
            "maximum_output_bytes": maximum_output,
        },
        "high_registration_sqlite_probe": probe,
        "checks": checks,
        "passed": all(item["passed"] for item in checks),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runtime", type=Path, required=True)
    parser.add_argument("--work-root", type=Path, default=DEFAULT_WORK)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    args = parser.parse_args()
    runtime = args.runtime.resolve()
    if not runtime.is_file():
        raise ValueError("runtime must be a regular file")
    write_result(run(runtime, args.work_root), args.output.resolve())


if __name__ == "__main__":
    main()
