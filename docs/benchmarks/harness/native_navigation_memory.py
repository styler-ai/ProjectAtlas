#!/usr/bin/env python3
"""Inspect an original Linux navigation run without evaluating performance targets.

Only immutable artifacts from this repository's completed manual release workflow
are executable inputs. New fixtures and observations are diagnostic; they cannot
clear the original result or reproduce its VM.
"""

from __future__ import annotations

import argparse
import array
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import tarfile
import time
import zipfile

if sys.platform == "linux":
    import fcntl
    import termios

from indexing_acceptance import SOURCE_FILE, digest, fixture, state
from mcp_composition import spawn_owned_process, terminate_owned_process


ROOT = Path(__file__).resolve().parents[3]
TARGET = "x86_64-unknown-linux-gnu"
MAX_ARCHIVE_BYTES = 256 * 1024 * 1024
TOTAL_SECONDS = 540
COMMAND_SECONDS = 90
PROC_FIELDS = {"VmRSS", "VmHWM", "RssAnon", "RssFile", "RssShmem", "Threads"}


def api(repo: str, suffix: str) -> dict:
    """Use gh's authenticated API/redirect handling without logging credentials."""
    return json.loads(subprocess.check_output(
        ["gh", "api", f"repos/{repo}/{suffix}"], text=True, timeout=60))


def verify_file(path: Path, expected: str, size: int | None = None) -> None:
    if not re.fullmatch(r"[0-9a-f]{64}", expected):
        raise ValueError("Invalid original digest")
    if not path.is_file() or path.stat().st_size > MAX_ARCHIVE_BYTES:
        raise ValueError("Missing or oversized original")
    if size is not None and path.stat().st_size != size:
        raise ValueError("Artifact length differs")
    if digest(path) != expected:
        raise ValueError("Artifact digest differs")


def download_artifact(repo: str, run_id: int, artifact: dict, destination: Path) -> Path:
    if type(artifact["id"]) is not int or artifact["id"] <= 0:
        raise ValueError("Invalid original artifact ID")
    if artifact["expired"] or artifact["workflow_run"]["id"] != run_id:
        raise ValueError("Artifact does not belong to the original run")
    if artifact["size_in_bytes"] > MAX_ARCHIVE_BYTES:
        raise ValueError("Oversized artifact")
    expected = artifact["digest"]
    if not expected.startswith("sha256:"):
        raise ValueError("Original artifact has no SHA-256 digest")
    path = destination / f"{artifact['id']}.zip"
    options = []
    if "--allow-escape-sequences" in subprocess.check_output(["gh", "api", "--help"], text=True):
        options.append("--allow-escape-sequences")
    with path.open("xb") as stream:
        subprocess.run(["gh", "api", f"repos/{repo}/actions/artifacts/{artifact['id']}/zip", *options],
                       stdout=stream, check=True, timeout=60)
    verify_file(path, expected[7:], artifact["size_in_bytes"])
    return path


def original_report(run: dict, report: dict, repo: str, run_id: int) -> None:
    """Refuse foreign, incomplete, or non-native provenance before execution."""
    if (run["id"] != run_id or run["event"] != "workflow_dispatch"
            or run["path"] != ".github/workflows/release.yml"
            or run["head_repository"]["full_name"] != repo
            or run["status"] != "completed"):
        raise ValueError("Expected a completed same-repository manual release run")
    if not re.fullmatch(r"(?:[0-9a-f]{40}|[0-9a-f]{64})", run["head_sha"]):
        raise ValueError("Invalid original source object")
    if (report["schema_version"] != 1 or report["source_revision"] != run["head_sha"]
            or report["workflow_run_id"] != str(run_id)
            or report["candidate_origin"] != "installed release workflow"
            or report["machine"] != "x86_64" or not report["platform"].startswith("Linux-")
            or report["telemetry"] != "enabled"):
        raise ValueError("Original report identity differs")
    for name in ("baseline", "candidate"):
        rows = [r for r in report["samples"] if r["case"] == "detailed" and r["runtime"] == name]
        if len(rows) != 9 or any(r["output_bytes"] <= 4096 for r in rows):
            raise ValueError("Original detailed-navigation inventory cannot support the observation")
        version = report["runtimes"][name]["version"]
        if not re.fullmatch(r"0\.5\.\d+(?:-rc\d+)?", version):
            raise ValueError("Unexpected original runtime version")
    if report["runtimes"]["baseline"]["version"] != "0.5.0":
        raise ValueError("Expected the registered 0.5.0 baseline")


def runtime_from_archive(archive: Path, output: Path, expected: str) -> Path:
    """Preserve the authenticated package layout through the standard data filter."""
    if not hasattr(tarfile, "data_filter"):
        raise ValueError("Safe package extraction requires Python tar data filtering")
    with tarfile.open(archive, "r:gz") as package:
        inventory = package.getmembers()
        members = [m for m in inventory if m.name == "projectatlas/projectatlas"]
        if (len(members) != 1 or not members[0].isfile() or len(inventory) > 4096
                or sum(m.size for m in inventory) > MAX_ARCHIVE_BYTES):
            raise ValueError("Archive runtime is missing, indirect, duplicated, or oversized")
        try:
            for member in inventory:
                tarfile.data_filter(member, str(output))
            output.mkdir(exist_ok=False)
            package.extractall(output, members=inventory, filter="data")
        except tarfile.FilterError as error:
            raise ValueError("Package member would escape its private root") from error
    runtime = output / "projectatlas/projectatlas"
    verify_file(runtime, expected)
    runtime.chmod(0o755)
    return runtime


def memory_status(text: str) -> dict | None:
    values = {}
    for line in text.splitlines():
        key, _, value = line.partition(":")
        if key in PROC_FIELDS:
            fields = value.split()
            values[key] = int(fields[0]) * (1024 if fields[1:] == ["kB"] else 1)
    # Exited/zombie processes can retain a status file with only Threads.
    return values if values.get("VmRSS", 0) > 0 else None


def process_tree(pid: int) -> list[int]:
    found = {pid}
    pending = [pid]
    while pending:
        parent = pending.pop()
        for task in Path(f"/proc/{parent}/task").glob("*"):
            try:
                children = (task / "children").read_text().split()
            except (FileNotFoundError, ProcessLookupError):
                continue
            for value in children:
                child = int(value)
                if child not in found:
                    found.add(child)
                    pending.append(child)
                    if len(found) > 16:
                        raise ValueError("Unexpected diagnostic process tree")
    return sorted(found)


def snapshot(pid: int, binary: Path, destination: Path, label: str) -> dict:
    """An exited process is unavailable, never a successful zero-RSS snapshot."""
    try:
        status = memory_status(Path(f"/proc/{pid}/status").read_text())
        smaps = Path(f"/proc/{pid}/smaps").read_text()
        if status is None:
            return {"phase": label, "available": False, "reason": "resident status unavailable"}
        if not smaps.strip():
            return {"phase": label, "available": False, "reason": "mapping data unavailable"}
        classes = {}
        category = None
        for line in smaps.splitlines():
            if re.match(r"^[0-9a-f]+-[0-9a-f]+ ", line):
                fields = line.split(None, 5)
                path = fields[5] if len(fields) == 6 else ""
                category = ("runtime_image" if path == str(binary) else "heap" if path == "[heap]"
                            else "stack" if path.startswith("[stack") else "fixture_database" if "projectatlas.db" in path
                            else "system_file" if path.startswith("/") else "anonymous_or_special")
            elif category is not None:
                key, _, value = line.partition(":")
                if key in {"Rss", "Pss", "Anonymous", "Private_Dirty", "Private_Clean"}:
                    bucket = classes.setdefault(category, {})
                    bucket[key] = bucket.get(key, 0) + int(value.split()[0]) * 1024
        if not classes:
            return {"phase": label, "available": False, "reason": "mapping data unavailable"}
        rollup = Path(f"/proc/{pid}/smaps_rollup").read_text()
        if not rollup.strip():
            return {"phase": label, "available": False, "reason": "rollup data unavailable"}
        path = destination / (label + ".smaps")
        path.write_text(smaps)
        rollup_path = destination / (label + ".smaps_rollup")
        rollup_path.write_text(rollup)
        return {"phase": label, "available": True, "status": status, "mapping_classes": classes,
                "smaps_sha256": digest(path), "rollup_sha256": digest(rollup_path),
                "captured_monotonic": time.monotonic(), "pid": pid}
    except (FileNotFoundError, ProcessLookupError):
        return {"phase": label, "available": False, "reason": "process data unavailable"}


def query_command(binary: Path, version: str) -> list[str]:
    return [str(binary), "--format", "json", "--require-version", version, "symbols", "relations",
            "--view", "detailed", "--file", SOURCE_FILE, "--symbol", "item_0_0", "--limit", "10", "--output-bytes", "32768"]


def observe(command: list[str], root: Path, env: dict, output: Path, hold: bool, deadline: float) -> dict:
    if sys.platform != "linux":
        raise ValueError("Native memory composition requires Linux /proc")
    output.mkdir(exist_ok=False)
    preserved = state(root)
    options = {"pipesize": 4096} if hold else {}
    process, job = spawn_owned_process(command, cwd=root, env=env, stdout=subprocess.PIPE,
                                       stderr=subprocess.PIPE, **options)
    result = {"state": "running", "pid": process.pid,
              "mode": "backpressured_stdout" if hold else "ordinary_pipe", "timeline": [], "snapshots": []}
    chunks = []
    stdout = b""
    stderr = b""
    started = time.monotonic()
    result["started_monotonic"] = started
    snapshots = 0
    peak = 0
    try:
        if job is not None:
            job.resume()
        fd = process.stdout.fileno()
        if hold:
            if fcntl.fcntl(fd, fcntl.F_GETPIPE_SZ) != 4096:
                raise ValueError("Unexpected stdout pipe capacity")
            pending = array.array("i", [0])
            while process.poll() is None:
                if time.monotonic() >= deadline or time.monotonic() - started > COMMAND_SECONDS:
                    raise TimeoutError("Output hold exceeded its budget")
                fcntl.ioctl(fd, termios.FIONREAD, pending, True)
                if pending[0] == 4096:
                    break
                time.sleep(.001)
            if process.poll() is not None or pending[0] != 4096:
                raise ValueError("Output did not reach the diagnostic hold point")
            result["snapshots"].append(snapshot(process.pid, Path(command[0]), output, "stdout_pipe_full"))
        os.set_blocking(fd, False)
        while process.poll() is None:
            if time.monotonic() >= deadline or time.monotonic() - started > COMMAND_SECONDS:
                raise TimeoutError("Observation exceeded its budget")
            row = {"offset_seconds": time.monotonic() - started, "stdout_bytes_observed": sum(map(len, chunks)), "processes": []}
            pending = array.array("i", [0])
            fcntl.ioctl(fd, termios.FIONREAD, pending, True)
            row["stdout_bytes_pending"] = pending[0]
            for pid in process_tree(process.pid):
                try:
                    memory = memory_status(Path(f"/proc/{pid}/status").read_text())
                    if memory is not None and os.getpgid(pid) == process.pid:
                        row["processes"].append({"pid": pid, "executable": Path(os.readlink(f"/proc/{pid}/exe")).name, **memory})
                except (FileNotFoundError, ProcessLookupError):
                    continue
            result["timeline"].append(row)
            rss = sum(p["VmRSS"] for p in row["processes"])
            if snapshots < 4 and rss >= peak + 512 * 1024:
                result["snapshots"].append(snapshot(process.pid, Path(command[0]), output, f"observed_peak_{snapshots}"))
                snapshots += 1
                peak = rss
            try:
                data = os.read(fd, 65536)
                if data:
                    chunks.append(data)
                    if sum(map(len, chunks)) > 65536:
                        raise ValueError("Observed output exceeded its bound")
            except BlockingIOError:
                pass
            time.sleep(.001)
        remaining, stderr = process.communicate(timeout=max(.1, min(COMMAND_SECONDS, deadline - time.monotonic())))
        stdout = b"".join(chunks) + (remaining or b"")
        if job is not None:
            job.wait_for_zero_active(5)
        if process.returncode != 0 or len(stdout) > 65536:
            raise ValueError("Observed query failed or exceeded its output bound")
        payload = json.loads(stdout)
        if payload.get("symbol_relations", {}).get("returned") != 1 or "item_0_0" not in json.dumps(payload):
            raise ValueError("Observed query lost its expected relationship")
        if state(root) != preserved:
            raise ValueError("Observation changed publication or authored state")
        result.update(state="complete", state_preserved=True)
        return result
    except BaseException as error:
        result.update(state="failed", error=repr(error))
        try:
            terminate_owned_process(process, job)
        except BaseException as cleanup_error:
            result["cleanup_error"] = repr(cleanup_error)
            error.add_note(f"Owned process cleanup failed: {cleanup_error}")
        if not stdout:
            stdout = b"".join(chunks)
        if isinstance(error, subprocess.TimeoutExpired):
            stdout += error.output or b""
            stderr = error.stderr or b""
        # The owned tree is stopped. Retain a bounded prefix still buffered in
        # each pipe, including a timeout before the stdout hold point is full.
        for stream, name in ((process.stdout, "stdout"), (process.stderr, "stderr")):
            if not stream.closed:
                try:
                    os.set_blocking(stream.fileno(), False)
                    tail = os.read(stream.fileno(), 65537)
                    if name == "stdout":
                        stdout += tail
                    else:
                        stderr += tail
                except (BlockingIOError, OSError) as capture_error:
                    result.setdefault("capture_errors", []).append(repr(capture_error))
        raise
    finally:
        process.stdout.close()
        process.stderr.close()
        if job is not None:
            job.close()
        (output / "stdout").write_bytes(stdout[:65536])
        (output / "stderr").write_bytes(stderr[:65536])
        result.update(output_bytes=min(len(stdout), 65536), output_sha256=digest(output / "stdout"),
                      stdout_truncated=len(stdout) > 65536, stderr_truncated=len(stderr) > 65536,
                      memory_observed=any(row["processes"] for row in result["timeline"]) or any(s["available"] for s in result["snapshots"]))
        (output / "observation.json").write_text(json.dumps(result, indent=2) + "\n")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--original-run-id", type=int, required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    args = parser.parse_args()
    if sys.platform != "linux" or platform.machine() != "x86_64" or args.original_run_id <= 0:
        raise ValueError("Expected Linux x86_64 and a positive original run ID")
    repo = os.environ["GITHUB_REPOSITORY"]
    if not re.fullmatch(r"[\w.-]+/[\w.-]+", repo):
        raise ValueError("Invalid same-repository selector")
    output = args.output_root.resolve()
    output.mkdir(parents=True, exist_ok=False)
    deadline = time.monotonic() + TOTAL_SECONDS
    run = api(repo, f"actions/runs/{args.original_run_id}")
    inventory = api(repo, f"actions/runs/{args.original_run_id}/artifacts?per_page=100")
    if inventory["total_count"] != len(inventory["artifacts"]):
        raise ValueError("Original artifact inventory is incomplete")
    def selected(name: str) -> dict:
        rows = [a for a in inventory["artifacts"] if a["name"] == name]
        if len(rows) != 1 or rows[0]["workflow_run"]["head_sha"] != run["head_sha"]:
            raise ValueError("Missing, duplicate, or mismatched original artifact")
        return rows[0]
    report_zip = download_artifact(repo, args.original_run_id, selected("indexing-performance-linux-x64-posix"), output)
    with zipfile.ZipFile(report_zip) as archive:
        if archive.namelist() != ["results.json"] or archive.getinfo("results.json").file_size > 4 * 1024 * 1024:
            raise ValueError("Unexpected original report archive")
        original_bytes = archive.read("results.json")
        original = json.loads(original_bytes)
    original_report(run, original, repo, args.original_run_id)
    (output / "original-results.json").write_bytes(original_bytes)
    for relative in ("docs/benchmarks/harness/indexing_acceptance.py", "docs/benchmarks/harness/mcp_composition.py", "docs/benchmarks/v0.5.1-indexing-baselines.json"):
        expected = subprocess.check_output(["git", "show", run["head_sha"] + ":" + relative], cwd=ROOT, timeout=30)
        if (ROOT / relative).read_bytes() != expected:
            raise ValueError("Deterministic fixture dependencies differ from original source")
    catalog = json.loads((ROOT / "docs/benchmarks/v0.5.1-indexing-baselines.json").read_text())
    baseline = original["baseline_archive"]
    if baseline["name"] != f"projectatlas-v0.5.0-{TARGET}.tar.gz" or catalog["assets"].get(baseline["name"]) != baseline["sha256"]:
        raise ValueError("Baseline is not the registered release asset")
    baseline_dir = output / "baseline-download"
    baseline_dir.mkdir()
    subprocess.run(["gh", "release", "download", "v0.5.0", "--repo", repo, "--pattern", baseline["name"], "--dir", str(baseline_dir)], check=True, timeout=60)
    baseline_archive = baseline_dir / baseline["name"]
    verify_file(baseline_archive, baseline["sha256"])
    candidate = original["candidate_archive"]
    version = original["runtimes"]["candidate"]["version"]
    if candidate["name"] != f"projectatlas-v{version}-{TARGET}.tar.gz":
        raise ValueError("Unexpected original candidate archive name")
    package_zip = download_artifact(repo, args.original_run_id, selected("projectatlas-" + TARGET), output)
    with zipfile.ZipFile(package_zip) as archive:
        if archive.namelist() != [candidate["name"]] or archive.getinfo(candidate["name"]).file_size > MAX_ARCHIVE_BYTES:
            raise ValueError("Unexpected original package archive")
        candidate_archive = output / candidate["name"]
        candidate_archive.write_bytes(archive.read(candidate["name"]))
    verify_file(candidate_archive, candidate["sha256"])
    (output / "original-artifacts.json").write_text(json.dumps({
        "run": run, "report_artifact": selected("indexing-performance-linux-x64-posix"),
        "package_artifact": selected("projectatlas-" + TARGET), "baseline_archive": baseline,
        "candidate_archive": candidate,
    }, indent=2) + "\n")
    env = os.environ.copy()
    for key in ("PROJECTATLAS_NO_TELEMETRY", "GH_TOKEN", "GITHUB_TOKEN"):
        env.pop(key, None)
    env["LC_ALL"] = "C"
    for key in subprocess.check_output(["git", "rev-parse", "--local-env-vars"], cwd=ROOT, text=True).splitlines():
        env.pop(key, None)
    runtimes = {}
    roots = {}
    for name, archive in (("baseline", baseline_archive), ("candidate", candidate_archive)):
        directory = output / "runtimes" / name
        directory.mkdir(parents=True)
        runtimes[name] = runtime_from_archive(archive, directory / "package", original["runtimes"][name]["sha256"])
        current = output / "navigation" / name
        fixture(current)
        roots[name] = current
        for arguments in (["init"], ["purpose", "set", SOURCE_FILE, "Own the reference navigation declarations."]):
            command = [str(runtimes[name]), "--format", "json", "--require-version", original["runtimes"][name]["version"], *arguments]
            process, job = spawn_owned_process(command, cwd=current, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                stdout, stderr = process.communicate(timeout=min(COMMAND_SECONDS, max(.1, deadline - time.monotonic())))
            except BaseException:
                terminate_owned_process(process, job)
                raise
            finally:
                if job is not None:
                    job.close()
            if process.returncode or len(stdout) > 1024 * 1024:
                raise ValueError("Reconstructed fixture initialization failed: " + stderr.decode(errors="replace")[:512])
    result = {"state": "running", "scope": "Diagnostic only; different native VM and reconstructed fixtures. No registered sampler, target evaluation, numerical retry, acceptance waiver or speed claim.",
              "original_run_id": args.original_run_id, "source_revision": run["head_sha"],
              "original_report_sha256": hashlib.sha256(original_bytes).hexdigest(), "original_report_artifact_sha256": digest(report_zip),
              "original_source_dirty": original["source_dirty"], "runtimes": original["runtimes"],
              "host": {"platform": platform.platform(), "libc": platform.libc_ver(), "machine": platform.machine(), "cpu_affinity": sorted(os.sched_getaffinity(0)),
                       "cpu_quota": Path("/sys/fs/cgroup/cpu.max").read_text().strip() if Path("/sys/fs/cgroup/cpu.max").is_file() else None},
              "method": "One ordinary-pipe pair, then one reversed backpressured-stdout pair; 1ms diagnostic process-tree observations; normal telemetry; no Campaign.run/evaluate or benchmark targets.", "observations": []}
    report = output / "diagnosis.json"
    def save() -> None:
        report.write_text(json.dumps(result, indent=2) + "\n")
    try:
        save()
        for hold, order in ((False, ("baseline", "candidate")), (True, ("candidate", "baseline"))):
            for name in order:
                command = query_command(runtimes[name], original["runtimes"][name]["version"])
                observation = output / (name + ("-hold" if hold else "-ordinary"))
                result["active_observation"] = {"runtime": name, "directory": observation.name}
                save()
                row = observe(command, roots[name], env, observation, hold, deadline)
                result["observations"].append({"runtime": name, **row})
                result.pop("active_observation")
                save()
        result["state"] = "complete"
        save()
    except BaseException as error:
        result.update(state="failed", error=repr(error))
        save()
        raise


if __name__ == "__main__":
    main()
