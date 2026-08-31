#!/usr/bin/env python3
"""Run the ProjectAtlas #342 reverse-caller performance decision matrix."""

from __future__ import annotations

import argparse
import base64
import hashlib
import io
import json
import os
import sqlite3
import statistics
import subprocess
import tarfile
import tempfile
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
) -> dict[str, Any]:
    """Run one bounded child process and retain its exact raw streams."""

    environment = os.environ.copy()
    if trace_path is not None:
        environment["PROJECTATLAS_REVERSE_CALLER_TRACE"] = str(trace_path)
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


def require_success(measured: dict[str, Any], label: str) -> None:
    """Stop the matrix when an installation or scan setup failed."""

    if measured["returncode"] != 0:
        stderr = measured["stderr"].decode("utf-8", errors="replace")
        raise RuntimeError(f"{label} failed with {measured['returncode']}: {stderr}")


def sanitize_text(value: bytes, fixture_root: Path) -> str:
    """Redact temporary fixture paths without changing comparison bytes."""

    return value.decode("utf-8", errors="replace").replace(str(fixture_root), "<fixture>")


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


def run_summary(
    binary: Path,
    root: Path,
    target: dict[str, Any],
    shape: str,
    limit: int,
    arm: str,
    ordinal: int,
) -> dict[str, Any]:
    """Run one public summary invocation and retain trace plus raw output."""

    # Keep observer output outside the indexed fixture so writing it cannot
    # turn the following summary invocation into a ProjectAtlas refresh.
    trace_path = root.parent / f"{root.name}-trace-{arm}-{ordinal:04d}.json"
    measured = run_process(
        [
            str(binary),
            "--format",
            "json",
            "summary",
            target["path"],
            "--limit",
            str(limit),
        ],
        root,
        trace_path=trace_path,
    )
    result = serialize_process(measured, root)
    result["target"] = target["path"]
    result["limit"] = limit
    if measured["returncode"] == 0:
        try:
            payload = json.loads(measured["stdout"])
        except json.JSONDecodeError as error:
            raise AssertionError(f"summary emitted invalid JSON: {error}") from error
        assert_semantics(payload, target, shape)
        result["decoded_summary"] = payload
        if not trace_path.is_file():
            raise AssertionError(f"successful summary did not retain {trace_path}")
        result["query_observations"] = json.loads(trace_path.read_text(encoding="utf-8"))
        trace_path.unlink()
    else:
        result["query_observations"] = []
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


def aggregate_queries(runs: list[dict[str, Any]]) -> dict[str, Any]:
    """Summarize observed production query events without synthetic SQL."""

    observations = [
        observation
        for run in runs
        for observation in run["query_observations"]
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
        "allocation_proxy_bytes": sum(
            observation["row_bytes"] for observation in observations
        ),
    }


def measure_shape(
    binary: Path,
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
        ordinal = 0
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
        metrics = {
            key: statistics.median(run[key] for run in runs)
            for key in ("wall_ms", "cpu_ms", "peak_rss_bytes", "stdout_bytes")
        }
        return {
            "targets": targets,
            "setup": setup,
            "runs": runs,
            "aggregate": metrics,
            "query_observations": aggregate_queries(runs),
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


def failure_case(binary: Path, name: str, arm: str) -> dict[str, Any]:
    """Capture one negative or failure result through the same CLI boundary."""

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
        elif name == "stale-source":
            target_path = targets[0]["path"]
            (root / target_path).write_text(
                "pub fn target_0() {}\npub fn stale_added() {}\n",
                encoding="utf-8",
            )
        else:
            raise ValueError(name)
        measured = run_process(
            [
                str(binary),
                "--format",
                "json",
                "summary",
                target_path,
                "--limit",
                str(SUMMARY_LIMIT),
            ],
            root,
        )
        result = serialize_process(measured, root)
        result["case"] = name
        result["target"] = target_path
        if name == "corrupt-relation":
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
    parser.add_argument("--candidate-binary", type=Path, required=True)
    parser.add_argument("--candidate-patch", type=Path, default=ROOT / "docs/benchmarks/reverse-caller-candidate.patch")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repeats", type=int, default=3)
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error("--repeats must be positive")
    baseline_binary = validate_input(args.baseline_binary, "baseline binary")
    candidate_binary = validate_input(args.candidate_binary, "candidate binary")
    candidate_patch = validate_input(args.candidate_patch, "candidate patch")
    output = args.output.resolve()
    if not output.is_relative_to(ROOT):
        parser.error("--output must remain inside the repository")
    if sha256_file(baseline_binary) == sha256_file(candidate_binary):
        parser.error("baseline and candidate binaries must be distinct")
    if not candidate_patch.is_relative_to(ROOT):
        parser.error("--candidate-patch must remain inside the repository")
    candidate_patch_preflight = preflight_candidate_patch(candidate_patch)

    baseline: dict[str, Any] = {}
    candidate: dict[str, Any] = {}
    for shape in SHAPES:
        baseline[shape] = measure_shape(baseline_binary, shape, args.repeats, "baseline")
        candidate[shape] = measure_shape(candidate_binary, shape, args.repeats, "candidate")

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

    failures: dict[str, dict[str, Any]] = {}
    for case in ("missing-summary", "corrupt-relation", "stale-source"):
        failures[case] = {
            "baseline": failure_case(baseline_binary, case, "baseline"),
            "candidate": failure_case(candidate_binary, case, "candidate"),
        }
        semantic_findings.extend(
            f"{case}: {finding}"
            for finding in compare_raw_runs(
                failures[case]["baseline"], failures[case]["candidate"]
            )
        )

    result = {
        "schema": "projectatlas.reverse-caller-performance.v3",
        "issue": 342,
        "repository_revision": git_revision(),
        "repeats": args.repeats,
        "baseline": {
            "binary_sha256": sha256_file(baseline_binary),
            "fixtures": baseline,
        },
        "candidate": {
            "binary_sha256": sha256_file(candidate_binary),
            "candidate_patch": {
                "path": candidate_patch.relative_to(ROOT).as_posix(),
                "sha256": sha256_file(candidate_patch),
            },
            "fixtures": candidate,
        },
        "candidate_patch_preflight": candidate_patch_preflight,
        "failure_cases": failures,
        "decision": decision(baseline, candidate, semantic_findings),
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(
        json.dumps(
            {
                "output": output.relative_to(ROOT).as_posix(),
                "outcome": result["decision"]["outcome"],
                "semantic_findings": len(semantic_findings),
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
