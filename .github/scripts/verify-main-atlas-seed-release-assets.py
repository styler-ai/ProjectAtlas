#!/usr/bin/env python3
"""Discover an optional exact-version ProjectAtlas main Atlas seed asset pair."""

from __future__ import annotations

import argparse
import filecmp
import re
import tempfile
from pathlib import Path

from release_version import parse_release_version


ASSET_PREFIX = "projectatlas-main-atlas-seed-"
SNAPSHOT_DIGEST = r"[0-9a-f]{64}"


def seed_candidates(source: Path) -> list[Path]:
    """Return the bounded regular seed-prefixed inventory."""

    candidates = sorted(
        (entry for entry in source.iterdir() if entry.name.startswith(ASSET_PREFIX)),
        key=lambda entry: entry.name,
    )
    if not candidates:
        return []
    if any(not entry.is_file() or entry.is_symlink() for entry in candidates):
        raise ValueError("main Atlas seed assets must be regular, non-symlink files")
    return candidates


def discover_seed_assets(source: Path, release_tag: str) -> list[Path]:
    """Return one validated archive/manifest pair, or an empty list when absent."""
    version = parse_release_version(release_tag, source="release")
    candidates = seed_candidates(source)
    if not candidates:
        return []

    basename_pattern = re.compile(
        rf"{re.escape(ASSET_PREFIX + version.tag)}-(?P<digest>{SNAPSHOT_DIGEST})"
    )
    pairs: dict[str, set[str]] = {}
    for candidate in candidates:
        suffix = next(
            (value for value in (".tar.zst", ".manifest.json") if candidate.name.endswith(value)),
            None,
        )
        if suffix is None:
            raise ValueError(f"unexpected main Atlas seed asset: {candidate.name}")
        basename = candidate.name[: -len(suffix)]
        match = basename_pattern.fullmatch(basename)
        if match is None:
            raise ValueError(
                f"main Atlas seed asset does not preserve exact release tag {version.tag}: "
                f"{candidate.name}"
            )
        pairs.setdefault(match.group("digest"), set()).add(suffix)

    if len(pairs) != 1:
        raise ValueError("main Atlas seed assets must contain exactly one snapshot digest")
    digest, suffixes = next(iter(pairs.items()))
    expected_suffixes = {".tar.zst", ".manifest.json"}
    if suffixes != expected_suffixes or len(candidates) != 2:
        raise ValueError("main Atlas seed archive and manifest must form one complete pair")
    basename = f"{ASSET_PREFIX}{version.tag}-{digest}"
    return [source / f"{basename}{suffix}" for suffix in (".tar.zst", ".manifest.json")]


def validate_hosted_seed_assets(
    hosted: Path, staged: Path, release_tag: str
) -> list[Path]:
    """Validate immutable hosted assets, including one-member upload recovery."""

    hosted_candidates = seed_candidates(hosted)
    if not hosted_candidates:
        return []
    staged_assets = discover_seed_assets(staged, release_tag)
    if len(hosted_candidates) == 1:
        if not staged_assets:
            raise ValueError(
                "a partial hosted main Atlas seed requires the complete staged pair"
            )
        staged_by_name = {asset.name: asset for asset in staged_assets}
        candidate = hosted_candidates[0]
        staged_candidate = staged_by_name.get(candidate.name)
        if staged_candidate is None or not filecmp.cmp(
            candidate, staged_candidate, shallow=False
        ):
            raise ValueError(
                f"partial hosted main Atlas seed asset differs: {candidate.name}"
            )
        return hosted_candidates

    hosted_assets = discover_seed_assets(hosted, release_tag)
    if staged_assets:
        staged_by_name = {asset.name: asset for asset in staged_assets}
        if [asset.name for asset in hosted_assets] != [
            asset.name for asset in staged_assets
        ] or any(
            not filecmp.cmp(asset, staged_by_name[asset.name], shallow=False)
            for asset in hosted_assets
        ):
            raise ValueError("hosted main Atlas seed pair differs from the staged pair")
    return hosted_assets


def self_test() -> None:
    """Cover absence, stable/RC identity, and malformed inventory failure."""
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        assert discover_seed_assets(root, "v1.2.3") == []

        digest = "a" * 64
        stable_basename = f"{ASSET_PREFIX}v1.2.3-{digest}"
        for suffix in (".tar.zst", ".manifest.json"):
            (root / f"{stable_basename}{suffix}").write_bytes(suffix.encode("ascii"))
        assert [path.name for path in discover_seed_assets(root, "v1.2.3")] == [
            f"{stable_basename}.tar.zst",
            f"{stable_basename}.manifest.json",
        ]
        try:
            discover_seed_assets(root, "v1.2.3-rc1")
        except ValueError:
            pass
        else:
            raise AssertionError("stable seed assets were accepted for an RC")

        for path in tuple(root.iterdir()):
            path.unlink()
        rc_basename = f"{ASSET_PREFIX}v9.8.7-rc12-{'b' * 64}"
        archive = root / f"{rc_basename}.tar.zst"
        manifest = root / f"{rc_basename}.manifest.json"
        archive.write_bytes(b"archive")
        manifest.write_text("{}\n", encoding="utf-8")
        assert len(discover_seed_assets(root, "v9.8.7-rc12")) == 2

        manifest.unlink()
        try:
            discover_seed_assets(root, "v9.8.7-rc12")
        except ValueError:
            pass
        else:
            raise AssertionError("incomplete seed asset pair was accepted")

        archive.unlink()
        malformed = root / f"{ASSET_PREFIX}v9.8.7-rc12-not-a-digest.tar.zst"
        malformed.write_bytes(b"archive")
        try:
            discover_seed_assets(root, "v9.8.7-rc12")
        except ValueError:
            pass
        else:
            raise AssertionError("malformed seed asset was accepted")

        malformed.unlink()
        for suffix in (".tar.zst", ".manifest.json"):
            (root / f"{rc_basename}{suffix}").write_bytes(b"pair")
        second_basename = f"{ASSET_PREFIX}v9.8.7-rc12-{'c' * 64}"
        for suffix in (".tar.zst", ".manifest.json"):
            (root / f"{second_basename}{suffix}").write_bytes(b"second")
        try:
            discover_seed_assets(root, "v9.8.7-rc12")
        except ValueError:
            pass
        else:
            raise AssertionError("multiple seed snapshot digests were accepted")

        for path in tuple(root.iterdir()):
            path.unlink()
        non_regular = root / f"{ASSET_PREFIX}v9.8.7-rc12-{'d' * 64}.tar.zst"
        non_regular.mkdir()
        try:
            discover_seed_assets(root, "v9.8.7-rc12")
        except ValueError:
            pass
        else:
            raise AssertionError("non-regular seed asset was accepted")

    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        staged = root / "staged"
        hosted = root / "hosted"
        missing_staged = root / "missing-staged"
        staged.mkdir()
        hosted.mkdir()
        missing_staged.mkdir()
        basename = f"{ASSET_PREFIX}v9.8.7-rc12-{'e' * 64}"
        for suffix in (".tar.zst", ".manifest.json"):
            (staged / f"{basename}{suffix}").write_bytes(suffix.encode("ascii"))
        hosted_archive = hosted / f"{basename}.tar.zst"
        hosted_archive.write_bytes(b".tar.zst")
        try:
            validate_hosted_seed_assets(
                hosted, missing_staged, "v9.8.7-rc12"
            )
        except ValueError:
            pass
        else:
            raise AssertionError("partial hosted seed was accepted without a staged pair")
        assert [asset.name for asset in validate_hosted_seed_assets(
            hosted, staged, "v9.8.7-rc12"
        )] == [hosted_archive.name]
        hosted_archive.write_bytes(b"mismatch")
        try:
            validate_hosted_seed_assets(hosted, staged, "v9.8.7-rc12")
        except ValueError:
            pass
        else:
            raise AssertionError("mismatched partial hosted seed asset was accepted")
        hosted_archive.write_bytes(b".tar.zst")
        hosted_manifest = hosted / f"{basename}.manifest.json"
        hosted_manifest.write_bytes(b".manifest.json")
        assert len(validate_hosted_seed_assets(hosted, staged, "v9.8.7-rc12")) == 2
        hosted_manifest.write_bytes(b"mismatch")
        try:
            validate_hosted_seed_assets(hosted, staged, "v9.8.7-rc12")
        except ValueError:
            pass
        else:
            raise AssertionError("mismatched hosted seed pair was accepted")


def main() -> None:
    """Validate optional seed assets and write their leaf names when requested."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path)
    parser.add_argument("--release-tag")
    parser.add_argument("--list-file", type=Path)
    parser.add_argument("--staged-source", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        print("main Atlas seed release-asset self-test passed")
        return
    if args.source is None or args.release_tag is None:
        parser.error("--source and --release-tag are required")
    try:
        assets = (
            validate_hosted_seed_assets(
                args.source, args.staged_source, args.release_tag
            )
            if args.staged_source is not None
            else discover_seed_assets(args.source, args.release_tag)
        )
    except (OSError, ValueError) as error:
        parser.error(str(error))
    names = [asset.name for asset in assets]
    if args.list_file is not None:
        args.list_file.write_text("".join(f"{name}\n" for name in names), encoding="utf-8")
    for name in names:
        print(name)


if __name__ == "__main__":
    main()
