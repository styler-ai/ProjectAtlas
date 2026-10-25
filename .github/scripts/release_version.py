#!/usr/bin/env python3
"""Classify ProjectAtlas stable and release-candidate versions for CI."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path


NUMBER = r"(?:0|[1-9][0-9]*)"
VERSION_PATTERN = re.compile(
    rf"(?P<prefix>v?)(?P<major>{NUMBER})\.(?P<minor>{NUMBER})\."
    rf"(?P<patch>{NUMBER})(?:-rc(?P<rc>[1-9][0-9]*))?"
)
DEVELOPMENT_PATTERN = re.compile(rf"{NUMBER}\.{NUMBER}\.{NUMBER}-dev\.{NUMBER}")


@dataclass(frozen=True)
class ReleaseVersion:
    """One validated stable or release-candidate version."""

    numbers: tuple[int, int, int]
    package_version: str
    tag: str
    stable_version: str
    stable_tag: str
    milestone: str
    is_prerelease: bool
    rc_number: int | None


def parse_release_version(value: str, *, source: str) -> ReleaseVersion:
    """Parse a release tag or workspace package version, failing closed."""
    if source not in {"release", "workspace"}:
        raise ValueError(f"unsupported version source: {source}")
    match = VERSION_PATTERN.fullmatch(value)
    expected_prefix = "v" if source == "release" else ""
    if match is None or match.group("prefix") != expected_prefix:
        expected = "vMAJOR.MINOR.PATCH[-rcN]" if source == "release" else "MAJOR.MINOR.PATCH[-rcN]"
        raise ValueError(f"version must match {expected} with canonical positive rcN")
    numbers = tuple(int(match.group(name)) for name in ("major", "minor", "patch"))
    stable_version = ".".join(str(part) for part in numbers)
    rc = match.group("rc")
    package_version = stable_version + (f"-rc{rc}" if rc else "")
    stable_tag = f"v{stable_version}"
    return ReleaseVersion(
        numbers=numbers,
        package_version=package_version,
        tag=f"v{package_version}",
        stable_version=stable_version,
        stable_tag=stable_tag,
        milestone=f"{stable_tag}-00",
        is_prerelease=rc is not None,
        rc_number=int(rc) if rc else None,
    )


def classify_workspace_version(value: str) -> ReleaseVersion | None:
    """Return an eligible release or ``None`` for the supported development form."""
    if DEVELOPMENT_PATTERN.fullmatch(value):
        return None
    return parse_release_version(value, source="workspace")


def github_outputs(version: ReleaseVersion | None) -> dict[str, str]:
    """Build validated GitHub Actions outputs for one classification."""
    if version is None:
        return {"eligible": "false"}
    return {
        "eligible": "true",
        "package_version": version.package_version,
        "tag": version.tag,
        "stable_version": version.stable_version,
        "stable_tag": version.stable_tag,
        "milestone": version.milestone,
        "is_prerelease": str(version.is_prerelease).lower(),
    }


def expected_latest_tag(
    version: ReleaseVersion, *, release_exists: bool, latest_before: str | None
) -> str:
    """Return the required post-publication Latest tag, failing closed for repairs."""
    if not version.is_prerelease and not release_exists:
        return version.tag
    if not latest_before:
        raise ValueError(
            "an RC or release repair requires the current Latest stable release"
        )
    latest = parse_release_version(latest_before, source="release")
    if latest.is_prerelease:
        raise ValueError(
            f"GitHub Latest is not a canonical stable ProjectAtlas tag: {latest_before}"
        )
    return latest.tag


def release_records(payload: object) -> list[dict[str, object]]:
    """Normalize one raw or ``gh api --paginate --slurp`` release response."""
    if not isinstance(payload, list):
        raise ValueError("GitHub releases response must be a list")
    if payload and all(isinstance(page, list) for page in payload):
        values = [record for page in payload for record in page]
    elif any(isinstance(page, list) for page in payload):
        raise ValueError("GitHub releases response mixes pages and records")
    else:
        values = payload
    if any(not isinstance(record, dict) for record in values):
        raise ValueError("GitHub releases response contains a non-object record")
    return values


def latest_published_rc(payload: object, stable: ReleaseVersion) -> ReleaseVersion | None:
    """Return the highest published RC for one stable base version."""
    if stable.is_prerelease:
        raise ValueError("prior RC lookup requires a stable release version")
    candidates: list[ReleaseVersion] = []
    for record in release_records(payload):
        tag = record.get("tag_name")
        draft = record.get("draft")
        prerelease = record.get("prerelease")
        if not isinstance(tag, str) or not isinstance(draft, bool) or not isinstance(
            prerelease, bool
        ):
            raise ValueError("GitHub release record has invalid tag or classification fields")
        try:
            candidate = parse_release_version(tag, source="release")
        except ValueError:
            continue
        if (
            not draft
            and prerelease
            and candidate.is_prerelease
            and candidate.stable_tag == stable.stable_tag
        ):
            candidates.append(candidate)
    return max(candidates, key=lambda candidate: candidate.rc_number or 0, default=None)


def require_prior_rc_ancestor(
    prior_rc: ReleaseVersion, head: str, *, repo: Path = Path(".")
) -> None:
    """Require the selected published RC to be an ancestor of the stable head."""
    result = subprocess.run(
        [
            "git",
            "merge-base",
            "--is-ancestor",
            f"{prior_rc.tag}^{{commit}}",
            f"{head}^{{commit}}",
        ],
        cwd=repo,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode == 0:
        return
    if result.returncode == 1:
        raise ValueError(
            f"published RC {prior_rc.tag} is not an ancestor of stable head {head}"
        )
    detail = result.stderr.strip() or result.stdout.strip() or "git merge-base failed"
    raise ValueError(f"could not verify published RC ancestry: {detail}")


def self_test() -> None:
    """Exercise generic stable, RC, development, and malformed inputs."""
    stable = parse_release_version("v1.2.3", source="release")
    assert stable.package_version == "1.2.3"
    assert stable.stable_tag == "v1.2.3"
    assert stable.milestone == "v1.2.3-00"
    assert stable.numbers == (1, 2, 3)
    assert not stable.is_prerelease
    assert stable.rc_number is None
    assert github_outputs(stable) == {
        "eligible": "true",
        "package_version": "1.2.3",
        "tag": "v1.2.3",
        "stable_version": "1.2.3",
        "stable_tag": "v1.2.3",
        "milestone": "v1.2.3-00",
        "is_prerelease": "false",
    }

    candidate = classify_workspace_version("10.20.30-rc12")
    assert candidate is not None
    assert candidate.tag == "v10.20.30-rc12"
    assert candidate.stable_version == "10.20.30"
    assert candidate.milestone == "v10.20.30-00"
    assert candidate.is_prerelease
    assert candidate.rc_number == 12
    assert classify_workspace_version("7.8.9-dev.0") is None
    assert classify_workspace_version("7.8.9-dev.12") is None
    assert github_outputs(None) == {"eligible": "false"}
    assert github_outputs(candidate)["is_prerelease"] == "true"
    assert (
        expected_latest_tag(stable, release_exists=False, latest_before=None)
        == stable.tag
    )
    assert (
        expected_latest_tag(candidate, release_exists=False, latest_before="v1.2.3")
        == "v1.2.3"
    )
    assert (
        expected_latest_tag(stable, release_exists=True, latest_before="v1.2.2")
        == "v1.2.2"
    )
    for target, release_exists, latest_before in (
        (candidate, False, None),
        (stable, True, None),
        (candidate, True, "v1.2.3-rc1"),
        (stable, True, "nightly"),
    ):
        try:
            expected_latest_tag(
                target,
                release_exists=release_exists,
                latest_before=latest_before,
            )
        except ValueError:
            pass
        else:
            raise AssertionError("invalid Latest release state was accepted")
    releases = [
        [
            {"tag_name": "v10.20.30-rc2", "draft": False, "prerelease": True},
            {"tag_name": "v10.20.30-rc9", "draft": False, "prerelease": True},
            {"tag_name": "v10.20.30-rc20", "draft": True, "prerelease": True},
        ],
        [
            {"tag_name": "v10.20.30", "draft": False, "prerelease": False},
            {"tag_name": "v10.20.31-rc99", "draft": False, "prerelease": True},
            {"tag_name": "nightly", "draft": False, "prerelease": True},
        ],
    ]
    prior = latest_published_rc(releases, parse_release_version("v10.20.30", source="release"))
    assert prior is not None and prior.tag == "v10.20.30-rc9"
    assert latest_published_rc([], stable) is None
    try:
        latest_published_rc(releases, candidate)
    except ValueError:
        pass
    else:
        raise AssertionError("prior RC lookup accepted an RC target")

    with tempfile.TemporaryDirectory(prefix="projectatlas-release-version-") as temp_dir:
        repo = Path(temp_dir)

        def git(*arguments: str) -> str:
            return subprocess.run(
                ["git", *arguments],
                cwd=repo,
                text=True,
                capture_output=True,
                check=True,
            ).stdout.strip()

        git("init", "--quiet")
        git("config", "user.name", "ProjectAtlas release self-test")
        git("config", "user.email", "release-self-test@example.invalid")
        git("config", "commit.gpgsign", "false")
        git("config", "tag.gpgsign", "false")
        git("config", "core.hooksPath", ".git/disabled-hooks")
        git("commit", "--quiet", "--allow-empty", "--message", "candidate")
        git("tag", prior.tag)
        git("commit", "--quiet", "--allow-empty", "--message", "stable")
        require_prior_rc_ancestor(prior, git("rev-parse", "HEAD"), repo=repo)
        git("checkout", "--quiet", "--orphan", "divergent")
        git("commit", "--quiet", "--allow-empty", "--message", "divergent")
        try:
            require_prior_rc_ancestor(prior, git("rev-parse", "HEAD"), repo=repo)
        except ValueError:
            pass
        else:
            raise AssertionError("divergent stable head accepted published RC ancestry")

    invalid = (
        ("1.2.3", "release"),
        ("v1.2.3", "workspace"),
        ("v01.2.3", "release"),
        ("v1.02.3", "release"),
        ("v1.2.03", "release"),
        ("v1.2.3-rc0", "release"),
        ("v1.2.3-rc01", "release"),
        ("v1.2.3-rc.1", "release"),
        ("v1.2.3-beta1", "release"),
        ("v1.2.3+build", "release"),
        ("v1.2.3\n", "release"),
    )
    for value, source in invalid:
        try:
            parse_release_version(value, source=source)
        except ValueError:
            continue
        raise AssertionError(f"invalid {source} version was accepted: {value!r}")
    for value in ("7.8.9.dev1", "7.8.9-dev", "7.8.9-dev.01", "v7.8.9-dev.1"):
        try:
            classify_workspace_version(value)
        except ValueError:
            continue
        raise AssertionError(f"invalid workspace development version was accepted: {value!r}")
    try:
        parse_release_version("v1.2.3", source="unknown")
    except ValueError:
        pass
    else:
        raise AssertionError("unknown version source was accepted")


def main() -> None:
    """Classify one version or run the bounded self-test."""
    parser = argparse.ArgumentParser()
    parser.add_argument("version", nargs="?")
    parser.add_argument("--source", choices=("release", "workspace"), default="release")
    parser.add_argument("--github-output", type=Path)
    parser.add_argument("--require-prior-rc-from", type=Path)
    parser.add_argument("--require-prior-rc-ancestor-of")
    parser.add_argument("--resolve-expected-latest", action="store_true")
    parser.add_argument("--latest-before")
    parser.add_argument("--release-exists", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        print("release version self-test passed")
        return
    if args.version is None:
        parser.error("version is required")
    try:
        version = (
            classify_workspace_version(args.version)
            if args.source == "workspace"
            else parse_release_version(args.version, source="release")
        )
    except ValueError as error:
        parser.error(str(error))
    outputs = github_outputs(version)
    if args.resolve_expected_latest:
        if version is None:
            parser.error("development versions cannot resolve release Latest state")
        try:
            outputs["expected_latest"] = expected_latest_tag(
                version,
                release_exists=args.release_exists,
                latest_before=args.latest_before,
            )
        except ValueError as error:
            parser.error(str(error))
    elif args.latest_before is not None or args.release_exists:
        parser.error("Latest-state inputs require --resolve-expected-latest")
    if (args.require_prior_rc_from is None) != (
        args.require_prior_rc_ancestor_of is None
    ):
        parser.error(
            "--require-prior-rc-from and --require-prior-rc-ancestor-of must be used together"
        )
    if args.require_prior_rc_from is not None:
        if version is None:
            parser.error("development versions cannot require a prior RC")
        try:
            payload = json.loads(args.require_prior_rc_from.read_text(encoding="utf-8"))
            prior_rc = latest_published_rc(payload, version)
        except (OSError, ValueError, json.JSONDecodeError) as error:
            parser.error(str(error))
        if prior_rc is None:
            parser.error(
                f"stable release {version.tag} requires a published prerelease for the same base version"
            )
        try:
            require_prior_rc_ancestor(prior_rc, args.require_prior_rc_ancestor_of)
        except (OSError, ValueError) as error:
            parser.error(str(error))
        outputs["prior_rc_tag"] = prior_rc.tag
    output_path = args.github_output or (Path(path) if (path := os.environ.get("GITHUB_OUTPUT")) else None)
    if output_path is not None:
        with output_path.open("a", encoding="utf-8", newline="\n") as output:
            for name, value in outputs.items():
                output.write(f"{name}={value}\n")
    for name, value in outputs.items():
        print(f"{name}={value}")


if __name__ == "__main__":
    main()
