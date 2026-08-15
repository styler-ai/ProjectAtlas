#!/usr/bin/env python3
"""Measure one high-fanout classified-document refresh with existing harness tools."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sqlite3
import subprocess
import time
from pathlib import Path
from typing import Any, Callable

from system_scale import (
    ROOT,
    clear_git_repository_environment,
    collect_measured_process,
    command,
    database_counts,
    database_profile,
    persistent_sizes,
    prepare_medium,
    remove_tree,
    run_measured,
    spawn_owned_process,
    storage_state,
    terminate_owned_process,
    wait_for_idle_watch_baseline,
    wait_for_indexed_marker,
    write_result,
)

SOURCE_FILES = 1_024
FANOUT = 256
DOCUMENT = Path("docs/high-fanout.md")
DEFAULT_WORK = ROOT / "target/benchmarks/classified-document-refresh/current"
DEFAULT_OUTPUT = ROOT / "docs/benchmarks/v0.4.5-rc1-classified-document-refresh.json"
SQLITE_PROBE = (
    "repository_graph::tests::"
    "high_fanout_document_refresh_has_bounded_sql_and_changed_rows"
)


def measured_json(
    runtime: Path,
    arguments: list[str],
    *,
    cwd: Path,
    env: dict[str, str],
    timeout_seconds: float,
) -> tuple[dict[str, Any], dict[str, Any]]:
    """Measure one candidate command without the historical v0.4.0 version pin."""
    run = run_measured(
        [str(runtime), "--format", "json", *arguments],
        cwd=cwd,
        env=env,
        timeout_seconds=timeout_seconds,
    )
    if run["returncode"] != 0:
        raise RuntimeError(
            f"{' '.join(arguments)} failed ({run['returncode']}): {run['stderr']}"
        )
    return run, json.loads(run.pop("stdout"))


def measured_watch_edit(
    runtime: Path,
    *,
    cwd: Path,
    env: dict[str, str],
    timeout_seconds: float,
    edit: Callable[[], None],
    readiness_file: Path,
    writer_probe_database: Path,
) -> tuple[dict[str, Any], dict[str, Any]]:
    """Measure the post-readiness refresh using the shared process samplers."""
    readiness_path = readiness_file.relative_to(cwd).as_posix()
    readiness_marker = f"projectatlas-watch-ready-{time.time_ns()}"
    with readiness_file.open("a", encoding="utf-8", newline="\n") as stream:
        stream.write(f"// {readiness_marker}\n")
    arguments = [
        str(runtime),
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
        exact_baseline = wait_for_idle_watch_baseline(job) if job is not None else None
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
    if metrics["returncode"] != 0:
        raise RuntimeError(f"watch failed ({metrics['returncode']}): {metrics['stderr']}")
    return metrics, json.loads(metrics.pop("stdout"))


def document_rows(database: Path) -> dict[str, int]:
    """Count the durable rows directly owned by the measured document path."""
    path = DOCUMENT.as_posix()
    queries = {
        "nodes": "SELECT COUNT(*) FROM nodes WHERE path = ?1",
        "classifications": (
            "SELECT COUNT(*) FROM file_content_classifications WHERE path = ?1"
        ),
        "file_texts": "SELECT COUNT(*) FROM file_texts WHERE path = ?1",
        "parse_metadata": "SELECT COUNT(*) FROM source_parse_metadata WHERE path = ?1",
        "symbols": "SELECT COUNT(*) FROM symbols WHERE path = ?1",
        "symbol_relations": "SELECT COUNT(*) FROM symbol_relations WHERE path = ?1",
        "graph_entities": (
            "SELECT COUNT(*) FROM graph_entities WHERE repository_path = ?1"
        ),
        "graph_relations": (
            "SELECT COUNT(*) FROM graph_relations WHERE source_entity_key IN ("
            "SELECT entity_key FROM graph_entities WHERE repository_path = ?1)"
        ),
        "graph_occurrences": (
            "SELECT COUNT(*) FROM graph_relation_occurrences WHERE file_path = ?1"
        ),
        "graph_coverage": (
            "SELECT COUNT(*) FROM graph_coverage WHERE scope_path = ?1"
        ),
        "graph_exports": (
            "SELECT COUNT(*) FROM graph_entity_exports WHERE owner_path = ?1"
        ),
        "graph_dependencies": (
            "SELECT COUNT(*) FROM graph_relation_dependencies WHERE owner_path = ?1"
        ),
    }
    connection = sqlite3.connect(f"file:{database.as_posix()}?mode=ro", uri=True)
    try:
        counts = {
            name: int(connection.execute(sql, [path]).fetchone()[0])
            for name, sql in queries.items()
        }
    finally:
        connection.close()
    counts["total"] = sum(counts.values())
    return counts


def sqlite_publication_probe() -> dict[str, Any]:
    """Run the owning traced SQLite publication test and retain its exact counts."""
    arguments = [
        "cargo",
        "test",
        "--locked",
        "-p",
        "projectatlas-db",
        SQLITE_PROBE,
        "--",
        "--exact",
        "--nocapture",
    ]
    completed = subprocess.run(
        arguments,
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        timeout=180,
    )
    output = completed.stdout + completed.stderr
    match = re.search(
        r"high-fanout document refresh: links=(\d+) statements=(\d+) changed_rows=(\d+)",
        output,
    )
    if completed.returncode != 0 or match is None:
        raise RuntimeError(f"SQLite publication probe failed:\n{output}")
    return {
        "command": " ".join(arguments),
        "links": int(match.group(1)),
        "statements": int(match.group(2)),
        "changed_rows": int(match.group(3)),
    }


def prepare_fixture(root: Path) -> None:
    prepare_medium(root, SOURCE_FILES)
    document = root / DOCUMENT
    document.parent.mkdir()
    links = "".join(
        f"- [caller {index:04}](../src/caller_{index:04}.rs)\n"
        for index in range(FANOUT)
    )
    document.write_text(
        "# High-fanout document\n\n" + links,
        encoding="utf-8",
        newline="\n",
    )
    command("git", "add", DOCUMENT.as_posix(), cwd=root)
    command("git", "commit", "-q", "-m", "classified document fixture", cwd=root)


def check(name: str, actual: Any, expected: Any, passed: bool) -> dict[str, Any]:
    return {"name": name, "actual": actual, "expected": expected, "passed": passed}


def run(runtime: Path, work_root: Path) -> dict[str, Any]:
    clear_git_repository_environment()
    allowed = (ROOT / "target/benchmarks/classified-document-refresh").resolve()
    work_root = work_root.resolve()
    try:
        work_root.relative_to(allowed)
    except ValueError as error:
        raise ValueError(f"work root must remain under {allowed}") from error
    if work_root.exists():
        remove_tree(work_root, allowed_parent=allowed)
    prepare_fixture(work_root)

    env = os.environ.copy()
    env["PROJECTATLAS_NO_TELEMETRY"] = "1"
    database = work_root / ".projectatlas/projectatlas.db"
    pre_scan = storage_state(work_root)
    scan_process, scan_report = measured_json(
        runtime,
        ["scan", "."],
        cwd=work_root,
        env=env,
        timeout_seconds=180,
    )
    full_counts = database_counts(database)
    post_scan = persistent_sizes(work_root)
    before_rows = document_rows(database)

    def edit_document() -> None:
        document = work_root / DOCUMENT
        content = document.read_text(encoding="utf-8")
        document.write_text(
            content.replace(
                "# High-fanout document", "# High-fanout document, refreshed", 1
            ),
            encoding="utf-8",
            newline="\n",
        )

    refresh_process, refresh_report = measured_watch_edit(
        runtime,
        cwd=work_root,
        env=env,
        timeout_seconds=180,
        edit=edit_document,
        readiness_file=work_root / "src/caller_0000.rs",
        writer_probe_database=database,
    )
    refresh_counts = database_counts(database)
    after_rows = document_rows(database)
    final_storage = persistent_sizes(work_root)
    sqlite_probe = sqlite_publication_probe()
    output_bytes = refresh_process["stdout_bytes"] + refresh_process["stderr_bytes"]
    checks = [
        check(
            "full build includes the fixture",
            scan_report["text_index"]["candidates"],
            f"> {SOURCE_FILES}",
            scan_report["text_index"]["candidates"] > SOURCE_FILES,
        ),
        check(
            "one refresh text candidate",
            refresh_report["text_index"]["candidates"],
            1,
            refresh_report["text_index"]["candidates"] == 1,
        ),
        check(
            "one refresh parse",
            refresh_report["last_symbols"]["parsed"],
            1,
            refresh_report["last_symbols"]["parsed"] == 1,
        ),
        check(
            "document fanout retained",
            after_rows["graph_relations"],
            FANOUT,
            before_rows["graph_relations"] == FANOUT
            and after_rows["graph_relations"] == FANOUT,
        ),
        check(
            "refresh generation published",
            refresh_counts["generation"],
            refresh_process["readiness_generation"] + 1,
            refresh_counts["generation"]
            == refresh_process["readiness_generation"] + 1,
        ),
        check(
            "SQLite probe fanout",
            sqlite_probe["links"],
            FANOUT,
            sqlite_probe["links"] == FANOUT,
        ),
        check(
            "bounded output",
            output_bytes,
            "<= 65536 bytes",
            output_bytes <= 65_536,
        ),
        check(
            "writer probe observed publication",
            refresh_process["writer_availability"]["attempts"],
            "> 0",
            refresh_process["writer_availability"]["attempts"] > 0,
        ),
        check(
            "final WAL drained",
            final_storage["wal_bytes"],
            0,
            final_storage["wal_bytes"] == 0,
        ),
        check(
            "no staging database remains",
            final_storage["stage_directories"],
            0,
            final_storage["stage_directories"] == 0,
        ),
    ]
    return {
        "schema": "projectatlas.classified-document-refresh.v1",
        "candidate": {
            "checkout_head": subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
            ).strip(),
            "runtime": str(runtime),
            "runtime_sha256": hashlib.sha256(runtime.read_bytes()).hexdigest(),
        },
        "fixture": {
            "source_files": SOURCE_FILES,
            "fanout": FANOUT,
            "document": DOCUMENT.as_posix(),
            "tracked": {
                "files": len(subprocess.check_output(
                    ["git", "ls-files"], cwd=work_root, text=True
                ).splitlines()),
            },
        },
        "full_build": {
            "pre_storage": pre_scan,
            "process": scan_process,
            "report": scan_report,
            "counts": full_counts,
            "post_storage": post_scan,
            "database_profile": database_profile(database),
        },
        "one_document_refresh": {
            "process": refresh_process,
            "report": refresh_report,
            "counts": refresh_counts,
            "owned_rows_before": before_rows,
            "owned_rows_after": after_rows,
            "final_storage": final_storage,
            "output_bytes": output_bytes,
        },
        "sqlite_publication_probe": sqlite_probe,
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
    result = run(runtime, args.work_root)
    write_result(result, args.output.resolve())


if __name__ == "__main__":
    main()
