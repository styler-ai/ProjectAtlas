#!/usr/bin/env python3
"""Verify committed benchmark publications retain their closed input locks."""

from __future__ import annotations

import ast
import hashlib
import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PUBLICATIONS = (
    (
        "system-scale",
        "docs/benchmarks/harness/system_scale.py",
        "SYSTEM_SCALE_MEASUREMENT_INPUTS",
        "docs/benchmarks/v0.4-system-scale-preregistration.json",
        "docs/benchmarks/v0.4-system-scale-results.json",
    ),
    (
        "agent-navigation",
        "docs/benchmarks/harness/agent_navigation.py",
        "AGENT_NAVIGATION_MEASUREMENT_INPUTS",
        "docs/benchmarks/v0.4-agent-navigation-preregistration.json",
        "docs/benchmarks/v0.4-agent-navigation-results.json",
    ),
)
CANDIDATE_INPUT_PATHS = (
    ".cargo",
    "Cargo.lock",
    "Cargo.toml",
    "crates",
    "plugins/projectatlas",
    "rust-toolchain.toml",
)


def committed_object(relative: str, object_type: str) -> bytes | None:
    """Return one committed Git blob or tree without reading working-tree bytes."""

    process = subprocess.run(
        ["git", "cat-file", object_type, f"HEAD:{relative}"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        timeout=120,
    )
    return process.stdout if process.returncode == 0 else None


def required_paths(harness: str, constant: str) -> tuple[str, ...]:
    """Read one literal measurement-owner tuple from the committed harness."""

    payload = committed_object(harness, "blob")
    if payload is None:
        raise ValueError(f"committed harness is missing: {harness}")
    module = ast.parse(payload.decode("utf-8"), filename=harness)
    for node in module.body:
        if not isinstance(node, ast.Assign):
            continue
        if any(
            isinstance(target, ast.Name) and target.id == constant
            for target in node.targets
        ):
            value = ast.literal_eval(node.value)
            if isinstance(value, tuple) and all(
                isinstance(path, str) for path in value
            ):
                return value
            break
    raise ValueError(f"{constant} must remain a literal tuple of paths")


def input_lock_status(
    required: tuple[str, ...],
    locked: object,
    actual_digests: dict[str, str | None],
) -> tuple[list[str], list[str]]:
    """Separate publication corruption from ordinary input invalidation."""

    if not isinstance(locked, dict):
        return ["measurement input lock is not an object"], []
    if set(locked) != set(required):
        return [], ["measurement input path set changed"]

    errors: list[str] = []
    historical: list[str] = []
    for relative in required:
        expected = locked.get(relative)
        if (
            not isinstance(expected, str)
            or len(expected) != 64
            or any(character not in "0123456789abcdef" for character in expected)
        ):
            errors.append(f"malformed digest for {relative}")
            continue
        actual = actual_digests.get(relative)
        if actual is None:
            historical.append(f"committed measurement input is missing: {relative}")
        elif actual != expected:
            historical.append(f"committed measurement input changed: {relative}")
    return errors, historical


def candidate_source_revision(source_identity: object) -> str | None:
    """Return the measured source revision when its provenance is usable."""

    if not isinstance(source_identity, dict):
        return None
    revision = source_identity.get("checkout_head")
    if (
        not isinstance(revision, str)
        or len(revision) != 40
        or any(character not in "0123456789abcdef" for character in revision)
    ):
        return None
    return revision


def candidate_input_status(source_identity: object) -> tuple[str | None, str | None]:
    """Compare candidate-owned inputs without requiring commit equality."""

    revision = candidate_source_revision(source_identity)
    if revision is None:
        return "unavailable", "candidate source identity is missing or malformed"

    present = subprocess.run(
        ["git", "cat-file", "-e", f"{revision}^{{commit}}"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        timeout=120,
    )
    if present.returncode != 0:
        fetched = subprocess.run(
            ["git", "fetch", "--no-tags", "--depth=1", "origin", revision],
            cwd=ROOT,
            check=False,
            capture_output=True,
            timeout=120,
        )
        if fetched.returncode != 0:
            return "unavailable", "measured candidate source is unavailable"

    compared = subprocess.run(
        ["git", "diff", "--quiet", revision, "HEAD", "--", *CANDIDATE_INPUT_PATHS],
        cwd=ROOT,
        check=False,
        capture_output=True,
        timeout=120,
    )
    if compared.returncode == 1:
        return "historical", "candidate runtime, MCP, skill, or plugin inputs changed"
    if compared.returncode != 0:
        return "unavailable", "candidate-owned inputs could not be compared"
    return None, None


def publication_status(
    label: str,
    harness: str,
    constant: str,
    preregistration_path: str,
    result_path: str,
) -> tuple[list[str], str]:
    """Return integrity failures and the candidate-relative publication status."""

    errors: list[str] = []
    preregistration_payload = committed_object(preregistration_path, "blob")
    result_payload = committed_object(result_path, "blob")
    if preregistration_payload is None:
        return [], f"{label}: unavailable (committed preregistration is missing)"
    if result_payload is None:
        return [], f"{label}: unavailable (committed result is missing)"
    try:
        preregistration = json.loads(preregistration_payload)
        result = json.loads(result_payload)
    except json.JSONDecodeError as error:
        return [f"{label}: committed publication JSON is invalid: {error}"], ""

    try:
        required = required_paths(harness, constant)
    except (SyntaxError, UnicodeDecodeError, ValueError) as error:
        return [], f"{label}: unavailable ({error})"
    if not isinstance(preregistration, dict) or not isinstance(result, dict):
        return [f"{label}: committed publication roots must be objects"], ""

    locked = preregistration.get("measurement_inputs")
    effective = result.get("effective_preregistration")
    if effective != preregistration:
        errors.append(f"{label}: published result has a stale effective preregistration")

    recorded_preregistration_digest = result.get("preregistration_sha256")
    if recorded_preregistration_digest is not None:
        actual_preregistration_digest = hashlib.sha256(
            preregistration_payload
        ).hexdigest()
        if recorded_preregistration_digest != actual_preregistration_digest:
            errors.append(f"{label}: published preregistration digest is stale")

    actual_digests: dict[str, str | None] = {}
    for relative in required:
        actual_payload = committed_object(relative, "blob")
        if actual_payload is None:
            actual_payload = committed_object(relative, "tree")
        actual_digests[relative] = (
            None if actual_payload is None else hashlib.sha256(actual_payload).hexdigest()
        )
    lock_errors, historical = input_lock_status(required, locked, actual_digests)
    errors.extend(f"{label}: {error}" for error in lock_errors)
    if errors:
        return errors, ""

    candidate_state, candidate_reason = candidate_input_status(
        result.get("candidate_source_identity")
    )
    if candidate_state == "unavailable":
        return [], f"{label}: unavailable ({candidate_reason})"
    if candidate_state == "historical" and candidate_reason is not None:
        historical.append(candidate_reason)
    if historical:
        return [], f"{label}: historical ({'; '.join(historical)})"
    return [], f"{label}: eligible (measurement and candidate inputs match)"


def self_test() -> None:
    """Keep input invalidation nonblocking without accepting corrupt locks."""

    required = ("harness.py", "fixtures")
    locked = {"harness.py": "a" * 64, "fixtures": "b" * 64}
    errors, historical = input_lock_status(
        required, locked, {"harness.py": "a" * 64, "fixtures": "b" * 64}
    )
    assert not errors and not historical
    errors, historical = input_lock_status(
        required, locked, {"harness.py": "c" * 64, "fixtures": "b" * 64}
    )
    assert not errors and historical
    errors, historical = input_lock_status(
        required, {"harness.py": "a" * 64}, {"harness.py": "a" * 64}
    )
    assert not errors and historical
    errors, historical = input_lock_status(
        required,
        {"harness.py": "not-a-digest", "fixtures": "b" * 64},
        {"harness.py": None, "fixtures": "b" * 64},
    )
    assert errors and not historical
    assert candidate_source_revision({"checkout_head": "a" * 40}) == "a" * 40
    assert candidate_source_revision({"checkout_head": "not-a-revision"}) is None
    assert candidate_source_revision(None) is None


def main() -> int:
    """Verify both published campaigns without executing either campaign."""

    self_test()
    outcomes = [publication_status(*publication) for publication in PUBLICATIONS]
    errors = [error for publication_errors, _ in outcomes for error in publication_errors]
    statuses = [status for _, status in outcomes if status]
    if statuses:
        print("\n".join(statuses))
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
