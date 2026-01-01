"""
Purpose: Scan repositories and build ProjectAtlas record entries.
"""

from __future__ import annotations

import hashlib
import re
import os
from pathlib import Path
from typing import Iterable, Sequence

from projectatlas.config import AtlasConfig
from projectatlas.models import Record


PURPOSE_COMMENT_PREFIXES = ("#", "//")
PURPOSE_RE = re.compile(r"Purpose:\s*(.+)$")
JAVADOC_START_RE = re.compile(r"^\s*/\*\*")
JAVADOC_END_RE = re.compile(r"\*/")
PY_DOCSTRING_START_RE = re.compile(r'^[rubfRUBF]*("""|\'\'\')')
PY_CODING_RE = re.compile(r"^#.*coding[:=]")
OVERVIEW_KEY_ORDER = (
    "tracked_files",
    "tracked_folders",
    "source_extensions",
    "exclude_dir_names",
    "exclude_path_prefixes",
)


def is_excluded_rel_path(rel_path: Path, config: AtlasConfig) -> bool:
    """Check whether a relative path should be excluded."""
    if rel_path == Path("."):
        return False
    if any(part in config.exclude_dir_names for part in rel_path.parts):
        return True
    if any(
        part.endswith(suffix)
        for part in rel_path.parts
        for suffix in config.exclude_dir_suffixes
    ):
        return True
    rel_posix = rel_path.as_posix()
    return any(
        rel_posix == prefix or rel_posix.startswith(f"{prefix}/")
        for prefix in config.exclude_path_prefixes
    )


def file_extension(path: Path) -> str:
    """Return a normalized file extension, handling .d.ts specially."""
    if path.name.endswith(".d.ts"):
        return ".d.ts"
    return path.suffix.lower()


def is_source_file(path: Path, config: AtlasConfig) -> bool:
    """Check whether a file should be scanned for Purpose headers."""
    return file_extension(path) in config.source_extensions


def iter_repo_paths(config: AtlasConfig) -> tuple[list[Path], list[Path]]:
    """Walk the repo and return tracked folders and source files."""
    folders, files, _, _ = iter_repo_paths_with_untracked(config)
    return folders, files


def iter_repo_paths_with_untracked(
    config: AtlasConfig,
) -> tuple[list[Path], list[Path], list[Path], list[str]]:
    """Walk the repo and return tracked and untracked paths."""
    root = config.root
    folders: list[Path] = []
    files: list[Path] = []
    untracked_files: list[Path] = []
    excluded_paths: set[str] = set()
    for root_dir, dirnames, filenames in os.walk(root):
        root_dir_path = Path(root_dir)
        rel_root = root_dir_path.relative_to(root)
        if is_excluded_rel_path(rel_root, config):
            if rel_root != Path("."):
                excluded_paths.add(rel_root.as_posix())
            dirnames[:] = []
            continue
        folders.append(root_dir_path)
        dirnames[:] = [
            name
            for name in dirnames
            if not is_excluded_rel_path(rel_root / name, config)
        ]
        for name in filenames:
            path = root_dir_path / name
            rel_path = path.relative_to(root)
            if is_excluded_rel_path(rel_path.parent, config):
                continue
            rel_posix = rel_path.as_posix()
            if is_under_prefix(rel_posix, config.non_source_path_prefixes):
                untracked_files.append(path)
                continue
            if not is_source_file(path, config):
                untracked_files.append(path)
                continue
            files.append(path)
    folders_sorted = sorted(
        folders, key=lambda p: p.relative_to(root).as_posix()
    )
    files_sorted = sorted(files, key=lambda p: p.relative_to(root).as_posix())
    untracked_sorted = sorted(
        untracked_files, key=lambda p: p.relative_to(root).as_posix()
    )
    excluded_sorted = sorted(excluded_paths)
    return folders_sorted, files_sorted, untracked_sorted, excluded_sorted


def normalize_summary(summary: str) -> str:
    """Normalize a Purpose summary to a single spaced line."""
    return re.sub(r"\s+", " ", summary.strip())


def summarize_extensions(paths: Sequence[Path]) -> list[str]:
    """Summarize extensions for a sequence of file paths."""
    counts: dict[str, int] = {}
    for path in paths:
        ext = path.suffix.lower() or "<no_ext>"
        counts[ext] = counts.get(ext, 0) + 1
    return [f"{ext}={counts[ext]}" for ext in sorted(counts)]


def is_under_prefix(rel_posix: str, prefixes: set[str]) -> bool:
    """Check whether a path is under any of the provided prefixes."""
    return any(
        rel_posix == prefix or rel_posix.startswith(f"{prefix}/")
        for prefix in prefixes
    )


def is_allowed_untracked(rel_path: Path, config: AtlasConfig) -> bool:
    """Check whether an untracked path is allowed by policy."""
    if rel_path.name in config.allowed_untracked_filenames:
        return True
    rel_posix = rel_path.as_posix()
    if rel_posix in config.untracked_allowlist_files:
        return True
    return is_under_prefix(rel_posix, config.untracked_allowlist_dir_prefixes)


def is_asset_file(path: Path, config: AtlasConfig) -> bool:
    """Check whether a file is considered an asset."""
    return path.suffix.lower() in config.asset_extensions


def list_existing_asset_roots(config: AtlasConfig) -> list[str]:
    """List asset root prefixes that exist in the repo."""
    roots: list[str] = []
    for prefix in sorted(config.asset_allowed_prefixes):
        if (config.root / prefix).exists():
            roots.append(prefix)
    return roots


def extract_purpose_from_lines(lines: Sequence[str]) -> str | None:
    """Extract the Purpose summary from a sequence of lines."""
    for raw in lines:
        cleaned = raw.strip()
        cleaned = re.sub(r"^/\*\*+", "", cleaned)
        cleaned = re.sub(r"\*/$", "", cleaned)
        cleaned = re.sub(r"^\*+", "", cleaned).strip()
        match = PURPOSE_RE.search(cleaned)
        if match:
            return normalize_summary(match.group(1))
    return None


def extract_javadoc_purpose(
    lines: list[str], max_scan_lines: int
) -> tuple[str | None, list[str]]:
    """Extract Purpose from a Javadoc-style header block."""
    idx = 0
    if lines and lines[0].startswith("#!"):
        idx += 1
    while idx < len(lines) and not lines[idx].strip():
        idx += 1
    if idx >= len(lines) or not JAVADOC_START_RE.match(lines[idx]):
        return None, ["missing Javadoc-style Purpose header"]
    block_lines: list[str] = []
    block_lines.append(lines[idx])
    idx += 1
    while idx < len(lines) and idx < max_scan_lines:
        line = lines[idx]
        block_lines.append(line)
        if JAVADOC_END_RE.search(line):
            break
        idx += 1
    else:
        return None, ["unterminated Javadoc-style header"]
    summary = extract_purpose_from_lines(block_lines)
    if not summary:
        return None, ["missing Purpose line in Javadoc-style header"]
    return summary, []


def extract_python_docstring_purpose(
    lines: list[str], max_scan_lines: int
) -> tuple[str | None, list[str]]:
    """Extract Purpose from a Python module docstring."""
    idx = 0
    while idx < len(lines):
        line = lines[idx].strip()
        if line.startswith("#!") or PY_CODING_RE.match(line) or not line:
            idx += 1
            continue
        if line.startswith("#"):
            idx += 1
            continue
        break
    if idx >= len(lines):
        return None, ["missing module docstring Purpose header"]
    match = PY_DOCSTRING_START_RE.match(lines[idx].strip())
    if not match:
        return None, ["missing module docstring Purpose header"]
    delimiter = match.group(1)
    block_lines: list[str] = []
    start_line = lines[idx].strip()
    remainder = start_line[match.end() :]
    if delimiter in remainder:
        before, _ = remainder.split(delimiter, 1)
        block_lines.append(before)
        summary = extract_purpose_from_lines(block_lines)
        if not summary:
            return None, ["missing Purpose line in module docstring"]
        return summary, []
    idx += 1
    while idx < len(lines) and idx < max_scan_lines:
        line = lines[idx]
        if delimiter in line:
            before, _ = line.split(delimiter, 1)
            block_lines.append(before)
            break
        block_lines.append(line)
        idx += 1
    else:
        return None, ["unterminated module docstring"]
    summary = extract_purpose_from_lines(block_lines)
    if not summary:
        return None, ["missing Purpose line in module docstring"]
    return summary, []


def extract_vue_purpose(
    lines: list[str], max_scan_lines: int
) -> tuple[str | None, list[str]]:
    """Extract Purpose from the first Vue <script> or <style> block."""
    for tag in ("script", "style"):
        start = None
        for idx, line in enumerate(lines):
            if re.match(rf"\s*<{tag}\b", line):
                start = idx
                break
        if start is None:
            continue
        end = None
        for idx in range(start + 1, len(lines)):
            if re.match(rf"\s*</{tag}>", lines[idx]):
                end = idx
                break
        if end is None:
            return None, [f"unterminated <{tag}> block"]
        block_lines = lines[start + 1 : end]
        summary, issues = extract_javadoc_purpose(block_lines, max_scan_lines)
        if summary:
            return summary, issues
        return None, [f"missing Javadoc-style Purpose header in <{tag}> block"]
    return None, ["missing Javadoc-style Purpose header in <script> or <style> block"]


def extract_purpose_header(
    path: Path, config: AtlasConfig
) -> tuple[str | None, list[str]]:
    """Extract the Purpose summary for a file by type."""
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        text = path.read_text(encoding="utf-8", errors="replace")
    lines = text.splitlines()
    ext = file_extension(path)
    if ext == ".py":
        return extract_python_docstring_purpose(lines, config.max_scan_lines)
    if ext == ".vue":
        return extract_vue_purpose(lines, config.max_scan_lines)
    return extract_javadoc_purpose(lines, config.max_scan_lines)


def validate_summary(summary: str, config: AtlasConfig) -> list[str]:
    """Validate a Purpose summary against map rules."""
    problems: list[str] = []
    if not summary:
        problems.append("summary is empty")
    if config.summary_no_commas and "," in summary:
        problems.append("summary contains a comma")
    if config.summary_ascii_only and not summary.isascii():
        problems.append("summary contains non-ASCII characters")
    if len(summary) > config.summary_max_length:
        problems.append("summary exceeds length limit")
    return problems


def build_file_records(
    files: Sequence[Path],
    config: AtlasConfig,
) -> tuple[list[Record], list[str], dict[str, list[str]]]:
    """Build file records and error lists for Purpose headers."""
    records: list[Record] = []
    missing: list[str] = []
    invalid: dict[str, list[str]] = {}
    for path in files:
        rel = path.relative_to(config.root).as_posix()
        summary, header_issues = extract_purpose_header(path, config)
        if not summary:
            if any(
                issue.startswith("missing Javadoc-style")
                or issue.startswith("missing module docstring")
                for issue in header_issues
            ):
                missing.append(rel)
            else:
                invalid[rel] = header_issues
            records.append(Record(rel, "MISSING", "missing"))
            continue
        issues = validate_summary(summary, config)
        if issues:
            invalid[rel] = issues
            records.append(Record(rel, "INVALID", "invalid"))
            continue
        records.append(Record(rel, summary, "header"))
    return records, missing, invalid


def read_manual_file_entries(
    config: AtlasConfig,
) -> tuple[list[Record], list[str], dict[str, list[str]], list[str]]:
    """Read manual file entries for non-source config files."""
    if config.manual_files_path is None:
        return [], [], {}, []
    if not config.manual_files_path.exists():
        return [], [], {}, [
            f"manual file list missing: {config.manual_files_path}"
        ]
    entries: list[Record] = []
    missing: list[str] = []
    invalid: dict[str, list[str]] = {}
    in_manual = False
    for raw in config.manual_files_path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line:
            continue
        if line.startswith("#") or line.startswith("//"):
            continue
        if line.startswith("manual_files["):
            in_manual = True
            continue
        if line.startswith("folders[") or line.startswith("files["):
            in_manual = False
            continue
        if not in_manual or line.startswith("-") or ":" in line:
            continue
        parts = [part.strip() for part in line.split(",", 1)]
        if len(parts) != 2:
            continue
        rel_path, summary = parts
        rel_posix = Path(rel_path).as_posix()
        full_path = config.root / rel_posix
        if not full_path.exists():
            missing.append(rel_posix)
            entries.append(Record(rel_posix, "MISSING", "missing"))
            continue
        issues = validate_summary(summary, config)
        if issues:
            invalid[rel_posix] = issues
            entries.append(Record(rel_posix, "INVALID", "invalid"))
            continue
        entries.append(Record(rel_posix, summary, "manual"))
    return entries, missing, invalid, []


def merge_manual_file_records(
    records: Sequence[Record], manual: Sequence[Record]
) -> list[Record]:
    """Merge manual file records into the auto-discovered list."""
    merged: dict[str, Record] = {record.path: record for record in records}
    for record in manual:
        if record.path not in merged:
            merged[record.path] = record
    return sorted(merged.values(), key=lambda entry: entry.path)


def read_folder_purpose(
    folder_path: Path, config: AtlasConfig
) -> tuple[str | None, list[str]]:
    """Read a folder Purpose summary from a .purpose file."""
    purpose_path = folder_path / config.purpose_filename
    if not purpose_path.exists():
        return None, ["missing .purpose file"]
    summary = ""
    for raw in purpose_path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line:
            continue
        if line.startswith(PURPOSE_COMMENT_PREFIXES):
            continue
        summary = normalize_summary(raw)
        match = PURPOSE_RE.search(summary)
        if match:
            summary = normalize_summary(match.group(1))
        if summary:
            break
    if not summary:
        return "", ["missing Purpose summary"]
    issues = validate_summary(summary, config)
    if issues:
        return summary, issues
    return summary, []


def build_folder_records(
    folders: Sequence[Path],
    config: AtlasConfig,
) -> tuple[list[Record], list[str], dict[str, list[str]]]:
    """Build folder records from per-folder .purpose files."""
    records: list[Record] = []
    missing: list[str] = []
    invalid: dict[str, list[str]] = {}
    for folder_path in folders:
        rel = folder_path.relative_to(config.root).as_posix()
        summary, issues = read_folder_purpose(folder_path, config)
        if issues:
            if "missing .purpose file" in issues:
                missing.append(rel)
                records.append(Record(rel, "MISSING", "missing"))
            else:
                invalid[rel] = issues
                records.append(Record(rel, "INVALID", "invalid"))
            continue
        records.append(Record(rel, summary, "purpose"))
    return records, missing, invalid


def build_folder_tree_with_summaries(
    folders: Sequence[Path],
    summaries: dict[str, str],
    config: AtlasConfig,
) -> list[str]:
    """Build a stable tree snapshot with folder summaries."""
    entries: list[str] = []
    rel_paths = sorted(path.relative_to(config.root).as_posix() for path in folders)
    for rel in rel_paths:
        summary = summaries.get(rel, "MISSING")
        if rel == ".":
            entries.append(f". - {summary}")
            continue
        depth = rel.count("/")
        name = rel.split("/")[-1]
        entries.append(f"{'  ' * depth}{name}/ - {summary}")
    return entries


def build_summary_duplicates(records: Sequence[Record]) -> list[str]:
    """Find duplicate summaries across records."""
    grouped: dict[str, list[str]] = {}
    for record in records:
        summary = record.summary
        if summary in {"MISSING", "INVALID"}:
            continue
        grouped.setdefault(summary, []).append(record.path)
    duplicates: list[str] = []
    for summary, paths in sorted(grouped.items()):
        if len(paths) > 1:
            duplicates.append(f"{summary} :: {' | '.join(sorted(paths))}")
    return duplicates


def compute_file_hash(records: Sequence[Record]) -> str:
    """Compute the file list hash for the map output."""
    payload = "\n".join(f"{rec.path}|{rec.summary}" for rec in records)
    return hashlib.sha256(payload.encode("utf-8")).hexdigest()


def compute_folder_hash(paths: Sequence[Path], config: AtlasConfig) -> str:
    """Compute the folder list hash for the map output."""
    payload = "\n".join(path.relative_to(config.root).as_posix() for path in paths)
    return hashlib.sha256(payload.encode("utf-8")).hexdigest()


def compute_overview(
    folders: Sequence[Path], files: Sequence[Path], config: AtlasConfig
) -> dict[str, int]:
    """Compute overview counts for the map header."""
    return {
        "tracked_files": len(files),
        "tracked_folders": len(folders),
        "source_extensions": len(config.source_extensions),
        "exclude_dir_names": len(config.exclude_dir_names),
        "exclude_path_prefixes": len(config.exclude_path_prefixes),
    }


def format_overview(overview: dict[str, int]) -> str:
    """Format the overview mapping for the map header."""
    parts = [f"{key}={overview[key]}" for key in OVERVIEW_KEY_ORDER]
    return f"overview: {' '.join(parts)}"


def read_overview(map_path: Path) -> tuple[dict[str, int] | None, list[str]]:
    """Read the overview line from the map file."""
    if not map_path.exists():
        return None, ["atlas map is missing"]
    for raw in map_path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line.startswith("overview:"):
            continue
        payload = line.split(":", 1)[1].strip()
        if not payload:
            return None, ["overview line is empty"]
        overview: dict[str, int] = {}
        for token in payload.split():
            if "=" not in token:
                return None, ["overview line is malformed"]
            key, value = token.split("=", 1)
            if key not in OVERVIEW_KEY_ORDER:
                return None, ["overview contains unknown keys"]
            if not value.isdigit():
                return None, ["overview values must be integers"]
            overview[key] = int(value)
        if any(key not in overview for key in OVERVIEW_KEY_ORDER):
            return None, ["overview is missing required keys"]
        return overview, []
    return None, ["overview line is missing"]


def read_map_hashes(map_path: Path) -> tuple[str | None, str | None]:
    """Read file and folder hashes from the current map file."""
    if not map_path.exists():
        return None, None
    file_hash: str | None = None
    folder_hash: str | None = None
    for raw in map_path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line.startswith("file_hash:"):
            _, value = line.split(":", 1)
            file_hash = value.strip().strip('"')
        if line.startswith("folder_hash:"):
            _, value = line.split(":", 1)
            folder_hash = value.strip().strip('"')
    return file_hash, folder_hash


def format_list(items: Iterable[str]) -> str:
    """Format items as a newline-separated bullet list."""
    return "\n".join(f" - {item}" for item in items)
