"""
Purpose: Load and normalize ProjectAtlas configuration from TOML.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

import os

import tomllib


DEFAULT_PURPOSE_FILENAME = ".purpose"
DEFAULT_MAP_PATH = Path(".projectatlas/projectatlas.toon")
DEFAULT_NONSOURCE_FILES_PATH = Path(".projectatlas/projectatlas-nonsource-files.toon")
DEFAULT_SOURCE_EXTENSIONS = {
    ".ts",
    ".tsx",
    ".js",
    ".jsx",
    ".vue",
    ".css",
    ".mjs",
    ".cjs",
    ".d.ts",
    ".py",
    ".java",
    ".kt",
    ".scala",
    ".cs",
    ".go",
    ".rs",
    ".rb",
    ".php",
    ".c",
    ".h",
    ".cpp",
    ".hpp",
    ".cc",
    ".cxx",
    ".swift",
    ".lua",
    ".sh",
    ".bash",
    ".zsh",
    ".ps1",
    ".psm1",
    ".psd1",
    ".sql",
}
DEFAULT_EXCLUDE_DIR_NAMES = {
    ".cache",
    ".egg-info",
    ".git",
    ".idea",
    ".mypy_cache",
    ".projectatlas",
    ".pytest_cache",
    ".tmp",
    ".venv",
    "__pycache__",
    "artifacts",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "sandbox",
    "temp",
    "test-results",
    "tmp",
}
DEFAULT_EXCLUDE_DIR_SUFFIXES = {".egg-info"}
DEFAULT_EXCLUDE_PATH_PREFIXES: set[str] = set()
DEFAULT_NON_SOURCE_PATH_PREFIXES: set[str] = set()
DEFAULT_ALLOWED_UNTRACKED_FILENAMES = {DEFAULT_PURPOSE_FILENAME}
DEFAULT_UNTRACKED_ALLOWLIST_DIR_PREFIXES: set[str] = set()
DEFAULT_UNTRACKED_ALLOWLIST_FILES: set[str] = set()
DEFAULT_ASSET_ALLOWED_PREFIXES: set[str] = set()
DEFAULT_ASSET_EXTENSIONS = {
    ".bmp",
    ".gif",
    ".ico",
    ".jpeg",
    ".jpg",
    ".pdf",
    ".png",
    ".svg",
    ".ttf",
    ".webp",
    ".woff",
    ".woff2",
}
DEFAULT_MAX_SCAN_LINES = 80
DEFAULT_SUMMARY_MAX_LENGTH = 140
DEFAULT_SUMMARY_ASCII_ONLY = True
DEFAULT_SUMMARY_NO_COMMAS = True
DEFAULT_PURPOSE_DEFAULT_STYLE = "javadoc"
DEFAULT_LINE_COMMENT_PREFIXES = ("//", "#", "--", ";")
DEFAULT_PURPOSE_STYLES = {
    ".py": "python-docstring",
    ".vue": "vue-block",
    ".go": "line-comment",
    ".rs": "line-comment",
    ".rb": "line-comment",
    ".sh": "line-comment",
    ".bash": "line-comment",
    ".zsh": "line-comment",
    ".ps1": "line-comment",
    ".psm1": "line-comment",
    ".psd1": "line-comment",
    ".lua": "line-comment",
    ".sql": "line-comment",
    ".cs": "line-comment",
    ".swift": "line-comment",
    ".c": "block-comment",
    ".h": "block-comment",
    ".cpp": "block-comment",
    ".hpp": "block-comment",
    ".cc": "block-comment",
    ".cxx": "block-comment",
    ".php": "block-comment",
    ".java": "javadoc",
    ".kt": "javadoc",
    ".scala": "javadoc",
}


class ConfigError(ValueError):
    """Signal an invalid ProjectAtlas configuration."""


@dataclass(frozen=True)
class AtlasConfig:
    """Define the normalized configuration for ProjectAtlas."""

    root: Path
    map_path: Path
    nonsource_files_path: Path | None
    purpose_filename: str
    source_extensions: set[str]
    exclude_dir_names: set[str]
    exclude_dir_suffixes: set[str]
    exclude_path_prefixes: set[str]
    non_source_path_prefixes: set[str]
    allowed_untracked_filenames: set[str]
    untracked_allowlist_dir_prefixes: set[str]
    untracked_allowlist_files: set[str]
    asset_allowed_prefixes: set[str]
    asset_extensions: set[str]
    max_scan_lines: int
    summary_max_length: int
    summary_ascii_only: bool
    summary_no_commas: bool
    purpose_styles: dict[str, str]
    purpose_default_style: str
    line_comment_prefixes: tuple[str, ...]


def find_config_path(root: Path) -> Path | None:
    """Locate the first ProjectAtlas config file under the root."""
    candidates = [
        root / ".projectatlas" / "config.toml",
        root / "projectatlas.toml",
    ]
    for candidate in candidates:
        if candidate.exists():
            return candidate
    return None


def _as_set(raw: Any, field: str) -> set[str]:
    if raw is None:
        return set()
    if not isinstance(raw, list):
        raise ConfigError(f"{field} must be a list")
    return {str(item) for item in raw}


def _as_list(raw: Any, field: str) -> list[str]:
    if raw is None:
        return []
    if not isinstance(raw, list):
        raise ConfigError(f"{field} must be a list")
    return [str(item) for item in raw]


def _as_map(raw: Any, field: str) -> dict[str, str]:
    if raw is None:
        return {}
    if not isinstance(raw, dict):
        raise ConfigError(f"{field} must be a table")
    return {str(key): str(value) for key, value in raw.items()}


def _as_path(raw: Any, field: str, base: Path) -> Path:
    if raw is None:
        raise ConfigError(f"{field} is required")
    value = Path(str(raw))
    if not value.is_absolute():
        value = (base / value).resolve()
    return value


def load_config(config_path: Path | None, root: Path | None = None) -> AtlasConfig:
    """Load ProjectAtlas configuration from TOML (or defaults)."""
    root_dir = root or Path.cwd()
    config_file = config_path or find_config_path(root_dir)
    data: dict[str, Any] = {}
    if config_file:
        data = tomllib.loads(config_file.read_text(encoding="utf-8"))
        base_dir = config_file.parent
    else:
        base_dir = root_dir

    project = data.get("project", {})
    scan = data.get("scan", {})
    summary = data.get("summary_rules", {})
    untracked = data.get("untracked", {})
    purpose = data.get("purpose", {})

    root_path = project.get("root")
    if root_path is None:
        if config_file and config_file.parent.name == ".projectatlas":
            root_path = config_file.parent.parent.resolve()
        elif config_file:
            root_path = config_file.parent.resolve()
        else:
            root_path = root_dir
    else:
        if (
            config_file
            and str(root_path).strip() in {".", "./"}
            and config_file.parent.name == ".projectatlas"
        ):
            root_path = config_file.parent.parent.resolve()
        else:
            root_path = _as_path(root_path, "project.root", base_dir)

    map_path = project.get("map_path", DEFAULT_MAP_PATH)
    map_path = _as_path(map_path, "project.map_path", root_path)

    nonsource_files_path = project.get("nonsource_files_path")
    if nonsource_files_path is None:
        nonsource_files_path = project.get("manual_files_path")
    if nonsource_files_path is None:
        nonsource_path = None
    else:
        nonsource_path = _as_path(
            nonsource_files_path,
            "project.nonsource_files_path",
            root_path,
        )

    purpose_filename = str(project.get("purpose_filename", DEFAULT_PURPOSE_FILENAME))

    source_extensions = _as_set(scan.get("source_extensions"), "scan.source_extensions")
    if not source_extensions:
        source_extensions = set(DEFAULT_SOURCE_EXTENSIONS)

    exclude_dir_names = _as_set(scan.get("exclude_dir_names"), "scan.exclude_dir_names")
    if not exclude_dir_names:
        exclude_dir_names = set(DEFAULT_EXCLUDE_DIR_NAMES)

    exclude_dir_suffixes = _as_set(
        scan.get("exclude_dir_suffixes"), "scan.exclude_dir_suffixes"
    )
    if not exclude_dir_suffixes:
        exclude_dir_suffixes = set(DEFAULT_EXCLUDE_DIR_SUFFIXES)

    exclude_path_prefixes = _as_set(
        scan.get("exclude_path_prefixes"), "scan.exclude_path_prefixes"
    )
    if not exclude_path_prefixes:
        exclude_path_prefixes = set(DEFAULT_EXCLUDE_PATH_PREFIXES)

    non_source_path_prefixes = _as_set(
        scan.get("non_source_path_prefixes"), "scan.non_source_path_prefixes"
    )
    if not non_source_path_prefixes:
        non_source_path_prefixes = set(DEFAULT_NON_SOURCE_PATH_PREFIXES)

    purpose_styles = _as_map(
        purpose.get("styles_by_extension"), "purpose.styles_by_extension"
    )
    if not purpose_styles:
        purpose_styles = dict(DEFAULT_PURPOSE_STYLES)

    purpose_default_style = str(
        purpose.get("default_style", DEFAULT_PURPOSE_DEFAULT_STYLE)
    )

    line_comment_prefixes = _as_list(
        purpose.get("line_comment_prefixes"), "purpose.line_comment_prefixes"
    )
    if not line_comment_prefixes:
        line_comment_prefixes = list(DEFAULT_LINE_COMMENT_PREFIXES)

    allowed_untracked_filenames = _as_set(
        untracked.get("allowed_filenames"), "untracked.allowed_filenames"
    )
    if not allowed_untracked_filenames:
        allowed_untracked_filenames = set(DEFAULT_ALLOWED_UNTRACKED_FILENAMES)

    untracked_allowlist_dir_prefixes = _as_set(
        untracked.get("allowlist_dir_prefixes"), "untracked.allowlist_dir_prefixes"
    )
    if not untracked_allowlist_dir_prefixes:
        untracked_allowlist_dir_prefixes = set(
            DEFAULT_UNTRACKED_ALLOWLIST_DIR_PREFIXES
        )

    untracked_allowlist_files = _as_set(
        untracked.get("allowlist_files"), "untracked.allowlist_files"
    )
    if not untracked_allowlist_files:
        untracked_allowlist_files = set(DEFAULT_UNTRACKED_ALLOWLIST_FILES)

    asset_allowed_prefixes = _as_set(
        untracked.get("asset_allowed_prefixes"), "untracked.asset_allowed_prefixes"
    )
    if not asset_allowed_prefixes:
        asset_allowed_prefixes = set(DEFAULT_ASSET_ALLOWED_PREFIXES)

    asset_extensions = _as_set(
        untracked.get("asset_extensions"), "untracked.asset_extensions"
    )
    if not asset_extensions:
        asset_extensions = set(DEFAULT_ASSET_EXTENSIONS)

    max_scan_lines = int(scan.get("max_scan_lines", DEFAULT_MAX_SCAN_LINES))
    summary_max_length = int(
        summary.get("max_length", DEFAULT_SUMMARY_MAX_LENGTH)
    )
    summary_ascii_only = bool(
        summary.get("ascii_only", DEFAULT_SUMMARY_ASCII_ONLY)
    )
    summary_no_commas = bool(
        summary.get("no_commas", DEFAULT_SUMMARY_NO_COMMAS)
    )

    return AtlasConfig(
        root=root_path,
        map_path=map_path,
        nonsource_files_path=nonsource_path,
        purpose_filename=purpose_filename,
        source_extensions=source_extensions,
        exclude_dir_names=exclude_dir_names,
        exclude_dir_suffixes=exclude_dir_suffixes,
        exclude_path_prefixes=exclude_path_prefixes,
        non_source_path_prefixes=non_source_path_prefixes,
        allowed_untracked_filenames=allowed_untracked_filenames,
        untracked_allowlist_dir_prefixes=untracked_allowlist_dir_prefixes,
        untracked_allowlist_files=untracked_allowlist_files,
        asset_allowed_prefixes=asset_allowed_prefixes,
        asset_extensions=asset_extensions,
        max_scan_lines=max_scan_lines,
        summary_max_length=summary_max_length,
        summary_ascii_only=summary_ascii_only,
        summary_no_commas=summary_no_commas,
        purpose_styles=purpose_styles,
        purpose_default_style=purpose_default_style,
        line_comment_prefixes=tuple(line_comment_prefixes),
    )


def default_config_text() -> str:
    """Return a config.toml payload for the provided settings."""
    return build_config_text()


def build_config_text(
    source_extensions: Iterable[str] | None = None,
    purpose_styles: dict[str, str] | None = None,
    default_style: str | None = None,
    line_comment_prefixes: Iterable[str] | None = None,
) -> str:
    """Build a config.toml payload from the supplied settings."""
    extensions = sorted(source_extensions or DEFAULT_SOURCE_EXTENSIONS)
    style_map = purpose_styles or {}
    default_style = default_style or DEFAULT_PURPOSE_DEFAULT_STYLE
    prefixes = list(line_comment_prefixes or DEFAULT_LINE_COMMENT_PREFIXES)

    def format_list(values: Iterable[str]) -> str:
        return "[" + ", ".join(f"\"{value}\"" for value in values) + "]"

    lines = [
        "[project]",
        'root = "."',
        'map_path = ".projectatlas/projectatlas.toon"',
        'nonsource_files_path = ".projectatlas/projectatlas-nonsource-files.toon"',
        'purpose_filename = ".purpose"',
        "",
        "[scan]",
        f"source_extensions = {format_list(extensions)}",
        "exclude_dir_names = [\".git\", \".projectatlas\", \".venv\", \"__pycache__\", \".egg-info\", \"node_modules\", \"dist\", \"build\"]",
        "exclude_dir_suffixes = [\".egg-info\"]",
        "exclude_path_prefixes = []",
        "non_source_path_prefixes = []",
        "max_scan_lines = 80",
        "",
        "[purpose]",
        f"default_style = \"{default_style}\"",
        f"line_comment_prefixes = {format_list(prefixes)}",
    ]
    if style_map:
        lines.append("")
        lines.append("[purpose.styles_by_extension]")
        for ext, style in sorted(style_map.items()):
            lines.append(f"\"{ext}\" = \"{style}\"")
    else:
        lines.append("# styles_by_extension = { \".go\" = \"line-comment\" }")
    lines.extend(
        [
            "",
            "[summary_rules]",
            "ascii_only = true",
            "no_commas = true",
            "max_length = 140",
            "",
            "[untracked]",
            "allowed_filenames = [\".purpose\"]",
            "allowlist_dir_prefixes = []",
            "allowlist_files = []",
            "asset_allowed_prefixes = []",
            f"asset_extensions = {format_list(sorted(DEFAULT_ASSET_EXTENSIONS))}",
        ]
    )
    return "\n".join(lines) + "\n"


def detect_language_extensions(root: Path) -> set[str]:
    """Detect source extensions in a repo using the default language list."""
    detected: set[str] = set()
    for root_dir, dirnames, filenames in os.walk(root):
        root_dir_path = Path(root_dir)
        rel_root = root_dir_path.relative_to(root)
        if rel_root != Path(".") and any(
            part in DEFAULT_EXCLUDE_DIR_NAMES for part in rel_root.parts
        ):
            dirnames[:] = []
            continue
        dirnames[:] = [
            name
            for name in dirnames
            if name not in DEFAULT_EXCLUDE_DIR_NAMES
            and not any(name.endswith(suffix) for suffix in DEFAULT_EXCLUDE_DIR_SUFFIXES)
        ]
        for name in filenames:
            ext = ".d.ts" if name.endswith(".d.ts") else Path(name).suffix.lower()
            if not ext:
                continue
            if ext in DEFAULT_SOURCE_EXTENSIONS:
                detected.add(ext)
    return detected


def detect_purpose_styles(extensions: Iterable[str]) -> dict[str, str]:
    """Return a Purpose style map for detected extensions."""
    return {ext: DEFAULT_PURPOSE_STYLES[ext] for ext in extensions if ext in DEFAULT_PURPOSE_STYLES}
