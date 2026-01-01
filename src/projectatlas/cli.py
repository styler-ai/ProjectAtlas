"""
Purpose: Provide the ProjectAtlas command-line entry points.
"""

from __future__ import annotations

import argparse
import os
import sys
from datetime import datetime, timezone
from pathlib import Path

from projectatlas.atlas import (
    build_file_records,
    build_folder_records,
    build_folder_tree_with_summaries,
    build_summary_duplicates,
    compute_file_hash,
    compute_folder_hash,
    compute_overview,
    format_list,
    iter_repo_paths,
    iter_repo_paths_with_untracked,
    list_existing_asset_roots,
    read_map_hashes,
    read_manual_file_entries,
    read_overview,
    summarize_extensions,
)
from projectatlas.config import AtlasConfig, default_config_text, load_config
from projectatlas.models import AtlasSnapshot
from projectatlas.output import write_json, write_toon


TRUTHY_ENV = {"1", "true", "yes", "on"}


def is_truthy_env(value: str | None) -> bool:
    """Check whether an environment value is truthy."""
    if value is None:
        return False
    return value.strip().lower() in TRUTHY_ENV


def build_snapshot(config: AtlasConfig) -> AtlasSnapshot:
    """Build an AtlasSnapshot for the current repository."""
    folders, files = iter_repo_paths(config)
    file_records, _, _ = build_file_records(files, config)
    manual_records, _, _, _ = read_manual_file_entries(config)
    if manual_records:
        from projectatlas.atlas import merge_manual_file_records

        file_records = merge_manual_file_records(file_records, manual_records)
    folder_records, _, _ = build_folder_records(folders, config)
    folder_summary_map = {record.path: record.summary for record in folder_records}
    folder_tree = build_folder_tree_with_summaries(
        folders, folder_summary_map, config
    )
    folder_duplicates = build_summary_duplicates(folder_records)
    file_duplicates = build_summary_duplicates(file_records)
    file_hash = compute_file_hash(file_records)
    folder_hash = compute_folder_hash(folders, config)
    overview = compute_overview(folders, files, config)
    generated_at = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    return AtlasSnapshot(
        folder_records=folder_records,
        file_records=file_records,
        folder_tree=folder_tree,
        folder_duplicates=folder_duplicates,
        file_duplicates=file_duplicates,
        file_hash=file_hash,
        folder_hash=folder_hash,
        generated_at=generated_at,
        overview=overview,
    )


def seed_purpose_files(config: AtlasConfig) -> int:
    """Create missing .purpose files for tracked folders."""
    folders, _ = iter_repo_paths(config)
    created = 0
    for folder_path in folders:
        purpose_path = folder_path / config.purpose_filename
        if purpose_path.exists():
            continue
        rel = folder_path.relative_to(config.root).as_posix()
        purpose_path.write_text(f"# path: {rel}\n", encoding="utf-8")
        created += 1
    sys.stderr.write(f"Seeded {created} {config.purpose_filename} files.\n")
    return 0


def write_default_files(config_root: Path) -> None:
    """Write default configuration and manual file template."""
    project_dir = config_root / ".projectatlas"
    project_dir.mkdir(parents=True, exist_ok=True)
    config_path = project_dir / "config.toml"
    if not config_path.exists():
        config_path.write_text(default_config_text(), encoding="utf-8")
    manual_path = project_dir / "projectatlas-manual-files.toon"
    if not manual_path.exists():
        manual_path.write_text(
            "manual_files[]:\n"
            "  # path,summary\n",
            encoding="utf-8",
        )


def run_map(config: AtlasConfig, write_json_output: bool) -> int:
    """Generate and write the ProjectAtlas map."""
    if is_truthy_env(os.getenv("PROJECTATLAS_SKIP_UPDATE")):
        sys.stderr.write("Skipping ProjectAtlas map update.\n")
        return 0
    snapshot = build_snapshot(config)
    write_toon(snapshot, config)
    if write_json_output:
        write_json(snapshot, config)
    return 0


def run_lint(
    config: AtlasConfig,
    strict_folders: bool,
    report_untracked: bool,
    strict_untracked: bool,
) -> int:
    """Validate the ProjectAtlas map against current repo state."""
    folders, files, untracked_files, excluded_paths = (
        iter_repo_paths_with_untracked(config)
    )
    file_records, missing_headers, invalid_headers = build_file_records(
        files, config
    )
    manual_records, manual_missing, manual_invalid, manual_errors = (
        read_manual_file_entries(config)
    )
    manual_paths = {record.path for record in manual_records}
    if manual_records:
        from projectatlas.atlas import merge_manual_file_records

        file_records = merge_manual_file_records(file_records, manual_records)
    folder_records, missing_folders, invalid_folders = build_folder_records(
        folders, config
    )

    errors: list[str] = []
    if manual_errors:
        errors.append("Manual file list errors:")
        errors.append(format_list(manual_errors))
    if manual_missing:
        errors.append("Missing manual file entries:")
        errors.append(format_list(manual_missing))
    if manual_invalid:
        errors.append("Invalid manual file summaries:")
        errors.append(format_list(manual_invalid))
    if missing_headers:
        errors.append("Missing Purpose headers:")
        errors.append(format_list(missing_headers))
    if invalid_headers:
        errors.append("Invalid Purpose headers:")
        for path, issues in sorted(invalid_headers.items()):
            errors.append(f" - {path}: {', '.join(issues)}")
    if invalid_folders:
        errors.append("Invalid folder Purpose summaries:")
        for path, issues in sorted(invalid_folders.items()):
            errors.append(f" - {path}: {', '.join(issues)}")
    if strict_folders and missing_folders:
        errors.append("Missing folder Purpose files:")
        errors.append(format_list(missing_folders))

    enforce_untracked = strict_untracked
    if report_untracked and not enforce_untracked:
        enforce_untracked = not is_truthy_env(os.getenv("CI"))
    if is_truthy_env(os.getenv("PROJECTATLAS_ALLOW_UNTRACKED")):
        enforce_untracked = False

    if report_untracked:
        report: list[str] = []
        allowed_untracked: list[Path] = []
        disallowed_untracked: list[Path] = []
        asset_outside_roots: list[Path] = []
        from projectatlas.atlas import (
            is_allowed_untracked,
            is_asset_file,
            is_under_prefix,
        )

        for path in untracked_files:
            rel = path.relative_to(config.root)
            rel_posix = rel.as_posix()
            if rel_posix in manual_paths:
                allowed_untracked.append(path)
                continue
            if is_asset_file(path, config) and not is_under_prefix(
                rel_posix, config.asset_allowed_prefixes
            ):
                asset_outside_roots.append(path)
                disallowed_untracked.append(path)
                continue
            if is_allowed_untracked(rel, config):
                allowed_untracked.append(path)
            else:
                disallowed_untracked.append(path)

        report.append(
            "Untracked files (non-source extensions): "
            f"{len(untracked_files)} (allowed {len(allowed_untracked)}, "
            f"disallowed {len(disallowed_untracked)})"
        )
        if disallowed_untracked:
            report.append("Disallowed untracked files:")
            report.append(
                format_list(
                    p.relative_to(config.root).as_posix()
                    for p in disallowed_untracked
                )
            )
            report.append("Disallowed extension counts:")
            report.append(format_list(summarize_extensions(disallowed_untracked)))
        else:
            report.append("Disallowed untracked files: 0")
        report.append("Allowed untracked extension counts:")
        allowed_summary = summarize_extensions(allowed_untracked)
        report.append(format_list(allowed_summary) if allowed_summary else " (none)")
        if allowed_untracked and not is_truthy_env(os.getenv("CI")):
            report.append("Allowed untracked files:")
            report.append(
                format_list(
                    p.relative_to(config.root).as_posix()
                    for p in allowed_untracked
                )
            )

        asset_roots = list_existing_asset_roots(config)
        report.append(f"Asset roots present: {len(asset_roots)}")
        report.append(format_list(asset_roots) if asset_roots else " (none)")
        if asset_outside_roots:
            report.append("Asset files outside allowed roots:")
            report.append(
                format_list(
                    p.relative_to(config.root).as_posix()
                    for p in asset_outside_roots
                )
            )

        if excluded_paths:
            report.append(f"Excluded paths present: {len(excluded_paths)}")
            report.append(format_list(excluded_paths))
        else:
            report.append("Excluded paths present: 0")
        sys.stderr.write("\n".join(report) + "\n")
        if enforce_untracked and disallowed_untracked:
            errors.append("Untracked files detected.")

    map_overview, overview_issues = read_overview(config.map_path)
    expected_overview = compute_overview(folders, files, config)
    if map_overview is None:
        errors.append("Atlas map missing overview. Run: projectatlas map")
    elif overview_issues:
        errors.append("Atlas map overview invalid. Run: projectatlas map")
    elif map_overview != expected_overview:
        errors.append("Atlas map overview stale. Run: projectatlas map")

    map_file_hash, map_folder_hash = read_map_hashes(config.map_path)
    expected_file_hash = compute_file_hash(file_records)
    expected_folder_hash = compute_folder_hash(folders, config)
    if map_file_hash is None or map_folder_hash is None:
        errors.append("Atlas map missing hashes. Run: projectatlas map")
    else:
        if map_file_hash != expected_file_hash:
            errors.append("Atlas map file hash stale. Run: projectatlas map")
        if map_folder_hash != expected_folder_hash:
            errors.append("Atlas map folder hash stale. Run: projectatlas map")

    if errors:
        sys.stderr.write("\n".join(errors) + "\n")
        return 1
    return 0


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parse CLI arguments."""
    parser = argparse.ArgumentParser(prog="projectatlas")
    parser.add_argument(
        "--config",
        type=Path,
        help="Path to ProjectAtlas config.toml",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    init_cmd = subparsers.add_parser("init")
    init_cmd.add_argument(
        "--seed-purpose",
        action="store_true",
        help="Create missing .purpose files after init",
    )

    map_cmd = subparsers.add_parser("map")
    map_cmd.add_argument(
        "--json",
        action="store_true",
        help="Write JSON output alongside the TOON map",
    )
    map_cmd.add_argument(
        "--force",
        action="store_true",
        help="Run map generation even when CI is detected",
    )

    lint_cmd = subparsers.add_parser("lint")
    lint_cmd.add_argument("--strict-folders", action="store_true")
    lint_cmd.add_argument("--report-untracked", action="store_true")
    lint_cmd.add_argument("--strict-untracked", action="store_true")

    subparsers.add_parser("seed-purpose")

    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run the ProjectAtlas CLI."""
    args = parse_args(argv or sys.argv[1:])
    config = load_config(args.config)

    if args.command == "init":
        write_default_files(config.root)
        if args.seed_purpose:
            return seed_purpose_files(config)
        return 0
    if args.command == "seed-purpose":
        return seed_purpose_files(config)
    if args.command == "map":
        if (
            not args.force
            and (is_truthy_env(os.getenv("CI")) or is_truthy_env(os.getenv("GITHUB_ACTIONS")))
        ):
            sys.stderr.write("Skipping ProjectAtlas map update in CI.\n")
            return 0
        return run_map(config, write_json_output=args.json)
    if args.command == "lint":
        return run_lint(
            config,
            strict_folders=args.strict_folders,
            report_untracked=args.report_untracked,
            strict_untracked=args.strict_untracked,
        )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
