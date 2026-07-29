#!/usr/bin/env python3
"""Select the newest input-compatible optional-parser release handoff."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PROOF_INPUTS = Path(__file__).with_name("optional-parser-proof-inputs.py")
RELEASE_ASSET_NAME = "optional-parser-pack-release-assets"
CommandRunner = Callable[[list[str]], subprocess.CompletedProcess[str]]


def valid_commit_sha(value: object) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 40
        and all(character in "0123456789abcdef" for character in value)
    )


def run_command(arguments: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        arguments,
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        timeout=120,
    )


def workflow_run_pages(repository: str, runner: CommandRunner) -> list[object]:
    response = runner(
        [
            "gh",
            "api",
            "--paginate",
            "--slurp",
            (
                f"/repos/{repository}/actions/workflows/optional-parser-pack.yml/runs"
                "?event=workflow_dispatch&status=success&per_page=100"
            ),
        ]
    )
    if response.returncode != 0:
        raise RuntimeError(response.stderr.strip() or "workflow-run lookup failed")
    pages = json.loads(response.stdout)
    if not isinstance(pages, list):
        raise ValueError("paginated workflow-run response must be a JSON array")
    return pages


def ordered_runs(pages: list[object]) -> list[tuple[str, str]]:
    runs: list[tuple[int, str, str]] = []
    seen: set[str] = set()
    for page in pages:
        if not isinstance(page, dict) or not isinstance(page.get("workflow_runs"), list):
            raise ValueError("each workflow-run page must contain workflow_runs")
        for run in page["workflow_runs"]:
            if not isinstance(run, dict):
                raise ValueError("workflow run must be a JSON object")
            run_id = str(run.get("id", ""))
            head_sha = run.get("head_sha")
            run_number = run.get("run_number")
            if (
                not run_id.isdecimal()
                or not valid_commit_sha(head_sha)
                or not isinstance(run_number, int)
            ):
                raise ValueError("workflow run identity is malformed")
            if run_id in seen:
                continue
            seen.add(run_id)
            runs.append((run_number, run_id, head_sha))
    runs.sort(reverse=True)
    return [(run_id, head_sha) for _, run_id, head_sha in runs]


def has_release_asset(repository: str, run_id: str, runner: CommandRunner) -> bool:
    response = runner(
        [
            "gh",
            "api",
            f"/repos/{repository}/actions/runs/{run_id}/artifacts?per_page=100",
        ]
    )
    if response.returncode != 0:
        return False
    payload = json.loads(response.stdout)
    if not isinstance(payload, dict) or not isinstance(payload.get("artifacts"), list):
        raise ValueError("workflow artifact response must contain artifacts")
    return any(
        isinstance(artifact, dict)
        and artifact.get("name") == RELEASE_ASSET_NAME
        and artifact.get("expired") is False
        for artifact in payload["artifacts"]
    )


def select_reusable_run(
    pages: list[object],
    promotion_sha: str,
    repository: str,
    runner: CommandRunner,
) -> str | None:
    if not valid_commit_sha(promotion_sha):
        raise ValueError("promotion commit identity is malformed")
    for run_id, run_sha in ordered_runs(pages):
        fetched = runner(["git", "fetch", "--no-tags", "origin", run_sha])
        if fetched.returncode != 0:
            continue
        compatible = runner(
            [
                sys.executable,
                str(PROOF_INPUTS),
                "--base",
                run_sha,
                "--head",
                promotion_sha,
            ]
        )
        if compatible.returncode == 0 and has_release_asset(repository, run_id, runner):
            return run_id
    return None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repository", required=True)
    parser.add_argument("--promotion-sha", required=True)
    args = parser.parse_args()
    run_id = select_reusable_run(
        workflow_run_pages(args.repository, run_command),
        args.promotion_sha,
        args.repository,
        run_command,
    )
    if run_id is None:
        raise RuntimeError(
            "no unexpired clean optional-parser handoff matches the release inputs"
        )
    print(run_id)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
