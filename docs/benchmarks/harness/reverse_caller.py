#!/usr/bin/env python3
"""Run the ProjectAtlas #342 reverse-caller performance matrix."""

from __future__ import annotations

import argparse
import json
import sqlite3
import statistics
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Any

import psutil


SHAPES = {
    "small": (4, 1),
    "high-symbol": (320, 1),
    "high-import": (1, 240),
    "duplicate-alias": (1, 1),
    "representative-large": (120, 120),
}
RUN_TIMEOUT_SECONDS = 120


def run_process(command: list[str], cwd: Path) -> dict[str, Any]:
    """Run one bounded command while sampling child process resources."""

    started = time.perf_counter()
    with tempfile.TemporaryFile() as stdout_file, tempfile.TemporaryFile() as stderr_file:
        process = subprocess.Popen(command, cwd=cwd, stdout=stdout_file, stderr=stderr_file)
        observed = psutil.Process(process.pid)
        peak_rss = 0
        cpu_seconds = 0.0
        while process.poll() is None:
            if time.perf_counter() - started >= RUN_TIMEOUT_SECONDS:
                process.kill()
                process.wait()
                raise TimeoutError(f"benchmark command exceeded {RUN_TIMEOUT_SECONDS}s: {command}")
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
        stdout, stderr = stdout_file.read(), stderr_file.read()
        try:
            peak_rss = max(peak_rss, observed.memory_info().rss)
        except psutil.Error:
            pass
    if process.returncode:
        raise RuntimeError(
            f"{command} failed with {process.returncode}: {stderr.decode(errors='replace')}"
        )
    return {
        "wall_ms": (time.perf_counter() - started) * 1000,
        "cpu_ms": cpu_seconds * 1000,
        "peak_rss_bytes": peak_rss,
        "encoded_output_bytes": len(stdout),
        "stdout": stdout,
    }


def write_fixture(root: Path, shape: str) -> None:
    """Create one minimal real-source repository for a workload shape."""

    root.joinpath("src").mkdir(parents=True)
    symbols, callers = SHAPES[shape]
    target = "\n".join(f"pub fn target_{index}() {{}}" for index in range(symbols))
    root.joinpath("src/target.rs").write_text(target + "\n", encoding="utf-8")
    for caller in range(callers):
        selected = caller % symbols
        lines = [
            f"use crate::target::target_{selected} as target_alias_{selected};",
            f"fn caller_{caller}() {{ target_alias_{selected}(); }}",
        ]
        if shape == "duplicate-alias":
            lines.insert(1, "use crate::other::target_0 as target_alias_0;")
        root.joinpath("src", f"caller_{caller}.rs").write_text(
            "\n".join(lines) + "\n", encoding="utf-8"
        )
    if shape == "duplicate-alias":
        root.joinpath("src/other.rs").write_text("pub fn target_0() {}\n", encoding="utf-8")


def sqlite_evidence(database: Path) -> dict[str, Any]:
    """Record exact current import-alias reads and their covering plan."""

    connection = sqlite3.connect(database)
    matching = connection.execute(
        """
        SELECT path, source_name, target_name, line
        FROM symbol_relations
        WHERE kind = 'imports' AND target_name LIKE '%target%' ESCAPE '\\'
        ORDER BY path, line, source_name, target_name
        """
    ).fetchall()
    caller_paths = sorted({row[0] for row in matching})
    placeholders = ",".join("?" for _ in caller_paths) or "NULL"
    path_rows = connection.execute(
        f"""
        SELECT path, source_name, target_name, line
        FROM symbol_relations INDEXED BY idx_symbol_import_alias_lookup
        WHERE kind = 'imports' AND path IN ({placeholders})
        ORDER BY path, line, source_name, target_name
        """,
        caller_paths,
    ).fetchall()
    plan = connection.execute(
        """
        EXPLAIN QUERY PLAN
        SELECT path, source_name, target_name, line
        FROM symbol_relations INDEXED BY idx_symbol_import_alias_lookup
        WHERE kind = 'imports' AND target_name LIKE '%target%' ESCAPE '\\'
        ORDER BY path, line, source_name, target_name
        LIMIT 500
        """
    ).fetchall()
    path_plan = connection.execute(
        """
        EXPLAIN QUERY PLAN
        SELECT path, source_name, target_name, line
        FROM symbol_relations INDEXED BY idx_symbol_import_alias_lookup
        WHERE kind = 'imports' AND path = 'src/caller_0.rs'
        ORDER BY path, line, source_name, target_name
        LIMIT 1000
        """
    ).fetchall()
    relation_bytes = sum(
        len(str(path).encode()) + len(str(source).encode()) + len(str(target).encode()) + 8
        for path, source, target, _line in path_rows
    )
    connection.close()
    return {
        "matching_term_statements": 1,
        "caller_path_statements": len(caller_paths),
        "import_alias_statements": len(caller_paths) + 1,
        "matching_rows": len(matching),
        "caller_path_rows": len(path_rows),
        "import_relation_bytes": relation_bytes,
        "allocation_proxy_bytes": relation_bytes,
        "query_plan": [row[3] for row in plan],
        "exact_path_query_plan": [row[3] for row in path_plan],
    }


def measure_shape(binary: Path, shape: str, repeats: int) -> dict[str, Any]:
    """Build, scan, and repeatedly summarize one generated shape."""

    with tempfile.TemporaryDirectory(prefix=f"projectatlas-{shape}-") as directory:
        root = Path(directory)
        write_fixture(root, shape)
        database = root / ".projectatlas" / "projectatlas.db"
        run_process([str(binary), "init", "--no-scan"], root)
        run_process([str(binary), "scan"], root)
        runs = []
        for _ in range(repeats):
            measured = run_process(
                [
                    str(binary),
                    "--format",
                    "json",
                    "summary",
                    "src/target.rs",
                    "--limit",
                    "500",
                ],
                root,
            )
            json.loads(measured.pop("stdout"))
            runs.append(measured)
        return {
            "median_wall_ms": statistics.median(run["wall_ms"] for run in runs),
            "median_cpu_ms": statistics.median(run["cpu_ms"] for run in runs),
            "median_peak_rss_bytes": statistics.median(
                run["peak_rss_bytes"] for run in runs
            ),
            "median_encoded_output_bytes": statistics.median(
                run["encoded_output_bytes"] for run in runs
            ),
            "runs": runs,
            "sqlite": sqlite_evidence(database),
        }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--repeats", type=int, default=7)
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error("--repeats must be positive")
    if not args.binary.is_file():
        parser.error(f"missing binary: {args.binary}")
    result = {
        "binary": str(args.binary),
        "repeats": args.repeats,
        "fixtures": {
            shape: measure_shape(args.binary, shape, args.repeats) for shape in SHAPES
        },
    }
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
