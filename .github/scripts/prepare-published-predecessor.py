#!/usr/bin/env python3
"""Bind installed upgrade tests to the published v0.4.5 runtime and plugin."""

import argparse
import hashlib
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
    asset = f"projectatlas-v0.4.5-{args.suffix}.{'zip' if windows else 'tar.gz'}"
    subprocess.run([
        "gh", "release", "download", "v0.4.5", "--repo", "styler-ai/ProjectAtlas",
        "--pattern", asset, "--pattern", "SHA256SUMS", "--dir", str(root),
    ], check=True, timeout=180)
    archive = root / asset
    with archive.open("rb") as source:
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    checksums = root / "SHA256SUMS"
    if checksums.read_text(encoding="utf-8").splitlines().count(f"{digest}  {asset}") != 1:
        raise ValueError("Published predecessor archive does not match SHA256SUMS")
    executable = root / ("projectatlas.exe" if windows else "projectatlas")
    if windows:
        with zipfile.ZipFile(archive) as package, executable.open("xb") as output:
            with package.open("projectatlas.exe") as source:
                shutil.copyfileobj(source, output)
    else:
        with tarfile.open(archive) as package, executable.open("xb") as output:
            member = package.getmember("projectatlas/projectatlas")
            if not member.isfile():
                raise ValueError("Published predecessor executable is not a regular file")
            with package.extractfile(member) as source:
                shutil.copyfileobj(source, output)
        executable.chmod(0o755)
    repository = root / "plugin.git"
    subprocess.run(["git", "init", "--bare", str(repository)], check=True, timeout=30)
    git = ["git", "--git-dir", str(repository)]
    subprocess.run([*git, "fetch", "--depth=1", "https://github.com/styler-ai/ProjectAtlas",
                    "refs/tags/v0.4.5"], check=True, timeout=180)
    commit = subprocess.check_output([*git, "rev-parse", "FETCH_HEAD^{commit}"], text=True).strip()
    if commit != "72b424b7bb79b0d413dfb8c1bdd8eae9dc4e196b":
        raise ValueError("Published predecessor plugin tag changed")
    source_archive = root / "plugin-source.zip"
    subprocess.run([*git, "archive", "--format=zip", f"--output={source_archive}",
                    commit, "plugins/projectatlas"], check=True, timeout=30)
    source_root = root / "source"
    with zipfile.ZipFile(source_archive) as package:
        package.extractall(source_root)
    bindings = {
        "PROJECTATLAS_PREDECESSOR_EXECUTABLE": executable,
        "PROJECTATLAS_PREDECESSOR_EXECUTABLE_SHA256": hashlib.sha256(executable.read_bytes()).hexdigest(),
        "PROJECTATLAS_PREDECESSOR_SOURCE": source_root,
        "PROJECTATLAS_PREDECESSOR_ARCHIVE": archive,
        "PROJECTATLAS_PREDECESSOR_CHECKSUMS": checksums,
    }
    with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as environment:
        for name, path in bindings.items():
            environment.write(f"{name}={path}\n")


if __name__ == "__main__":
    main()
