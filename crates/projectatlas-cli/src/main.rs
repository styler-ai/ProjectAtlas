//! Purpose: Provide the `ProjectAtlas` 3 command-line adapter.

mod atlas_map;

use atlas_map::{
    LintOptions, imported_purpose_records, init_project, lint_map, load_atlas_config,
    seed_purpose_files, write_map,
};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use projectatlas_core::outline::{build_outline, estimate_tokens};
use projectatlas_core::symbols::{RelationKind, SymbolGraph, SymbolKind};
use projectatlas_core::telemetry::{TokenOverview, usage_from_estimates, usage_from_text};
use projectatlas_core::toon::{
    encode_agent_payload, render_health, render_nodes, render_outline, render_overview,
    render_symbol_relations, render_symbols, render_token_overview,
};
use projectatlas_core::{
    Node, NodeKind, PurposeSource, PurposeStatus, normalize_repo_path, repo_path_to_native,
    validated_repo_file_key,
};
use projectatlas_db::{AtlasStore, DbError, HealthResolution, IndexedFileText};
use projectatlas_fs::{ScanOptions, scan_path, scan_repo};
use projectatlas_service::{
    CodeSlice, FileSummaryReport, SearchReport, SymbolSliceSelector, build_file_summary,
    file_path_matches_glob, file_summary_baseline_text, read_indexed_code_slice, read_symbol_slice,
    search_indexed_files,
};
use projectatlas_symbols::extract_symbol_graph;
use rayon::ThreadPoolBuilder;
use rayon::prelude::*;
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::schemars;
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Default relative path for the `SQLite` index.
const DEFAULT_DB_PATH: &str = ".projectatlas/projectatlas.db";
/// Default session identifier for token telemetry.
const DEFAULT_SESSION_ID: &str = "default";
/// Maximum file size parsed for symbols by default.
const MAX_SYMBOL_FILE_BYTES: u64 = 2_000_000;
/// Default maximum rows returned per structured file-summary section.
const DEFAULT_FILE_SUMMARY_LIMIT: usize = 25;
/// One-shot watcher refresh mode.
const WATCH_MODE_ONCE: &str = "single-refresh";
/// Event-backed watcher mode.
const WATCH_MODE_NOTIFY: &str = "notify";
/// Portable fallback watcher mode.
const WATCH_MODE_POLLING: &str = "portable-polling";
/// MCP tools required for the agent-first repository-intelligence surface.
const REQUIRED_MCP_TOOL_NAMES: &[&str] = &[
    "atlas_scan",
    "atlas_overview",
    "atlas_folders",
    "atlas_files",
    "atlas_outline",
    "atlas_file_summary",
    "atlas_search",
    "atlas_slice",
    "atlas_symbols_build",
    "atlas_symbols",
    "atlas_symbol_relations",
    "atlas_health",
    "atlas_health_resolve",
    "atlas_token_report",
    "atlas_parity_report",
    "atlas_settings",
    "atlas_watch_status",
    "atlas_watch_once",
    "atlas_strip_legacy_purpose",
    "atlas_reset_index",
    "atlas_purpose_set",
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

/// Top-level parsed CLI arguments.
#[derive(Debug, Parser)]
#[command(name = "projectatlas")]
#[command(about = "ProjectAtlas 3 repository intelligence engine")]
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
    /// Subcommand to execute.
    #[command(subcommand)]
    command: Command,
}

/// Supported `ProjectAtlas` CLI commands.
#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize `ProjectAtlas` files in a repository.
    Init {
        /// Create missing folder purpose files after initialization.
        #[arg(long)]
        seed_purpose: bool,
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
    /// Create missing folder purpose files.
    SeedPurpose,
    /// Scan a repository and replace the durable index.
    Scan {
        /// Repository root to scan.
        #[arg(default_value = ".")]
        path: PathBuf,
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
        /// Maximum number of files to return.
        #[arg(long, default_value_t = 10)]
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
    },
    /// Report structural health findings.
    HealthCheck,
    /// Resolve a deterministic health finding with agent rationale.
    Health {
        /// Health subcommand to run.
        #[command(subcommand)]
        command: HealthCommand,
    },
    /// Validate `ProjectAtlas` map, purpose summaries, and structure drift.
    Lint {
        /// Fail when folder purpose files are missing.
        #[arg(long)]
        strict_folders: bool,
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
    },
    /// Check repository-intelligence parity readiness.
    Parity {
        /// Parity subcommand to run.
        #[command(subcommand)]
        command: ParityCommand,
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
    Mcp,
    /// Print a project-local MCP configuration with absolute runtime paths.
    McpConfig {
        /// MCP server name to emit.
        #[arg(long, default_value = "projectatlas")]
        server_name: String,
    },
    /// Manage purpose metadata stored in the durable index.
    Purpose {
        /// Purpose subcommand to run.
        #[command(subcommand)]
        command: PurposeCommand,
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
}

/// Parity gate subcommands.
#[derive(Debug, Subcommand)]
enum ParityCommand {
    /// Report whether the current index satisfies a parity profile.
    Report {
        /// Parity profile to evaluate.
        #[arg(long, default_value = "repository-intelligence")]
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
    ensure_parent_dir(&cli.db)?;
    match &cli.command {
        Command::Init { seed_purpose } => {
            let root = std::env::current_dir().map_err(|source| CliError::Io {
                path: PathBuf::from("."),
                source,
            })?;
            let report = init_project(&root, *seed_purpose)?;
            write_stdout(&report)?;
        }
        Command::Map { json, force } => {
            if !force && (truthy_env("CI") || truthy_env("GITHUB_ACTIONS")) {
                write_stderr("Skipping ProjectAtlas map update in CI.\n")?;
                return Ok(());
            }
            let config = load_atlas_config(cli.config.as_deref())?;
            write_map(&config, *json)?;
        }
        Command::SeedPurpose => {
            let config = load_atlas_config(cli.config.as_deref())?;
            let created = seed_purpose_files(&config)?;
            write_stderr(&format!("Seeded {created} .purpose files.\n"))?;
        }
        Command::Scan { path } => {
            let root = canonical_project_root(path)?;
            let scan_config = load_scan_import_config(cli.config.as_deref(), &root)?;
            let scan_options = scan_config.as_ref().map_or_else(
                ScanOptions::default,
                atlas_map::AtlasMapConfig::scan_options,
            );
            let nodes = scan_repo(&root, &scan_options)?;
            let mut store = AtlasStore::open(&cli.db)?;
            store.set_project_root(&root)?;
            store.replace_scan(&nodes)?;
            let text_index = refresh_text_index_for_nodes(&mut store, &root, &nodes)?;
            if let Some(config) = scan_config.as_ref() {
                for record in imported_purpose_records(config)? {
                    store.set_purpose(&record.path, &record.summary, PurposeSource::Imported)?;
                }
            }
            let symbol_options = SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None);
            let symbol_report = build_symbols_for_index(&mut store, &root, &symbol_options, None)?;
            let overview = store.overview()?;
            let report = ScanReport {
                overview,
                text_index,
                symbols: symbol_report,
            };
            print_output(
                cli.format,
                &encode_agent_payload(&json!({ "scan": report })),
                &report,
            )?;
        }
        Command::Overview => {
            let store = AtlasStore::open(&cli.db)?;
            let overview = store.overview()?;
            let toon = render_overview(&overview);
            print_tracked_output_estimate(
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
            let store = AtlasStore::open(&cli.db)?;
            let selected = store.load_ranked_nodes(query, NodeKind::Folder, None, *limit, 0)?;
            let toon = render_nodes("folders", &selected);
            print_tracked_output_estimate(
                cli.format,
                &store,
                &cli.session,
                "folders",
                None,
                Some(query.clone()),
                estimated_source_tokens_for_indexed_files(&store, None, None)?,
                &toon,
                &selected,
            )?;
        }
        Command::Files {
            query,
            folder,
            file_pattern,
            limit,
        } => {
            let store = AtlasStore::open(&cli.db)?;
            let query_text = query.as_deref().unwrap_or("");
            let folder_filter = folder
                .as_deref()
                .map(normalized_folder_filter)
                .transpose()?;
            let selected = ranked_file_nodes(
                &store,
                query_text,
                folder_filter.as_deref(),
                file_pattern.as_deref(),
                *limit,
            )?;
            let baseline_tokens = estimated_source_tokens_for_indexed_files(
                &store,
                folder_filter.as_deref(),
                file_pattern.as_deref(),
            )?;
            let toon = render_nodes("files", &selected);
            print_tracked_output_estimate(
                cli.format,
                &store,
                &cli.session,
                "files",
                file_pattern.clone().or(folder_filter),
                query.clone(),
                baseline_tokens,
                &toon,
                &selected,
            )?;
        }
        Command::Outline { file, lines } => {
            let store = AtlasStore::open(&cli.db)?;
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
            let store = AtlasStore::open(&cli.db)?;
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
            let store = AtlasStore::open(&cli.db)?;
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
            let store = AtlasStore::open(&cli.db)?;
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
                let mut store = AtlasStore::open(&cli.db)?;
                let options = SymbolBuildOptions::new(*max_bytes, *max_workers, *timeout_seconds);
                let report = build_symbols_for_index(&mut store, path, &options, None)?;
                print_output(
                    cli.format,
                    &encode_agent_payload(&json!({ "symbols_build": report })),
                    &report,
                )?;
            }
            SymbolsCommand::List { file, query, limit } => {
                let store = AtlasStore::open(&cli.db)?;
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
                let store = AtlasStore::open(&cli.db)?;
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
                let store = AtlasStore::open(&cli.db)?;
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
        } => {
            let mut store = AtlasStore::open(&cli.db)?;
            let root = canonical_project_root(path)?;
            let scan_options = scan_options_for_root(cli.config.as_deref(), &root)?;
            let symbol_options =
                SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, *max_workers, *timeout_seconds);
            let report = run_watch_loop(
                &mut store,
                &root,
                *once,
                *poll_seconds,
                *max_cycles,
                &symbol_options,
                &scan_options,
            )?;
            print_output(
                cli.format,
                &encode_agent_payload(&json!({ "watch": report })),
                &report,
            )?;
        }
        Command::HealthCheck => {
            let store = AtlasStore::open(&cli.db)?;
            let findings = store.unresolved_health_findings(&store.resolved_health_ids()?)?;
            let toon = render_health(&findings);
            print_tracked_output_estimate(
                cli.format,
                &store,
                &cli.session,
                "health-check",
                None,
                None,
                estimated_source_tokens_for_indexed_files(&store, None, None)?,
                &toon,
                &findings,
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
                let store = AtlasStore::open(&cli.db)?;
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
            report_untracked,
            strict_untracked,
        } => {
            let config = load_atlas_config(cli.config.as_deref())?;
            let (mut report, mut exit_code) = lint_map(
                &config,
                LintOptions {
                    strict_folders: *strict_folders,
                    report_untracked: *report_untracked,
                    strict_untracked: *strict_untracked,
                },
            )?;
            let (db_report, db_exit_code) = lint_database_if_present(&cli.db)?;
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
        Command::Token { session, view } => {
            let store = AtlasStore::open(&cli.db)?;
            let overview = store.token_overview(session.as_deref())?;
            match view {
                TokenView::Agent => {
                    print_output(cli.format, &render_token_overview(&overview), &overview)?;
                }
                TokenView::Tui => {
                    write_stdout(&render_token_dashboard(&overview, session.as_deref()))?;
                }
            }
        }
        Command::Parity { command } => match command {
            ParityCommand::Report { profile } => {
                let store = AtlasStore::open(&cli.db)?;
                let report = build_parity_report(&store, profile)?;
                let ok = report.ok;
                print_output(cli.format, &render_parity_report(&report), &report)?;
                if !ok {
                    std::process::exit(1);
                }
            }
        },
        Command::StripLegacyPurpose {
            path,
            apply,
            dry_run,
            strip_source_headers,
        } => {
            let report = strip_legacy_purpose(
                path,
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
        Command::Mcp => {
            run_mcp_server(&cli)?;
        }
        Command::McpConfig { server_name } => {
            let report = build_mcp_config_report(server_name, &cli.db, cli.config.as_deref())?;
            print_output(cli.format, &render_mcp_config_report(&report), &report)?;
        }
        Command::Purpose { command } => match command {
            PurposeCommand::Set { path, purpose } => {
                let store = AtlasStore::open(&cli.db)?;
                store.set_purpose(path, purpose, PurposeSource::Agent)?;
                write_stdout(&format!("purpose set: {path}\n"))?;
            }
        },
    }
    Ok(())
}

/// Build a standards-compliant MCP configuration document for this binary.
fn build_mcp_config_report(
    server_name: &str,
    db: &Path,
    config: Option<&Path>,
) -> Result<McpConfigDocument, CliError> {
    let executable = std::env::current_exe().map_err(|source| CliError::Io {
        path: PathBuf::from("current executable"),
        source,
    })?;
    let mut args = vec!["--db".to_string(), absolute_path(db)?.display().to_string()];
    if let Some(config_path) = resolved_mcp_config_path(config)? {
        args.push("--config".to_string());
        args.push(config_path.display().to_string());
    }
    args.push("mcp".to_string());
    let mut mcp_servers = BTreeMap::new();
    mcp_servers.insert(
        server_name.to_string(),
        McpServerConfig {
            command: executable.display().to_string(),
            args,
        },
    );
    Ok(McpConfigDocument { mcp_servers })
}

/// Resolve the config path that should travel with generated MCP configs.
fn resolved_mcp_config_path(config: Option<&Path>) -> Result<Option<PathBuf>, CliError> {
    if let Some(path) = config {
        return Ok(Some(absolute_path(path)?));
    }
    let current_dir = std::env::current_dir().map_err(|source| CliError::Io {
        path: PathBuf::from("."),
        source,
    })?;
    for candidate in [
        current_dir.join(".projectatlas").join("config.toml"),
        current_dir.join("projectatlas.toml"),
    ] {
        if candidate.exists() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// Return an absolute path without requiring the target to exist.
fn absolute_path(path: &Path) -> Result<PathBuf, CliError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let current_dir = std::env::current_dir().map_err(|source| CliError::Io {
        path: PathBuf::from("."),
        source,
    })?;
    Ok(current_dir.join(path))
}

/// Render MCP configuration as TOON for agents.
fn render_mcp_config_report(report: &McpConfigDocument) -> String {
    encode_agent_payload(&json!({ "mcp_config": report }))
}

/// Run the official RMCP stdio server.
fn run_mcp_server(cli: &Cli) -> Result<(), CliError> {
    let server =
        ProjectAtlasMcpServer::new(cli.db.clone(), cli.config.clone(), cli.session.clone());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_time()
        .build()
        .map_err(|source| CliError::Mcp(source.to_string()))?;
    runtime.block_on(async move {
        server
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|source| CliError::Mcp(source.to_string()))?
            .waiting()
            .await
            .map_err(|source| CliError::Mcp(source.to_string()))
            .map(|_| ())
    })
}

/// Return whether an environment variable is set to a truthy value.
fn truthy_env(name: &str) -> bool {
    std::env::var(name)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// Create the parent directory for a path when it has one.
fn ensure_parent_dir(path: &Path) -> Result<(), CliError> {
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

/// Return a canonical absolute project root.
fn canonical_project_root(root: &Path) -> Result<PathBuf, CliError> {
    root.canonicalize().map_err(|source| CliError::Io {
        path: root.to_path_buf(),
        source,
    })
}

/// Load map configuration for purpose import during scan.
fn load_scan_import_config(
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

/// Load scan options for a project root from `ProjectAtlas` config when present.
fn scan_options_for_root(config_path: Option<&Path>, root: &Path) -> Result<ScanOptions, CliError> {
    Ok(load_scan_import_config(config_path, root)?
        .as_ref()
        .map_or_else(
            ScanOptions::default,
            atlas_map::AtlasMapConfig::scan_options,
        ))
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

/// Record estimated-token telemetry for the exact emitted CLI payload.
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

/// Record a usage event from a fast baseline estimate and actual atlas payload.
fn record_usage_estimate(
    store: &AtlasStore,
    session: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    estimated_without_projectatlas: usize,
    projectatlas_text: &str,
) -> Result<(), CliError> {
    if telemetry_disabled() {
        return Ok(());
    }
    store.record_usage(&usage_from_estimates(
        session,
        command,
        path,
        query,
        estimated_without_projectatlas,
        estimate_tokens(projectatlas_text),
    ))?;
    Ok(())
}

/// Record a usage event from baseline and emitted text unless telemetry is disabled.
fn record_usage_text(
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
fn telemetry_disabled() -> bool {
    truthy_env("PROJECTATLAS_NO_TELEMETRY")
}

/// Estimate broad source tokens represented by indexed files with SQL aggregates.
fn estimated_source_tokens_for_indexed_files(
    store: &AtlasStore,
    folder: Option<&str>,
    file_pattern: Option<&str>,
) -> Result<usize, CliError> {
    validate_file_pattern(file_pattern)?;
    let mut total = 0usize;
    store.visit_file_token_estimates(folder, |path, size_bytes| {
        if file_path_matches_glob(&path, file_pattern).unwrap_or(false) {
            total =
                total.saturating_add(estimated_source_tokens_for_file_metadata(&path, size_bytes));
        }
        Ok(true)
    })?;
    Ok(total)
}

/// Estimate source tokens for one indexed file without reading it.
fn estimated_source_tokens_for_file_node(node: &Node) -> usize {
    estimated_source_tokens_for_file_metadata(&node.path, node.size_bytes)
}

/// Estimate source tokens for persisted file metadata.
fn estimated_source_tokens_for_file_metadata(path: &str, size_bytes: Option<u64>) -> usize {
    size_bytes.map_or_else(|| estimate_tokens(path), byte_size_to_tokens)
}

/// Estimate source tokens from a byte count with the shared token heuristic.
fn byte_size_to_tokens(bytes: u64) -> usize {
    let token_estimate = bytes.div_ceil(4);
    usize::try_from(token_estimate).unwrap_or(usize::MAX)
}

/// Estimate source tokens from a searched byte count.
fn byte_count_to_tokens(bytes: usize) -> usize {
    if bytes == 0 { 0 } else { bytes.div_ceil(4) }
}

/// Validate an optional file glob through the shared service matcher.
fn validate_file_pattern(file_pattern: Option<&str>) -> Result<(), CliError> {
    let _matches = file_path_matches_glob("", file_pattern)?;
    Ok(())
}

/// Load ranked file nodes in bounded pages and apply exact glob semantics.
fn ranked_file_nodes(
    store: &AtlasStore,
    query: &str,
    folder: Option<&str>,
    file_pattern: Option<&str>,
    limit: usize,
) -> Result<Vec<projectatlas_core::IndexedNode>, CliError> {
    validate_file_pattern(file_pattern)?;
    if file_pattern.is_none_or(|pattern| pattern.trim().is_empty() || pattern.trim() == "*") {
        return Ok(store.load_ranked_nodes(query, NodeKind::File, folder, limit.max(1), 0)?);
    }
    let target = limit.max(1);
    let batch_size = target.saturating_mul(20).clamp(50, 500);
    let mut offset = 0usize;
    let mut selected = Vec::new();
    loop {
        let batch = store.load_ranked_nodes(query, NodeKind::File, folder, batch_size, offset)?;
        if batch.is_empty() {
            break;
        }
        offset = offset.saturating_add(batch.len());
        for node in batch {
            if file_path_matches_glob(&node.node.path, file_pattern)? {
                selected.push(node);
                if selected.len() >= target {
                    return Ok(selected);
                }
            }
        }
    }
    Ok(selected)
}

/// Estimate source tokens for repository paths referenced by symbols/relations.
fn estimated_source_tokens_for_paths<'a>(
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
fn estimated_source_tokens_for_path(store: &AtlasStore, path: &str) -> Result<usize, CliError> {
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
fn file_summary_usage_baseline(
    store: &AtlasStore,
    report: &FileSummaryReport,
) -> Result<String, CliError> {
    read_indexed_file_content(store, &report.file_path)
        .or_else(|_| file_summary_baseline_text(report).map_err(CliError::from))
}

/// Scan command report.
#[derive(Debug, Serialize)]
struct ScanReport {
    /// Repository overview after scan.
    overview: projectatlas_core::Overview,
    /// Persisted text search index report.
    text_index: TextIndexReport,
    /// Symbol graph build report.
    symbols: SymbolBuildReport,
}

/// Persisted file-text index report.
#[derive(Clone, Debug, Serialize)]
struct TextIndexReport {
    /// File nodes considered for indexed text.
    candidates: usize,
    /// UTF-8 files persisted for SQLite-backed search.
    indexed: usize,
    /// Files skipped because text could not be decoded as UTF-8.
    binary_or_non_utf8: usize,
    /// Source bytes stored in the text index.
    bytes: usize,
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

/// One parity check row.
#[derive(Debug, Serialize)]
struct ParityCheck {
    /// Stable check name.
    name: String,
    /// `pass` or `fail`.
    status: String,
    /// Concrete evidence for this check.
    detail: String,
}

/// Symbol graph build report.
#[derive(Debug, Serialize)]
struct SymbolBuildReport {
    /// Indexed file candidates considered for symbols.
    candidates: usize,
    /// Files parsed during this build.
    parsed: usize,
    /// Files skipped because they were unchanged and already had symbols.
    unchanged: usize,
    /// Files skipped because they exceeded the configured size limit.
    too_large: usize,
    /// Files skipped because content was not valid UTF-8.
    binary_or_non_utf8: usize,
    /// Files skipped because the build deadline was reached.
    timed_out: usize,
    /// Worker thread count requested for parser work.
    max_workers: usize,
    /// Optional timeout seconds requested for parser work.
    timeout_seconds: Option<u64>,
    /// Symbols persisted.
    symbols: usize,
    /// Relations persisted.
    relations: usize,
    /// Node summaries refreshed from symbol graphs.
    summaries: usize,
    /// Generated purpose suggestions that still need agent review.
    purpose_suggestions: usize,
}

/// Watch command report.
#[derive(Debug, Serialize)]
struct WatchReport {
    /// Watcher mode.
    mode: String,
    /// Completed refresh cycles.
    cycles: usize,
    /// Whether the command ran a single refresh and exited.
    once: bool,
    /// Reason the watcher fell back from event mode, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    fallback_reason: Option<String>,
    /// Last persisted text search index report.
    text_index: TextIndexReport,
    /// Last symbol refresh report.
    last_symbols: SymbolBuildReport,
}

/// Debounced filesystem changes observed by watcher mode.
#[derive(Debug, Default)]
struct WatchChangeSet {
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
struct LegacyPurposeReport {
    /// Whether files were modified.
    applied: bool,
    /// Number of `.purpose` files found.
    purpose_files_found: usize,
    /// Number of `.purpose` files removed.
    purpose_files_removed: usize,
    /// Source header candidates found.
    source_header_candidates: Vec<String>,
    /// Legacy purpose file paths.
    purpose_files: Vec<String>,
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
}

/// Local settings report.
#[derive(Debug, Serialize)]
struct SettingsReport {
    /// Runtime cache directory that owns local `ProjectAtlas` state.
    cache_dir: PathStatus,
    /// `SQLite` database file status.
    db: PathStatus,
    /// `SQLite` write-ahead log file status.
    db_wal: PathStatus,
    /// `SQLite` shared-memory sidecar file status.
    db_shm: PathStatus,
    /// `SQLite` rollback journal sidecar file status.
    db_journal: PathStatus,
    /// Project-local MCP configuration file status.
    mcp_config: PathStatus,
    /// Config file used for map/lint/scan imports, when discovered.
    config_path: Option<String>,
    /// Repository root used by map/lint config.
    repo_root: String,
    /// Generated map path.
    map_path: String,
    /// Non-source summary path.
    nonsource_files_path: String,
    /// Default output format.
    default_format: String,
    /// Default search case sensitivity.
    default_search_case_sensitive: bool,
    /// Source used by search commands.
    search_source: String,
    /// Watcher runtime status.
    watcher: WatchStatusReport,
    /// Current index statistics, if the index exists.
    index: Option<SettingsIndexStats>,
}

/// Filesystem status for a diagnostic path.
#[derive(Debug, Serialize)]
struct PathStatus {
    /// Normalized native path.
    path: String,
    /// Whether the path exists.
    exists: bool,
    /// File size in bytes when the path is an existing file.
    size_bytes: Option<u64>,
}

/// Indexed state summary for settings diagnostics.
#[derive(Debug, Serialize)]
struct SettingsIndexStats {
    /// Canonical project root stored in the index metadata.
    project_root: Option<String>,
    /// Indexed file count.
    files: usize,
    /// Indexed folder count.
    folders: usize,
    /// Missing purpose count.
    missing_purposes: usize,
    /// Stale purpose count.
    stale_purposes: usize,
    /// Suggested purpose count.
    suggested_purposes: usize,
    /// Persisted searchable text rows.
    indexed_text_files: usize,
    /// Persisted searchable text bytes.
    indexed_text_bytes: usize,
    /// Persisted symbol count.
    symbols: usize,
    /// Persisted symbol relation count.
    relations: usize,
    /// Token telemetry event count.
    token_calls: usize,
    /// Unresolved structural health finding count.
    health_findings: usize,
}

/// Watcher status report.
#[derive(Debug, Serialize)]
struct WatchStatusReport {
    /// Whether a watcher implementation is available in this binary.
    available: bool,
    /// Whether a watcher is active.
    active: bool,
    /// Watcher mode.
    mode: String,
    /// Whether event-backed watching is available.
    event_backend_available: bool,
    /// Operational recommendation.
    recommendation: String,
}

/// Runtime index/cache cleanup report.
#[derive(Debug, Serialize)]
struct ResetIndexReport {
    /// Whether files were modified.
    applied: bool,
    /// Whether the command only previewed paths.
    dry_run: bool,
    /// Runtime files selected for cleanup.
    files: Vec<PathStatus>,
    /// Number of selected files removed.
    removed: usize,
}

/// MCP parameter payload for tools that accept a repository path.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasPathParams {
    /// Repository root path. Defaults to the MCP process working directory.
    path: Option<String>,
}

/// MCP parameter payload for scanning and symbol refresh.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasScanParams {
    /// Repository root path. Defaults to the MCP process working directory.
    path: Option<String>,
    /// Maximum file size to parse for symbols.
    max_bytes: Option<u64>,
    /// Maximum parser worker threads.
    max_workers: Option<usize>,
    /// Stop starting parser work after this many seconds.
    timeout_seconds: Option<u64>,
}

/// MCP parameter payload for ranked node lookup.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasQueryParams {
    /// Search query for path and purpose matching.
    query: Option<String>,
    /// Folder path to constrain file lookup.
    folder: Option<String>,
    /// Optional repository-relative glob filter.
    file_pattern: Option<String>,
    /// Maximum number of rows to return.
    limit: Option<usize>,
}

/// MCP parameter payload for outlining a file.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasOutlineParams {
    /// Repository-relative file path.
    file: String,
    /// Number of non-empty preview lines to include.
    lines: Option<usize>,
}

/// MCP parameter payload for deterministic file summaries.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasFileSummaryParams {
    /// Repository-relative file path.
    file: String,
    /// Maximum rows per functions/methods/classes/types/calls section.
    limit: Option<usize>,
}

/// MCP parameter payload for text search.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasSearchParams {
    /// Literal, regex, or fuzzy pattern to search for.
    pattern: String,
    /// Treat the pattern as a regex.
    regex: Option<bool>,
    /// Treat the pattern as a fuzzy subsequence.
    fuzzy: Option<bool>,
    /// Match case-sensitively.
    case_sensitive: Option<bool>,
    /// Optional repository-relative glob filter.
    file_pattern: Option<String>,
    /// Number of context lines before and after a match.
    context_lines: Option<usize>,
    /// Pagination start index.
    start_index: Option<usize>,
    /// Maximum matches to return.
    limit: Option<usize>,
}

/// MCP parameter payload for exact source slices.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasSliceParams {
    /// Repository-relative file path.
    file: String,
    /// One-based start line when no symbol is supplied.
    start_line: Option<usize>,
    /// Optional one-based end line.
    end_line: Option<usize>,
    /// Symbol name to slice instead of line numbers.
    symbol: Option<String>,
    /// Optional parent symbol for disambiguating `symbol`.
    symbol_parent: Option<String>,
    /// Optional symbol kind for disambiguating `symbol`.
    symbol_kind: Option<String>,
    /// Optional source line for disambiguating `symbol`.
    symbol_line: Option<usize>,
}

/// MCP parameter payload for symbol and relation lookup.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasSymbolsParams {
    /// Optional repository-relative file path.
    file: Option<String>,
    /// Optional symbol, signature, relation, or path query.
    query: Option<String>,
    /// Maximum rows to return.
    limit: Option<usize>,
}

/// MCP parameter payload for token savings reports.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasTokenParams {
    /// Optional session id filter.
    session: Option<String>,
}

/// MCP parameter payload for parity reports.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasParityParams {
    /// Parity profile. Defaults to repository-intelligence.
    profile: Option<String>,
}

/// MCP parameter payload for legacy purpose cleanup.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasStripLegacyParams {
    /// Repository root path. Defaults to the MCP process working directory.
    path: Option<String>,
    /// Remove legacy `.purpose` files when true.
    apply: Option<bool>,
    /// Preview cleanup without modifying files.
    dry_run: Option<bool>,
    /// Also report conservative source Purpose header candidates.
    strip_source_headers: Option<bool>,
}

/// MCP parameter payload for runtime index cleanup.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasResetIndexParams {
    /// Remove runtime index/cache files when true.
    apply: Option<bool>,
    /// Preview cleanup without modifying files.
    dry_run: Option<bool>,
    /// Also remove generated project-local MCP config.
    include_mcp_config: Option<bool>,
}

/// MCP parameter payload for setting purpose metadata.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasPurposeSetParams {
    /// Indexed repository-relative path.
    path: String,
    /// Agent-approved purpose one-liner.
    purpose: String,
}

/// MCP parameter payload for resolving health findings.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasHealthResolveParams {
    /// Stable finding id from `atlas_health`.
    finding_id: String,
    /// Finding category.
    category: String,
    /// Primary path.
    path: String,
    /// Optional related path.
    related_path: Option<String>,
    /// Agent rationale for resolving the finding.
    rationale: String,
}

/// Native `ProjectAtlas` MCP server backed by the same services as the CLI.
#[derive(Debug, Clone)]
pub(crate) struct ProjectAtlasMcpServer {
    /// Path to the durable `SQLite` index.
    db_path: PathBuf,
    /// Optional `ProjectAtlas` configuration path.
    config_path: Option<PathBuf>,
    /// Token telemetry session id.
    session: String,
    /// Official RMCP tool router.
    tool_router: ToolRouter<Self>,
}

impl ProjectAtlasMcpServer {
    /// Create a `ProjectAtlas` MCP server instance.
    pub(crate) fn new(db_path: PathBuf, config_path: Option<PathBuf>, session: String) -> Self {
        Self {
            db_path,
            config_path,
            session,
            tool_router: Self::tool_router(),
        }
    }

    /// Open the durable index.
    fn open_store(&self) -> Result<AtlasStore, CliError> {
        AtlasStore::open(&self.db_path).map_err(CliError::from)
    }

    /// Open the durable index for mutation.
    fn open_mut_store(&self) -> Result<AtlasStore, CliError> {
        AtlasStore::open(&self.db_path).map_err(CliError::from)
    }

    /// Return a path parameter as a native path buffer.
    fn path_or_current(path: Option<String>) -> PathBuf {
        path.map_or_else(|| PathBuf::from("."), PathBuf::from)
    }

    /// Return a query parameter with a stable default.
    fn query_or_empty(query: Option<String>) -> String {
        query.unwrap_or_default()
    }

    /// Convert a command result into an agent-readable TOON MCP text payload.
    fn as_mcp_text(result: Result<String, CliError>) -> String {
        match result {
            Ok(text) => text,
            Err(error) => encode_agent_payload(&json!({
                "error": {
                    "message": error.to_string()
                }
            })),
        }
    }
}

#[tool_router(router = tool_router)]
impl ProjectAtlasMcpServer {
    /// Scan a repository, import purpose metadata, rebuild symbols, and return an overview.
    #[tool(
        name = "atlas_scan",
        description = "Scan repository structure, import ProjectAtlas purpose metadata, rebuild symbols, and return a TOON overview."
    )]
    fn atlas_scan(&self, Parameters(params): Parameters<AtlasScanParams>) -> String {
        Self::as_mcp_text((|| {
            let path = Self::path_or_current(params.path);
            let root = canonical_project_root(&path)?;
            let scan_config = load_scan_import_config(self.config_path.as_deref(), &root)?;
            let scan_options = scan_config.as_ref().map_or_else(
                ScanOptions::default,
                atlas_map::AtlasMapConfig::scan_options,
            );
            let nodes = scan_repo(&root, &scan_options)?;
            let mut store = self.open_mut_store()?;
            store.set_project_root(&root)?;
            store.replace_scan(&nodes)?;
            let text_index = refresh_text_index_for_nodes(&mut store, &root, &nodes)?;
            if let Some(config) = scan_config.as_ref() {
                for record in imported_purpose_records(config)? {
                    store.set_purpose(&record.path, &record.summary, PurposeSource::Imported)?;
                }
            }
            let symbol_options = SymbolBuildOptions::new(
                params.max_bytes.unwrap_or(MAX_SYMBOL_FILE_BYTES),
                params.max_workers,
                params.timeout_seconds,
            );
            let symbols = build_symbols_for_index(&mut store, &root, &symbol_options, None)?;
            let overview = store.overview()?;
            Ok(encode_agent_payload(&json!({
                "scan": ScanReport { overview, text_index, symbols }
            })))
        })())
    }

    /// Return the indexed repository overview.
    #[tool(
        name = "atlas_overview",
        description = "Return a compact TOON overview of indexed files, folders, and purpose coverage."
    )]
    fn atlas_overview(&self) -> String {
        Self::as_mcp_text((|| {
            let store = self.open_store()?;
            let overview = store.overview()?;
            let toon = render_overview(&overview);
            record_usage_estimate(
                &store,
                &self.session,
                "mcp.atlas_overview",
                None,
                None,
                estimated_source_tokens_for_indexed_files(&store, None, None)?,
                &toon,
            )?;
            Ok(toon)
        })())
    }

    /// Rank folders before an agent chooses a work area.
    #[tool(
        name = "atlas_folders",
        description = "Rank repository folders by query and purpose so agents choose a work area before opening files."
    )]
    fn atlas_folders(&self, Parameters(params): Parameters<AtlasQueryParams>) -> String {
        Self::as_mcp_text((|| {
            let store = self.open_store()?;
            let query = Self::query_or_empty(params.query);
            let selected = store.load_ranked_nodes(
                &query,
                NodeKind::Folder,
                None,
                params.limit.unwrap_or(10),
                0,
            )?;
            let toon = render_nodes("folders", &selected);
            record_usage_estimate(
                &store,
                &self.session,
                "mcp.atlas_folders",
                None,
                Some(query),
                estimated_source_tokens_for_indexed_files(&store, None, None)?,
                &toon,
            )?;
            Ok(toon)
        })())
    }

    /// Rank files after an agent has chosen a folder or query.
    #[tool(
        name = "atlas_files",
        description = "Rank repository files by query, purpose, and optional folder before an agent opens source."
    )]
    fn atlas_files(&self, Parameters(params): Parameters<AtlasQueryParams>) -> String {
        Self::as_mcp_text((|| {
            let store = self.open_store()?;
            let query = Self::query_or_empty(params.query);
            let folder_filter = params
                .folder
                .as_deref()
                .map(normalized_folder_filter)
                .transpose()?;
            let selected = ranked_file_nodes(
                &store,
                &query,
                folder_filter.as_deref(),
                params.file_pattern.as_deref(),
                params.limit.unwrap_or(10),
            )?;
            let baseline_tokens = estimated_source_tokens_for_indexed_files(
                &store,
                folder_filter.as_deref(),
                params.file_pattern.as_deref(),
            )?;
            let toon = render_nodes("files", &selected);
            record_usage_estimate(
                &store,
                &self.session,
                "mcp.atlas_files",
                params.file_pattern.or(folder_filter),
                Some(query),
                baseline_tokens,
                &toon,
            )?;
            Ok(toon)
        })())
    }

    /// Build a compact file outline.
    #[tool(
        name = "atlas_outline",
        description = "Return compact TOON outline and preview context for a selected file."
    )]
    fn atlas_outline(&self, Parameters(params): Parameters<AtlasOutlineParams>) -> String {
        Self::as_mcp_text((|| {
            let file = PathBuf::from(&params.file);
            let store = self.open_store()?;
            let file_key = validated_indexed_file_key(&store, &file)?;
            let content = read_indexed_file_content(&store, &file_key)?;
            let language = store
                .load_node_by_path(&file_key)?
                .and_then(|node| node.node.language);
            let outline = build_outline(&file_key, language, &content, params.lines.unwrap_or(12));
            let toon = render_outline(&outline);
            record_usage_text(
                &store,
                &self.session,
                "mcp.atlas_outline",
                Some(file_key),
                None,
                &content,
                &toon,
            )?;
            Ok(toon)
        })())
    }

    /// Return deterministic structured file intelligence from the deep index.
    #[tool(
        name = "atlas_file_summary",
        description = "Return structured TOON file intelligence: purpose state, observed summary, imports, symbols, line ranges, and calls."
    )]
    fn atlas_file_summary(&self, Parameters(params): Parameters<AtlasFileSummaryParams>) -> String {
        Self::as_mcp_text((|| {
            let store = self.open_store()?;
            let file = PathBuf::from(&params.file);
            let report = build_file_summary(
                &store,
                &file,
                params.limit.unwrap_or(DEFAULT_FILE_SUMMARY_LIMIT),
            )?;
            let toon = render_file_summary(&report);
            record_usage_text(
                &store,
                &self.session,
                "mcp.atlas_file_summary",
                Some(report.file_path.clone()),
                None,
                &file_summary_usage_baseline(&store, &report)?,
                &toon,
            )?;
            Ok(toon)
        })())
    }

    /// Search selected indexed files with optional context lines.
    #[tool(
        name = "atlas_search",
        description = "Search indexed files with literal, regex, or fuzzy matching, file filters, pagination, and TOON results."
    )]
    fn atlas_search(&self, Parameters(params): Parameters<AtlasSearchParams>) -> String {
        Self::as_mcp_text((|| {
            let store = self.open_store()?;
            let report = search_indexed_files(
                &store,
                &params.pattern,
                params.regex.unwrap_or(false),
                params.fuzzy.unwrap_or(false),
                params.case_sensitive.unwrap_or(false),
                params.file_pattern.as_deref(),
                params.context_lines.unwrap_or(0),
                params.start_index.unwrap_or(0),
                params.limit.unwrap_or(20),
            )?;
            let toon = render_search_report(&report);
            record_usage_estimate(
                &store,
                &self.session,
                "mcp.atlas_search",
                params.file_pattern,
                Some(params.pattern),
                byte_count_to_tokens(report.searched_bytes),
                &toon,
            )?;
            Ok(toon)
        })())
    }

    /// Return an exact line or symbol slice from a selected file.
    #[tool(
        name = "atlas_slice",
        description = "Return exact source for a selected line range or indexed symbol, after folder/file orientation."
    )]
    fn atlas_slice(&self, Parameters(params): Parameters<AtlasSliceParams>) -> String {
        Self::as_mcp_text((|| {
            let file = PathBuf::from(&params.file);
            let store = self.open_store()?;
            let report = if let Some(symbol) = params.symbol {
                read_symbol_slice(
                    &store,
                    &file,
                    &SymbolSliceSelector {
                        name: &symbol,
                        parent: params.symbol_parent.as_deref(),
                        kind: params.symbol_kind.as_deref(),
                        line: params.symbol_line,
                    },
                )?
            } else {
                if params.symbol_parent.is_some()
                    || params.symbol_kind.is_some()
                    || params.symbol_line.is_some()
                {
                    return Err(CliError::InvalidInput(
                        "symbol disambiguators require symbol".to_string(),
                    ));
                }
                let start_line = params.start_line.ok_or_else(|| {
                    CliError::InvalidInput(
                        "start_line is required unless symbol is provided".to_string(),
                    )
                })?;
                read_indexed_code_slice(&store, &file, start_line, params.end_line)?
            };
            let toon = render_code_slice(&report);
            record_usage_text(
                &store,
                &self.session,
                "mcp.atlas_slice",
                Some(report.path.clone()),
                None,
                &read_indexed_file_content(&store, &report.path)?,
                &toon,
            )?;
            Ok(toon)
        })())
    }

    /// Rebuild symbol graphs for indexed files.
    #[tool(
        name = "atlas_symbols_build",
        description = "Rebuild ProjectAtlas symbol graphs for indexed files and return a TOON build report."
    )]
    fn atlas_symbols_build(&self, Parameters(params): Parameters<AtlasScanParams>) -> String {
        Self::as_mcp_text((|| {
            let mut store = self.open_mut_store()?;
            let path = Self::path_or_current(params.path);
            let options = SymbolBuildOptions::new(
                params.max_bytes.unwrap_or(MAX_SYMBOL_FILE_BYTES),
                params.max_workers,
                params.timeout_seconds,
            );
            let report = build_symbols_for_index(&mut store, &path, &options, None)?;
            Ok(encode_agent_payload(&json!({ "symbols_build": report })))
        })())
    }

    /// List indexed symbols.
    #[tool(
        name = "atlas_symbols",
        description = "List indexed symbols by optional file and query as compact TOON."
    )]
    fn atlas_symbols(&self, Parameters(params): Parameters<AtlasSymbolsParams>) -> String {
        Self::as_mcp_text((|| {
            let store = self.open_store()?;
            let symbols = store.load_symbols(
                params.file.as_deref(),
                params.query.as_deref(),
                params.limit.unwrap_or(50),
            )?;
            let toon = render_symbols(&symbols);
            let baseline_tokens = estimated_source_tokens_for_paths(
                &store,
                symbols.iter().map(|symbol| symbol.path.as_str()),
            )?;
            record_usage_estimate(
                &store,
                &self.session,
                "mcp.atlas_symbols",
                params.file,
                params.query,
                baseline_tokens,
                &toon,
            )?;
            Ok(toon)
        })())
    }

    /// List indexed symbol relations.
    #[tool(
        name = "atlas_symbol_relations",
        description = "List imports, calls, dependencies, and containment edges as compact TOON."
    )]
    fn atlas_symbol_relations(&self, Parameters(params): Parameters<AtlasSymbolsParams>) -> String {
        Self::as_mcp_text((|| {
            let store = self.open_store()?;
            let relations = store.load_symbol_relations(
                params.file.as_deref(),
                params.query.as_deref(),
                params.limit.unwrap_or(50),
            )?;
            let toon = render_symbol_relations(&relations);
            let baseline_tokens = estimated_source_tokens_for_paths(
                &store,
                relations.iter().map(|relation| relation.path.as_str()),
            )?;
            record_usage_estimate(
                &store,
                &self.session,
                "mcp.atlas_symbol_relations",
                params.file,
                params.query,
                baseline_tokens,
                &toon,
            )?;
            Ok(toon)
        })())
    }

    /// Return structural health findings.
    #[tool(
        name = "atlas_health",
        description = "Return ProjectAtlas structural health findings for cleanup and refactor work."
    )]
    fn atlas_health(&self) -> String {
        Self::as_mcp_text((|| {
            let store = self.open_store()?;
            let findings = store.unresolved_health_findings(&store.resolved_health_ids()?)?;
            let toon = render_health(&findings);
            record_usage_estimate(
                &store,
                &self.session,
                "mcp.atlas_health",
                None,
                None,
                estimated_source_tokens_for_indexed_files(&store, None, None)?,
                &toon,
            )?;
            Ok(toon)
        })())
    }

    /// Mark an intentional deterministic health finding as resolved.
    #[tool(
        name = "atlas_health_resolve",
        description = "Mark a deterministic ProjectAtlas health finding as agent-resolved with rationale."
    )]
    fn atlas_health_resolve(
        &self,
        Parameters(params): Parameters<AtlasHealthResolveParams>,
    ) -> String {
        Self::as_mcp_text((|| {
            let store = self.open_store()?;
            let resolution = HealthResolution {
                finding_id: params.finding_id,
                category: params.category,
                path: params.path,
                related_path: params.related_path,
                rationale: params.rationale,
            };
            store.resolve_health_finding(&resolution)?;
            Ok(encode_agent_payload(
                &json!({ "health_resolution": resolution }),
            ))
        })())
    }

    /// Return token savings telemetry.
    #[tool(
        name = "atlas_token_report",
        description = "Return ProjectAtlas token-savings telemetry for the whole index or one session."
    )]
    fn atlas_token_report(&self, Parameters(params): Parameters<AtlasTokenParams>) -> String {
        Self::as_mcp_text((|| {
            let store = self.open_store()?;
            Ok(render_token_overview(
                &store.token_overview(params.session.as_deref())?,
            ))
        })())
    }

    /// Return repository-intelligence parity readiness.
    #[tool(
        name = "atlas_parity_report",
        description = "Return a ProjectAtlas repository-intelligence parity gate report for release and agent-runtime readiness."
    )]
    fn atlas_parity_report(&self, Parameters(params): Parameters<AtlasParityParams>) -> String {
        Self::as_mcp_text((|| {
            let store = self.open_store()?;
            let profile = params
                .profile
                .unwrap_or_else(|| "repository-intelligence".to_string());
            Ok(render_parity_report(&build_parity_report(
                &store, &profile,
            )?))
        })())
    }

    /// Return local settings and cache/index locations.
    #[tool(
        name = "atlas_settings",
        description = "Return ProjectAtlas local settings, config, and durable index paths."
    )]
    fn atlas_settings(&self) -> String {
        Self::as_mcp_text((|| {
            let report = build_settings_report(
                &self.db_path,
                self.config_path.as_deref(),
                OutputFormat::Toon,
            )?;
            Ok(render_settings_report(&report))
        })())
    }

    /// Return watcher availability and operating mode.
    #[tool(
        name = "atlas_watch_status",
        description = "Return ProjectAtlas watcher availability and current operating mode."
    )]
    fn atlas_watch_status(&self) -> String {
        let mut report = watcher_status_report(false);
        if !self.db_path.exists() {
            report.recommendation.push_str(
                " Run `atlas_scan` first when no ProjectAtlas index exists for this project.",
            );
        }
        Self::as_mcp_text(Ok(render_watch_status(&report)))
    }

    /// Run one incremental refresh pass.
    #[tool(
        name = "atlas_watch_once",
        description = "Run one MCP-safe watcher refresh pass over the repository and rebuild changed symbols."
    )]
    fn atlas_watch_once(&self, Parameters(params): Parameters<AtlasPathParams>) -> String {
        Self::as_mcp_text((|| {
            let mut store = self.open_mut_store()?;
            let root = canonical_project_root(&Self::path_or_current(params.path))?;
            let scan_options = scan_options_for_root(self.config_path.as_deref(), &root)?;
            let symbol_options = SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None);
            let report = run_watch_loop(
                &mut store,
                &root,
                true,
                1,
                1,
                &symbol_options,
                &scan_options,
            )?;
            Ok(encode_agent_payload(&json!({ "watch": report })))
        })())
    }

    /// Preview or remove legacy `.purpose` files.
    #[tool(
        name = "atlas_strip_legacy_purpose",
        description = "Preview or remove legacy .purpose files after their metadata has been imported to SQLite."
    )]
    fn atlas_strip_legacy_purpose(
        &self,
        Parameters(params): Parameters<AtlasStripLegacyParams>,
    ) -> String {
        Self::as_mcp_text((|| {
            let report = strip_legacy_purpose(
                &Self::path_or_current(params.path),
                self.config_path.as_deref(),
                params.apply.unwrap_or(false),
                params.dry_run.unwrap_or(false),
                params
                    .strip_source_headers
                    .unwrap_or_else(|| self.config_path.is_some()),
            )?;
            Ok(encode_agent_payload(
                &json!({ "legacy_purpose_migration": report }),
            ))
        })())
    }

    /// Preview or remove local runtime index/cache files.
    #[tool(
        name = "atlas_reset_index",
        description = "Preview or clear ProjectAtlas local SQLite index/cache files for recovery."
    )]
    fn atlas_reset_index(&self, Parameters(params): Parameters<AtlasResetIndexParams>) -> String {
        Self::as_mcp_text((|| {
            let report = reset_index_files(
                &self.db_path,
                params.apply.unwrap_or(false),
                params.dry_run.unwrap_or(false),
                params.include_mcp_config.unwrap_or(false),
            )?;
            Ok(encode_agent_payload(&json!({ "reset_index": report })))
        })())
    }

    /// Set an agent-approved purpose in the durable index.
    #[tool(
        name = "atlas_purpose_set",
        description = "Set agent-approved ProjectAtlas purpose metadata for one indexed path."
    )]
    fn atlas_purpose_set(&self, Parameters(params): Parameters<AtlasPurposeSetParams>) -> String {
        Self::as_mcp_text((|| {
            let store = self.open_store()?;
            store.set_purpose(&params.path, &params.purpose, PurposeSource::Agent)?;
            Ok(encode_agent_payload(&json!({
                "purpose_set": {
                    "path": params.path,
                    "status": "approved"
                }
            })))
        })())
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ProjectAtlasMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "ProjectAtlas provides TOON-first repository orientation, folder/file ranking, structured file summaries, symbol graph lookup, exact slices, health checks, and token telemetry for coding agents.",
        )
    }
}

/// Render a search report as compact TOON.
fn render_search_report(report: &SearchReport) -> String {
    encode_agent_payload(&json!({ "search": report }))
}

/// Render repository-intelligence parity as compact TOON.
fn render_parity_report(report: &ParityReport) -> String {
    encode_agent_payload(&json!({ "parity": report }))
}

/// Render a code slice as compact TOON.
fn render_code_slice(slice: &CodeSlice) -> String {
    encode_agent_payload(&json!({ "slice": slice }))
}

/// Render settings as compact TOON.
fn render_settings_report(report: &SettingsReport) -> String {
    encode_agent_payload(&json!({ "settings": report }))
}

/// Render watcher status as compact TOON.
fn render_watch_status(report: &WatchStatusReport) -> String {
    encode_agent_payload(&json!({ "watch_status": report }))
}

/// Build settings diagnostics shared by CLI and MCP.
fn build_settings_report(
    db: &Path,
    config_path: Option<&Path>,
    format: OutputFormat,
) -> Result<SettingsReport, CliError> {
    let config = load_atlas_config(config_path)?;
    let absolute_db = absolute_path(db)?;
    let cache_dir = absolute_db
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let resolved_config = resolved_mcp_config_path(config_path)?;
    let index = if absolute_db.exists() {
        let store = AtlasStore::open(&absolute_db)?;
        Some(settings_index_stats(&store)?)
    } else {
        None
    };
    Ok(SettingsReport {
        cache_dir: path_status(&cache_dir)?,
        db: path_status(&absolute_db)?,
        db_wal: path_status(&db_sidecar_path(&absolute_db, "wal"))?,
        db_shm: path_status(&db_sidecar_path(&absolute_db, "shm"))?,
        db_journal: path_status(&db_sidecar_path(&absolute_db, "journal"))?,
        mcp_config: path_status(&mcp_config_path_for_db(&absolute_db))?,
        config_path: resolved_config.map(|path| normalize_display_path(&path)),
        repo_root: normalize_display_path(&config.root),
        map_path: normalize_display_path(&config.map_path),
        nonsource_files_path: normalize_display_path(&config.nonsource_files_path),
        default_format: format!("{format:?}").to_ascii_lowercase(),
        default_search_case_sensitive: false,
        search_source: "sqlite-file-text".to_string(),
        watcher: watcher_status_report(false),
        index,
    })
}

/// Build index statistics for settings diagnostics.
fn settings_index_stats(store: &AtlasStore) -> Result<SettingsIndexStats, CliError> {
    let overview = store.overview()?;
    let health_findings = store
        .unresolved_health_findings(&store.resolved_health_ids()?)?
        .len();
    Ok(SettingsIndexStats {
        project_root: store.project_root()?,
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
fn reset_index_files(
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

/// Return a diagnostic status for one path.
fn path_status(path: &Path) -> Result<PathStatus, CliError> {
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
fn db_sidecar_path(db: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}-{suffix}", db.display()))
}

/// Return the project-local MCP config path associated with a database path.
fn mcp_config_path_for_db(db: &Path) -> PathBuf {
    db.parent().map_or_else(
        || PathBuf::from("projectatlas.mcp.json"),
        |parent| parent.join("projectatlas.mcp.json"),
    )
}

/// Normalize a path for JSON/TOON diagnostics.
fn normalize_display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Render a human-facing token savings dashboard for terminal use.
fn render_token_dashboard(overview: &TokenOverview, session: Option<&str>) -> String {
    let rate = overview.savings_rate.unwrap_or(0.0);
    let positive_rate = rate.clamp(0.0, 1.0);
    let filled = (positive_rate * 32.0).round() as usize;
    let empty = 32usize.saturating_sub(filled);
    let session_label = session.unwrap_or("all sessions");
    let rate_label = overview.savings_rate.map_or_else(
        || "unknown".to_string(),
        |value| format!("{:.1}%", value * 100.0),
    );
    let saved_label = signed_count(overview.estimated_saved);
    format!(
        "\
+--------------------------------------------------+\n\
| ProjectAtlas Token Savings                       |\n\
+--------------------------------------------------+\n\
| Session | {session_label}\n\
| Calls   | {calls}\n\
| Saved   | {saved_label} tokens\n\
| Before  | {without} tokens avoided\n\
| After   | {with} tokens used through atlas\n\
| Rate    | {rate_label}\n\
+--------------------------------------------------+\n\
| Funnel  | overview > folders > files > exact slice\n\
| Impact  | wrong folders and wrong-file opens avoided\n\
| Reads   | unnecessary full-code reads avoided\n\
| Savings | [{bar}{rest}] {rate_label}\n\
+--------------------------------------------------+\n",
        calls = overview.calls,
        without = grouped_count(overview.estimated_without_projectatlas),
        with = grouped_count(overview.estimated_with_projectatlas),
        bar = "#".repeat(filled),
        rest = ".".repeat(empty),
    )
}

/// Format an unsigned count with thousands separators.
fn grouped_count(value: usize) -> String {
    let raw = value.to_string();
    let mut grouped = String::with_capacity(raw.len() + raw.len() / 3);
    for (index, character) in raw.chars().enumerate() {
        if index > 0 && (raw.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(character);
    }
    grouped
}

/// Format a signed count with thousands separators.
fn signed_count(value: isize) -> String {
    if value < 0 {
        format!("-{}", grouped_count(value.unsigned_abs()))
    } else {
        grouped_count(usize::try_from(value).unwrap_or(usize::MAX))
    }
}

/// Build a watcher status report from a lightweight runtime probe.
fn watcher_status_report(active: bool) -> WatchStatusReport {
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
fn lint_database_if_present(db: &Path) -> Result<(String, i32), CliError> {
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
fn notify_runtime_available() -> bool {
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

/// Build the current repository-intelligence parity report.
fn build_parity_report(store: &AtlasStore, profile: &str) -> Result<ParityReport, CliError> {
    if profile != "repository-intelligence" {
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
    let watcher = watcher_status_report(false);

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
        "purpose-coverage",
        overview.missing_purposes == 0 && overview.suggested_purposes == 0,
        &format!(
            "{} missing, {} suggested, {} stale purposes",
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
        watcher.available,
        &format!("watcher mode {}", watcher.mode),
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
        "scan, overview, folders, files, summary, outline, search, slice, symbols, watch, health, token, parity, mcp are compiled",
    );
    push_check(
        &mut checks,
        "mcp-surface",
        required_mcp_surface_present(),
        "atlas_* tools cover scan, overview, folders, files, summary, outline, search, slice, symbols, health, token, settings, watch, parity, and reset-index",
    );
    let ok = checks.iter().all(|check| check.status == "pass");
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
        watcher_mode: watcher.mode,
        checks,
    })
}

/// Append one parity check.
fn push_check(checks: &mut Vec<ParityCheck>, name: &str, passed: bool, detail: &str) {
    checks.push(ParityCheck {
        name: name.to_string(),
        status: if passed { "pass" } else { "fail" }.to_string(),
        detail: detail.to_string(),
    });
}

/// Return whether the compiled CLI surface contains required command families.
fn required_cli_surface_present() -> bool {
    let required = [
        "init",
        "map",
        "seed-purpose",
        "scan",
        "overview",
        "folders",
        "files",
        "outline",
        "summary",
        "search",
        "slice",
        "symbols",
        "settings",
        "watch-status",
        "watch",
        "health-check",
        "health",
        "lint",
        "token",
        "parity",
        "strip-legacy-purpose",
        "mcp",
        "mcp-config",
        "purpose",
    ];
    let command = Cli::command();
    required.iter().all(|name| {
        command
            .get_subcommands()
            .any(|subcommand| subcommand.get_name() == *name)
    })
}

/// Return whether the compiled MCP surface contains required tool families.
fn required_mcp_surface_present() -> bool {
    let router = ProjectAtlasMcpServer::tool_router();
    REQUIRED_MCP_TOOL_NAMES
        .iter()
        .all(|name| router.has_route(name))
}

/// Render a deterministic file summary as compact TOON.
fn render_file_summary(report: &FileSummaryReport) -> String {
    encode_agent_payload(&json!({ "file_summary": report }))
}

/// Options controlling source parsing during symbol graph builds.
#[derive(Clone, Copy, Debug)]
struct SymbolBuildOptions {
    /// Maximum file size parsed for symbols.
    max_bytes: u64,
    /// Optional maximum worker threads for parser work.
    max_workers: Option<usize>,
    /// Optional deadline for starting parser work.
    timeout: Option<Duration>,
    /// Serialized timeout value for reports.
    timeout_seconds: Option<u64>,
}

impl SymbolBuildOptions {
    /// Create symbol build options from CLI/MCP values.
    fn new(max_bytes: u64, max_workers: Option<usize>, timeout_seconds: Option<u64>) -> Self {
        Self {
            max_bytes,
            max_workers: max_workers.filter(|workers| *workers > 0),
            timeout: timeout_seconds.map(Duration::from_secs),
            timeout_seconds,
        }
    }

    /// Return the worker count that will be reported.
    fn reported_workers(self) -> usize {
        self.max_workers
            .unwrap_or_else(|| thread::available_parallelism().map_or(1, usize::from))
    }

    /// Return whether the parser build deadline has elapsed.
    fn is_timed_out(self, started_at: Instant) -> bool {
        self.timeout
            .is_some_and(|timeout| started_at.elapsed() >= timeout)
    }
}

/// Source file queued for symbol parsing.
#[derive(Clone, Debug)]
struct SymbolParseJob {
    /// Repository-relative file path.
    path: String,
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
struct SymbolParseSuccess {
    /// Repository-relative file path.
    path: String,
    /// Extracted symbol graph.
    graph: SymbolGraph,
    /// Observed one-line source summary.
    summary: String,
    /// Optional generated purpose suggestion.
    purpose_suggestion: Option<String>,
}

/// Outcome from one parser worker.
#[derive(Debug)]
enum SymbolParseOutcome {
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
fn build_symbols_for_index(
    store: &mut AtlasStore,
    root: &Path,
    options: &SymbolBuildOptions,
    previous_hashes: Option<&HashMap<String, String>>,
) -> Result<SymbolBuildReport, CliError> {
    build_symbols_for_paths(store, root, options, previous_hashes, None)
}

/// Build symbol graphs for selected indexed files.
fn build_symbols_for_paths(
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
        .filter(|node| is_symbol_candidate(node.node.language.as_deref()))
    {
        report.candidates += 1;
        if node
            .node
            .size_bytes
            .is_some_and(|size| size > options.max_bytes)
        {
            store.clear_source_index_for_path(&node.node.path)?;
            report.too_large += 1;
            continue;
        }
        let symbol_count = store.symbol_count_for_path(&node.node.path)?;
        if symbol_count > 0
            && node.node.content_hash.as_ref().is_some_and(|hash| {
                previous_hashes.and_then(|hashes| hashes.get(&node.node.path)) == Some(hash)
            })
        {
            report.unchanged += 1;
            continue;
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
                store.clear_source_index_for_path(&path)?;
                report.timed_out += 1;
            }
            SymbolParseOutcome::BinaryOrNonUtf8 { path } => {
                store.clear_source_index_for_path(&path)?;
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
fn parse_symbol_jobs(
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
fn parse_symbol_job(
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
fn empty_symbol_build_report() -> SymbolBuildReport {
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

/// Create a deterministic one-line observed summary from extracted symbols.
fn summarize_symbol_graph(graph: &SymbolGraph, fallback: Option<&str>) -> String {
    if graph.symbols.is_empty() {
        return fallback.map_or_else(
            || "Indexed source file with no declarations found.".to_string(),
            str::to_string,
        );
    }
    let language = graph.language.as_deref().unwrap_or("source");
    let primary_names = primary_symbol_names(graph, 4);
    let primary_kinds = primary_symbol_kinds(graph);
    let imports = relation_targets(graph, RelationKind::Imports, 2);
    let dependencies = relation_targets(graph, RelationKind::DependsOn, 3);
    if !dependencies.is_empty() {
        return format!(
            "{language} manifest declaring {} and depending on {}.",
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

/// Return a compact phrase describing the most important symbol kinds.
fn primary_symbol_kinds(graph: &SymbolGraph) -> String {
    let mut function_like = 0_usize;
    let mut type_like = 0_usize;
    let mut manifest_like = 0_usize;
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
            SymbolKind::Module | SymbolKind::Value | SymbolKind::Import | SymbolKind::Unknown => {}
        }
    }
    if manifest_like > 0 && function_like == 0 && type_like == 0 {
        return "manifest entries".to_string();
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

/// Return stable names for the most important declaration symbols.
fn primary_symbol_names(graph: &SymbolGraph, limit: usize) -> Vec<String> {
    let mut names = graph
        .symbols
        .iter()
        .filter(|symbol| {
            !matches!(
                symbol.kind,
                SymbolKind::Import | SymbolKind::Dependency | SymbolKind::Unknown
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
fn relation_targets(graph: &SymbolGraph, kind: RelationKind, limit: usize) -> Vec<String> {
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

/// Create a generated file-purpose suggestion from an observed summary.
fn suggest_file_purpose(path: &str, summary: &str) -> String {
    let name = path
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(path);
    format!("Provide {name} behavior: {}", summary.trim_end_matches('.'))
}

/// Return whether a language should be parsed for symbols.
fn is_symbol_candidate(language: Option<&str>) -> bool {
    language.is_some_and(|language| {
        !matches!(
            language,
            "text" | "json" | "yaml" | "xml" | "config" | "markdown"
        )
    })
}

/// Normalize and validate a user-supplied path as a repository-relative file key.
fn validated_file_key(file: &Path) -> Result<String, CliError> {
    validated_repo_file_key(file).map_err(|source| CliError::InvalidInput(source.to_string()))
}

/// Normalize a folder filter into the repository path convention.
fn normalized_folder_filter(folder: &str) -> Result<String, CliError> {
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
fn validated_indexed_file_key(store: &AtlasStore, file: &Path) -> Result<String, CliError> {
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
fn indexed_project_root(store: &AtlasStore) -> Result<PathBuf, CliError> {
    store.project_root()?.map(PathBuf::from).ok_or_else(|| {
        CliError::InvalidInput(
            "indexed project root is missing; run projectatlas scan <project-root> first"
                .to_string(),
        )
    })
}

/// Build an absolute native path for a previously validated indexed file key.
fn indexed_native_path(store: &AtlasStore, file_key: &str) -> Result<PathBuf, CliError> {
    Ok(indexed_project_root(store)?.join(repo_path_to_native(file_key)))
}

/// Read content for a previously validated indexed file key.
fn read_indexed_file_content(store: &AtlasStore, file_key: &str) -> Result<String, CliError> {
    let native = indexed_native_path(store, file_key)?;
    fs::read_to_string(&native).map_err(|source| CliError::Io {
        path: native,
        source,
    })
}

/// Run the watcher refresh loop.
fn run_watch_loop(
    store: &mut AtlasStore,
    root: &Path,
    once: bool,
    poll_seconds: u64,
    max_cycles: usize,
    symbol_options: &SymbolBuildOptions,
    scan_options: &ScanOptions,
) -> Result<WatchReport, CliError> {
    if once {
        return run_single_watch_refresh(store, root, symbol_options, scan_options);
    }
    match run_notify_watch_loop(
        store,
        root,
        poll_seconds,
        max_cycles,
        symbol_options,
        scan_options,
    ) {
        Ok(report) => Ok(report),
        Err(error) => run_polling_watch_loop(
            store,
            root,
            poll_seconds,
            max_cycles,
            symbol_options,
            scan_options,
            Some(error.to_string()),
        ),
    }
}

/// Run one deterministic watcher refresh and exit.
fn run_single_watch_refresh(
    store: &mut AtlasStore,
    root: &Path,
    symbol_options: &SymbolBuildOptions,
    scan_options: &ScanOptions,
) -> Result<WatchReport, CliError> {
    let last_refresh = refresh_index(store, root, symbol_options, scan_options)?;
    Ok(WatchReport {
        mode: WATCH_MODE_ONCE.to_string(),
        cycles: 1,
        once: true,
        fallback_reason: None,
        text_index: last_refresh.text_index,
        last_symbols: last_refresh.symbols,
    })
}

/// Run an event-backed watcher loop with `notify`.
fn run_notify_watch_loop(
    store: &mut AtlasStore,
    root: &Path,
    poll_seconds: u64,
    max_cycles: usize,
    symbol_options: &SymbolBuildOptions,
    scan_options: &ScanOptions,
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
    let mut last_refresh = refresh_index(store, &watch_root, symbol_options, scan_options)?;
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
        last_symbols: last_refresh.symbols,
    })
}

/// Wait for a debounced batch of relevant filesystem events.
fn wait_for_index_event(
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
fn notify_result_changes(
    root: &Path,
    scan_options: &ScanOptions,
    result: notify::Result<Event>,
) -> Result<WatchChangeSet, CliError> {
    let event = result.map_err(|source| CliError::Watcher(source.to_string()))?;
    Ok(notify_event_changes(root, scan_options, &event))
}

/// Convert a `notify` event into index-relevant changes.
fn notify_event_changes(root: &Path, scan_options: &ScanOptions, event: &Event) -> WatchChangeSet {
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
fn event_kind_affects_index(kind: EventKind) -> bool {
    !matches!(kind, EventKind::Access(_))
}

/// Return whether a native event path belongs to indexed repository content.
fn watch_path_affects_index(root: &Path, path: &Path, scan_options: &ScanOptions) -> bool {
    let candidate = absolute_watch_path(root, path);
    let relative = candidate.strip_prefix(root).unwrap_or(path);
    if relative.as_os_str().is_empty() {
        return true;
    }
    for component in relative.components() {
        match component {
            Component::Normal(name) => {
                let name = name.to_string_lossy();
                if name == ".purpose"
                    || scan_options
                        .exclude_dir_names
                        .iter()
                        .any(|excluded| excluded == name.as_ref())
                {
                    return false;
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    true
}

/// Return an absolute path for a watcher event path.
fn absolute_watch_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

/// Return whether a path event requires a full scan for correctness.
fn watch_path_requires_full_scan(root: &Path, path: &Path) -> bool {
    if path == root {
        return true;
    }
    path.is_dir()
        || path.file_name().is_some_and(|name| name == ".gitignore")
        || normalize_repo_path(root, path).is_ok_and(|normalized| normalized == ".")
}

/// Run the portable polling watcher fallback loop.
fn run_polling_watch_loop(
    store: &mut AtlasStore,
    root: &Path,
    poll_seconds: u64,
    max_cycles: usize,
    symbol_options: &SymbolBuildOptions,
    scan_options: &ScanOptions,
    fallback_reason: Option<String>,
) -> Result<WatchReport, CliError> {
    let mut cycles = 0;
    let mut last_refresh = refresh_index(store, root, symbol_options, scan_options)?;
    cycles += 1;
    while max_cycles == 0 || cycles < max_cycles {
        thread::sleep(Duration::from_secs(poll_seconds.max(1)));
        last_refresh = refresh_index(store, root, symbol_options, scan_options)?;
        cycles += 1;
    }
    Ok(WatchReport {
        mode: WATCH_MODE_POLLING.to_string(),
        cycles,
        once: false,
        fallback_reason,
        text_index: last_refresh.text_index,
        last_symbols: last_refresh.symbols,
    })
}

/// Combined refresh output for watcher and one-shot refresh paths.
struct IndexRefreshReport {
    /// Persisted text search index refresh report.
    text_index: TextIndexReport,
    /// Deep symbol graph refresh report.
    symbols: SymbolBuildReport,
}

/// Refresh filesystem and symbol state.
fn refresh_index(
    store: &mut AtlasStore,
    root: &Path,
    symbol_options: &SymbolBuildOptions,
    scan_options: &ScanOptions,
) -> Result<IndexRefreshReport, CliError> {
    let root = canonical_project_root(root)?;
    let previous_hashes = indexed_file_hashes(store)?;
    let nodes = scan_repo(&root, scan_options)?;
    store.set_project_root(&root)?;
    store.replace_scan(&nodes)?;
    let text_index = refresh_text_index_for_nodes(store, &root, &nodes)?;
    let symbols = build_symbols_for_index(store, &root, symbol_options, Some(&previous_hashes))?;
    Ok(IndexRefreshReport {
        text_index,
        symbols,
    })
}

/// Refresh filesystem and symbol state for a debounced event batch.
fn refresh_index_for_changes(
    store: &mut AtlasStore,
    root: &Path,
    changes: &WatchChangeSet,
    symbol_options: &SymbolBuildOptions,
    scan_options: &ScanOptions,
) -> Result<IndexRefreshReport, CliError> {
    if changes.requires_full_scan {
        return refresh_index(store, root, symbol_options, scan_options);
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
    }
    if !absent_paths.is_empty() {
        store.mark_paths_absent(&absent_paths)?;
    }
    let text_index = refresh_text_index_for_changed_paths(store, &root, &changed_paths, &nodes)?;
    let target_paths = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .map(|node| node.path.clone())
        .collect::<HashSet<_>>();
    if target_paths.is_empty() {
        return Ok(IndexRefreshReport {
            text_index,
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
    Ok(IndexRefreshReport {
        text_index,
        symbols,
    })
}

/// Refresh the persisted text index for every scanned file node.
fn refresh_text_index_for_nodes(
    store: &mut AtlasStore,
    root: &Path,
    nodes: &[Node],
) -> Result<TextIndexReport, CliError> {
    let file_paths = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .map(|node| node.path.clone())
        .collect::<Vec<_>>();
    refresh_text_index_for_changed_paths(
        store,
        root,
        &file_paths.iter().cloned().collect::<HashSet<_>>(),
        nodes,
    )
}

/// Refresh persisted text index rows for an incremental path set.
fn refresh_text_index_for_changed_paths(
    store: &mut AtlasStore,
    root: &Path,
    changed_paths: &HashSet<String>,
    nodes: &[Node],
) -> Result<TextIndexReport, CliError> {
    let mut considered_paths = changed_paths.iter().cloned().collect::<Vec<_>>();
    considered_paths.sort();
    let texts = indexed_file_texts_for_nodes(root, nodes)?;
    let report = TextIndexReport {
        candidates: nodes
            .iter()
            .filter(|node| node.kind == NodeKind::File)
            .count(),
        indexed: texts.len(),
        binary_or_non_utf8: nodes
            .iter()
            .filter(|node| node.kind == NodeKind::File)
            .count()
            .saturating_sub(texts.len()),
        bytes: texts
            .iter()
            .map(|text| text.byte_count)
            .fold(0usize, usize::saturating_add),
    };
    store.replace_file_texts_for_paths(&considered_paths, &texts)?;
    Ok(report)
}

/// Build indexed text rows for UTF-8 scanned files.
fn indexed_file_texts_for_nodes(
    root: &Path,
    nodes: &[Node],
) -> Result<Vec<IndexedFileText>, CliError> {
    let mut texts = Vec::new();
    for node in nodes.iter().filter(|node| node.kind == NodeKind::File) {
        let native_path = root.join(repo_path_to_native(&node.path));
        let bytes = fs::read(&native_path).map_err(|source| CliError::Io {
            path: native_path.clone(),
            source,
        })?;
        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };
        texts.push(IndexedFileText {
            path: node.path.clone(),
            content_hash: node.content_hash.clone(),
            byte_count: content.len(),
            line_count: content.lines().count(),
            content,
        });
    }
    Ok(texts)
}

/// Load indexed file hashes for incremental refresh comparison.
fn indexed_file_hashes(store: &AtlasStore) -> Result<HashMap<String, String>, CliError> {
    Ok(store
        .load_nodes()?
        .into_iter()
        .filter(|node| node.node.kind == NodeKind::File)
        .filter_map(|node| node.node.content_hash.map(|hash| (node.node.path, hash)))
        .collect::<HashMap<_, _>>())
}

/// Load indexed file hashes for selected repository paths.
fn indexed_file_hashes_for_paths(
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
fn sorted_watch_paths(paths: &HashSet<PathBuf>) -> Vec<PathBuf> {
    let mut paths = paths.iter().cloned().collect::<Vec<_>>();
    paths.sort();
    paths
}

/// Normalize a deleted path if it belongs to the watched repository.
fn normalized_deleted_path(root: &Path, path: &Path) -> Result<Option<String>, CliError> {
    match normalize_repo_path(root, path) {
        Ok(path) => Ok(Some(path)),
        Err(projectatlas_core::CoreError::PathOutsideRoot { .. }) => Ok(None),
        Err(source) => Err(CliError::InvalidInput(source.to_string())),
    }
}

/// Inspect and optionally remove legacy `.purpose` files.
fn strip_legacy_purpose(
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
fn indexed_purpose_files(root: &Path, nodes: &[Node]) -> Vec<String> {
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
fn purpose_header_candidates(root: &Path, nodes: &[Node]) -> Result<Vec<String>, CliError> {
    let mut candidates = Vec::new();
    for node in nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .filter(|node| is_symbol_candidate(node.language.as_deref()))
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
    use super::{
        Node, NodeKind, OutputFormat, ProjectAtlasMcpServer, REQUIRED_MCP_TOOL_NAMES, ScanOptions,
        TokenOverview, byte_count_to_tokens, estimated_source_tokens_for_file_node,
        event_kind_affects_index, primary_symbol_names, relation_targets, render_token_dashboard,
        required_mcp_surface_present, reset_index_files, serialized_output, suggest_file_purpose,
        summarize_symbol_graph, watch_path_affects_index, watch_path_requires_full_scan,
        watcher_status_report,
    };
    use notify::EventKind;
    use projectatlas_core::symbols::{
        CodeSymbol, ParserKind, RelationKind, SymbolGraph, SymbolKind, SymbolRelation,
    };
    use rmcp::model::{CallToolRequestParams, ClientInfo};
    use rmcp::{ClientHandler, ServiceExt};
    use serde_json::{Map, Value, json};
    use std::error::Error;
    use std::fs;
    use std::io;

    /// Minimal MCP client handler for in-process routing tests.
    #[derive(Clone, Default)]
    struct TestMcpClient;

    impl ClientHandler for TestMcpClient {
        fn get_info(&self) -> ClientInfo {
            ClientInfo::default()
        }
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
            language: Some("cargo-toml".to_string()),
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
            "cargo-toml manifest declaring projectatlas and depending on rmcp, serde."
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
            "rust file, 0 bytes"
        );
        assert_eq!(
            suggest_file_purpose("src/empty.rs", "rust file, 0 bytes"),
            "Provide empty.rs behavior: rust file, 0 bytes"
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
        };
        require_condition(
            watch_path_affects_index(root, &root.join("src/lib.rs"), &scan_options),
            "source file event should refresh the index",
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
        let router = ProjectAtlasMcpServer::tool_router();
        assert!(required_mcp_surface_present());
        for required_tool in REQUIRED_MCP_TOOL_NAMES {
            assert!(router.has_route(required_tool), "{required_tool} missing");
        }
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
    fn token_dashboard_is_human_readable_and_ascii() {
        let dashboard = render_token_dashboard(
            &TokenOverview {
                calls: 3,
                estimated_without_projectatlas: 12_000,
                estimated_with_projectatlas: 3_000,
                estimated_saved: 9_000,
                savings_rate: Some(0.75),
            },
            Some("session-a"),
        );

        assert!(dashboard.contains("ProjectAtlas Token Savings"));
        assert!(dashboard.contains("| Session | session-a"));
        assert!(dashboard.contains("| Saved   | 9,000 tokens"));
        assert!(dashboard.contains("[########################........] 75.0%"));
        assert!(dashboard.contains("wrong-file opens"));
        assert!(dashboard.contains("overview > folders > files > exact slice"));
        assert!(dashboard.is_ascii());
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
        fs::write(
            repo.join("src").join("main.rs"),
            "fn main() {\n    helper();\n}\n\nfn helper() {}\n",
        )?;
        let db = temp.path().join("projectatlas.db");
        let server = ProjectAtlasMcpServer::new(db, None, "mcp-test".to_string());
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

        let mut scan_args = Map::new();
        scan_args.insert(
            "path".to_string(),
            Value::String(repo.to_string_lossy().to_string()),
        );
        let scan = client
            .peer()
            .call_tool(CallToolRequestParams::new("atlas_scan").with_arguments(scan_args))
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
        if !summary_text.contains("purpose_status: suggested") {
            return Err("atlas_file_summary result did not expose purpose status".into());
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
        if !slice_text.contains("project-relative indexed file path") {
            return Err("atlas_slice did not reject outside-repository absolute paths".into());
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
        if !token_text.contains("calls: 2") {
            return Err("atlas_token_report did not count MCP usage events".into());
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

        client.cancel().await?;
        server_handle.await?.map_err(std::io::Error::other)?;
        Ok(())
    }
}
