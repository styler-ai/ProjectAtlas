#!/usr/bin/env python3
"""Run the explicit, bounded 0.5.1 installed indexing comparison.

This is an opt-in diagnostic, not an automatic CI benchmark campaign. Results
retain failures and sampled RSS; they do not claim allocation or physical I/O.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import sqlite3
import statistics
import subprocess
import tarfile
import threading
import time
import zipfile


from mcp_composition import McpClient, spawn_owned_process, terminate_owned_process


SOURCE_FILE = "src/group_0/module_0.rs"
DECLARATIONS_PER_FILE = 12
COMMAND_TIMEOUT = 1800


def digest(path: Path) -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def archive_runtime_digest(path: Path) -> str:
    """Hash the exact native runtime member without extracting archive paths."""
    if path.suffix == ".zip":
        with zipfile.ZipFile(path) as package, package.open("projectatlas.exe") as executable:
            return hashlib.file_digest(executable, "sha256").hexdigest()
    with tarfile.open(path) as package:
        member = package.getmember("projectatlas/projectatlas")
        if not member.isfile():
            raise ValueError("runtime archive member is not a regular file")
        with package.extractfile(member) as executable:
            return hashlib.file_digest(executable, "sha256").hexdigest()


def fixture(root: Path, files: int = 1024, padding: int = 0) -> None:
    root.mkdir(parents=True, exist_ok=False)
    for index in range(files):
        path = root / f"src/group_{index // 64}/module_{index}.rs"
        path.parent.mkdir(parents=True, exist_ok=True)
        source = "//! Generated indexing reference fixture.\n" + "".join(
            f"pub fn item_{index}_{item}(n: u64) -> u64 {{ n.saturating_add({item}) }}\n"
            for item in range(DECLARATIONS_PER_FILE)
        )
        path.write_text(source + ("// " + "x" * padding + "\n" if padding else ""), encoding="utf-8")
    (root / "README.md").write_text(
        "# Generated indexing reference\n\n[Source](src/group_0/module_0.rs)\n", encoding="utf-8"
    )


def database(root: Path) -> Path:
    return root / ".projectatlas/projectatlas.db"


def state(root: Path) -> dict:
    with sqlite3.connect(database(root).resolve().as_uri() + "?mode=ro", uri=True) as connection:
        identity = connection.execute(
            "SELECT hex(project_instance_id), active_generation FROM project_identity"
        ).fetchone()
        publication = dict(connection.execute(
            "SELECT key,value FROM metadata WHERE key IN "
            "('index_publication_state','index_publication_generation')"
        ))
        counts = {
            table: connection.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]
            for table in ("symbols", "symbol_relations", "graph_entities", "graph_relations")
        }
        purpose = connection.execute(
            "SELECT p.purpose,p.status FROM purposes p JOIN nodes n ON n.id=p.node_id WHERE n.path=?",
            (SOURCE_FILE,),
        ).fetchone()
    if identity is None or publication.get("index_publication_state") != "complete":
        raise AssertionError("fixture has no complete project publication")
    return {"identity": identity[0], "generation": identity[1], "publication": publication,
            "counts": counts, "purpose": list(purpose) if purpose else None}


def symbol_report(payload: dict) -> dict:
    """Select the shared report without confusing its integer symbol count for a report."""
    report = payload.get("scan", {}).get("report", payload)
    if "parsed" in report:
        return report
    symbols = report.get("symbols", report.get("last_symbols"))
    return symbols if isinstance(symbols, dict) else {}


class Campaign:
    def __init__(self, output: Path, baseline: Path, candidate: Path, version: str):
        self.output = output
        self.runtimes = {"baseline": baseline, "candidate": candidate}
        self.versions = {"baseline": "0.5.0", "candidate": version}
        self.result = {
            "schema_version": 1, "passed": False, "telemetry": "enabled",
            "source_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True, timeout=30).strip(),
            "source_dirty": bool(subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=all"], text=True, timeout=30).strip()),
            "workflow_run_id": os.environ.get("GITHUB_RUN_ID"),
            "candidate_origin": "installed release workflow" if os.environ.get("GITHUB_ACTIONS") == "true" else "local draft; not installed release acceptance",
            "platform": platform.platform(), "machine": platform.machine(),
            "python": platform.python_version(),
            "automatic_budget_observation": "omitted timeout uses native selected deadline; exact selection is covered by owning causal Rust tests",
            "resource_method": "psutil sampled process-tree RSS at 5ms; I/O and allocations unavailable",
            "runtimes": {name: {"version": self.versions[name], "sha256": digest(path)}
                         for name, path in self.runtimes.items()},
            "samples": [], "checks": [],
        }
        self.env = os.environ.copy()
        self.env.pop("PROJECTATLAS_NO_TELEMETRY", None)
        for key in subprocess.check_output(["git", "rev-parse", "--local-env-vars"], text=True).splitlines():
            self.env.pop(key, None)

    def save(self) -> None:
        self.output.write_text(json.dumps(self.result, indent=2) + "\n", encoding="utf-8")

    def run(self, runtime: str, root: Path, arguments: list[str], case: str | None = None,
            expected_success: bool = True) -> dict:
        import psutil

        if psutil.__version__ != "7.0.0":
            raise RuntimeError("measurement requires the hash-locked psutil 7.0.0 environment")
        command = [str(self.runtimes[runtime]), "--format", "json", "--require-version",
                   self.versions[runtime], *arguments]
        started = time.perf_counter()
        process, job = spawn_owned_process(command, cwd=root, env=self.env,
                                           stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        stop = threading.Event()
        peak = [0]
        sampler_errors = []

        def sample() -> None:
            while not stop.is_set():
                try:
                    parent = psutil.Process(process.pid)
                    rss = 0
                    for child in [parent, *parent.children(recursive=True)]:
                        try:
                            rss += child.memory_info().rss
                        except (psutil.NoSuchProcess, psutil.ZombieProcess):
                            pass
                    peak[0] = max(peak[0], rss)
                except (psutil.NoSuchProcess, psutil.ZombieProcess):
                    pass
                except Exception as error:
                    sampler_errors.append(str(error))
                    return
                stop.wait(0.005)

        sampler = threading.Thread(target=sample, daemon=True)
        try:
            sampler.start()
            if job is not None:
                job.resume()
            stdout, stderr = process.communicate(timeout=COMMAND_TIMEOUT)
            elapsed = time.perf_counter() - started
            if job is not None:
                job.wait_for_zero_active(5)
        except BaseException:
            terminate_owned_process(process, job)
            raise
        finally:
            stop.set()
            sampler.join(timeout=5)
            if job is not None:
                job.close()
        if sampler.is_alive() or sampler_errors or (case is not None and not peak[0]):
            raise RuntimeError(f"resource sample unavailable: {sampler_errors}")
        if (process.returncode == 0) != expected_success:
            raise RuntimeError(f"{runtime} {arguments}: exit {process.returncode}: {stderr.decode(errors='replace')[:4096]}")
        if expected_success:
            payload = json.loads(stdout)
        else:
            # The published predecessor prints some work refusals as human diagnostics.
            try:
                payload = json.loads(stderr)
            except json.JSONDecodeError:
                payload = {"error_text": stderr.decode(errors="replace")[:4096]}
        report = payload.get("scan", {}).get("report", payload)
        symbols = symbol_report(payload)
        if case:
            with sqlite3.connect(database(root).resolve().as_uri() + "?mode=ro", uri=True) as connection:
                source_bytes = connection.execute(
                    "SELECT SUM(size_bytes) FROM nodes WHERE exists_now=1 AND kind='file'"
                ).fetchone()[0]
            self.result["samples"].append({
                "runtime": runtime, "case": case, "wall_seconds": elapsed,
                "sampled_peak_rss_bytes": peak[0], "output_bytes": len(stdout),
                "symbols": symbols, "text_index": report.get("text_index"),
                "fixture_source_bytes": source_bytes,
                "source_bytes_per_second": source_bytes / elapsed if case in {"initial", "full", "rebuild"} else None,
            })
            self.save()
        return payload

    def check(self, condition: bool, name: str) -> None:
        self.result["checks"].append({"name": name, "passed": bool(condition)})
        self.save()
        if not condition:
            raise AssertionError(name)

    def full_work(self, payload: dict, files: int, name: str, added: int = 0) -> None:
        symbols = symbol_report(payload)
        self.check(symbols.get("parsed") == files + 1 and symbols.get("unchanged") == 0,
                   name + ": complete required parse, not a cached no-op")
        self.check(symbols.get("symbols") == files * DECLARATIONS_PER_FILE + 1 + added
                   and symbols.get("relations") == files * DECLARATIONS_PER_FILE
                   and all(symbols.get(field) == 0 for field in ("too_large", "binary_or_non_utf8", "timed_out")),
                   name + ": complete expected declarations and calls without parser omissions")

    def reference(self, root: Path) -> None:
        for runtime in self.runtimes:
            fixture(root / runtime)
        for iteration in range(3):
            order = ("baseline", "candidate") if iteration % 2 == 0 else ("candidate", "baseline")
            for runtime in order:
                current = root / runtime
                arguments = ["init"] if iteration == 0 else ["scan", "."]
                case = "initial" if iteration == 0 else "full"
                self.full_work(self.run(runtime, current, arguments, case), 1024, case, iteration)
                self.full_work(self.run(runtime, current, ["scan", "."], "full"), 1024, "full", iteration)
                self.full_work(self.run(runtime, current, ["symbols", "build"], "rebuild"), 1024, "rebuild", iteration)
                before = state(current)
                for _ in range(2):
                    report = self.run(runtime, current, ["watch", "--once"], "cold_noop")
                    self.check(state(current) == before and not report.get("text_index", {}).get("indexed", 0),
                               "unchanged refresh preserves publication and has no text publication")
                path = current / SOURCE_FILE
                with path.open("a", encoding="utf-8") as source:
                    source.write(f"\npub fn reference_edit_{iteration}() {{}}\n")
                report = self.run(runtime, current, ["watch", "--once"], "sparse")
                self.check(report.get("last_symbols", {}).get("parsed") == 1,
                           "sparse refresh parses only changed source")
                after = state(current)
                self.check(after["identity"] == before["identity"] and after["generation"] == before["generation"] + 1
                           and after["counts"]["symbols"] == before["counts"]["symbols"] + 1,
                           "sparse refresh publishes the exact added declaration once and preserves identity")
        for iteration in range(2):
            order = ("candidate", "baseline") if iteration % 2 == 0 else ("baseline", "candidate")
            for runtime in order:
                fresh = root / f"{runtime}-initial-{iteration}"
                fixture(fresh)
                # The original procedure's extra initial samples contain all three edits.
                with (fresh / SOURCE_FILE).open("a", encoding="utf-8") as source:
                    for edit in range(3):
                        source.write(f"\npub fn reference_edit_{edit}() {{}}\n")
                self.full_work(self.run(runtime, fresh, ["init"], "initial"), 1024, "initial", 3)

    def navigation(self, root: Path) -> None:
        root.mkdir(parents=True)
        states = {}
        for runtime in self.runtimes:
            current = root / runtime
            fixture(current)
            self.run(runtime, current, ["init"])
            self.run(runtime, current, ["purpose", "set", SOURCE_FILE, "Own the reference navigation declarations."])
            states[runtime] = state(current)
        for iteration in range(10):
            for runtime in (("baseline", "candidate") if iteration % 2 == 0 else ("candidate", "baseline")):
                current = root / runtime
                for case, arguments in [
                    ("summary", ["summary", SOURCE_FILE, "--limit", "10"]),
                    ("detailed", ["symbols", "relations", "--view", "detailed", "--file", SOURCE_FILE,
                                  "--symbol", "item_0_0", "--limit", "10", "--output-bytes", "32768"]),
                    ("compatible_open", ["watch", "--once"]),
                ]:
                    payload = self.run(runtime, current, arguments, case if iteration else None)
                    if case == "summary":
                        self.check(payload.get("file_path") == SOURCE_FILE and "item_0_0" in json.dumps(payload),
                                   "summary retains exact path and useful source facts")
                    elif case == "detailed":
                        self.check(payload.get("symbol_relations", {}).get("returned") == 1
                                   and "item_0_0" in json.dumps(payload)
                                   and SOURCE_FILE in json.dumps(payload),
                                   "detailed response retains useful exact-anchor relationship")
                    self.check(state(current) == states[runtime],
                               runtime + " current-contract navigation/open preserves complete state")

        # A prior semantic contract requires one-time repair, not a steady-state no-op.
        upgrade = root / "semantic-upgrade"
        fixture(upgrade)
        self.run("baseline", upgrade, ["init"])
        self.run("baseline", upgrade, ["purpose", "set", SOURCE_FILE, "Own the reference navigation declarations."])
        before = state(upgrade)
        backup = root / "prior-contract.db"
        with sqlite3.connect(database(upgrade)) as source, sqlite3.connect(backup) as target:
            source.backup(target)
        for iteration in range(3):
            for runtime in (("baseline", "candidate") if iteration % 2 == 0 else ("candidate", "baseline")):
                with sqlite3.connect(backup) as source, sqlite3.connect(database(upgrade)) as target:
                    source.backup(target)
                arguments = ["scan"] if runtime == "baseline" else ["watch", "--once"]
                payload = self.run(runtime, upgrade, arguments, "semantic_upgrade")
                after = state(upgrade)
                self.check(after["identity"] == before["identity"]
                           and after["purpose"] == before["purpose"]
                           and after["counts"] == before["counts"]
                           and after["generation"] == before["generation"] + 1,
                           runtime + " semantic reconciliation preserves complete graph/identity/purpose and publishes once")
                if runtime == "baseline":
                    self.full_work(payload, 1024, "semantic upgrade full-work comparison")
                else:
                    self.run(runtime, upgrade, ["watch", "--once"])
                    self.check(state(upgrade) == after, "semantic upgrade subsequent current-contract open is a no-op")
        # Rollback is correctness proof after the paired timings.
        self.run("candidate", upgrade, ["watch", "--once"])
        self.run("baseline", upgrade, ["watch", "--once"])
        after = state(upgrade)
        self.check(after["identity"] == before["identity"] and after["purpose"] == before["purpose"]
                   and after["counts"] == before["counts"], "semantic rollback preserves complete authored and derived state")

    def larger_and_dense(self, root: Path) -> None:
        for runtime in self.runtimes:
            large = root / runtime / "large"
            fixture(large, files=2048, padding=8192)
            self.full_work(self.run(runtime, large, ["init"], "large_initial"), 2048, "larger file/byte corpus")
            self.run(runtime, large, ["purpose", "set", SOURCE_FILE, "Preserve large-fixture authored purpose."])
            dense = root / runtime / "dense"
            dense.mkdir()
            for index in range(256):
                (dense / f"module_{index}.py").write_text(
                    "".join(f"import module_{target}\n" for target in range(256) if target != index), encoding="utf-8")
            self.run(runtime, dense, ["init"], "dense_initial")
            self.check(state(dense)["counts"]["symbol_relations"] >= 256 * 255,
                       runtime + " dense corpus contains complete intended relationships")
        for iteration in range(3):
            order = ("baseline", "candidate") if iteration % 2 == 0 else ("candidate", "baseline")
            for runtime in order:
                for kind, source_file, addition in [
                    ("large", SOURCE_FILE, f"\npub fn expanded_edit_{iteration}() {{}}\n"),
                    ("dense", "module_0.py", f"\ndef expanded_edit_{iteration}():\n    pass\n"),
                ]:
                    current = root / runtime / kind
                    before = state(current)
                    with (current / source_file).open("a", encoding="utf-8") as source:
                        source.write(addition)
                    report = self.run(runtime, current, ["watch", "--once"], kind + "_sparse")
                    self.check(report.get("last_symbols", {}).get("parsed") == 1,
                               runtime + " " + kind + " refresh reuses unchanged parse results")
                    if kind == "large" and runtime == "candidate":
                        self.check(report.get("text_index", {}).get("indexed") == 1,
                                   "large sparse refresh stages only the changed text")
                    after = state(current)
                    self.check(after["identity"] == before["identity"] and after["purpose"] == before["purpose"]
                               and after["generation"] == before["generation"] + 1
                               and after["counts"]["symbols"] == before["counts"]["symbols"] + 1,
                               runtime + " " + kind + " refresh publishes exact added export once and preserves state")
        # Fault proof is separate from the paired timings; it must not warm only one participant.
        current = root / "candidate" / "large"
        before = state(current)
        with (current / SOURCE_FILE).open("a", encoding="utf-8") as source:
            source.write("\npub fn deadline_retry() {}\n")
        refusal = self.run("candidate", current, ["watch", "--once", "--timeout-seconds", "0"], expected_success=False)
        self.check("deadline" in json.dumps(refusal), "zero budget refuses specifically at its deadline")
        self.check(state(current) == before, "deadline refusal preserves large publication and purpose")
        retry = self.run("candidate", current, ["watch", "--once"])
        after = state(current)
        self.check(retry.get("last_symbols", {}).get("parsed") == 1
                   and after["identity"] == before["identity"] and after["purpose"] == before["purpose"]
                   and after["generation"] == before["generation"] + 1
                   and after["counts"]["symbols"] == before["counts"]["symbols"] + 1,
                   "omitted-budget retry publishes exact work after injected refusal")
        for kind in ("large", "dense"):
            current = root / "candidate" / kind
            incremental = state(current)
            self.run("candidate", current, ["scan", "."], kind + "_full_convergence")
            complete = state(current)
            self.check(incremental["identity"] == complete["identity"]
                       and incremental["counts"] == complete["counts"],
                       kind + " affected update converges with complete full scan")

    def warm(self, root: Path) -> None:
        self.result["warm_mcp"] = []
        for runtime in self.runtimes:
            current = root / runtime
            fixture(current)
            self.run(runtime, current, ["init"])
            before = state(current)
            client = McpClient(self.runtimes[runtime], current, self.env,
                               request_timeout_seconds=COMMAND_TIMEOUT,
                               required_version=self.versions[runtime])
            try:
                for iteration in range(10):
                    response, milliseconds = client.call("atlas_watch_once", {"project_path": str(current)})
                    self.check("watch:" in response and "error:" not in response,
                               runtime + " warm MCP refresh succeeds")
                    if iteration:
                        self.result["warm_mcp"].append({"runtime": runtime,
                            "wall_seconds": milliseconds / 1000,
                            "output_bytes": len(response.encode()),
                            "source_work": "not exposed by this public response; owning causal tests required"})
                    self.check(state(current) == before, runtime + " warm MCP preserves publication")
            finally:
                client.close()
            path = current / SOURCE_FILE
            metadata = path.stat()
            original = path.read_bytes()
            changed = original.replace(b"saturating_add", b"saturating_sub")
            self.check(changed != original and len(changed) == len(original), "same-size offline edit fixture")
            path.write_bytes(changed)
            os.utime(path, ns=(metadata.st_atime_ns, metadata.st_mtime_ns))
            self.run(runtime, current, ["watch", "--once"], "offline_same_metadata")
            with sqlite3.connect(database(current)) as connection:
                content = connection.execute("SELECT content FROM file_texts WHERE path=?", (SOURCE_FILE,)).fetchone()
            self.check(content is not None and "saturating_sub" in content[0],
                       runtime + " offline same-metadata edit is published completely")

    def evaluate(self) -> None:
        self.result["passed"] = False
        summaries = {}
        self.result["summary"] = summaries
        failures = []

        def target(condition: bool, name: str) -> None:
            self.result["checks"].append({"name": name, "passed": bool(condition)})
            if not condition:
                failures.append(name)

        for case in ("initial", "full", "rebuild", "cold_noop", "sparse", "summary", "detailed", "compatible_open", "large_sparse", "dense_sparse", "semantic_upgrade"):
            summaries[case] = {}
            for runtime in self.runtimes:
                samples = [row for row in self.result["samples"] if row["case"] == case and row["runtime"] == runtime]
                expected = {"initial": 3, "full": 5, "rebuild": 3, "cold_noop": 6, "sparse": 3, "large_sparse": 3, "dense_sparse": 3, "semantic_upgrade": 3}.get(case, 9)
                self.check(len(samples) == expected, f"{runtime} {case} exact sample count")
                summaries[case][runtime] = {
                    "median_seconds": statistics.median(row["wall_seconds"] for row in samples),
                    "maximum_sampled_rss_bytes": max(row["sampled_peak_rss_bytes"] for row in samples),
                    "median_source_bytes_per_second": statistics.median(row["source_bytes_per_second"] for row in samples)
                    if case in {"initial", "full", "rebuild"} else None,
                }
            baseline, candidate = summaries[case]["baseline"], summaries[case]["candidate"]
            ratio = {"initial": .9, "full": .9, "rebuild": .9, "sparse": .5, "large_sparse": .5, "dense_sparse": .9}.get(case, 1.1)
            target(candidate["median_seconds"] <= baseline["median_seconds"] * ratio, case + " latency target")
            target(candidate["maximum_sampled_rss_bytes"] <= baseline["maximum_sampled_rss_bytes"] * 1.1,
                   case + " sampled RSS target")
            if case in {"initial", "full", "rebuild"}:
                target(candidate["median_source_bytes_per_second"] >= baseline["median_source_bytes_per_second"] * 1.1,
                       case + " complete-source throughput target")
            if case == "cold_noop" and platform.system() == "Linux":
                target(candidate["median_seconds"] < .060, "Linux cold no-op below 60ms")
        warm = {}
        for runtime in self.runtimes:
            samples = [row["wall_seconds"] for row in self.result["warm_mcp"] if row["runtime"] == runtime]
            self.check(len(samples) == 9, runtime + " warm MCP exact sample count")
            warm[runtime] = statistics.median(samples)
        summaries["warm_mcp"] = {"median_seconds": warm, "rss": "not measured by public MCP route"}
        target(warm["candidate"] <= warm["baseline"] * 1.1, "warm MCP exact reconciliation latency target")
        self.result["passed"] = not failures
        self.save()
        if failures:
            raise AssertionError("; ".join(failures))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--candidate-version", required=True)
    parser.add_argument("--candidate-archive", type=Path)
    parser.add_argument("--baseline-archive", type=Path, required=True)
    parser.add_argument("--baseline-checksums", type=Path, required=True)
    parser.add_argument("--work-root", type=Path, required=True)
    args = parser.parse_args()
    root = args.work_root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    campaign = Campaign(root / "results.json", args.baseline.resolve(strict=True),
                        args.candidate.resolve(strict=True), args.candidate_version)
    try:
        archive_digest = digest(args.baseline_archive)
        catalog = json.loads((Path(__file__).resolve().parents[1] /
                              "v0.5.1-indexing-baselines.json").read_text(encoding="utf-8"))
        campaign.check(catalog["baseline_version"] == "0.5.0"
                       and catalog["assets"].get(args.baseline_archive.name) == archive_digest,
                       "baseline archive matches preregistered immutable identity")
        line = f"{archive_digest}  {args.baseline_archive.name}"
        campaign.check(args.baseline_checksums.read_text(encoding="utf-8").splitlines().count(line) == 1,
                       "baseline archive matches published checksum")
        campaign.check(archive_runtime_digest(args.baseline_archive) == digest(args.baseline),
                       "baseline executable belongs to verified archive")
        if args.candidate_archive is not None:
            campaign.check(archive_runtime_digest(args.candidate_archive) == digest(args.candidate),
                           "candidate executable belongs to workflow package")
            campaign.result["candidate_archive"] = {
                "name": args.candidate_archive.name, "sha256": digest(args.candidate_archive)}
        elif os.environ.get("GITHUB_ACTIONS") == "true":
            raise ValueError("installed workflow comparison requires candidate archive provenance")
        campaign.result["baseline_archive"] = {"name": args.baseline_archive.name, "sha256": archive_digest}
        for runtime in campaign.runtimes:
            info = campaign.run(runtime, root, ["runtime-info"])
            campaign.check(info.get("version") == campaign.versions[runtime], runtime + " exact version")
        campaign.reference(root / "reference")
        campaign.navigation(root / "navigation")
        campaign.larger_and_dense(root / "expanded")
        campaign.warm(root / "warm")
        campaign.evaluate()
    except BaseException as error:
        campaign.result["failure"] = f"{type(error).__name__}: {error}"
        campaign.save()
        raise


if __name__ == "__main__":
    main()
