#!/usr/bin/env python3
"""Validate and stage an exact clean optional-parser release handoff."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import tempfile
from pathlib import Path
from typing import Any


TARGETS = (
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
)
PACK_ID = "broad-parser"
PROOF_NAME = "optional-parser-pack-proof-aggregate.json"


def digest(path: Path) -> str:
    """Return one file's lowercase SHA-256."""
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_object(path: Path) -> dict[str, Any]:
    """Load one JSON object or fail closed."""
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path.name} must contain a JSON object")
    return value


def require(condition: bool, message: str) -> None:
    """Raise one bounded validation error."""
    if not condition:
        raise ValueError(message)


def stage_release_assets(
    source: Path,
    output: Path,
    release_version: str,
    revision: str,
    cargo_lock: Path,
) -> list[Path]:
    """Validate a clean handoff and stage versioned release assets."""
    require(
        re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", release_version) is not None,
        "release version must match vMAJOR.MINOR.PATCH",
    )
    require(re.fullmatch(r"[0-9a-f]{40}", revision) is not None, "invalid run revision")
    expected_files = {PROOF_NAME}
    for target in TARGETS:
        expected_files.add(f"projectatlas-{PACK_ID}-{target}.tar.zst")
        expected_files.add(f"cargo-layer-{target}.json")
    source_entries = list(source.iterdir())
    require(
        all(path.is_file() and not path.is_symlink() for path in source_entries),
        "clean handoff contains a non-regular file",
    )
    actual_files = {path.name for path in source_entries}
    require(actual_files == expected_files, "clean handoff file inventory differs")

    proof = load_object(source / PROOF_NAME)
    version = release_version.removeprefix("v")
    require(proof.get("schema_version") == 2, "unsupported aggregate proof schema")
    require(proof.get("pack_id") == PACK_ID, "aggregate proof pack differs")
    require(proof.get("projectatlas_version") == version, "aggregate proof version differs")
    platforms = proof.get("platforms")
    require(isinstance(platforms, list), "aggregate proof platforms must be a list")
    require(
        [platform.get("platform") for platform in platforms if isinstance(platform, dict)]
        == list(TARGETS),
        "aggregate proof supported target set or order differs",
    )
    cargo_lock_sha256 = digest(cargo_lock)
    common_digests = (
        "accepted_manifest_sha256",
        "capability_set_digest",
        "fixture_corpus_sha256",
    )

    staged: list[Path] = []
    output.mkdir(parents=True, exist_ok=True)
    require(not any(output.iterdir()), "release output directory must be empty")
    for target, platform in zip(TARGETS, platforms, strict=True):
        require(platform.get("schema_version") == 2, f"{target} proof schema differs")
        require(platform.get("pack_id") == PACK_ID, f"{target} pack differs")
        candidate = platform.get("candidate")
        require(isinstance(candidate, dict), f"{target} candidate proof is missing")
        require(
            candidate.get("projectatlas_revision") == revision,
            f"{target} candidate revision differs",
        )
        require(candidate.get("cargo_package_version") == version, f"{target} Cargo version differs")
        require(
            candidate.get("intended_release_version") == version,
            f"{target} intended release differs",
        )
        require(candidate.get("source_state") == "clean", f"{target} source was not clean")
        require(
            candidate.get("cargo_lock_sha256") == cargo_lock_sha256,
            f"{target} Cargo.lock digest differs",
        )
        for field in common_digests:
            require(
                platform.get(field) == proof.get(field),
                f"{target} {field} differs from aggregate proof",
            )

        archive_name = f"projectatlas-{PACK_ID}-{target}.tar.zst"
        archive = source / archive_name
        require(platform.get("archive_name") == archive_name, f"{target} archive name differs")
        require(platform.get("archive_bytes") == archive.stat().st_size, f"{target} archive size differs")
        require(platform.get("archive_sha256") == digest(archive), f"{target} archive digest differs")

        runner = platform.get("runner")
        require(isinstance(runner, dict), f"{target} fresh-runner proof is missing")
        for field in (
            "fresh_host",
            "repository_inputs_absent",
            "build_tools_not_invoked",
            "working_directory_outside_pack",
            "ambient_library_paths_cleared",
        ):
            require(runner.get(field) == "verified", f"{target} runner {field} is not verified")
        network = runner.get("network_denial")
        require(isinstance(network, dict), f"{target} network-denial proof is missing")
        require(
            all(network.get(field) is True for field in ("dns_denied", "direct_tcp_denied", "https_denied")),
            f"{target} network denial is incomplete",
        )
        grammars = platform.get("grammars")
        require(
            isinstance(grammars, list)
            and grammars
            and all(isinstance(grammar, dict) and grammar.get("worker_probe_passed") is True for grammar in grammars),
            f"{target} grammar probe proof is incomplete",
        )
        memory = platform.get("memory")
        require(
            isinstance(memory, dict)
            and memory.get("limit_enforced") == "verified"
            and memory.get("process_tree_cleaned") == "verified",
            f"{target} memory/process-tree proof is incomplete",
        )

        receipt = load_object(source / f"cargo-layer-{target}.json")
        require(receipt.get("schema_version") == 1, f"{target} receipt schema differs")
        require(receipt.get("target") == target, f"{target} receipt target differs")
        require(receipt.get("revision") == revision, f"{target} receipt revision differs")
        require(receipt.get("disposition") == "clean", f"{target} construction was not clean")
        require(receipt.get("restore_attempted") is False, f"{target} attempted cache restore")
        require(receipt.get("exact_hit") is False, f"{target} used an exact cache hit")
        require(receipt.get("compatible_hit") is False, f"{target} used a compatible cache hit")
        require(receipt.get("restored_entries") == 0, f"{target} restored cache entries")
        require(receipt.get("restored_bytes") == 0, f"{target} restored cache bytes")
        require(receipt.get("save_eligible") is False, f"{target} clean run became save eligible")

        destination = output / f"projectatlas-{release_version}-{PACK_ID}-{target}.tar.zst"
        shutil.copyfile(archive, destination)
        staged.append(destination)

    proof_destination = output / f"projectatlas-{release_version}-optional-parser-pack-proof.json"
    shutil.copyfile(source / PROOF_NAME, proof_destination)
    staged.append(proof_destination)
    return staged


def self_test() -> None:
    """Exercise success and tamper rejection without external dependencies."""
    with tempfile.TemporaryDirectory(prefix="projectatlas-parser-release-") as temporary:
        root = Path(temporary)
        source = root / "source"
        output = root / "output"
        source.mkdir()
        cargo_lock = root / "Cargo.lock"
        cargo_lock.write_bytes(b"locked\n")
        revision = "a" * 40
        common = {
            "accepted_manifest_sha256": "b" * 64,
            "capability_set_digest": "c" * 64,
            "fixture_corpus_sha256": "d" * 64,
        }
        platforms = []
        for target in TARGETS:
            archive_name = f"projectatlas-{PACK_ID}-{target}.tar.zst"
            archive = source / archive_name
            archive.write_bytes(target.encode("utf-8"))
            platforms.append(
                {
                    "schema_version": 2,
                    "pack_id": PACK_ID,
                    "platform": target,
                    "candidate": {
                        "projectatlas_revision": revision,
                        "cargo_package_version": "0.4.0",
                        "intended_release_version": "0.4.0",
                        "cargo_lock_sha256": digest(cargo_lock),
                        "source_state": "clean",
                    },
                    "archive_name": archive_name,
                    "archive_sha256": digest(archive),
                    "archive_bytes": archive.stat().st_size,
                    **common,
                    "runner": {
                        "fresh_host": "verified",
                        "repository_inputs_absent": "verified",
                        "build_tools_not_invoked": "verified",
                        "working_directory_outside_pack": "verified",
                        "ambient_library_paths_cleared": "verified",
                        "network_denial": {
                            "dns_denied": True,
                            "direct_tcp_denied": True,
                            "https_denied": True,
                        },
                    },
                    "grammars": [{"language_id": "fixture", "worker_probe_passed": True}],
                    "memory": {
                        "limit_enforced": "verified",
                        "process_tree_cleaned": "verified",
                    },
                }
            )
            (source / f"cargo-layer-{target}.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "target": target,
                        "revision": revision,
                        "disposition": "clean",
                        "restore_attempted": False,
                        "exact_hit": False,
                        "compatible_hit": False,
                        "restored_entries": 0,
                        "restored_bytes": 0,
                        "save_eligible": False,
                    }
                ),
                encoding="utf-8",
            )
        (source / PROOF_NAME).write_text(
            json.dumps(
                {
                    "schema_version": 2,
                    "pack_id": PACK_ID,
                    "projectatlas_version": "0.4.0",
                    **common,
                    "platforms": platforms,
                }
            ),
            encoding="utf-8",
        )
        staged = stage_release_assets(source, output, "v0.4.0", revision, cargo_lock)
        require(len(staged) == 3 and all(path.is_file() for path in staged), "self-test staging failed")
        (source / f"projectatlas-{PACK_ID}-{TARGETS[0]}.tar.zst").write_bytes(b"tampered")
        try:
            stage_release_assets(source, root / "tampered-output", "v0.4.0", revision, cargo_lock)
        except ValueError:
            return
        raise RuntimeError("tampered archive passed validation")


def main() -> None:
    """Run validation or its bounded self-test."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--release-version")
    parser.add_argument("--revision")
    parser.add_argument("--cargo-lock", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        print("Optional-parser release-asset self-test passed")
        return
    if None in (args.source, args.output, args.release_version, args.revision, args.cargo_lock):
        parser.error("validation requires source, output, release-version, revision, and cargo-lock")
    staged = stage_release_assets(
        args.source,
        args.output,
        args.release_version,
        args.revision,
        args.cargo_lock,
    )
    for path in staged:
        print(path)


if __name__ == "__main__":
    main()
