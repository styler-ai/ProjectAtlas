#!/usr/bin/env python3
"""Attribute native navigation RSS after the installed campaign, without evaluating targets.

Reuse its exact packages and completed navigation fixtures. Keep native program
high-water counters separate from the original 5 ms sampled process-tree RSS.
"""

import argparse
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import time

from indexing_acceptance import SOURCE_FILE, digest, state
from mcp_composition import spawn_owned_process, terminate_owned_process


DIAGNOSTIC_BUDGET_SECONDS = 180


def native_peak(path: Path, require_positive: bool = True) -> int:
    """Read GNU KiB or current Darwin byte counters, retaining the raw record."""
    if not path.is_file() or path.stat().st_size > 16384:
        raise ValueError("Native resource output is missing or oversized")
    text = path.read_text(encoding="utf-8")
    if sys.platform == "linux":
        value = text.strip()
        if not value.isascii() or not value.isdecimal():
            raise ValueError("GNU time did not report numeric maximum RSS")
        result = int(value) * 1024
    else:
        matches = re.findall(r"^\s*(\d+)\s+maximum resident set size\s*$", text, re.MULTILINE)
        if len(matches) != 1:
            raise ValueError("Darwin time did not report exactly one maximum RSS")
        result = int(matches[0])
    if require_positive and result <= 0:
        raise ValueError("Native program RSS is not positive")
    return result


def observe(command: list[str], root: Path, env: dict, output: Path, name: str, deadline: float, require_positive: bool = True) -> dict:
    """Run one owned process group, preserving output and bounded timeout cleanup."""
    remaining = deadline - time.perf_counter()
    if remaining <= 0:
        raise TimeoutError("Supplemental diagnostic exhausted its total budget")
    counter = output / (name + ".native-time")
    stdout, stderr = output / (name + ".stdout"), output / (name + ".stderr")
    if counter.exists():
        raise FileExistsError(counter)
    options = ["-f", "%M"] if sys.platform == "linux" else ["-l"]
    started = time.perf_counter()
    with stdout.open("xb") as out, stderr.open("xb") as err:
        process, job = spawn_owned_process(["/usr/bin/time", *options, "-o", str(counter), *command],
                                           cwd=root, env=env, stdout=out, stderr=err)
        try:
            process.communicate(timeout=min(120, remaining))
        except BaseException:
            terminate_owned_process(process, job)
            raise
        finally:
            if job is not None:
                job.close()
    if process.returncode or stdout.stat().st_size > 65536:
        raise RuntimeError(f"Native observation failed: exit={process.returncode}")
    return {"native_program_peak_rss_bytes": native_peak(counter, require_positive),
            "native_counter_sha256": digest(counter), "output_sha256": digest(stdout),
            "output_bytes": stdout.stat().st_size,
            "diagnostic_wall_seconds": time.perf_counter() - started}


def main() -> None:
    deadline = time.perf_counter() + DIAGNOSTIC_BUDGET_SECONDS
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--campaign-root", type=Path, required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    args = parser.parse_args()
    if sys.platform not in {"linux", "darwin"}:
        raise RuntimeError("Native resource attribution supports Linux and Darwin only")
    campaign = args.campaign_root.resolve(strict=True)
    original = campaign / "results.json"
    original_hash = digest(original)
    source = json.loads(original.read_text(encoding="utf-8"))
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True, timeout=30).strip()
    if source["source_revision"] != revision:
        raise ValueError("Original campaign belongs to a different source revision")
    if source["machine"] != platform.machine():
        raise ValueError("Original campaign belongs to a different native architecture")
    if os.environ.get("GITHUB_ACTIONS") == "true" and (
        source.get("workflow_run_id") != os.environ.get("GITHUB_RUN_ID")
        or source.get("candidate_origin") != "installed release workflow"
    ):
        raise ValueError("Original campaign is not from this installed workflow run")
    binaries = {"baseline": args.baseline.resolve(strict=True), "candidate": args.candidate.resolve(strict=True)}
    if any(digest(binary) != source["runtimes"][name]["sha256"] for name, binary in binaries.items()):
        raise ValueError("Executable differs from the original packaged campaign")
    for name in binaries:
        if sum(row["runtime"] == name and row["case"] == "detailed" for row in source["samples"]) != 9:
            raise ValueError("Original detailed-navigation samples are incomplete")
    roots = {name: (campaign / "navigation" / name).resolve(strict=True) for name in binaries}
    preserved = {name: state(root) for name, root in roots.items()}
    output = args.output_root.resolve()
    if output == campaign or campaign in output.parents or output in campaign.parents:
        raise ValueError("Supplemental output must be separate from the original campaign")
    output.mkdir(parents=True, exist_ok=False)
    result = {"state": "running", "scope": "Diagnostic native program RSS only; no target evaluation or acceptance waiver; no speed claim.",
              "source_revision": revision, "original_report_sha256": original_hash,
              "platform": platform.platform(), "machine": platform.machine(),
              "native_time_sha256": digest(Path("/usr/bin/time")),
              "total_budget_seconds": DIAGNOSTIC_BUDGET_SECONDS,
              "native_units": "KiB converted to bytes" if sys.platform == "linux" else "bytes",
              "method": "One discarded warmup per runtime, then nine serial alternating pairs; original navigation fixtures and packages; normal telemetry, C locale for native counter labels; native time program high-water distinct from original sampled process-tree RSS.",
              "runtimes": source["runtimes"], "attempts": [], "samples": []}
    report = output / "diagnosis.json"
    env = os.environ.copy()
    env.pop("PROJECTATLAS_NO_TELEMETRY", None)
    env["LC_ALL"] = "C"
    for key in subprocess.check_output(["git", "rev-parse", "--local-env-vars"], text=True, timeout=30).splitlines():
        env.pop(key, None)

    def save() -> None:
        report.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")

    try:
        save()
        result["native_floor_control"] = observe(["/usr/bin/true"], output, env, output, "native-floor", deadline, require_positive=False)
        for round_index in range(10):
            order = ("baseline", "candidate") if round_index % 2 == 0 else ("candidate", "baseline")
            for position, name in enumerate(order):
                attempt = {"round": round_index, "position": position, "runtime": name, "warmup": round_index == 0}
                result["attempts"].append(attempt)
                save()
                command = [str(binaries[name]), "--format", "json", "--require-version", source["runtimes"][name]["version"],
                           "symbols", "relations", "--view", "detailed", "--file", SOURCE_FILE,
                           "--symbol", "item_0_0", "--limit", "10", "--output-bytes", "32768"]
                label = f"{round_index}-{position}-{name}"
                row = observe(command, roots[name], env, output, label, deadline)
                payload = json.loads((output / (label + ".stdout")).read_bytes())
                if payload.get("symbol_relations", {}).get("returned") != 1 or "item_0_0" not in json.dumps(payload):
                    raise ValueError("Detailed navigation lost the expected relationship")
                if state(roots[name]) != preserved[name] or digest(original) != original_hash:
                    raise ValueError("Supplemental observation changed original publication or report")
                if not attempt["warmup"]:
                    result["samples"].append({**attempt, **row})
                save()
        result["maximum_native_program_rss_bytes"] = {
            name: max(row["native_program_peak_rss_bytes"] for row in result["samples"] if row["runtime"] == name)
            for name in binaries}
        result["state"] = "complete"
        save()
        print(json.dumps(result["maximum_native_program_rss_bytes"]))
    except BaseException as error:
        result.update(state="failed", error=repr(error))
        save()
        raise


if __name__ == "__main__":
    main()
