#!/usr/bin/env python3
"""Run the preregistered three-arm Codex source-navigation benchmark."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
import re
import statistics
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

from mcp_composition import PURPOSES, clear_git_repository_environment, remove_tree
from system_scale import (
    persistent_sizes,
    prepare_huge,
    prepare_medium,
    prepare_small,
    run_measured,
)


ROOT = Path(__file__).resolve().parents[3]
DEFAULT_WORK = ROOT / "target/benchmarks/agent-navigation/current"
DEFAULT_CORPUS_CACHE = ROOT / "target/benchmarks/system-scale/corpus-cache"
ARMS = ("v0.4", "v0.3.26", "plain")
CASES = ("small-clean", "small-dirty", "small-non-git", "medium", "huge-vscode")
NON_TOOL_ITEMS = {"agent_message", "reasoning"}
READ_ONLY_MCP_TOOLS = frozenset(
    {
        "atlas_config",
        "atlas_file_summary",
        "atlas_files",
        "atlas_folders",
        "atlas_health",
        "atlas_ignore_list",
        "atlas_lint",
        "atlas_mcp_config",
        "atlas_next",
        "atlas_outline",
        "atlas_overview",
        "atlas_parity_report",
        "atlas_purpose_queue",
        "atlas_root",
        "atlas_runtime_info",
        "atlas_search",
        "atlas_session_brief",
        "atlas_settings",
        "atlas_slice",
        "atlas_symbol_relations",
        "atlas_symbols",
        "atlas_task_status",
        "atlas_token_report",
        "atlas_watch_status",
    }
)
ENVIRONMENT_KEYS = {
    "platform",
    "python",
    "logical_cpus",
    "os_name",
    "codex_version",
    "codex_sha256",
}
PATH_PLACEHOLDERS = {
    "{REPO_ROOT}": "ProjectAtlas checkout root",
    "{USER_HOME}": "current operating-system user home",
}


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def candidate_path(value: str) -> Path:
    path = Path(os.path.expandvars(value))
    if not path.is_absolute():
        path = ROOT / path
    return path.resolve(strict=True)


def redact_local_paths(value: Any) -> Any:
    if isinstance(value, dict):
        return {key: redact_local_paths(item) for key, item in value.items()}
    if isinstance(value, list):
        return [redact_local_paths(item) for item in value]
    if not isinstance(value, str):
        return value
    redacted = value
    for path, placeholder in (
        (ROOT, "{REPO_ROOT}"),
        (Path.home(), "{USER_HOME}"),
    ):
        for spelling in {str(path), str(path).replace("\\", "/")}:
            redacted = re.sub(
                re.escape(spelling), placeholder, redacted, flags=re.IGNORECASE
            )
    return redacted


def utf8_size(value: Any) -> int:
    if value is None:
        return 0
    if isinstance(value, bytes):
        return len(value)
    if isinstance(value, str):
        return len(value.encode("utf-8"))
    return len(
        json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    )


def output_bytes(item: dict[str, Any]) -> int:
    for key in ("aggregated_output", "result", "output"):
        if key in item:
            return utf8_size(item[key])
    return 0


def parse_self_audit(
    response: str, marker: str
) -> tuple[str, dict[str, Any] | None, str | None]:
    position = response.rfind(marker)
    if position < 0:
        return response, None, f"final response omitted {marker!r}"
    answer = response[:position].rstrip()
    payload = response[position + len(marker) :].lstrip()
    try:
        audit, end = json.JSONDecoder().raw_decode(payload)
    except json.JSONDecodeError as error:
        return answer, None, f"invalid self-audit JSON: {error}"
    if payload[end:].strip():
        return answer, None, "self-audit JSON was not the final response content"
    error = validate_self_audit(audit)
    return answer, audit if error is None else None, error


def validate_self_audit(audit: Any) -> str | None:
    if not isinstance(audit, dict):
        return "self-audit must be an object"
    for category in ("productive", "wrong"):
        visits = audit.get(category)
        if not isinstance(visits, dict):
            return f"self-audit {category!r} must be an object"
        for kind in ("folders", "files", "relations"):
            values = visits.get(kind)
            if not isinstance(values, list) or not all(
                isinstance(value, str) for value in values
            ):
                return f"self-audit {category}.{kind} must be a string array"
    for key in ("backtracks", "broad_reads", "full_reads"):
        value = audit.get(key)
        if not isinstance(value, int) or isinstance(value, bool) or value < 0:
            return f"self-audit {key!r} must be a non-negative integer"
    return None


def parse_trace(raw_jsonl: str, audit_marker: str) -> dict[str, Any]:
    events: list[dict[str, Any]] = []
    invalid_lines: list[dict[str, Any]] = []
    item_events: dict[str, dict[str, Any]] = {}
    anonymous_items: list[dict[str, Any]] = []
    usage: Counter[str] = Counter()
    event_types: Counter[str] = Counter()
    final_response = ""
    for line_number, line in enumerate(raw_jsonl.splitlines(), 1):
        if not line.strip():
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError as error:
            invalid_lines.append(
                {"line": line_number, "error": str(error), "text": line}
            )
            continue
        if not isinstance(event, dict):
            invalid_lines.append(
                {"line": line_number, "error": "event is not an object", "text": line}
            )
            continue
        events.append(event)
        event_type = str(event.get("type", "unknown"))
        event_types[event_type] += 1
        event_usage = event.get("usage")
        if event_type == "turn.completed" and isinstance(event_usage, dict):
            for key, value in event_usage.items():
                if isinstance(value, int) and not isinstance(value, bool):
                    usage[key] += value
        item = event.get("item")
        if event_type.startswith("item.") and isinstance(item, dict):
            item_id = item.get("id")
            if isinstance(item_id, str):
                item_events[item_id] = item
            elif event_type in {"item.completed", "item.failed"}:
                anonymous_items.append(item)
            if (
                event_type == "item.completed"
                and item.get("type") == "agent_message"
                and isinstance(item.get("text"), str)
            ):
                final_response = item["text"]

    items = [*item_events.values(), *anonymous_items]
    item_counts = Counter(str(item.get("type", "unknown")) for item in items)
    tool_items = [
        item for item in items if str(item.get("type", "unknown")) not in NON_TOOL_ITEMS
    ]
    tool_counts = Counter(str(item.get("type", "unknown")) for item in tool_items)
    mcp_calls = []
    for item in tool_items:
        if item.get("type") != "mcp_tool_call":
            continue
        mcp_calls.append(
            {
                "server": item.get("server") or item.get("server_name"),
                "tool": item.get("tool") or item.get("tool_name") or item.get("name"),
                "arguments": item.get("arguments", item.get("args")),
                "status": item.get("status"),
                "error": item.get("error"),
                "emitted_bytes": output_bytes(item),
            }
        )
    answer, self_audit, audit_error = parse_self_audit(final_response, audit_marker)
    return {
        "event_count": len(events),
        "event_types": dict(sorted(event_types.items())),
        "invalid_lines": invalid_lines,
        "item_counts": dict(sorted(item_counts.items())),
        "tool_calls_by_type": dict(sorted(tool_counts.items())),
        "mcp_calls": mcp_calls,
        "tool_emitted_bytes": sum(output_bytes(item) for item in tool_items),
        "provider_usage": dict(sorted(usage.items())),
        "final_response": final_response,
        "answer": answer,
        "self_audit": self_audit,
        "self_audit_error": audit_error,
    }


def evaluate_answer(answer: str, rubric: dict[str, Any]) -> dict[str, Any]:
    folded = answer.casefold()
    required = [str(value) for value in rubric.get("required_terms", [])]
    forbidden = [str(value) for value in rubric.get("forbidden_terms", [])]
    any_of = [[str(value) for value in group] for group in rubric.get("any_of", [])]
    missing = [value for value in required if value.casefold() not in folded]
    present_forbidden = [value for value in forbidden if value.casefold() in folded]
    missing_groups = [
        group
        for group in any_of
        if not any(value.casefold() in folded for value in group)
    ]
    return {
        "passed": not missing and not present_forbidden and not missing_groups,
        "missing_required_terms": missing,
        "present_forbidden_terms": present_forbidden,
        "missing_any_of_groups": missing_groups,
    }


def projectatlas_mcp_contract(trace: dict[str, Any], arm_name: str) -> dict[str, Any]:
    calls = [
        call for call in trace["mcp_calls"] if call.get("server") == "projectatlas"
    ]
    successful = sum(call.get("status") == "completed" for call in calls)
    return {
        "expected": arm_name != "plain",
        "observed_calls": len(calls),
        "successful_calls": successful,
        "passed": not calls if arm_name == "plain" else successful > 0,
    }


def toml_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def build_command(
    candidate: dict[str, Any],
    arm_name: str,
    fixture: Path,
    task_prompt: str,
) -> tuple[list[str], str]:
    arm = candidate["arms"][arm_name]
    common = candidate["codex"]
    prompt = task_prompt
    skill_path = arm.get("skill_path")
    if skill_path:
        prompt = (
            f"{task_prompt}\n\n"
            "Before navigating, read and follow the complete packaged ProjectAtlas "
            f"skill at this exact path: {candidate_path(skill_path)}\n"
            "Use the configured projectatlas MCP server as that skill directs."
        )
    else:
        prompt = (
            f"{task_prompt}\n\n"
            "Control arm: ProjectAtlas, its skill, plugin, CLI, runtime, and MCP "
            "server are unavailable and must not be invoked. Navigate with ordinary "
            "read-only source tools."
        )
    arguments = [
        str(candidate_path(common["executable"])),
        "exec",
        "--json",
        "--ephemeral",
        "--ignore-user-config",
        "--ignore-rules",
        "--skip-git-repo-check",
        "--color",
        "never",
        "--sandbox",
        str(common["sandbox"]),
        "--cd",
        str(fixture.resolve()),
        "--model",
        str(common["model"]),
        "-c",
        f"model_reasoning_effort={toml_string(str(common['reasoning_effort']))}",
        "-c",
        f"approval_policy={toml_string(str(common['approval_policy']))}",
    ]
    for key, value in sorted(common.get("config", {}).items()):
        arguments.extend(["-c", f"{key}={value}"])
    if arm_name != "plain":
        runtime = candidate_path(arm["runtime"])
        replacements = {
            "fixture": str(fixture.resolve()),
            "db": str((fixture / ".projectatlas/projectatlas.db").resolve()),
            "config": str((fixture / ".projectatlas/config.toml").resolve()),
        }
        mcp_arguments = [
            str(value).format_map(replacements) for value in arm["mcp_args"]
        ]
        arguments.extend(
            [
                "-c",
                f"mcp_servers.projectatlas.command={toml_string(str(runtime))}",
                "-c",
                f"mcp_servers.projectatlas.args={json.dumps(mcp_arguments)}",
                "-c",
                "mcp_servers.projectatlas.required=true",
                "-c",
                "mcp_servers.projectatlas.default_tools_approval_mode="
                f"{toml_string(str(common['mcp_approval']['default_mode']))}",
                "-c",
                f"mcp_servers.projectatlas.cwd={toml_string(str(fixture.resolve()))}",
            ]
        )
        for tool_name in common["mcp_approval"]["read_only_tools"]:
            if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", tool_name) is None:
                raise ValueError(f"invalid approved MCP tool name: {tool_name!r}")
            arguments.extend(
                [
                    "-c",
                    f"mcp_servers.projectatlas.tools.{tool_name}.approval_mode="
                    f"{toml_string('approve')}",
                ]
            )
        for key, value in sorted(arm.get("mcp_env", {}).items()):
            if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", key) is None:
                raise ValueError(f"invalid MCP environment name: {key!r}")
            arguments.extend(
                [
                    "-c",
                    f"mcp_servers.projectatlas.env.{key}={toml_string(str(value))}",
                ]
            )
    arguments.append(prompt)
    return arguments, prompt


def source_state(root: Path) -> dict[str, Any]:
    digest = hashlib.sha256()
    if (root / ".git").exists():
        status = subprocess.check_output(
            ["git", "status", "--porcelain=v1", "-z", "--untracked-files=all"],
            cwd=root,
        )
        diff = subprocess.check_output(
            ["git", "diff", "--binary", "--no-ext-diff", "HEAD", "--"], cwd=root
        )
        untracked = subprocess.check_output(
            ["git", "ls-files", "--others", "--exclude-standard", "-z"], cwd=root
        )
        digest.update(status)
        digest.update(diff)
        for name in sorted(filter(None, untracked.split(b"\0"))):
            path = root / os.fsdecode(name)
            digest.update(name)
            if path.is_file():
                digest.update(path.read_bytes())
        return {
            "kind": "git-worktree",
            "sha256": digest.hexdigest(),
            "status": status.decode("utf-8", errors="replace").replace("\0", "\n"),
        }
    files = []
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root)
        if not path.is_file() or any(
            part in {".git", ".projectatlas"} for part in relative.parts
        ):
            continue
        normalized = relative.as_posix()
        digest.update(normalized.encode("utf-8"))
        digest.update(path.read_bytes())
        files.append(normalized)
    return {
        "kind": "file-manifest",
        "sha256": digest.hexdigest(),
        "files": files,
    }


def prepare_case(
    case: str,
    run_root: Path,
    preregistration: dict[str, Any],
    corpus_cache: Path,
) -> Path:
    if case.startswith("small-"):
        return prepare_small(run_root)[case.removeprefix("small-")]
    fixture = run_root / case
    if case == "medium":
        prepare_medium(
            fixture,
            int(preregistration["corpora"]["medium"]["caller_files"]),
        )
    elif case == "huge-vscode":
        prepare_huge(
            fixture,
            preregistration["corpora"]["huge"],
            corpus_cache,
        )
    else:
        raise ValueError(f"unknown benchmark case: {case}")
    return fixture


def combine_setup_measurements(
    measurements: list[dict[str, Any]], fixture: Path
) -> dict[str, Any]:
    storage = persistent_sizes(fixture)
    storage_keys = (
        "database_bytes",
        "wal_bytes",
        "shm_bytes",
        "staging_bytes",
        "stage_directories",
    )
    peak_storage = {
        key: max(
            [int(storage.get(key, 0))]
            + [int(row.get("peak_storage", {}).get(key, 0)) for row in measurements]
        )
        for key in storage_keys
    }
    return {
        "passed": all(
            row.get("returncode") == 0 and not row.get("timed_out")
            for row in measurements
        ),
        "commands": measurements,
        "wall_seconds": sum(
            float(row.get("wall_seconds", 0.0)) for row in measurements
        ),
        "cpu_seconds": sum(float(row.get("cpu_seconds", 0.0)) for row in measurements),
        "peak_rss_bytes": max(
            (int(row.get("peak_rss_bytes", 0)) for row in measurements), default=0
        ),
        "process_read_transfer_bytes": sum(
            int(row.get("process_read_transfer_bytes", 0)) for row in measurements
        ),
        "process_write_transfer_bytes": sum(
            int(row.get("process_write_transfer_bytes", 0)) for row in measurements
        ),
        "peak_storage": peak_storage,
        "persistent_storage": storage,
    }


def prepare_projectatlas_arm(
    preregistration: dict[str, Any],
    arm_name: str,
    case: str,
    fixture: Path,
) -> dict[str, Any]:
    arm = preregistration["candidate"]["arms"][arm_name]
    if arm_name == "plain":
        return combine_setup_measurements([], fixture)
    candidate = preregistration["candidate"]
    runtime = str(candidate_path(arm["runtime"]))
    environment = os.environ.copy()
    environment.update(
        {
            str(key): str(value)
            for key, value in candidate["codex"].get("environment", {}).items()
        }
    )
    environment.update(
        {str(key): str(value) for key, value in arm.get("mcp_env", {}).items()}
    )
    environment["PROJECTATLAS_NO_TELEMETRY"] = "1"
    timeout_seconds = float(candidate["setup_timeout_seconds"])
    commands = [
        [
            runtime,
            "--require-version",
            str(arm["require_version"]),
            "init",
            "--force-rescan",
        ]
    ]
    fixture_name = case.removeprefix("small-") if case.startswith("small-") else None
    if fixture_name is not None:
        commands.extend(
            [
                [
                    runtime,
                    "--require-version",
                    str(arm["require_version"]),
                    "purpose",
                    "set",
                    path,
                    purpose,
                ]
                for path, purpose in PURPOSES[fixture_name].items()
            ]
        )
    measurements = []
    for command in commands:
        measurement = run_measured(
            command,
            cwd=fixture,
            env=environment,
            timeout_seconds=timeout_seconds,
        )
        measurement["command"] = command
        measurements.append(measurement)
        if measurement["returncode"] != 0 or measurement["timed_out"]:
            break
    return combine_setup_measurements(measurements, fixture)


def navigation_context(trace: dict[str, Any], arm: dict[str, Any]) -> dict[str, Any]:
    skill_bytes = Path(arm["skill_path"]).stat().st_size if arm.get("skill_path") else 0
    discovery_bytes = int(arm.get("tool_discovery_bytes", 0))
    gross_bytes = int(trace["tool_emitted_bytes"])
    setup_bytes = skill_bytes + discovery_bytes
    net_bytes = gross_bytes + setup_bytes
    return {
        "accounting": "observed-tool-output-plus-preregistered-discovery-and-exact-skill",
        "token_estimate": "ceil(utf8-bytes/4), separate from provider counters",
        "gross_navigation_bytes": gross_bytes,
        "gross_navigation_tokens": math.ceil(gross_bytes / 4),
        "skill_bytes": skill_bytes,
        "tool_discovery_bytes": discovery_bytes,
        "setup_bytes": setup_bytes,
        "net_navigation_bytes": net_bytes,
        "net_navigation_tokens": math.ceil(net_bytes / 4),
    }


def schedule(repeats: int) -> list[dict[str, Any]]:
    rows = []
    for repeat in range(repeats):
        rotated = ARMS[repeat % len(ARMS) :] + ARMS[: repeat % len(ARMS)]
        for case in CASES:
            for arm in rotated:
                rows.append(
                    {
                        "run_id": f"r{repeat + 1:02d}-{case}-{arm}",
                        "repeat": repeat + 1,
                        "case": case,
                        "arm": arm,
                    }
                )
    return rows


def numeric_distribution(values: list[int | float]) -> dict[str, Any]:
    numbers = [float(value) for value in values]
    return {
        "count": len(numbers),
        "values": values,
        "median": statistics.median(numbers),
        "observed_tail": "maximum",
        "maximum": max(numbers),
    }


def aggregate_runs(runs: list[dict[str, Any]]) -> dict[str, Any]:
    grouped: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for run in runs:
        grouped[(run["case"], run["arm"])].append(run)
    groups: dict[str, Any] = {}
    metric_paths = {
        "wall_seconds": ("measurement", "wall_seconds"),
        "cpu_seconds": ("measurement", "cpu_seconds"),
        "peak_rss_bytes": ("measurement", "peak_rss_bytes"),
        "process_read_transfer_bytes": (
            "measurement",
            "process_read_transfer_bytes",
        ),
        "process_write_transfer_bytes": (
            "measurement",
            "process_write_transfer_bytes",
        ),
        "gross_navigation_bytes": (
            "navigation_context",
            "gross_navigation_bytes",
        ),
        "net_navigation_bytes": ("navigation_context", "net_navigation_bytes"),
        "gross_navigation_tokens": (
            "navigation_context",
            "gross_navigation_tokens",
        ),
        "net_navigation_tokens": ("navigation_context", "net_navigation_tokens"),
        "setup_wall_seconds": ("economics", "setup_wall_seconds"),
        "setup_cpu_seconds": ("economics", "setup_cpu_seconds"),
        "setup_peak_rss_bytes": ("economics", "setup_peak_rss_bytes"),
        "setup_persistent_bytes": ("economics", "setup_persistent_bytes"),
        "cold_wall_seconds": ("economics", "cold_wall_seconds"),
        "cold_cpu_seconds": ("economics", "cold_cpu_seconds"),
        "cold_peak_rss_bytes": ("economics", "cold_peak_rss_bytes"),
        "cold_read_transfer_bytes": ("economics", "cold_read_transfer_bytes"),
        "cold_write_transfer_bytes": ("economics", "cold_write_transfer_bytes"),
        "cold_peak_storage_bytes": ("economics", "cold_peak_storage_bytes"),
        "post_trial_persistent_bytes": (
            "economics",
            "post_trial_persistent_bytes",
        ),
    }
    for (case, arm), rows in sorted(grouped.items()):
        distributions = {}
        for name, (section, key) in metric_paths.items():
            values = [
                row[section][key]
                for row in rows
                if isinstance(row.get(section, {}).get(key), (int, float))
                and not isinstance(row[section][key], bool)
            ]
            if values:
                distributions[name] = numeric_distribution(values)
        usage_keys = sorted(
            {
                key
                for row in rows
                for key in row.get("trace", {}).get("provider_usage", {})
            }
        )
        provider_usage = {}
        for key in usage_keys:
            values = [
                row["trace"]["provider_usage"][key]
                for row in rows
                if key in row.get("trace", {}).get("provider_usage", {})
            ]
            provider_usage[key] = numeric_distribution(values)
        tool_calls: Counter[str] = Counter()
        for row in rows:
            tool_calls.update(row.get("trace", {}).get("tool_calls_by_type", {}))
        audit_metrics: dict[str, list[int]] = defaultdict(list)
        for row in rows:
            trace = row.get("trace", {})
            audit = trace.get("self_audit")
            if not isinstance(audit, dict):
                continue
            for category in ("productive", "wrong"):
                for kind in ("folders", "files", "relations"):
                    audit_metrics[f"{category}_{kind}"].append(
                        len(audit[category][kind])
                    )
            for key in ("backtracks", "broad_reads", "full_reads"):
                audit_metrics[key].append(audit[key])
            audit_metrics["mcp_calls"].append(len(trace.get("mcp_calls", [])))
            audit_metrics["tool_calls"].append(
                sum(trace.get("tool_calls_by_type", {}).values())
            )
        for name, values in audit_metrics.items():
            distributions[name] = numeric_distribution(values)
        groups[f"{case}/{arm}"] = {
            "run_ids": [row["run_id"] for row in rows],
            "scheduled": len(rows),
            "completed": sum(
                row.get("execution_status") == "completed" for row in rows
            ),
            "failed": sum(row.get("execution_status") == "failed" for row in rows),
            "excluded": sum(bool(row.get("excluded")) for row in rows),
            "correct": sum(
                bool(row.get("correctness", {}).get("passed")) for row in rows
            ),
            "distributions": distributions,
            "provider_usage": provider_usage,
            "tool_calls_by_type": dict(sorted(tool_calls.items())),
        }
    comparisons: dict[str, Any] = {}
    for case in CASES:
        candidate_group = groups.get(f"{case}/v0.4")
        if candidate_group is None:
            continue
        for baseline in ("v0.3.26", "plain"):
            baseline_group = groups.get(f"{case}/{baseline}")
            if baseline_group is None:
                continue
            savings = {}
            common_metrics = set(candidate_group["distributions"]) & set(
                baseline_group["distributions"]
            )
            for metric in sorted(common_metrics):
                candidate_metric = candidate_group["distributions"][metric]
                baseline_metric = baseline_group["distributions"][metric]
                savings[metric] = {
                    "candidate_median": candidate_metric["median"],
                    "baseline_median": baseline_metric["median"],
                    "median_percent_saving": percent_saving(
                        candidate_metric["median"], baseline_metric["median"]
                    ),
                    "tail_statistic": "observed maximum",
                    "candidate_tail": candidate_metric["maximum"],
                    "baseline_tail": baseline_metric["maximum"],
                    "tail_percent_saving": percent_saving(
                        candidate_metric["maximum"], baseline_metric["maximum"]
                    ),
                }
            provider = {}
            common_usage = set(candidate_group["provider_usage"]) & set(
                baseline_group["provider_usage"]
            )
            for metric in sorted(common_usage):
                candidate_metric = candidate_group["provider_usage"][metric]
                baseline_metric = baseline_group["provider_usage"][metric]
                provider[metric] = {
                    "candidate_median": candidate_metric["median"],
                    "baseline_median": baseline_metric["median"],
                    "median_percent_difference": percent_saving(
                        candidate_metric["median"], baseline_metric["median"]
                    ),
                    "tail_statistic": "observed maximum",
                    "candidate_tail": candidate_metric["maximum"],
                    "baseline_tail": baseline_metric["maximum"],
                    "tail_percent_difference": percent_saving(
                        candidate_metric["maximum"], baseline_metric["maximum"]
                    ),
                    "causal_attribution": False,
                }
            candidate_warm = candidate_group["distributions"].get("wall_seconds")
            baseline_warm = baseline_group["distributions"].get("wall_seconds")
            candidate_setup = candidate_group["distributions"].get("setup_wall_seconds")
            baseline_setup = baseline_group["distributions"].get("setup_wall_seconds")
            break_even = None
            if candidate_warm and baseline_warm and candidate_setup and baseline_setup:
                warm_saving = baseline_warm["median"] - candidate_warm["median"]
                incremental_setup = candidate_setup["median"] - baseline_setup["median"]
                if warm_saving > 0:
                    break_even = (
                        0
                        if incremental_setup <= 0
                        else math.ceil(incremental_setup / warm_saving)
                    )
            comparisons[f"{case}/v0.4-vs-{baseline}"] = {
                "lower_is_better_percent_savings": savings,
                "provider_usage_descriptive_only": provider,
                "wall_time_break_even_tasks": break_even,
            }
    return {
        "all_run_ids": [run["run_id"] for run in runs],
        "scheduled": len(runs),
        "completed": sum(run.get("execution_status") == "completed" for run in runs),
        "failed": sum(run.get("execution_status") == "failed" for run in runs),
        "excluded": sum(bool(run.get("excluded")) for run in runs),
        "groups": groups,
        "comparisons": comparisons,
        "provider_usage_note": (
            "Provider counters are reported separately and are not attributed "
            "causally to navigation."
        ),
    }


def percent_saving(candidate: float, baseline: float) -> float | None:
    if baseline == 0:
        return None
    return round((baseline - candidate) / baseline * 100, 6)


def actual_environment(candidate: dict[str, Any]) -> dict[str, Any]:
    executable_path = candidate_path(candidate["codex"]["executable"])
    executable = str(executable_path)
    version = subprocess.check_output(
        [executable, "--version"], text=True, timeout=30
    ).strip()
    return {
        "platform": platform.platform(),
        "python": platform.python_version(),
        "logical_cpus": os.cpu_count(),
        "os_name": os.name,
        "codex_version": version,
        "codex_sha256": file_sha256(executable_path),
    }


def validate_preregistration(preregistration: dict[str, Any]) -> dict[str, Any]:
    if preregistration["status"] != "locked_for_final_measurement":
        raise ValueError("preregistration is not locked_for_final_measurement")
    if set(preregistration["candidate"]["arms"]) != set(ARMS):
        raise ValueError(f"candidate arms must be exactly {ARMS}")
    if set(preregistration["prompts"]["cases"]) != set(CASES):
        raise ValueError(f"prompt cases must be exactly {CASES}")
    if set(preregistration["rubric"]["cases"]) != set(CASES):
        raise ValueError(f"rubric cases must be exactly {CASES}")
    candidate = preregistration["candidate"]
    common = candidate["codex"]
    codex_path = candidate_path(common["executable"])
    if file_sha256(codex_path) != common["sha256"]:
        raise ValueError("Codex executable SHA-256 does not match preregistration")
    protocol = preregistration["protocol"]
    if protocol["tail_statistic"] != "observed_maximum":
        raise ValueError("three-repeat publication must use observed_maximum tail")
    if int(protocol["repeats"]) < 3:
        raise ValueError("publication requires at least three repeats")
    if common["sandbox"] != "read-only" or common["approval_policy"] != "never":
        raise ValueError(
            "publication trials require read-only sandbox and never approval"
        )
    mcp_approval = common["mcp_approval"]
    read_only_tools = mcp_approval["read_only_tools"]
    if mcp_approval["default_mode"] != "prompt":
        raise ValueError("unlisted ProjectAtlas MCP tools must retain prompt approval")
    if not read_only_tools or len(set(read_only_tools)) != len(read_only_tools):
        raise ValueError(
            "approved ProjectAtlas MCP read-only tools must be nonempty and unique"
        )
    approved_tools = frozenset(map(str, read_only_tools))
    if approved_tools != READ_ONLY_MCP_TOOLS:
        raise ValueError(
            "approved ProjectAtlas MCP tools must match the locked read-only inventory: "
            f"missing={sorted(READ_ONLY_MCP_TOOLS - approved_tools)}, "
            f"unexpected={sorted(approved_tools - READ_ONLY_MCP_TOOLS)}"
        )
    reserved_config = (
        "approval_policy",
        "sandbox_mode",
        "model",
        "model_reasoning_effort",
        "mcp_servers",
        "plugins",
        "skills",
    )
    conflicting_config = [
        key
        for key in common.get("config", {})
        if any(
            key == reserved or key.startswith(f"{reserved}.")
            for reserved in reserved_config
        )
    ]
    if conflicting_config:
        raise ValueError(
            "common Codex config must not override controlled model, permission, "
            f"skill, or MCP fields: {conflicting_config}"
        )
    identities = {}
    for arm_name in ARMS:
        arm = candidate["arms"][arm_name]
        if arm_name == "plain":
            if arm.get("runtime") or arm.get("skill_path") or arm.get("mcp_args"):
                raise ValueError(
                    "plain arm must not configure ProjectAtlas runtime or skill"
                )
            identities[arm_name] = {"projectatlas": False}
            continue
        runtime = candidate_path(arm["runtime"])
        skill = candidate_path(arm["skill_path"])
        runtime_sha256 = file_sha256(runtime)
        skill_sha256 = file_sha256(skill)
        if runtime_sha256 != arm["runtime_sha256"]:
            raise ValueError(f"{arm_name} runtime SHA-256 does not match candidate")
        if skill_sha256 != arm["skill_sha256"]:
            raise ValueError(f"{arm_name} skill SHA-256 does not match candidate")
        version = subprocess.check_output(
            [runtime, "--version"], text=True, timeout=30
        ).strip()
        if version != arm["version"]:
            raise ValueError(
                f"{arm_name} runtime version {version!r} != {arm['version']!r}"
            )
        expected_release = "0.4.0" if arm_name == "v0.4" else "0.3.26"
        if arm["require_version"] != expected_release:
            raise ValueError(f"{arm_name} require_version must be {expected_release}")
        if re.search(rf"(?<!\d){re.escape(expected_release)}(?!\d)", version) is None:
            raise ValueError(
                f"{arm_name} must identify ProjectAtlas {expected_release}, got {version!r}"
            )
        identities[arm_name] = {
            "projectatlas": True,
            "runtime": str(runtime),
            "runtime_sha256": runtime_sha256,
            "version": version,
            "skill_path": str(skill),
            "skill_sha256": skill_sha256,
            "skill_bytes": skill.stat().st_size,
            "tool_discovery_bytes": int(arm.get("tool_discovery_bytes", 0)),
        }
    return identities


def trial_economics(
    setup: dict[str, Any], measurement: dict[str, Any], fixture: Path
) -> dict[str, Any]:
    post_trial_storage = persistent_sizes(fixture)
    setup_storage = int(setup["persistent_storage"]["total_bytes"])
    agent_peak_storage = sum(
        int(measurement.get("peak_storage", {}).get(key, 0))
        for key in ("database_bytes", "wal_bytes", "shm_bytes", "staging_bytes")
    )
    return {
        "setup_wall_seconds": float(setup["wall_seconds"]),
        "setup_cpu_seconds": float(setup["cpu_seconds"]),
        "setup_peak_rss_bytes": int(setup["peak_rss_bytes"]),
        "setup_persistent_bytes": setup_storage,
        "cold_wall_seconds": float(setup["wall_seconds"])
        + float(measurement["wall_seconds"]),
        "cold_cpu_seconds": float(setup["cpu_seconds"])
        + float(measurement["cpu_seconds"]),
        "cold_peak_rss_bytes": max(
            int(setup["peak_rss_bytes"]), int(measurement["peak_rss_bytes"])
        ),
        "cold_read_transfer_bytes": int(setup["process_read_transfer_bytes"])
        + int(measurement["process_read_transfer_bytes"]),
        "cold_write_transfer_bytes": int(setup["process_write_transfer_bytes"])
        + int(measurement["process_write_transfer_bytes"]),
        "cold_peak_storage_bytes": max(setup_storage, agent_peak_storage),
        "post_trial_persistent_bytes": int(post_trial_storage["total_bytes"]),
    }


def run_trial(
    row: dict[str, Any],
    preregistration: dict[str, Any],
    work_root: Path,
    corpus_cache: Path,
) -> dict[str, Any]:
    run_root = work_root / row["run_id"]
    fixture = prepare_case(row["case"], run_root, preregistration, corpus_cache)
    source_before_setup = source_state(fixture)
    candidate = preregistration["candidate"]
    setup = prepare_projectatlas_arm(preregistration, row["arm"], row["case"], fixture)
    source_after_setup = source_state(fixture)
    setup_source_unchanged = (
        source_before_setup["sha256"] == source_after_setup["sha256"]
    )
    if not setup["passed"] or not setup_source_unchanged:
        return {
            **row,
            "excluded": False,
            "execution_status": "failed",
            "failure": {
                "type": "setup_failed",
                "message": (
                    "ProjectAtlas setup failed"
                    if not setup["passed"]
                    else "setup mutated benchmark source"
                ),
            },
            "fixture": {
                "path": str(fixture),
                "setup_source_state": {
                    "passed": setup_source_unchanged,
                    "before": source_before_setup,
                    "after": source_after_setup,
                },
            },
            "setup": setup,
        }
    before = source_after_setup
    prompt = preregistration["prompts"]["cases"][row["case"]]
    audit_instruction = preregistration["prompts"]["self_audit_instruction"]
    task_prompt = f"{prompt}\n\n{audit_instruction}"
    arguments, effective_prompt = build_command(
        candidate, row["arm"], fixture, task_prompt
    )
    environment = os.environ.copy()
    environment.update(
        {
            str(key): str(value)
            for key, value in candidate["codex"].get("environment", {}).items()
        }
    )
    environment["PROJECTATLAS_NO_TELEMETRY"] = "1"
    measurement = run_measured(
        arguments,
        cwd=fixture,
        env=environment,
        timeout_seconds=float(candidate["codex"]["timeout_seconds"]),
    )
    after = source_state(fixture)
    marker = preregistration["prompts"]["self_audit_marker"]
    raw_jsonl = measurement.pop("stdout")
    raw_stderr = measurement.pop("stderr")
    trace = parse_trace(raw_jsonl, marker)
    mcp_contract = projectatlas_mcp_contract(trace, row["arm"])
    correctness = evaluate_answer(
        trace["answer"], preregistration["rubric"]["cases"][row["case"]]
    )
    mutation = {
        "passed": before["sha256"] == after["sha256"],
        "before": before,
        "after": after,
    }
    execution_status = (
        "completed"
        if measurement["returncode"] == 0
        and not measurement["timed_out"]
        and not trace["invalid_lines"]
        and trace["final_response"]
        and trace["self_audit_error"] is None
        and mcp_contract["passed"]
        and mutation["passed"]
        else "failed"
    )
    economics = trial_economics(setup, measurement, fixture)
    return {
        **row,
        "excluded": False,
        "execution_status": execution_status,
        "task_prompt": prompt,
        "effective_prompt": effective_prompt,
        "command": arguments,
        "cache_policy": candidate["codex"]["cache_policy"],
        "fixture": {
            "path": str(fixture),
            "setup_source_state": {
                "passed": True,
                "before": source_before_setup,
                "after": source_after_setup,
            },
            "source_state": mutation,
        },
        "setup": setup,
        "measurement": measurement,
        "economics": economics,
        "raw_jsonl": raw_jsonl,
        "raw_stderr": raw_stderr,
        "trace": trace,
        "projectatlas_mcp_contract": mcp_contract,
        "correctness": correctness,
        "navigation_context": navigation_context(trace, candidate["arms"][row["arm"]]),
    }


def safe_child(path: Path, parent: Path, label: str) -> Path:
    resolved = path.resolve()
    allowed = parent.resolve()
    if resolved == allowed or allowed not in resolved.parents:
        raise ValueError(f"{label} must be a child of {allowed}")
    return resolved


def write_result(result: dict[str, Any], output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(f"{output.name}.tmp")
    temporary.write_text(
        json.dumps(redact_local_paths(result), indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    temporary.replace(output)
    print(output)


def append_checkpoint(record: dict[str, Any], journal: Path) -> None:
    journal.parent.mkdir(parents=True, exist_ok=True)
    with journal.open("a", encoding="utf-8", newline="\n") as stream:
        stream.write(
            json.dumps(
                redact_local_paths(record),
                ensure_ascii=False,
                separators=(",", ":"),
            )
        )
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())


def run_benchmark(args: argparse.Namespace) -> dict[str, Any]:
    clear_git_repository_environment()
    preregistration_path = args.preregistration.resolve(strict=True)
    preregistration = json.loads(preregistration_path.read_text(encoding="utf-8"))
    if not isinstance(preregistration, dict):
        raise ValueError("preregistration must be a JSON object")
    expected_repeats = int(preregistration["protocol"]["repeats"])
    if args.repeats != expected_repeats:
        raise ValueError(
            f"--repeats {args.repeats} does not match preregistered {expected_repeats}"
        )
    identities = validate_preregistration(preregistration)
    environment = actual_environment(preregistration["candidate"])
    expected_environment = preregistration["environment"]["expected"]
    mismatches = {
        key: {"expected": value, "actual": environment.get(key)}
        for key, value in expected_environment.items()
        if key in ENVIRONMENT_KEYS and environment.get(key) != value
    }
    if mismatches:
        raise ValueError(f"environment does not match preregistration: {mismatches}")
    allowed = ROOT / "target/benchmarks/agent-navigation"
    work_root = safe_child(args.work_root, allowed, "--work-root")
    corpus_cache = args.corpus_cache.resolve()
    system_scale_root = (ROOT / "target/benchmarks/system-scale").resolve()
    if (
        corpus_cache != system_scale_root
        and system_scale_root not in corpus_cache.parents
    ):
        raise ValueError(
            f"--corpus-cache must be {system_scale_root} or one of its children"
        )
    if args.output.resolve() == work_root or work_root in args.output.resolve().parents:
        raise ValueError("--output must not be inside --work-root")
    output = args.output.resolve()
    journal = output.with_name(f"{output.name}.journal.jsonl")
    if output.exists() or journal.exists():
        raise ValueError(
            f"refusing to overwrite retained benchmark state: {output} or {journal}"
        )
    if work_root.exists():
        remove_tree(work_root)
    work_root.mkdir(parents=True)
    planned = schedule(args.repeats)
    # ponytail: raw traces stay in memory for one atomic result; checkpoint if
    # preregistered repeats or Codex output bounds grow beyond the default matrix.
    runs = []
    for row in planned:
        record: dict[str, Any]
        try:
            record = run_trial(row, preregistration, work_root, corpus_cache)
        except Exception as error:
            record = {
                **row,
                "excluded": False,
                "execution_status": "failed",
                "failure": {
                    "type": type(error).__name__,
                    "message": str(error),
                },
            }
        finally:
            run_root = work_root / row["run_id"]
            if run_root.exists():
                try:
                    remove_tree(run_root)
                except Exception as error:
                    record["execution_status"] = "failed"
                    record["cleanup_failure"] = {
                        "type": type(error).__name__,
                        "message": str(error),
                    }
        runs.append(record)
        append_checkpoint(record, journal)
    journal_sha256 = file_sha256(journal)
    result = {
        "schema_version": 1,
        "preregistration": str(preregistration_path),
        "preregistration_sha256": file_sha256(preregistration_path),
        "effective_preregistration": preregistration,
        "repeat_count": args.repeats,
        "schedule": planned,
        "candidate_identities": identities,
        "environment": environment,
        "path_placeholders": PATH_PLACEHOLDERS,
        "rerun_command": [
            sys.executable,
            str(Path(__file__).resolve()),
            "--preregistration",
            str(preregistration_path),
            "--output",
            str(args.output.resolve()),
            "--work-root",
            str(work_root),
            "--corpus-cache",
            str(corpus_cache),
            "--repeats",
            str(args.repeats),
        ],
        "runs": runs,
        "aggregate": aggregate_runs(runs),
        "checkpoint_journal_sha256": journal_sha256,
    }
    result["all_scheduled_runs_retained"] = [row["run_id"] for row in planned] == [
        row["run_id"] for row in runs
    ]
    result["checkpoint_journal_retained"] = False
    return result


def positive_integer(value: str) -> int:
    parsed = int(value)
    if parsed < 1:
        raise argparse.ArgumentTypeError("must be at least 1")
    return parsed


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--preregistration", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--work-root", type=Path, default=DEFAULT_WORK)
    parser.add_argument("--corpus-cache", type=Path, default=DEFAULT_CORPUS_CACHE)
    parser.add_argument("--repeats", type=positive_integer, default=3)
    args = parser.parse_args()
    if args.output.resolve().exists():
        raise SystemExit(
            f"refusing to overwrite existing output: {args.output.resolve()}"
        )
    try:
        result = run_benchmark(args)
    except Exception as error:
        result = {
            "schema_version": 1,
            "preregistration": str(args.preregistration.resolve()),
            "repeat_count": args.repeats,
            "all_scheduled_runs_retained": False,
            "failure": {
                "type": type(error).__name__,
                "message": str(error),
            },
        }
    output = args.output.resolve()
    write_result(result, output)
    journal = output.with_name(f"{output.name}.journal.jsonl")
    if result.get("all_scheduled_runs_retained") and journal.exists():
        journal.unlink()
    if not result.get("all_scheduled_runs_retained"):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
