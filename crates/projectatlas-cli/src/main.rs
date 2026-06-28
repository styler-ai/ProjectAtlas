//! Purpose: Provide the `ProjectAtlas` 3 command-line adapter.

mod atlas_map;
mod mcp;
mod runtime;

use atlas_map::{
    LintOptions, init_project, lint_map, load_atlas_config, seed_purpose_files, write_map,
};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use projectatlas_core::outline::build_outline;
use projectatlas_core::telemetry::TokenOverview;
use projectatlas_core::toon::{
    encode_agent_payload, render_health, render_nodes, render_outline, render_overview,
    render_symbol_relations, render_symbols, render_token_overview,
};
use projectatlas_core::{NodeKind, PurposeSource};
use projectatlas_db::{AtlasStore, DbError, HealthResolution};
use projectatlas_service::{
    CodeSlice, FileSummaryReport, SearchReport, SymbolSliceSelector, build_file_summary,
    read_indexed_code_slice, read_symbol_slice, search_indexed_files,
};
use runtime::{
    MAX_SYMBOL_FILE_BYTES, ScanRuntimePlan, SettingsReport, SymbolBuildOptions, WatchStatusReport,
    absolute_path, build_settings_report, build_symbols_for_index, byte_count_to_tokens,
    default_mcp_project_root, defaultable_cli_project_root,
    estimated_source_tokens_for_indexed_files, estimated_source_tokens_for_paths,
    file_summary_usage_baseline, lint_database_if_present, normalized_folder_filter,
    open_atlas_store, ranked_file_nodes, read_indexed_file_content, record_usage_estimate,
    record_usage_text, reset_index_files, resolved_mcp_config_path, run_scan_pipeline,
    run_watch_loop, strip_legacy_purpose, validated_indexed_file_key, watcher_status_report,
};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

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
        /// Maximum UTF-8 file size persisted into `SQLite` text search.
        #[arg(long)]
        text_index_max_bytes: Option<u64>,
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
    /// Print structured runtime identity and capability information.
    RuntimeInfo,
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
        Command::Scan {
            path,
            text_index_max_bytes,
        } => {
            let path = defaultable_cli_project_root(path, &cli.db, cli.config.as_deref())?;
            let plan =
                ScanRuntimePlan::for_path(cli.config.as_deref(), &path, *text_index_max_bytes)?;
            let symbol_options = SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None);
            let mut store = open_atlas_store(&cli.db)?;
            let report = run_scan_pipeline(&mut store, &plan, &symbol_options)?;
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
            let store = open_atlas_store(&cli.db)?;
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
            let store = open_atlas_store(&cli.db)?;
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
        Command::HealthCheck => {
            let store = open_atlas_store(&cli.db)?;
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
            let store = open_atlas_store(&cli.db)?;
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
                let store = open_atlas_store(&cli.db)?;
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
        Command::Mcp => {
            mcp::run_mcp_server(cli.db.clone(), cli.config.clone(), cli.session.clone())?;
        }
        Command::McpConfig { server_name } => {
            let report = build_mcp_config_report(server_name, &cli.db, cli.config.as_deref())?;
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
    let absolute_db = absolute_path(db)?;
    let mut args = vec!["--db".to_string(), mcp_launch_path(&absolute_db)];
    let resolved_config = resolved_mcp_config_path(&absolute_db, config)?;
    if let Some(config_path) = resolved_config.as_ref() {
        args.push("--config".to_string());
        args.push(mcp_launch_path(config_path));
    }
    args.push("mcp".to_string());
    let project_root = default_mcp_project_root(&absolute_db, resolved_config.as_deref())?;
    let mut mcp_servers = BTreeMap::new();
    mcp_servers.insert(
        server_name.to_string(),
        McpServerConfig {
            command: executable.display().to_string(),
            args,
            cwd: mcp_launch_path(&project_root),
        },
    );
    Ok(McpConfigDocument { mcp_servers })
}

/// Render a native path for MCP launch config without Windows extended prefixes.
fn mcp_launch_path(path: &Path) -> String {
    normalize_windows_launch_path(path.display().to_string())
}

/// Normalize Windows extended path prefixes for external MCP launchers.
#[cfg(windows)]
fn normalize_windows_launch_path(path: String) -> String {
    const DEVICE_PREFIX: &str = r"\\?\";
    const DEVICE_PREFIX_ALT: &str = "//?/";
    const UNC_DEVICE_PREFIX: &str = r"\\?\UNC\";
    const UNC_DEVICE_PREFIX_ALT: &str = "//?/UNC/";
    let normalized = if let Some(rest) = path.strip_prefix(UNC_DEVICE_PREFIX) {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(UNC_DEVICE_PREFIX_ALT) {
        format!("//{rest}")
    } else if let Some(rest) = path.strip_prefix(DEVICE_PREFIX) {
        rest.to_string()
    } else if let Some(rest) = path.strip_prefix(DEVICE_PREFIX_ALT) {
        rest.to_string()
    } else {
        path
    };
    normalized.replace('/', "\\")
}

/// Return non-Windows paths unchanged.
#[cfg(not(windows))]
fn normalize_windows_launch_path(path: String) -> String {
    path
}

/// Render MCP configuration as TOON for agents.
fn render_mcp_config_report(report: &McpConfigDocument) -> String {
    encode_agent_payload(&json!({ "mcp_config": report }))
}

/// Build stable runtime identity and capability information.
fn build_runtime_info() -> RuntimeInfoReport {
    RuntimeInfoReport {
        project: "ProjectAtlas".to_string(),
        major_version: PROJECTATLAS_MAJOR_VERSION,
        version: env!("CARGO_PKG_VERSION").to_string(),
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

/// Stable runtime identity and capability report for installers.
#[derive(Debug, Serialize)]
struct RuntimeInfoReport {
    /// Product name.
    project: String,
    /// Major `ProjectAtlas` architecture version.
    major_version: u8,
    /// Cargo package version.
    version: String,
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
        mcp::required_mcp_surface_present(),
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
        "runtime-info",
        "purpose",
    ];
    let command = Cli::command();
    required.iter().all(|name| {
        command
            .get_subcommands()
            .any(|subcommand| subcommand.get_name() == *name)
    })
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
        event_kind_affects_index, primary_symbol_names, refresh_text_index_for_nodes,
        relation_targets, reset_index_files, suggest_file_purpose, summarize_symbol_graph,
        watch_path_affects_index, watch_path_requires_full_scan, watcher_status_report,
    };
    use super::{OutputFormat, build_runtime_info, render_token_dashboard, serialized_output};
    use notify::EventKind;
    use projectatlas_core::Node;
    use projectatlas_core::NodeKind;
    use projectatlas_core::symbols::{
        CodeSymbol, ParserKind, RelationKind, SymbolGraph, SymbolKind, SymbolRelation,
    };
    use projectatlas_core::telemetry::TokenOverview;
    use projectatlas_db::AtlasStore;
    use projectatlas_fs::ScanOptions;
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
