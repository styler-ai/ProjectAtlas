#!/usr/bin/env python3
"""Decide whether optional-parser proof survives repository metadata changes."""

from __future__ import annotations

import argparse
import fnmatch
import json
import subprocess

METADATA_ONLY_PATTERNS = (
    ".github/ISSUE_TEMPLATE/**",
    ".github/pull_request_template.md",
    "AGENTS.md",
    "README.md",
    "docs/**",
    "openspec/**",
)


def classify_paths(paths: list[str]) -> dict[str, object]:
    normalized = sorted({path for path in paths if path})
    metadata_only = [
        path
        for path in normalized
        if any(fnmatch.fnmatchcase(path, pattern) for pattern in METADATA_ONLY_PATTERNS)
    ]
    invalidating = [path for path in normalized if path not in metadata_only]
    return {
        "reusable": not invalidating,
        "metadata_only": metadata_only,
        "invalidating": invalidating,
    }


def changed_paths(base: str, head: str) -> list[str]:
    process = subprocess.run(
        [
            "git",
            "diff",
            "--no-renames",
            "--name-only",
            "--diff-filter=ACDMRTUXB",
            "-z",
            base,
            head,
            "--",
        ],
        check=False,
        capture_output=True,
        timeout=120,
    )
    if process.returncode != 0:
        raise RuntimeError(process.stderr.decode("utf-8", errors="replace").strip())
    return [
        path.decode("utf-8", errors="strict")
        for path in process.stdout.split(b"\0")
        if path
    ]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    args = parser.parse_args()
    result = classify_paths(changed_paths(args.base, args.head))
    print(json.dumps(result, separators=(",", ":"), sort_keys=True))
    return 0 if result["reusable"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
