//! Purpose: Provide the `ProjectAtlas` 3 command-line adapter.

mod atlas_map;
#[cfg(test)]
mod language_capability_registry_tests;
mod mcp;
#[cfg(test)]
mod optional_pack_candidate_readiness_tests;
#[cfg(test)]
mod relation_traceability_contract_tests;
#[cfg(test)]
mod repository_intelligence_contract_tests;
#[cfg(test)]
mod repository_intelligence_scoreboard_tests;
#[cfg(test)]
mod repository_ownership_contract_tests;
mod runtime;
mod structural;
#[cfg(test)]
mod task_evidence_plan_tests;
mod token_tui;

use atlas_map::{
    IgnoreEntryKind, LintOptions, add_ignore_entry, effective_config_report, init_gitignore,
    init_project_with_config, lint_map, list_ignore_entries, load_atlas_config,
    remove_ignore_entry, write_map,
};
use clap::{Parser, Subcommand, ValueEnum};
use projectatlas_core::health::Severity;
use projectatlas_core::outline::build_outline;
use projectatlas_core::telemetry::{
    TokenCalibrationOverview, TokenTrendWindow as CoreTokenTrendWindow,
};
use projectatlas_core::toon::{
    encode_agent_payload, render_outline, render_overview, render_ranked_node_rows,
    render_ranked_nodes, render_symbol_relations, render_symbols, render_token_overview,
    render_token_trends,
};
use projectatlas_core::{
    PurposeSource, PurposeStatus, normalize_native_path_display, normalize_repo_path_prefix,
};
use projectatlas_db::{AtlasStore, DbError, HealthQuery, HealthResolution, HealthScope};
use projectatlas_service::{
    CodeSlice, FileSummaryReport, SearchReport, SymbolSliceSelector, build_file_summary,
    read_indexed_code_slice, read_symbol_slice, search_indexed_files,
};
use runtime::{
    DEFAULT_HEALTH_LIMIT, InitBootstrapOptions, InitHostConfigStatus, InitSetupReport,
    MAX_HEALTH_LIMIT, MAX_SYMBOL_FILE_BYTES, PurposeLintLevel, PurposeReviewRequest,
    ScanRuntimePlan, SettingsReport, SymbolBuildOptions, WatchStatusReport, absolute_path,
    build_settings_report, build_symbols_for_index, byte_count_to_tokens, canonical_project_root,
    default_mcp_project_root, defaultable_cli_project_root,
    estimated_source_tokens_for_indexed_files, estimated_source_tokens_for_paths,
    file_summary_usage_baseline, init_config_path, init_path_status, lint_database_if_present,
    next_step_report, next_step_report_payload, normalized_folder_filter, open_atlas_store,
    purpose_curation_page, ranked_file_nodes_with_reasons, ranked_folder_nodes_with_reasons,
    read_indexed_file_content, record_directory_walk_usage_estimate, record_usage_estimate,
    record_usage_text, render_health_page, render_purpose_curation_page,
    render_purpose_review_report, reset_index_files, resolved_mcp_config_path, review_purposes,
    run_init_bootstrap, run_scan_pipeline, run_watch_loop, strip_legacy_purpose,
    validated_indexed_file_key, watcher_status_report,
};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
#[cfg(test)]
use token_tui::render_token_dashboard;
use token_tui::{
    TokenDashboardTheme, render_token_dashboard_with_theme, render_token_trend_dashboard_with_theme,
};

/// Default relative path for the `SQLite` index.
const DEFAULT_DB_PATH: &str = ".projectatlas/projectatlas.db";
/// `ProjectAtlas` major architecture version.
const PROJECTATLAS_MAJOR_VERSION: u8 = 3;
/// Default session identifier for token telemetry.
const DEFAULT_SESSION_ID: &str = "default";
/// Default maximum rows returned per structured file-summary section.
const DEFAULT_FILE_SUMMARY_LIMIT: usize = 25;
/// One-shot watcher refresh mode.
const WATCH_MODE_ONCE: &str = "single-refresh";
/// Event-backed watcher mode.
const WATCH_MODE_NOTIFY: &str = "notify";
/// Portable fallback watcher mode.
const WATCH_MODE_POLLING: &str = "portable-polling";
/// Default parity profile for repository-intelligence checks.
pub(crate) const REPOSITORY_INTELLIGENCE_PROFILE: &str = "repository-intelligence";
/// CLI command families required for the agent-first repository-intelligence surface.
const REQUIRED_CLI_COMMANDS: &[RequiredCliCommand] = &[
    RequiredCliCommand::Init,
    RequiredCliCommand::Map,
    RequiredCliCommand::Scan,
    RequiredCliCommand::Overview,
    RequiredCliCommand::Folders,
    RequiredCliCommand::Files,
    RequiredCliCommand::Next,
    RequiredCliCommand::Outline,
    RequiredCliCommand::Summary,
    RequiredCliCommand::Search,
    RequiredCliCommand::Slice,
    RequiredCliCommand::Symbols,
    RequiredCliCommand::Settings,
    RequiredCliCommand::Root,
    RequiredCliCommand::Config,
    RequiredCliCommand::Ignore,
    RequiredCliCommand::WatchStatus,
    RequiredCliCommand::Watch,
    RequiredCliCommand::HealthCheck,
    RequiredCliCommand::Health,
    RequiredCliCommand::Lint,
    RequiredCliCommand::Token,
    RequiredCliCommand::Parity,
    RequiredCliCommand::StripLegacyPurpose,
    RequiredCliCommand::ResetIndex,
    RequiredCliCommand::Mcp,
    RequiredCliCommand::McpConfig,
    RequiredCliCommand::RuntimeInfo,
    RequiredCliCommand::Purpose,
];

/// Error type for CLI boundary failures.
#[derive(Debug, Error)]
enum CliError {
    /// Database operation failed.
    #[error("{0}")]
    Db(#[from] DbError),
    /// Shared service operation failed.
    #[error("{0}")]
    Service(#[from] projectatlas_service::ServiceError),
    /// Filesystem scanner operation failed.
    #[error("{0}")]
    Fs(#[from] projectatlas_fs::FsError),
    /// File or directory operation failed.
    #[error("io error for {path:?}: {source}")]
    Io {
        /// Path involved in the IO failure.
        path: PathBuf,
        /// Source IO error.
        source: std::io::Error,
    },
    /// Output stream write failed.
    #[error("output write failed: {0}")]
    Output(#[from] io::Error),
    /// JSON serialization failed.
    #[error("json serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    /// MCP runtime failed.
    #[error("mcp server failed: {0}")]
    Mcp(String),
    /// Watcher runtime failed.
    #[error("watcher failed: {0}")]
    Watcher(String),
    /// Repository inputs changed while a full scan was staging.
    #[error("full-scan inputs changed before publication: {detail}")]
    ScanInputsChanged {
        /// First detected input difference.
        detail: String,
    },
    /// Atlas map operation failed.
    #[error("{0}")]
    AtlasMap(#[from] atlas_map::AtlasMapError),
    /// User input was invalid.
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

/// CLI output serialization format.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    /// Token-efficient object notation for agent-facing responses.
    Toon,
    /// Pretty JSON for scripts and external machine consumers.
    Json,
}

/// Token report presentation mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum TokenView {
    /// Structured agent/script output controlled by the global format flag.
    Agent,
    /// Human terminal dashboard with a compact savings diagram.
    Tui,
}

/// Token TUI color theme.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum TokenTheme {
    /// Dark reference dashboard theme.
    Dark,
    /// Light dashboard theme for light terminal backgrounds.
    Light,
}

impl From<TokenTheme> for TokenDashboardTheme {
    fn from(theme: TokenTheme) -> Self {
        match theme {
            TokenTheme::Dark => Self::Dark,
            TokenTheme::Light => Self::Light,
        }
    }
}

/// Token trend grouping window.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum TokenTrendWindow {
    /// Group token telemetry by day.
    Day,
    /// Group token telemetry by week.
    Week,
    /// Group token telemetry by month.
    Month,
    /// Group token telemetry by year.
    Year,
}

impl From<TokenTrendWindow> for CoreTokenTrendWindow {
    fn from(window: TokenTrendWindow) -> Self {
        match window {
            TokenTrendWindow::Day => Self::Day,
            TokenTrendWindow::Week => Self::Week,
            TokenTrendWindow::Month => Self::Month,
            TokenTrendWindow::Year => Self::Year,
        }
    }
}

/// Health severity filter accepted by CLI commands.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum HealthSeverityArg {
    /// Informational finding.
    Info,
    /// Warning finding.
    Warning,
    /// Error finding.
    Error,
}

impl From<HealthSeverityArg> for Severity {
    fn from(value: HealthSeverityArg) -> Self {
        match value {
            HealthSeverityArg::Info => Self::Info,
            HealthSeverityArg::Warning => Self::Warning,
            HealthSeverityArg::Error => Self::Error,
        }
    }
}

/// Purpose curation strictness accepted by `projectatlas lint`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum PurposeLintLevelArg {
    /// Require agent review for folders and high-impact files only.
    Low,
    /// Also require agent review for source files.
    Medium,
    /// Require agent review for every indexed file and folder.
    Strict,
}

impl From<PurposeLintLevelArg> for PurposeLintLevel {
    fn from(value: PurposeLintLevelArg) -> Self {
        match value {
            PurposeLintLevelArg::Low => Self::Low,
            PurposeLintLevelArg::Medium => Self::Medium,
            PurposeLintLevelArg::Strict => Self::Strict,
        }
    }
}

/// Manual `ProjectAtlas` ignore entry kind for CLI input.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum IgnoreKind {
    /// Ignore every directory with this name anywhere under the repository.
    DirName,
    /// Ignore one repository-relative path subtree.
    PathPrefix,
}

/// MCP host configuration format.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
enum HarnessConfig {
    /// Standard `.mcp.json` shape with `mcpServers`.
    McpJson,
    /// Codex-compatible `.mcp.json` shape with a project-root `cwd` hint.
    Codex,
    /// Claude Code plugin/project MCP config shape.
    ClaudeCode,
    /// `OpenCode` `opencode.json` MCP config shape.
    #[value(name = "opencode")]
    OpenCode,
}

impl From<IgnoreKind> for IgnoreEntryKind {
    fn from(value: IgnoreKind) -> Self {
        match value {
            IgnoreKind::DirName => Self::DirName,
            IgnoreKind::PathPrefix => Self::PathPrefix,
        }
    }
}

/// Top-level parsed CLI arguments.
#[derive(Debug, Parser)]
#[command(name = "projectatlas")]
#[command(about = "ProjectAtlas 3 repository intelligence engine")]
#[command(version)]
struct Cli {
    /// Path to the `SQLite` index file.
    #[arg(long, default_value = DEFAULT_DB_PATH)]
    db: PathBuf,
    /// Response format to emit.
    #[arg(long, value_enum, default_value_t = OutputFormat::Toon)]
    format: OutputFormat,
    /// Session id used when recording token telemetry.
    #[arg(long, default_value = DEFAULT_SESSION_ID)]
    session: String,
    /// Path to `ProjectAtlas` config.toml for map/lint/init workflows.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Require this exact runtime version before executing the selected command.
    #[arg(long)]
    require_version: Option<String>,
    /// Subcommand to execute.
    #[command(subcommand)]
    command: Command,
}

/// Supported `ProjectAtlas` CLI commands.
#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize `ProjectAtlas` files in a repository.
    Init {
        /// Create/verify the project surface without running the scan/index pipeline.
        #[arg(long)]
        no_scan: bool,
        /// Run the scan/index phase even when a future freshness check could skip it.
        #[arg(long)]
        force_rescan: bool,
        /// Maximum UTF-8 file size persisted into `SQLite` text search during the init scan.
        #[arg(long)]
        text_index_max_bytes: Option<u64>,
    },
    /// Generate the `ProjectAtlas` TOON map.
    Map {
        /// Also write JSON next to the TOON map.
        #[arg(long)]
        json: bool,
        /// Run map generation even when CI environment variables are present.
        #[arg(long)]
        force: bool,
    },
    /// Scan a repository and replace the durable index.
    Scan {
        /// Repository root to scan.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Maximum UTF-8 file size persisted into `SQLite` text search.
        #[arg(long)]
        text_index_max_bytes: Option<u64>,
    },
    /// Print a repository overview.
    Overview,
    /// Rank folders before inspecting files.
    Folders {
        /// Search query for path and purpose matching.
        query: String,
        /// Maximum number of folders to return.
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Rank files, optionally inside an already-selected folder.
    Files {
        /// Search query for path and purpose matching.
        query: Option<String>,
        /// Folder path to constrain the search.
        #[arg(long)]
        folder: Option<String>,
        /// Optional repository-relative glob filter.
        #[arg(long)]
        file_pattern: Option<String>,
        /// Include indexed file text as a bounded fallback ranking signal.
        #[arg(long, default_value_t = false)]
        include_content: bool,
        /// Maximum number of files to return.
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Recommend the next indexed folders, files, and inspection commands.
    Next {
        /// Task or navigation query.
        query: String,
        /// Maximum number of folders and files to return.
        #[arg(long, default_value_t = 3)]
        limit: usize,
    },
    /// Build a compact outline for a chosen file.
    Outline {
        /// File path to outline.
        file: PathBuf,
        /// Number of non-empty preview lines to include.
        #[arg(long, default_value_t = 12)]
        lines: usize,
    },
    /// Return structured deterministic file intelligence from the deep index.
    Summary {
        /// Repository-relative file path to summarize.
        file: PathBuf,
        /// Maximum rows per functions/methods/classes/types/calls section.
        #[arg(long, default_value_t = DEFAULT_FILE_SUMMARY_LIMIT)]
        limit: usize,
    },
    /// Search indexed files with literal, regex, or fuzzy matching.
    Search {
        /// Literal, regex, or fuzzy pattern to search for.
        pattern: String,
        /// Treat the pattern as a regex.
        #[arg(long, conflicts_with = "fuzzy")]
        regex: bool,
        /// Treat the pattern as a fuzzy subsequence.
        #[arg(long, conflicts_with = "regex")]
        fuzzy: bool,
        /// Match case-sensitively.
        #[arg(long)]
        case_sensitive: bool,
        /// Optional repository-relative glob filter.
        #[arg(long)]
        file_pattern: Option<String>,
        /// Number of context lines before and after a match.
        #[arg(long, default_value_t = 0)]
        context_lines: usize,
        /// Pagination start index.
        #[arg(long, default_value_t = 0)]
        start_index: usize,
        /// Maximum matches to return.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Return an exact source line slice after a file has been selected.
    Slice {
        /// File path to slice.
        file: PathBuf,
        /// One-based start line.
        #[arg(long)]
        start_line: Option<usize>,
        /// Optional one-based end line.
        #[arg(long)]
        end_line: Option<usize>,
        /// Slice a symbol by name instead of passing line numbers.
        #[arg(long)]
        symbol: Option<String>,
        /// Optional parent symbol for disambiguating `--symbol`.
        #[arg(long)]
        symbol_parent: Option<String>,
        /// Optional symbol kind for disambiguating `--symbol`.
        #[arg(long)]
        symbol_kind: Option<String>,
        /// Optional source line for disambiguating `--symbol`.
        #[arg(long)]
        symbol_line: Option<usize>,
    },
    /// Inspect and rebuild the `ProjectAtlas` symbol graph.
    Symbols {
        /// Symbol graph subcommand to run.
        #[command(subcommand)]
        command: SymbolsCommand,
    },
    /// Print local `ProjectAtlas` settings and cache/index locations.
    Settings,
    /// Show, verify, or bind the project-local root.
    Root {
        /// Root subcommand to run.
        #[command(subcommand)]
        command: Option<RootCommand>,
    },
    /// Print the effective `ProjectAtlas` configuration.
    Config {
        /// Print the normalized configuration used by scan, map, lint, and watch.
        #[arg(long)]
        print: bool,
    },
    /// Manage the manual `ProjectAtlas` ignore layer in config.
    Ignore {
        /// Ignore subcommand to run.
        #[command(subcommand)]
        command: IgnoreCommand,
    },
    /// Print watcher availability and current status.
    WatchStatus,
    /// Watch a repository and refresh the index when files change.
    Watch {
        /// Repository root to watch.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Run one refresh pass and exit.
        #[arg(long)]
        once: bool,
        /// Debounce interval in seconds for event mode and poll interval for fallback mode.
        #[arg(long, default_value_t = 2)]
        poll_seconds: u64,
        /// Maximum refresh cycles before exiting. Zero means no limit.
        #[arg(long, default_value_t = 0)]
        max_cycles: usize,
        /// Maximum parser worker threads during refresh.
        #[arg(long)]
        max_workers: Option<usize>,
        /// Stop starting parser work after this many seconds during refresh.
        #[arg(long)]
        timeout_seconds: Option<u64>,
        /// Maximum UTF-8 file size persisted into `SQLite` text search.
        #[arg(long)]
        text_index_max_bytes: Option<u64>,
    },
    /// Report structural health findings.
    HealthCheck {
        /// Pagination start index after filters are applied.
        #[arg(long, default_value_t = 0)]
        start_index: usize,
        /// Maximum findings to return.
        #[arg(long, default_value_t = DEFAULT_HEALTH_LIMIT)]
        limit: usize,
        /// Optional finding category filter.
        #[arg(long)]
        category: Option<String>,
        /// Optional severity filter.
        #[arg(long, value_enum)]
        severity: Option<HealthSeverityArg>,
        /// Optional repository-relative primary or related path prefix.
        #[arg(long)]
        path_prefix: Option<String>,
        /// Return counts and paging metadata without finding rows.
        #[arg(long)]
        summary_only: bool,
        /// Restrict findings to source files and folders that contain source files.
        #[arg(long)]
        source_only: bool,
    },
    /// Resolve a deterministic health finding with agent rationale.
    Health {
        /// Health subcommand to run.
        #[command(subcommand)]
        command: HealthCommand,
    },
    /// Validate database purpose metadata, untracked files, and structure drift.
    Lint {
        /// Deprecated compatibility flag; database folder purpose linting uses `--purpose-level`.
        #[arg(long)]
        strict_folders: bool,
        /// Purpose curation strictness for `SQLite` health linting.
        #[arg(long, value_enum, default_value_t = PurposeLintLevelArg::Low)]
        purpose_level: PurposeLintLevelArg,
        /// Report non-source files not covered by source scanning.
        #[arg(long)]
        report_untracked: bool,
        /// Fail when disallowed untracked files exist.
        #[arg(long)]
        strict_untracked: bool,
    },
    /// Print estimated token savings for recorded funnel usage.
    Token {
        /// Optional session id filter.
        #[arg(long)]
        session: Option<String>,
        /// Presentation mode for the token report.
        #[arg(long, value_enum, default_value_t = TokenView::Agent)]
        view: TokenView,
        /// Optional trend grouping window.
        #[arg(long, value_enum)]
        trend: Option<TokenTrendWindow>,
        /// Optional local tokenizer calibration for indexed UTF-8 files.
        #[arg(long, value_parser = ["o200k_base", "cl100k_base"])]
        tokenizer: Option<String>,
        /// Color theme for the human terminal dashboard.
        #[arg(long, value_enum, default_value_t = TokenTheme::Dark)]
        theme: TokenTheme,
    },
    /// Check repository-intelligence parity readiness.
    Parity {
        /// Parity subcommand to run.
        #[command(subcommand)]
        command: Option<ParityCommand>,
        /// Parity profile to evaluate when omitting the `report` subcommand.
        #[arg(long, default_value = REPOSITORY_INTELLIGENCE_PROFILE)]
        profile: String,
    },
    /// Dry-run or apply cleanup of legacy `.purpose` metadata files.
    StripLegacyPurpose {
        /// Repository root to inspect.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Remove legacy `.purpose` files.
        #[arg(long, conflicts_with = "dry_run")]
        apply: bool,
        /// Preview cleanup without modifying files.
        #[arg(long)]
        dry_run: bool,
        /// Also report conservative source Purpose header candidates.
        #[arg(long)]
        strip_source_headers: bool,
    },
    /// Preview or clear local runtime index/cache files.
    ResetIndex {
        /// Remove runtime index/cache files. Without this flag the command previews only.
        #[arg(long, conflicts_with = "dry_run")]
        apply: bool,
        /// Preview cleanup without modifying files.
        #[arg(long)]
        dry_run: bool,
        /// Also remove generated project-local MCP config.
        #[arg(long)]
        include_mcp_config: bool,
    },
    /// Run the native `ProjectAtlas` MCP server over stdio.
    Mcp {
        /// Allow absolute path MCP calls to route to the nearest already-indexed `ProjectAtlas` root.
        #[arg(long)]
        nearest_project: bool,
    },
    /// Print a project-local MCP configuration with absolute runtime paths.
    McpConfig {
        /// MCP server name to emit.
        #[arg(long, default_value = "projectatlas")]
        server_name: String,
        /// Harness-specific config shape to emit.
        #[arg(long, value_enum, default_value_t = HarnessConfig::McpJson)]
        harness: HarnessConfig,
        /// Include `mcp --nearest-project` in the generated server startup args.
        #[arg(long)]
        nearest_project: bool,
    },
    /// Print structured runtime identity and capability information.
    RuntimeInfo,
    /// Manage purpose metadata stored in the durable index.
    Purpose {
        /// Purpose subcommand to run.
        #[command(subcommand)]
        command: PurposeCommand,
    },
}

/// Project root diagnostics and binding subcommands.
#[derive(Debug, Subcommand)]
enum RootCommand {
    /// Bind a repository root and regenerate project-local MCP configs.
    Set {
        /// Repository root to bind.
        path: PathBuf,
        /// Include `mcp --nearest-project` in generated project-local MCP configs.
        #[arg(long)]
        nearest_project: bool,
    },
    /// Show the root, DB, config, and runtime identity `ProjectAtlas` will use.
    Show,
    /// Verify DB/config/root identity agree.
    Verify,
}

/// Manual ignore management subcommands.
#[derive(Debug, Subcommand)]
enum IgnoreCommand {
    /// List effective `ProjectAtlas` manual ignore policy.
    List,
    /// Create a project-root `.gitignore` when it is missing.
    InitGitignore,
    /// Add one manual ignore entry to `.projectatlas/config.toml`.
    Add {
        /// Ignore kind to add.
        #[arg(long, value_enum)]
        kind: IgnoreKind,
        /// Directory name or repository-relative path prefix.
        value: String,
    },
    /// Remove one manual ignore entry from `.projectatlas/config.toml`.
    Remove {
        /// Ignore kind to remove. Omit to remove from both manual ignore lists.
        #[arg(long, value_enum)]
        kind: Option<IgnoreKind>,
        /// Directory name or repository-relative path prefix.
        value: String,
    },
}

/// Purpose metadata subcommands.
#[derive(Debug, Subcommand)]
enum PurposeCommand {
    /// Set an agent-approved purpose for an indexed path.
    Set {
        /// Indexed repository-relative path.
        path: String,
        /// Agent-approved purpose one-liner.
        purpose: String,
    },
    /// Batch review existing purpose records from a JSON file.
    Review {
        /// JSON file containing review items or an object with an `items` array.
        #[arg(long)]
        from_file: PathBuf,
        /// Apply reviewed purposes. Without this flag the command previews only.
        #[arg(long)]
        apply: bool,
    },
    /// Return a bounded queue of paths that need purpose curation.
    Queue {
        /// Pagination start index after filters are applied.
        #[arg(long, default_value_t = 0)]
        start_index: usize,
        /// Maximum findings to return.
        #[arg(long, default_value_t = DEFAULT_HEALTH_LIMIT)]
        limit: usize,
        /// Optional finding category filter.
        #[arg(long)]
        category: Option<String>,
        /// Optional severity filter.
        #[arg(long, value_enum)]
        severity: Option<HealthSeverityArg>,
        /// Optional repository-relative primary or related path prefix.
        #[arg(long)]
        path_prefix: Option<String>,
        /// Return counts and paging metadata without queue rows.
        #[arg(long)]
        summary_only: bool,
        /// Include non-source files and asset-only folders in the queue.
        #[arg(long)]
        include_assets: bool,
        /// Include low-priority files instead of the default folder-first queue.
        #[arg(long)]
        include_low_priority_files: bool,
    },
}

/// Parity gate subcommands.
#[derive(Debug, Subcommand)]
enum ParityCommand {
    /// Report whether the current index satisfies a parity profile.
    Report {
        /// Parity profile to evaluate.
        #[arg(long, default_value = REPOSITORY_INTELLIGENCE_PROFILE)]
        profile: String,
    },
}

/// Health metadata subcommands.
#[derive(Debug, Subcommand)]
enum HealthCommand {
    /// Mark a deterministic finding as resolved for this project.
    Resolve {
        /// Stable finding id from `projectatlas health-check`.
        finding_id: String,
        /// Finding category.
        category: String,
        /// Primary path.
        path: String,
        /// Optional related path.
        #[arg(long)]
        related_path: Option<String>,
        /// Agent rationale for resolving the finding.
        #[arg(long)]
        rationale: String,
    },
}

/// Symbol graph subcommands.
#[derive(Debug, Subcommand)]
enum SymbolsCommand {
    /// Rebuild symbols for indexed files.
    Build {
        /// Repository root used to read indexed files.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Maximum file size parsed for symbols.
        #[arg(long, default_value_t = MAX_SYMBOL_FILE_BYTES)]
        max_bytes: u64,
        /// Maximum parser worker threads. Defaults to Rayon automatic sizing.
        #[arg(long)]
        max_workers: Option<usize>,
        /// Stop starting parser work after this many seconds.
        #[arg(long)]
        timeout_seconds: Option<u64>,
    },
    /// List symbols by optional file and query.
    List {
        /// Optional repository-relative file path.
        #[arg(long)]
        file: Option<String>,
        /// Optional symbol or signature query.
        #[arg(long)]
        query: Option<String>,
        /// Maximum symbols to return.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// List symbol relations by optional file and query.
    Relations {
        /// Optional repository-relative file path.
        #[arg(long)]
        file: Option<String>,
        /// Optional source, target, or context query.
        #[arg(long)]
        query: Option<String>,
        /// Maximum relations to return.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Return an exact source slice for a named symbol.
    Slice {
        /// Repository-relative file path.
        file: PathBuf,
        /// Symbol name to locate.
        symbol: String,
        /// Optional parent symbol for disambiguation.
        #[arg(long)]
        symbol_parent: Option<String>,
        /// Optional symbol kind for disambiguation.
        #[arg(long)]
        symbol_kind: Option<String>,
        /// Optional source line for disambiguation.
        #[arg(long)]
        symbol_line: Option<usize>,
    },
}

/// Parse arguments, execute the command, and convert failures to process exit.
fn main() {
    if let Err(error) = run() {
        if write_stderr(&format!("error: {error}\n")).is_err() {
            std::process::exit(1);
        }
        std::process::exit(1);
    }
}

/// Execute the selected CLI command.
fn run() -> Result<(), CliError> {
    let cli = Cli::parse();
    if let Some(required_version) = cli.require_version.as_deref() {
        validate_required_runtime_version(required_version)?;
    }
    match &cli.command {
        Command::Init {
            no_scan,
            force_rescan,
            text_index_max_bytes,
        } => {
            let root = std::env::current_dir().map_err(|source| CliError::Io {
                path: PathBuf::from("."),
                source,
            })?;
            let db_path = absolute_path(&cli.db)?;
            let config_path = init_config_path(&root, cli.config.as_deref());
            let mut report = run_init_bootstrap(
                &root,
                &db_path,
                Some(&config_path),
                &InitBootstrapOptions {
                    no_scan: *no_scan,
                    force_rescan: *force_rescan,
                    text_index_max_bytes: *text_index_max_bytes,
                },
            )?;
            write_init_mcp_config_files(
                &mut report,
                &root.join(".projectatlas"),
                &db_path,
                &config_path,
                false,
            );
            print_output(
                cli.format,
                &encode_agent_payload(&json!({ "init": report })),
                &report,
            )?;
            if !report.ok {
                return Err(CliError::InvalidInput(
                    "projectatlas init completed with failed phase(s); see report".to_string(),
                ));
            }
        }
        Command::Map { json, force } => {
            if !force && (truthy_env("CI") || truthy_env("GITHUB_ACTIONS")) {
                write_stderr("Skipping ProjectAtlas map update in CI.\n")?;
                return Ok(());
            }
            let config = load_atlas_config(cli.config.as_deref())?;
            write_map(&config, *json)?;
        }
        Command::Scan {
            path,
            text_index_max_bytes,
        } => {
            let path = defaultable_cli_project_root(path, &cli.db, cli.config.as_deref())?;
            let plan =
                ScanRuntimePlan::for_path(cli.config.as_deref(), &path, *text_index_max_bytes)?;
            let symbol_options = SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None);
            let mut store = open_atlas_store(&cli.db)?;
            let report = run_scan_pipeline(&mut store, &cli.db, &plan, &symbol_options)?;
            print_output(
                cli.format,
                &encode_agent_payload(&json!({ "scan": report })),
                &report,
            )?;
        }
        Command::Overview => {
            let store = open_atlas_store(&cli.db)?;
            let overview = store.overview()?;
            let toon = render_overview(&overview);
            print_tracked_directory_output_estimate(
                cli.format,
                &store,
                &cli.session,
                "overview",
                None,
                None,
                estimated_source_tokens_for_indexed_files(&store, None, None)?,
                &toon,
                &overview,
            )?;
        }
        Command::Folders { query, limit } => {
            let store = open_atlas_store(&cli.db)?;
            let selected = ranked_folder_nodes_with_reasons(&store, query, *limit)?;
            let toon = render_ranked_nodes("folders", &selected);
            let payload = render_ranked_node_rows("folders", &selected);
            print_tracked_directory_output_estimate(
                cli.format,
                &store,
                &cli.session,
                "folders",
                None,
                Some(query.clone()),
                estimated_source_tokens_for_indexed_files(&store, None, None)?,
                &toon,
                &payload,
            )?;
        }
        Command::Files {
            query,
            folder,
            file_pattern,
            include_content,
            limit,
        } => {
            let store = open_atlas_store(&cli.db)?;
            let query_text = query.as_deref().unwrap_or("");
            let folder_filter = folder
                .as_deref()
                .map(normalized_folder_filter)
                .transpose()?;
            let baseline_tokens = estimated_source_tokens_for_indexed_files(
                &store,
                folder_filter.as_deref(),
                file_pattern.as_deref(),
            )?;
            let selected = ranked_file_nodes_with_reasons(
                &store,
                query_text,
                folder_filter.as_deref(),
                file_pattern.as_deref(),
                *limit,
                *include_content,
            )?;
            let toon = render_ranked_nodes("files", &selected);
            let payload = render_ranked_node_rows("files", &selected);
            print_tracked_output_estimate(
                cli.format,
                &store,
                &cli.session,
                "files",
                file_pattern.clone().or(folder_filter),
                query.clone(),
                baseline_tokens,
                &toon,
                &payload,
            )?;
        }
        Command::Next { query, limit } => {
            let store = open_atlas_store(&cli.db)?;
            let report = next_step_report(&store, query, Some(*limit))?;
            let payload = next_step_report_payload(&report);
            let toon = encode_agent_payload(&json!({ "next": payload }));
            print_tracked_directory_output_estimate(
                cli.format,
                &store,
                &cli.session,
                "next",
                None,
                Some(query.clone()),
                estimated_source_tokens_for_indexed_files(&store, None, None)?,
                &toon,
                &payload,
            )?;
        }
        Command::Outline { file, lines } => {
            let store = open_atlas_store(&cli.db)?;
            let file_key = validated_indexed_file_key(&store, file)?;
            let content = read_indexed_file_content(&store, &file_key)?;
            let language = store
                .load_node_by_path(&file_key)?
                .and_then(|node| node.node.language);
            let outline = build_outline(&file_key, language, &content, *lines);
            let toon = render_outline(&outline);
            print_tracked_output_text(
                cli.format,
                &store,
                &cli.session,
                "outline",
                Some(file_key),
                None,
                &content,
                &toon,
                &outline,
            )?;
        }
        Command::Summary { file, limit } => {
            let store = open_atlas_store(&cli.db)?;
            let report = build_file_summary(&store, file, *limit)?;
            let toon = render_file_summary(&report);
            print_tracked_output_text(
                cli.format,
                &store,
                &cli.session,
                "summary",
                Some(report.file_path.clone()),
                None,
                &file_summary_usage_baseline(&store, &report)?,
                &toon,
                &report,
            )?;
        }
        Command::Search {
            pattern,
            regex,
            fuzzy,
            case_sensitive,
            file_pattern,
            context_lines,
            start_index,
            limit,
        } => {
            let store = open_atlas_store(&cli.db)?;
            let report = search_indexed_files(
                &store,
                pattern,
                *regex,
                *fuzzy,
                *case_sensitive,
                file_pattern.as_deref(),
                *context_lines,
                *start_index,
                *limit,
            )?;
            let toon = render_search_report(&report);
            print_tracked_output_estimate(
                cli.format,
                &store,
                &cli.session,
                "search",
                file_pattern.clone(),
                Some(pattern.clone()),
                byte_count_to_tokens(report.searched_bytes),
                &toon,
                &report,
            )?;
        }
        Command::Slice {
            file,
            start_line,
            end_line,
            symbol,
            symbol_parent,
            symbol_kind,
            symbol_line,
        } => {
            let store = open_atlas_store(&cli.db)?;
            let report = if let Some(symbol) = symbol {
                read_symbol_slice(
                    &store,
                    file,
                    &SymbolSliceSelector {
                        name: symbol,
                        parent: symbol_parent.as_deref(),
                        kind: symbol_kind.as_deref(),
                        line: *symbol_line,
                    },
                )?
            } else {
                if symbol_parent.is_some() || symbol_kind.is_some() || symbol_line.is_some() {
                    return Err(CliError::InvalidInput(
                        "symbol disambiguators require --symbol".to_string(),
                    ));
                }
                let start_line = start_line.ok_or_else(|| {
                    CliError::InvalidInput(
                        "start-line is required unless --symbol is provided".to_string(),
                    )
                })?;
                read_indexed_code_slice(&store, file, start_line, *end_line)?
            };
            let toon = render_code_slice(&report);
            print_tracked_output_text(
                cli.format,
                &store,
                &cli.session,
                "slice",
                Some(report.path.clone()),
                None,
                &read_indexed_file_content(&store, &report.path)?,
                &toon,
                &report,
            )?;
        }
        Command::Symbols { command } => match command {
            SymbolsCommand::Build {
                path,
                max_bytes,
                max_workers,
                timeout_seconds,
            } => {
                let path = defaultable_cli_project_root(path, &cli.db, cli.config.as_deref())?;
                let mut store = open_atlas_store(&cli.db)?;
                let options = SymbolBuildOptions::new(*max_bytes, *max_workers, *timeout_seconds);
                let report = build_symbols_for_index(&mut store, &path, &options, None)?;
                print_output(
                    cli.format,
                    &encode_agent_payload(&json!({ "symbols_build": report })),
                    &report,
                )?;
            }
            SymbolsCommand::List { file, query, limit } => {
                let store = open_atlas_store(&cli.db)?;
                let symbols = store.load_symbols(file.as_deref(), query.as_deref(), *limit)?;
                let toon = render_symbols(&symbols);
                let baseline_tokens = estimated_source_tokens_for_paths(
                    &store,
                    symbols.iter().map(|symbol| symbol.path.as_str()),
                )?;
                print_tracked_output_estimate(
                    cli.format,
                    &store,
                    &cli.session,
                    "symbols",
                    file.clone(),
                    query.clone(),
                    baseline_tokens,
                    &toon,
                    &symbols,
                )?;
            }
            SymbolsCommand::Relations { file, query, limit } => {
                let store = open_atlas_store(&cli.db)?;
                let relations =
                    store.load_symbol_relations(file.as_deref(), query.as_deref(), *limit)?;
                let toon = render_symbol_relations(&relations);
                let baseline_tokens = estimated_source_tokens_for_paths(
                    &store,
                    relations.iter().map(|relation| relation.path.as_str()),
                )?;
                print_tracked_output_estimate(
                    cli.format,
                    &store,
                    &cli.session,
                    "symbol-relations",
                    file.clone(),
                    query.clone(),
                    baseline_tokens,
                    &toon,
                    &relations,
                )?;
            }
            SymbolsCommand::Slice {
                file,
                symbol,
                symbol_parent,
                symbol_kind,
                symbol_line,
            } => {
                let store = open_atlas_store(&cli.db)?;
                let report = read_symbol_slice(
                    &store,
                    file,
                    &SymbolSliceSelector {
                        name: symbol,
                        parent: symbol_parent.as_deref(),
                        kind: symbol_kind.as_deref(),
                        line: *symbol_line,
                    },
                )?;
                let toon = render_code_slice(&report);
                print_tracked_output_text(
                    cli.format,
                    &store,
                    &cli.session,
                    "symbol-slice",
                    Some(report.path.clone()),
                    Some(symbol.clone()),
                    &read_indexed_file_content(&store, &report.path)?,
                    &toon,
                    &report,
                )?;
            }
        },
        Command::Settings => {
            let report = build_settings_report(&cli.db, cli.config.as_deref(), cli.format)?;
            let toon = render_settings_report(&report);
            print_output(cli.format, &toon, &report)?;
        }
        Command::Root { command } => match command {
            Some(RootCommand::Set {
                path,
                nearest_project,
            }) => {
                let root = canonical_project_root(path)?;
                let report = bind_project_root(&root, *nearest_project)?;
                print_output(cli.format, &render_root_report(&report), &report)?;
            }
            None | Some(RootCommand::Show) => {
                let report = build_root_report(&cli.db, cli.config.as_deref())?;
                print_output(cli.format, &render_root_report(&report), &report)?;
            }
            Some(RootCommand::Verify) => {
                let report = build_root_report(&cli.db, cli.config.as_deref())?;
                let verified = report.verified;
                print_output(cli.format, &render_root_report(&report), &report)?;
                if !verified {
                    std::process::exit(1);
                }
            }
        },
        Command::Config { print: _ } => {
            let config = load_atlas_config(cli.config.as_deref())?;
            let report = effective_config_report(&config);
            print_output(
                cli.format,
                &encode_agent_payload(&json!({ "config": report })),
                &report,
            )?;
        }
        Command::Ignore { command } => match command {
            IgnoreCommand::List => {
                let project_root = default_mcp_project_root(&cli.db, cli.config.as_deref())?;
                let report = list_ignore_entries(cli.config.as_deref(), &project_root)?;
                print_output(
                    cli.format,
                    &encode_agent_payload(&json!({ "ignore": report })),
                    &report,
                )?;
            }
            IgnoreCommand::InitGitignore => {
                let project_root = default_mcp_project_root(&cli.db, cli.config.as_deref())?;
                let report = init_gitignore(cli.config.as_deref(), &project_root)?;
                print_output(
                    cli.format,
                    &encode_agent_payload(&json!({ "gitignore": report })),
                    &report,
                )?;
            }
            IgnoreCommand::Add { kind, value } => {
                let project_root = default_mcp_project_root(&cli.db, cli.config.as_deref())?;
                let report =
                    add_ignore_entry(cli.config.as_deref(), &project_root, (*kind).into(), value)?;
                print_output(
                    cli.format,
                    &encode_agent_payload(&json!({ "ignore": report })),
                    &report,
                )?;
            }
            IgnoreCommand::Remove { kind, value } => {
                let project_root = default_mcp_project_root(&cli.db, cli.config.as_deref())?;
                let report = remove_ignore_entry(
                    cli.config.as_deref(),
                    &project_root,
                    kind.map(Into::into),
                    value,
                )?;
                print_output(
                    cli.format,
                    &encode_agent_payload(&json!({ "ignore": report })),
                    &report,
                )?;
            }
        },
        Command::WatchStatus => {
            let report = watcher_status_report(false);
            let toon = render_watch_status(&report);
            print_output(cli.format, &toon, &report)?;
        }
        Command::Watch {
            path,
            once,
            poll_seconds,
            max_cycles,
            max_workers,
            timeout_seconds,
            text_index_max_bytes,
        } => {
            let path = defaultable_cli_project_root(path, &cli.db, cli.config.as_deref())?;
            let mut store = open_atlas_store(&cli.db)?;
            let plan =
                ScanRuntimePlan::for_path(cli.config.as_deref(), &path, *text_index_max_bytes)?;
            let symbol_options =
                SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, *max_workers, *timeout_seconds);
            let report = run_watch_loop(
                &mut store,
                &plan.root,
                *once,
                *poll_seconds,
                *max_cycles,
                &symbol_options,
                &plan.scan_options,
                plan.text_options,
            )?;
            print_output(
                cli.format,
                &encode_agent_payload(&json!({ "watch": report })),
                &report,
            )?;
        }
        Command::HealthCheck {
            start_index,
            limit,
            category,
            severity,
            path_prefix,
            summary_only,
            source_only,
        } => {
            let store = open_atlas_store(&cli.db)?;
            let query = health_query_from_cli(
                *start_index,
                *limit,
                category.as_deref(),
                *severity,
                path_prefix.as_deref(),
                *summary_only,
                if *source_only {
                    HealthScope::source_only()
                } else {
                    HealthScope::all()
                },
            );
            let page =
                store.unresolved_health_findings_page(&store.resolved_health_ids()?, &query)?;
            let toon = render_health_page(&page, &query);
            print_tracked_directory_output_estimate(
                cli.format,
                &store,
                &cli.session,
                "health-check",
                None,
                None,
                estimated_source_tokens_for_indexed_files(&store, None, None)?,
                &toon,
                &page,
            )?;
        }
        Command::Health { command } => match command {
            HealthCommand::Resolve {
                finding_id,
                category,
                path,
                related_path,
                rationale,
            } => {
                let store = open_atlas_store(&cli.db)?;
                let resolution = HealthResolution {
                    finding_id: finding_id.clone(),
                    category: category.clone(),
                    path: path.clone(),
                    related_path: related_path.clone(),
                    rationale: rationale.clone(),
                };
                store.resolve_health_finding(&resolution)?;
                print_output(
                    cli.format,
                    &encode_agent_payload(&json!({ "health_resolution": resolution })),
                    &resolution,
                )?;
            }
        },
        Command::Lint {
            strict_folders,
            purpose_level,
            report_untracked,
            strict_untracked,
        } => {
            let db_path = absolute_path(&cli.db)?;
            let config = load_atlas_config(cli.config.as_deref())?.with_db_path(db_path.clone());
            let (mut report, mut exit_code) = lint_map(
                &config,
                LintOptions {
                    strict_folders: *strict_folders,
                    report_untracked: *report_untracked,
                    strict_untracked: *strict_untracked,
                },
            )?;
            let (db_report, db_exit_code) =
                lint_database_if_present(&db_path, (*purpose_level).into())?;
            if !db_report.is_empty() {
                if !report.ends_with('\n') {
                    report.push('\n');
                }
                report.push_str(&db_report);
            }
            exit_code = exit_code.max(db_exit_code);
            write_stderr(&report)?;
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
        }
        Command::Token {
            session,
            view,
            trend,
            tokenizer,
            theme,
        } => {
            let store = open_atlas_store(&cli.db)?;
            if let Some(window) = trend {
                if tokenizer.is_some() {
                    return Err(CliError::InvalidInput(
                        "--tokenizer is only supported for token overview reports".to_string(),
                    ));
                }
                let report = store.token_trends(session.as_deref(), (*window).into())?;
                match view {
                    TokenView::Agent => {
                        print_output(cli.format, &render_token_trends(&report), &report)?;
                    }
                    TokenView::Tui => {
                        write_stdout(&render_token_trend_dashboard_with_theme(
                            &report,
                            (*theme).into(),
                        ))?;
                    }
                }
            } else {
                let mut overview = store.token_overview(session.as_deref())?;
                if let Some(tokenizer) = tokenizer.as_deref() {
                    overview.set_calibration(build_token_calibration(&store, tokenizer)?);
                }
                match view {
                    TokenView::Agent => {
                        print_output(cli.format, &render_token_overview(&overview), &overview)?;
                    }
                    TokenView::Tui => {
                        write_stdout(&render_token_dashboard_with_theme(
                            &overview,
                            session.as_deref(),
                            (*theme).into(),
                        ))?;
                    }
                }
            }
        }
        Command::Parity { command, profile } => {
            let profile = match command {
                Some(ParityCommand::Report { profile }) => profile,
                None => profile,
            };
            let store = open_atlas_store(&cli.db)?;
            let report = build_parity_report(&store, profile)?;
            let ok = report.ok;
            print_output(cli.format, &render_parity_report(&report), &report)?;
            if !ok {
                std::process::exit(1);
            }
        }
        Command::StripLegacyPurpose {
            path,
            apply,
            dry_run,
            strip_source_headers,
        } => {
            let path = defaultable_cli_project_root(path, &cli.db, cli.config.as_deref())?;
            let report = strip_legacy_purpose(
                &path,
                cli.config.as_deref(),
                *apply,
                *dry_run,
                *strip_source_headers,
            )?;
            print_output(
                cli.format,
                &encode_agent_payload(&json!({ "legacy_purpose_migration": report })),
                &report,
            )?;
        }
        Command::ResetIndex {
            apply,
            dry_run,
            include_mcp_config,
        } => {
            let report = reset_index_files(&cli.db, *apply, *dry_run, *include_mcp_config)?;
            print_output(
                cli.format,
                &encode_agent_payload(&json!({ "reset_index": report })),
                &report,
            )?;
        }
        Command::Mcp { nearest_project } => {
            mcp::run_mcp_server(
                cli.db.clone(),
                cli.config.clone(),
                cli.session.clone(),
                *nearest_project,
            )?;
        }
        Command::McpConfig {
            server_name,
            harness,
            nearest_project,
        } => {
            let report = build_harness_mcp_config_report(
                *harness,
                server_name,
                &cli.db,
                cli.config.as_deref(),
                *nearest_project,
            )?;
            print_output(cli.format, &render_mcp_config_report(&report), &report)?;
        }
        Command::RuntimeInfo => {
            let report = build_runtime_info();
            print_output(cli.format, &render_runtime_info(&report), &report)?;
        }
        Command::Purpose { command } => match command {
            PurposeCommand::Set { path, purpose } => {
                let store = open_atlas_store(&cli.db)?;
                store.set_purpose(path, purpose, PurposeSource::Agent)?;
                let report = PurposeSetReport {
                    purpose_set: PurposeSetPayload {
                        path: path.clone(),
                        status: PurposeStatus::Approved,
                        source: PurposeSource::Agent,
                        agent_reviewed: true,
                    },
                };
                print_output(cli.format, &encode_agent_payload(&report), &report)?;
            }
            PurposeCommand::Review { from_file, apply } => {
                let store = open_atlas_store(&cli.db)?;
                let requests = load_purpose_review_requests(from_file)?;
                let report = review_purposes(&store, &requests, *apply)?;
                print_output(cli.format, &render_purpose_review_report(&report), &report)?;
                if report.failed > 0 {
                    std::process::exit(1);
                }
            }
            PurposeCommand::Queue {
                start_index,
                limit,
                category,
                severity,
                path_prefix,
                summary_only,
                include_assets,
                include_low_priority_files,
            } => {
                let store = open_atlas_store(&cli.db)?;
                let query = health_query_from_cli(
                    *start_index,
                    *limit,
                    category.as_deref(),
                    *severity,
                    path_prefix.as_deref(),
                    *summary_only,
                    purpose_queue_scope(*include_assets, *include_low_priority_files),
                );
                let page = purpose_curation_page(&store, &query)?;
                print_output(cli.format, &render_purpose_curation_page(&page), &page)?;
            }
        },
    }
    Ok(())
}

/// Build a harness-specific MCP configuration document for this binary.
fn build_harness_mcp_config_report(
    harness: HarnessConfig,
    server_name: &str,
    db: &Path,
    config: Option<&Path>,
    nearest_project: bool,
) -> Result<serde_json::Value, CliError> {
    let config = build_mcp_config_report(server_name, db, config, nearest_project)?;
    Ok(match harness {
        HarnessConfig::McpJson | HarnessConfig::Codex => serde_json::to_value(config)?,
        HarnessConfig::ClaudeCode => {
            let mut mcp_servers = BTreeMap::new();
            for (name, server) in config.mcp_servers {
                mcp_servers.insert(
                    name,
                    ClaudeMcpServerConfig {
                        command: server.command,
                        args: server.args,
                    },
                );
            }
            serde_json::to_value(ClaudeMcpConfigDocument { mcp_servers })?
        }
        HarnessConfig::OpenCode => {
            let mut mcp = BTreeMap::new();
            for (name, server) in config.mcp_servers {
                let mut command = Vec::with_capacity(server.args.len() + 1);
                command.push(server.command);
                command.extend(server.args);
                mcp.insert(
                    name,
                    OpenCodeMcpServerConfig {
                        server_type: "local".to_string(),
                        command,
                        cwd: server.cwd,
                        enabled: true,
                    },
                );
            }
            serde_json::to_value(OpenCodeConfigDocument {
                schema: "https://opencode.ai/config.json".to_string(),
                mcp,
            })?
        }
    })
}

/// Build a standards-compliant MCP configuration document for this binary.
fn build_mcp_config_report(
    server_name: &str,
    db: &Path,
    config: Option<&Path>,
    nearest_project: bool,
) -> Result<McpConfigDocument, CliError> {
    let executable = std::env::current_exe().map_err(|source| CliError::Io {
        path: PathBuf::from("current executable"),
        source,
    })?;
    let absolute_db = absolute_path(db)?;
    let mut args = vec![
        "--require-version".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
        "--db".to_string(),
        mcp_launch_path(&absolute_db),
    ];
    let resolved_config = resolved_mcp_config_path(&absolute_db, config)?;
    if let Some(config_path) = resolved_config.as_ref() {
        args.push("--config".to_string());
        args.push(mcp_launch_path(config_path));
    }
    args.push("mcp".to_string());
    if nearest_project {
        args.push("--nearest-project".to_string());
    }
    let project_root = default_mcp_project_root(&absolute_db, resolved_config.as_deref())?;
    let mut mcp_servers = BTreeMap::new();
    mcp_servers.insert(
        server_name.to_string(),
        McpServerConfig {
            command: mcp_launch_path(&executable),
            args,
            cwd: mcp_launch_path(&project_root),
        },
    );
    Ok(McpConfigDocument { mcp_servers })
}

/// Validate a caller-provided runtime version guard.
fn validate_required_runtime_version(required_version: &str) -> Result<(), CliError> {
    let normalized = required_version.trim().trim_start_matches('v');
    let current = env!("CARGO_PKG_VERSION");
    if normalized == current {
        Ok(())
    } else {
        Err(CliError::InvalidInput(format!(
            "ProjectAtlas runtime version {current} does not satisfy required version {required_version}"
        )))
    }
}

/// Render a native path for MCP launch config without Windows extended prefixes.
fn mcp_launch_path(path: &Path) -> String {
    native_launch_path(&normalize_native_path_display(path))
}

/// Render a normalized diagnostic path as a Windows-native launcher path.
#[cfg(windows)]
fn native_launch_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("//") {
        format!(r"\\{}", rest.replace('/', "\\"))
    } else {
        path.replace('/', "\\")
    }
}

/// Return non-Windows paths unchanged.
#[cfg(not(windows))]
fn native_launch_path(path: &str) -> String {
    path.to_string()
}

/// Render MCP configuration as TOON for agents.
fn render_mcp_config_report(report: &serde_json::Value) -> String {
    encode_agent_payload(&json!({ "mcp_config": report }))
}

/// Build stable runtime identity and capability information.
fn build_runtime_info() -> RuntimeInfoReport {
    RuntimeInfoReport {
        project: "ProjectAtlas".to_string(),
        major_version: PROJECTATLAS_MAJOR_VERSION,
        version: env!("CARGO_PKG_VERSION").to_string(),
        executable: std::env::current_exe()
            .ok()
            .map(|path| normalize_native_path_display(&path)),
        repository: env!("CARGO_PKG_REPOSITORY").to_string(),
        capabilities: vec![
            "cli".to_string(),
            "mcp".to_string(),
            "sqlite".to_string(),
            "toon".to_string(),
            "symbol-index".to_string(),
            "text-search".to_string(),
            "watch".to_string(),
            "token-telemetry".to_string(),
        ],
        text_format: "TOON".to_string(),
        output_formats: vec!["toon".to_string(), "json".to_string()],
        mcp_tools: mcp::REQUIRED_MCP_TOOL_NAMES
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
    }
}

/// Render runtime information as compact TOON.
fn render_runtime_info(report: &RuntimeInfoReport) -> String {
    encode_agent_payload(&json!({ "runtime": report }))
}

/// Bind a project root without creating any machine-global root state.
fn bind_project_root(root: &Path, nearest_project: bool) -> Result<RootReport, CliError> {
    if !root.is_dir() {
        return Err(CliError::InvalidInput(format!(
            "project root {} is not a directory",
            root.display()
        )));
    }
    let atlas_dir = root.join(".projectatlas");
    let db_path = atlas_dir.join("projectatlas.db");
    init_project_with_config(root, None)?;
    let config_path = init_config_path(root, None);
    {
        let store = open_atlas_store(&db_path)?;
        store.set_project_root(root)?;
    }
    write_mcp_config_file(
        &atlas_dir.join("projectatlas.mcp.json"),
        HarnessConfig::McpJson,
        &db_path,
        &config_path,
        nearest_project,
    )?;
    write_mcp_config_file(
        &atlas_dir.join("projectatlas.claude.mcp.json"),
        HarnessConfig::ClaudeCode,
        &db_path,
        &config_path,
        nearest_project,
    )?;
    write_mcp_config_file(
        &atlas_dir.join("projectatlas.opencode.json"),
        HarnessConfig::OpenCode,
        &db_path,
        &config_path,
        nearest_project,
    )?;
    build_root_report(&db_path, Some(&config_path))
}

/// Write all generated host MCP configs expected after first-run init.
fn write_init_mcp_config_files(
    report: &mut InitSetupReport,
    atlas_dir: &Path,
    db_path: &Path,
    config_path: &Path,
    nearest_project: bool,
) {
    for (harness_name, file_name, harness) in [
        ("mcp_json", "projectatlas.mcp.json", HarnessConfig::McpJson),
        (
            "claude_code",
            "projectatlas.claude.mcp.json",
            HarnessConfig::ClaudeCode,
        ),
        (
            "opencode",
            "projectatlas.opencode.json",
            HarnessConfig::OpenCode,
        ),
    ] {
        let path = atlas_dir.join(file_name);
        let existed = path.exists();
        let (status, error) =
            match write_mcp_config_file(&path, harness, db_path, config_path, nearest_project) {
                Ok(()) => (init_path_status(existed), None),
                Err(error) => {
                    report.ok = false;
                    (runtime::InitPhaseStatus::Failed, Some(error.to_string()))
                }
            };
        report.host_configs.push(InitHostConfigStatus {
            harness: harness_name,
            status,
            path: normalize_native_path_display(path),
            error,
        });
    }
    if !report.ok {
        report
            .next_steps
            .push("Fix generated host MCP config errors and rerun projectatlas init.".to_string());
    }
}

/// Write one generated MCP config document as pretty JSON.
fn write_mcp_config_file(
    path: &Path,
    harness: HarnessConfig,
    db_path: &Path,
    config_path: &Path,
    nearest_project: bool,
) -> Result<(), CliError> {
    let value = build_harness_mcp_config_report(
        harness,
        "projectatlas",
        db_path,
        Some(config_path),
        nearest_project,
    )?;
    let text = format!("{}\n", serde_json::to_string_pretty(&value)?);
    fs::write(path, text).map_err(|source| CliError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Load batch purpose review requests from a JSON file.
fn load_purpose_review_requests(path: &Path) -> Result<Vec<PurposeReviewRequest>, CliError> {
    let text = fs::read_to_string(path).map_err(|source| CliError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let value: serde_json::Value = serde_json::from_str(&text)?;
    let items = value.get("items").cloned().unwrap_or(value);
    let requests: Vec<PurposeReviewRequest> = serde_json::from_value(items)?;
    if requests.is_empty() {
        return Err(CliError::InvalidInput(
            "purpose review input must contain at least one item".to_string(),
        ));
    }
    Ok(requests)
}

/// Build a project-local root identity report.
fn build_root_report(db: &Path, config_path: Option<&Path>) -> Result<RootReport, CliError> {
    let settings = build_settings_report(db, config_path, OutputFormat::Toon)?;
    let db_project_root = settings
        .index
        .as_ref()
        .and_then(|index| index.project_root.clone());
    let atlas_dir = Path::new(&settings.db.path)
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let runtime = build_runtime_info();
    Ok(RootReport {
        root: settings.repo_root.clone(),
        detection_source: settings.root_detection_source.clone(),
        db_path: settings.db.path.clone(),
        config_path: settings.config_path.clone(),
        config_project_root: settings
            .config_path
            .as_ref()
            .map(|_| settings.repo_root.clone()),
        db_project_root,
        mcp_config_path: settings.mcp_config.path.clone(),
        claude_mcp_config_path: normalize_native_path_display(
            atlas_dir.join("projectatlas.claude.mcp.json"),
        ),
        opencode_config_path: normalize_native_path_display(
            atlas_dir.join("projectatlas.opencode.json"),
        ),
        runtime_executable: runtime.executable,
        runtime_version: runtime.version,
        verified: settings.root_verified,
        mismatches: settings.root_mismatches,
    })
}

/// Return whether an environment variable is set to a truthy value.
fn truthy_env(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// Emit either TOON or JSON to stdout.
fn print_output<T: serde::Serialize>(
    format: OutputFormat,
    toon: &str,
    payload: &T,
) -> Result<(), CliError> {
    write_stdout(&serialized_output(format, toon, payload)?)
}

/// Serialize output exactly as the CLI will emit it.
fn serialized_output<T: serde::Serialize>(
    format: OutputFormat,
    toon: &str,
    payload: &T,
) -> Result<String, CliError> {
    match format {
        OutputFormat::Toon => Ok(toon.to_string()),
        OutputFormat::Json => Ok(format!("{}\n", serde_json::to_string_pretty(payload)?)),
    }
}

/// Build a bounded DB health query from CLI filter arguments.
fn health_query_from_cli(
    start_index: usize,
    limit: usize,
    category: Option<&str>,
    severity: Option<HealthSeverityArg>,
    path_prefix: Option<&str>,
    summary_only: bool,
    scope: HealthScope,
) -> HealthQuery {
    HealthQuery {
        start_index,
        limit: limit.clamp(1, MAX_HEALTH_LIMIT),
        category: trimmed_cli_filter(category),
        severity: severity.map(Severity::from),
        path_prefix: trimmed_cli_filter(path_prefix)
            .map(|value| normalize_repo_path_prefix(&value)),
        summary_only,
        scope,
    }
}

/// Return the DB scope for purpose queue CLI switches.
fn purpose_queue_scope(include_assets: bool, include_low_priority_files: bool) -> HealthScope {
    match (include_assets, include_low_priority_files) {
        (false, false) => HealthScope::purpose_default(),
        (true, false) => HealthScope::purpose_with_assets(),
        (false, true) => HealthScope::purpose_with_source_files(),
        (true, true) => HealthScope::all(),
    }
}

/// Return a trimmed non-empty CLI string filter.
fn trimmed_cli_filter(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

/// Record estimated-token telemetry for the exact emitted CLI payload.
fn print_tracked_directory_output_estimate<T: serde::Serialize>(
    format: OutputFormat,
    store: &AtlasStore,
    session: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    estimated_without_projectatlas: usize,
    toon: &str,
    payload: &T,
) -> Result<(), CliError> {
    let output = serialized_output(format, toon, payload)?;
    record_directory_walk_usage_estimate(
        store,
        session,
        command,
        path,
        query,
        estimated_without_projectatlas,
        &output,
    )?;
    write_stdout(&output)
}

/// Record candidate-set telemetry for the exact emitted CLI payload.
fn print_tracked_output_estimate<T: serde::Serialize>(
    format: OutputFormat,
    store: &AtlasStore,
    session: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    estimated_without_projectatlas: usize,
    toon: &str,
    payload: &T,
) -> Result<(), CliError> {
    let output = serialized_output(format, toon, payload)?;
    record_usage_estimate(
        store,
        session,
        command,
        path,
        query,
        estimated_without_projectatlas,
        &output,
    )?;
    write_stdout(&output)
}

/// Record baseline-text telemetry for the exact emitted CLI payload.
fn print_tracked_output_text<T: serde::Serialize>(
    format: OutputFormat,
    store: &AtlasStore,
    session: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    baseline_text: &str,
    toon: &str,
    payload: &T,
) -> Result<(), CliError> {
    let output = serialized_output(format, toon, payload)?;
    record_usage_text(store, session, command, path, query, baseline_text, &output)?;
    write_stdout(&output)
}

/// Agent-facing payload for a CLI purpose update.
#[derive(Debug, Serialize)]
struct PurposeSetReport {
    /// Purpose update result details.
    purpose_set: PurposeSetPayload,
}

/// Stable serialized schema for a CLI purpose update.
#[derive(Debug, Serialize)]
struct PurposeSetPayload {
    /// Indexed repository-relative path whose purpose was updated.
    path: String,
    /// Durable purpose status after the update.
    status: PurposeStatus,
    /// Source of the durable purpose after the update.
    source: PurposeSource,
    /// Whether the purpose has been agent-reviewed.
    agent_reviewed: bool,
}

/// Repository-intelligence parity report.
#[derive(Debug, Serialize)]
struct ParityReport {
    /// Evaluated parity profile.
    profile: String,
    /// Whether every required check passed.
    ok: bool,
    /// Current repository overview.
    overview: projectatlas_core::Overview,
    /// Files with persisted UTF-8 search text.
    indexed_text_files: usize,
    /// UTF-8 source bytes available through SQLite-backed search.
    indexed_text_bytes: usize,
    /// Persisted symbols.
    symbols: usize,
    /// Persisted symbol relations.
    relations: usize,
    /// Current unresolved health finding count.
    health_findings: usize,
    /// Token telemetry events counted for the active/default report.
    token_calls: usize,
    /// Runtime watcher mode detected in this process.
    watcher_mode: String,
    /// Required parity checks.
    checks: Vec<ParityCheck>,
}

/// Agent-facing parity payload wrapper.
#[derive(Debug, Serialize)]
struct ParityPayload<'a> {
    /// Repository-intelligence parity report.
    parity: &'a ParityReport,
}

/// One parity check row.
#[derive(Debug, Serialize)]
struct ParityCheck {
    /// Stable check name.
    name: String,
    /// Stable check status.
    status: ParityCheckStatus,
    /// Concrete evidence for this check.
    detail: String,
}

/// Stable status values for repository-intelligence parity checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum ParityCheckStatus {
    /// The parity check passed.
    Pass,
    /// The parity check failed.
    Fail,
}

impl ParityCheckStatus {
    /// Return a check status from a boolean predicate.
    fn from_passed(passed: bool) -> Self {
        if passed { Self::Pass } else { Self::Fail }
    }
}

/// Required CLI command families whose variants must remain constructible.
#[derive(Clone, Copy, Debug)]
enum RequiredCliCommand {
    /// `projectatlas init`.
    Init,
    /// `projectatlas map`.
    Map,
    /// `projectatlas scan`.
    Scan,
    /// `projectatlas overview`.
    Overview,
    /// `projectatlas folders`.
    Folders,
    /// `projectatlas files`.
    Files,
    /// `projectatlas next`.
    Next,
    /// `projectatlas outline`.
    Outline,
    /// `projectatlas summary`.
    Summary,
    /// `projectatlas search`.
    Search,
    /// `projectatlas slice`.
    Slice,
    /// `projectatlas symbols`.
    Symbols,
    /// `projectatlas settings`.
    Settings,
    /// `projectatlas root`.
    Root,
    /// `projectatlas config`.
    Config,
    /// `projectatlas ignore`.
    Ignore,
    /// `projectatlas watch-status`.
    WatchStatus,
    /// `projectatlas watch`.
    Watch,
    /// `projectatlas health-check`.
    HealthCheck,
    /// `projectatlas health`.
    Health,
    /// `projectatlas lint`.
    Lint,
    /// `projectatlas token`.
    Token,
    /// `projectatlas parity`.
    Parity,
    /// `projectatlas strip-legacy-purpose`.
    StripLegacyPurpose,
    /// `projectatlas reset-index`.
    ResetIndex,
    /// `projectatlas mcp`.
    Mcp,
    /// `projectatlas mcp-config`.
    McpConfig,
    /// `projectatlas runtime-info`.
    RuntimeInfo,
    /// `projectatlas purpose`.
    Purpose,
}

impl RequiredCliCommand {
    /// Stable command name used in reports and parity diagnostics.
    fn name(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Map => "map",
            Self::Scan => "scan",
            Self::Overview => "overview",
            Self::Folders => "folders",
            Self::Files => "files",
            Self::Next => "next",
            Self::Outline => "outline",
            Self::Summary => "summary",
            Self::Search => "search",
            Self::Slice => "slice",
            Self::Symbols => "symbols",
            Self::Settings => "settings",
            Self::Root => "root",
            Self::Config => "config",
            Self::Ignore => "ignore",
            Self::WatchStatus => "watch-status",
            Self::Watch => "watch",
            Self::HealthCheck => "health-check",
            Self::Health => "health",
            Self::Lint => "lint",
            Self::Token => "token",
            Self::Parity => "parity",
            Self::StripLegacyPurpose => "strip-legacy-purpose",
            Self::ResetIndex => "reset-index",
            Self::Mcp => "mcp",
            Self::McpConfig => "mcp-config",
            Self::RuntimeInfo => "runtime-info",
            Self::Purpose => "purpose",
        }
    }

    /// Construct the actual CLI enum variant so parity is tied to compiled command families.
    fn command(self) -> Command {
        match self {
            Self::Init => Command::Init {
                no_scan: true,
                force_rescan: false,
                text_index_max_bytes: None,
            },
            Self::Map => Command::Map {
                json: false,
                force: false,
            },
            Self::Scan => Command::Scan {
                path: PathBuf::from("."),
                text_index_max_bytes: None,
            },
            Self::Overview => Command::Overview,
            Self::Folders => Command::Folders {
                query: String::new(),
                limit: 1,
            },
            Self::Files => Command::Files {
                query: None,
                folder: None,
                file_pattern: None,
                include_content: false,
                limit: 1,
            },
            Self::Next => Command::Next {
                query: String::new(),
                limit: 1,
            },
            Self::Outline => Command::Outline {
                file: PathBuf::from("src/lib.rs"),
                lines: 1,
            },
            Self::Summary => Command::Summary {
                file: PathBuf::from("src/lib.rs"),
                limit: 1,
            },
            Self::Search => Command::Search {
                pattern: String::new(),
                regex: false,
                fuzzy: false,
                case_sensitive: false,
                file_pattern: None,
                context_lines: 0,
                start_index: 0,
                limit: 1,
            },
            Self::Slice => Command::Slice {
                file: PathBuf::from("src/lib.rs"),
                start_line: Some(1),
                end_line: None,
                symbol: None,
                symbol_parent: None,
                symbol_kind: None,
                symbol_line: None,
            },
            Self::Symbols => Command::Symbols {
                command: SymbolsCommand::List {
                    file: None,
                    query: None,
                    limit: 1,
                },
            },
            Self::Settings => Command::Settings,
            Self::Root => Command::Root {
                command: Some(RootCommand::Show),
            },
            Self::Config => Command::Config { print: true },
            Self::Ignore => Command::Ignore {
                command: IgnoreCommand::List,
            },
            Self::WatchStatus => Command::WatchStatus,
            Self::Watch => Command::Watch {
                path: PathBuf::from("."),
                once: true,
                poll_seconds: 1,
                max_cycles: 1,
                max_workers: None,
                timeout_seconds: None,
                text_index_max_bytes: None,
            },
            Self::HealthCheck => Command::HealthCheck {
                start_index: 0,
                limit: 1,
                category: None,
                severity: None,
                path_prefix: None,
                summary_only: true,
                source_only: false,
            },
            Self::Health => Command::Health {
                command: HealthCommand::Resolve {
                    finding_id: String::new(),
                    category: String::new(),
                    path: String::new(),
                    related_path: None,
                    rationale: String::new(),
                },
            },
            Self::Lint => Command::Lint {
                strict_folders: false,
                purpose_level: PurposeLintLevelArg::Low,
                report_untracked: false,
                strict_untracked: false,
            },
            Self::Token => Command::Token {
                session: None,
                view: TokenView::Agent,
                trend: None,
                tokenizer: None,
                theme: TokenTheme::Dark,
            },
            Self::Parity => Command::Parity {
                command: Some(ParityCommand::Report {
                    profile: REPOSITORY_INTELLIGENCE_PROFILE.to_string(),
                }),
                profile: REPOSITORY_INTELLIGENCE_PROFILE.to_string(),
            },
            Self::StripLegacyPurpose => Command::StripLegacyPurpose {
                path: PathBuf::from("."),
                apply: false,
                dry_run: true,
                strip_source_headers: false,
            },
            Self::ResetIndex => Command::ResetIndex {
                apply: false,
                dry_run: true,
                include_mcp_config: false,
            },
            Self::Mcp => Command::Mcp {
                nearest_project: false,
            },
            Self::McpConfig => Command::McpConfig {
                server_name: "projectatlas".to_string(),
                harness: HarnessConfig::McpJson,
                nearest_project: false,
            },
            Self::RuntimeInfo => Command::RuntimeInfo,
            Self::Purpose => Command::Purpose {
                command: PurposeCommand::Queue {
                    start_index: 0,
                    limit: 1,
                    category: None,
                    severity: None,
                    path_prefix: None,
                    summary_only: true,
                    include_assets: false,
                    include_low_priority_files: false,
                },
            },
        }
    }
}

/// `.mcp.json` compatible server configuration document.
#[derive(Debug, Serialize)]
struct McpConfigDocument {
    /// MCP server map keyed by server name.
    #[serde(rename = "mcpServers")]
    mcp_servers: BTreeMap<String, McpServerConfig>,
}

/// MCP server launch entry.
#[derive(Debug, Serialize)]
struct McpServerConfig {
    /// Absolute command path for the native `projectatlas` binary.
    command: String,
    /// Global CLI arguments followed by the `mcp` subcommand.
    args: Vec<String>,
    /// Project root working directory hint for MCP hosts that support it.
    cwd: String,
}

/// Claude Code MCP server launch entry.
#[derive(Debug, Serialize)]
struct ClaudeMcpServerConfig {
    /// Absolute command path for the native `projectatlas` binary.
    command: String,
    /// Global CLI arguments followed by the `mcp` subcommand.
    args: Vec<String>,
}

/// Claude Code `.mcp.json` compatible configuration document.
#[derive(Debug, Serialize)]
struct ClaudeMcpConfigDocument {
    /// MCP server map keyed by server name.
    #[serde(rename = "mcpServers")]
    mcp_servers: BTreeMap<String, ClaudeMcpServerConfig>,
}

/// `OpenCode` `opencode.json` compatible configuration document.
#[derive(Debug, Serialize)]
struct OpenCodeConfigDocument {
    /// `OpenCode` JSON schema URL.
    #[serde(rename = "$schema")]
    schema: String,
    /// MCP server map keyed by server name.
    mcp: BTreeMap<String, OpenCodeMcpServerConfig>,
}

/// `OpenCode` local MCP server launch entry.
#[derive(Debug, Serialize)]
struct OpenCodeMcpServerConfig {
    /// `OpenCode` local MCP type discriminator.
    #[serde(rename = "type")]
    server_type: String,
    /// Command array: executable followed by arguments.
    command: Vec<String>,
    /// Project root working directory.
    cwd: String,
    /// Whether the server is enabled by default.
    enabled: bool,
}

/// Stable runtime identity and capability report for installers.
#[derive(Debug, Serialize)]
struct RuntimeInfoReport {
    /// Product name.
    project: String,
    /// Major `ProjectAtlas` architecture version.
    major_version: u8,
    /// Cargo package version.
    version: String,
    /// Exact executable path for this runtime process, when available.
    executable: Option<String>,
    /// Repository URL embedded at build time.
    repository: String,
    /// Runtime capabilities available in this binary.
    capabilities: Vec<String>,
    /// Agent-facing payload format.
    text_format: String,
    /// Supported CLI output formats.
    output_formats: Vec<String>,
    /// Required MCP tool names compiled into the runtime.
    mcp_tools: Vec<String>,
}

/// Project-local root identity report.
#[derive(Debug, Serialize)]
struct RootReport {
    /// Canonical project root `ProjectAtlas` will use.
    root: String,
    /// Detection source for the selected root.
    detection_source: String,
    /// Durable `SQLite` database path.
    db_path: String,
    /// Config path used for project policy.
    config_path: Option<String>,
    /// Root stored in config, when config exists.
    config_project_root: Option<String>,
    /// Root stored in the DB metadata, when the DB exists.
    db_project_root: Option<String>,
    /// Generated generic MCP config path.
    mcp_config_path: String,
    /// Generated Claude Code MCP config path.
    claude_mcp_config_path: String,
    /// Generated `OpenCode` MCP config path.
    opencode_config_path: String,
    /// Current runtime executable path, when available.
    runtime_executable: Option<String>,
    /// Current runtime version.
    runtime_version: String,
    /// Whether config and DB roots agree with the selected root.
    verified: bool,
    /// Root mismatches that must be fixed before trusting the binding.
    mismatches: Vec<String>,
}

/// Render a search report as compact TOON.
fn render_search_report(report: &SearchReport) -> String {
    encode_agent_payload(&json!({ "search": report }))
}

/// Render repository-intelligence parity as compact TOON.
fn render_parity_report(report: &ParityReport) -> String {
    encode_agent_payload(&ParityPayload { parity: report })
}

/// Render a code slice as compact TOON.
fn render_code_slice(slice: &CodeSlice) -> String {
    encode_agent_payload(&json!({ "slice": slice }))
}

/// Render settings as compact TOON.
fn render_settings_report(report: &SettingsReport) -> String {
    encode_agent_payload(&json!({ "settings": report }))
}

/// Build an optional local tokenizer calibration over indexed UTF-8 files.
fn build_token_calibration(
    store: &AtlasStore,
    tokenizer: &str,
) -> Result<TokenCalibrationOverview, CliError> {
    let encoding = tiktoken::get_encoding(tokenizer).ok_or_else(|| {
        CliError::InvalidInput(format!(
            "unsupported tokenizer {tokenizer:?}; use o200k_base or cl100k_base"
        ))
    })?;
    let mut files = 0usize;
    let mut bytes = 0usize;
    let mut heuristic_tokens = 0usize;
    let mut calibrated_tokens = 0usize;
    store.visit_file_texts_for_search(None, false, |text| {
        files = files.saturating_add(1);
        bytes = bytes.saturating_add(text.byte_count);
        heuristic_tokens = heuristic_tokens.saturating_add(byte_count_to_tokens(text.byte_count));
        calibrated_tokens = calibrated_tokens.saturating_add(encoding.count(&text.content));
        Ok(true)
    })?;
    Ok(TokenCalibrationOverview {
        tokenizer: tokenizer.to_string(),
        provider: "local_tiktoken".to_string(),
        model: "tokenizer_calibration".to_string(),
        tokenizer_backend: tokenizer.to_string(),
        accuracy: "calibrated_local_tokenizer".to_string(),
        files,
        bytes,
        heuristic_tokens,
        calibrated_tokens,
        heuristic_to_calibrated_ratio: if calibrated_tokens == 0 {
            None
        } else {
            Some(heuristic_tokens as f64 / calibrated_tokens as f64)
        },
    })
}

/// Render root diagnostics as compact TOON.
fn render_root_report(report: &RootReport) -> String {
    encode_agent_payload(&json!({ "root": report }))
}

/// Render watcher status as compact TOON.
fn render_watch_status(report: &WatchStatusReport) -> String {
    encode_agent_payload(&json!({ "watch_status": report }))
}

/// Build the current repository-intelligence parity report.
fn build_parity_report(store: &AtlasStore, profile: &str) -> Result<ParityReport, CliError> {
    if profile != REPOSITORY_INTELLIGENCE_PROFILE {
        return Err(CliError::InvalidInput(format!(
            "unsupported parity profile {profile:?}"
        )));
    }
    let overview = store.overview()?;
    let file_count = overview.files;
    let indexed_text_files = store.file_text_count()?;
    let indexed_text_bytes = store.file_text_byte_count()?;
    let symbols = store.symbol_count()?;
    let relations = store.symbol_relation_count()?;
    let health_findings = store
        .unresolved_health_findings(&store.resolved_health_ids()?)?
        .len();
    let token_calls = store.token_overview(None)?.calls;
    let watcher_status = watcher_status_report(false);
    let watcher_mode = watcher_status.mode.clone();

    let mut checks = Vec::new();
    push_check(
        &mut checks,
        "profile-supported",
        true,
        "repository-intelligence profile is implemented",
    );
    push_check(
        &mut checks,
        "project-root",
        store.project_root()?.is_some(),
        "scan metadata records the canonical project root",
    );
    push_check(
        &mut checks,
        "structure-index",
        overview.files > 0 || overview.folders > 0,
        &format!(
            "{} files and {} folders indexed",
            overview.files, overview.folders
        ),
    );
    push_check(
        &mut checks,
        "purpose-health-surface",
        true,
        &format!(
            "{} missing, {} suggested, {} stale purposes visible through health and purpose queue",
            overview.missing_purposes, overview.suggested_purposes, overview.stale_purposes
        ),
    );
    push_check(
        &mut checks,
        "text-index",
        file_count == 0 || indexed_text_files > 0,
        &format!("{indexed_text_files}/{file_count} files have persisted UTF-8 search text"),
    );
    push_check(
        &mut checks,
        "symbol-index",
        file_count == 0 || symbols > 0,
        &format!("{symbols} symbols and {relations} relations persisted"),
    );
    push_check(
        &mut checks,
        "watcher-refresh",
        watcher_status.available,
        &format!(
            "watch-status probe reports mode {watcher_mode} and event backend available={}",
            watcher_status.event_backend_available
        ),
    );
    push_check(
        &mut checks,
        "health-surface",
        true,
        &format!("{health_findings} unresolved health findings currently visible"),
    );
    push_check(
        &mut checks,
        "token-telemetry",
        true,
        &format!("{token_calls} token telemetry events recorded"),
    );
    push_check(
        &mut checks,
        "cli-surface",
        required_cli_surface_present(),
        "required CLI command families are constructible from compiled command variants",
    );
    push_check(
        &mut checks,
        "mcp-surface",
        mcp::required_mcp_surface_present(),
        "required atlas_* tools are present in the generated RMCP route table",
    );
    let ok = checks
        .iter()
        .all(|check| check.status == ParityCheckStatus::Pass);
    Ok(ParityReport {
        profile: profile.to_string(),
        ok,
        overview,
        indexed_text_files,
        indexed_text_bytes,
        symbols,
        relations,
        health_findings,
        token_calls,
        watcher_mode,
        checks,
    })
}

/// Append one parity check.
fn push_check(checks: &mut Vec<ParityCheck>, name: &str, passed: bool, detail: &str) {
    checks.push(ParityCheck {
        name: name.to_string(),
        status: ParityCheckStatus::from_passed(passed),
        detail: detail.to_string(),
    });
}

/// Return whether the compiled CLI surface contains required command families.
fn required_cli_surface_present() -> bool {
    !REQUIRED_CLI_COMMANDS.is_empty()
        && REQUIRED_CLI_COMMANDS
            .iter()
            .all(|command| cli_command_name(&command.command()) == command.name())
}

/// Return the stable CLI name for a parsed command variant.
fn cli_command_name(command: &Command) -> &'static str {
    match command {
        Command::Init { .. } => "init",
        Command::Map { .. } => "map",
        Command::Scan { .. } => "scan",
        Command::Overview => "overview",
        Command::Folders { .. } => "folders",
        Command::Files { .. } => "files",
        Command::Next { .. } => "next",
        Command::Outline { .. } => "outline",
        Command::Summary { .. } => "summary",
        Command::Search { .. } => "search",
        Command::Slice { .. } => "slice",
        Command::Symbols { .. } => "symbols",
        Command::Settings => "settings",
        Command::Root { .. } => "root",
        Command::Config { .. } => "config",
        Command::Ignore { .. } => "ignore",
        Command::WatchStatus => "watch-status",
        Command::Watch { .. } => "watch",
        Command::HealthCheck { .. } => "health-check",
        Command::Health { .. } => "health",
        Command::Lint { .. } => "lint",
        Command::Token { .. } => "token",
        Command::Parity { .. } => "parity",
        Command::StripLegacyPurpose { .. } => "strip-legacy-purpose",
        Command::ResetIndex { .. } => "reset-index",
        Command::Mcp { .. } => "mcp",
        Command::McpConfig { .. } => "mcp-config",
        Command::RuntimeInfo => "runtime-info",
        Command::Purpose { .. } => "purpose",
    }
}

/// Render a deterministic file summary as compact TOON.
fn render_file_summary(report: &FileSummaryReport) -> String {
    encode_agent_payload(&json!({ "file_summary": report }))
}

/// Write text to stdout without using print macros.
fn write_stdout(text: &str) -> Result<(), CliError> {
    io::stdout().write_all(text.as_bytes())?;
    Ok(())
}

/// Write text to stderr without using print macros.
fn write_stderr(text: &str) -> Result<(), CliError> {
    io::stderr().write_all(text.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::mcp::{
        ProjectAtlasMcpServer, REQUIRED_MCP_TOOL_NAMES, mcp_tool_route_present,
        required_mcp_surface_present,
    };
    use super::runtime::{
        TextIndexOptions, byte_count_to_tokens, estimated_source_tokens_for_file_node,
        event_kind_affects_index, is_symbol_candidate, primary_symbol_names,
        refresh_structural_summaries_for_nodes, refresh_text_index_for_nodes,
        refresh_text_index_for_nodes_with_rows, relation_targets, reset_index_files,
        suggest_file_purpose, summarize_symbol_graph, watch_path_affects_index,
        watch_path_requires_full_scan, watcher_status_report,
    };
    use super::{
        OutputFormat, build_runtime_info, render_token_dashboard, serialized_output, truthy_env,
    };
    use notify::EventKind;
    use projectatlas_core::symbols::{
        CodeSymbol, ParserKind, RelationKind, SymbolGraph, SymbolKind, SymbolRelation,
    };
    use projectatlas_core::telemetry::TokenOverview;
    use projectatlas_core::{Node, NodeKind, normalize_native_path_display};
    use projectatlas_db::AtlasStore;
    use projectatlas_fs::ScanOptions;
    use rmcp::model::{CallToolRequestParams, ClientInfo};
    use rmcp::{ClientHandler, ServiceExt};
    use serde_json::{Map, Value, json};
    use std::error::Error;
    use std::fs;
    use std::io;
    use std::path::Path;

    /// Minimal MCP client handler for in-process routing tests.
    #[derive(Clone, Default)]
    struct TestMcpClient;

    impl ClientHandler for TestMcpClient {
        fn get_info(&self) -> ClientInfo {
            ClientInfo::default()
        }
    }

    #[cfg(unix)]
    fn create_directory_symlink(target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_directory_symlink(target: &Path, link: &Path) -> io::Result<()> {
        match std::os::windows::fs::symlink_dir(target, link) {
            Ok(()) => Ok(()),
            Err(source) if source.raw_os_error() == Some(1314) => {
                let status = std::process::Command::new("cmd")
                    .arg("/C")
                    .arg("mklink")
                    .arg("/J")
                    .arg(link)
                    .arg(target)
                    .status()?;
                if status.success() {
                    Ok(())
                } else {
                    Err(source)
                }
            }
            Err(source) => Err(source),
        }
    }

    fn require_selected_project_audit(
        text: &str,
        root: &Path,
        db: &Path,
        context: &str,
    ) -> Result<(), Box<dyn Error>> {
        let root_display = normalize_native_path_display(root.canonicalize()?);
        let db_display = normalize_native_path_display(db.canonicalize()?);
        if text.contains("selected_project:")
            && text.contains(&root_display)
            && text.contains(&db_display)
        {
            return Ok(());
        }
        Err(io::Error::other(format!(
            "{context} missing selected project audit root/db: {text}"
        ))
        .into())
    }

    #[test]
    fn summarizes_symbol_graph_from_observed_symbols_and_imports() {
        let graph = SymbolGraph {
            path: "src/service.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![
                test_symbol("src/service.rs", SymbolKind::Struct, "Service"),
                test_symbol("src/service.rs", SymbolKind::Method, "run"),
            ],
            relations: vec![test_relation(
                "src/service.rs",
                RelationKind::Imports,
                "std::path::Path",
            )],
        };

        assert_eq!(
            summarize_symbol_graph(&graph, Some("rust file, 10 bytes")),
            "rust source defining type and function Service, run with imports std::path::Path."
        );
    }

    #[test]
    fn summarizes_manifest_graph_from_dependencies() {
        let graph = SymbolGraph {
            path: "Cargo.toml".to_string(),
            language: Some("cargo-manifest".to_string()),
            parser: ParserKind::Manifest,
            symbols: vec![
                test_symbol("Cargo.toml", SymbolKind::Package, "projectatlas"),
                test_symbol("Cargo.toml", SymbolKind::Dependency, "serde"),
                test_symbol("Cargo.toml", SymbolKind::Dependency, "rmcp"),
            ],
            relations: vec![
                test_relation("Cargo.toml", RelationKind::DependsOn, "rmcp"),
                test_relation("Cargo.toml", RelationKind::DependsOn, "serde"),
            ],
        };

        assert_eq!(
            summarize_symbol_graph(&graph, None),
            "cargo manifest declaring projectatlas and depending on rmcp, serde."
        );
    }

    #[test]
    fn summarizes_empty_graph_from_fallback_without_approving_intent() {
        let graph = SymbolGraph {
            path: "src/empty.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::new(),
            relations: Vec::new(),
        };

        assert_eq!(
            summarize_symbol_graph(&graph, Some("rust file, 0 bytes")),
            "rust source file with no declarations found."
        );
        assert_eq!(
            suggest_file_purpose(
                "src/empty.rs",
                "rust source file with no declarations found."
            ),
            "Implement the empty source."
        );
        assert_eq!(
            suggest_file_purpose(
                "src/customers/service.rs",
                "rust source defining type and function CustomerService, boot."
            ),
            "Implement the customers service source around CustomerService and boot."
        );
        assert_eq!(
            suggest_file_purpose(
                "build.gradle.kts",
                "kotlin source defining functions bootRunE2E, copyE2EReports, verifyAtlas."
            ),
            "Define Gradle build tasks around bootRunE2E, copyE2EReports, and verifyAtlas."
        );
        assert_eq!(
            suggest_file_purpose(
                "src/auth/session.test.ts",
                "typescript source defining functions createsSession, rejectsExpiredSession."
            ),
            "Implement the auth session test source around createsSession and rejectsExpiredSession."
        );
    }

    #[test]
    fn summarizes_vue_composition_bindings_without_functions() {
        let graph = SymbolGraph {
            path: "src/ProductPanel.vue".to_string(),
            language: Some("vue".to_string()),
            parser: ParserKind::Structural,
            symbols: vec![
                test_symbol("src/ProductPanel.vue", SymbolKind::Value, "props"),
                test_symbol("src/ProductPanel.vue", SymbolKind::Value, "emit"),
                test_symbol(
                    "src/ProductPanel.vue",
                    SymbolKind::Value,
                    "currentPriceLabel",
                ),
            ],
            relations: vec![test_relation(
                "src/ProductPanel.vue",
                RelationKind::Imports,
                "import { computed, ref } from \"vue\";",
            )],
        };

        assert_eq!(
            summarize_symbol_graph(&graph, Some("vue file, 9990 bytes")),
            "vue source defining bindings currentPriceLabel, emit, props with imports import { computed, ref } from \"vue\";."
        );
    }

    #[test]
    fn summarizes_value_only_non_javascript_files_as_values() {
        let graph = SymbolGraph {
            path: "src/constants.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![
                test_symbol("src/constants.rs", SymbolKind::Value, "CACHE_LIMIT"),
                test_symbol("src/constants.rs", SymbolKind::Value, "DEFAULT_TIMEOUT"),
            ],
            relations: Vec::new(),
        };

        assert_eq!(
            summarize_symbol_graph(&graph, None),
            "rust source defining values CACHE_LIMIT, DEFAULT_TIMEOUT."
        );
    }

    #[test]
    fn symbol_candidate_policy_keeps_structural_formats_out_of_symbol_scan() {
        assert!(is_symbol_candidate("Cargo.toml", Some("cargo-manifest")));
        assert!(is_symbol_candidate("src/lib.rs", Some("rust")));
        assert!(!is_symbol_candidate(
            "fixtures/baselines.toon",
            Some("toon")
        ));
        assert!(!is_symbol_candidate("README.md", Some("markdown")));
    }

    #[test]
    fn summarizes_functions_before_javascript_constants_when_both_exist() {
        let graph = SymbolGraph {
            path: "scripts/generate.mjs".to_string(),
            language: Some("javascript".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![
                test_symbol("scripts/generate.mjs", SymbolKind::Value, "DATA_DIRECTORY"),
                test_symbol("scripts/generate.mjs", SymbolKind::Value, "OUTPUT_FILE"),
                test_symbol("scripts/generate.mjs", SymbolKind::Function, "sha256"),
                test_symbol(
                    "scripts/generate.mjs",
                    SymbolKind::Function,
                    "readDatasetEntry",
                ),
                test_symbol("scripts/generate.mjs", SymbolKind::Function, "main"),
            ],
            relations: vec![test_relation(
                "scripts/generate.mjs",
                RelationKind::Imports,
                "import path from \"node:path\";",
            )],
        };

        assert_eq!(
            summarize_symbol_graph(&graph, None),
            "javascript source defining functions main, readDatasetEntry, sha256 with imports import path from \"node:path\";."
        );
    }

    #[test]
    fn watcher_filters_relevant_index_events() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let scan_options = ScanOptions {
            exclude_dir_names: vec![
                ".git".to_string(),
                ".projectatlas".to_string(),
                "target".to_string(),
                "generated".to_string(),
            ],
            exclude_dir_suffixes: Vec::new(),
            exclude_path_prefixes: vec!["docs/api".to_string()],
        };
        require_condition(
            watch_path_affects_index(root, &root.join("src/lib.rs"), &scan_options),
            "source file event should refresh the index",
        )?;
        require_condition(
            !watch_path_affects_index(root, &root.join("../outside.rs"), &scan_options),
            "absolute parent traversal events should be ignored",
        )?;
        require_condition(
            !watch_path_affects_index(root, Path::new("../outside.rs"), &scan_options),
            "relative parent traversal events should be ignored",
        )?;
        require_condition(
            !watch_path_requires_full_scan(root, &root.join("src/lib.rs")),
            "source file event should use incremental refresh",
        )?;
        fs::create_dir(root.join("src"))?;
        require_condition(
            watch_path_requires_full_scan(root, &root.join("src")),
            "directory event should use full refresh",
        )?;
        require_condition(
            watch_path_requires_full_scan(root, &root.join(".gitignore")),
            "gitignore event should use full refresh",
        )?;
        require_condition(
            watch_path_affects_index(root, &root.join(".gitignore"), &scan_options),
            "gitignore event should refresh scanner rules",
        )?;
        fs::create_dir(root.join("local-state"))?;
        fs::write(root.join("local-state/cache.md"), "ignored local cache\n")?;
        fs::write(root.join(".gitignore"), "local-state/\n")?;
        require_condition(
            !watch_path_affects_index(root, &root.join("local-state/cache.md"), &scan_options),
            "gitignore-ignored local state events should be ignored",
        )?;
        require_condition(
            !watch_path_affects_index(
                root,
                &root.join(".projectatlas/projectatlas.db"),
                &scan_options,
            ),
            "ProjectAtlas database events should be ignored",
        )?;
        require_condition(
            !watch_path_affects_index(root, &root.join("target/debug/projectatlas"), &scan_options),
            "target directory events should be ignored",
        )?;
        require_condition(
            !watch_path_affects_index(root, &root.join("src/.purpose"), &scan_options),
            "legacy .purpose metadata events should be ignored",
        )?;
        require_condition(
            !watch_path_affects_index(root, &root.join("generated/out.rs"), &scan_options),
            "configured exclude directory events should be ignored",
        )?;
        require_condition(
            !watch_path_affects_index(root, &root.join("docs/api/noise.rs"), &scan_options),
            "configured exclude path-prefix events should be ignored",
        )?;
        require_condition(
            watch_path_affects_index(root, &root.join("src/api/live.rs"), &scan_options),
            "same directory name outside excluded prefix should be indexed",
        )?;
        require_condition(
            !event_kind_affects_index(EventKind::Access(notify::event::AccessKind::Any)),
            "access-only events should not refresh the index",
        )?;
        require_condition(
            event_kind_affects_index(EventKind::Modify(notify::event::ModifyKind::Any)),
            "modify events should refresh the index",
        )?;
        Ok(())
    }

    /// Return an error instead of panicking when a test condition fails.
    fn require_condition(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message.to_string()).into())
        }
    }

    #[test]
    fn required_mcp_surface_checks_actual_tool_routes() {
        assert!(required_mcp_surface_present());
        for required_tool in REQUIRED_MCP_TOOL_NAMES {
            assert!(
                mcp_tool_route_present(required_tool),
                "{required_tool} missing"
            );
        }
    }

    #[test]
    fn runtime_info_reports_stable_installer_contract() {
        let info = build_runtime_info();

        assert_eq!(info.project, "ProjectAtlas");
        assert_eq!(info.major_version, 3);
        assert!(
            info.capabilities
                .iter()
                .any(|capability| capability == "mcp")
        );
        assert_eq!(info.text_format, "TOON");
        assert!(
            info.mcp_tools.iter().any(|tool| tool == "atlas_scan"),
            "atlas_scan missing from runtime-info"
        );
    }

    #[test]
    fn text_index_skips_oversized_files_without_hiding_nodes() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::write(root.join("small.txt"), "small")?;
        fs::write(root.join("large.txt"), "large content")?;
        let nodes = vec![
            Node {
                path: "small.txt".to_string(),
                kind: NodeKind::File,
                parent_path: None,
                extension: Some(".txt".to_string()),
                language: Some("text".to_string()),
                size_bytes: Some(5),
                mtime_ns: Some(1),
                content_hash: Some("small-hash".to_string()),
            },
            Node {
                path: "large.txt".to_string(),
                kind: NodeKind::File,
                parent_path: None,
                extension: Some(".txt".to_string()),
                language: Some("text".to_string()),
                size_bytes: Some(13),
                mtime_ns: Some(1),
                content_hash: Some("large-hash".to_string()),
            },
        ];
        let mut store = AtlasStore::in_memory()?;
        let report =
            refresh_text_index_for_nodes(&mut store, root, &nodes, TextIndexOptions::new(5))?;

        require_condition(report.candidates == 2, "candidate count")?;
        require_condition(report.indexed == 1, "indexed count")?;
        require_condition(report.too_large == 1, "too-large count")?;
        require_condition(report.binary_or_non_utf8 == 0, "binary count")?;
        require_condition(report.skipped == 1, "skipped count")?;
        require_condition(report.max_bytes == 5, "max byte policy")?;
        require_condition(
            store.load_file_text("small.txt")?.is_some(),
            "small text indexed",
        )?;
        require_condition(
            store.load_file_text("large.txt")?.is_none(),
            "large text skipped",
        )?;
        Ok(())
    }

    #[test]
    fn structural_summary_refresh_clears_stale_summary_when_text_is_skipped()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::write(root.join("config.toml"), "[project]\nroot = \".\"\n")?;
        let nodes = vec![Node {
            path: "config.toml".to_string(),
            kind: NodeKind::File,
            parent_path: None,
            extension: Some(".toml".to_string()),
            language: Some("toml".to_string()),
            size_bytes: Some(19),
            mtime_ns: Some(1),
            content_hash: Some("config-hash".to_string()),
        }];
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&nodes)?;
        let text_refresh = refresh_text_index_for_nodes_with_rows(
            &mut store,
            root,
            &nodes,
            TextIndexOptions::new(100),
        )?;
        let first_report =
            refresh_structural_summaries_for_nodes(&mut store, &nodes, &text_refresh.rows)?;
        require_condition(first_report.summarized == 1, "initial structural summary")?;
        require_condition(
            store
                .load_node_by_path("config.toml")?
                .and_then(|node| node.summary)
                .is_some(),
            "summary should exist before skip",
        )?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "config.toml".to_string(),
            language: Some("toml".to_string()),
            parser: ParserKind::Manifest,
            symbols: vec![test_symbol("config.toml", SymbolKind::Value, "project")],
            relations: Vec::new(),
        })?;

        let skipped_text = refresh_text_index_for_nodes_with_rows(
            &mut store,
            root,
            &nodes,
            TextIndexOptions::new(5),
        )?;
        let stale_report =
            refresh_structural_summaries_for_nodes(&mut store, &nodes, &skipped_text.rows)?;
        require_condition(stale_report.too_large == 1, "structural too-large count")?;
        require_condition(stale_report.cleared == 1, "cleared stale summary count")?;
        require_condition(
            store
                .load_node_by_path("config.toml")?
                .and_then(|node| node.summary)
                .is_none(),
            "summary should be cleared after current text is skipped",
        )?;
        Ok(())
    }

    #[test]
    fn watcher_status_does_not_report_background_activity() {
        let status = watcher_status_report(false);

        assert!(status.available);
        assert!(!status.active);
        assert!(!status.mode.is_empty());
    }

    #[test]
    fn reset_index_preview_and_apply_are_file_scoped() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let db = temp.path().join("projectatlas.db");
        fs::write(&db, "db")?;
        fs::write(temp.path().join("projectatlas.db-wal"), "wal")?;
        fs::write(temp.path().join("projectatlas.mcp.json"), "{}")?;

        let preview = reset_index_files(&db, false, false, true)?;
        require_condition(!preview.applied, "preview should not apply")?;
        require_condition(preview.removed == 0, "preview should not remove files")?;
        require_condition(db.exists(), "preview removed database")?;

        let applied = reset_index_files(&db, true, false, true)?;
        require_condition(applied.applied, "apply should mark report applied")?;
        require_condition(applied.removed == 3, "apply removed unexpected file count")?;
        require_condition(!db.exists(), "database remained after apply")?;
        require_condition(
            !temp.path().join("projectatlas.db-wal").exists(),
            "wal remained after apply",
        )?;
        require_condition(
            !temp.path().join("projectatlas.mcp.json").exists(),
            "mcp config remained after apply",
        )?;
        Ok(())
    }

    #[test]
    fn primary_symbol_names_are_stable_deduped_and_limited() {
        let graph = SymbolGraph {
            path: "src/lib.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![
                test_symbol("src/lib.rs", SymbolKind::Function, "zeta"),
                test_symbol("src/lib.rs", SymbolKind::Function, "alpha"),
                test_symbol("src/lib.rs", SymbolKind::Function, "alpha"),
                test_symbol("src/lib.rs", SymbolKind::Function, "beta"),
            ],
            relations: Vec::new(),
        };

        assert_eq!(
            primary_symbol_names(&graph, 2),
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }

    #[test]
    fn relation_targets_are_stable_deduped_and_limited() {
        let graph = SymbolGraph {
            path: "src/lib.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::new(),
            relations: vec![
                test_relation("src/lib.rs", RelationKind::Imports, "zeta"),
                test_relation("src/lib.rs", RelationKind::Imports, "alpha"),
                test_relation("src/lib.rs", RelationKind::Imports, "alpha"),
            ],
        };

        assert_eq!(
            relation_targets(&graph, RelationKind::Imports, 2),
            vec!["alpha".to_string(), "zeta".to_string()]
        );
    }

    #[test]
    fn token_dashboard_is_human_readable_and_chart_backed() {
        let dashboard = render_token_dashboard(
            &TokenOverview::from_estimated_totals(3, 12_000, 3_000),
            Some("session-a"),
        );

        assert!(dashboard.contains("ProjectAtlas"));
        assert!(dashboard.contains("Token Impact"));
        assert!(dashboard.contains("session-a"));
        assert!(dashboard.contains("T O T A L   T O K E N S   A V O I D E D"));
        assert!(dashboard.contains("Without ProjectAtlas"));
        assert!(dashboard.contains("With ProjectAtlas"));
        assert!(dashboard.contains("Saved by ProjectAtlas"));
        assert!(dashboard.contains("F I L E   R E A D S   A V O I D E D"));
        assert!(dashboard.contains("S A V I N G S   C O M P O S I T I O N"));
        assert!(dashboard.contains("S I G N A L"));
        assert!(dashboard.contains("W H E R E   T H E   S A V I N G S   C A M E   F R O M"));
        assert!(dashboard.contains("C A L I B R A T I O N   &   N O T E S"));
        assert!(dashboard.contains("not_recorded"));
        assert!(dashboard.contains("Tokenizer audit"));
        assert!(
            dashboard
                .chars()
                .any(|character| matches!(character, '█' | '\u{2801}'..='\u{28ff}'))
        );
        assert!(!dashboard.contains("Gross tokens: without vs with ProjectAtlas"));
        assert!(!dashboard.contains("How ProjectAtlas helped"));
        assert!(!dashboard.contains("Saved-token trends"));
    }

    #[test]
    fn telemetry_baselines_use_source_size_without_reading_all_files() {
        let node = Node {
            path: "src/main.rs".to_string(),
            kind: NodeKind::File,
            parent_path: Some("src".to_string()),
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(41),
            mtime_ns: Some(1),
            content_hash: Some("hash".to_string()),
        };

        assert_eq!(estimated_source_tokens_for_file_node(&node), 11);
        assert_eq!(byte_count_to_tokens(9), 3);
    }

    #[test]
    fn json_output_serialization_is_measurable_for_telemetry() -> Result<(), Box<dyn Error>> {
        let payload = serde_json::json!({ "path": "src/main.rs", "lines": [1, 2, 3] });
        let toon = "path: src/main.rs\n";
        let json = serialized_output(OutputFormat::Json, toon, &payload)?;

        if !json.contains("\"path\": \"src/main.rs\"") {
            return Err(io::Error::other("json output did not contain path").into());
        }
        if !json.ends_with('\n') {
            return Err(io::Error::other("json output did not end with newline").into());
        }
        if json.len() <= toon.len() {
            return Err(io::Error::other("json output was not larger than toon fixture").into());
        }
        Ok(())
    }

    /// Build a compact test symbol.
    fn test_symbol(path: &str, kind: SymbolKind, name: &str) -> CodeSymbol {
        CodeSymbol {
            path: path.to_string(),
            language: Some("rust".to_string()),
            name: name.to_string(),
            kind,
            signature: name.to_string(),
            exported: false,
            documentation: None,
            line_start: 1,
            line_end: 1,
            parent: None,
            parser: ParserKind::TreeSitter,
            detail: None,
        }
    }

    /// Build a compact test relation.
    fn test_relation(path: &str, kind: RelationKind, target: &str) -> SymbolRelation {
        SymbolRelation {
            path: path.to_string(),
            source_name: "module".to_string(),
            target_name: target.to_string(),
            kind,
            line: 1,
            context: target.to_string(),
            parser: ParserKind::TreeSitter,
        }
    }

    #[tokio::test]
    async fn mcp_tools_return_toon_text_payloads() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        fs::create_dir(&repo)?;
        fs::create_dir(repo.join("src"))?;
        fs::create_dir(repo.join("assets"))?;
        fs::write(
            repo.join("src").join("main.rs"),
            "fn main() {\n    helper();\n}\n\nfn helper() {}\n",
        )?;
        fs::write(repo.join("src").join("detail.rs"), "fn detail() {}\n")?;
        fs::write(
            repo.join("assets").join("logo.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
        )?;
        let db = repo.join(".projectatlas").join("projectatlas.db");
        let server = ProjectAtlasMcpServer::new(db, None, "mcp-test".to_string(), false);
        let (server_transport, client_transport) = tokio::io::duplex(16_384);
        let server_handle = tokio::spawn(async move {
            server
                .serve(server_transport)
                .await
                .map_err(|error| error.to_string())?
                .waiting()
                .await
                .map_err(|error| error.to_string())?;
            Ok::<(), String>(())
        });
        let client = TestMcpClient.serve(client_transport).await?;
        let tools = client.peer().list_tools(Option::default()).await?;
        for required_tool in REQUIRED_MCP_TOOL_NAMES {
            if !tools.tools.iter().any(|tool| tool.name == *required_tool) {
                return Err(format!("{required_tool} tool was not registered").into());
            }
        }
        let schema_has_property =
            |tool_name: &str, property: &str| -> Result<bool, Box<dyn Error>> {
                let tool = tools
                    .tools
                    .iter()
                    .find(|tool| tool.name.as_ref() == tool_name)
                    .ok_or_else(|| std::io::Error::other(format!("{tool_name} missing")))?;
                let schema = serde_json::to_value(&tool.input_schema)?;
                Ok(schema
                    .get("properties")
                    .and_then(Value::as_object)
                    .is_some_and(|properties| properties.contains_key(property)))
            };
        if schema_has_property("atlas_folders", "nearest_project")? {
            return Err("atlas_folders advertised unused nearest_project parameter".into());
        }
        if !schema_has_property("atlas_files", "nearest_project")? {
            return Err("atlas_files did not advertise nearest_project parameter".into());
        }
        if schema_has_property("atlas_next", "nearest_project")? {
            return Err("atlas_next advertised unused nearest_project parameter".into());
        }

        let scan = client
            .peer()
            .call_tool(CallToolRequestParams::new("atlas_scan").with_arguments(Map::new()))
            .await?;
        let scan_text = scan
            .content
            .first()
            .and_then(|content| content.raw.as_text())
            .map(|text| text.text.as_str())
            .ok_or_else(|| std::io::Error::other("scan result did not contain text"))?;
        if !scan_text.contains("scan:") {
            return Err("atlas_scan result did not contain scan payload".into());
        }
        if !scan_text.contains("symbols:") {
            return Err("atlas_scan result did not contain symbols payload".into());
        }

        let mut symbols_args = Map::new();
        symbols_args.insert("file".to_string(), json!("src/main.rs"));
        let symbols = client
            .peer()
            .call_tool(CallToolRequestParams::new("atlas_symbols").with_arguments(symbols_args))
            .await?;
        let symbols_text = symbols
            .content
            .first()
            .and_then(|content| content.raw.as_text())
            .map(|text| text.text.as_str())
            .ok_or_else(|| std::io::Error::other("symbols result did not contain text"))?;
        if !symbols_text.contains("symbols[") {
            return Err("atlas_symbols result did not contain symbols table".into());
        }
        if !symbols_text.contains("helper") {
            return Err("atlas_symbols result did not contain helper symbol".into());
        }

        let mut summary_args = Map::new();
        summary_args.insert("file".to_string(), json!("src/main.rs"));
        let summary = client
            .peer()
            .call_tool(
                CallToolRequestParams::new("atlas_file_summary").with_arguments(summary_args),
            )
            .await?;
        let summary_text = summary
            .content
            .first()
            .and_then(|content| content.raw.as_text())
            .map(|text| text.text.as_str())
            .ok_or_else(|| std::io::Error::other("summary result did not contain text"))?;
        if !summary_text.contains("file_summary:") {
            return Err("atlas_file_summary result did not contain summary payload".into());
        }
        if !summary_text.contains("file_purpose_status: suggested") {
            return Err("atlas_file_summary result did not expose purpose status".into());
        }
        if !summary_text.contains("parser_kind: \"tree-sitter-symbol-graph\"") {
            return Err("atlas_file_summary result did not expose parser kind".into());
        }
        if !summary_text.contains("summary_status: ok") {
            return Err("atlas_file_summary result did not expose summary status".into());
        }
        if !summary_text.contains("helper") {
            return Err("atlas_file_summary result did not contain helper symbol".into());
        }

        let outside_path = temp.path().join("outside-project.txt");
        fs::write(&outside_path, "outside repo proof")?;
        let mut slice_args = Map::new();
        slice_args.insert(
            "file".to_string(),
            json!(outside_path.to_string_lossy().to_string()),
        );
        slice_args.insert("start_line".to_string(), json!(1));
        let slice = client
            .peer()
            .call_tool(CallToolRequestParams::new("atlas_slice").with_arguments(slice_args))
            .await?;
        let slice_text = slice
            .content
            .first()
            .and_then(|content| content.raw.as_text())
            .map(|text| text.text.as_str())
            .ok_or_else(|| std::io::Error::other("slice result did not contain text"))?;
        if !slice_text.contains("indexed ProjectAtlas project")
            || !slice_text.contains("Get-Content")
        {
            return Err(format!(
                "atlas_slice did not reject outside-repository absolute paths: {slice_text}"
            )
            .into());
        }

        let token_report = client
            .peer()
            .call_tool(CallToolRequestParams::new("atlas_token_report").with_arguments(Map::new()))
            .await?;
        let token_text = token_report
            .content
            .first()
            .and_then(|content| content.raw.as_text())
            .map(|text| text.text.as_str())
            .ok_or_else(|| std::io::Error::other("token report did not contain text"))?;
        if !token_text.contains("token_savings:") {
            return Err("atlas_token_report result did not contain token payload".into());
        }
        if truthy_env("PROJECTATLAS_NO_TELEMETRY") {
            if !token_text.contains("calls: 0") {
                return Err("atlas_token_report recorded MCP usage in no-telemetry mode".into());
            }
        } else {
            if !token_text.contains("calls: 2") {
                return Err("atlas_token_report did not count MCP usage events".into());
            }
            if !token_text.contains("buckets[") || !token_text.contains("heuristic_estimate") {
                return Err(
                    "atlas_token_report result did not contain bucket accuracy labels".into(),
                );
            }
        }

        let parity_report = client
            .peer()
            .call_tool(CallToolRequestParams::new("atlas_parity_report").with_arguments(Map::new()))
            .await?;
        let parity_text = parity_report
            .content
            .first()
            .and_then(|content| content.raw.as_text())
            .map(|text| text.text.as_str())
            .ok_or_else(|| std::io::Error::other("parity report did not contain text"))?;
        if !parity_text.contains("parity:")
            || !parity_text.contains("profile: \"repository-intelligence\"")
        {
            return Err("atlas_parity_report result did not contain parity payload".into());
        }

        let mut health_args = Map::new();
        health_args.insert("category".to_string(), json!("missing-purpose"));
        health_args.insert("path_prefix".to_string(), json!(".\\src\\"));
        health_args.insert("limit".to_string(), json!(1));
        let health = client
            .peer()
            .call_tool(CallToolRequestParams::new("atlas_health").with_arguments(health_args))
            .await?;
        let health_text = health
            .content
            .first()
            .and_then(|content| content.raw.as_text())
            .map(|text| text.text.as_str())
            .ok_or_else(|| std::io::Error::other("health result did not contain text"))?;
        if !health_text.contains("health:")
            || !health_text.contains("returned: 1")
            || !health_text.contains("limit: 1")
            || !health_text.contains("next_start_index: null")
            || !health_text.contains("source_only: false")
            || !health_text.contains("path_prefix: src")
            || !health_text.contains("health_findings[1]")
            || health_text.contains("suggested-purpose-review")
        {
            return Err(
                format!("atlas_health result was not bounded and filtered: {health_text}").into(),
            );
        }

        let mut summary_health_args = Map::new();
        summary_health_args.insert("category".to_string(), json!("missing-purpose"));
        summary_health_args.insert("path_prefix".to_string(), json!(".\\src\\"));
        summary_health_args.insert("limit".to_string(), json!(1));
        summary_health_args.insert("summary_only".to_string(), json!(true));
        let summary_health = client
            .peer()
            .call_tool(
                CallToolRequestParams::new("atlas_health").with_arguments(summary_health_args),
            )
            .await?;
        let summary_health_text = summary_health
            .content
            .first()
            .and_then(|content| content.raw.as_text())
            .map(|text| text.text.as_str())
            .ok_or_else(|| std::io::Error::other("summary health result did not contain text"))?;
        if !summary_health_text.contains("returned: 0")
            || !summary_health_text.contains("limit: 1")
            || !summary_health_text.contains("next_start_index: null")
            || !summary_health_text.contains("summary_only: true")
            || !summary_health_text.contains("health_findings[0]")
        {
            return Err(format!(
                "atlas_health summary_only result lost paging metadata: {summary_health_text}"
            )
            .into());
        }

        let purpose_queue = client
            .peer()
            .call_tool(CallToolRequestParams::new("atlas_purpose_queue").with_arguments(Map::new()))
            .await?;
        let purpose_queue_text = purpose_queue
            .content
            .first()
            .and_then(|content| content.raw.as_text())
            .map(|text| text.text.as_str())
            .ok_or_else(|| std::io::Error::other("purpose queue result did not contain text"))?;
        if !purpose_queue_text.contains("purpose_curation:")
            || !purpose_queue_text.contains("source_only: true")
            || !purpose_queue_text.contains("folder_scope: all")
            || !purpose_queue_text.contains("file_scope: high_impact")
            || !purpose_queue_text.contains("purpose_curation_items[")
            || !purpose_queue_text.contains("purpose_agent_reviewed,review_priority,review_reason")
            || !purpose_queue_text.contains("false,high,high_impact_file")
            || !purpose_queue_text.contains("suggested-purpose-review:src/main.rs:")
            || purpose_queue_text.contains("suggested-purpose-review:src/detail.rs:")
            || purpose_queue_text.contains("assets/logo.svg")
        {
            return Err(format!(
                "atlas_purpose_queue result did not contain folder-first curation payload: {purpose_queue_text}"
            )
            .into());
        }

        let mut asset_queue_args = Map::new();
        asset_queue_args.insert("include_assets".to_string(), json!(true));
        let asset_purpose_queue = client
            .peer()
            .call_tool(
                CallToolRequestParams::new("atlas_purpose_queue").with_arguments(asset_queue_args),
            )
            .await?;
        let asset_purpose_queue_text = asset_purpose_queue
            .content
            .first()
            .and_then(|content| content.raw.as_text())
            .map(|text| text.text.as_str())
            .ok_or_else(|| {
                std::io::Error::other("asset purpose queue result did not contain text")
            })?;
        if !asset_purpose_queue_text.contains("source_only: false")
            || !asset_purpose_queue_text.contains("folder_scope: all")
            || !asset_purpose_queue_text.contains("file_scope: high_impact_and_assets")
            || !asset_purpose_queue_text.contains("missing-purpose:assets/logo.svg:")
            || asset_purpose_queue_text.contains("suggested-purpose-review:src/detail.rs:")
        {
            return Err(format!(
                "atlas_purpose_queue include_assets did not include assets without low-priority source cleanup: {asset_purpose_queue_text}"
            )
            .into());
        }

        let mut broad_queue_args = Map::new();
        broad_queue_args.insert("include_low_priority_files".to_string(), json!(true));
        let broad_purpose_queue = client
            .peer()
            .call_tool(
                CallToolRequestParams::new("atlas_purpose_queue").with_arguments(broad_queue_args),
            )
            .await?;
        let broad_purpose_queue_text = broad_purpose_queue
            .content
            .first()
            .and_then(|content| content.raw.as_text())
            .map(|text| text.text.as_str())
            .ok_or_else(|| {
                std::io::Error::other("broad purpose queue result did not contain text")
            })?;
        if !broad_purpose_queue_text.contains("suggested-purpose-review:src/detail.rs:")
            || !broad_purpose_queue_text.contains("folder_scope: source_relevant")
            || !broad_purpose_queue_text.contains("file_scope: all_source")
            || !broad_purpose_queue_text.contains("false,low,generated_file_suggestion")
        {
            return Err(format!(
                "atlas_purpose_queue include_low_priority_files missed low-priority file payload: {broad_purpose_queue_text}"
            )
            .into());
        }

        client.cancel().await?;
        server_handle.await?.map_err(std::io::Error::other)?;
        Ok(())
    }

    #[tokio::test]
    async fn mcp_project_path_overrides_keep_projects_isolated() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let repo_a = temp.path().join("repo-a");
        let repo_b = temp.path().join("repo-b");
        for repo in [&repo_a, &repo_b] {
            fs::create_dir(repo)?;
            fs::create_dir(repo.join("src"))?;
        }
        fs::write(
            repo_a.join("src").join("lib.rs"),
            "pub fn alpha_project_a_marker() {}\n",
        )?;
        fs::write(
            repo_b.join("src").join("lib.rs"),
            "pub fn beta_project_b_marker() {}\n",
        )?;

        let db_a = repo_a.join(".projectatlas").join("projectatlas.db");
        let db_b = repo_b.join(".projectatlas").join("projectatlas.db");
        let server = ProjectAtlasMcpServer::new(
            db_a.clone(),
            None,
            "mcp-multi-project-test".to_string(),
            false,
        );
        let (server_transport, client_transport) = tokio::io::duplex(16_384);
        let server_handle = tokio::spawn(async move {
            server
                .serve(server_transport)
                .await
                .map_err(|error| error.to_string())?
                .waiting()
                .await
                .map_err(|error| error.to_string())?;
            Ok::<(), String>(())
        });
        let client = TestMcpClient.serve(client_transport).await?;

        macro_rules! call_text {
            ($tool:literal, $args:expr) => {{
                let result = client
                    .peer()
                    .call_tool(CallToolRequestParams::new($tool).with_arguments($args))
                    .await?;
                result
                    .content
                    .first()
                    .and_then(|content| content.raw.as_text())
                    .map(|text| text.text.clone())
                    .ok_or_else(|| {
                        std::io::Error::other(format!("{} result did not contain text", $tool))
                    })?
            }};
        }

        let scan_a = call_text!("atlas_scan", Map::new());
        if !scan_a.contains("scan:") {
            return Err("default atlas_scan did not scan the startup project".into());
        }
        let db_a_before_repo_b_scan = fs::read(&db_a)?;
        let db_a_hash_before_repo_b_scan = blake3::hash(&db_a_before_repo_b_scan);
        let db_a_metadata_before_repo_b_scan = fs::metadata(&db_a)?;

        let mut wrong_path_scan_args = Map::new();
        wrong_path_scan_args.insert(
            "path".to_string(),
            json!(repo_b.to_string_lossy().to_string()),
        );
        let wrong_path_scan = call_text!("atlas_scan", wrong_path_scan_args);
        if !wrong_path_scan.contains("outside the selected project root")
            || !wrong_path_scan.contains("normal filesystem tools")
        {
            return Err(format!(
                "atlas_scan allowed unindexed path-based access outside the active project: {wrong_path_scan}"
            )
            .into());
        }

        let mut scan_b_args = Map::new();
        scan_b_args.insert(
            "project_path".to_string(),
            json!(repo_b.to_string_lossy().to_string()),
        );
        let scan_b = call_text!("atlas_scan", scan_b_args);
        if !scan_b.contains("scan:") {
            return Err("project_path-selected atlas_scan did not scan repo B".into());
        }
        let db_a_after_repo_b_scan = fs::read(&db_a)?;
        let db_a_hash_after_repo_b_scan = blake3::hash(&db_a_after_repo_b_scan);
        let db_a_metadata_after_repo_b_scan = fs::metadata(&db_a)?;
        if db_a_hash_after_repo_b_scan != db_a_hash_before_repo_b_scan
            || db_a_metadata_after_repo_b_scan.len() != db_a_metadata_before_repo_b_scan.len()
        {
            return Err(
                "project_path-selected atlas_scan mutated the startup project database".into(),
            );
        }
        if !db_b.exists() {
            return Err("project_path-selected atlas_scan did not create repo B database".into());
        }

        let mut absolute_summary_a_args = Map::new();
        absolute_summary_a_args.insert(
            "file".to_string(),
            json!(
                repo_a
                    .join("src")
                    .join("lib.rs")
                    .to_string_lossy()
                    .to_string()
            ),
        );
        let absolute_summary_a = call_text!("atlas_file_summary", absolute_summary_a_args);
        if !absolute_summary_a.contains("alpha_project_a_marker")
            || !absolute_summary_a.contains("file_path: src/lib.rs")
        {
            return Err(format!(
                "absolute file path inside selected project was not accepted: {absolute_summary_a}"
            )
            .into());
        }

        let mut active_subdir_scan_args = Map::new();
        active_subdir_scan_args.insert(
            "path".to_string(),
            json!(repo_a.join("src").to_string_lossy().to_string()),
        );
        active_subdir_scan_args.insert("nearest_project".to_string(), json!(true));
        let active_subdir_scan = call_text!("atlas_scan", active_subdir_scan_args);
        if active_subdir_scan.contains("scan:")
            || !active_subdir_scan.contains("not the selected project root")
            || active_subdir_scan.contains("selected_project:")
        {
            return Err(format!(
                "nearest_project bypassed root assertion for active subdirectory: {active_subdir_scan}"
            )
            .into());
        }
        let mut active_relative_subdir_scan_args = Map::new();
        active_relative_subdir_scan_args.insert("path".to_string(), json!("src"));
        active_relative_subdir_scan_args.insert("nearest_project".to_string(), json!(true));
        let active_relative_subdir_scan =
            call_text!("atlas_scan", active_relative_subdir_scan_args);
        if active_relative_subdir_scan.contains("scan:")
            || !active_relative_subdir_scan.contains("not the selected project root")
            || active_relative_subdir_scan.contains("selected_project:")
        {
            return Err(format!(
                "nearest_project bypassed root assertion for relative active subdirectory: {active_relative_subdir_scan}"
            )
            .into());
        }

        let mut indexed_path_scan_b_args = Map::new();
        indexed_path_scan_b_args.insert(
            "path".to_string(),
            json!(repo_b.to_string_lossy().to_string()),
        );
        let indexed_path_scan_b = call_text!("atlas_scan", indexed_path_scan_b_args.clone());
        if indexed_path_scan_b.contains("scan:")
            || !indexed_path_scan_b.contains("outside the selected project root")
            || !indexed_path_scan_b.contains("Get-Content")
        {
            return Err(format!(
                "default-off atlas_scan routed to another indexed project: {indexed_path_scan_b}"
            )
            .into());
        }
        indexed_path_scan_b_args.insert("nearest_project".to_string(), json!(true));
        let indexed_path_scan_b = call_text!("atlas_scan", indexed_path_scan_b_args);
        if !indexed_path_scan_b.contains("scan:") {
            return Err("atlas_scan nearest_project override did not route indexed repo B".into());
        }
        let mut indexed_subdir_scan_b_args = Map::new();
        indexed_subdir_scan_b_args.insert(
            "path".to_string(),
            json!(repo_b.join("src").to_string_lossy().to_string()),
        );
        indexed_subdir_scan_b_args.insert("nearest_project".to_string(), json!(true));
        let indexed_subdir_scan_b = call_text!("atlas_scan", indexed_subdir_scan_b_args);
        if indexed_subdir_scan_b.contains("scan:")
            || !indexed_subdir_scan_b.contains("outside the selected project root")
            || indexed_subdir_scan_b.contains("selected_project:")
        {
            return Err(format!(
                "nearest_project treated an indexed project subdirectory as a root assertion: {indexed_subdir_scan_b}"
            )
            .into());
        }

        let mut absolute_summary_b_args = Map::new();
        absolute_summary_b_args.insert(
            "file".to_string(),
            json!(
                repo_b
                    .join("src")
                    .join("lib.rs")
                    .to_string_lossy()
                    .to_string()
            ),
        );
        let rejected_summary_b = call_text!("atlas_file_summary", absolute_summary_b_args.clone());
        if rejected_summary_b.contains("beta_project_b_marker")
            || !rejected_summary_b.contains("indexed ProjectAtlas project")
            || !rejected_summary_b.contains("Get-Content")
        {
            return Err(format!(
                "default-off absolute file routing did not fall back to filesystem guidance: {rejected_summary_b}"
            )
            .into());
        }
        absolute_summary_b_args.insert("nearest_project".to_string(), json!(true));
        let absolute_summary_b = call_text!("atlas_file_summary", absolute_summary_b_args);
        if !absolute_summary_b.contains("beta_project_b_marker")
            || !absolute_summary_b.contains("file_path: src/lib.rs")
        {
            return Err(format!(
                "absolute file path did not route to nearest indexed repo B with override: {absolute_summary_b}"
            )
            .into());
        }
        require_selected_project_audit(
            &absolute_summary_b,
            &repo_b,
            &db_b,
            "nearest-routed file summary",
        )?;

        let mut absolute_slice_b_args = Map::new();
        absolute_slice_b_args.insert(
            "file".to_string(),
            json!(
                repo_b
                    .join("src")
                    .join("lib.rs")
                    .to_string_lossy()
                    .to_string()
            ),
        );
        absolute_slice_b_args.insert("start_line".to_string(), json!(1));
        absolute_slice_b_args.insert("end_line".to_string(), json!(1));
        let rejected_slice_b = call_text!("atlas_slice", absolute_slice_b_args.clone());
        if rejected_slice_b.contains("beta_project_b_marker")
            || !rejected_slice_b.contains("indexed ProjectAtlas project")
            || !rejected_slice_b.contains("Get-Content")
        {
            return Err(format!(
                "default-off atlas_slice read another project instead of returning filesystem guidance: {rejected_slice_b}"
            )
            .into());
        }
        absolute_slice_b_args.insert("nearest_project".to_string(), json!(true));
        let absolute_slice_b = call_text!("atlas_slice", absolute_slice_b_args);
        if !absolute_slice_b.contains("beta_project_b_marker") {
            return Err("atlas_slice nearest_project override did not route indexed repo B".into());
        }
        require_selected_project_audit(&absolute_slice_b, &repo_b, &db_b, "nearest-routed slice")?;

        let mut absolute_files_b_args = Map::new();
        absolute_files_b_args.insert("query".to_string(), json!("beta"));
        absolute_files_b_args.insert(
            "folder".to_string(),
            json!(repo_b.join("src").to_string_lossy().to_string()),
        );
        let rejected_files_b = call_text!("atlas_files", absolute_files_b_args.clone());
        if rejected_files_b.contains("src/lib.rs")
            || !rejected_files_b.contains("indexed ProjectAtlas project")
            || !rejected_files_b.contains("Get-Content")
        {
            return Err(format!(
                "default-off atlas_files routed another project folder: {rejected_files_b}"
            )
            .into());
        }
        absolute_files_b_args.insert("nearest_project".to_string(), json!(true));
        let absolute_files_b = call_text!("atlas_files", absolute_files_b_args);
        if !absolute_files_b.contains("src/lib.rs")
            || absolute_files_b.contains("alpha_project_a_marker")
        {
            return Err(
                "absolute folder path did not route file ranking to indexed repo B with override"
                    .into(),
            );
        }
        require_selected_project_audit(
            &absolute_files_b,
            &repo_b,
            &db_b,
            "nearest-routed file ranking",
        )?;

        let mut explicit_project_summary_args = Map::new();
        explicit_project_summary_args.insert(
            "project_path".to_string(),
            json!(repo_a.to_string_lossy().to_string()),
        );
        explicit_project_summary_args.insert(
            "file".to_string(),
            json!(
                repo_b
                    .join("src")
                    .join("lib.rs")
                    .to_string_lossy()
                    .to_string()
            ),
        );
        explicit_project_summary_args.insert("nearest_project".to_string(), json!(true));
        let explicit_project_summary =
            call_text!("atlas_file_summary", explicit_project_summary_args);
        if explicit_project_summary.contains("beta_project_b_marker")
            || !explicit_project_summary.contains("indexed ProjectAtlas project")
            || !explicit_project_summary.contains("Get-Content")
        {
            return Err(format!(
                "explicit project_path did not stay isolated from nearest routing: {explicit_project_summary}"
            )
            .into());
        }

        let nested_active_repo = repo_a.join("nested-active-project");
        fs::create_dir_all(nested_active_repo.join("src"))?;
        fs::write(
            nested_active_repo.join("src").join("lib.rs"),
            "pub fn nested_active_marker() { nested_active_helper(); }\nfn nested_active_helper() {}\n",
        )?;
        let mut scan_nested_active_args = Map::new();
        scan_nested_active_args.insert(
            "project_path".to_string(),
            json!(nested_active_repo.to_string_lossy().to_string()),
        );
        let scan_nested_active = call_text!("atlas_scan", scan_nested_active_args);
        if !scan_nested_active.contains("scan:") {
            return Err("project_path-selected atlas_scan did not scan nested active repo".into());
        }
        let nested_active_db = nested_active_repo
            .join(".projectatlas")
            .join("projectatlas.db");
        let nested_active_file = nested_active_repo.join("src").join("lib.rs");
        let mut nested_active_summary_args = Map::new();
        nested_active_summary_args.insert(
            "file".to_string(),
            json!(nested_active_file.to_string_lossy().to_string()),
        );
        let rejected_nested_active =
            call_text!("atlas_file_summary", nested_active_summary_args.clone());
        if rejected_nested_active.contains("nested_active_marker") {
            return Err("default-off routing read nested active child through nearest DB".into());
        }
        nested_active_summary_args.insert("nearest_project".to_string(), json!(true));
        let nested_active_summary =
            call_text!("atlas_file_summary", nested_active_summary_args.clone());
        if !nested_active_summary.contains("nested_active_marker")
            || !nested_active_summary.contains("file_path: src/lib.rs")
            || nested_active_summary.contains("nested-active-project/src/lib.rs")
        {
            return Err(format!(
                "nearest routing did not prefer nested child DB under active root: {nested_active_summary}"
            )
            .into());
        }
        require_selected_project_audit(
            &nested_active_summary,
            &nested_active_repo,
            &nested_active_db,
            "nearest-routed nested summary",
        )?;
        let nested_active_outline = call_text!("atlas_outline", nested_active_summary_args.clone());
        if !nested_active_outline.contains("nested_active_marker") {
            return Err("atlas_outline did not route to nested child DB".into());
        }
        require_selected_project_audit(
            &nested_active_outline,
            &nested_active_repo,
            &nested_active_db,
            "nearest-routed nested outline",
        )?;
        let mut nested_active_slice_args = nested_active_summary_args.clone();
        nested_active_slice_args.insert("start_line".to_string(), json!(1));
        nested_active_slice_args.insert("end_line".to_string(), json!(1));
        let nested_active_slice = call_text!("atlas_slice", nested_active_slice_args);
        if !nested_active_slice.contains("nested_active_marker") {
            return Err("atlas_slice did not route to nested child DB".into());
        }
        require_selected_project_audit(
            &nested_active_slice,
            &nested_active_repo,
            &nested_active_db,
            "nearest-routed nested slice",
        )?;
        let mut nested_active_symbols_args = Map::new();
        nested_active_symbols_args.insert(
            "file".to_string(),
            json!(nested_active_file.to_string_lossy().to_string()),
        );
        nested_active_symbols_args.insert("query".to_string(), json!("nested_active_marker"));
        nested_active_symbols_args.insert("nearest_project".to_string(), json!(true));
        let nested_active_symbols = call_text!("atlas_symbols", nested_active_symbols_args.clone());
        if !nested_active_symbols.contains("nested_active_marker") {
            return Err("atlas_symbols did not route to nested child DB".into());
        }
        require_selected_project_audit(
            &nested_active_symbols,
            &nested_active_repo,
            &nested_active_db,
            "nearest-routed nested symbols",
        )?;
        let nested_active_relations =
            call_text!("atlas_symbol_relations", nested_active_symbols_args);
        if !nested_active_relations.contains("nested_active_marker") {
            return Err("atlas_symbol_relations did not route to nested child DB".into());
        }
        require_selected_project_audit(
            &nested_active_relations,
            &nested_active_repo,
            &nested_active_db,
            "nearest-routed nested symbol relations",
        )?;
        let mut nested_active_files_args = Map::new();
        nested_active_files_args.insert("query".to_string(), json!("nested_active_marker"));
        nested_active_files_args.insert(
            "folder".to_string(),
            json!(nested_active_repo.join("src").to_string_lossy().to_string()),
        );
        nested_active_files_args.insert("include_content".to_string(), json!(true));
        nested_active_files_args.insert("nearest_project".to_string(), json!(true));
        let nested_active_files = call_text!("atlas_files", nested_active_files_args);
        if !nested_active_files.contains("src/lib.rs")
            || nested_active_files.contains("nested-active-project/src/lib.rs")
        {
            return Err("atlas_files did not route folder filter to nested child DB".into());
        }
        require_selected_project_audit(
            &nested_active_files,
            &nested_active_repo,
            &nested_active_db,
            "nearest-routed nested file ranking",
        )?;

        let empty_repo = temp.path().join("repo-empty");
        fs::create_dir_all(empty_repo.join("src"))?;
        fs::write(
            empty_repo.join("src").join("lib.rs"),
            "pub fn unindexed_project_marker() {}\n",
        )?;
        let mut missing_index_file_args = Map::new();
        missing_index_file_args.insert(
            "file".to_string(),
            json!(
                empty_repo
                    .join("src")
                    .join("lib.rs")
                    .to_string_lossy()
                    .to_string()
            ),
        );
        missing_index_file_args.insert("nearest_project".to_string(), json!(true));
        let missing_index_file = call_text!("atlas_file_summary", missing_index_file_args);
        if !missing_index_file.contains("indexed ProjectAtlas project")
            || !missing_index_file.contains("Get-Content")
            || empty_repo.join(".projectatlas").exists()
        {
            return Err(
                "absolute file routing did not fail cleanly when no ancestor DB exists".into(),
            );
        }

        let partial_repo = temp.path().join("repo-partial-atlas");
        fs::create_dir_all(partial_repo.join(".projectatlas"))?;
        fs::create_dir_all(partial_repo.join("src"))?;
        fs::write(
            partial_repo.join("src").join("lib.rs"),
            "pub fn partial_project_marker() {}\n",
        )?;
        let mut partial_index_file_args = Map::new();
        partial_index_file_args.insert(
            "file".to_string(),
            json!(
                partial_repo
                    .join("src")
                    .join("lib.rs")
                    .to_string_lossy()
                    .to_string()
            ),
        );
        partial_index_file_args.insert("nearest_project".to_string(), json!(true));
        let partial_index_file = call_text!("atlas_file_summary", partial_index_file_args);
        if !partial_index_file.contains("indexed ProjectAtlas project")
            || !partial_index_file.contains("Get-Content")
            || partial_repo
                .join(".projectatlas")
                .join("projectatlas.db")
                .exists()
        {
            return Err(
                "nearest routing treated a .projectatlas folder without DB as indexed".into(),
            );
        }

        let invalid_db_repo = temp.path().join("repo-invalid-db");
        fs::create_dir_all(invalid_db_repo.join(".projectatlas"))?;
        fs::create_dir_all(invalid_db_repo.join("src"))?;
        fs::write(
            invalid_db_repo.join("src").join("lib.rs"),
            "pub fn invalid_db_project_marker() {}\n",
        )?;
        let invalid_db = invalid_db_repo
            .join(".projectatlas")
            .join("projectatlas.db");
        fs::write(&invalid_db, [])?;
        let mut invalid_db_args = Map::new();
        invalid_db_args.insert(
            "file".to_string(),
            json!(
                invalid_db_repo
                    .join("src")
                    .join("lib.rs")
                    .to_string_lossy()
                    .to_string()
            ),
        );
        invalid_db_args.insert("nearest_project".to_string(), json!(true));
        let invalid_db_summary = call_text!("atlas_file_summary", invalid_db_args);
        if !invalid_db_summary.contains("indexed ProjectAtlas project")
            || !invalid_db_summary.contains("Get-Content")
            || fs::metadata(&invalid_db)?.len() != 0
            || invalid_db.with_extension("db-wal").exists()
            || invalid_db.with_extension("db-shm").exists()
        {
            return Err(format!(
                "nearest routing mutated or accepted an invalid candidate DB: {invalid_db_summary}"
            )
            .into());
        }

        let nested_repo = repo_b.join("nested-project");
        fs::create_dir_all(nested_repo.join("src"))?;
        fs::write(
            nested_repo.join("src").join("lib.rs"),
            "pub fn nested_project_marker() {}\n",
        )?;
        let mut scan_nested_args = Map::new();
        scan_nested_args.insert(
            "project_path".to_string(),
            json!(nested_repo.to_string_lossy().to_string()),
        );
        let scan_nested = call_text!("atlas_scan", scan_nested_args);
        if !scan_nested.contains("scan:") {
            return Err("project_path-selected atlas_scan did not scan nested repo".into());
        }
        let mut nested_summary_args = Map::new();
        nested_summary_args.insert(
            "file".to_string(),
            json!(
                nested_repo
                    .join("src")
                    .join("lib.rs")
                    .to_string_lossy()
                    .to_string()
            ),
        );
        let rejected_nested_summary = call_text!("atlas_file_summary", nested_summary_args.clone());
        if rejected_nested_summary.contains("nested_project_marker")
            || !rejected_nested_summary.contains("indexed ProjectAtlas project")
            || !rejected_nested_summary.contains("Get-Content")
        {
            return Err(format!(
                "default-off nearest nested DB routing did not return filesystem guidance: {rejected_nested_summary}"
            )
            .into());
        }
        nested_summary_args.insert("nearest_project".to_string(), json!(true));
        let nested_summary = call_text!("atlas_file_summary", nested_summary_args.clone());
        if !nested_summary.contains("nested_project_marker")
            || !nested_summary.contains("file_path: src/lib.rs")
            || nested_summary.contains("nested-project/src/lib.rs")
        {
            return Err("nearest nested ProjectAtlas DB was not preferred".into());
        }
        fs::write(
            nested_repo.join("projectatlas.toml"),
            "[project]\nroot = \"..\"\n",
        )?;
        let nested_config_mismatch = call_text!("atlas_file_summary", nested_summary_args);
        if !nested_config_mismatch.contains("outside selected project root") {
            return Err("nearest DB routing did not reject config root mismatch".into());
        }

        let linked_source = repo_a.join("linked-src");
        match create_directory_symlink(&repo_a.join("src"), &linked_source) {
            Ok(()) => {
                let mut linked_source_args = Map::new();
                linked_source_args.insert(
                    "file".to_string(),
                    json!(linked_source.join("lib.rs").to_string_lossy().to_string()),
                );
                linked_source_args.insert("nearest_project".to_string(), json!(true));
                let linked_source_summary = call_text!("atlas_file_summary", linked_source_args);
                if !linked_source_summary.contains("alpha_project_a_marker")
                    || !linked_source_summary.contains("file_path: src/lib.rs")
                {
                    return Err(format!(
                        "nearest routing rejected a same-project symlink/junction alias: {linked_source_summary}"
                    )
                    .into());
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
                ) => {}
            Err(error) => return Err(error.into()),
        }

        let linked_repo_b = repo_a.join("linked-repo-b");
        match create_directory_symlink(&repo_b, &linked_repo_b) {
            Ok(()) => {
                let mut linked_summary_args = Map::new();
                linked_summary_args.insert(
                    "file".to_string(),
                    json!(
                        linked_repo_b
                            .join("src")
                            .join("lib.rs")
                            .to_string_lossy()
                            .to_string()
                    ),
                );
                linked_summary_args.insert("nearest_project".to_string(), json!(true));
                let linked_summary = call_text!("atlas_file_summary", linked_summary_args);
                if linked_summary.contains("beta_project_b_marker")
                    || !linked_summary.contains("symlink or junction")
                    || !linked_summary.contains("multiple plausible ProjectAtlas roots")
                    || !linked_summary.contains("Get-Content")
                {
                    return Err(format!(
                        "nearest routing did not reject symlink/junction ambiguity: {linked_summary}"
                    )
                    .into());
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
                ) => {}
            Err(error) => return Err(error.into()),
        }

        let mut search_b_args = Map::new();
        search_b_args.insert(
            "project_path".to_string(),
            json!(repo_b.to_string_lossy().to_string()),
        );
        search_b_args.insert("pattern".to_string(), json!("beta_project_b_marker"));
        let search_b = call_text!("atlas_search", search_b_args);
        if !search_b.contains("beta_project_b_marker") {
            return Err("per-call project_path search did not read repo B".into());
        }

        let mut default_search_a_args = Map::new();
        default_search_a_args.insert("pattern".to_string(), json!("alpha_project_a_marker"));
        let default_search_a = call_text!("atlas_search", default_search_a_args);
        if !default_search_a.contains("alpha_project_a_marker")
            || default_search_a.contains("beta_project_b_marker")
        {
            return Err("project_path-selected scan leaked into the active project".into());
        }

        let mut set_b_args = Map::new();
        set_b_args.insert(
            "project_path".to_string(),
            json!(repo_b.to_string_lossy().to_string()),
        );
        let set_b = call_text!("atlas_set_project_path", set_b_args);
        if !set_b.contains("project:") || !set_b.contains("status: active") {
            return Err("atlas_set_project_path did not report active project state".into());
        }

        let mut default_search_b_args = Map::new();
        default_search_b_args.insert("pattern".to_string(), json!("beta_project_b_marker"));
        let default_search_b = call_text!("atlas_search", default_search_b_args);
        if !default_search_b.contains("beta_project_b_marker")
            || default_search_b.contains("alpha_project_a_marker")
        {
            return Err("atlas_set_project_path did not switch the active project".into());
        }

        let mut override_search_a_args = Map::new();
        override_search_a_args.insert(
            "project_path".to_string(),
            json!(repo_a.to_string_lossy().to_string()),
        );
        override_search_a_args.insert("pattern".to_string(), json!("alpha_project_a_marker"));
        let override_search_a = call_text!("atlas_search", override_search_a_args);
        if !override_search_a.contains("alpha_project_a_marker") {
            return Err("per-call project_path search did not read repo A".into());
        }

        let mut missing_index_args = Map::new();
        missing_index_args.insert(
            "project_path".to_string(),
            json!(empty_repo.to_string_lossy().to_string()),
        );
        let missing_index_overview = call_text!("atlas_overview", missing_index_args);
        if !missing_index_overview.contains("index")
            || !missing_index_overview.contains("atlas_scan")
            || empty_repo.join(".projectatlas").exists()
        {
            return Err(
                "read-only per-call project_path did not fail cleanly for a missing index".into(),
            );
        }

        let mut still_default_b_args = Map::new();
        still_default_b_args.insert("pattern".to_string(), json!("beta_project_b_marker"));
        let still_default_b = call_text!("atlas_search", still_default_b_args);
        if !still_default_b.contains("beta_project_b_marker")
            || still_default_b.contains("alpha_project_a_marker")
        {
            return Err("per-call project_path override mutated active project state".into());
        }

        let mut mismatched_scan_args = Map::new();
        mismatched_scan_args.insert(
            "project_path".to_string(),
            json!(repo_b.to_string_lossy().to_string()),
        );
        mismatched_scan_args.insert(
            "path".to_string(),
            json!(repo_a.to_string_lossy().to_string()),
        );
        let mismatched_scan = call_text!("atlas_scan", mismatched_scan_args);
        if !mismatched_scan.contains("outside the selected project root") {
            return Err("atlas_scan did not reject mismatched project_path/path roots".into());
        }

        let default_runtime_info = call_text!("atlas_runtime_info", Map::new());
        if !default_runtime_info.contains("runtime:")
            || default_runtime_info.contains("mcp_nearest_project")
        {
            return Err(format!(
                "atlas_runtime_info should report runtime identity only: {default_runtime_info}"
            )
            .into());
        }
        let mut runtime_info_missing_project_args = Map::new();
        runtime_info_missing_project_args.insert(
            "project_path".to_string(),
            json!(empty_repo.to_string_lossy().to_string()),
        );
        let runtime_info_missing_project =
            call_text!("atlas_runtime_info", runtime_info_missing_project_args);
        if !runtime_info_missing_project.contains("runtime:")
            || runtime_info_missing_project.contains("mcp_nearest_project")
        {
            return Err(format!(
                "atlas_runtime_info should be project-agnostic: {runtime_info_missing_project}"
            )
            .into());
        }

        client.cancel().await?;
        server_handle.await?.map_err(std::io::Error::other)?;

        let server = ProjectAtlasMcpServer::new(
            db_a.clone(),
            None,
            "mcp-nearest-startup-test".to_string(),
            true,
        );
        let (server_transport, client_transport) = tokio::io::duplex(16_384);
        let server_handle = tokio::spawn(async move {
            server
                .serve(server_transport)
                .await
                .map_err(|error| error.to_string())?
                .waiting()
                .await
                .map_err(|error| error.to_string())?;
            Ok::<(), String>(())
        });
        let client = TestMcpClient.serve(client_transport).await?;

        macro_rules! call_text_on {
            ($tool:literal, $args:expr) => {{
                let result = client
                    .peer()
                    .call_tool(CallToolRequestParams::new($tool).with_arguments($args))
                    .await?;
                result
                    .content
                    .first()
                    .and_then(|content| content.raw.as_text())
                    .map(|text| text.text.clone())
                    .ok_or_else(|| {
                        std::io::Error::other(format!("{} result did not contain text", $tool))
                    })?
            }};
        }

        let startup_runtime_info = call_text_on!("atlas_runtime_info", Map::new());
        if !startup_runtime_info.contains("runtime:")
            || startup_runtime_info.contains("mcp_nearest_project")
        {
            return Err(format!(
                "atlas_runtime_info should remain identity-only when nearest-project startup is enabled: {startup_runtime_info}"
            )
            .into());
        }

        let mut startup_summary_b_args = Map::new();
        startup_summary_b_args.insert(
            "file".to_string(),
            json!(
                repo_b
                    .join("src")
                    .join("lib.rs")
                    .to_string_lossy()
                    .to_string()
            ),
        );
        let startup_summary_b = call_text_on!("atlas_file_summary", startup_summary_b_args.clone());
        if !startup_summary_b.contains("beta_project_b_marker")
            || !startup_summary_b.contains("file_path: src/lib.rs")
        {
            return Err(format!(
                "startup nearest-project setting did not route indexed repo B: {startup_summary_b}"
            )
            .into());
        }
        require_selected_project_audit(
            &startup_summary_b,
            &repo_b,
            &db_b,
            "startup nearest-routed file summary",
        )?;
        startup_summary_b_args.insert("nearest_project".to_string(), json!(false));
        let disabled_startup_summary_b =
            call_text_on!("atlas_file_summary", startup_summary_b_args);
        if disabled_startup_summary_b.contains("beta_project_b_marker")
            || !disabled_startup_summary_b.contains("indexed ProjectAtlas project")
            || !disabled_startup_summary_b.contains("Get-Content")
        {
            return Err(format!(
                "per-call nearest_project=false did not override startup setting: {disabled_startup_summary_b}"
            )
            .into());
        }

        let mut startup_partial_file_args = Map::new();
        startup_partial_file_args.insert(
            "file".to_string(),
            json!(
                partial_repo
                    .join("src")
                    .join("lib.rs")
                    .to_string_lossy()
                    .to_string()
            ),
        );
        let startup_partial_file = call_text_on!("atlas_file_summary", startup_partial_file_args);
        if !startup_partial_file.contains("indexed ProjectAtlas project")
            || !startup_partial_file.contains("Get-Content")
            || partial_repo
                .join(".projectatlas")
                .join("projectatlas.db")
                .exists()
        {
            return Err(format!(
                "startup nearest-project setting did not reject a project without DB: {startup_partial_file}"
            )
            .into());
        }

        client.cancel().await?;
        server_handle.await?.map_err(std::io::Error::other)?;
        Ok(())
    }
}
