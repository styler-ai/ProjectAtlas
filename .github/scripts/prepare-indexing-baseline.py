#!/usr/bin/env python3
"""Download the distinct published 0.5.0 indexing baseline without changing upgrade inputs."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suffix", required=True, choices=(
        "x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu",
        "x86_64-apple-darwin", "aarch64-apple-darwin",
    ))
    parser.add_argument("--destination", required=True, type=Path)
    args = parser.parse_args()
    root = args.destination.resolve()
    root.mkdir(parents=True, exist_ok=False)
    windows = args.suffix == "x86_64-pc-windows-msvc"
    asset = f"projectatlas-v0.5.0-{args.suffix}.{'zip' if windows else 'tar.gz'}"
    subprocess.run([
        "gh", "release", "download", "v0.5.0", "--repo", "styler-ai/ProjectAtlas",
        "--pattern", asset, "--pattern", "SHA256SUMS", "--dir", str(root),
    ], check=True, timeout=180)
    archive = root / asset
    with archive.open("rb") as source:
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    catalog = json.loads((Path(__file__).resolve().parents[2] /
                          "docs/benchmarks/v0.5.1-indexing-baselines.json").read_text(encoding="utf-8"))
    if catalog["baseline_version"] != "0.5.0" or catalog["assets"].get(asset) != digest:
        raise ValueError("Published baseline drifted from its preregistered archive identity")
    checksums = root / "SHA256SUMS"
    if checksums.read_text(encoding="utf-8").splitlines().count(f"{digest}  {asset}") != 1:
        raise ValueError("Published indexing baseline archive does not match SHA256SUMS")
    executable = root / ("projectatlas.exe" if windows else "projectatlas")
    if windows:
        with zipfile.ZipFile(archive) as package, executable.open("xb") as output:
            with package.open("projectatlas.exe") as source:
                shutil.copyfileobj(source, output)
    else:
        with tarfile.open(archive) as package, executable.open("xb") as output:
            member = package.getmember("projectatlas/projectatlas")
            if not member.isfile():
                raise ValueError("Baseline runtime is not a regular archive member")
            with package.extractfile(member) as source:
                shutil.copyfileobj(source, output)
        executable.chmod(0o755)
    bindings = {
        "PROJECTATLAS_INDEX_BASELINE_EXECUTABLE": str(executable),
        "PROJECTATLAS_INDEX_BASELINE_ARCHIVE": str(archive),
        "PROJECTATLAS_INDEX_BASELINE_CHECKSUMS": str(checksums),
    }
    if environment_file := os.environ.get("GITHUB_ENV"):
        with open(environment_file, "a", encoding="utf-8") as environment:
            for name, path in bindings.items():
                environment.write(f"{name}={path}\n")
    (root / "bindings.json").write_text(json.dumps(bindings, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
