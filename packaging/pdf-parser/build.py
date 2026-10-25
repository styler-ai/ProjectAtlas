#!/usr/bin/env python3
"""Verify the fixed PDF guest on its Linux x86-64 builder, or validate sources."""

import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import subprocess
import runpy


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true", help="replace the embedded artifact")
    parser.add_argument("--install-target", action="store_true", help="install the pinned WASI target for CI")
    parser.add_argument("--validate", action="store_true", help="also run guest format, lint, tests, and dependency policy")
    parser.add_argument("--source-only", action="store_true", help="validate source without canonical Linux x86-64 byte verification")
    args = parser.parse_args()
    if args.source_only and (not args.validate or args.write or args.install_target):
        parser.error("--source-only requires --validate and cannot write or install a WASI target")
    if not args.source_only and (platform.system() != "Linux" or platform.machine() != "x86_64"):
        parser.error("fixed PDF guest byte verification requires Linux x86-64; use --validate --source-only for native source checks")
    guest = Path(__file__).resolve().parent
    root = guest.parent.parent
    preflight = runpy.run_path(str(root / ".github/scripts/verify-rust-toolchain.py"))
    channel = preflight["read_declared_channel"](root / "rust-toolchain.toml")
    target = root / ".tmp" / "pdf-parser-build"
    env = os.environ.copy()
    if args.source_only:
        validate_sources(guest, root, channel, target, env)
        print("PDF guest source validation passed; canonical byte verification belongs to Linux x86-64 CI")
        return
    # Fetch the locked target tree before inspecting registry source paths.
    metadata = json.loads(subprocess.run(
        ["cargo", f"+{channel}", "metadata", "--locked", "--format-version", "1",
         "--filter-platform", "wasm32-wasip1", "--manifest-path", str(guest / "Cargo.toml")],
        cwd=root, check=True, capture_output=True, text=True, encoding="utf-8", timeout=180,
    ).stdout)
    selected = {node["id"] for node in metadata["resolve"]["nodes"]}
    remaps = {}
    for package in metadata["packages"]:
        if package["id"] not in selected:
            continue
        manifest = Path(package["manifest_path"])
        if not manifest.is_file():
            raise SystemExit(f"Resolved PDF guest dependency is not materialized: {manifest}")
        package_root = manifest.parent
        canonical = (Path("/registry") / f"{package['name']}-{package['version']}"
                     if package["source"] else Path("/projectatlas") / package_root.relative_to(root))
        for directory in {source.parent for source in package_root.rglob("*.rs")}:
            destination = (canonical / directory.relative_to(package_root)).as_posix() + "/"
            # A trailing separator leaves only the filename as the unmapped suffix.
            # Mapping just a registry root retains Windows backslashes in diagnostics.
            remaps[str(directory) + os.sep] = destination
            if not package["source"]:
                for base in (root, guest):
                    remaps[str(directory.relative_to(base)) + os.sep] = destination
    target.mkdir(parents=True, exist_ok=True)
    flags = "".join(
        f"--remap-path-prefix={source}={destination}\n"
        for source, destination in sorted(remaps.items())
    )
    # Cargo fingerprints the response filename, not changes to its contents.
    response = target / f"rustflags-{hashlib.sha256(flags.encode()).hexdigest()}.args"
    response.write_text(flags, encoding="utf-8", newline="\n")
    # Keep hundreds of source-directory mappings outside Windows' argv limit.
    env["CARGO_ENCODED_RUSTFLAGS"] = "@" + str(response)
    env.pop("RUSTFLAGS", None)
    if args.install_target:
        subprocess.run(["rustup", "target", "add", "--toolchain", channel, "wasm32-wasip1"], check=True, timeout=180)
    subprocess.run(
        ["cargo", f"+{channel}", "build", "--locked", "--release", "--manifest-path", str(guest / "Cargo.toml"),
         "--target", "wasm32-wasip1", "--target-dir", str(target)],
        cwd=root, env=env, check=True, timeout=600,
    )
    built = (target / "wasm32-wasip1" / "release" / "projectatlas_pdf_parser.wasm").read_bytes()
    artifact = guest / "parser.wasm"
    if args.write:
        artifact.write_bytes(built)
    elif artifact.read_bytes() != built:
        raise SystemExit(f"PDF parser artifact differs from its locked source: built sha256={hashlib.sha256(built).hexdigest()}; rebuild with --write")
    if args.validate:
        validate_sources(guest, root, channel, target, env)
    print(f"PDF parser verified: {len(built)} bytes, sha256={hashlib.sha256(built).hexdigest()}")


def validate_sources(guest: Path, root: Path, channel: str, target: Path, env: dict) -> None:
    """Check the guest's native behavior and locked WASI dependency policy on any host."""
    manifest = str(guest / "Cargo.toml")
    for command in [
        ["cargo", f"+{channel}", "fmt", "--manifest-path", manifest, "--check"],
        ["cargo", f"+{channel}", "clippy", "--locked", "--manifest-path", manifest, "--all-targets", "--target-dir", str(target), "--", "-D", "warnings"],
        ["cargo", f"+{channel}", "test", "--locked", "--manifest-path", manifest, "--target-dir", str(target)],
        # The shared policy includes native-only exceptions absent from this guest.
        # Allow only those unused-policy diagnostics; actual dependency findings fail.
        ["cargo", "deny", "--locked", "--manifest-path", manifest, "--config", str(root / "deny.toml"), "--target", "wasm32-wasip1", "check", "-D", "warnings",
         "-A", "unmatched-skip", "-A", "unnecessary-skip", "-A", "license-exception-not-encountered", "-A", "license-not-encountered"],
    ]:
        subprocess.run(command, cwd=root, env=env, check=True, timeout=600)


if __name__ == "__main__":
    main()
