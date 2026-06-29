//! Purpose: Generate and lint `ProjectAtlas` structure maps from Rust.

use blake3::Hasher;
use projectatlas_core::{
    NodeKind,
    language::{BROAD_SOURCE_EXTENSIONS, detect_language},
    validated_repo_file_key,
};
use projectatlas_db::AtlasStore;
use projectatlas_fs::{ScanOptions, scan_repo};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use toml_edit::{Array, DocumentMut, Item, Table, value};

/// Default `ProjectAtlas` purpose filename.
const DEFAULT_PURPOSE_FILENAME: &str = ".purpose";
/// Default generated map path.
const DEFAULT_MAP_PATH: &str = ".projectatlas/projectatlas.toon";
/// Default non-source summary input path.
const DEFAULT_NONSOURCE_PATH: &str = ".projectatlas/projectatlas-nonsource-files.toon";
/// Durable `.projectatlas` inputs indexed by `SQLite` but ignored by legacy map/lint.
const DURABLE_PROJECTATLAS_INPUT_PATHS: &[&str] = &[
    ".projectatlas/config.toml",
    ".projectatlas/projectatlas-nonsource-files.toon",
];
/// Default maximum number of lines scanned for purpose headers.
const DEFAULT_MAX_SCAN_LINES: usize = 80;
/// Default maximum UTF-8 file size persisted into `SQLite` text search.
pub(crate) const DEFAULT_TEXT_INDEX_MAX_BYTES: u64 = 2_000_000;
/// Default maximum purpose summary length.
const DEFAULT_SUMMARY_MAX_LENGTH: usize = 140;
/// Ordered overview keys written into the TOON map.
const OVERVIEW_KEYS: &[&str] = &[
    "tracked_source_files",
    "tracked_nonsource_files",
    "tracked_files_total",
    "tracked_folders",
    "source_extensions",
    "exclude_dir_names",
    "exclude_path_prefixes",
];
/// Source extensions scanned for Purpose metadata by default.
const DEFAULT_SOURCE_EXTENSIONS: &[&str] = BROAD_SOURCE_EXTENSIONS;
/// Directory names excluded from scans even when config is hand-edited.
const REQUIRED_EXCLUDE_DIR_NAMES: &[&str] = &[".git", ".projectatlas"];
/// Directory names excluded from scans by default.
const DEFAULT_EXCLUDE_DIR_NAMES: &[&str] = &[
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
    "target",
    "temp",
    "test-results",
    "tmp",
];
/// Asset extensions recognized for untracked-file reporting.
const DEFAULT_ASSET_EXTENSIONS: &[&str] = &[
    ".bmp", ".gif", ".ico", ".jpeg", ".jpg", ".pdf", ".png", ".svg", ".ttf", ".webp", ".woff",
    ".woff2",
];
/// Line comment prefixes supported by default.
const DEFAULT_LINE_COMMENT_PREFIXES: &[&str] = &["//", "#", "--", ";"];

/// Atlas map operation errors.
#[derive(Debug, Error)]
pub(crate) enum AtlasMapError {
    /// Filesystem operation failed.
    #[error("io error for {path:?}: {source}")]
    Io {
        /// Path involved in the failed operation.
        path: PathBuf,
        /// Source IO error.
        source: std::io::Error,
    },
    /// TOML parsing failed.
    #[error("toml parse error for {path:?}: {source}")]
    Toml {
        /// TOML path that failed to parse.
        path: PathBuf,
        /// Source TOML parse error.
        source: Box<toml::de::Error>,
    },
    /// Filesystem scanner failed.
    #[error("{0}")]
    Scan(#[from] projectatlas_fs::FsError),
    /// Durable index read failed.
    #[error("database error for {path:?}: {message}")]
    Database {
        /// Database path that failed.
        path: PathBuf,
        /// Source database error text.
        message: String,
    },
    /// Config or manual metadata referenced an unsafe repository path.
    #[error("invalid repository-relative path {path:?}: {message}")]
    InvalidRepositoryPath {
        /// Invalid path text.
        path: String,
        /// Validation failure.
        message: String,
    },
    /// Editable TOML config was malformed for the requested operation.
    #[error("toml edit error for {path:?}: {message}")]
    TomlEdit {
        /// Config path that failed to edit.
        path: PathBuf,
        /// TOML edit failure.
        message: String,
    },
}

/// Result alias for atlas map operations.
type AtlasMapResult<T> = Result<T, AtlasMapError>;

/// Raw deserialized config file.
#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    /// Project table.
    project: Option<RawProject>,
    /// Scan table.
    scan: Option<RawScan>,
    /// Purpose table.
    purpose: Option<RawPurpose>,
    /// Summary rules table.
    summary_rules: Option<RawSummaryRules>,
    /// Untracked policy table.
    untracked: Option<RawUntracked>,
}

/// Raw project table.
#[derive(Debug, Default, Deserialize)]
struct RawProject {
    /// Repository root.
    root: Option<String>,
    /// Generated map path.
    map_path: Option<String>,
    /// Non-source file summary path.
    nonsource_files_path: Option<String>,
    /// Legacy manual file summary path.
    manual_files_path: Option<String>,
    /// Purpose filename.
    purpose_filename: Option<String>,
}

/// Raw scan table.
#[derive(Debug, Default, Deserialize)]
struct RawScan {
    /// Source extensions.
    source_extensions: Option<Vec<String>>,
    /// Excluded directory names.
    exclude_dir_names: Option<Vec<String>>,
    /// Excluded directory suffixes.
    exclude_dir_suffixes: Option<Vec<String>>,
    /// Excluded path prefixes.
    exclude_path_prefixes: Option<Vec<String>>,
    /// Non-source path prefixes.
    non_source_path_prefixes: Option<Vec<String>>,
    /// Maximum header scan lines.
    max_scan_lines: Option<usize>,
    /// Maximum UTF-8 file size persisted into `SQLite` text search.
    text_index_max_bytes: Option<u64>,
}

/// Raw purpose table.
#[derive(Debug, Default, Deserialize)]
struct RawPurpose {
    /// Default purpose header style.
    default_style: Option<String>,
    /// Line-comment prefixes.
    line_comment_prefixes: Option<Vec<String>>,
    /// Per-extension purpose styles.
    styles_by_extension: Option<BTreeMap<String, String>>,
}

/// Raw summary-rules table.
#[derive(Debug, Default, Deserialize)]
struct RawSummaryRules {
    /// Whether purpose summaries must be ASCII.
    ascii_only: Option<bool>,
    /// Whether purpose summaries may contain commas.
    no_commas: Option<bool>,
    /// Maximum purpose summary length.
    max_length: Option<usize>,
}

/// Raw untracked table.
#[derive(Debug, Default, Deserialize)]
struct RawUntracked {
    /// Allowed untracked filenames.
    allowed_filenames: Option<Vec<String>>,
    /// Allowed untracked directory prefixes.
    allowlist_dir_prefixes: Option<Vec<String>>,
    /// Allowed untracked files.
    allowlist_files: Option<Vec<String>>,
    /// Allowed asset prefixes.
    asset_allowed_prefixes: Option<Vec<String>>,
    /// Asset extensions.
    asset_extensions: Option<Vec<String>>,
}

/// Normalized atlas map configuration.
#[derive(Clone, Debug)]
pub(crate) struct AtlasMapConfig {
    /// Repository root.
    pub(crate) root: PathBuf,
    /// Generated TOON map path.
    pub(crate) map_path: PathBuf,
    /// Non-source file summary path.
    pub(crate) nonsource_files_path: PathBuf,
    /// Purpose filename.
    purpose_filename: String,
    /// Source extensions that require purpose headers.
    source_extensions: BTreeSet<String>,
    /// Excluded directory names.
    exclude_dir_names: BTreeSet<String>,
    /// Excluded directory suffixes.
    exclude_dir_suffixes: BTreeSet<String>,
    /// Excluded repository-relative prefixes.
    exclude_path_prefixes: BTreeSet<String>,
    /// Prefixes treated as non-source even when extensions match.
    non_source_path_prefixes: BTreeSet<String>,
    /// Allowed untracked filenames.
    allowed_untracked_filenames: BTreeSet<String>,
    /// Allowed untracked directory prefixes.
    untracked_allowlist_dir_prefixes: BTreeSet<String>,
    /// Allowed untracked files.
    untracked_allowlist_files: BTreeSet<String>,
    /// Allowed asset root prefixes.
    asset_allowed_prefixes: BTreeSet<String>,
    /// Asset extensions.
    asset_extensions: BTreeSet<String>,
    /// Durable `SQLite` index path.
    db_path: PathBuf,
    /// Maximum lines to scan for purpose headers.
    max_scan_lines: usize,
    /// Maximum UTF-8 file size persisted into `SQLite` text search.
    text_index_max_bytes: u64,
    /// Maximum purpose summary length.
    summary_max_length: usize,
    /// Whether summaries must be ASCII.
    summary_ascii_only: bool,
    /// Whether summaries may contain commas.
    summary_no_commas: bool,
    /// Per-extension purpose styles.
    purpose_styles: BTreeMap<String, String>,
    /// Default purpose style.
    purpose_default_style: String,
    /// Supported line-comment prefixes.
    line_comment_prefixes: Vec<String>,
}

impl AtlasMapConfig {
    /// Return scanner options derived from the normalized project config.
    pub(crate) fn scan_options(&self) -> ScanOptions {
        ScanOptions {
            exclude_dir_names: self.exclude_dir_names.iter().cloned().collect(),
            exclude_path_prefixes: self.exclude_path_prefixes.iter().cloned().collect(),
        }
    }

    /// Return the configured maximum UTF-8 file size for `SQLite` text search.
    pub(crate) fn text_index_max_bytes(&self) -> u64 {
        self.text_index_max_bytes
    }
}

/// Serializable view of the effective `ProjectAtlas` configuration.
#[derive(Debug, Serialize)]
pub(crate) struct EffectiveConfigReport {
    /// Repository root.
    pub(crate) root: String,
    /// Generated TOON map path.
    pub(crate) map_path: String,
    /// Non-source purpose registry path.
    pub(crate) nonsource_files_path: String,
    /// Durable `SQLite` index path.
    pub(crate) db_path: String,
    /// Purpose metadata filename.
    pub(crate) purpose_filename: String,
    /// Source extensions treated as indexable project content.
    pub(crate) source_extensions: Vec<String>,
    /// Directory names excluded from normal scans.
    pub(crate) exclude_dir_names: Vec<String>,
    /// Repository-relative path prefixes excluded from normal scans.
    pub(crate) exclude_path_prefixes: Vec<String>,
    /// Configured non-source path prefixes.
    pub(crate) non_source_path_prefixes: Vec<String>,
    /// Default purpose style.
    pub(crate) purpose_default_style: String,
    /// Per-extension purpose style overrides.
    pub(crate) purpose_styles: BTreeMap<String, String>,
    /// Supported line comment prefixes for purpose headers.
    pub(crate) line_comment_prefixes: Vec<String>,
    /// Maximum file size persisted into `SQLite` text search.
    pub(crate) text_index_max_bytes: u64,
    /// Maximum purpose line length.
    pub(crate) summary_max_length: usize,
    /// Whether purpose summaries must be ASCII.
    pub(crate) summary_ascii_only: bool,
    /// Whether purpose summaries may not contain commas.
    pub(crate) summary_no_commas: bool,
}

/// Build the effective configuration report used by agents and docs.
pub(crate) fn effective_config_report(config: &AtlasMapConfig) -> EffectiveConfigReport {
    EffectiveConfigReport {
        root: config.root.display().to_string(),
        map_path: config.map_path.display().to_string(),
        nonsource_files_path: config.nonsource_files_path.display().to_string(),
        db_path: config.db_path.display().to_string(),
        purpose_filename: config.purpose_filename.clone(),
        source_extensions: config.source_extensions.iter().cloned().collect(),
        exclude_dir_names: config.exclude_dir_names.iter().cloned().collect(),
        exclude_path_prefixes: config.exclude_path_prefixes.iter().cloned().collect(),
        non_source_path_prefixes: config.non_source_path_prefixes.iter().cloned().collect(),
        purpose_default_style: config.purpose_default_style.clone(),
        purpose_styles: config.purpose_styles.clone(),
        line_comment_prefixes: config.line_comment_prefixes.clone(),
        text_index_max_bytes: config.text_index_max_bytes,
        summary_max_length: config.summary_max_length,
        summary_ascii_only: config.summary_ascii_only,
        summary_no_commas: config.summary_no_commas,
    }
}

/// `ProjectAtlas` map record.
#[derive(Clone, Debug, Eq, PartialEq)]
struct MapRecord {
    /// Repository-relative path.
    path: String,
    /// One-line purpose summary.
    summary: String,
    /// Source of the summary.
    source: String,
}

/// Snapshot written to `ProjectAtlas` TOON.
#[derive(Debug)]
struct AtlasSnapshot {
    /// Folder records.
    folder_records: Vec<MapRecord>,
    /// File records.
    file_records: Vec<MapRecord>,
    /// Folder tree lines.
    folder_tree: Vec<String>,
    /// Duplicate folder summaries.
    folder_duplicates: Vec<String>,
    /// Duplicate file summaries.
    file_duplicates: Vec<String>,
    /// File record hash.
    file_hash: String,
    /// Folder record hash.
    folder_hash: String,
    /// Generated timestamp.
    generated_at: String,
    /// Overview counters.
    overview: BTreeMap<String, usize>,
}

/// Result of collecting repository paths.
#[derive(Debug)]
struct RepoPaths {
    /// Folder paths.
    folders: Vec<String>,
    /// Source file paths.
    source_files: Vec<String>,
    /// Non-source file paths.
    untracked_files: Vec<String>,
    /// Excluded paths that exist.
    excluded_paths: Vec<String>,
}

/// Parsed non-source entry set and validation state.
#[derive(Debug)]
struct NonsourceEntries {
    /// Valid or placeholder records.
    records: Vec<MapRecord>,
    /// Entries pointing to missing paths.
    missing: Vec<String>,
    /// Entries with invalid summaries.
    invalid: BTreeMap<String, Vec<String>>,
    /// File-level parsing errors.
    errors: Vec<String>,
}

/// Lint options supplied by the CLI.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LintOptions {
    /// Whether missing folder purpose files fail lint.
    pub(crate) strict_folders: bool,
    /// Whether to print untracked-file report.
    pub(crate) report_untracked: bool,
    /// Whether untracked files fail lint.
    pub(crate) strict_untracked: bool,
}

/// Purpose record imported from legacy `ProjectAtlas` metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ImportedPurposeRecord {
    /// Repository-relative path.
    pub(crate) path: String,
    /// Imported purpose summary.
    pub(crate) summary: String,
}

/// Manual `ProjectAtlas` ignore entry kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IgnoreEntryKind {
    /// Exclude every directory with this name anywhere under the project root.
    DirName,
    /// Exclude one repository-relative path subtree.
    PathPrefix,
}

impl IgnoreEntryKind {
    /// Stable config key for this ignore kind.
    fn config_key(self) -> &'static str {
        match self {
            Self::DirName => "exclude_dir_names",
            Self::PathPrefix => "exclude_path_prefixes",
        }
    }

    /// Agent-facing ignore kind name.
    fn as_str(self) -> &'static str {
        match self {
            Self::DirName => "dir-name",
            Self::PathPrefix => "path-prefix",
        }
    }
}

/// Current `ProjectAtlas` ignore configuration report.
#[derive(Debug, Serialize)]
pub(crate) struct IgnoreListReport {
    /// Config file used for the manual `ProjectAtlas` ignore layer.
    pub(crate) config_path: String,
    /// `.gitignore` file that the scanner will honor when it exists.
    pub(crate) gitignore_path: String,
    /// Whether a `.gitignore` file currently exists at the project root.
    pub(crate) gitignore_present: bool,
    /// Scanner behavior for `.gitignore`.
    pub(crate) gitignore_mode: String,
    /// Order of the manual `ProjectAtlas` ignore layer.
    pub(crate) manual_layer_order: String,
    /// Effective directory-name excludes after defaults and config are applied.
    pub(crate) exclude_dir_names: Vec<String>,
    /// Effective repository-relative path-prefix excludes.
    pub(crate) exclude_path_prefixes: Vec<String>,
}

/// Result of creating a project-root `.gitignore` when it is missing.
#[derive(Debug, Serialize)]
pub(crate) struct GitignoreInitReport {
    /// `.gitignore` path that was checked.
    pub(crate) gitignore_path: String,
    /// Whether the file already existed before the command.
    pub(crate) existed: bool,
    /// Whether the command created the file.
    pub(crate) created: bool,
    /// Whether `.gitignore` rules are inherited dynamically by the scanner.
    pub(crate) gitignore_inherited: bool,
}

/// Result of adding or removing a manual `ProjectAtlas` ignore entry.
#[derive(Debug, Serialize)]
pub(crate) struct IgnoreMutationReport {
    /// Config file that was edited.
    pub(crate) config_path: String,
    /// `.gitignore` file that the scanner will honor when it exists.
    pub(crate) gitignore_path: String,
    /// Whether a `.gitignore` file currently exists at the project root.
    pub(crate) gitignore_present: bool,
    /// Mutation action.
    pub(crate) action: String,
    /// Ignore kind that was targeted, or `any` for a broad remove.
    pub(crate) kind: String,
    /// Normalized ignore value.
    pub(crate) value: String,
    /// Whether the config file changed.
    pub(crate) changed: bool,
    /// Scanner behavior for `.gitignore`.
    pub(crate) gitignore_mode: String,
    /// Order of the manual `ProjectAtlas` ignore layer.
    pub(crate) manual_layer_order: String,
    /// Effective directory-name excludes after the mutation.
    pub(crate) exclude_dir_names: Vec<String>,
    /// Effective repository-relative path-prefix excludes after the mutation.
    pub(crate) exclude_path_prefixes: Vec<String>,
}

/// Load atlas map configuration from disk.
pub(crate) fn load_atlas_config(config_path: Option<&Path>) -> AtlasMapResult<AtlasMapConfig> {
    let cwd = std::env::current_dir().map_err(|source| AtlasMapError::Io {
        path: PathBuf::from("."),
        source,
    })?;
    let config_file = match config_path {
        Some(path) => Some(path.to_path_buf()),
        None => find_config_path(&cwd),
    };
    let (raw, base_dir) = if let Some(path) = &config_file {
        let text = fs::read_to_string(path).map_err(|source| AtlasMapError::Io {
            path: path.clone(),
            source,
        })?;
        let parsed = toml::from_str::<RawConfig>(&text).map_err(|source| AtlasMapError::Toml {
            path: path.clone(),
            source: Box::new(source),
        })?;
        let parent = path.parent().map_or_else(|| cwd.clone(), Path::to_path_buf);
        (parsed, parent)
    } else {
        (RawConfig::default(), cwd.clone())
    };
    normalize_config(raw, config_file.as_deref(), &base_dir, &cwd)
}

/// Load atlas map configuration for an explicit project root.
pub(crate) fn load_atlas_config_for_root(root: &Path) -> AtlasMapResult<AtlasMapConfig> {
    if let Some(config_path) = find_config_path(root) {
        return load_atlas_config(Some(&config_path));
    }
    normalize_config(RawConfig::default(), None, root, root)
}

/// Write default `ProjectAtlas` config files.
pub(crate) fn init_project(root: &Path) -> AtlasMapResult<String> {
    let project_dir = root.join(".projectatlas");
    fs::create_dir_all(&project_dir).map_err(|source| AtlasMapError::Io {
        path: project_dir.clone(),
        source,
    })?;
    let config_path = project_dir.join("config.toml");
    if !config_path.exists() {
        fs::write(&config_path, default_config_text()).map_err(|source| AtlasMapError::Io {
            path: config_path.clone(),
            source,
        })?;
    }
    let nonsource_path = project_dir.join("projectatlas-nonsource-files.toon");
    if !nonsource_path.exists() {
        fs::write(&nonsource_path, "nonsource_files[]:\n  # path,summary\n").map_err(|source| {
            AtlasMapError::Io {
                path: nonsource_path.clone(),
                source,
            }
        })?;
    }
    Ok(String::new())
}

/// List effective `ProjectAtlas` ignore policy.
pub(crate) fn list_ignore_entries(
    config_path: Option<&Path>,
    project_root: &Path,
) -> AtlasMapResult<IgnoreListReport> {
    let path = resolve_config_edit_path(config_path, project_root)?;
    let config = if path.exists() {
        load_atlas_config(Some(&path))?
    } else {
        load_atlas_config_for_root(project_root)?
    };
    Ok(ignore_list_report(&path, &config))
}

/// Create a project-root `.gitignore` when it is missing.
pub(crate) fn init_gitignore(
    config_path: Option<&Path>,
    project_root: &Path,
) -> AtlasMapResult<GitignoreInitReport> {
    let path = resolve_config_edit_path(config_path, project_root)?;
    let config = if path.exists() {
        load_atlas_config(Some(&path))?
    } else {
        load_atlas_config_for_root(project_root)?
    };
    let gitignore_path = config.root.join(".gitignore");
    let existed = gitignore_path.exists();
    if !existed {
        fs::write(&gitignore_path, default_gitignore_text()).map_err(|source| {
            AtlasMapError::Io {
                path: gitignore_path.clone(),
                source,
            }
        })?;
    }
    Ok(GitignoreInitReport {
        gitignore_path: gitignore_path.display().to_string(),
        existed,
        created: !existed,
        gitignore_inherited: true,
    })
}

/// Add one manual `ProjectAtlas` ignore entry to config.
pub(crate) fn add_ignore_entry(
    config_path: Option<&Path>,
    project_root: &Path,
    kind: IgnoreEntryKind,
    value: &str,
) -> AtlasMapResult<IgnoreMutationReport> {
    let normalized = normalize_ignore_value(kind, value)?;
    let path = resolve_config_edit_path(config_path, project_root)?;
    let mut document = load_config_document_for_edit(&path)?;
    let mut values = string_array_values(&path, &document, kind)?;
    let changed = values.insert(normalized.clone());
    if changed {
        write_string_array(&mut document, kind.config_key(), &values)?;
        write_config_document(&path, &document)?;
    }
    let config = load_atlas_config(Some(&path))?;
    Ok(ignore_mutation_report(
        &path,
        "add",
        kind.as_str(),
        &normalized,
        changed,
        &config,
    ))
}

/// Remove one manual `ProjectAtlas` ignore entry from config.
pub(crate) fn remove_ignore_entry(
    config_path: Option<&Path>,
    project_root: &Path,
    kind: Option<IgnoreEntryKind>,
    value: &str,
) -> AtlasMapResult<IgnoreMutationReport> {
    let path = resolve_config_edit_path(config_path, project_root)?;
    let mut document = load_config_document_for_edit(&path)?;
    let mut changed = false;
    let normalized = if let Some(kind) = kind {
        let normalized = normalize_ignore_value(kind, value)?;
        let mut values = string_array_values(&path, &document, kind)?;
        if values.remove(&normalized) {
            changed = true;
            write_string_array(&mut document, kind.config_key(), &values)?;
        }
        normalized
    } else {
        let normalized_prefix = normalize_ignore_value(IgnoreEntryKind::PathPrefix, value)?;
        let normalized_dir = normalize_ignore_value(IgnoreEntryKind::DirName, value).ok();
        let mut prefix_values = string_array_values(&path, &document, IgnoreEntryKind::PathPrefix)?;
        if prefix_values.remove(&normalized_prefix) {
            changed = true;
            write_string_array(
                &mut document,
                IgnoreEntryKind::PathPrefix.config_key(),
                &prefix_values,
            )?;
        }
        if let Some(normalized_dir) = normalized_dir.as_deref() {
            let mut dir_values = string_array_values(&path, &document, IgnoreEntryKind::DirName)?;
            if dir_values.remove(normalized_dir) {
                changed = true;
                write_string_array(
                    &mut document,
                    IgnoreEntryKind::DirName.config_key(),
                    &dir_values,
                )?;
            }
        }
        normalized_prefix
    };
    if changed {
        write_config_document(&path, &document)?;
    }
    let config = load_atlas_config(Some(&path))?;
    Ok(ignore_mutation_report(
        &path,
        "remove",
        kind.map_or("any", IgnoreEntryKind::as_str),
        &normalized,
        changed,
        &config,
    ))
}

/// Generate and write the atlas map.
pub(crate) fn write_map(config: &AtlasMapConfig, write_json: bool) -> AtlasMapResult<()> {
    let snapshot = build_snapshot(config)?;
    write_toon(&snapshot, config)?;
    if write_json {
        write_json_map(&snapshot, config)?;
    }
    Ok(())
}

/// Extract approved legacy purpose records for `SQLite` import.
///
/// # Errors
///
/// Returns an error when repository scanning or purpose extraction fails.
pub(crate) fn imported_purpose_records(
    config: &AtlasMapConfig,
) -> AtlasMapResult<Vec<ImportedPurposeRecord>> {
    let mut imported = BTreeMap::new();
    append_existing_map_purpose_records(config, &mut imported)?;
    let paths = collect_repo_paths(config)?;
    let db_purposes = BTreeMap::new();
    let (file_records, _, _) = build_file_records(&paths.source_files, config, &db_purposes)?;
    let nonsource = read_nonsource_file_entries(config)?;
    let merged_file_records = merge_records(&file_records, &nonsource.records);
    let (folder_records, _, _) = build_folder_records(&paths.folders, config, &db_purposes)?;
    append_imported_records(&mut imported, &folder_records);
    append_imported_records(&mut imported, &merged_file_records);
    Ok(imported
        .into_iter()
        .map(|(path, summary)| ImportedPurposeRecord { path, summary })
        .collect())
}

/// Append valid imported records from map records.
fn append_imported_records(imported: &mut BTreeMap<String, String>, records: &[MapRecord]) {
    for record in records {
        if record.summary == "MISSING" || record.summary == "INVALID" {
            continue;
        }
        imported.insert(record.path.clone(), record.summary.clone());
    }
}

/// Append approved records from an existing committed atlas map.
fn append_existing_map_purpose_records(
    config: &AtlasMapConfig,
    imported: &mut BTreeMap<String, String>,
) -> AtlasMapResult<()> {
    if !config.map_path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(&config.map_path).map_err(|source| AtlasMapError::Io {
        path: config.map_path.clone(),
        source,
    })?;
    let mut in_record_rows = false;
    for line in content.lines().map(str::trim) {
        if line.starts_with("folders[") || line.starts_with("files[") {
            in_record_rows = true;
            continue;
        }
        if line.ends_with(':') {
            in_record_rows = false;
            continue;
        }
        if !in_record_rows || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cells = split_record_cells(line);
        if cells.len() < 2 {
            continue;
        }
        let summary = cells[1].trim();
        if summary.is_empty() || summary == "MISSING" || summary == "INVALID" {
            continue;
        }
        let path = normalize_repo_string(&cells[0])?;
        imported.insert(path, summary.to_string());
    }
    Ok(())
}

/// Load approved purpose records from the durable `SQLite` index.
fn load_db_purpose_records(config: &AtlasMapConfig) -> AtlasMapResult<BTreeMap<String, String>> {
    if !config.db_path.exists() {
        return Ok(BTreeMap::new());
    }
    let store = AtlasStore::open(&config.db_path).map_err(|source| AtlasMapError::Database {
        path: config.db_path.clone(),
        message: source.to_string(),
    })?;
    let nodes = store
        .load_nodes()
        .map_err(|source| AtlasMapError::Database {
            path: config.db_path.clone(),
            message: source.to_string(),
        })?;
    Ok(nodes
        .into_iter()
        .filter(|node| node.purpose.status == projectatlas_core::PurposeStatus::Approved)
        .filter_map(|node| {
            node.purpose
                .purpose
                .map(|purpose| (node.node.path, purpose))
        })
        .collect())
}

/// Lint the atlas map and return a report plus exit code.
pub(crate) fn lint_map(
    config: &AtlasMapConfig,
    options: LintOptions,
) -> AtlasMapResult<(String, i32)> {
    let paths = collect_repo_paths(config)?;
    let db_purposes = load_db_purpose_records(config)?;
    let (file_records, missing_headers, invalid_headers) =
        build_file_records(&paths.source_files, config, &db_purposes)?;
    let nonsource = read_nonsource_file_entries(config)?;
    let merged_file_records = merge_records(&file_records, &nonsource.records);
    let (folder_records, missing_folders, invalid_folders) =
        build_folder_records(&paths.folders, config, &db_purposes)?;
    let expected_overview = compute_overview(
        &paths,
        config,
        nonsource
            .records
            .iter()
            .filter(|record| record.source == "nonsource")
            .count(),
    );
    let expected_file_hash = compute_file_hash(&merged_file_records);
    let expected_folder_hash = compute_folder_hash(&paths.folders);
    let mut report = Vec::new();
    let mut errors = Vec::new();

    append_nonsource_errors(&mut errors, &nonsource);
    append_header_errors(&mut errors, &missing_headers, &invalid_headers, config);
    append_folder_errors(
        &mut errors,
        options.strict_folders,
        &missing_folders,
        &invalid_folders,
    );
    if options.report_untracked {
        append_untracked_report(&mut report, &mut errors, config, &paths, options)?;
    }
    append_stale_map_errors(
        &mut errors,
        config,
        &expected_overview,
        &expected_file_hash,
        &expected_folder_hash,
    )?;
    let _ = folder_records;
    if !errors.is_empty() {
        report.extend(errors);
        return Ok((join_report(&report), 1));
    }
    Ok((join_report(&report), 0))
}

/// Find a default config path under the current root.
fn find_config_path(root: &Path) -> Option<PathBuf> {
    let project_config = root.join(".projectatlas").join("config.toml");
    if project_config.exists() {
        return Some(project_config);
    }
    let flat_config = root.join("projectatlas.toml");
    if flat_config.exists() {
        return Some(flat_config);
    }
    None
}

/// Resolve the config file path that ignore commands should edit.
fn resolve_config_edit_path(
    config_path: Option<&Path>,
    project_root: &Path,
) -> AtlasMapResult<PathBuf> {
    let cwd = std::env::current_dir().map_err(|source| AtlasMapError::Io {
        path: PathBuf::from("."),
        source,
    })?;
    if let Some(path) = config_path {
        return Ok(if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        });
    }
    Ok(find_config_path(project_root)
        .unwrap_or_else(|| project_root.join(".projectatlas").join("config.toml")))
}

/// Load an editable TOML document, creating default config text when absent.
fn load_config_document_for_edit(path: &Path) -> AtlasMapResult<DocumentMut> {
    let text = if path.exists() {
        fs::read_to_string(path).map_err(|source| AtlasMapError::Io {
            path: path.to_path_buf(),
            source,
        })?
    } else {
        default_config_text()
    };
    text.parse::<DocumentMut>()
        .map_err(|source| AtlasMapError::TomlEdit {
            path: path.to_path_buf(),
            message: source.to_string(),
        })
}

/// Persist an editable TOML document to disk.
fn write_config_document(path: &Path, document: &DocumentMut) -> AtlasMapResult<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| AtlasMapError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(path, document.to_string()).map_err(|source| AtlasMapError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Read a string array from the `[scan]` table, returning an empty set when absent.
fn string_array_values(
    path: &Path,
    document: &DocumentMut,
    kind: IgnoreEntryKind,
) -> AtlasMapResult<BTreeSet<String>> {
    let key = kind.config_key();
    let Some(scan) = document.get("scan") else {
        return Ok(BTreeSet::new());
    };
    let Some(table) = scan.as_table() else {
        return Err(AtlasMapError::TomlEdit {
            path: path.to_path_buf(),
            message: "[scan] must be a TOML table".to_string(),
        });
    };
    let Some(item) = table.get(key) else {
        return Ok(BTreeSet::new());
    };
    let Some(array) = item.as_array() else {
        return Err(AtlasMapError::TomlEdit {
            path: path.to_path_buf(),
            message: format!("[scan].{key} must be an array of strings"),
        });
    };
    let mut values = BTreeSet::new();
    for value in array {
        let Some(text) = value.as_str() else {
            return Err(AtlasMapError::TomlEdit {
                path: path.to_path_buf(),
                message: format!("[scan].{key} must contain only strings"),
            });
        };
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            values.insert(normalize_ignore_value(kind, trimmed)?);
        }
    }
    Ok(values)
}

/// Replace one `[scan]` string array while preserving unrelated config content.
fn write_string_array(
    document: &mut DocumentMut,
    key: &str,
    values: &BTreeSet<String>,
) -> AtlasMapResult<()> {
    if document.get("scan").is_none() {
        document["scan"] = Item::Table(Table::new());
    }
    let Some(scan) = document["scan"].as_table_mut() else {
        return Err(AtlasMapError::TomlEdit {
            path: PathBuf::from("<config>"),
            message: "[scan] must be a TOML table".to_string(),
        });
    };
    let mut array = Array::new();
    for value in values {
        array.push(value.as_str());
    }
    scan[key] = value(array);
    Ok(())
}

/// Normalize raw config into runtime config.
fn normalize_config(
    raw: RawConfig,
    config_path: Option<&Path>,
    base_dir: &Path,
    cwd: &Path,
) -> AtlasMapResult<AtlasMapConfig> {
    let project = raw.project.unwrap_or_default();
    let root = match project.root {
        Some(root) if root.trim() == "." && config_path_is_projectatlas(config_path) => {
            project_root_for_projectatlas_config(config_path, cwd)
        }
        Some(root) => absolutize(base_dir, &root),
        None if config_path_is_projectatlas(config_path) => {
            project_root_for_projectatlas_config(config_path, cwd)
        }
        None => cwd.to_path_buf(),
    };
    let scan = raw.scan.unwrap_or_default();
    let purpose = raw.purpose.unwrap_or_default();
    let summary = raw.summary_rules.unwrap_or_default();
    let untracked = raw.untracked.unwrap_or_default();
    Ok(AtlasMapConfig {
        map_path: absolutize(
            &root,
            project.map_path.as_deref().unwrap_or(DEFAULT_MAP_PATH),
        ),
        nonsource_files_path: absolutize(
            &root,
            project
                .nonsource_files_path
                .as_deref()
                .or(project.manual_files_path.as_deref())
                .unwrap_or(DEFAULT_NONSOURCE_PATH),
        ),
        purpose_filename: project
            .purpose_filename
            .unwrap_or_else(|| DEFAULT_PURPOSE_FILENAME.to_string()),
        source_extensions: normalize_set(scan.source_extensions.unwrap_or_else(|| {
            DEFAULT_SOURCE_EXTENSIONS
                .iter()
                .map(ToString::to_string)
                .collect()
        })),
        exclude_dir_names: exclude_dir_name_set(scan.exclude_dir_names),
        exclude_dir_suffixes: string_set(scan.exclude_dir_suffixes, &[".egg-info"]),
        exclude_path_prefixes: normalize_prefix_set(scan.exclude_path_prefixes)?,
        non_source_path_prefixes: normalize_prefix_set(scan.non_source_path_prefixes)?,
        allowed_untracked_filenames: string_set(
            untracked.allowed_filenames,
            &[DEFAULT_PURPOSE_FILENAME],
        ),
        untracked_allowlist_dir_prefixes: normalize_prefix_set(untracked.allowlist_dir_prefixes)?,
        untracked_allowlist_files: normalize_prefix_set(untracked.allowlist_files)?,
        asset_allowed_prefixes: normalize_prefix_set(untracked.asset_allowed_prefixes)?,
        asset_extensions: normalize_set(untracked.asset_extensions.unwrap_or_else(|| {
            DEFAULT_ASSET_EXTENSIONS
                .iter()
                .map(ToString::to_string)
                .collect()
        })),
        db_path: root.join(".projectatlas").join("projectatlas.db"),
        max_scan_lines: scan.max_scan_lines.unwrap_or(DEFAULT_MAX_SCAN_LINES),
        text_index_max_bytes: scan
            .text_index_max_bytes
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_TEXT_INDEX_MAX_BYTES),
        summary_max_length: summary.max_length.unwrap_or(DEFAULT_SUMMARY_MAX_LENGTH),
        summary_ascii_only: summary.ascii_only.unwrap_or(true),
        summary_no_commas: summary.no_commas.unwrap_or(true),
        purpose_styles: normalize_style_map(purpose.styles_by_extension),
        purpose_default_style: purpose
            .default_style
            .unwrap_or_else(|| "javadoc".to_string()),
        line_comment_prefixes: purpose.line_comment_prefixes.unwrap_or_else(|| {
            DEFAULT_LINE_COMMENT_PREFIXES
                .iter()
                .map(ToString::to_string)
                .collect()
        }),
        root,
    })
}

/// Return whether the config path is inside `.projectatlas`.
fn config_path_is_projectatlas(config_path: Option<&Path>) -> bool {
    config_path
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .is_some_and(|name| name == ".projectatlas")
}

/// Return the project root implied by `.projectatlas/config.toml`.
fn project_root_for_projectatlas_config(config_path: Option<&Path>, cwd: &Path) -> PathBuf {
    let Some(root) = config_path
        .and_then(Path::parent)
        .and_then(Path::parent)
        .filter(|path| !path.as_os_str().is_empty() && *path != Path::new("."))
    else {
        return cwd.to_path_buf();
    };
    root.to_path_buf()
}

/// Convert a possibly relative path to an absolute path.
fn absolutize(base: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

/// Normalize extension strings into a lower-case set.
fn normalize_set(values: Vec<String>) -> BTreeSet<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
}

/// Normalize one manual ignore entry.
fn normalize_ignore_value(kind: IgnoreEntryKind, value: &str) -> AtlasMapResult<String> {
    match kind {
        IgnoreEntryKind::DirName => normalize_ignore_dir_name(value),
        IgnoreEntryKind::PathPrefix => {
            let normalized = normalize_repo_string(value)?;
            if normalized == "." {
                return Err(AtlasMapError::InvalidRepositoryPath {
                    path: value.to_string(),
                    message: "project root cannot be ignored by ProjectAtlas".to_string(),
                });
            }
            Ok(normalized)
        }
    }
}

/// Normalize one directory-name ignore entry.
fn normalize_ignore_dir_name(value: &str) -> AtlasMapResult<String> {
    let trimmed = value.trim().trim_matches('/').trim_matches('\\');
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return Err(AtlasMapError::InvalidRepositoryPath {
            path: value.to_string(),
            message: "directory name ignore must name one directory".to_string(),
        });
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err(AtlasMapError::InvalidRepositoryPath {
            path: value.to_string(),
            message:
                "directory-name ignores cannot contain path separators; use path-prefix instead"
                    .to_string(),
        });
    }
    Ok(trimmed.to_string())
}

/// Convert optional strings into a set with defaults.
fn string_set(values: Option<Vec<String>>, defaults: &[&str]) -> BTreeSet<String> {
    values
        .unwrap_or_else(|| defaults.iter().map(ToString::to_string).collect())
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect()
}

/// Normalize excluded directory names and preserve required internal excludes.
fn exclude_dir_name_set(values: Option<Vec<String>>) -> BTreeSet<String> {
    let mut names = string_set(values, DEFAULT_EXCLUDE_DIR_NAMES);
    names.extend(REQUIRED_EXCLUDE_DIR_NAMES.iter().map(ToString::to_string));
    names
}

/// Build a report for current ignore settings.
fn ignore_list_report(path: &Path, config: &AtlasMapConfig) -> IgnoreListReport {
    let gitignore_path = config.root.join(".gitignore");
    IgnoreListReport {
        config_path: path.display().to_string(),
        gitignore_path: gitignore_path.display().to_string(),
        gitignore_present: gitignore_path.exists(),
        gitignore_mode: "inherited-when-present".to_string(),
        manual_layer_order: "after-gitignore".to_string(),
        exclude_dir_names: config.exclude_dir_names.iter().cloned().collect(),
        exclude_path_prefixes: config.exclude_path_prefixes.iter().cloned().collect(),
    }
}

/// Build a report for an ignore mutation.
fn ignore_mutation_report(
    path: &Path,
    action: &str,
    kind: &str,
    value: &str,
    changed: bool,
    config: &AtlasMapConfig,
) -> IgnoreMutationReport {
    let gitignore_path = config.root.join(".gitignore");
    IgnoreMutationReport {
        config_path: path.display().to_string(),
        gitignore_path: gitignore_path.display().to_string(),
        gitignore_present: gitignore_path.exists(),
        action: action.to_string(),
        kind: kind.to_string(),
        value: value.to_string(),
        changed,
        gitignore_mode: "inherited-when-present".to_string(),
        manual_layer_order: "after-gitignore".to_string(),
        exclude_dir_names: config.exclude_dir_names.iter().cloned().collect(),
        exclude_path_prefixes: config.exclude_path_prefixes.iter().cloned().collect(),
    }
}

/// Normalize path-prefix strings into slash-separated values.
fn normalize_prefix_set(values: Option<Vec<String>>) -> AtlasMapResult<BTreeSet<String>> {
    let mut prefixes = BTreeSet::new();
    for value in values.unwrap_or_default() {
        let normalized = normalize_repo_string(&value)?;
        if !normalized.is_empty() {
            prefixes.insert(normalized);
        }
    }
    Ok(prefixes)
}

/// Normalize per-extension purpose styles.
fn normalize_style_map(values: Option<BTreeMap<String, String>>) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    map.insert(".py".to_string(), "python-docstring".to_string());
    map.insert(".vue".to_string(), "vue-block".to_string());
    map.insert(".rs".to_string(), "line-comment".to_string());
    map.insert(".go".to_string(), "line-comment".to_string());
    map.insert(".sh".to_string(), "line-comment".to_string());
    map.insert(".bash".to_string(), "line-comment".to_string());
    map.insert(".zsh".to_string(), "line-comment".to_string());
    map.insert(".ps1".to_string(), "line-comment".to_string());
    map.insert(".psm1".to_string(), "line-comment".to_string());
    map.insert(".psd1".to_string(), "line-comment".to_string());
    map.insert(".sql".to_string(), "line-comment".to_string());
    if let Some(values) = values {
        for (extension, style) in values {
            map.insert(extension.to_ascii_lowercase(), style);
        }
    }
    map
}

/// Collect repository folders, source files, and non-source files.
fn collect_repo_paths(config: &AtlasMapConfig) -> AtlasMapResult<RepoPaths> {
    let options = config.scan_options();
    let nodes = scan_repo(&config.root, &options)?;
    let mut folders = Vec::new();
    let mut source_files = Vec::new();
    let mut untracked_files = Vec::new();
    let mut excluded_paths = BTreeSet::new();
    for node in nodes {
        if has_excluded_suffix_component(&node.path, &config.exclude_dir_suffixes) {
            excluded_paths.insert(node.path);
            continue;
        }
        match node.kind {
            NodeKind::Folder => {
                if is_legacy_map_metadata_folder(&node.path) {
                    continue;
                }
                folders.push(node.path);
            }
            NodeKind::File => {
                if is_durable_projectatlas_input(&node.path) {
                    continue;
                }
                if is_source_node(
                    &node.path,
                    node.extension.as_deref(),
                    node.language.as_deref(),
                    config,
                ) {
                    source_files.push(node.path);
                } else {
                    untracked_files.push(node.path);
                }
            }
        }
    }
    folders.sort();
    source_files.sort();
    untracked_files.sort();
    Ok(RepoPaths {
        folders,
        source_files,
        untracked_files,
        excluded_paths: excluded_paths.into_iter().collect(),
    })
}

/// Return whether any path component has an excluded suffix.
fn has_excluded_suffix_component(path: &str, suffixes: &BTreeSet<String>) -> bool {
    path.split('/').any(|part| {
        suffixes
            .iter()
            .any(|suffix| !suffix.is_empty() && part.ends_with(suffix))
    })
}

/// Return whether a folder is `ProjectAtlas` metadata ignored by legacy map/lint.
fn is_legacy_map_metadata_folder(path: &str) -> bool {
    path == ".projectatlas"
}

/// Return whether a file is a durable `ProjectAtlas` input outside legacy map/lint.
fn is_durable_projectatlas_input(path: &str) -> bool {
    DURABLE_PROJECTATLAS_INPUT_PATHS.contains(&path)
}

/// Return whether a scanned file should be treated as source.
fn is_source_node(
    path: &str,
    extension: Option<&str>,
    language: Option<&str>,
    config: &AtlasMapConfig,
) -> bool {
    if is_under_any_prefix(path, &config.non_source_path_prefixes) {
        return false;
    }
    extension.is_some_and(|extension| config.source_extensions.contains(extension))
        || is_path_special_source_family(language)
}

/// Return whether the scanner detected a source-like file family without relying on extension policy.
fn is_path_special_source_family(language: Option<&str>) -> bool {
    matches!(
        language,
        Some("cargo-manifest" | "cargo-lock" | "rust-build-script" | "dockerfile" | "makefile")
    )
}

/// Build the full atlas snapshot.
fn build_snapshot(config: &AtlasMapConfig) -> AtlasMapResult<AtlasSnapshot> {
    let paths = collect_repo_paths(config)?;
    let db_purposes = load_db_purpose_records(config)?;
    let (file_records, _, _) = build_file_records(&paths.source_files, config, &db_purposes)?;
    let nonsource = read_nonsource_file_entries(config)?;
    let merged_file_records = merge_records(&file_records, &nonsource.records);
    let (folder_records, _, _) = build_folder_records(&paths.folders, config, &db_purposes)?;
    let folder_summary_map = folder_records
        .iter()
        .map(|record| (record.path.clone(), record.summary.clone()))
        .collect::<BTreeMap<_, _>>();
    let folder_tree = build_folder_tree(&paths.folders, &folder_summary_map);
    let folder_duplicates = build_summary_duplicates(&folder_records);
    let file_duplicates = build_summary_duplicates(&merged_file_records);
    let file_hash = compute_file_hash(&merged_file_records);
    let folder_hash = compute_folder_hash(&paths.folders);
    let overview = compute_overview(
        &paths,
        config,
        nonsource
            .records
            .iter()
            .filter(|record| record.source == "nonsource")
            .count(),
    );
    Ok(AtlasSnapshot {
        folder_records,
        file_records: merged_file_records,
        folder_tree,
        folder_duplicates,
        file_duplicates,
        generated_at: stable_generated_at(config, &file_hash, &folder_hash),
        file_hash,
        folder_hash,
        overview,
    })
}

/// Preserve an existing timestamp when map contents are unchanged.
fn stable_generated_at(config: &AtlasMapConfig, file_hash: &str, folder_hash: &str) -> String {
    if let Ok(content) = fs::read_to_string(&config.map_path) {
        let (existing_file_hash, existing_folder_hash) = read_hashes(&content);
        if existing_file_hash.as_deref() == Some(file_hash)
            && existing_folder_hash.as_deref() == Some(folder_hash)
            && let Some(existing_generated_at) = read_generated_at(&content)
        {
            return existing_generated_at;
        }
    }
    generated_at()
}

/// Return a simple UTC-ish generated timestamp.
fn generated_at() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    format!("unix:{seconds}")
}

/// Build file records and validation lists.
fn build_file_records(
    files: &[String],
    config: &AtlasMapConfig,
    db_purposes: &BTreeMap<String, String>,
) -> AtlasMapResult<(Vec<MapRecord>, Vec<String>, BTreeMap<String, Vec<String>>)> {
    let mut records = Vec::new();
    let mut missing = Vec::new();
    let mut invalid = BTreeMap::new();
    for rel_path in files {
        if let Some(summary) = db_purposes.get(rel_path) {
            records.push(MapRecord {
                path: rel_path.clone(),
                summary: summary.clone(),
                source: "database".to_string(),
            });
            continue;
        }
        let path = repo_join(&config.root, rel_path);
        let (summary, header_issues) = extract_purpose_header(&path, rel_path, config)?;
        if let Some(summary) = summary {
            let issues = validate_summary(&summary, config);
            if issues.is_empty() {
                records.push(MapRecord {
                    path: rel_path.clone(),
                    summary,
                    source: "header".to_string(),
                });
            } else {
                invalid.insert(rel_path.clone(), issues);
                records.push(missing_record(rel_path));
            }
        } else if header_issues
            .iter()
            .any(|issue| issue.starts_with("missing "))
        {
            missing.push(rel_path.clone());
            records.push(missing_record(rel_path));
        } else {
            invalid.insert(rel_path.clone(), header_issues);
            records.push(invalid_record(rel_path));
        }
    }
    Ok((records, missing, invalid))
}

/// Build folder records and validation lists.
fn build_folder_records(
    folders: &[String],
    config: &AtlasMapConfig,
    db_purposes: &BTreeMap<String, String>,
) -> AtlasMapResult<(Vec<MapRecord>, Vec<String>, BTreeMap<String, Vec<String>>)> {
    let mut records = Vec::new();
    let mut missing = Vec::new();
    let mut invalid = BTreeMap::new();
    for folder in folders {
        if let Some(summary) = db_purposes.get(folder) {
            records.push(MapRecord {
                path: folder.clone(),
                summary: summary.clone(),
                source: "database".to_string(),
            });
            continue;
        }
        let (summary, issues) = read_folder_purpose(folder, config)?;
        if let Some(summary) = summary {
            if issues.is_empty() {
                records.push(MapRecord {
                    path: folder.clone(),
                    summary,
                    source: "purpose".to_string(),
                });
            } else {
                invalid.insert(folder.clone(), issues);
                records.push(invalid_record(folder));
            }
        } else if issues.iter().any(|issue| issue == "missing .purpose file") {
            missing.push(folder.clone());
            records.push(missing_record(folder));
        } else {
            invalid.insert(folder.clone(), issues);
            records.push(invalid_record(folder));
        }
    }
    Ok((records, missing, invalid))
}

/// Create a missing placeholder record.
fn missing_record(path: &str) -> MapRecord {
    MapRecord {
        path: path.to_string(),
        summary: "MISSING".to_string(),
        source: "missing".to_string(),
    }
}

/// Create an invalid placeholder record.
fn invalid_record(path: &str) -> MapRecord {
    MapRecord {
        path: path.to_string(),
        summary: "INVALID".to_string(),
        source: "invalid".to_string(),
    }
}

/// Extract a purpose header from a file.
fn extract_purpose_header(
    path: &Path,
    rel_path: &str,
    config: &AtlasMapConfig,
) -> AtlasMapResult<(Option<String>, Vec<String>)> {
    let content = fs::read_to_string(path).map_err(|source| AtlasMapError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let lines = content.lines().map(ToString::to_string).collect::<Vec<_>>();
    let style = resolve_purpose_style(rel_path, config);
    let result = match style.as_str() {
        "python-docstring" => extract_python_docstring_purpose(&lines, config.max_scan_lines),
        "vue-block" => extract_vue_purpose(&lines, config.max_scan_lines),
        "javadoc" => extract_javadoc_purpose(&lines, config.max_scan_lines),
        "block-comment" => extract_block_comment_purpose(&lines, config.max_scan_lines),
        "line-comment" => extract_line_comment_purpose(
            &lines,
            config.max_scan_lines,
            &config.line_comment_prefixes,
        ),
        _ => (None, vec![format!("unsupported Purpose style: {style}")]),
    };
    Ok(result)
}

/// Resolve configured purpose style for a relative path.
fn resolve_purpose_style(path: &str, config: &AtlasMapConfig) -> String {
    let extension = normalized_extension(path);
    config
        .purpose_styles
        .get(&extension)
        .cloned()
        .unwrap_or_else(|| config.purpose_default_style.clone())
}

/// Extract a purpose from a Javadoc-style block.
fn extract_javadoc_purpose(
    lines: &[String],
    max_scan_lines: usize,
) -> (Option<String>, Vec<String>) {
    let Some(start) = first_content_line(lines) else {
        return (
            None,
            vec!["missing Javadoc-style Purpose header".to_string()],
        );
    };
    if !lines[start].trim_start().starts_with("/**") {
        return (
            None,
            vec!["missing Javadoc-style Purpose header".to_string()],
        );
    }
    let block = collect_until(lines, start, max_scan_lines, "*/");
    match block {
        Some(block) => purpose_from_lines(&block, None).map_or_else(
            || {
                (
                    None,
                    vec!["missing Purpose line in Javadoc-style header".to_string()],
                )
            },
            |summary| (Some(summary), Vec::new()),
        ),
        None => (None, vec!["unterminated Javadoc-style header".to_string()]),
    }
}

/// Extract a purpose from a generic block comment.
fn extract_block_comment_purpose(
    lines: &[String],
    max_scan_lines: usize,
) -> (Option<String>, Vec<String>) {
    let Some(start) = first_content_line(lines) else {
        return (
            None,
            vec!["missing block comment Purpose header".to_string()],
        );
    };
    if !lines[start].trim_start().starts_with("/*") {
        return (
            None,
            vec!["missing block comment Purpose header".to_string()],
        );
    }
    let block = collect_until(lines, start, max_scan_lines, "*/");
    match block {
        Some(block) => purpose_from_lines(&block, None).map_or_else(
            || {
                (
                    None,
                    vec!["missing Purpose line in block comment header".to_string()],
                )
            },
            |summary| (Some(summary), Vec::new()),
        ),
        None => (None, vec!["unterminated block comment header".to_string()]),
    }
}

/// Extract a purpose from a Python module docstring.
fn extract_python_docstring_purpose(
    lines: &[String],
    max_scan_lines: usize,
) -> (Option<String>, Vec<String>) {
    let Some(start) = first_python_doc_line(lines) else {
        return (
            None,
            vec!["missing module docstring Purpose header".to_string()],
        );
    };
    let trimmed = lines[start].trim_start();
    let delimiter = if trimmed.starts_with("\"\"\"") {
        "\"\"\""
    } else if trimmed.starts_with("'''") {
        "'''"
    } else {
        return (
            None,
            vec!["missing module docstring Purpose header".to_string()],
        );
    };
    let block = collect_python_docstring(lines, start, max_scan_lines, delimiter);
    match block {
        Some(block) => purpose_from_lines(&block, None).map_or_else(
            || {
                (
                    None,
                    vec!["missing Purpose line in module docstring".to_string()],
                )
            },
            |summary| (Some(summary), Vec::new()),
        ),
        None => (None, vec!["unterminated module docstring".to_string()]),
    }
}

/// Extract a purpose from a Vue script or style block.
fn extract_vue_purpose(lines: &[String], max_scan_lines: usize) -> (Option<String>, Vec<String>) {
    for tag in ["script", "style"] {
        let Some(start) = lines
            .iter()
            .position(|line| line.trim_start().starts_with(&format!("<{tag}")))
        else {
            continue;
        };
        let Some(end) = lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .find_map(|(index, line)| {
                line.trim_start()
                    .starts_with(&format!("</{tag}>"))
                    .then_some(index)
            })
        else {
            return (None, vec![format!("unterminated <{tag}> block")]);
        };
        return extract_javadoc_purpose(&lines[start + 1..end], max_scan_lines);
    }
    (
        None,
        vec!["missing Javadoc-style Purpose header in <script> or <style> block".to_string()],
    )
}

/// Extract a purpose from a line-comment header.
fn extract_line_comment_purpose(
    lines: &[String],
    max_scan_lines: usize,
    prefixes: &[String],
) -> (Option<String>, Vec<String>) {
    let mut comment_lines = Vec::new();
    for line in lines.iter().take(max_scan_lines) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if comment_lines.is_empty() {
                continue;
            }
            break;
        }
        if trimmed.starts_with("#!") && comment_lines.is_empty() {
            continue;
        }
        if prefixes.iter().any(|prefix| trimmed.starts_with(prefix)) {
            comment_lines.push(trimmed.to_string());
            continue;
        }
        break;
    }
    if comment_lines.is_empty() {
        return (
            None,
            vec!["missing line-comment Purpose header".to_string()],
        );
    }
    purpose_from_lines(&comment_lines, Some(prefixes)).map_or_else(
        || {
            (
                None,
                vec!["missing Purpose line in line-comment header".to_string()],
            )
        },
        |summary| (Some(summary), Vec::new()),
    )
}

/// Return the first content line after shebangs and blanks.
fn first_content_line(lines: &[String]) -> Option<usize> {
    lines.iter().enumerate().find_map(|(index, line)| {
        let trimmed = line.trim();
        (!trimmed.is_empty() && !trimmed.starts_with("#!")).then_some(index)
    })
}

/// Return the first Python docstring candidate line.
fn first_python_doc_line(lines: &[String]) -> Option<usize> {
    lines.iter().enumerate().find_map(|(index, line)| {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("#!")
            || trimmed.starts_with('#')
            || trimmed.contains("coding:")
            || trimmed.contains("coding=")
        {
            None
        } else {
            Some(index)
        }
    })
}

/// Collect lines until a marker appears.
fn collect_until(
    lines: &[String],
    start: usize,
    max_scan_lines: usize,
    marker: &str,
) -> Option<Vec<String>> {
    let mut block = Vec::new();
    for line in lines.iter().skip(start).take(max_scan_lines) {
        block.push(line.clone());
        if line.contains(marker) {
            return Some(block);
        }
    }
    None
}

/// Collect a Python docstring body.
fn collect_python_docstring(
    lines: &[String],
    start: usize,
    max_scan_lines: usize,
    delimiter: &str,
) -> Option<Vec<String>> {
    let first = lines[start].trim_start();
    let after_open = first.strip_prefix(delimiter)?;
    if let Some((before_close, _)) = after_open.split_once(delimiter) {
        return Some(vec![before_close.to_string()]);
    }
    let mut block = vec![after_open.to_string()];
    for line in lines.iter().skip(start + 1).take(max_scan_lines) {
        if let Some((before_close, _)) = line.split_once(delimiter) {
            block.push(before_close.to_string());
            return Some(block);
        }
        block.push(line.clone());
    }
    None
}

/// Extract a normalized purpose from comment lines.
fn purpose_from_lines(lines: &[String], prefixes: Option<&[String]>) -> Option<String> {
    lines.iter().find_map(|line| {
        let mut cleaned = line.trim().to_string();
        if let Some(prefixes) = prefixes {
            cleaned = strip_line_comment_prefix(&cleaned, prefixes);
        }
        cleaned = cleaned
            .trim_start_matches("/**")
            .trim_start_matches("/*")
            .trim_start_matches('*')
            .trim_end_matches("*/")
            .trim()
            .to_string();
        cleaned
            .split_once("Purpose:")
            .map(|(_, summary)| normalize_summary(summary))
    })
}

/// Strip a line-comment prefix.
fn strip_line_comment_prefix(line: &str, prefixes: &[String]) -> String {
    for prefix in prefixes {
        if let Some(remainder) = line.strip_prefix(prefix) {
            return remainder.trim_start_matches('!').trim_start().to_string();
        }
    }
    line.to_string()
}

/// Normalize a summary to a single-line value.
fn normalize_summary(summary: &str) -> String {
    summary.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Validate a purpose summary.
fn validate_summary(summary: &str, config: &AtlasMapConfig) -> Vec<String> {
    let mut problems = Vec::new();
    if summary.is_empty() {
        problems.push("summary is empty".to_string());
    }
    if config.summary_no_commas && summary.contains(',') {
        problems.push("summary contains a comma".to_string());
    }
    if config.summary_ascii_only && !summary.is_ascii() {
        problems.push("summary contains non-ASCII characters".to_string());
    }
    if summary.len() > config.summary_max_length {
        problems.push("summary exceeds length limit".to_string());
    }
    problems
}

/// Read folder purpose metadata.
fn read_folder_purpose(
    folder: &str,
    config: &AtlasMapConfig,
) -> AtlasMapResult<(Option<String>, Vec<String>)> {
    let purpose_path = repo_join(&config.root, folder).join(&config.purpose_filename);
    if !purpose_path.exists() {
        return Ok((None, vec!["missing .purpose file".to_string()]));
    }
    let content = fs::read_to_string(&purpose_path).map_err(|source| AtlasMapError::Io {
        path: purpose_path,
        source,
    })?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        let summary = trimmed.split_once("Purpose:").map_or_else(
            || normalize_summary(trimmed),
            |(_, value)| normalize_summary(value),
        );
        let issues = validate_summary(&summary, config);
        return Ok((Some(summary), issues));
    }
    Ok((None, vec!["missing Purpose summary".to_string()]))
}

/// Read non-source file entries.
fn read_nonsource_file_entries(config: &AtlasMapConfig) -> AtlasMapResult<NonsourceEntries> {
    if !config.nonsource_files_path.exists() {
        return Ok(NonsourceEntries {
            records: Vec::new(),
            missing: Vec::new(),
            invalid: BTreeMap::new(),
            errors: vec![format!(
                "non-source file list missing: {}",
                config.nonsource_files_path.display()
            )],
        });
    }
    let content =
        fs::read_to_string(&config.nonsource_files_path).map_err(|source| AtlasMapError::Io {
            path: config.nonsource_files_path.clone(),
            source,
        })?;
    let mut in_nonsource = false;
    let mut records = Vec::new();
    let mut missing = Vec::new();
    let mut invalid = BTreeMap::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        if line.starts_with("nonsource_files[") || line.starts_with("manual_files[") {
            in_nonsource = true;
            continue;
        }
        if line.starts_with("folders[") || line.starts_with("files[") {
            in_nonsource = false;
            continue;
        }
        if !in_nonsource || line.starts_with('-') || line.ends_with(':') {
            continue;
        }
        let cells = split_record_cells(line);
        if cells.len() < 2 {
            continue;
        }
        let rel_path = match normalize_repo_string(&cells[0]) {
            Ok(path) => path,
            Err(error) => {
                invalid.insert(cells[0].clone(), vec![error.to_string()]);
                continue;
            }
        };
        let summary = normalize_summary(&cells[1]);
        if !repo_join(&config.root, &rel_path).exists() {
            missing.push(rel_path.clone());
            records.push(missing_record(&rel_path));
            continue;
        }
        let issues = validate_summary(&summary, config);
        if issues.is_empty() {
            records.push(MapRecord {
                path: rel_path,
                summary,
                source: "nonsource".to_string(),
            });
        } else {
            invalid.insert(rel_path.clone(), issues);
            records.push(invalid_record(&rel_path));
        }
    }
    Ok(NonsourceEntries {
        records,
        missing,
        invalid,
        errors: Vec::new(),
    })
}

/// Merge source and non-source records.
fn merge_records(source: &[MapRecord], nonsource: &[MapRecord]) -> Vec<MapRecord> {
    let mut merged = source
        .iter()
        .map(|record| (record.path.clone(), record.clone()))
        .collect::<BTreeMap<_, _>>();
    for record in nonsource {
        merged
            .entry(record.path.clone())
            .or_insert_with(|| record.clone());
    }
    merged.into_values().collect()
}

/// Build duplicate summary entries.
fn build_summary_duplicates(records: &[MapRecord]) -> Vec<String> {
    let mut grouped: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for record in records {
        if record.summary == "MISSING" || record.summary == "INVALID" {
            continue;
        }
        grouped
            .entry(&record.summary)
            .or_default()
            .push(&record.path);
    }
    grouped
        .into_iter()
        .filter_map(|(summary, mut paths)| {
            if paths.len() < 2 {
                None
            } else {
                paths.sort_unstable();
                Some(format!("{summary} :: {}", paths.join(" | ")))
            }
        })
        .collect()
}

/// Build folder tree lines.
fn build_folder_tree(folders: &[String], summaries: &BTreeMap<String, String>) -> Vec<String> {
    folders
        .iter()
        .map(|folder| {
            let summary = summaries
                .get(folder)
                .map_or("MISSING", std::string::String::as_str);
            if folder == "." {
                format!(". - {summary}")
            } else {
                let depth = folder.matches('/').count();
                let name = folder.rsplit('/').next().unwrap_or(folder);
                format!("{}{name}/ - {summary}", "  ".repeat(depth))
            }
        })
        .collect()
}

/// Compute overview counters.
fn compute_overview(
    paths: &RepoPaths,
    config: &AtlasMapConfig,
    nonsource_count: usize,
) -> BTreeMap<String, usize> {
    let mut overview = BTreeMap::new();
    overview.insert("tracked_source_files".to_string(), paths.source_files.len());
    overview.insert("tracked_nonsource_files".to_string(), nonsource_count);
    overview.insert(
        "tracked_files_total".to_string(),
        paths.source_files.len() + nonsource_count,
    );
    overview.insert("tracked_folders".to_string(), paths.folders.len());
    overview.insert(
        "source_extensions".to_string(),
        config.source_extensions.len(),
    );
    overview.insert(
        "exclude_dir_names".to_string(),
        config.exclude_dir_names.len(),
    );
    overview.insert(
        "exclude_path_prefixes".to_string(),
        config.exclude_path_prefixes.len(),
    );
    overview
}

/// Compute file record hash.
fn compute_file_hash(records: &[MapRecord]) -> String {
    let payload = records
        .iter()
        .map(|record| format!("{}|{}", record.path, record.summary))
        .collect::<Vec<_>>()
        .join("\n");
    hash_text(&payload)
}

/// Compute folder path hash.
fn compute_folder_hash(folders: &[String]) -> String {
    hash_text(&folders.join("\n"))
}

/// Hash a text payload with BLAKE3.
fn hash_text(payload: &str) -> String {
    let mut hasher = Hasher::new();
    hasher.update(payload.as_bytes());
    hasher.finalize().to_hex().to_string()
}

/// Render a TOON snapshot.
fn render_toon(snapshot: &AtlasSnapshot, config: &AtlasMapConfig) -> String {
    let mut lines = Vec::new();
    lines.push("version: 1".to_string());
    lines.push(format!("generated_at: {}", snapshot.generated_at));
    lines.push(format!("file_hash: \"{}\"", snapshot.file_hash));
    lines.push(format!("folder_hash: \"{}\"", snapshot.folder_hash));
    lines.push("root: .".to_string());
    lines.push(format_overview(&snapshot.overview));
    lines.push("source_extensions[]:".to_string());
    lines.extend(
        config
            .source_extensions
            .iter()
            .map(|extension| format!("  - {extension}")),
    );
    lines.push("exclude_dir_names[]:".to_string());
    lines.extend(
        config
            .exclude_dir_names
            .iter()
            .map(|name| format!("  - {name}")),
    );
    lines.push("exclude_path_prefixes[]:".to_string());
    lines.extend(
        config
            .exclude_path_prefixes
            .iter()
            .map(|prefix| format!("  - {prefix}")),
    );
    append_record_rows(&mut lines, "folders", &snapshot.folder_records);
    append_record_rows(&mut lines, "files", &snapshot.file_records);
    append_list(
        &mut lines,
        "folder_summary_duplicates",
        &snapshot.folder_duplicates,
    );
    append_list(
        &mut lines,
        "file_summary_duplicates",
        &snapshot.file_duplicates,
    );
    append_list(&mut lines, "folder_tree", &snapshot.folder_tree);
    lines.join("\n") + "\n"
}

/// Append TOON record rows.
fn append_record_rows(lines: &mut Vec<String>, label: &str, records: &[MapRecord]) {
    lines.push(format!(
        "{label}[{}]{{path,summary,source}}:",
        records.len()
    ));
    lines.extend(records.iter().map(|record| {
        format!(
            "  {},{},{}",
            toon_cell(&record.path),
            toon_cell(&record.summary),
            toon_cell(&record.source)
        )
    }));
}

/// Append TOON list rows.
fn append_list(lines: &mut Vec<String>, label: &str, entries: &[String]) {
    lines.push(format!("{label}[]:"));
    lines.extend(
        entries
            .iter()
            .map(|entry| format!("  - {}", toon_cell(entry))),
    );
}

/// Render a TOON scalar cell with JSON-compatible escaping when needed.
fn toon_cell(value: &str) -> String {
    if needs_quoted_cell(value) {
        quote_toon_string(value)
    } else {
        value.to_string()
    }
}

/// Return whether a tabular TOON cell needs quotes.
fn needs_quoted_cell(value: &str) -> bool {
    value.is_empty()
        || value.chars().any(|character| {
            matches!(
                character,
                ',' | '"' | '\\' | '\n' | '\r' | '\t' | '[' | ']' | '{' | '}'
            ) || character.is_control()
        })
        || value.trim() != value
}

/// Quote a string with JSON-compatible escapes for TOON scalar cells.
fn quote_toon_string(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            character if character.is_control() => {
                push_unicode_escape_digits(&mut quoted, character as u32);
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

/// Split a compact TOON record row into cells.
fn split_record_cells(line: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;
    while let Some(character) = chars.next() {
        match character {
            '"' if in_quotes => in_quotes = false,
            '"' if current.trim().is_empty() => in_quotes = true,
            '\\' if in_quotes => push_escaped_char(&mut current, &mut chars),
            ',' if !in_quotes => {
                cells.push(current.trim().to_string());
                current.clear();
            }
            character => current.push(character),
        }
    }
    cells.push(current.trim().to_string());
    cells
}

/// Push one escaped character from a quoted TOON cell.
fn push_escaped_char(current: &mut String, chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    match chars.next() {
        Some('"') => current.push('"'),
        Some('\\') | None => current.push('\\'),
        Some('n') => current.push('\n'),
        Some('r') => current.push('\r'),
        Some('t') => current.push('\t'),
        Some('u') => push_unicode_escape(current, chars),
        Some(other) => current.push(other),
    }
}

/// Push a four-digit Unicode escape into a quoted string.
fn push_unicode_escape_digits(output: &mut String, value: u32) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    output.push_str("\\u");
    for shift in [12, 8, 4, 0] {
        let index = ((value >> shift) & 0x0f) as usize;
        output.push(char::from(HEX[index]));
    }
}

/// Push a four-digit Unicode escape when present.
fn push_unicode_escape(current: &mut String, chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    let mut digits = String::with_capacity(4);
    for _ in 0..4 {
        if let Some(digit) = chars.next() {
            digits.push(digit);
        }
    }
    if let Ok(value) = u32::from_str_radix(&digits, 16)
        && let Some(character) = char::from_u32(value)
    {
        current.push(character);
        return;
    }
    current.push_str("\\u");
    current.push_str(&digits);
}

/// Format overview counters.
fn format_overview(overview: &BTreeMap<String, usize>) -> String {
    let parts = OVERVIEW_KEYS
        .iter()
        .filter_map(|key| overview.get(*key).map(|value| format!("{key}={value}")))
        .collect::<Vec<_>>();
    format!("overview: {}", parts.join(" "))
}

/// Write TOON map to disk.
fn write_toon(snapshot: &AtlasSnapshot, config: &AtlasMapConfig) -> AtlasMapResult<()> {
    if let Some(parent) = config.map_path.parent() {
        fs::create_dir_all(parent).map_err(|source| AtlasMapError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(&config.map_path, render_toon(snapshot, config)).map_err(|source| AtlasMapError::Io {
        path: config.map_path.clone(),
        source,
    })
}

/// Write JSON map next to TOON map.
fn write_json_map(snapshot: &AtlasSnapshot, config: &AtlasMapConfig) -> AtlasMapResult<()> {
    let json_path = config.map_path.with_extension("json");
    let payload = serde_json::json!({
        "version": 1,
        "generated_at": snapshot.generated_at,
        "file_hash": snapshot.file_hash,
        "folder_hash": snapshot.folder_hash,
        "root": ".",
        "overview": snapshot.overview,
        "folders": snapshot.folder_records.iter().map(record_json).collect::<Vec<_>>(),
        "files": snapshot.file_records.iter().map(record_json).collect::<Vec<_>>(),
        "folder_summary_duplicates": snapshot.folder_duplicates,
        "file_summary_duplicates": snapshot.file_duplicates,
        "folder_tree": snapshot.folder_tree,
    });
    fs::write(&json_path, serde_json::to_string_pretty(&payload)? + "\n").map_err(|source| {
        AtlasMapError::Io {
            path: json_path,
            source,
        }
    })
}

/// Convert a map record to JSON.
fn record_json(record: &MapRecord) -> serde_json::Value {
    serde_json::json!({
        "path": record.path,
        "summary": record.summary,
        "source": record.source,
    })
}

/// Append non-source validation errors.
fn append_nonsource_errors(errors: &mut Vec<String>, nonsource: &NonsourceEntries) {
    if !nonsource.errors.is_empty() {
        errors.push("Non-source file list errors:".to_string());
        errors.push(format_list(&nonsource.errors));
    }
    if !nonsource.missing.is_empty() {
        errors.push("Missing non-source file entries:".to_string());
        errors.push(format_list(&nonsource.missing));
    }
    if !nonsource.invalid.is_empty() {
        errors.push("Invalid non-source file summaries:".to_string());
        append_invalid_map(errors, &nonsource.invalid);
    }
}

/// Append source header validation errors.
fn append_header_errors(
    errors: &mut Vec<String>,
    missing_headers: &[String],
    invalid_headers: &BTreeMap<String, Vec<String>>,
    config: &AtlasMapConfig,
) {
    if !missing_headers.is_empty() {
        errors.push("Missing Purpose headers:".to_string());
        errors.push(format_list(missing_headers));
        append_purpose_style_suggestions(errors, missing_headers, config);
    }
    if !invalid_headers.is_empty() {
        errors.push("Invalid Purpose headers:".to_string());
        append_invalid_map(errors, invalid_headers);
    }
}

/// Append purpose-style config suggestions for missing source headers.
fn append_purpose_style_suggestions(
    errors: &mut Vec<String>,
    missing_headers: &[String],
    config: &AtlasMapConfig,
) {
    let suggestions = missing_purpose_style_suggestions(missing_headers, config);
    if !suggestions.is_empty() {
        errors.push("Purpose style suggestions:".to_string());
        errors.push(format_list(&suggestions));
    }
}

/// Build purpose-style suggestions for source extensions using the default style.
fn missing_purpose_style_suggestions(
    missing_headers: &[String],
    config: &AtlasMapConfig,
) -> Vec<String> {
    let mut extensions = BTreeSet::new();
    for path in missing_headers {
        let extension = normalized_extension(path);
        if extension.is_empty() || config.purpose_styles.contains_key(&extension) {
            continue;
        }
        extensions.insert(extension);
    }
    extensions
        .into_iter()
        .map(|extension| {
            let style = suggested_purpose_style(&extension, config);
            let label = if is_default_source_extension(&extension) {
                "known source extension"
            } else {
                "custom source extension"
            };
            format!(
                "{extension}: add [purpose.styles_by_extension] \"{extension}\" = \"{style}\" ({label})"
            )
        })
        .collect()
}

/// Suggest a purpose style from known language tables, falling back to the configured default.
fn suggested_purpose_style(extension: &str, config: &AtlasMapConfig) -> String {
    match detect_language(Some(extension)).as_deref() {
        Some(
            "javascript" | "typescript" | "tsx" | "rust" | "go" | "shell" | "powershell" | "batch"
            | "ruby" | "python" | "groovy" | "protobuf" | "sql" | "graphql" | "toml" | "yaml"
            | "config" | "perl" | "lua" | "r" | "haskell" | "ocaml" | "fsharp" | "clojure" | "vim"
            | "zig",
        )
        | None => "line-comment".to_string(),
        Some(
            "c" | "cpp" | "h" | "hpp" | "java" | "kotlin" | "csharp" | "objective-c" | "swift"
            | "scala" | "css" | "dart",
        ) => "block-comment".to_string(),
        Some(_) => config.purpose_default_style.clone(),
    }
}

/// Return whether an extension is part of the default source-extension table.
fn is_default_source_extension(extension: &str) -> bool {
    DEFAULT_SOURCE_EXTENSIONS
        .iter()
        .any(|default| default.eq_ignore_ascii_case(extension))
}

/// Append folder validation errors.
fn append_folder_errors(
    errors: &mut Vec<String>,
    strict_folders: bool,
    missing_folders: &[String],
    invalid_folders: &BTreeMap<String, Vec<String>>,
) {
    if !invalid_folders.is_empty() {
        errors.push("Invalid folder Purpose summaries:".to_string());
        append_invalid_map(errors, invalid_folders);
    }
    if strict_folders && !missing_folders.is_empty() {
        errors.push("Missing folder Purpose files:".to_string());
        errors.push(format_list(missing_folders));
    }
}

/// Append invalid path issue map.
fn append_invalid_map(errors: &mut Vec<String>, invalid: &BTreeMap<String, Vec<String>>) {
    errors.extend(
        invalid
            .iter()
            .map(|(path, issues)| format!(" - {path}: {}", issues.join(", "))),
    );
}

/// Append untracked-file report and optional errors.
fn append_untracked_report(
    report: &mut Vec<String>,
    errors: &mut Vec<String>,
    config: &AtlasMapConfig,
    paths: &RepoPaths,
    options: LintOptions,
) -> AtlasMapResult<()> {
    let nonsource = read_nonsource_file_entries(config)?;
    let nonsource_paths = nonsource
        .records
        .iter()
        .map(|record| record.path.as_str())
        .collect::<BTreeSet<_>>();
    let db_purposes = load_db_purpose_records(config)?;
    let mut allowed = Vec::new();
    let mut disallowed = Vec::new();
    let mut asset_outside_roots = Vec::new();
    for path in &paths.untracked_files {
        if nonsource_paths.contains(path.as_str())
            || db_purposes.contains_key(path)
            || is_allowed_untracked(path, config)
        {
            allowed.push(path.clone());
        } else if is_asset_file(path, config)
            && !is_under_any_prefix(path, &config.asset_allowed_prefixes)
        {
            asset_outside_roots.push(path.clone());
            disallowed.push(path.clone());
        } else {
            disallowed.push(path.clone());
        }
    }
    report.push(format!(
        "Untracked files (non-source extensions): {} (allowed {}, disallowed {})",
        paths.untracked_files.len(),
        allowed.len(),
        disallowed.len()
    ));
    if disallowed.is_empty() {
        report.push("Disallowed untracked files: 0".to_string());
    } else {
        report.push("Disallowed untracked files:".to_string());
        report.push(format_list(&disallowed));
        report.push("Disallowed extension counts:".to_string());
        report.push(format_list(&summarize_extensions(&disallowed)));
    }
    report.push("Allowed untracked extension counts:".to_string());
    let allowed_summary = summarize_extensions(&allowed);
    report.push(if allowed_summary.is_empty() {
        " (none)".to_string()
    } else {
        format_list(&allowed_summary)
    });
    report.push(format!(
        "Asset roots present: {}",
        existing_asset_roots(config).len()
    ));
    if !asset_outside_roots.is_empty() {
        report.push("Asset files outside allowed roots:".to_string());
        report.push(format_list(&asset_outside_roots));
    }
    report.push(format!(
        "Excluded paths present: {}",
        paths.excluded_paths.len()
    ));
    if options.strict_untracked && !disallowed.is_empty() {
        errors.push("Untracked files detected.".to_string());
    }
    Ok(())
}

/// Append stale map validation errors.
fn append_stale_map_errors(
    errors: &mut Vec<String>,
    config: &AtlasMapConfig,
    expected_overview: &BTreeMap<String, usize>,
    expected_file_hash: &str,
    expected_folder_hash: &str,
) -> AtlasMapResult<()> {
    if !config.map_path.exists() {
        errors.push("Atlas map missing. Run: projectatlas map".to_string());
        return Ok(());
    }
    let content = fs::read_to_string(&config.map_path).map_err(|source| AtlasMapError::Io {
        path: config.map_path.clone(),
        source,
    })?;
    match read_overview(&content) {
        Some(overview) if &overview == expected_overview => {}
        Some(_) => errors.push("Atlas map overview stale. Run: projectatlas map".to_string()),
        None => errors.push("Atlas map overview invalid. Run: projectatlas map".to_string()),
    }
    let (file_hash, folder_hash) = read_hashes(&content);
    if file_hash.as_deref() != Some(expected_file_hash) {
        errors.push("Atlas map file hash stale. Run: projectatlas map".to_string());
    }
    if folder_hash.as_deref() != Some(expected_folder_hash) {
        errors.push("Atlas map folder hash stale. Run: projectatlas map".to_string());
    }
    Ok(())
}

/// Parse overview counters from TOON.
fn read_overview(content: &str) -> Option<BTreeMap<String, usize>> {
    let line = content
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("overview:"))?;
    let payload = line.split_once(':')?.1.trim();
    let mut overview = BTreeMap::new();
    for token in payload.split_whitespace() {
        let (key, value) = token.split_once('=')?;
        overview.insert(key.to_string(), value.parse::<usize>().ok()?);
    }
    if OVERVIEW_KEYS.iter().all(|key| overview.contains_key(*key)) {
        Some(overview)
    } else {
        None
    }
}

/// Parse file and folder hashes from TOON.
fn read_hashes(content: &str) -> (Option<String>, Option<String>) {
    let mut file_hash = None;
    let mut folder_hash = None;
    for line in content.lines().map(str::trim) {
        if let Some((_, value)) = line.split_once("file_hash:") {
            file_hash = Some(value.trim().trim_matches('"').to_string());
        }
        if let Some((_, value)) = line.split_once("folder_hash:") {
            folder_hash = Some(value.trim().trim_matches('"').to_string());
        }
    }
    (file_hash, folder_hash)
}

/// Parse the generated timestamp from TOON.
fn read_generated_at(content: &str) -> Option<String> {
    content.lines().map(str::trim).find_map(|line| {
        line.split_once("generated_at:")
            .map(|(_, value)| value.trim().to_string())
    })
}

/// Return whether an untracked path is allowed.
fn is_allowed_untracked(path: &str, config: &AtlasMapConfig) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    config.allowed_untracked_filenames.contains(name)
        || config.untracked_allowlist_files.contains(path)
        || is_under_any_prefix(path, &config.untracked_allowlist_dir_prefixes)
}

/// Return whether a file is an asset by extension.
fn is_asset_file(path: &str, config: &AtlasMapConfig) -> bool {
    config
        .asset_extensions
        .contains(&normalized_extension(path))
}

/// List existing asset roots.
fn existing_asset_roots(config: &AtlasMapConfig) -> Vec<String> {
    config
        .asset_allowed_prefixes
        .iter()
        .filter(|prefix| repo_join(&config.root, prefix).exists())
        .cloned()
        .collect()
}

/// Summarize extensions for reporting.
fn summarize_extensions(paths: &[String]) -> Vec<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for path in paths {
        let extension = normalized_extension(path);
        let key = if extension.is_empty() {
            "<no_ext>".to_string()
        } else {
            extension
        };
        *counts.entry(key).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(extension, count)| format!("{extension}={count}"))
        .collect()
}

/// Format report list items.
fn format_list(items: &[String]) -> String {
    items
        .iter()
        .map(|item| format!(" - {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Join report sections with newlines.
fn join_report(report: &[String]) -> String {
    if report.is_empty() {
        String::new()
    } else {
        report.join("\n") + "\n"
    }
}

/// Return whether a path is below any configured prefix.
fn is_under_any_prefix(path: &str, prefixes: &BTreeSet<String>) -> bool {
    prefixes
        .iter()
        .any(|prefix| path == prefix || path.starts_with(&format!("{prefix}/")))
}

/// Join a repository-relative slash path onto a root.
fn repo_join(root: &Path, rel_path: &str) -> PathBuf {
    if rel_path == "." {
        return root.to_path_buf();
    }
    rel_path
        .split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

/// Normalize a path-like string to repository slash format.
fn normalize_repo_string(path: &str) -> AtlasMapResult<String> {
    let value = path.trim();
    if value.is_empty() || value == "." {
        return Ok(".".to_string());
    }
    validated_repo_file_key(Path::new(value)).map_err(|source| {
        AtlasMapError::InvalidRepositoryPath {
            path: path.to_string(),
            message: source.to_string(),
        }
    })
}

/// Return a normalized extension from a repository path.
fn normalized_extension(path: &str) -> String {
    if path.ends_with(".d.ts") {
        return ".d.ts".to_string();
    }
    let file_name = path.rsplit('/').next().unwrap_or(path);
    match file_name.rsplit_once('.') {
        Some((prefix, suffix)) if !prefix.is_empty() => format!(".{}", suffix.to_ascii_lowercase()),
        _ => String::new(),
    }
}

/// Build default config text.
fn default_config_text() -> String {
    let source_extensions = toml_array(DEFAULT_SOURCE_EXTENSIONS);
    [
        "[project]",
        "root = \".\"",
        "map_path = \".projectatlas/projectatlas.toon\"",
        "nonsource_files_path = \".projectatlas/projectatlas-nonsource-files.toon\"",
        "purpose_filename = \".purpose\"",
        "",
        "[scan]",
        &format!("source_extensions = {source_extensions}"),
        "exclude_dir_names = [\".git\", \".projectatlas\", \".venv\", \"__pycache__\", \"node_modules\", \"dist\", \"build\", \"target\"]",
        "exclude_dir_suffixes = [\".egg-info\"]",
        "exclude_path_prefixes = []",
        "non_source_path_prefixes = []",
        "max_scan_lines = 80",
        &format!("text_index_max_bytes = {DEFAULT_TEXT_INDEX_MAX_BYTES}"),
        "",
        "[purpose]",
        "default_style = \"line-comment\"",
        "line_comment_prefixes = [\"//\", \"#\", \"--\", \";\"]",
        "",
        "[purpose.styles_by_extension]",
        "\".rs\" = \"line-comment\"",
        "",
        "[summary_rules]",
        "ascii_only = true",
        "no_commas = true",
        "max_length = 140",
        "",
        "[untracked]",
        "allowed_filenames = [\".purpose\"]",
        "allowlist_dir_prefixes = [\".githooks\"]",
        "allowlist_files = []",
        "asset_allowed_prefixes = []",
        "asset_extensions = [\".png\", \".jpg\", \".jpeg\", \".svg\", \".gif\", \".webp\", \".ico\", \".pdf\"]",
        "",
    ]
    .join("\n")
}

/// Default `.gitignore` text created only by the explicit setup helper.
fn default_gitignore_text() -> String {
    [
        "# ProjectAtlas local runtime state",
        ".projectatlas/*.db",
        ".projectatlas/*.db-*",
        ".projectatlas/projectatlas.mcp.json",
        "",
    ]
    .join("\n")
}

/// Render string values as a TOML array.
fn toml_array(values: &[&str]) -> String {
    let items = values
        .iter()
        .map(|value| format!("\"{value}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{items}]")
}

impl From<serde_json::Error> for AtlasMapError {
    fn from(source: serde_json::Error) -> Self {
        Self::Io {
            path: PathBuf::from("<json>"),
            source: std::io::Error::other(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AtlasMapConfig, DEFAULT_TEXT_INDEX_MAX_BYTES, MapRecord,
        append_existing_map_purpose_records, append_record_rows, exclude_dir_name_set,
        normalize_repo_string, project_root_for_projectatlas_config, split_record_cells,
        stable_generated_at, toon_cell,
    };
    use std::collections::{BTreeMap, BTreeSet};

    fn test_config(map_path: std::path::PathBuf) -> AtlasMapConfig {
        let root = map_path.parent().map_or_else(
            || std::path::PathBuf::from("."),
            std::path::Path::to_path_buf,
        );
        AtlasMapConfig {
            root: root.clone(),
            map_path,
            nonsource_files_path: root.join("projectatlas-nonsource-files.toon"),
            purpose_filename: ".purpose".to_string(),
            source_extensions: BTreeSet::new(),
            exclude_dir_names: BTreeSet::new(),
            exclude_dir_suffixes: BTreeSet::new(),
            exclude_path_prefixes: BTreeSet::new(),
            non_source_path_prefixes: BTreeSet::new(),
            allowed_untracked_filenames: BTreeSet::new(),
            untracked_allowlist_dir_prefixes: BTreeSet::new(),
            untracked_allowlist_files: BTreeSet::new(),
            asset_allowed_prefixes: BTreeSet::new(),
            asset_extensions: BTreeSet::new(),
            db_path: root.join("projectatlas.db"),
            max_scan_lines: 80,
            text_index_max_bytes: DEFAULT_TEXT_INDEX_MAX_BYTES,
            summary_max_length: 140,
            summary_ascii_only: true,
            summary_no_commas: true,
            purpose_styles: BTreeMap::new(),
            purpose_default_style: "line-comment".to_string(),
            line_comment_prefixes: vec!["//".to_string()],
        }
    }

    #[test]
    fn exclude_dir_names_preserve_required_internal_excludes() {
        let names = exclude_dir_name_set(Some(vec!["target".to_string()]));

        assert!(names.contains("target"));
        assert!(names.contains(".git"));
        assert!(names.contains(".projectatlas"));
    }

    #[test]
    fn toon_record_rows_escape_commas_quotes_and_newlines() {
        let mut lines = Vec::new();
        append_record_rows(
            &mut lines,
            "files",
            &[MapRecord {
                path: "docs/a,b.md".to_string(),
                summary: "Explain \"quoted\"\nsummary".to_string(),
                source: "source".to_string(),
            }],
        );

        assert_eq!(lines[0], "files[1]{path,summary,source}:");
        assert_eq!(
            lines[1],
            "  \"docs/a,b.md\",\"Explain \\\"quoted\\\"\\nsummary\",source"
        );
    }

    #[test]
    fn toon_record_parser_accepts_quoted_and_legacy_cells() {
        assert_eq!(
            split_record_cells("\"docs/a,b.md\",\"Summary, with comma\",source"),
            vec![
                "docs/a,b.md".to_string(),
                "Summary, with comma".to_string(),
                "source".to_string()
            ]
        );
        assert_eq!(
            split_record_cells("logo.png,Demo asset"),
            vec!["logo.png".to_string(), "Demo asset".to_string()]
        );
    }

    #[test]
    fn simple_toon_cells_remain_unquoted() {
        assert_eq!(toon_cell("src/main.rs"), "src/main.rs");
        assert_eq!(toon_cell("Plain summary"), "Plain summary");
    }

    #[test]
    fn repo_metadata_paths_reject_parent_traversal_and_absolute_paths()
    -> Result<(), Box<dyn std::error::Error>> {
        if normalize_repo_string("../outside.txt").is_ok() {
            return Err(std::io::Error::other("parent traversal was accepted").into());
        }
        if normalize_repo_string("C:/outside.txt").is_ok() {
            return Err(std::io::Error::other("absolute Windows path was accepted").into());
        }
        let normalized = normalize_repo_string("docs\\guide.md")?;
        if normalized != "docs/guide.md" {
            return Err(
                std::io::Error::other(format!("normalized path mismatch: {normalized}")).into(),
            );
        }
        Ok(())
    }

    #[test]
    fn bare_projectatlas_config_path_resolves_root_to_cwd() {
        let cwd = std::path::Path::new("repo");
        assert_eq!(
            project_root_for_projectatlas_config(
                Some(std::path::Path::new(".projectatlas/config.toml")),
                cwd,
            ),
            cwd
        );
        assert_eq!(
            project_root_for_projectatlas_config(
                Some(std::path::Path::new("./.projectatlas/config.toml")),
                cwd,
            ),
            cwd
        );
    }

    #[test]
    fn generated_at_is_stable_when_map_hashes_match() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let map_path = temp.path().join("projectatlas.toon");
        std::fs::write(
            &map_path,
            "version: 1\ngenerated_at: unix:123\nfile_hash: \"files\"\nfolder_hash: \"folders\"\n",
        )?;
        let config = test_config(map_path);

        let unchanged_generated_at = stable_generated_at(&config, "files", "folders");
        if unchanged_generated_at != "unix:123" {
            return Err(std::io::Error::other(format!(
                "expected stable timestamp, got {unchanged_generated_at}"
            ))
            .into());
        }
        let changed_generated_at = stable_generated_at(&config, "changed", "folders");
        if changed_generated_at == "unix:123" {
            return Err(std::io::Error::other("stale timestamp survived hash change").into());
        }
        Ok(())
    }

    #[test]
    fn existing_map_rows_seed_imported_purposes() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let map_path = temp.path().join("projectatlas.toon");
        std::fs::write(
            &map_path,
            [
                "version: 1",
                "folders[1]{path,summary,source}:",
                "  .,Repository root,database",
                "files[2]{path,summary,source}:",
                "  Cargo.toml,Rust workspace manifest,database",
                "  \"docs/a,b.md\",\"Quoted, summary\",database",
                "folder_summary_duplicates[]:",
            ]
            .join("\n"),
        )?;
        let config = test_config(map_path);
        let mut imported = BTreeMap::new();

        append_existing_map_purpose_records(&config, &mut imported)?;

        if imported.get(".").map(String::as_str) != Some("Repository root") {
            return Err(std::io::Error::other("root purpose was not imported").into());
        }
        if imported.get("Cargo.toml").map(String::as_str) != Some("Rust workspace manifest") {
            return Err(std::io::Error::other("Cargo purpose was not imported").into());
        }
        if imported.get("docs/a,b.md").map(String::as_str) != Some("Quoted, summary") {
            return Err(std::io::Error::other("quoted file purpose was not imported").into());
        }
        Ok(())
    }
}
