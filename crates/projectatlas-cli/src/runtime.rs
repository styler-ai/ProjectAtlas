//! Purpose: Coordinate shared `ProjectAtlas` CLI and MCP runtime workflows.
//! Shared runtime orchestration for the `ProjectAtlas` CLI and MCP adapters.

use crate::atlas_map::{
    self, imported_purpose_records, load_atlas_config, load_atlas_config_for_root,
};
use crate::structural::{
    is_scanner_fallback_summary, is_structural_summary_candidate, structural_summary_for_path,
};
use crate::{
    CliError, OutputFormat, WATCH_MODE_NOTIFY, WATCH_MODE_ONCE, WATCH_MODE_POLLING, truthy_env,
};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use projectatlas_core::language::{LanguageParserSupport, language_spec};
use projectatlas_core::outline::estimate_tokens;
use projectatlas_core::symbols::{RelationKind, SymbolGraph, SymbolKind};
use projectatlas_core::telemetry::{
    TOKEN_BASELINE_DIRECTORY_WALK, TOKEN_BASELINE_SELECTED_CANDIDATES,
    TOKEN_BUCKET_NAVIGATION_AVOIDANCE, TOKEN_CONFIDENCE_INFERRED, TOKEN_CONFIDENCE_POLICY_ESTIMATE,
    usage_from_estimates_with_context, usage_from_text,
};
use projectatlas_core::{
    Node, NodeKind, Overview, PurposeSource, PurposeStatus, normalize_native_path_display,
    normalize_native_path_display_str, normalize_repo_path, repo_path_to_native,
    validated_repo_file_key,
};
use projectatlas_db::{AtlasStore, IndexedFileText};
use projectatlas_fs::{ScanOptions, gitignore_excludes_path, scan_path, scan_repo};
use projectatlas_service::{
    FilePathMatcher, FileSummaryReport, file_summary_baseline_text, load_ranked_file_nodes,
};
use projectatlas_symbols::extract_symbol_graph;
use rayon::ThreadPoolBuilder;
use rayon::prelude::*;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

/// Maximum file size parsed for symbols by default.
pub(crate) const MAX_SYMBOL_FILE_BYTES: u64 = 2_000_000;

/// Built-in purposes for reserved project-local `ProjectAtlas` metadata inputs.
const BUILTIN_PROJECTATLAS_PURPOSES: &[(&str, &str)] = &[
    (
        ".projectatlas",
        "Store project-local ProjectAtlas metadata, configuration, and runtime state.",
    ),
    (
        ".projectatlas/config.toml",
        "Configure project-local ProjectAtlas scan, lint, purpose, and output policy.",
    ),
    (
        ".projectatlas/projectatlas-nonsource-files.toon",
        "Declare project-local non-source file purposes for ProjectAtlas map compatibility.",
    ),
];

/// Resolved scan runtime policy shared by CLI and MCP adapters.
pub(crate) struct ScanRuntimePlan {
    /// Canonical project root.
    pub(crate) root: PathBuf,
    /// Optional `ProjectAtlas` config discovered for the root.
    pub(crate) config: Option<atlas_map::AtlasMapConfig>,
    /// Filesystem scanner options derived from config.
    pub(crate) scan_options: ScanOptions,
    /// `SQLite` text-index options derived from config and command override.
    pub(crate) text_options: TextIndexOptions,
}

impl ScanRuntimePlan {
    /// Resolve scan policy for one project path.
    pub(crate) fn for_path(
        config_path: Option<&Path>,
        path: &Path,
        text_index_max_bytes: Option<u64>,
    ) -> Result<Self, CliError> {
        let root = canonical_project_root(path)?;
        let config = load_scan_import_config(config_path, &root)?;
        let scan_options = config.as_ref().map_or_else(
            ScanOptions::default,
            atlas_map::AtlasMapConfig::scan_options,
        );
        let text_options = text_index_options(config.as_ref(), text_index_max_bytes);
        Ok(Self {
            root,
            config,
            scan_options,
            text_options,
        })
    }
}

/// Scan command report shared by CLI and MCP adapters.
#[derive(Debug, Serialize)]
pub(crate) struct ScanReport {
    /// Repository overview after scan.
    pub(crate) overview: Overview,
    /// Legacy purpose records imported into the current index.
    pub(crate) purpose_import: PurposeImportReport,
    /// Persisted text search index report.
    pub(crate) text_index: TextIndexReport,
    /// Structural summaries refreshed for declaration-light files.
    pub(crate) structural_summaries: StructuralSummaryReport,
    /// Symbol graph build report.
    pub(crate) symbols: SymbolBuildReport,
}

/// Legacy purpose import counts from a scan.
#[derive(Debug, Default, Serialize)]
pub(crate) struct PurposeImportReport {
    /// Purpose records imported into indexed nodes.
    pub(crate) imported: usize,
    /// Legacy purpose records skipped because the path is no longer indexed.
    pub(crate) skipped_stale: usize,
}

/// Return a canonical absolute project root.
pub(crate) fn canonical_project_root(root: &Path) -> Result<PathBuf, CliError> {
    root.canonicalize().map_err(|source| CliError::Io {
        path: root.to_path_buf(),
        source,
    })
}

/// Load map configuration for purpose import during scan.
pub(crate) fn load_scan_import_config(
    config_path: Option<&Path>,
    scan_path: &Path,
) -> Result<Option<atlas_map::AtlasMapConfig>, CliError> {
    if let Some(config_path) = config_path {
        return Ok(Some(load_atlas_config(Some(config_path))?));
    }
    let project_config = scan_path.join(".projectatlas").join("config.toml");
    if project_config.exists() {
        return Ok(Some(load_atlas_config(Some(&project_config))?));
    }
    let flat_config = scan_path.join("projectatlas.toml");
    if flat_config.exists() {
        return Ok(Some(load_atlas_config(Some(&flat_config))?));
    }
    Ok(None)
}

/// Open or create a durable index, creating the parent directory only on demand.
pub(crate) fn open_atlas_store(path: &Path) -> Result<AtlasStore, CliError> {
    ensure_parent_dir(path)?;
    AtlasStore::open(path).map_err(CliError::from)
}

/// Create the parent directory for a path when it has one.
pub(crate) fn ensure_parent_dir(path: &Path) -> Result<(), CliError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    fs::create_dir_all(parent).map_err(|source| CliError::Io {
        path: parent.to_path_buf(),
        source,
    })
}

/// Resolve the default MCP project root without trusting the process cwd.
pub(crate) fn default_mcp_project_root(
    db: &Path,
    config_path: Option<&Path>,
) -> Result<PathBuf, CliError> {
    if let Some(config_path) = config_path {
        let config = load_atlas_config(Some(config_path))?;
        return canonical_project_root(&config.root);
    }
    if db.exists() {
        let store = AtlasStore::open(db)?;
        if let Some(project_root) = store.project_root()? {
            return canonical_project_root(Path::new(&project_root));
        }
    }
    if let Some(project_root) = project_root_from_db_path(db) {
        return canonical_project_root(&project_root);
    }
    let current_dir = std::env::current_dir().map_err(|source| CliError::Io {
        path: PathBuf::from("."),
        source,
    })?;
    canonical_project_root(&current_dir)
}

/// Resolve a CLI repository-root argument, using indexed state for the default `.`.
pub(crate) fn defaultable_cli_project_root(
    path: &Path,
    db: &Path,
    config_path: Option<&Path>,
) -> Result<PathBuf, CliError> {
    if path == Path::new(".") {
        return default_mcp_project_root(db, config_path);
    }
    Ok(path.to_path_buf())
}

/// Infer a project root from a default `.projectatlas/projectatlas.db` path.
fn project_root_from_db_path(db: &Path) -> Option<PathBuf> {
    let parent = db.parent()?;
    let cache_dir_name = parent.file_name()?;
    if cache_dir_name != ".projectatlas" {
        return None;
    }
    parent
        .parent()
        .filter(|root| !root.as_os_str().is_empty())
        .map_or_else(|| Some(PathBuf::from(".")), |root| Some(root.to_path_buf()))
}

/// Load scan options for a project root from `ProjectAtlas` config when present.
pub(crate) fn scan_options_for_root(
    config_path: Option<&Path>,
    root: &Path,
) -> Result<ScanOptions, CliError> {
    Ok(load_scan_import_config(config_path, root)?
        .as_ref()
        .map_or_else(
            ScanOptions::default,
            atlas_map::AtlasMapConfig::scan_options,
        ))
}

/// Resolve text-index persistence options from command override and config.
pub(crate) fn text_index_options(
    config: Option<&atlas_map::AtlasMapConfig>,
    max_bytes_override: Option<u64>,
) -> TextIndexOptions {
    let max_bytes = max_bytes_override
        .filter(|value| *value > 0)
        .or_else(|| config.map(atlas_map::AtlasMapConfig::text_index_max_bytes))
        .unwrap_or(atlas_map::DEFAULT_TEXT_INDEX_MAX_BYTES);
    TextIndexOptions::new(max_bytes)
}

/// Execute the full scan/index/symbol pipeline for a resolved project plan.
pub(crate) fn run_scan_pipeline(
    store: &mut AtlasStore,
    plan: &ScanRuntimePlan,
    symbol_options: &SymbolBuildOptions,
) -> Result<ScanReport, CliError> {
    let nodes = scan_repo(&plan.root, &plan.scan_options)?;
    store.set_project_root(&plan.root)?;
    store.replace_scan(&nodes)?;
    seed_builtin_projectatlas_purposes(store, &nodes)?;
    let text_refresh =
        refresh_text_index_for_nodes_with_rows(store, &plan.root, &nodes, plan.text_options)?;
    let text_index = text_refresh.report.clone();
    let indexed_paths = nodes
        .iter()
        .map(|node| node.path.as_str())
        .collect::<HashSet<_>>();
    let mut purpose_import = PurposeImportReport::default();
    if let Some(config) = plan.config.as_ref() {
        for record in imported_purpose_records(config)? {
            if !indexed_paths.contains(record.path.as_str()) {
                purpose_import.skipped_stale += 1;
                continue;
            }
            store.set_purpose(&record.path, &record.summary, PurposeSource::Imported)?;
            purpose_import.imported += 1;
        }
    }
    let symbols = build_symbols_for_index(store, &plan.root, symbol_options, None)?;
    let structural_summaries =
        refresh_structural_summaries_for_nodes(store, &nodes, &text_refresh.rows)?;
    let overview = store.overview()?;
    Ok(ScanReport {
        overview,
        purpose_import,
        text_index,
        structural_summaries,
        symbols,
    })
}

/// Record a usage event from a fast baseline estimate and actual atlas payload.
pub(crate) fn record_usage_estimate(
    store: &AtlasStore,
    session: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    estimated_without_projectatlas: usize,
    projectatlas_text: &str,
) -> Result<(), CliError> {
    record_usage_estimate_with_context(
        store,
        session,
        command,
        path,
        query,
        estimated_without_projectatlas,
        projectatlas_text,
        TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
        TOKEN_BASELINE_SELECTED_CANDIDATES,
        TOKEN_CONFIDENCE_INFERRED,
    )
}

/// Record a usage event from a fast baseline estimate and explicit baseline semantics.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_usage_estimate_with_context(
    store: &AtlasStore,
    session: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    estimated_without_projectatlas: usize,
    projectatlas_text: &str,
    token_savings_bucket: &str,
    baseline_kind: &str,
    confidence: &str,
) -> Result<(), CliError> {
    if telemetry_disabled() {
        return Ok(());
    }
    store.record_usage(&usage_from_estimates_with_context(
        session,
        command,
        path,
        query,
        estimated_without_projectatlas,
        estimate_tokens(projectatlas_text),
        token_savings_bucket,
        baseline_kind,
        confidence,
    ))?;
    Ok(())
}

/// Record a broad directory-walk avoidance estimate.
pub(crate) fn record_directory_walk_usage_estimate(
    store: &AtlasStore,
    session: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    estimated_without_projectatlas: usize,
    projectatlas_text: &str,
) -> Result<(), CliError> {
    record_usage_estimate_with_context(
        store,
        session,
        command,
        path,
        query,
        estimated_without_projectatlas,
        projectatlas_text,
        TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
        TOKEN_BASELINE_DIRECTORY_WALK,
        TOKEN_CONFIDENCE_POLICY_ESTIMATE,
    )
}

/// Record a usage event from baseline and emitted text unless telemetry is disabled.
pub(crate) fn record_usage_text(
    store: &AtlasStore,
    session: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    baseline_text: &str,
    projectatlas_text: &str,
) -> Result<(), CliError> {
    if telemetry_disabled() {
        return Ok(());
    }
    store.record_usage(&usage_from_text(
        session,
        command,
        path,
        query,
        baseline_text,
        projectatlas_text,
    ))?;
    Ok(())
}

/// Return whether telemetry writes are disabled for read-only review contexts.
pub(crate) fn telemetry_disabled() -> bool {
    truthy_env("PROJECTATLAS_NO_TELEMETRY")
}

/// Estimate broad source tokens represented by indexed files with SQL aggregates.
pub(crate) fn estimated_source_tokens_for_indexed_files(
    store: &AtlasStore,
    folder: Option<&str>,
    file_pattern: Option<&str>,
) -> Result<usize, CliError> {
    let matcher = FilePathMatcher::new(file_pattern)?;
    let mut total = 0usize;
    store.visit_file_token_estimates(folder, |path, size_bytes| {
        if matcher.is_match(&path) {
            total =
                total.saturating_add(estimated_source_tokens_for_file_metadata(&path, size_bytes));
        }
        Ok(true)
    })?;
    Ok(total)
}

/// Estimate source tokens for one indexed file without reading it.
pub(crate) fn estimated_source_tokens_for_file_node(node: &Node) -> usize {
    estimated_source_tokens_for_file_metadata(&node.path, node.size_bytes)
}

/// Estimate source tokens for persisted file metadata.
pub(crate) fn estimated_source_tokens_for_file_metadata(
    path: &str,
    size_bytes: Option<u64>,
) -> usize {
    size_bytes.map_or_else(|| estimate_tokens(path), byte_size_to_tokens)
}

/// Estimate source tokens from a byte count with the shared token heuristic.
pub(crate) fn byte_size_to_tokens(bytes: u64) -> usize {
    let token_estimate = bytes.div_ceil(4);
    usize::try_from(token_estimate).unwrap_or(usize::MAX)
}

/// Estimate source tokens from a searched byte count.
pub(crate) fn byte_count_to_tokens(bytes: usize) -> usize {
    if bytes == 0 { 0 } else { bytes.div_ceil(4) }
}

/// Load ranked file nodes in bounded pages and apply exact glob semantics.
pub(crate) fn ranked_file_nodes(
    store: &AtlasStore,
    query: &str,
    folder: Option<&str>,
    file_pattern: Option<&str>,
    limit: usize,
) -> Result<Vec<projectatlas_core::IndexedNode>, CliError> {
    Ok(load_ranked_file_nodes(
        store,
        query,
        folder,
        file_pattern,
        limit,
    )?)
}

/// Estimate source tokens for repository paths referenced by symbols/relations.
pub(crate) fn estimated_source_tokens_for_paths<'a>(
    store: &AtlasStore,
    paths: impl Iterator<Item = &'a str>,
) -> Result<usize, CliError> {
    let mut seen = HashSet::new();
    let mut total = 0usize;
    for path in paths {
        if seen.insert(path.to_string()) {
            total = total.saturating_add(estimated_source_tokens_for_path(store, path)?);
        }
    }
    Ok(total)
}

/// Estimate source tokens for one indexed path, falling back safely for stale rows.
pub(crate) fn estimated_source_tokens_for_path(
    store: &AtlasStore,
    path: &str,
) -> Result<usize, CliError> {
    if let Some(indexed) = store.load_node_by_path(path)?
        && indexed.node.kind == NodeKind::File
    {
        return Ok(estimated_source_tokens_for_file_node(&indexed.node));
    }
    Ok(read_indexed_file_content(store, path).map_or_else(
        |_| estimate_tokens(path),
        |content| estimate_tokens(&content),
    ))
}

/// Build the best available baseline for a file-summary usage event.
pub(crate) fn file_summary_usage_baseline(
    store: &AtlasStore,
    report: &FileSummaryReport,
) -> Result<String, CliError> {
    read_indexed_file_content(store, &report.file_path)
        .or_else(|_| file_summary_baseline_text(report).map_err(CliError::from))
}

/// Persisted file-text index report.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct TextIndexReport {
    /// File nodes considered for indexed text.
    pub(crate) candidates: usize,
    /// UTF-8 files persisted for `SQLite`-backed search.
    pub(crate) indexed: usize,
    /// Files skipped because text could not be decoded as UTF-8.
    pub(crate) binary_or_non_utf8: usize,
    /// Files skipped because they exceeded the configured text-index size cap.
    pub(crate) too_large: usize,
    /// Total files skipped from the persisted text index.
    pub(crate) skipped: usize,
    /// Maximum UTF-8 file size persisted into `SQLite` text search.
    pub(crate) max_bytes: u64,
    /// Source bytes stored in the text index.
    pub(crate) bytes: usize,
}

/// Deterministic structural-summary refresh report.
#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct StructuralSummaryReport {
    /// Indexed files considered for structural summaries.
    pub(crate) candidates: usize,
    /// Files whose observed summaries were refreshed.
    pub(crate) summarized: usize,
    /// Existing observed summaries cleared because current content was not summarizable.
    pub(crate) cleared: usize,
    /// Files skipped because they exceeded the parser size limit.
    pub(crate) too_large: usize,
    /// Files skipped because content was not valid UTF-8.
    pub(crate) binary_or_non_utf8: usize,
    /// Generated purpose suggestions that still need agent review.
    pub(crate) purpose_suggestions: usize,
}

/// Options controlling full-text persistence for `SQLite` search.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextIndexOptions {
    /// Maximum UTF-8 file size persisted into `SQLite` text search.
    pub(crate) max_bytes: u64,
}

impl TextIndexOptions {
    /// Create text-index options from config and command overrides.
    pub(crate) fn new(max_bytes: u64) -> Self {
        Self { max_bytes }
    }
}

/// Outcome of considering one file for persisted text search.
#[derive(Clone, Debug)]
pub(crate) struct TextIndexRow {
    /// Repository-relative path considered for text indexing.
    path: String,
    /// Persistable text row when the file is search-indexed.
    text: Option<IndexedFileText>,
    /// Indexing outcome for reporting.
    reason: TextIndexSkipReason,
}

/// Persisted text refresh result plus rows reused by structural summarizers.
pub(crate) struct TextIndexRefresh {
    /// Aggregate report rendered to callers.
    pub(crate) report: TextIndexReport,
    /// Per-file text outcomes from the same scan batch.
    pub(crate) rows: Vec<TextIndexRow>,
}

/// Text-index outcome categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TextIndexSkipReason {
    /// File text was persisted for search.
    Indexed,
    /// File exceeded the configured text-index size cap.
    TooLarge,
    /// File was binary or not valid UTF-8.
    BinaryOrNonUtf8,
}

/// Symbol graph build report.
#[derive(Debug, Serialize)]
pub(crate) struct SymbolBuildReport {
    /// Indexed file candidates considered for symbols.
    pub(crate) candidates: usize,
    /// Files parsed during this build.
    pub(crate) parsed: usize,
    /// Files skipped because they were unchanged and already had symbols.
    pub(crate) unchanged: usize,
    /// Files skipped because they exceeded the configured size limit.
    pub(crate) too_large: usize,
    /// Files skipped because content was not valid UTF-8.
    pub(crate) binary_or_non_utf8: usize,
    /// Files skipped because the build deadline was reached.
    pub(crate) timed_out: usize,
    /// Worker thread count requested for parser work.
    pub(crate) max_workers: usize,
    /// Optional timeout seconds requested for parser work.
    pub(crate) timeout_seconds: Option<u64>,
    /// Symbols persisted.
    pub(crate) symbols: usize,
    /// Relations persisted.
    pub(crate) relations: usize,
    /// Node summaries refreshed from symbol graphs.
    pub(crate) summaries: usize,
    /// Generated purpose suggestions that still need agent review.
    pub(crate) purpose_suggestions: usize,
}

/// Watch command report.
#[derive(Debug, Serialize)]
pub(crate) struct WatchReport {
    /// Watcher mode.
    pub(crate) mode: String,
    /// Completed refresh cycles.
    pub(crate) cycles: usize,
    /// Whether the command ran a single refresh and exited.
    pub(crate) once: bool,
    /// Reason the watcher fell back from event mode, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fallback_reason: Option<String>,
    /// Last persisted text search index report.
    pub(crate) text_index: TextIndexReport,
    /// Last structural summary refresh report.
    pub(crate) structural_summaries: StructuralSummaryReport,
    /// Last symbol refresh report.
    pub(crate) last_symbols: SymbolBuildReport,
}

/// Debounced filesystem changes observed by watcher mode.
#[derive(Debug, Default)]
pub(crate) struct WatchChangeSet {
    /// Whether a full scan is required for correctness.
    requires_full_scan: bool,
    /// Relevant native paths from event batches.
    paths: HashSet<PathBuf>,
}

impl WatchChangeSet {
    /// Return whether there is work to refresh.
    fn has_changes(&self) -> bool {
        self.requires_full_scan || !self.paths.is_empty()
    }

    /// Merge another event batch into this set.
    fn merge(&mut self, other: Self) {
        self.requires_full_scan |= other.requires_full_scan;
        self.paths.extend(other.paths);
    }
}

/// Legacy purpose cleanup report.
#[derive(Debug, Serialize)]
pub(crate) struct LegacyPurposeReport {
    /// Whether files were modified.
    pub(crate) applied: bool,
    /// Number of `.purpose` files found.
    pub(crate) purpose_files_found: usize,
    /// Number of `.purpose` files removed.
    pub(crate) purpose_files_removed: usize,
    /// Source header candidates found.
    pub(crate) source_header_candidates: Vec<String>,
    /// Legacy purpose file paths.
    pub(crate) purpose_files: Vec<String>,
}

/// Local settings report.
#[derive(Debug, Serialize)]
pub(crate) struct SettingsReport {
    /// Runtime cache directory that owns local `ProjectAtlas` state.
    pub(crate) cache_dir: PathStatus,
    /// `SQLite` database file status.
    pub(crate) db: PathStatus,
    /// `SQLite` write-ahead log file status.
    pub(crate) db_wal: PathStatus,
    /// `SQLite` shared-memory sidecar file status.
    pub(crate) db_shm: PathStatus,
    /// `SQLite` rollback journal sidecar file status.
    pub(crate) db_journal: PathStatus,
    /// Project-local MCP configuration file status.
    pub(crate) mcp_config: PathStatus,
    /// Config file used for map/lint/scan imports, when discovered.
    pub(crate) config_path: Option<String>,
    /// Repository root used by map/lint config.
    pub(crate) repo_root: String,
    /// Source that selected the repository root.
    pub(crate) root_detection_source: String,
    /// Whether config and DB root metadata agree.
    pub(crate) root_verified: bool,
    /// Root mismatches that should be fixed before trusting the binding.
    pub(crate) root_mismatches: Vec<String>,
    /// Generated map path.
    pub(crate) map_path: String,
    /// Non-source summary path.
    pub(crate) nonsource_files_path: String,
    /// Default output format.
    pub(crate) default_format: String,
    /// Default search case sensitivity.
    pub(crate) default_search_case_sensitive: bool,
    /// Source used by search commands.
    pub(crate) search_source: String,
    /// Maximum UTF-8 file size persisted into `SQLite` text search.
    pub(crate) text_index_max_bytes: u64,
    /// Watcher runtime status.
    pub(crate) watcher: WatchStatusReport,
    /// Current index statistics, if the index exists.
    pub(crate) index: Option<SettingsIndexStats>,
}

/// Filesystem status for a diagnostic path.
#[derive(Debug, Serialize)]
pub(crate) struct PathStatus {
    /// Normalized native path.
    pub(crate) path: String,
    /// Whether the path exists.
    pub(crate) exists: bool,
    /// File size in bytes when the path is an existing file.
    pub(crate) size_bytes: Option<u64>,
}

/// Indexed state summary for settings diagnostics.
#[derive(Debug, Serialize)]
pub(crate) struct SettingsIndexStats {
    /// Canonical project root stored in the index metadata.
    pub(crate) project_root: Option<String>,
    /// Indexed file count.
    pub(crate) files: usize,
    /// Indexed folder count.
    pub(crate) folders: usize,
    /// Missing purpose count.
    pub(crate) missing_purposes: usize,
    /// Stale purpose count.
    pub(crate) stale_purposes: usize,
    /// Suggested purpose count.
    pub(crate) suggested_purposes: usize,
    /// Persisted searchable text rows.
    pub(crate) indexed_text_files: usize,
    /// Persisted searchable text bytes.
    pub(crate) indexed_text_bytes: usize,
    /// Persisted symbol count.
    pub(crate) symbols: usize,
    /// Persisted symbol relation count.
    pub(crate) relations: usize,
    /// Token telemetry event count.
    pub(crate) token_calls: usize,
    /// Unresolved structural health finding count.
    pub(crate) health_findings: usize,
}

/// Watcher status report.
#[derive(Debug, Serialize)]
pub(crate) struct WatchStatusReport {
    /// Whether a watcher implementation is available in this binary.
    pub(crate) available: bool,
    /// Whether a watcher is active.
    pub(crate) active: bool,
    /// Watcher mode.
    pub(crate) mode: String,
    /// Whether event-backed watching is available.
    pub(crate) event_backend_available: bool,
    /// Operational recommendation.
    pub(crate) recommendation: String,
}

/// Runtime index/cache cleanup report.
#[derive(Debug, Serialize)]
pub(crate) struct ResetIndexReport {
    /// Whether files were modified.
    pub(crate) applied: bool,
    /// Whether the command only previewed paths.
    pub(crate) dry_run: bool,
    /// Runtime files selected for cleanup.
    files: Vec<PathStatus>,
    /// Number of selected files removed.
    pub(crate) removed: usize,
}

/// Build settings diagnostics shared by CLI and MCP.
pub(crate) fn build_settings_report(
    db: &Path,
    config_path: Option<&Path>,
    format: OutputFormat,
) -> Result<SettingsReport, CliError> {
    let absolute_db = absolute_path(db)?;
    let resolved_config = resolved_mcp_config_path(&absolute_db, config_path)?;
    let config = if let Some(config_path) = resolved_config.as_deref() {
        load_atlas_config(Some(config_path))?
    } else {
        let project_root = default_mcp_project_root(&absolute_db, None)?;
        load_atlas_config_for_root(&project_root)?
    };
    let cache_dir = absolute_db
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let index = if absolute_db.exists() {
        let store = AtlasStore::open(&absolute_db)?;
        Some(settings_index_stats(&store)?)
    } else {
        None
    };
    let repo_root = normalize_display_path(&config.root);
    let db_project_root = index
        .as_ref()
        .and_then(|stats| stats.project_root.as_ref())
        .cloned();
    let mut root_mismatches = Vec::new();
    if let Some(db_root) = db_project_root.as_ref()
        && db_root != &repo_root
    {
        root_mismatches.push(format!(
            "db root {db_root:?} does not match config root {repo_root:?}"
        ));
    }
    let root_detection_source = if resolved_config.is_some() {
        "config"
    } else if db_project_root.is_some() {
        "db"
    } else {
        "db-path-or-cwd"
    }
    .to_string();
    Ok(SettingsReport {
        cache_dir: path_status(&cache_dir)?,
        db: path_status(&absolute_db)?,
        db_wal: path_status(&db_sidecar_path(&absolute_db, "wal"))?,
        db_shm: path_status(&db_sidecar_path(&absolute_db, "shm"))?,
        db_journal: path_status(&db_sidecar_path(&absolute_db, "journal"))?,
        mcp_config: path_status(&mcp_config_path_for_db(&absolute_db))?,
        config_path: resolved_config.map(|path| normalize_display_path(&path)),
        repo_root,
        root_detection_source,
        root_verified: root_mismatches.is_empty(),
        root_mismatches,
        map_path: normalize_display_path(&config.map_path),
        nonsource_files_path: normalize_display_path(&config.nonsource_files_path),
        default_format: format!("{format:?}").to_ascii_lowercase(),
        default_search_case_sensitive: false,
        search_source: "sqlite-file-text".to_string(),
        text_index_max_bytes: config.text_index_max_bytes(),
        watcher: watcher_status_report(false),
        index,
    })
}

/// Build index statistics for settings diagnostics.
pub(crate) fn settings_index_stats(store: &AtlasStore) -> Result<SettingsIndexStats, CliError> {
    let overview = store.overview()?;
    let health_findings = store
        .unresolved_health_findings(&store.resolved_health_ids()?)?
        .len();
    Ok(SettingsIndexStats {
        project_root: store
            .project_root()?
            .map(|path| normalize_native_path_display_str(&path)),
        files: overview.files,
        folders: overview.folders,
        missing_purposes: overview.missing_purposes,
        stale_purposes: overview.stale_purposes,
        suggested_purposes: overview.suggested_purposes,
        indexed_text_files: store.file_text_count()?,
        indexed_text_bytes: store.file_text_byte_count()?,
        symbols: store.symbol_count()?,
        relations: store.symbol_relation_count()?,
        token_calls: store.token_overview(None)?.calls,
        health_findings,
    })
}

/// Preview or remove local runtime index/cache files.
pub(crate) fn reset_index_files(
    db: &Path,
    apply: bool,
    dry_run: bool,
    include_mcp_config: bool,
) -> Result<ResetIndexReport, CliError> {
    let absolute_db = absolute_path(db)?;
    let mut targets = vec![
        absolute_db.clone(),
        db_sidecar_path(&absolute_db, "wal"),
        db_sidecar_path(&absolute_db, "shm"),
        db_sidecar_path(&absolute_db, "journal"),
    ];
    if include_mcp_config {
        targets.push(mcp_config_path_for_db(&absolute_db));
    }
    targets.sort();
    targets.dedup();
    let files = targets
        .iter()
        .map(|path| path_status(path))
        .collect::<Result<Vec<_>, _>>()?;
    let should_apply = apply && !dry_run;
    let mut removed = 0;
    if should_apply {
        for target in &targets {
            if target.is_file() {
                fs::remove_file(target).map_err(|source| CliError::Io {
                    path: target.clone(),
                    source,
                })?;
                removed += 1;
            }
        }
    }
    Ok(ResetIndexReport {
        applied: should_apply,
        dry_run: !should_apply,
        files,
        removed,
    })
}

/// Resolve the config path that should travel with generated MCP configs.
pub(crate) fn resolved_mcp_config_path(
    db: &Path,
    config: Option<&Path>,
) -> Result<Option<PathBuf>, CliError> {
    if let Some(path) = config {
        return Ok(Some(absolute_path(path)?));
    }
    let mut candidate_roots = Vec::new();
    if db.exists() {
        let store = AtlasStore::open(db)?;
        if let Some(project_root) = store.project_root()? {
            candidate_roots.push(PathBuf::from(project_root));
        }
    }
    let absolute_db = absolute_path(db)?;
    if let Some(project_root) = project_root_from_db_path(&absolute_db) {
        candidate_roots.push(project_root);
    }
    for root in candidate_roots {
        for candidate in config_candidates_for_root(&root) {
            if candidate.exists() {
                return Ok(Some(absolute_path(&candidate)?));
            }
        }
    }
    Ok(None)
}

/// Return supported config paths for one project root.
fn config_candidates_for_root(root: &Path) -> [PathBuf; 2] {
    [
        root.join(".projectatlas").join("config.toml"),
        root.join("projectatlas.toml"),
    ]
}

/// Return an absolute path without requiring the target to exist.
pub(crate) fn absolute_path(path: &Path) -> Result<PathBuf, CliError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let current_dir = std::env::current_dir().map_err(|source| CliError::Io {
        path: PathBuf::from("."),
        source,
    })?;
    Ok(current_dir.join(path))
}

/// Return a diagnostic status for one path.
pub(crate) fn path_status(path: &Path) -> Result<PathStatus, CliError> {
    let absolute = absolute_path(path)?;
    let metadata = fs::metadata(&absolute).ok();
    Ok(PathStatus {
        path: normalize_display_path(&absolute),
        exists: metadata.is_some(),
        size_bytes: metadata
            .as_ref()
            .and_then(|metadata| metadata.is_file().then_some(metadata.len())),
    })
}

/// Return the path to a `SQLite` sidecar file.
pub(crate) fn db_sidecar_path(db: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}-{suffix}", db.display()))
}

/// Return the project-local MCP config path associated with a database path.
pub(crate) fn mcp_config_path_for_db(db: &Path) -> PathBuf {
    db.parent().map_or_else(
        || PathBuf::from("projectatlas.mcp.json"),
        |parent| parent.join("projectatlas.mcp.json"),
    )
}

/// Normalize a path for JSON/TOON diagnostics.
pub(crate) fn normalize_display_path(path: &Path) -> String {
    normalize_native_path_display(path)
}

/// Build a watcher status report from a lightweight runtime probe.
pub(crate) fn watcher_status_report(active: bool) -> WatchStatusReport {
    let notify_available = notify_runtime_available();
    let mode = if notify_available {
        WATCH_MODE_NOTIFY
    } else {
        WATCH_MODE_POLLING
    };
    let recommendation = if notify_available {
        "Run `projectatlas watch --once` for one refresh or `projectatlas watch` for event-backed refresh with portable polling fallback."
    } else {
        "Run `projectatlas watch --once` for one refresh or `projectatlas watch` for portable polling refresh."
    };
    WatchStatusReport {
        available: true,
        active,
        mode: mode.to_string(),
        event_backend_available: notify_available,
        recommendation: recommendation.to_string(),
    }
}

/// Build lint output for an existing `SQLite` index.
pub(crate) fn lint_database_if_present(db: &Path) -> Result<(String, i32), CliError> {
    if !db.exists() {
        return Ok((String::new(), 0));
    }
    let store = AtlasStore::open(db)?;
    let findings = store.unresolved_health_findings(&store.resolved_health_ids()?)?;
    let blocking = findings
        .iter()
        .filter(|finding| {
            matches!(
                finding.category.as_str(),
                "missing-purpose"
                    | "suggested-purpose-review"
                    | "stale-purpose"
                    | "duplicate-purpose"
                    | "repeated-temporary-folder"
            )
        })
        .collect::<Vec<_>>();
    if blocking.is_empty() {
        return Ok((String::new(), 0));
    }
    let mut report = String::from("ProjectAtlas SQLite index health findings:\n");
    for finding in blocking {
        writeln!(
            &mut report,
            "- [{}] {}: {}",
            finding.category, finding.path, finding.recommendation
        )
        .map_err(|source| CliError::Output(io::Error::other(source.to_string())))?;
    }
    Ok((report, 1))
}

/// Return whether the platform watcher can be constructed in this process.
pub(crate) fn notify_runtime_available() -> bool {
    let (sender, _receiver) = mpsc::channel();
    RecommendedWatcher::new(
        move |result: notify::Result<Event>| {
            if sender.send(result).is_err() {
                // Receiver shutdown only means this status probe is done.
            }
        },
        Config::default(),
    )
    .is_ok()
}

/// Options controlling source parsing during symbol graph builds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SymbolBuildOptions {
    /// Maximum file size parsed for symbols.
    pub(crate) max_bytes: u64,
    /// Optional maximum worker threads for parser work.
    max_workers: Option<usize>,
    /// Optional deadline for starting parser work.
    timeout: Option<Duration>,
    /// Serialized timeout value for reports.
    pub(crate) timeout_seconds: Option<u64>,
}

impl SymbolBuildOptions {
    /// Create symbol build options from CLI/MCP values.
    pub(crate) fn new(
        max_bytes: u64,
        max_workers: Option<usize>,
        timeout_seconds: Option<u64>,
    ) -> Self {
        Self {
            max_bytes,
            max_workers: max_workers.filter(|workers| *workers > 0),
            timeout: timeout_seconds.map(Duration::from_secs),
            timeout_seconds,
        }
    }

    /// Return the worker count that will be reported.
    pub(crate) fn reported_workers(self) -> usize {
        self.max_workers
            .unwrap_or_else(|| thread::available_parallelism().map_or(1, usize::from))
    }

    /// Return whether the parser build deadline has elapsed.
    pub(crate) fn is_timed_out(self, started_at: Instant) -> bool {
        self.timeout
            .is_some_and(|timeout| started_at.elapsed() >= timeout)
    }
}

/// Source file queued for symbol parsing.
#[derive(Clone, Debug)]
pub(crate) struct SymbolParseJob {
    /// Repository-relative file path.
    pub(crate) path: String,
    /// Native absolute file path.
    native_path: PathBuf,
    /// Detected language name.
    language: Option<String>,
    /// Existing node summary fallback.
    fallback_summary: Option<String>,
    /// Whether a generated purpose suggestion should be written.
    purpose_missing: bool,
}

/// Successful parser output waiting for sequential DB persistence.
#[derive(Debug)]
pub(crate) struct SymbolParseSuccess {
    /// Repository-relative file path.
    pub(crate) path: String,
    /// Extracted symbol graph.
    graph: SymbolGraph,
    /// Observed one-line source summary.
    summary: String,
    /// Optional generated purpose suggestion.
    purpose_suggestion: Option<String>,
}

/// Outcome from one parser worker.
#[derive(Debug)]
pub(crate) enum SymbolParseOutcome {
    /// Source parsed successfully.
    Parsed(SymbolParseSuccess),
    /// File was skipped because the build deadline elapsed.
    TimedOut {
        /// Repository-relative file path.
        path: String,
    },
    /// File was skipped because it was not UTF-8 source text.
    BinaryOrNonUtf8 {
        /// Repository-relative file path.
        path: String,
    },
    /// Source read failed.
    Io {
        /// Native path that failed to read.
        path: PathBuf,
        /// Source IO error.
        source: io::Error,
    },
}

/// Build symbol graphs for indexed files.
pub(crate) fn build_symbols_for_index(
    store: &mut AtlasStore,
    root: &Path,
    options: &SymbolBuildOptions,
    previous_hashes: Option<&HashMap<String, String>>,
) -> Result<SymbolBuildReport, CliError> {
    build_symbols_for_paths(store, root, options, previous_hashes, None)
}

/// Build symbol graphs for selected indexed files.
pub(crate) fn build_symbols_for_paths(
    store: &mut AtlasStore,
    root: &Path,
    options: &SymbolBuildOptions,
    previous_hashes: Option<&HashMap<String, String>>,
    target_paths: Option<&HashSet<String>>,
) -> Result<SymbolBuildReport, CliError> {
    let root = root.canonicalize().map_err(|source| CliError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    let nodes = if let Some(paths) = target_paths {
        let mut sorted_paths = paths.iter().cloned().collect::<Vec<_>>();
        sorted_paths.sort();
        store.load_nodes_by_paths(&sorted_paths)?
    } else {
        store.load_nodes()?
    };
    let mut report = SymbolBuildReport {
        candidates: 0,
        parsed: 0,
        unchanged: 0,
        too_large: 0,
        binary_or_non_utf8: 0,
        timed_out: 0,
        max_workers: options.reported_workers(),
        timeout_seconds: options.timeout_seconds,
        symbols: 0,
        relations: 0,
        summaries: 0,
        purpose_suggestions: 0,
    };
    let mut jobs = Vec::new();
    for node in nodes
        .iter()
        .filter(|node| node.node.kind == NodeKind::File)
        .filter(|node| is_symbol_candidate(&node.node.path, node.node.language.as_deref()))
    {
        report.candidates += 1;
        if node
            .node
            .size_bytes
            .is_some_and(|size| size > options.max_bytes)
        {
            clear_skipped_symbol_index(store, &node.node.path, node.node.language.as_deref())?;
            report.too_large += 1;
            continue;
        }
        let symbol_count = store.symbol_count_for_path(&node.node.path)?;
        if node.node.content_hash.as_ref().is_some_and(|hash| {
            previous_hashes.and_then(|hashes| hashes.get(&node.node.path)) == Some(hash)
        }) {
            let has_source_index =
                symbol_count > 0 || store.load_source_parse_metadata(&node.node.path)?.is_some();
            if has_source_index {
                report.unchanged += 1;
                continue;
            }
        }
        jobs.push(SymbolParseJob {
            path: node.node.path.clone(),
            native_path: root.join(repo_path_to_native(&node.node.path)),
            language: node.node.language.clone(),
            fallback_summary: node.summary.clone(),
            purpose_missing: node.purpose.status == PurposeStatus::Missing,
        });
    }
    let started_at = Instant::now();
    for outcome in parse_symbol_jobs(&jobs, options, started_at)? {
        match outcome {
            SymbolParseOutcome::Parsed(parsed) => {
                report.symbols += parsed.graph.symbols.len();
                report.relations += parsed.graph.relations.len();
                store.set_node_summary(&parsed.path, &parsed.summary)?;
                report.summaries += 1;
                if let Some(suggestion) = parsed.purpose_suggestion {
                    store.set_suggested_purpose(&parsed.path, &suggestion)?;
                    report.purpose_suggestions += 1;
                }
                store.replace_symbol_graph(&parsed.graph)?;
                report.parsed += 1;
            }
            SymbolParseOutcome::TimedOut { path } => {
                clear_skipped_symbol_index_for_path(store, &path)?;
                report.timed_out += 1;
            }
            SymbolParseOutcome::BinaryOrNonUtf8 { path } => {
                clear_skipped_symbol_index_for_path(store, &path)?;
                report.binary_or_non_utf8 += 1;
            }
            SymbolParseOutcome::Io { path, source } => {
                return Err(CliError::Io { path, source });
            }
        }
    }
    Ok(report)
}

/// Parse queued symbol jobs with optional worker limits.
pub(crate) fn parse_symbol_jobs(
    jobs: &[SymbolParseJob],
    options: &SymbolBuildOptions,
    started_at: Instant,
) -> Result<Vec<SymbolParseOutcome>, CliError> {
    let mut builder = ThreadPoolBuilder::new();
    if let Some(max_workers) = options.max_workers {
        builder = builder.num_threads(max_workers);
    }
    let pool = builder
        .build()
        .map_err(|source| CliError::InvalidInput(format!("symbol worker pool failed: {source}")))?;
    Ok(pool.install(|| {
        jobs.par_iter()
            .map(|job| parse_symbol_job(job, options, started_at))
            .collect::<Vec<_>>()
    }))
}

/// Parse one source file into a symbol graph.
pub(crate) fn parse_symbol_job(
    job: &SymbolParseJob,
    options: &SymbolBuildOptions,
    started_at: Instant,
) -> SymbolParseOutcome {
    if options.is_timed_out(started_at) {
        return SymbolParseOutcome::TimedOut {
            path: job.path.clone(),
        };
    }
    let bytes = match fs::read(&job.native_path) {
        Ok(bytes) => bytes,
        Err(source) => {
            return SymbolParseOutcome::Io {
                path: job.native_path.clone(),
                source,
            };
        }
    };
    let Ok(content) = String::from_utf8(bytes) else {
        return SymbolParseOutcome::BinaryOrNonUtf8 {
            path: job.path.clone(),
        };
    };
    if options.is_timed_out(started_at) {
        return SymbolParseOutcome::TimedOut {
            path: job.path.clone(),
        };
    }
    let graph = extract_symbol_graph(&job.path, job.language.as_deref(), &content);
    let summary = summarize_symbol_graph(&graph, job.fallback_summary.as_deref());
    let purpose_suggestion = job
        .purpose_missing
        .then(|| suggest_file_purpose(&job.path, &summary));
    SymbolParseOutcome::Parsed(SymbolParseSuccess {
        path: job.path.clone(),
        graph,
        summary,
        purpose_suggestion,
    })
}

/// Return an empty symbol build report.
pub(crate) fn empty_symbol_build_report() -> SymbolBuildReport {
    SymbolBuildReport {
        candidates: 0,
        parsed: 0,
        unchanged: 0,
        too_large: 0,
        binary_or_non_utf8: 0,
        timed_out: 0,
        max_workers: 0,
        timeout_seconds: None,
        symbols: 0,
        relations: 0,
        summaries: 0,
        purpose_suggestions: 0,
    }
}

/// Create a deterministic one-line content summary from extracted symbols.
pub(crate) fn summarize_symbol_graph(graph: &SymbolGraph, fallback: Option<&str>) -> String {
    if graph.symbols.is_empty() {
        if let Some(fallback) = fallback.filter(|summary| !is_scanner_fallback_summary(summary)) {
            return fallback.to_string();
        }
        let language = observed_language_label(graph.language.as_deref());
        return format!("{language} source file with no declarations found.");
    }
    let language = observed_language_label(graph.language.as_deref());
    let primary_names = primary_symbol_names(graph, 4);
    let primary_kinds = primary_symbol_kinds(graph);
    let imports = relation_targets(graph, RelationKind::Imports, 2);
    let dependencies = relation_targets(graph, RelationKind::DependsOn, 3);
    if !dependencies.is_empty() {
        let subject = observed_manifest_subject(&language);
        return format!(
            "{subject} declaring {} and depending on {}.",
            primary_names.join(", "),
            dependencies.join(", ")
        );
    }
    if !imports.is_empty() {
        return format!(
            "{language} source defining {} {} with imports {}.",
            primary_kinds,
            primary_names.join(", "),
            imports.join(", ")
        );
    }
    format!(
        "{language} source defining {} {}.",
        primary_kinds,
        primary_names.join(", ")
    )
}

/// Return a readable language label for agent-facing content summaries.
fn observed_language_label(language: Option<&str>) -> String {
    match language.unwrap_or("source") {
        "cargo-manifest" => "cargo manifest".to_string(),
        "cargo-lock" => "cargo lock".to_string(),
        "rust-build-script" => "rust build script".to_string(),
        "objective-c" => "Objective-C".to_string(),
        "csharp" => "C#".to_string(),
        "cpp" => "C++".to_string(),
        other => other.replace('-', " "),
    }
}

/// Return the subject phrase for manifest-style content summaries.
fn observed_manifest_subject(language: &str) -> String {
    if language.contains("manifest") {
        language.to_string()
    } else {
        format!("{language} manifest")
    }
}

/// Return a compact phrase describing the most important symbol kinds.
pub(crate) fn primary_symbol_kinds(graph: &SymbolGraph) -> String {
    let mut function_like = 0_usize;
    let mut type_like = 0_usize;
    let mut manifest_like = 0_usize;
    let mut value_like = 0_usize;
    for symbol in &graph.symbols {
        match symbol.kind {
            SymbolKind::Function | SymbolKind::Method => function_like += 1,
            SymbolKind::Class
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Trait
            | SymbolKind::Interface
            | SymbolKind::Type => type_like += 1,
            SymbolKind::Package | SymbolKind::Workspace | SymbolKind::Dependency => {
                manifest_like += 1;
            }
            SymbolKind::Value => value_like += 1,
            SymbolKind::Module | SymbolKind::Import | SymbolKind::Unknown => {}
        }
    }
    if manifest_like > 0 && function_like == 0 && type_like == 0 {
        return "manifest entries".to_string();
    }
    if value_like > 0 && function_like == 0 && type_like == 0 {
        return value_only_symbol_kind_label(graph, value_like);
    }
    match (type_like, function_like) {
        (0, 0) => "symbols".to_string(),
        (0, 1) => "function".to_string(),
        (0, _) => "functions".to_string(),
        (1, 0) => "type".to_string(),
        (_, 0) => "types".to_string(),
        (1, 1) => "type and function".to_string(),
        (1, _) => "type and functions".to_string(),
        (_, 1) => "types and function".to_string(),
        (_, _) => "types and functions".to_string(),
    }
}

/// Return the right value-only summary noun for the indexed language.
pub(crate) fn value_only_symbol_kind_label(graph: &SymbolGraph, count: usize) -> String {
    let language = graph.language.as_deref().unwrap_or_default();
    let binding_language = matches!(
        language,
        "javascript" | "typescript" | "tsx" | "vue" | "svelte"
    ) || graph
        .symbols
        .iter()
        .any(|symbol| symbol.detail.as_deref() == Some("fallback-composition-binding"));
    let singular = if binding_language { "binding" } else { "value" };
    let plural = if binding_language {
        "bindings"
    } else {
        "values"
    };
    if count == 1 {
        singular.to_string()
    } else {
        plural.to_string()
    }
}

/// Return stable names for the most important declaration symbols.
pub(crate) fn primary_symbol_names(graph: &SymbolGraph, limit: usize) -> Vec<String> {
    let has_primary_definitions = graph.symbols.iter().any(|symbol| {
        matches!(
            symbol.kind,
            SymbolKind::Function
                | SymbolKind::Method
                | SymbolKind::Class
                | SymbolKind::Struct
                | SymbolKind::Enum
                | SymbolKind::Trait
                | SymbolKind::Interface
                | SymbolKind::Type
        )
    });
    let mut names = graph
        .symbols
        .iter()
        .filter(|symbol| {
            if has_primary_definitions && symbol.kind == SymbolKind::Value {
                return false;
            }
            !matches!(
                symbol.kind,
                SymbolKind::Import
                    | SymbolKind::Dependency
                    | SymbolKind::Module
                    | SymbolKind::Unknown
            )
        })
        .map(|symbol| symbol.name.clone())
        .collect::<Vec<_>>();
    if names.is_empty() {
        names = graph
            .symbols
            .iter()
            .map(|symbol| symbol.name.clone())
            .collect::<Vec<_>>();
    }
    names.sort();
    names.dedup();
    names.truncate(limit);
    if names.is_empty() {
        vec!["indexed symbols".to_string()]
    } else {
        names
    }
}

/// Return relation targets for one relation kind.
pub(crate) fn relation_targets(
    graph: &SymbolGraph,
    kind: RelationKind,
    limit: usize,
) -> Vec<String> {
    let mut targets = graph
        .relations
        .iter()
        .filter(|relation| relation.kind == kind)
        .map(|relation| relation.target_name.clone())
        .collect::<Vec<_>>();
    targets.sort();
    targets.dedup();
    targets.truncate(limit);
    targets
}

/// Create a generated file-purpose suggestion from a path and content summary.
pub(crate) fn suggest_file_purpose(path: &str, summary: &str) -> String {
    let name = path
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(path);
    let stem = name.split_once('.').map_or(name, |(stem, _)| stem);
    let subject = stem.replace(['-', '_'], " ");
    if summary.contains("dataset manifest") {
        if let Some(datasets) = summary_between(summary, " including ", " and keys") {
            format!("Define the {subject} dataset manifest for {datasets}.")
        } else {
            format!("Define the {subject} dataset manifest.")
        }
    } else if let Some(workflow) = summary_between(summary, "yaml workflow ", " triggered") {
        format!("Define the {workflow} workflow.")
    } else if summary.contains("manifest") {
        if let Some(package) = summary_between(summary, " manifest for ", " with ") {
            format!("Define the {package} manifest.")
        } else {
            format!("Define the {subject} manifest.")
        }
    } else if let Some(title) = summary_between(summary, "document titled ", " with ") {
        format!("Document {title}.")
    } else if summary.contains("stylesheet") {
        format!("Style the {subject} stylesheet.")
    } else if summary.contains("config") {
        format!("Configure {subject}.")
    } else {
        format!("Implement {subject}.")
    }
}

/// Return a non-empty substring between two markers.
fn summary_between<'a>(summary: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let after_start = summary.split_once(start)?.1;
    let value = after_start.split_once(end)?.0.trim();
    (!value.is_empty()).then_some(value)
}

/// Return whether a language should be parsed for symbols.
pub(crate) fn is_symbol_candidate(path: &str, language: Option<&str>) -> bool {
    if path.ends_with("Cargo.toml")
        || path.ends_with("Cargo.lock")
        || matches!(language, Some("cargo-manifest" | "cargo-lock"))
    {
        return true;
    }
    if matches!(language, Some("vue")) {
        return true;
    }
    language.is_some_and(|language| {
        !matches!(
            language_spec(language).map(|spec| spec.parser_support),
            Some(LanguageParserSupport::Structural)
        )
    })
}

/// Clear stale symbol output while preserving structural summaries when present.
fn clear_skipped_symbol_index(
    store: &AtlasStore,
    path: &str,
    language: Option<&str>,
) -> Result<(), CliError> {
    if is_structural_summary_candidate(path, language) {
        store.clear_symbol_graph_for_path(path)?;
    } else {
        store.clear_source_index_for_path(path)?;
    }
    Ok(())
}

/// Clear stale symbol output for a skipped path loaded from the index.
fn clear_skipped_symbol_index_for_path(store: &AtlasStore, path: &str) -> Result<(), CliError> {
    let language = store
        .load_node_by_path(path)?
        .and_then(|indexed| indexed.node.language);
    clear_skipped_symbol_index(store, path, language.as_deref())
}

/// Normalize and validate a user-supplied path as a repository-relative file key.
pub(crate) fn validated_file_key(file: &Path) -> Result<String, CliError> {
    validated_repo_file_key(file).map_err(|source| CliError::InvalidInput(source.to_string()))
}

/// Normalize a folder filter into the repository path convention.
pub(crate) fn normalized_folder_filter(folder: &str) -> Result<String, CliError> {
    let trimmed = folder.trim().trim_end_matches(['/', '\\']);
    if trimmed.is_empty() || trimmed == "." {
        return Ok(".".to_string());
    }
    validated_file_key(Path::new(trimmed)).map_err(|_error| {
        CliError::InvalidInput(format!(
            "folder filter {folder:?} must be a project-relative path"
        ))
    })
}

/// Validate that a path belongs to the indexed project file set.
pub(crate) fn validated_indexed_file_key(
    store: &AtlasStore,
    file: &Path,
) -> Result<String, CliError> {
    let file_key = validated_file_key(file)?;
    let indexed = store
        .load_node_by_path(&file_key)?
        .ok_or_else(|| CliError::InvalidInput(format!("file {file_key:?} is not indexed")))?;
    if indexed.node.kind != NodeKind::File {
        return Err(CliError::InvalidInput(format!(
            "path {file_key:?} is not an indexed file"
        )));
    }
    Ok(file_key)
}

/// Load the project root recorded by the latest scan.
pub(crate) fn indexed_project_root(store: &AtlasStore) -> Result<PathBuf, CliError> {
    store.project_root()?.map(PathBuf::from).ok_or_else(|| {
        CliError::InvalidInput(
            "indexed project root is missing; run projectatlas scan <project-root> first"
                .to_string(),
        )
    })
}

/// Build an absolute native path for a previously validated indexed file key.
pub(crate) fn indexed_native_path(store: &AtlasStore, file_key: &str) -> Result<PathBuf, CliError> {
    Ok(indexed_project_root(store)?.join(repo_path_to_native(file_key)))
}

/// Read content for a previously validated indexed file key.
pub(crate) fn read_indexed_file_content(
    store: &AtlasStore,
    file_key: &str,
) -> Result<String, CliError> {
    let native = indexed_native_path(store, file_key)?;
    fs::read_to_string(&native).map_err(|source| CliError::Io {
        path: native,
        source,
    })
}

/// Run the watcher refresh loop.
pub(crate) fn run_watch_loop(
    store: &mut AtlasStore,
    root: &Path,
    once: bool,
    poll_seconds: u64,
    max_cycles: usize,
    symbol_options: &SymbolBuildOptions,
    scan_options: &ScanOptions,
    text_options: TextIndexOptions,
) -> Result<WatchReport, CliError> {
    if once {
        return run_single_watch_refresh(store, root, symbol_options, scan_options, text_options);
    }
    match run_notify_watch_loop(
        store,
        root,
        poll_seconds,
        max_cycles,
        symbol_options,
        scan_options,
        text_options,
    ) {
        Ok(report) => Ok(report),
        Err(error) => run_polling_watch_loop(
            store,
            root,
            poll_seconds,
            max_cycles,
            symbol_options,
            scan_options,
            text_options,
            Some(error.to_string()),
        ),
    }
}

/// Run one deterministic watcher refresh and exit.
pub(crate) fn run_single_watch_refresh(
    store: &mut AtlasStore,
    root: &Path,
    symbol_options: &SymbolBuildOptions,
    scan_options: &ScanOptions,
    text_options: TextIndexOptions,
) -> Result<WatchReport, CliError> {
    let last_refresh = refresh_index(store, root, symbol_options, scan_options, text_options)?;
    Ok(WatchReport {
        mode: WATCH_MODE_ONCE.to_string(),
        cycles: 1,
        once: true,
        fallback_reason: None,
        text_index: last_refresh.text_index,
        structural_summaries: last_refresh.structural_summaries,
        last_symbols: last_refresh.symbols,
    })
}

/// Run an event-backed watcher loop with `notify`.
pub(crate) fn run_notify_watch_loop(
    store: &mut AtlasStore,
    root: &Path,
    poll_seconds: u64,
    max_cycles: usize,
    symbol_options: &SymbolBuildOptions,
    scan_options: &ScanOptions,
    text_options: TextIndexOptions,
) -> Result<WatchReport, CliError> {
    let watch_root = root.canonicalize().map_err(|source| CliError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    let (sender, receiver) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(
        move |result: notify::Result<Event>| {
            if sender.send(result).is_err() {
                // Receiver shutdown means the command is exiting.
            }
        },
        Config::default(),
    )
    .map_err(|source| CliError::Watcher(source.to_string()))?;
    watcher
        .watch(&watch_root, RecursiveMode::Recursive)
        .map_err(|source| CliError::Watcher(source.to_string()))?;
    let debounce = Duration::from_secs(poll_seconds.max(1));
    let mut cycles = 0;
    let mut last_refresh = refresh_index(
        store,
        &watch_root,
        symbol_options,
        scan_options,
        text_options,
    )?;
    cycles += 1;
    while max_cycles == 0 || cycles < max_cycles {
        let changes = wait_for_index_event(&receiver, &watch_root, debounce, scan_options)?;
        if changes.has_changes() {
            last_refresh = refresh_index_for_changes(
                store,
                &watch_root,
                &changes,
                symbol_options,
                scan_options,
                text_options,
            )?;
            cycles += 1;
        }
    }
    Ok(WatchReport {
        mode: WATCH_MODE_NOTIFY.to_string(),
        cycles,
        once: false,
        fallback_reason: None,
        text_index: last_refresh.text_index,
        structural_summaries: last_refresh.structural_summaries,
        last_symbols: last_refresh.symbols,
    })
}

/// Wait for a debounced batch of relevant filesystem events.
pub(crate) fn wait_for_index_event(
    receiver: &mpsc::Receiver<notify::Result<Event>>,
    root: &Path,
    debounce: Duration,
    scan_options: &ScanOptions,
) -> Result<WatchChangeSet, CliError> {
    let mut changes = notify_result_changes(
        root,
        scan_options,
        receiver.recv().map_err(|source| {
            CliError::Watcher(format!("watch event channel disconnected: {source}"))
        })?,
    )?;
    loop {
        match receiver.recv_timeout(debounce) {
            Ok(result) => {
                changes.merge(notify_result_changes(root, scan_options, result)?);
            }
            Err(RecvTimeoutError::Timeout) => break,
            Err(RecvTimeoutError::Disconnected) => {
                return Err(CliError::Watcher(
                    "watch event channel disconnected".to_string(),
                ));
            }
        }
    }
    Ok(changes)
}

/// Convert a `notify` result into index-relevant changes.
pub(crate) fn notify_result_changes(
    root: &Path,
    scan_options: &ScanOptions,
    result: notify::Result<Event>,
) -> Result<WatchChangeSet, CliError> {
    let event = result.map_err(|source| CliError::Watcher(source.to_string()))?;
    Ok(notify_event_changes(root, scan_options, &event))
}

/// Convert a `notify` event into index-relevant changes.
pub(crate) fn notify_event_changes(
    root: &Path,
    scan_options: &ScanOptions,
    event: &Event,
) -> WatchChangeSet {
    if !event_kind_affects_index(event.kind) {
        return WatchChangeSet::default();
    }
    let mut changes = WatchChangeSet::default();
    for path in &event.paths {
        if !watch_path_affects_index(root, path, scan_options) {
            continue;
        }
        let absolute = absolute_watch_path(root, path);
        if watch_path_requires_full_scan(root, &absolute) {
            changes.requires_full_scan = true;
        }
        changes.paths.insert(absolute);
    }
    changes
}

/// Return whether a `notify` event kind can change indexed content.
pub(crate) fn event_kind_affects_index(kind: EventKind) -> bool {
    !matches!(kind, EventKind::Access(_))
}

/// Return whether a native event path belongs to indexed repository content.
pub(crate) fn watch_path_affects_index(
    root: &Path,
    path: &Path,
    scan_options: &ScanOptions,
) -> bool {
    let candidate = absolute_watch_path(root, path);
    let Some(relative) = safe_watch_relative_path(root, &candidate) else {
        return false;
    };
    if relative == "." {
        return true;
    }
    // Unknown ignore state should not admit a path into the incremental index.
    let Ok(gitignore_ignored) = gitignore_excludes_path(root, &candidate) else {
        return false;
    };
    if gitignore_ignored {
        return false;
    }
    !relative.split('/').any(|component| component == ".purpose")
        && !scan_options.excludes_relative_path(&relative)
}

/// Return a safe normalized repository path for a watcher event.
fn safe_watch_relative_path(root: &Path, candidate: &Path) -> Option<String> {
    let relative = normalize_repo_path(root, candidate).ok()?;
    if relative == "." {
        return Some(relative);
    }
    if relative
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return None;
    }
    Some(relative)
}

/// Return an absolute path for a watcher event path.
pub(crate) fn absolute_watch_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

/// Return whether a path event requires a full scan for correctness.
pub(crate) fn watch_path_requires_full_scan(root: &Path, path: &Path) -> bool {
    if path == root {
        return true;
    }
    path.is_dir()
        || path.file_name().is_some_and(|name| name == ".gitignore")
        || normalize_repo_path(root, path).is_ok_and(|normalized| normalized == ".")
}

/// Run the portable polling watcher fallback loop.
pub(crate) fn run_polling_watch_loop(
    store: &mut AtlasStore,
    root: &Path,
    poll_seconds: u64,
    max_cycles: usize,
    symbol_options: &SymbolBuildOptions,
    scan_options: &ScanOptions,
    text_options: TextIndexOptions,
    fallback_reason: Option<String>,
) -> Result<WatchReport, CliError> {
    let mut cycles = 0;
    let mut last_refresh = refresh_index(store, root, symbol_options, scan_options, text_options)?;
    cycles += 1;
    while max_cycles == 0 || cycles < max_cycles {
        thread::sleep(Duration::from_secs(poll_seconds.max(1)));
        last_refresh = refresh_index(store, root, symbol_options, scan_options, text_options)?;
        cycles += 1;
    }
    Ok(WatchReport {
        mode: WATCH_MODE_POLLING.to_string(),
        cycles,
        once: false,
        fallback_reason,
        text_index: last_refresh.text_index,
        structural_summaries: last_refresh.structural_summaries,
        last_symbols: last_refresh.symbols,
    })
}

/// Combined refresh output for watcher and one-shot refresh paths.
pub(crate) struct IndexRefreshReport {
    /// Persisted text search index refresh report.
    pub(crate) text_index: TextIndexReport,
    /// Structural summary refresh report.
    pub(crate) structural_summaries: StructuralSummaryReport,
    /// Deep symbol graph refresh report.
    symbols: SymbolBuildReport,
}

/// Refresh filesystem and symbol state.
pub(crate) fn refresh_index(
    store: &mut AtlasStore,
    root: &Path,
    symbol_options: &SymbolBuildOptions,
    scan_options: &ScanOptions,
    text_options: TextIndexOptions,
) -> Result<IndexRefreshReport, CliError> {
    let root = canonical_project_root(root)?;
    let previous_hashes = indexed_file_hashes(store)?;
    let nodes = scan_repo(&root, scan_options)?;
    store.set_project_root(&root)?;
    store.replace_scan(&nodes)?;
    seed_builtin_projectatlas_purposes(store, &nodes)?;
    let text_refresh = refresh_text_index_for_nodes_with_rows(store, &root, &nodes, text_options)?;
    let text_index = text_refresh.report.clone();
    let symbols = build_symbols_for_index(store, &root, symbol_options, Some(&previous_hashes))?;
    let structural_summaries =
        refresh_structural_summaries_for_nodes(store, &nodes, &text_refresh.rows)?;
    Ok(IndexRefreshReport {
        text_index,
        structural_summaries,
        symbols,
    })
}

/// Refresh filesystem and symbol state for a debounced event batch.
pub(crate) fn refresh_index_for_changes(
    store: &mut AtlasStore,
    root: &Path,
    changes: &WatchChangeSet,
    symbol_options: &SymbolBuildOptions,
    scan_options: &ScanOptions,
    text_options: TextIndexOptions,
) -> Result<IndexRefreshReport, CliError> {
    if changes.requires_full_scan {
        return refresh_index(store, root, symbol_options, scan_options, text_options);
    }
    let root = canonical_project_root(root)?;
    let mut nodes = Vec::new();
    let mut absent_paths = Vec::new();
    for path in sorted_watch_paths(&changes.paths) {
        if path.exists() {
            if let Some(node) = scan_path(&root, &path, scan_options)? {
                nodes.push(node);
            }
        } else if let Some(path_key) = normalized_deleted_path(&root, &path)? {
            absent_paths.push(path_key);
        }
    }
    let changed_paths = nodes
        .iter()
        .map(|node| node.path.clone())
        .chain(absent_paths.iter().cloned())
        .collect::<HashSet<_>>();
    let previous_hashes = indexed_file_hashes_for_paths(store, &changed_paths)?;
    store.set_project_root(&root)?;
    if !nodes.is_empty() {
        store.upsert_scan_nodes(&nodes)?;
        seed_builtin_projectatlas_purposes(store, &nodes)?;
    }
    if !absent_paths.is_empty() {
        store.mark_paths_absent(&absent_paths)?;
    }
    let text_refresh = refresh_text_index_for_changed_paths_with_rows(
        store,
        &root,
        &changed_paths,
        &nodes,
        text_options,
    )?;
    let text_index = text_refresh.report.clone();
    let target_paths = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .map(|node| node.path.clone())
        .collect::<HashSet<_>>();
    if target_paths.is_empty() {
        let structural_summaries =
            refresh_structural_summaries_for_nodes(store, &nodes, &text_refresh.rows)?;
        return Ok(IndexRefreshReport {
            text_index,
            structural_summaries,
            symbols: empty_symbol_build_report(),
        });
    }
    let symbols = build_symbols_for_paths(
        store,
        &root,
        symbol_options,
        Some(&previous_hashes),
        Some(&target_paths),
    )?;
    let structural_summaries =
        refresh_structural_summaries_for_nodes(store, &nodes, &text_refresh.rows)?;
    Ok(IndexRefreshReport {
        text_index,
        structural_summaries,
        symbols,
    })
}

/// Seed built-in purposes for reserved `ProjectAtlas` metadata nodes when needed.
pub(crate) fn seed_builtin_projectatlas_purposes(
    store: &AtlasStore,
    nodes: &[Node],
) -> Result<(), CliError> {
    let indexed_paths = nodes
        .iter()
        .map(|node| node.path.as_str())
        .collect::<HashSet<_>>();
    for (path, purpose) in BUILTIN_PROJECTATLAS_PURPOSES {
        if !indexed_paths.contains(path) {
            continue;
        }
        let Some(indexed) = store.load_node_by_path(path)? else {
            continue;
        };
        if indexed.purpose.status != PurposeStatus::Approved {
            store.set_purpose(path, purpose, PurposeSource::Imported)?;
        }
    }
    Ok(())
}

/// Refresh deterministic structural summaries for indexed declaration-light files.
pub(crate) fn refresh_structural_summaries_for_nodes(
    store: &mut AtlasStore,
    nodes: &[Node],
    text_rows: &[TextIndexRow],
) -> Result<StructuralSummaryReport, CliError> {
    let paths = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .filter(|node| is_structural_summary_candidate(&node.path, node.language.as_deref()))
        .map(|node| node.path.clone())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Ok(StructuralSummaryReport::default());
    }
    let indexed_nodes = store.load_nodes_by_paths(&paths)?;
    let symbol_counts = store.symbol_counts_for_paths(&paths)?;
    let text_by_path = text_rows
        .iter()
        .filter_map(|row| row.text.as_ref().map(|text| (text.path.as_str(), text)))
        .collect::<HashMap<_, _>>();
    let reason_by_path = text_rows
        .iter()
        .map(|row| (row.path.as_str(), row.reason))
        .collect::<HashMap<_, _>>();
    let mut report = StructuralSummaryReport {
        candidates: indexed_nodes.len(),
        ..StructuralSummaryReport::default()
    };
    for indexed in indexed_nodes {
        if reason_by_path.get(indexed.node.path.as_str()) == Some(&TextIndexSkipReason::TooLarge)
            || indexed
                .node
                .size_bytes
                .is_some_and(|size_bytes| size_bytes > MAX_SYMBOL_FILE_BYTES)
        {
            store.clear_node_summary(&indexed.node.path)?;
            report.cleared += 1;
            report.too_large += 1;
            continue;
        }
        let Some(text) = text_by_path.get(indexed.node.path.as_str()) else {
            store.clear_node_summary(&indexed.node.path)?;
            report.cleared += 1;
            if reason_by_path.get(indexed.node.path.as_str())
                == Some(&TextIndexSkipReason::BinaryOrNonUtf8)
            {
                report.binary_or_non_utf8 += 1;
            }
            continue;
        };
        if symbol_counts
            .get(indexed.node.path.as_str())
            .is_some_and(|count| *count > 0)
            && indexed.summary.as_deref().is_some_and(|summary| {
                !summary.trim().is_empty() && !is_scanner_fallback_summary(summary)
            })
        {
            continue;
        }
        let Some(summary) = structural_summary_for_path(
            &indexed.node.path,
            indexed.node.language.as_deref(),
            &text.content,
        ) else {
            store.clear_node_summary(&indexed.node.path)?;
            report.cleared += 1;
            continue;
        };
        store.set_node_summary(&indexed.node.path, &summary)?;
        report.summarized += 1;
        if indexed.purpose.status == PurposeStatus::Missing {
            store.set_suggested_purpose(
                &indexed.node.path,
                &suggest_file_purpose(&indexed.node.path, &summary),
            )?;
            report.purpose_suggestions += 1;
        }
    }
    Ok(report)
}

/// Refresh the persisted text index for every scanned file node.
#[cfg(test)]
pub(crate) fn refresh_text_index_for_nodes(
    store: &mut AtlasStore,
    root: &Path,
    nodes: &[Node],
    options: TextIndexOptions,
) -> Result<TextIndexReport, CliError> {
    Ok(refresh_text_index_for_nodes_with_rows(store, root, nodes, options)?.report)
}

/// Refresh the persisted text index and retain the in-memory text rows.
pub(crate) fn refresh_text_index_for_nodes_with_rows(
    store: &mut AtlasStore,
    root: &Path,
    nodes: &[Node],
    options: TextIndexOptions,
) -> Result<TextIndexRefresh, CliError> {
    let file_paths = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .map(|node| node.path.clone())
        .collect::<Vec<_>>();
    refresh_text_index_for_changed_paths_with_rows(
        store,
        root,
        &file_paths.iter().cloned().collect::<HashSet<_>>(),
        nodes,
        options,
    )
}

/// Refresh persisted text index rows for an incremental path set and retain outcomes.
pub(crate) fn refresh_text_index_for_changed_paths_with_rows(
    store: &mut AtlasStore,
    root: &Path,
    changed_paths: &HashSet<String>,
    nodes: &[Node],
    options: TextIndexOptions,
) -> Result<TextIndexRefresh, CliError> {
    let mut considered_paths = changed_paths.iter().cloned().collect::<Vec<_>>();
    considered_paths.sort();
    let text_rows = indexed_file_texts_for_nodes(root, nodes, options)?;
    let texts = text_rows
        .iter()
        .filter_map(|row| row.text.clone())
        .collect::<Vec<_>>();
    let file_candidates = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .count();
    let binary_or_non_utf8 = text_rows
        .iter()
        .filter(|row| row.reason == TextIndexSkipReason::BinaryOrNonUtf8)
        .count();
    let too_large = text_rows
        .iter()
        .filter(|row| row.reason == TextIndexSkipReason::TooLarge)
        .count();
    let report = TextIndexReport {
        candidates: file_candidates,
        indexed: texts.len(),
        binary_or_non_utf8,
        too_large,
        skipped: file_candidates.saturating_sub(texts.len()),
        max_bytes: options.max_bytes,
        bytes: texts
            .iter()
            .map(|text| text.byte_count)
            .fold(0usize, usize::saturating_add),
    };
    store.replace_file_texts_for_paths(&considered_paths, &texts)?;
    Ok(TextIndexRefresh {
        report,
        rows: text_rows,
    })
}

/// Build indexed text rows for UTF-8 scanned files with size caps.
pub(crate) fn indexed_file_texts_for_nodes(
    root: &Path,
    nodes: &[Node],
    options: TextIndexOptions,
) -> Result<Vec<TextIndexRow>, CliError> {
    let mut rows = Vec::new();
    for node in nodes.iter().filter(|node| node.kind == NodeKind::File) {
        if node
            .size_bytes
            .is_some_and(|size_bytes| size_bytes > options.max_bytes)
        {
            rows.push(TextIndexRow {
                path: node.path.clone(),
                text: None,
                reason: TextIndexSkipReason::TooLarge,
            });
            continue;
        }
        let native_path = root.join(repo_path_to_native(&node.path));
        let bytes = fs::read(&native_path).map_err(|source| CliError::Io {
            path: native_path.clone(),
            source,
        })?;
        let Ok(content) = String::from_utf8(bytes) else {
            rows.push(TextIndexRow {
                path: node.path.clone(),
                text: None,
                reason: TextIndexSkipReason::BinaryOrNonUtf8,
            });
            continue;
        };
        rows.push(TextIndexRow {
            path: node.path.clone(),
            reason: TextIndexSkipReason::Indexed,
            text: Some(IndexedFileText {
                path: node.path.clone(),
                content_hash: node.content_hash.clone(),
                byte_count: content.len(),
                line_count: content.lines().count(),
                content,
            }),
        });
    }
    Ok(rows)
}

/// Load indexed file hashes for incremental refresh comparison.
pub(crate) fn indexed_file_hashes(store: &AtlasStore) -> Result<HashMap<String, String>, CliError> {
    Ok(store
        .load_nodes()?
        .into_iter()
        .filter(|node| node.node.kind == NodeKind::File)
        .filter_map(|node| node.node.content_hash.map(|hash| (node.node.path, hash)))
        .collect::<HashMap<_, _>>())
}

/// Load indexed file hashes for selected repository paths.
pub(crate) fn indexed_file_hashes_for_paths(
    store: &AtlasStore,
    paths: &HashSet<String>,
) -> Result<HashMap<String, String>, CliError> {
    let mut sorted_paths = paths.iter().cloned().collect::<Vec<_>>();
    sorted_paths.sort();
    Ok(store
        .load_nodes_by_paths(&sorted_paths)?
        .into_iter()
        .filter(|node| node.node.kind == NodeKind::File)
        .filter_map(|node| node.node.content_hash.map(|hash| (node.node.path, hash)))
        .collect::<HashMap<_, _>>())
}

/// Return event paths in deterministic order.
pub(crate) fn sorted_watch_paths(paths: &HashSet<PathBuf>) -> Vec<PathBuf> {
    let mut paths = paths.iter().cloned().collect::<Vec<_>>();
    paths.sort();
    paths
}

/// Normalize a deleted path if it belongs to the watched repository.
pub(crate) fn normalized_deleted_path(
    root: &Path,
    path: &Path,
) -> Result<Option<String>, CliError> {
    match normalize_repo_path(root, path) {
        Ok(path) => Ok(Some(path)),
        Err(projectatlas_core::CoreError::PathOutsideRoot { .. }) => Ok(None),
        Err(source) => Err(CliError::InvalidInput(source.to_string())),
    }
}

/// Inspect and optionally remove legacy `.purpose` files.
pub(crate) fn strip_legacy_purpose(
    root: &Path,
    config_path: Option<&Path>,
    apply: bool,
    dry_run: bool,
    strip_source_headers: bool,
) -> Result<LegacyPurposeReport, CliError> {
    let root = root.canonicalize().map_err(|source| CliError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    let scan_options = scan_options_for_root(config_path, &root)?;
    let nodes = scan_repo(&root, &scan_options)?;
    let effective_dry_run = dry_run || !apply;
    let purpose_files = indexed_purpose_files(&root, &nodes);
    let mut removed = 0;
    if !effective_dry_run {
        for path in &purpose_files {
            let native = root.join(repo_path_to_native(path));
            fs::remove_file(&native).map_err(|source| CliError::Io {
                path: native,
                source,
            })?;
            removed += 1;
        }
    }
    let source_header_candidates = if strip_source_headers {
        purpose_header_candidates(&root, &nodes)?
    } else {
        Vec::new()
    };
    Ok(LegacyPurposeReport {
        applied: !effective_dry_run,
        purpose_files_found: purpose_files.len(),
        purpose_files_removed: removed,
        source_header_candidates,
        purpose_files,
    })
}

/// Collect `.purpose` files only from folders included in the normal index.
pub(crate) fn indexed_purpose_files(root: &Path, nodes: &[Node]) -> Vec<String> {
    let mut purpose_files = Vec::new();
    for node in nodes.iter().filter(|node| node.kind == NodeKind::Folder) {
        let relative = if node.path == "." {
            ".purpose".to_string()
        } else {
            format!("{}/.purpose", node.path)
        };
        let native = root.join(repo_path_to_native(&relative));
        if native.exists() {
            purpose_files.push(relative);
        }
    }
    purpose_files.sort();
    purpose_files
}

/// Return source files that appear to start with legacy Purpose headers.
pub(crate) fn purpose_header_candidates(
    root: &Path,
    nodes: &[Node],
) -> Result<Vec<String>, CliError> {
    let mut candidates = Vec::new();
    for node in nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .filter(|node| is_symbol_candidate(&node.path, node.language.as_deref()))
    {
        let path = root.join(repo_path_to_native(&node.path));
        let content = fs::read_to_string(&path).map_err(|source| CliError::Io { path, source })?;
        if content
            .lines()
            .take(3)
            .any(|line| line.trim_start().contains("Purpose:"))
        {
            candidates.push(node.path.clone());
        }
    }
    Ok(candidates)
}
