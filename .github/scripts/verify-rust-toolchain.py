#!/usr/bin/env python3
"""Verify the repository-owned Rust toolchain before build or release work."""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import shutil
import stat
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from unittest.mock import patch


CHANNEL_RE = re.compile(r'^\s*channel\s*=\s*"([^"]+)"\s*$', re.MULTILINE)
VERSION_RE = re.compile(r"^\d+\.\d+\.\d+$")
TARGET_TRIPLE_RE = re.compile(
    r"^(?:aarch64|x86_64)-(?:unknown-linux-(?:gnu|musl)|"
    r"pc-windows-(?:gnu|msvc)|apple-darwin)$"
)
RELEASE_RE = re.compile(r"(?:^|\n)release:\s*(\d+\.\d+\.\d+)")
COMMIT_RE = re.compile(r"\(([0-9a-f]{10,40})\s")
PROXY_DIGEST_CHUNK_SIZE = 1024 * 1024


class PreflightError(RuntimeError):
    """Raised for an invalid repository or toolchain identity."""


@dataclass(frozen=True)
class ToolEvidence:
    """Observed identity for one Rust tool."""

    name: str
    path: str | None
    output: str | None
    release: str | None = None
    commit: str | None = None


def read_declared_channel(toolchain_file: Path) -> str:
    """Read the one exact numeric channel from ``rust-toolchain.toml``."""

    try:
        text = toolchain_file.read_text(encoding="utf-8")
    except OSError as error:
        raise PreflightError(f"cannot read {toolchain_file}: {error}") from error
    channels = CHANNEL_RE.findall(text)
    if len(channels) != 1:
        raise PreflightError(
            f"{toolchain_file} must contain exactly one channel assignment"
        )
    channel = channels[0]
    if not VERSION_RE.fullmatch(channel):
        raise PreflightError(
            f"{toolchain_file} must pin an exact numeric version, got {channel!r}"
        )
    return channel


def version_matches(expected: str, actual: str | None) -> bool:
    """Return whether a rustup name is the exact release or its target triple."""

    if actual == expected:
        return True
    if not actual or not actual.startswith(f"{expected}-"):
        return False
    return bool(TARGET_TRIPLE_RE.fullmatch(actual[len(expected) + 1 :]))


def same_directory(left: str | Path, right: str | Path) -> bool:
    """Compare identity, falling back only for missing/non-directory paths."""

    left_path = Path(left)
    right_path = Path(right)
    try:
        return left_path.samefile(right_path)
    except (FileNotFoundError, NotADirectoryError):
        # Missing/non-directory paths are retained for deterministic unit-test
        # and diagnostic comparisons; existing paths must use filesystem identity.
        pass
    except (PermissionError, OSError, ValueError, RuntimeError):
        return False

    try:
        left_path = os.path.normcase(str(left_path.resolve(strict=False)))
        right_path = os.path.normcase(str(right_path.resolve(strict=False)))
    except (OSError, ValueError, RuntimeError):
        return False
    return left_path == right_path


def executable_digest(path: Path) -> tuple[int, bytes] | None:
    """Return a bounded size/digest identity for one regular executable file."""

    try:
        metadata = path.stat()
        if not stat.S_ISREG(metadata.st_mode):
            return None
        digest = hashlib.sha256()
        remaining = metadata.st_size
        with path.open("rb") as executable:
            while remaining:
                chunk = executable.read(min(PROXY_DIGEST_CHUNK_SIZE, remaining))
                if not chunk:
                    return None
                digest.update(chunk)
                remaining -= len(chunk)
        if path.stat().st_size != metadata.st_size:
            return None
        return metadata.st_size, digest.digest()
    except (OSError, ValueError, RuntimeError):
        return None


def inventory_contains(expected: str, output: str | None) -> bool:
    """Return whether a non-installing Rustup inventory lists the expected toolchain."""

    if output is None:
        return False
    return any(
        version_matches(expected, line.split(maxsplit=1)[0])
        for line in output.splitlines()
        if line.split(maxsplit=1)
    )


def commits_match(first: str | None, second: str | None) -> bool:
    """Return whether two full or abbreviated commit identities agree."""

    return bool(first and second) and (
        first.startswith(second) or second.startswith(first)
    )


def executable_error(name: str, path: str | None, proxy_directory: Path) -> str | None:
    """Return a missing or out-of-proxy diagnostic for one executable path."""

    if path is None:
        return f"{name} is missing from PATH"
    if name == "rustup":
        return None
    if not same_directory(Path(path).parent, proxy_directory):
        return (
            f"{name} is selected at {path!r} outside rustup proxy directory "
            f"{str(proxy_directory)!r}"
        )
    return None


def evaluate(
    expected: str,
    *,
    rustup_available: bool,
    rustup_error: str | None = None,
    active_toolchain: str | None,
    override: str | None,
    proxy_directory: Path,
    evidence: tuple[ToolEvidence, ...],
) -> list[str]:
    """Return every mismatch so the report includes expected and actual facts."""

    errors: list[str] = []
    if not rustup_available:
        errors.append("rustup is missing from PATH")
    elif rustup_error:
        errors.append(rustup_error)
    if override and not version_matches(expected, override):
        errors.append(
            f"RUSTUP_TOOLCHAIN override is {override!r}; expected {expected!r}"
        )
    if not version_matches(expected, active_toolchain):
        errors.append(
            f"active rustup toolchain is {active_toolchain or '<unavailable>'!r}; "
            f"expected {expected!r}"
        )

    for tool in evidence:
        path_error = executable_error(tool.name, tool.path, proxy_directory)
        if path_error:
            errors.append(path_error)
        if tool.output is None:
            errors.append(f"{tool.name} did not report an identity")
    rustc = next((tool for tool in evidence if tool.name == "rustc"), None)
    cargo = next((tool for tool in evidence if tool.name == "cargo"), None)
    clippy = next((tool for tool in evidence if tool.name == "clippy"), None)
    rustfmt = next((tool for tool in evidence if tool.name == "rustfmt"), None)
    if rustc and rustc.release != expected:
        errors.append(
            f"rustc release is {rustc.release or '<unavailable>'!r}; expected {expected!r}"
        )
    if cargo and cargo.release != expected:
        errors.append(
            f"cargo release is {cargo.release or '<unavailable>'!r}; expected {expected!r}"
        )
    expected_clippy = f"0.1.{expected.split('.')[1]}"
    if clippy and clippy.release != expected_clippy:
        errors.append(
            f"clippy release is {clippy.release or '<unavailable>'!r}; "
            f"expected {expected_clippy!r} for Rust {expected}"
        )
    if rustc and rustfmt and rustc.commit and rustfmt.commit:
        if not commits_match(rustc.commit, rustfmt.commit):
            errors.append(
                f"rustfmt commit {rustfmt.commit!r} does not match rustc commit "
                f"{rustc.commit!r}"
            )
    elif rustfmt:
        errors.append("rustfmt did not report a commit identity")
    if clippy and not clippy.commit:
        errors.append("clippy did not report a commit identity")
    elif rustc and clippy and rustc.commit and clippy.commit:
        if not commits_match(rustc.commit, clippy.commit):
            errors.append(
                f"clippy commit {clippy.commit!r} does not match rustc commit "
                f"{rustc.commit!r}"
            )
    return errors


def run(
    command: list[str], root: Path, *, timeout: int = 20
) -> subprocess.CompletedProcess[str] | None:
    """Run one bounded identity probe without mutating repository state."""

    try:
        return subprocess.run(
            command,
            cwd=root,
            check=False,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None


def output_of(result: subprocess.CompletedProcess[str] | None) -> str | None:
    """Return combined command output when the probe completed successfully."""

    if result is None or result.returncode != 0:
        return None
    return f"{result.stdout}{result.stderr}".strip()


def evidence_for(
    name: str, path: str | None, arguments: list[str], root: Path
) -> ToolEvidence:
    """Capture one validated executable's bounded version output."""

    output = output_of(run([path, *arguments], root)) if path else None
    release = None
    commit = None
    if output:
        if name in {"rustc", "cargo"}:
            match = RELEASE_RE.search(output)
            release = match.group(1) if match else None
            if name == "cargo" and release is None:
                match = re.search(r"^cargo\s+(\d+\.\d+\.\d+)", output)
                release = match.group(1) if match else None
            match = re.search(r"commit-hash:\s*([0-9a-f]{10,40})", output)
            commit = match.group(1) if match else None
        elif name == "clippy":
            match = re.search(r"^clippy\s+(\d+\.\d+\.\d+)", output)
            release = match.group(1) if match else None
            match = COMMIT_RE.search(output)
            commit = match.group(1) if match else None
        else:
            match = COMMIT_RE.search(output)
            commit = match.group(1) if match else None
    return ToolEvidence(name, path, output, release, commit)


def preflight(root: Path, *, install: bool) -> int:
    """Run the repository preflight and print all expected/actual identities."""

    expected = read_declared_channel(root / "rust-toolchain.toml")
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    proxy_directory = cargo_home / "bin"
    paths = {
        "rustup": shutil.which("rustup"),
        "rustc": shutil.which("rustc"),
        "cargo": shutil.which("cargo"),
        "cargo-clippy": shutil.which("cargo-clippy"),
        "rustfmt": shutil.which("rustfmt"),
    }
    rustup_path = paths["rustup"]
    path_errors = {
        name: error
        for name, path in paths.items()
        if name != "rustup"
        if (error := executable_error(name, path, proxy_directory))
    }
    rustup_error = "rustup is missing from PATH" if rustup_path is None else None
    if rustup_path and not path_errors:
        rustup_executable = Path(rustup_path)
        rustup_digest: tuple[int, bytes] | None = None
        try:
            same_backing_executable = True
            for name in ("rustc", "cargo", "cargo-clippy", "rustfmt"):
                candidate = Path(paths[name])
                if rustup_executable.samefile(candidate):
                    continue
                if rustup_digest is None:
                    rustup_digest = executable_digest(rustup_executable)
                if rustup_digest is None or executable_digest(candidate) != rustup_digest:
                    same_backing_executable = False
                    break
        except (OSError, ValueError, RuntimeError):
            same_backing_executable = False
        if not same_backing_executable:
            rustup_error = (
                "rustup is not the executable backing the validated Rust proxies"
            )
    override = os.environ.get("RUSTUP_TOOLCHAIN")
    override_error = None
    if override and not version_matches(expected, override):
        override_error = (
            f"RUSTUP_TOOLCHAIN override is {override!r}; expected {expected!r}"
        )
    active = None
    install_error = None
    evidence = (
        ToolEvidence("rustc", paths["rustc"], None),
        ToolEvidence("cargo", paths["cargo"], None),
        ToolEvidence("clippy", paths["cargo-clippy"], None),
        ToolEvidence("rustfmt", paths["rustfmt"], None),
    )
    manager_blockers = list(path_errors.values())
    if rustup_error:
        manager_blockers.insert(0, rustup_error)
    if override_error:
        manager_blockers.append(override_error)
    expected_installed = False
    if rustup_path and not manager_blockers:
        inventory_result = run([rustup_path, "toolchain", "list"], root)
        inventory_output = output_of(inventory_result)
        inventory_available = (
            inventory_result is not None and inventory_result.returncode == 0
        )
        if not install and not inventory_available:
            rustup_error = "rustup did not report installed toolchains"
        elif not install and not inventory_contains(expected, inventory_output):
            rustup_error = f"expected Rust toolchain {expected!r} is not installed"
        elif install:
            result = run(
                [
                    rustup_path,
                    "toolchain",
                    "install",
                    expected,
                    "--profile",
                    "minimal",
                    "--component",
                    "clippy,rustfmt",
                ],
                root,
                timeout=120,
            )
            if result is None or result.returncode != 0:
                install_error = "rustup toolchain install failed"
            else:
                inventory_result = run([rustup_path, "toolchain", "list"], root)
                inventory_output = output_of(inventory_result)
                if (
                    inventory_result is None
                    or inventory_result.returncode != 0
                ):
                    rustup_error = "rustup did not report installed toolchains"
                elif not inventory_contains(expected, inventory_output):
                    rustup_error = (
                        "rustup install did not make the expected toolchain available"
                    )
                else:
                    expected_installed = True
        else:
            expected_installed = True
    if (
        rustup_path
        and expected_installed
        and not path_errors
        and not rustup_error
        and not override_error
    ):
        rustup_result = run([rustup_path, "show", "active-toolchain"], root)
        active_output = output_of(rustup_result)
        active = active_output.split()[0] if active_output else None
        if rustup_result is None or rustup_result.returncode != 0:
            rustup_error = "rustup did not report an active toolchain"
        evidence = (
            evidence_for("rustc", paths["rustc"], ["-Vv"], root),
            evidence_for("cargo", paths["cargo"], ["-Vv"], root),
            evidence_for(
                "clippy", paths["cargo-clippy"], ["--version", "--verbose"], root
            ),
            evidence_for(
                "rustfmt", paths["rustfmt"], ["--version", "--verbose"], root
            ),
        )

    errors = evaluate(
        expected,
        rustup_available=rustup_path is not None,
        rustup_error=rustup_error,
        active_toolchain=active,
        override=override,
        proxy_directory=proxy_directory,
        evidence=evidence,
    )
    if install_error:
        errors.insert(0, install_error)

    print(f"Rust toolchain preflight: expected {expected}")
    print(f"  rustup: {rustup_path or '<missing>'}")
    print(f"  active: {active or '<unavailable>'}")
    if override:
        print(f"  RUSTUP_TOOLCHAIN override: {override}")
    for tool in evidence:
        actual = tool.output.replace("\n", " | ") if tool.output else "<unavailable>"
        print(f"  {tool.name}: path={tool.path or '<missing>'}; {actual}")
    if errors:
        print("Rust toolchain preflight failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1
    print("Rust toolchain preflight passed")
    return 0


def self_test() -> None:
    """Exercise declaration and mismatch boundaries without invoking Rust."""

    expected = "1.98.0"
    historical = "1.93.1"
    expected_clippy = f"0.1.{expected.split('.')[1]}"
    historical_clippy = f"0.1.{historical.split('.')[1]}"
    with __import__("tempfile").TemporaryDirectory() as temporary:
        toolchain = Path(temporary) / "rust-toolchain.toml"
        toolchain.write_text(f'channel = "{expected}"\n', encoding="utf-8")
        assert read_declared_channel(toolchain) == expected
        for declaration in ('channel = "stable"\n',):
            toolchain.write_text(declaration, encoding="utf-8")
            try:
                read_declared_channel(toolchain)
            except PreflightError:
                pass
            else:
                raise AssertionError(f"accepted invalid declaration {declaration!r}")
        toolchain.write_text(f'channel = "{historical}"\n', encoding="utf-8")
        assert read_declared_channel(toolchain) == historical
        toolchain.write_text(
            f'channel = "{expected}"\nchannel = "{expected}"\n', encoding="utf-8"
        )
        try:
            read_declared_channel(toolchain)
        except PreflightError:
            pass
        else:
            raise AssertionError("accepted duplicate channel declarations")

    with __import__("tempfile").TemporaryDirectory() as temporary:
        existing = Path(temporary) / "cargo-home" / "bin"
        existing.mkdir(parents=True)
        assert same_directory(existing, existing)
        case_variant = existing.with_name("BIN")
        if case_variant.exists():
            assert same_directory(existing, case_variant)
        symlink = Path(temporary) / "proxy-link"
        try:
            symlink.symlink_to(existing, target_is_directory=True)
        except (OSError, NotImplementedError):
            pass
        else:
            assert same_directory(existing, symlink)
        with patch.object(Path, "samefile", side_effect=PermissionError("denied")):
            assert not same_directory(existing, existing)
        with patch.object(
            Path, "samefile", side_effect=FileNotFoundError("missing")
        ), patch.object(Path, "resolve", side_effect=PermissionError("denied")):
            assert not same_directory(existing, existing)
        missing = Path(temporary) / "missing"
        assert same_directory(missing, missing)
        assert not same_directory(missing, Path(temporary) / "other-missing")

    proxy = Path("/rustup/.cargo/bin")
    evidence = (
        ToolEvidence("rustc", str(proxy / "rustc"), "", historical, "rustc-hash"),
        ToolEvidence("cargo", str(proxy / "cargo"), "", historical, None),
        ToolEvidence(
            "clippy", str(proxy / "cargo-clippy"), "", historical_clippy, "rustc-hash"
        ),
        ToolEvidence("rustfmt", str(proxy / "rustfmt"), "", None, "rustc-hash"),
    )
    errors = evaluate(
        expected,
        rustup_available=False,
        active_toolchain=historical,
        override=historical,
        proxy_directory=proxy,
        evidence=evidence,
    )
    assert any("rustup is missing" in error for error in errors)
    assert any("RUSTUP_TOOLCHAIN override" in error for error in errors)
    assert any("rustc release" in error for error in errors)
    assert any("cargo release" in error for error in errors)
    assert any("clippy release" in error for error in errors)
    valid_evidence = (
        ToolEvidence("rustc", str(proxy / "rustc"), "", expected, "abcdef1234567890"),
        ToolEvidence("cargo", str(proxy / "cargo"), "", expected, "cargo-hash"),
        ToolEvidence(
            "clippy",
            str(proxy / "cargo-clippy"),
            "",
            expected_clippy,
            "abcdef1234567890",
        ),
        ToolEvidence(
            "rustfmt", str(proxy / "rustfmt"), "", None, "abcdef1234567890"
        ),
    )
    valid_errors = evaluate(
        expected,
        rustup_available=True,
        active_toolchain=f"{expected}-x86_64-pc-windows-msvc",
        override=expected,
        proxy_directory=proxy,
        evidence=valid_evidence,
    )
    assert not valid_errors
    for clippy_commit, expected_error in (
        ("abcdef1234", None),
        ("different-clippy-commit", "clippy commit"),
        (None, "clippy did not report a commit identity"),
    ):
        candidate_evidence = tuple(
            ToolEvidence(
                tool.name,
                tool.path,
                tool.output,
                tool.release,
                clippy_commit,
            )
            if tool.name == "clippy"
            else tool
            for tool in valid_evidence
        )
        candidate_errors = evaluate(
            expected,
            rustup_available=True,
            active_toolchain=f"{expected}-x86_64-pc-windows-msvc",
            override=expected,
            proxy_directory=proxy,
            evidence=candidate_evidence,
        )
        if expected_error is None:
            assert not candidate_errors
        else:
            assert any(expected_error in error for error in candidate_errors)
    precedence_errors = evaluate(
        expected,
        rustup_available=True,
        active_toolchain=expected,
        override=None,
        proxy_directory=proxy,
        evidence=(
            ToolEvidence("rustc", str(proxy / "rustc"), "", expected, "rustc-hash"),
            ToolEvidence(
                "cargo", "/opt/homebrew/bin/cargo", "", expected, "cargo-hash"
            ),
            ToolEvidence(
                "clippy", str(proxy / "cargo-clippy"), "", expected_clippy, "rustc-hash"
            ),
            ToolEvidence("rustfmt", str(proxy / "rustfmt"), "", None, "rustc-hash"),
        ),
    )
    assert any("cargo is selected" in error for error in precedence_errors)
    for target in (
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ):
        assert version_matches(expected, f"{expected}-{target}")
    assert not version_matches(expected, f"{expected}-evil")
    assert not version_matches(expected, f"{expected}-evil-wrapper-code")
    linux_hostile = f"{expected}-x86_64-unknown-linux-evil"
    darwin_hostile = f"{expected}-aarch64-apple-darwin-evil"
    for hostile in (linux_hostile, darwin_hostile):
        assert not version_matches(expected, hostile)
        assert not inventory_contains(expected, f"{hostile} (default)\n")
    assert not version_matches(expected, "stable")
    assert any(
        "active rustup toolchain" in error
        for error in evaluate(
            expected,
            rustup_available=True,
            active_toolchain=linux_hostile,
            override=expected,
            proxy_directory=proxy,
            evidence=(),
        )
    )

    with __import__("tempfile").TemporaryDirectory() as temporary:
        root = Path(temporary)
        cargo_home = root / "cargo"
        proxy = cargo_home / "bin"
        outside = root / "outside"
        proxy.mkdir(parents=True)
        outside.mkdir()
        (root / "rust-toolchain.toml").write_text(
            f'channel = "{expected}"\n', encoding="utf-8"
        )
        executable_names = ("rustup", "rustc", "cargo", "cargo-clippy", "rustfmt")
        valid_paths = {name: str(proxy / name) for name in executable_names}
        rustup_target = outside / "rustup-manager"
        rustup_target.write_text("rustup manager\n", encoding="utf-8")

        def link_to_rustup(path: Path) -> bool:
            try:
                path.symlink_to(rustup_target)
                return True
            except (OSError, NotImplementedError):
                if path.exists() or path.is_symlink():
                    path.unlink()
                path.hardlink_to(rustup_target)
                return False

        proxy_links = [
            link_to_rustup(Path(valid_paths[name]))
            for name in ("rustc", "cargo", "cargo-clippy", "rustfmt")
        ]
        symlinked_proxies = all(proxy_links)
        link_to_rustup(Path(valid_paths["rustup"]))
        current_paths = valid_paths.copy()
        requested: list[str] = []
        calls: list[list[str]] = []
        sentinels: dict[str, Path] = {}
        manager_state = {
            "installed": True,
            "install_returncode": 0,
            "install_timeout": False,
            "inventory_returncodes": [],
        }

        def fake_which(command: str) -> str | None:
            requested.append(command)
            return current_paths.get(command)

        def identity_output(name: str) -> str:
            if name == "rustup":
                return f"{expected}-x86_64-pc-windows-msvc (default)\n"
            if name == "rustc":
                return (
                    "rustc 1.98.0\n"
                    "commit-hash: abcdef123456\n"
                    "release: 1.98.0\n"
                )
            if name == "cargo":
                return (
                    "cargo 1.98.0\n"
                    "release: 1.98.0\n"
                    "commit-hash: abcdef123456\n"
                )
            if name == "cargo-clippy":
                return "clippy 0.1.98 (abcdef123456 2026-08-18)\n"
            return "rustfmt 1.9.0-stable (abcdef123456 2026-08-18)\n"

        def tracking_run(
            command: list[str], run_root: Path, *, timeout: int = 20
        ) -> subprocess.CompletedProcess[str] | None:
            del run_root, timeout
            executable = os.path.normcase(str(Path(command[0])))
            name = next(
                name
                for name, path in current_paths.items()
                if os.path.normcase(str(Path(path))) == executable
            )
            calls.append(command)
            if name in sentinels:
                sentinels[name].write_text("invoked\n", encoding="utf-8")
            if name == "rustup" and command[1:3] == ["toolchain", "list"]:
                inventory = (
                    f"{expected}-x86_64-pc-windows-msvc (default)\n"
                    if manager_state["installed"]
                    else "stable-x86_64-pc-windows-msvc (default)\n"
                )
                returncode = (
                    manager_state["inventory_returncodes"].pop(0)
                    if manager_state["inventory_returncodes"]
                    else 0
                )
                return subprocess.CompletedProcess(
                    command, returncode, stdout=inventory, stderr=""
                )
            if name == "rustup" and command[1:3] == ["toolchain", "install"]:
                if manager_state["install_timeout"]:
                    return None
                returncode = manager_state["install_returncode"]
                if returncode == 0:
                    manager_state["installed"] = True
                return subprocess.CompletedProcess(
                    command, returncode, stdout="", stderr=""
                )
            return subprocess.CompletedProcess(
                command, 0, stdout=identity_output(name), stderr=""
            )

        with patch.dict(
            os.environ,
            {"CARGO_HOME": str(cargo_home), "RUSTUP_TOOLCHAIN": expected},
            clear=False,
        ), patch(f"{__name__}.shutil.which", side_effect=fake_which), patch(
            f"{__name__}.run", side_effect=tracking_run
        ):
            assert preflight(root, install=False) == 0
        assert set(requested) == set(executable_names)
        clippy_calls = [
            call for call in calls if Path(call[0]).stem.lower() == "cargo-clippy"
        ]
        assert clippy_calls == [
            [valid_paths["cargo-clippy"], "--version", "--verbose"]
        ]
        assert not any(
            Path(call[0]).stem.lower() == "cargo" and "clippy" in call
            for call in calls
        )

        homebrew_rustup = outside / "homebrew-rustup"
        link_to_rustup(homebrew_rustup)
        current_paths["rustup"] = str(homebrew_rustup)
        requested.clear()
        calls.clear()
        with patch.dict(
            os.environ,
            {"CARGO_HOME": str(cargo_home), "RUSTUP_TOOLCHAIN": expected},
            clear=False,
        ), patch(f"{__name__}.shutil.which", side_effect=fake_which), patch(
            f"{__name__}.run", side_effect=tracking_run
        ):
            assert preflight(root, install=False) == 0
        assert any(call[0] == str(homebrew_rustup) for call in calls)
        if symlinked_proxies:
            assert all(
                Path(valid_paths[name]).is_symlink()
                for name in ("rustc", "cargo", "cargo-clippy", "rustfmt")
            )

        manager_state["installed"] = True
        manager_state["install_returncode"] = 0
        manager_state["install_timeout"] = False
        current_paths = valid_paths.copy()
        current_paths["rustup"] = str(homebrew_rustup)
        requested.clear()
        calls.clear()
        with patch.dict(
            os.environ,
            {"CARGO_HOME": str(cargo_home), "RUSTUP_TOOLCHAIN": expected},
            clear=False,
        ), patch(f"{__name__}.shutil.which", side_effect=fake_which), patch(
            f"{__name__}.run", side_effect=tracking_run
        ):
            assert preflight(root, install=True) == 0
        assert calls[0][1:3] == ["toolchain", "list"]
        assert calls[1][1:3] == ["toolchain", "install"]
        assert calls[1][3] == expected
        assert calls[2][1:3] == ["toolchain", "list"]
        assert calls[3][1:3] == ["show", "active-toolchain"]

        manager_state["install_returncode"] = 1
        requested.clear()
        calls.clear()
        with patch.dict(
            os.environ,
            {"CARGO_HOME": str(cargo_home), "RUSTUP_TOOLCHAIN": expected},
            clear=False,
        ), patch(f"{__name__}.shutil.which", side_effect=fake_which), patch(
            f"{__name__}.run", side_effect=tracking_run
        ):
            assert preflight(root, install=True) == 1
        assert [call[1:3] for call in calls] == [
            ["toolchain", "list"],
            ["toolchain", "install"],
        ]
        manager_state["install_returncode"] = 0

        current_paths = valid_paths.copy()
        current_paths["rustup"] = str(homebrew_rustup)
        requested.clear()
        calls.clear()
        with patch.dict(
            os.environ,
            {"CARGO_HOME": str(cargo_home), "RUSTUP_TOOLCHAIN": historical},
            clear=False,
        ), patch(f"{__name__}.shutil.which", side_effect=fake_which), patch(
            f"{__name__}.run", side_effect=tracking_run
        ):
            assert preflight(root, install=True) == 1
        assert set(requested) == set(executable_names)
        assert not calls, "invalid override allowed a Rustup subprocess"

        hostile_install_sentinel = outside / "hostile-override-install-invoked"
        sentinels = {"rustup": hostile_install_sentinel}
        requested.clear()
        calls.clear()
        with patch.dict(
            os.environ,
            {"CARGO_HOME": str(cargo_home), "RUSTUP_TOOLCHAIN": linux_hostile},
            clear=False,
        ), patch(f"{__name__}.shutil.which", side_effect=fake_which), patch(
            f"{__name__}.run", side_effect=tracking_run
        ):
            assert preflight(root, install=True) == 1
        assert set(requested) == set(executable_names)
        assert not calls, "hostile target override allowed a Rustup subprocess"
        assert not hostile_install_sentinel.exists(), (
            "hostile target override reached Rustup install"
        )
        sentinels = {}

        manager_state["installed"] = False
        manager_state["install_returncode"] = 0
        manager_state["install_timeout"] = False
        requested.clear()
        calls.clear()
        with patch.dict(
            os.environ,
            {"CARGO_HOME": str(cargo_home), "RUSTUP_TOOLCHAIN": expected},
            clear=False,
        ), patch(f"{__name__}.shutil.which", side_effect=fake_which), patch(
            f"{__name__}.run", side_effect=tracking_run
        ):
            assert preflight(root, install=False) == 1
        assert [call[1:3] for call in calls] == [["toolchain", "list"]]

        requested.clear()
        calls.clear()
        with patch.dict(
            os.environ,
            {"CARGO_HOME": str(cargo_home), "RUSTUP_TOOLCHAIN": expected},
            clear=False,
        ), patch(f"{__name__}.shutil.which", side_effect=fake_which), patch(
            f"{__name__}.run", side_effect=tracking_run
        ):
            assert preflight(root, install=True) == 0
        assert calls[0][1:3] == ["toolchain", "list"]
        assert calls[1][1:3] == ["toolchain", "install"]
        assert calls[1][3] == expected
        assert calls[2][1:3] == ["toolchain", "list"]
        assert calls[3][1:3] == ["show", "active-toolchain"]

        manager_state["installed"] = False
        manager_state["inventory_returncodes"] = [1, 0]
        requested.clear()
        calls.clear()
        with patch.dict(
            os.environ,
            {"CARGO_HOME": str(cargo_home), "RUSTUP_TOOLCHAIN": expected},
            clear=False,
        ), patch(f"{__name__}.shutil.which", side_effect=fake_which), patch(
            f"{__name__}.run", side_effect=tracking_run
        ):
            assert preflight(root, install=True) == 0
        assert [call[1:3] for call in calls[:4]] == [
            ["toolchain", "list"],
            ["toolchain", "install"],
            ["toolchain", "list"],
            ["show", "active-toolchain"],
        ]

        manager_state["installed"] = False
        manager_state["inventory_returncodes"] = [0, 1]
        requested.clear()
        calls.clear()
        with patch.dict(
            os.environ,
            {"CARGO_HOME": str(cargo_home), "RUSTUP_TOOLCHAIN": expected},
            clear=False,
        ), patch(f"{__name__}.shutil.which", side_effect=fake_which), patch(
            f"{__name__}.run", side_effect=tracking_run
        ):
            assert preflight(root, install=True) == 1
        assert [call[1:3] for call in calls] == [
            ["toolchain", "list"],
            ["toolchain", "install"],
            ["toolchain", "list"],
        ]

        manager_state["installed"] = False
        manager_state["install_returncode"] = 1
        requested.clear()
        calls.clear()
        with patch.dict(
            os.environ,
            {"CARGO_HOME": str(cargo_home), "RUSTUP_TOOLCHAIN": expected},
            clear=False,
        ), patch(f"{__name__}.shutil.which", side_effect=fake_which), patch(
            f"{__name__}.run", side_effect=tracking_run
        ):
            assert preflight(root, install=True) == 1
        assert [call[1:3] for call in calls] == [
            ["toolchain", "list"],
            ["toolchain", "install"],
        ]

        manager_state["install_returncode"] = 0
        manager_state["install_timeout"] = True
        requested.clear()
        calls.clear()
        with patch.dict(
            os.environ,
            {"CARGO_HOME": str(cargo_home), "RUSTUP_TOOLCHAIN": expected},
            clear=False,
        ), patch(f"{__name__}.shutil.which", side_effect=fake_which), patch(
            f"{__name__}.run", side_effect=tracking_run
        ):
            assert preflight(root, install=True) == 1
        assert [call[1:3] for call in calls] == [
            ["toolchain", "list"],
            ["toolchain", "install"],
        ]
        manager_state["installed"] = True
        manager_state["install_timeout"] = False

        windows_proxy_paths = valid_paths.copy()
        proxy_bytes = rustup_target.read_bytes()
        for name in ("rustc", "cargo", "cargo-clippy", "rustfmt"):
            proxy_path = Path(windows_proxy_paths[name])
            proxy_path.unlink()
            proxy_path.write_bytes(proxy_bytes)
        current_paths = windows_proxy_paths.copy()
        current_paths["rustup"] = str(homebrew_rustup)
        requested.clear()
        calls.clear()
        with patch.dict(
            os.environ,
            {"CARGO_HOME": str(cargo_home), "RUSTUP_TOOLCHAIN": expected},
            clear=False,
        ), patch(f"{__name__}.shutil.which", side_effect=fake_which), patch(
            f"{__name__}.run", side_effect=tracking_run
        ):
            assert preflight(root, install=False) == 0
        assert any(call[0] == str(homebrew_rustup) for call in calls)

        with patch.object(Path, "stat", side_effect=PermissionError("denied")):
            assert executable_digest(homebrew_rustup) is None
            requested.clear()
            calls.clear()
            with patch.dict(
                os.environ,
                {"CARGO_HOME": str(cargo_home), "RUSTUP_TOOLCHAIN": expected},
                clear=False,
            ), patch(f"{__name__}.shutil.which", side_effect=fake_which), patch(
                f"{__name__}.run", side_effect=tracking_run
            ):
                assert preflight(root, install=True) == 1
        assert not calls, "stat failure allowed a Rustup probe"
        with patch.object(Path, "open", side_effect=PermissionError("denied")):
            assert executable_digest(homebrew_rustup) is None

        fake_rustup = outside / "fake-rustup"
        fake_rustup.write_text("rustup invalid\n", encoding="utf-8")
        assert len(fake_rustup.read_bytes()) == len(rustup_target.read_bytes())
        fake_rustup_sentinel = outside / "fake-rustup-invoked"
        current_paths = valid_paths.copy()
        current_paths["rustup"] = str(fake_rustup)
        sentinels = {"rustup": fake_rustup_sentinel}
        requested.clear()
        calls.clear()
        with patch.dict(
            os.environ,
            {"CARGO_HOME": str(cargo_home), "RUSTUP_TOOLCHAIN": expected},
            clear=False,
        ), patch(f"{__name__}.shutil.which", side_effect=fake_which), patch(
            f"{__name__}.run", side_effect=tracking_run
        ):
            assert preflight(root, install=True) == 1
        assert set(requested) == set(executable_names)
        assert not calls, "untrusted rustup was probed or installed"
        assert not fake_rustup_sentinel.exists(), "untrusted rustup was invoked"

        direct_tool_names = ("rustc", "cargo", "cargo-clippy", "rustfmt")
        for invalid_name in direct_tool_names:
            current_paths = valid_paths.copy()
            current_paths["rustup"] = str(homebrew_rustup)
            invalid_path = outside / invalid_name
            invalid_path.write_text("untrusted executable\n", encoding="utf-8")
            current_paths[invalid_name] = str(invalid_path)
            sentinels = {invalid_name: outside / f"{invalid_name}-invoked"}
            requested.clear()
            calls.clear()
            with patch.dict(
                os.environ,
                {"CARGO_HOME": str(cargo_home), "RUSTUP_TOOLCHAIN": expected},
                clear=False,
            ), patch(f"{__name__}.shutil.which", side_effect=fake_which), patch(
                f"{__name__}.run", side_effect=tracking_run
            ):
                assert preflight(root, install=True) == 1
            assert set(requested) == set(executable_names)
            assert not calls, f"{invalid_name} provenance allowed a probe"
            assert not sentinels[invalid_name].exists(), (
                f"out-of-proxy {invalid_name} was invoked"
            )

        current_paths = valid_paths.copy()
        current_paths.pop("rustup")
        requested.clear()
        calls.clear()
        with patch.dict(
            os.environ,
            {"CARGO_HOME": str(cargo_home), "RUSTUP_TOOLCHAIN": expected},
            clear=False,
        ), patch(f"{__name__}.shutil.which", side_effect=fake_which), patch(
            f"{__name__}.run", side_effect=tracking_run
        ):
            assert preflight(root, install=True) == 1
        assert set(requested) == set(executable_names)
        assert not calls, "missing rustup was probed"
    print("Rust toolchain preflight self-test passed")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--install",
        action="store_true",
        help="install the declared toolchain through rustup without changing its default",
    )
    parser.add_argument(
        "--self-test", action="store_true", help="run offline parser/mismatch tests"
    )
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    return preflight(Path(__file__).resolve().parents[2], install=args.install)


if __name__ == "__main__":
    raise SystemExit(main())
