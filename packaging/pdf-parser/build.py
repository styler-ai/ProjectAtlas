#!/usr/bin/env python3
"""Rebuild the fixed PDF guest and verify its checked-in bytes (or --write them)."""

import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import tomllib


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true", help="replace the embedded artifact")
    parser.add_argument("--install-target", action="store_true", help="install the pinned WASI target for CI")
    parser.add_argument("--validate", action="store_true", help="also run guest format, lint, tests, and dependency policy")
    args = parser.parse_args()
    guest = Path(__file__).resolve().parent
    root = guest.parent.parent
    channel = tomllib.loads((root / "rust-toolchain.toml").read_text(encoding="utf-8"))["toolchain"]["channel"]
    target = root / ".tmp" / "pdf-parser-build"
    env = os.environ.copy()
    cargo_home = Path(env.get("CARGO_HOME", str(Path.home() / ".cargo"))).resolve()
    remaps = [f"--remap-path-prefix={root}=/projectatlas"]
    # Strip machine-specific registry roots from embedded parser diagnostics.
    for registry in sorted((cargo_home / "registry" / "src").glob("*")):
        if registry.is_dir():
            remaps.append(f"--remap-path-prefix={registry}=/registry")
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(remaps)
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
        raise SystemExit("PDF parser artifact differs from its locked source; rebuild with --write")
    if args.validate:
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
    print(f"PDF parser verified: {len(built)} bytes, sha256={hashlib.sha256(built).hexdigest()}")


if __name__ == "__main__":
    main()
