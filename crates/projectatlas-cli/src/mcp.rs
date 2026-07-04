//! Purpose: Serve `ProjectAtlas` repository intelligence over MCP.
//! Native MCP adapter for `ProjectAtlas` agent integrations.

use crate::atlas_map::load_atlas_config;
use crate::runtime::{
    DEFAULT_HEALTH_LIMIT, MAX_HEALTH_LIMIT, MAX_SYMBOL_FILE_BYTES, PurposeReviewRequest,
    ScanRuntimePlan, SymbolBuildOptions, build_settings_report, build_symbols_for_index,
    byte_count_to_tokens, canonical_project_root, config_root_mismatch_error,
    default_mcp_project_root, estimated_source_tokens_for_indexed_files,
    estimated_source_tokens_for_paths, file_summary_usage_baseline, normalized_folder_filter,
    open_atlas_store, purpose_curation_page, ranked_file_nodes, read_indexed_file_content,
    record_directory_walk_usage_estimate, record_usage_estimate, record_usage_text,
    render_health_page, render_purpose_curation_page, render_purpose_review_report,
    reset_index_files, review_purposes, run_scan_pipeline, run_watch_loop, strip_legacy_purpose,
    validated_indexed_file_key, watcher_status_report,
};
use crate::{
    CliError, DEFAULT_FILE_SUMMARY_LIMIT, OutputFormat, build_parity_report, render_code_slice,
    render_file_summary, render_parity_report, render_search_report, render_settings_report,
    render_token_dashboard, render_token_trend_dashboard, render_watch_status,
};
use projectatlas_core::health::Severity;
use projectatlas_core::outline::build_outline;
use projectatlas_core::telemetry::TokenTrendWindow;
use projectatlas_core::toon::{
    encode_agent_payload, render_nodes, render_outline, render_overview, render_symbol_relations,
    render_symbols, render_token_overview, render_token_trends,
};
use projectatlas_core::{
    NodeKind, PurposeSource, PurposeStatus, normalize_repo_path_prefix, validated_repo_node_key,
};
use projectatlas_db::{AtlasStore, HealthQuery, HealthResolution, HealthScope};
use projectatlas_service::{
    SymbolSliceSelector, build_file_summary, read_indexed_code_slice, read_symbol_slice,
    search_indexed_files,
};
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::schemars;
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// MCP tools required for the agent-first repository-intelligence surface.
pub(crate) const REQUIRED_MCP_TOOL_NAMES: &[&str] = &[
    MCP_TOOL_ATLAS_SET_PROJECT_PATH,
    MCP_TOOL_ATLAS_SCAN,
    MCP_TOOL_ATLAS_OVERVIEW,
    MCP_TOOL_ATLAS_FOLDERS,
    MCP_TOOL_ATLAS_FILES,
    MCP_TOOL_ATLAS_OUTLINE,
    MCP_TOOL_ATLAS_FILE_SUMMARY,
    MCP_TOOL_ATLAS_SEARCH,
    MCP_TOOL_ATLAS_SLICE,
    MCP_TOOL_ATLAS_SYMBOLS_BUILD,
    MCP_TOOL_ATLAS_SYMBOLS,
    MCP_TOOL_ATLAS_SYMBOL_RELATIONS,
    MCP_TOOL_ATLAS_HEALTH,
    MCP_TOOL_ATLAS_HEALTH_RESOLVE,
    MCP_TOOL_ATLAS_TOKEN_REPORT,
    MCP_TOOL_ATLAS_PARITY_REPORT,
    MCP_TOOL_ATLAS_SETTINGS,
    MCP_TOOL_ATLAS_WATCH_STATUS,
    MCP_TOOL_ATLAS_WATCH_ONCE,
    MCP_TOOL_ATLAS_STRIP_LEGACY_PURPOSE,
    MCP_TOOL_ATLAS_RESET_INDEX,
    MCP_TOOL_ATLAS_PURPOSE_QUEUE,
    MCP_TOOL_ATLAS_PURPOSE_SET,
    MCP_TOOL_ATLAS_PURPOSE_REVIEW,
];

/// MCP tool name for active project selection.
const MCP_TOOL_ATLAS_SET_PROJECT_PATH: &str = "atlas_set_project_path";
/// MCP tool name for repository scans.
const MCP_TOOL_ATLAS_SCAN: &str = "atlas_scan";
/// MCP tool name for repository overviews.
const MCP_TOOL_ATLAS_OVERVIEW: &str = "atlas_overview";
/// MCP tool name for folder ranking.
const MCP_TOOL_ATLAS_FOLDERS: &str = "atlas_folders";
/// MCP tool name for file ranking.
const MCP_TOOL_ATLAS_FILES: &str = "atlas_files";
/// MCP tool name for file outlines.
const MCP_TOOL_ATLAS_OUTLINE: &str = "atlas_outline";
/// MCP tool name for file summaries.
const MCP_TOOL_ATLAS_FILE_SUMMARY: &str = "atlas_file_summary";
/// MCP tool name for indexed search.
const MCP_TOOL_ATLAS_SEARCH: &str = "atlas_search";
/// MCP tool name for source slices.
const MCP_TOOL_ATLAS_SLICE: &str = "atlas_slice";
/// MCP tool name for symbol builds.
const MCP_TOOL_ATLAS_SYMBOLS_BUILD: &str = "atlas_symbols_build";
/// MCP tool name for symbol lookup.
const MCP_TOOL_ATLAS_SYMBOLS: &str = "atlas_symbols";
/// MCP tool name for symbol relation lookup.
const MCP_TOOL_ATLAS_SYMBOL_RELATIONS: &str = "atlas_symbol_relations";
/// MCP tool name for health pages.
const MCP_TOOL_ATLAS_HEALTH: &str = "atlas_health";
/// MCP tool name for health resolutions.
const MCP_TOOL_ATLAS_HEALTH_RESOLVE: &str = "atlas_health_resolve";
/// MCP tool name for token reports.
const MCP_TOOL_ATLAS_TOKEN_REPORT: &str = "atlas_token_report";
/// MCP tool name for parity reports.
const MCP_TOOL_ATLAS_PARITY_REPORT: &str = "atlas_parity_report";
/// MCP tool name for settings reports.
const MCP_TOOL_ATLAS_SETTINGS: &str = "atlas_settings";
/// MCP tool name for watcher status.
const MCP_TOOL_ATLAS_WATCH_STATUS: &str = "atlas_watch_status";
/// MCP tool name for one-shot watcher refreshes.
const MCP_TOOL_ATLAS_WATCH_ONCE: &str = "atlas_watch_once";
/// MCP tool name for legacy purpose cleanup.
const MCP_TOOL_ATLAS_STRIP_LEGACY_PURPOSE: &str = "atlas_strip_legacy_purpose";
/// MCP tool name for runtime index reset.
const MCP_TOOL_ATLAS_RESET_INDEX: &str = "atlas_reset_index";
/// MCP tool name for purpose queue lookup.
const MCP_TOOL_ATLAS_PURPOSE_QUEUE: &str = "atlas_purpose_queue";
/// MCP tool name for purpose updates.
const MCP_TOOL_ATLAS_PURPOSE_SET: &str = "atlas_purpose_set";
/// MCP tool name for purpose reviews.
const MCP_TOOL_ATLAS_PURPOSE_REVIEW: &str = "atlas_purpose_review";
/// `ProjectAtlas` local state directory name.
const PROJECTATLAS_DIR_NAME: &str = ".projectatlas";
/// `ProjectAtlas` `SQLite` database filename.
const PROJECTATLAS_DB_FILE_NAME: &str = "projectatlas.db";
/// Project-local nested config filename.
const PROJECTATLAS_CONFIG_FILE_NAME: &str = "config.toml";
/// Project-local flat config filename.
const PROJECTATLAS_FLAT_CONFIG_FILE_NAME: &str = "projectatlas.toml";
/// Missing-index recovery guidance for MCP tools.
const MISSING_INDEX_GUIDANCE: &str =
    "run atlas_scan with project_path or atlas_set_project_path first";
/// Recovery guidance when a path names a subfolder rather than another selected root.
const SELECTED_ROOT_ASSERTION_GUIDANCE: &str = "pass project_path or call atlas_set_project_path for another repository, or use normal filesystem tools for files inside the selected project";
/// Recovery guidance when a path escapes the selected `ProjectAtlas` root.
const OUTSIDE_SELECTED_PROJECT_GUIDANCE: &str = "pass project_path or call atlas_set_project_path for that repository, or use normal filesystem tools for files outside the selected ProjectAtlas project";
/// Current-directory root alias.
const CURRENT_DIR_ALIAS: &str = ".";
/// `ProjectAtlas` MCP server display name.
const MCP_SERVER_NAME: &str = "ProjectAtlas";
/// MCP error lock-poison message.
const MCP_PROJECT_STATE_LOCK_POISONED: &str = "MCP project state lock poisoned";
/// Prefix used only if structured MCP error serialization fails.
const MCP_ERROR_SERIALIZATION_FALLBACK_PREFIX: &str = "error: ";
/// MCP payload key for scan reports.
const MCP_PAYLOAD_SCAN: &str = "scan";
/// MCP payload key for symbol-build reports.
const MCP_PAYLOAD_SYMBOLS_BUILD: &str = "symbols_build";
/// MCP payload key for health-resolution reports.
const MCP_PAYLOAD_HEALTH_RESOLUTION: &str = "health_resolution";
/// MCP payload key for token trend reports.
const MCP_PAYLOAD_TOKEN_TRENDS: &str = "token_trends";
/// MCP payload key for token savings reports.
const MCP_PAYLOAD_TOKEN_SAVINGS: &str = "token_savings";
/// MCP payload key for optional chart strings.
const MCP_PAYLOAD_CHART: &str = "chart";
/// MCP payload key for watcher reports.
const MCP_PAYLOAD_WATCH: &str = "watch";
/// MCP payload key for legacy-purpose migration reports.
const MCP_PAYLOAD_LEGACY_PURPOSE_MIGRATION: &str = "legacy_purpose_migration";
/// MCP payload key for reset-index reports.
const MCP_PAYLOAD_RESET_INDEX: &str = "reset_index";
/// MCP telemetry event for overview calls.
const MCP_EVENT_ATLAS_OVERVIEW: &str = "mcp.atlas_overview";
/// MCP telemetry event for folder calls.
const MCP_EVENT_ATLAS_FOLDERS: &str = "mcp.atlas_folders";
/// MCP telemetry event for file calls.
const MCP_EVENT_ATLAS_FILES: &str = "mcp.atlas_files";
/// MCP telemetry event for outline calls.
const MCP_EVENT_ATLAS_OUTLINE: &str = "mcp.atlas_outline";
/// MCP telemetry event for file-summary calls.
const MCP_EVENT_ATLAS_FILE_SUMMARY: &str = "mcp.atlas_file_summary";
/// MCP telemetry event for search calls.
const MCP_EVENT_ATLAS_SEARCH: &str = "mcp.atlas_search";
/// MCP telemetry event for slice calls.
const MCP_EVENT_ATLAS_SLICE: &str = "mcp.atlas_slice";
/// MCP telemetry event for symbol calls.
const MCP_EVENT_ATLAS_SYMBOLS: &str = "mcp.atlas_symbols";
/// MCP telemetry event for symbol-relation calls.
const MCP_EVENT_ATLAS_SYMBOL_RELATIONS: &str = "mcp.atlas_symbol_relations";
/// MCP telemetry event for health calls.
const MCP_EVENT_ATLAS_HEALTH: &str = "mcp.atlas_health";
/// MCP telemetry event for purpose-queue calls.
const MCP_EVENT_ATLAS_PURPOSE_QUEUE: &str = "mcp.atlas_purpose_queue";
/// Node payload label for rendered folder rows.
const NODE_LABEL_FOLDERS: &str = "folders";
/// Node payload label for rendered file rows.
const NODE_LABEL_FILES: &str = "files";
/// Error when a symbol disambiguator is supplied without a symbol name.
const SYMBOL_DISAMBIGUATOR_WITHOUT_SYMBOL_ERROR: &str = "symbol disambiguators require symbol";
/// Error when a line slice omits its start line.
const START_LINE_REQUIRED_ERROR: &str = "start_line is required unless symbol is provided";
/// Separator for diagnostic lists of accepted severity names.
const SEVERITY_EXPECTED_SEPARATOR: &str = ", ";
/// Final separator for diagnostic lists of accepted severity names.
const SEVERITY_EXPECTED_FINAL_SEPARATOR: &str = ", or ";
/// Token trend validation error suffix.
const TOKEN_TREND_WINDOW_ERROR_SUFFIX: &str = "expected day, week, month, or year";
/// Watch-status recommendation when no index exists.
const WATCH_STATUS_SCAN_RECOMMENDATION: &str =
    " Run `atlas_scan` first when no ProjectAtlas index exists for this project.";
/// Agent-facing MCP server instructions.
const MCP_SERVER_INSTRUCTIONS: &str = "ProjectAtlas provides TOON-first repository orientation, folder/file ranking, structured file summaries, symbol graph lookup, exact slices, health checks, and token telemetry for coding agents.";

/// Optional active-project override accepted by MCP tools.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasProjectParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
}

/// MCP parameter payload for selecting the active project.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasSetProjectPathParams {
    /// Project root to make active for calls that omit `project_path`.
    project_path: String,
}

/// Run the official RMCP stdio server.
pub(crate) fn run_mcp_server(
    db_path: PathBuf,
    config_path: Option<PathBuf>,
    session: String,
) -> Result<(), CliError> {
    let server = ProjectAtlasMcpServer::new(db_path, config_path, session);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
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

/// Return whether the compiled MCP surface contains required tool families.
pub(crate) fn required_mcp_surface_present() -> bool {
    REQUIRED_MCP_TOOL_NAMES
        .iter()
        .all(|name| mcp_tool_route_present(name))
}

/// Return whether the generated RMCP router has a concrete tool route.
pub(crate) fn mcp_tool_route_present(name: &str) -> bool {
    ProjectAtlasMcpServer::tool_router().has_route(name)
}
/// MCP parameter payload for scanning and symbol refresh.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasScanParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Repository root path. Defaults to the configured or indexed project root.
    path: Option<String>,
    /// Maximum file size to parse for symbols.
    max_bytes: Option<u64>,
    /// Maximum parser worker threads.
    max_workers: Option<usize>,
    /// Stop starting parser work after this many seconds.
    timeout_seconds: Option<u64>,
    /// Maximum UTF-8 file size persisted into `SQLite` text search.
    text_index_max_bytes: Option<u64>,
}

/// MCP parameter payload for one-shot watcher refresh.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasWatchOnceParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Repository root path. Defaults to the configured or indexed project root.
    path: Option<String>,
    /// Maximum parser worker threads.
    max_workers: Option<usize>,
    /// Stop starting parser work after this many seconds.
    timeout_seconds: Option<u64>,
    /// Maximum UTF-8 file size persisted into `SQLite` text search.
    text_index_max_bytes: Option<u64>,
}

/// MCP parameter payload for ranked node lookup.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasQueryParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Search query for path and purpose matching.
    query: Option<String>,
    /// Folder path to constrain file lookup.
    folder: Option<String>,
    /// Optional repository-relative glob filter.
    file_pattern: Option<String>,
    /// Include indexed file text as a bounded fallback ranking signal.
    include_content: Option<bool>,
    /// Maximum number of rows to return.
    limit: Option<usize>,
}

/// MCP parameter payload for outlining a file.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasOutlineParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Repository-relative file path.
    file: String,
    /// Number of non-empty preview lines to include.
    lines: Option<usize>,
}

/// MCP parameter payload for deterministic file summaries.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasFileSummaryParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Repository-relative file path.
    file: String,
    /// Maximum rows per functions/methods/classes/types/calls section.
    limit: Option<usize>,
}

/// MCP parameter payload for text search.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasSearchParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
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
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
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
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
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
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional session id filter.
    session: Option<String>,
    /// Include a readable ASCII chart in the MCP result.
    include_chart: Option<bool>,
    /// Optional trend grouping window: day, week, month, or year.
    trend_window: Option<String>,
}

/// MCP parameter payload for bounded health finding lookup.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasHealthParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Pagination start index after filters are applied.
    start_index: Option<usize>,
    /// Maximum findings to return, capped to a safe MCP page size.
    limit: Option<usize>,
    /// Optional finding category filter.
    category: Option<String>,
    /// Optional severity filter: info, warning, or error.
    severity: Option<String>,
    /// Optional repository-relative primary or related path prefix.
    path_prefix: Option<String>,
    /// Return counts and paging metadata without finding rows.
    summary_only: Option<bool>,
    /// Restrict findings to source files and folders that contain source files.
    source_only: Option<bool>,
    /// Include non-source files and asset-only folders in the purpose queue.
    include_assets: Option<bool>,
    /// Include low-priority files in the purpose queue.
    include_low_priority_files: Option<bool>,
}

/// MCP parameter payload for parity reports.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasParityParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Parity profile. Defaults to repository-intelligence.
    profile: Option<String>,
}

/// MCP parameter payload for legacy purpose cleanup.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasStripLegacyParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Repository root path. Defaults to the configured or indexed project root.
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
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
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
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Indexed repository-relative path.
    path: String,
    /// Agent-approved purpose one-liner.
    purpose: String,
}

/// MCP payload for one batch purpose review item.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasPurposeReviewItem {
    /// Indexed repository-relative path.
    path: String,
    /// Agent-reviewed purpose one-liner. Required for generated suggestions.
    purpose: Option<String>,
    /// Confirm the existing non-generated purpose after inspection.
    confirm_existing: Option<bool>,
}

/// MCP parameter payload for batch purpose review.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasPurposeReviewParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Purpose records to agent-review.
    items: Vec<AtlasPurposeReviewItem>,
    /// Apply reviewed purposes. Defaults to false for preview.
    apply: Option<bool>,
}

/// MCP parameter payload for resolving health findings.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasHealthResolveParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
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

/// Active `ProjectAtlas` database and configuration selected for MCP calls.
#[derive(Debug, Clone)]
struct McpProjectState {
    /// Canonical selected repository root.
    root: PathBuf,
    /// Selected durable `SQLite` index path.
    db_path: PathBuf,
    /// Selected scan/import configuration path.
    config_path: Option<PathBuf>,
}

/// Agent-facing error payload for failed MCP calls.
#[derive(Debug, Serialize)]
struct McpErrorResponse {
    /// Structured MCP error details.
    error: McpErrorPayload,
}

/// Stable serialized schema for MCP error details.
#[derive(Debug, Serialize)]
struct McpErrorPayload {
    /// Human-readable error and recovery guidance.
    message: String,
}

/// Agent-facing payload for the selected MCP project.
#[derive(Debug, Serialize)]
struct McpProjectStateResponse {
    /// Selected project details.
    project: McpProjectStatePayload,
}

/// Stable serialized schema for the selected MCP project.
#[derive(Debug, Serialize)]
struct McpProjectStatePayload {
    /// Canonical repository root.
    root: String,
    /// Selected durable `SQLite` index path.
    db: String,
    /// Selected configuration path when present.
    config: Option<String>,
    /// Active selection status.
    status: McpProjectStatus,
}

/// Status values for MCP project selection responses.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpProjectStatus {
    /// The selected project is active for defaulted MCP calls.
    Active,
}

/// Agent-facing payload for an MCP purpose update.
#[derive(Debug, Serialize)]
struct McpPurposeSetResponse {
    /// Purpose update result details.
    purpose_set: McpPurposeSetPayload,
}

/// Stable serialized schema for an MCP purpose update.
#[derive(Debug, Serialize)]
struct McpPurposeSetPayload {
    /// Indexed repository-relative path whose purpose was updated.
    path: String,
    /// Durable purpose status after the update.
    status: PurposeStatus,
    /// Source of the durable purpose after the update.
    source: PurposeSource,
    /// Whether the purpose has been agent-reviewed.
    agent_reviewed: bool,
}

/// Native `ProjectAtlas` MCP server backed by the same services as the CLI.
#[derive(Debug, Clone)]
pub(crate) struct ProjectAtlasMcpServer {
    /// Active project state for calls that omit `project_path`.
    project_state: Arc<RwLock<McpProjectState>>,
    /// Token telemetry session id.
    session: String,
    /// Official RMCP tool router.
    tool_router: ToolRouter<Self>,
}

impl ProjectAtlasMcpServer {
    /// Create a `ProjectAtlas` MCP server instance.
    pub(crate) fn new(db_path: PathBuf, config_path: Option<PathBuf>, session: String) -> Self {
        Self {
            project_state: Arc::new(RwLock::new(Self::startup_project_state(
                db_path,
                config_path,
            ))),
            session,
            tool_router: Self::tool_router(),
        }
    }

    /// Open the durable index without creating a new project database.
    fn open_store(state: &McpProjectState) -> Result<AtlasStore, CliError> {
        if !state.db_path.exists() {
            return Err(CliError::InvalidInput(format!(
                "ProjectAtlas index '{}' is missing for selected project root '{}'; {MISSING_INDEX_GUIDANCE}",
                state.db_path.display(),
                state.root.display()
            )));
        }
        AtlasStore::open(&state.db_path).map_err(CliError::from)
    }

    /// Open the durable index for mutation.
    fn open_mut_store(state: &McpProjectState) -> Result<AtlasStore, CliError> {
        open_atlas_store(&state.db_path)
    }

    /// Build startup state from CLI-supplied DB/config paths.
    fn startup_project_state(db_path: PathBuf, config_path: Option<PathBuf>) -> McpProjectState {
        let root = Self::startup_project_root(&db_path, config_path.as_deref());
        let config_path = config_path.filter(|path| Self::config_matches_project_root(&root, path));
        McpProjectState {
            root,
            db_path,
            config_path,
        }
    }

    /// Resolve startup root best-effort so server construction stays infallible.
    fn startup_project_root(db_path: &Path, config_path: Option<&Path>) -> PathBuf {
        if let Ok(root) = default_mcp_project_root(db_path, config_path) {
            return root;
        }
        if let Some(root) = Self::project_root_from_default_db_path(db_path) {
            return root;
        }
        std::env::current_dir()
            .ok()
            .and_then(|root| canonical_project_root(&root).ok())
            .unwrap_or_else(|| PathBuf::from(CURRENT_DIR_ALIAS))
    }

    /// Infer a root from a conventional `root/.projectatlas/projectatlas.db` path.
    fn project_root_from_default_db_path(db_path: &Path) -> Option<PathBuf> {
        let atlas_dir = db_path.parent()?;
        if atlas_dir.file_name()? != PROJECTATLAS_DIR_NAME {
            return None;
        }
        let root = atlas_dir.parent()?;
        canonical_project_root(root)
            .ok()
            .or_else(|| Some(root.to_path_buf()))
    }

    /// Read the active MCP project state.
    fn active_project_state(&self) -> Result<McpProjectState, CliError> {
        self.project_state
            .read()
            .map(|state| state.clone())
            .map_err(|_poisoned| CliError::Mcp(MCP_PROJECT_STATE_LOCK_POISONED.to_string()))
    }

    /// Replace the active MCP project state.
    fn set_active_project_state(&self, state: McpProjectState) -> Result<(), CliError> {
        *self
            .project_state
            .write()
            .map_err(|_poisoned| CliError::Mcp(MCP_PROJECT_STATE_LOCK_POISONED.to_string()))? =
            state;
        Ok(())
    }

    /// Return active state or a per-call project override.
    fn state_for_project_path(
        &self,
        project_path: Option<String>,
    ) -> Result<McpProjectState, CliError> {
        let project_path = Self::normalized_optional_path(project_path);
        project_path.map_or_else(
            || self.active_project_state(),
            |path| Self::project_state_from_root(Path::new(&path)),
        )
    }

    /// Return selected state and validate an optional root assertion.
    fn state_and_root_path(
        &self,
        project_path: Option<String>,
        path: Option<String>,
    ) -> Result<(McpProjectState, PathBuf), CliError> {
        let state = self.state_for_project_path(project_path.clone())?;
        let root = match (
            Self::normalized_optional_path(project_path),
            Self::normalized_optional_path(path),
        ) {
            (None, Some(path)) => match Self::path_or_project_root(&state, Some(path.clone())) {
                Ok(root) => root,
                Err(active_error) => {
                    let Some(indexed_state) =
                        Self::project_state_from_existing_indexed_root(&path)?
                    else {
                        return Err(active_error);
                    };
                    let root = indexed_state.root.clone();
                    return Ok((indexed_state, root));
                }
            },
            (_, path) => Self::path_or_project_root(&state, path)?,
        };
        Ok((state, root))
    }

    /// Normalize optional project/root path text from MCP payloads.
    fn normalized_optional_path(path: Option<String>) -> Option<String> {
        path.map(|path| path.trim().to_string())
            .filter(|path| !path.is_empty())
    }

    /// Build `ProjectAtlas` state for one project root.
    fn project_state_from_root(root: &Path) -> Result<McpProjectState, CliError> {
        let root = canonical_project_root(root)?;
        if !root.is_dir() {
            return Err(CliError::InvalidInput(format!(
                "project path '{}' is not a directory",
                root.display()
            )));
        }
        let db_path = Self::projectatlas_db_path(&root);
        let config_path = Self::config_path_for_project_root(&root)?;
        Ok(McpProjectState {
            root,
            db_path,
            config_path,
        })
    }

    /// Build project state from an explicitly addressed root only when it is already indexed.
    fn project_state_from_existing_indexed_root(
        root: &str,
    ) -> Result<Option<McpProjectState>, CliError> {
        let Ok(root) = canonical_project_root(Path::new(root)) else {
            return Ok(None);
        };
        if !root.is_dir() {
            return Ok(None);
        }
        let db_path = Self::projectatlas_db_path(&root);
        if !db_path.is_file() {
            return Ok(None);
        }
        let config_path = Self::config_path_for_project_root(&root)?;
        Ok(Some(McpProjectState {
            root,
            db_path,
            config_path,
        }))
    }

    /// Return the standard `ProjectAtlas` DB path for one project root.
    fn projectatlas_db_path(root: &Path) -> PathBuf {
        root.join(PROJECTATLAS_DIR_NAME)
            .join(PROJECTATLAS_DB_FILE_NAME)
    }

    /// Return the standard nested `ProjectAtlas` config path for one project root.
    fn projectatlas_nested_config_path(root: &Path) -> PathBuf {
        root.join(PROJECTATLAS_DIR_NAME)
            .join(PROJECTATLAS_CONFIG_FILE_NAME)
    }

    /// Return the flat `ProjectAtlas` config path for one project root.
    fn projectatlas_flat_config_path(root: &Path) -> PathBuf {
        root.join(PROJECTATLAS_FLAT_CONFIG_FILE_NAME)
    }

    /// Find a project-local config and reject stale configs pointing at another root.
    fn config_path_for_project_root(root: &Path) -> Result<Option<PathBuf>, CliError> {
        for config_path in [
            Self::projectatlas_nested_config_path(root),
            Self::projectatlas_flat_config_path(root),
        ] {
            if config_path.exists() {
                Self::validate_project_config_root(root, &config_path)?;
                return Ok(Some(config_path));
            }
        }
        Ok(None)
    }

    /// Ensure a selected project's config cannot redirect the MCP root.
    fn validate_project_config_root(root: &Path, config_path: &Path) -> Result<(), CliError> {
        let config = load_atlas_config(Some(config_path))?;
        let config_root = canonical_project_root(&config.root)?;
        if config_root != root {
            return Err(config_root_mismatch_error(config_path, &config_root, root));
        }
        Ok(())
    }

    /// Return whether a startup config belongs to the selected root.
    fn config_matches_project_root(root: &Path, config_path: &Path) -> bool {
        Self::validate_project_config_root(root, config_path).is_ok()
    }

    /// Render active project state for agents.
    fn render_project_state(state: &McpProjectState) -> Result<String, CliError> {
        let payload = McpProjectStateResponse {
            project: McpProjectStatePayload {
                root: state.root.display().to_string(),
                db: state.db_path.display().to_string(),
                config: state
                    .config_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                status: McpProjectStatus::Active,
            },
        };
        Self::encode_serialized_payload(payload)
    }

    /// Return a path parameter or the selected project root.
    fn path_or_project_root(
        state: &McpProjectState,
        path: Option<String>,
    ) -> Result<PathBuf, CliError> {
        let Some(value) = path else {
            return Ok(state.root.clone());
        };
        if value.is_empty() {
            return Ok(state.root.clone());
        }
        let original = value.clone();
        let candidate = PathBuf::from(value);
        let resolved = if candidate.is_absolute() {
            candidate
        } else {
            state.root.join(candidate)
        };
        let resolved = canonical_project_root(&resolved)?;
        if resolved == state.root {
            Ok(resolved)
        } else if resolved.starts_with(&state.root) {
            let resolved_display = resolved.display();
            let project_root_display = state.root.display();
            Err(CliError::InvalidInput(format!(
                "MCP path '{original}' resolves to '{resolved_display}', not the selected project root '{project_root_display}'; {SELECTED_ROOT_ASSERTION_GUIDANCE}"
            )))
        } else {
            let resolved_display = resolved.display();
            let project_root_display = state.root.display();
            Err(CliError::InvalidInput(format!(
                "MCP path '{original}' resolves to '{resolved_display}', outside the selected project root '{project_root_display}'; {OUTSIDE_SELECTED_PROJECT_GUIDANCE}"
            )))
        }
    }

    /// Validate an MCP purpose path as an indexed folder or file key.
    fn validated_indexed_node_key(store: &AtlasStore, path: &str) -> Result<String, CliError> {
        let node_key = validated_repo_node_key(std::path::Path::new(path))
            .map_err(Self::selected_project_path_error)?;
        if store.load_node_by_path(&node_key)?.is_none() {
            return Err(CliError::InvalidInput(format!(
                "path {node_key:?} is not indexed in the MCP-bound project"
            )));
        }
        Ok(node_key)
    }

    /// Validate an optional repository-relative MCP file key.
    fn validated_optional_file_key(path: Option<String>) -> Result<Option<String>, CliError> {
        path.map(|path| {
            validated_repo_node_key(std::path::Path::new(&path))
                .map_err(Self::selected_project_path_error)
        })
        .transpose()
    }

    /// Add selected-project guidance to repository-relative path errors.
    fn selected_project_path_error(message: impl std::fmt::Display) -> CliError {
        CliError::InvalidInput(format!("{message}; {OUTSIDE_SELECTED_PROJECT_GUIDANCE}"))
    }

    /// Return a query parameter with a stable default.
    fn query_or_empty(query: Option<String>) -> String {
        query.unwrap_or_default()
    }

    /// Encode one serializable payload as agent-readable TOON text.
    fn encode_serialized_payload<T>(payload: T) -> Result<String, CliError>
    where
        T: Serialize,
    {
        Ok(encode_agent_payload(&serde_json::to_value(payload)?))
    }

    /// Encode a dynamic top-level payload key without relying on `json!` key syntax.
    fn encode_named_payload<T>(key: &str, payload: &T) -> Result<String, CliError>
    where
        T: Serialize,
    {
        let mut object = serde_json::Map::new();
        object.insert(key.to_string(), serde_json::to_value(payload)?);
        Ok(encode_agent_payload(&serde_json::Value::Object(object)))
    }

    /// Encode two dynamic top-level payload keys without relying on `json!` key syntax.
    fn encode_two_named_payloads<T, U>(
        first_key: &str,
        first_payload: &T,
        second_key: &str,
        second_payload: &U,
    ) -> Result<String, CliError>
    where
        T: Serialize,
        U: Serialize,
    {
        let mut object = serde_json::Map::new();
        object.insert(first_key.to_string(), serde_json::to_value(first_payload)?);
        object.insert(
            second_key.to_string(),
            serde_json::to_value(second_payload)?,
        );
        Ok(encode_agent_payload(&serde_json::Value::Object(object)))
    }

    /// Encode an MCP error as a structured agent-readable payload.
    fn encode_error_payload(error: &CliError) -> String {
        let payload = McpErrorResponse {
            error: McpErrorPayload {
                message: error.to_string(),
            },
        };
        serde_json::to_value(payload).map_or_else(
            |source| {
                let mut message = MCP_ERROR_SERIALIZATION_FALLBACK_PREFIX.to_string();
                message.push_str(&source.to_string());
                message
            },
            |value| encode_agent_payload(&value),
        )
    }

    /// Convert a command result into an agent-readable TOON MCP text payload.
    fn as_mcp_text(result: Result<String, CliError>) -> String {
        match result {
            Ok(text) => text,
            Err(error) => Self::encode_error_payload(&error),
        }
    }
}

/// Convert MCP health parameters into a DB health query.
fn health_query_from_params(
    params: &AtlasHealthParams,
    scope: HealthScope,
) -> Result<HealthQuery, CliError> {
    Ok(HealthQuery {
        start_index: params.start_index.unwrap_or(0),
        limit: params
            .limit
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_HEALTH_LIMIT)
            .min(MAX_HEALTH_LIMIT),
        category: trimmed_filter(params.category.as_deref()),
        severity: trimmed_filter(params.severity.as_deref())
            .as_deref()
            .map(parse_health_severity)
            .transpose()?,
        path_prefix: trimmed_filter(params.path_prefix.as_deref())
            .map(|value| normalize_repo_path_prefix(&value)),
        summary_only: params.summary_only.unwrap_or(false),
        scope,
    })
}

/// Return the DB scope for MCP purpose queue parameters.
fn purpose_queue_scope(params: &AtlasHealthParams) -> HealthScope {
    match (
        params.include_assets.unwrap_or(false),
        params.include_low_priority_files.unwrap_or(false),
    ) {
        (false, false) => HealthScope::purpose_default(),
        (true, false) => HealthScope::purpose_with_assets(),
        (false, true) => HealthScope::purpose_with_source_files(),
        (true, true) => HealthScope::all(),
    }
}

/// Return a trimmed non-empty string parameter.
fn trimmed_filter(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

/// Parse an MCP health severity filter.
fn parse_health_severity(value: &str) -> Result<Severity, CliError> {
    let trimmed = value.trim();
    trimmed.parse::<Severity>().map_err(|_source| {
        let expected = expected_health_severity_names();
        CliError::InvalidInput(format!(
            "invalid health severity '{trimmed}'; expected {expected}"
        ))
    })
}

/// Render accepted health severity names for diagnostics.
fn expected_health_severity_names() -> String {
    let mut expected = Severity::Info.as_str().to_string();
    expected.push_str(SEVERITY_EXPECTED_SEPARATOR);
    expected.push_str(Severity::Warning.as_str());
    expected.push_str(SEVERITY_EXPECTED_FINAL_SEPARATOR);
    expected.push_str(Severity::Error.as_str());
    expected
}

#[tool_router(router = tool_router)]
impl ProjectAtlasMcpServer {
    /// Select the active project root for subsequent MCP calls.
    #[tool(
        name = "atlas_set_project_path",
        description = "Select the active ProjectAtlas project root for later MCP calls that omit project_path."
    )]
    fn atlas_set_project_path(
        &self,
        Parameters(params): Parameters<AtlasSetProjectPathParams>,
    ) -> String {
        Self::as_mcp_text((|| {
            let state = Self::project_state_from_root(Path::new(&params.project_path))?;
            self.set_active_project_state(state.clone())?;
            Self::render_project_state(&state)
        })())
    }

    /// Scan a repository, import purpose metadata, rebuild symbols, and return an overview.
    #[tool(
        name = "atlas_scan",
        description = "Scan repository structure, import ProjectAtlas purpose metadata, rebuild symbols, and return a TOON overview."
    )]
    fn atlas_scan(&self, Parameters(params): Parameters<AtlasScanParams>) -> String {
        Self::as_mcp_text((|| {
            let (state, path) = self.state_and_root_path(params.project_path, params.path)?;
            let plan = ScanRuntimePlan::for_path(
                state.config_path.as_deref(),
                &path,
                params.text_index_max_bytes,
            )?;
            let mut store = Self::open_mut_store(&state)?;
            let symbol_options = SymbolBuildOptions::new(
                params.max_bytes.unwrap_or(MAX_SYMBOL_FILE_BYTES),
                params.max_workers,
                params.timeout_seconds,
            );
            let report = run_scan_pipeline(&mut store, &plan, &symbol_options)?;
            Self::encode_named_payload(MCP_PAYLOAD_SCAN, &report)
        })())
    }

    /// Return the indexed repository overview.
    #[tool(
        name = "atlas_overview",
        description = "Return a compact TOON overview of indexed files, folders, and purpose coverage."
    )]
    fn atlas_overview(&self, Parameters(params): Parameters<AtlasProjectParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.state_for_project_path(params.project_path)?;
            let store = Self::open_store(&state)?;
            let overview = store.overview()?;
            let toon = render_overview(&overview);
            record_directory_walk_usage_estimate(
                &store,
                &self.session,
                MCP_EVENT_ATLAS_OVERVIEW,
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
            let state = self.state_for_project_path(params.project_path)?;
            let store = Self::open_store(&state)?;
            let query = Self::query_or_empty(params.query);
            let selected = store.load_ranked_nodes(
                &query,
                NodeKind::Folder,
                None,
                params.limit.unwrap_or(10),
                0,
            )?;
            let toon = render_nodes(NODE_LABEL_FOLDERS, &selected);
            record_directory_walk_usage_estimate(
                &store,
                &self.session,
                MCP_EVENT_ATLAS_FOLDERS,
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
        description = "Rank repository files by query, purpose, optional folder, and optional indexed text fallback before an agent opens source."
    )]
    fn atlas_files(&self, Parameters(params): Parameters<AtlasQueryParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.state_for_project_path(params.project_path)?;
            let store = Self::open_store(&state)?;
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
                params.include_content.unwrap_or(false),
            )?;
            let baseline_tokens = estimated_source_tokens_for_indexed_files(
                &store,
                folder_filter.as_deref(),
                params.file_pattern.as_deref(),
            )?;
            let toon = render_nodes(NODE_LABEL_FILES, &selected);
            record_usage_estimate(
                &store,
                &self.session,
                MCP_EVENT_ATLAS_FILES,
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
            let state = self.state_for_project_path(params.project_path)?;
            let file = PathBuf::from(&params.file);
            let store = Self::open_store(&state)?;
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
                MCP_EVENT_ATLAS_OUTLINE,
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
        description = "Return structured TOON file intelligence: file purpose, content summary, imports, symbols, line ranges, and calls."
    )]
    fn atlas_file_summary(&self, Parameters(params): Parameters<AtlasFileSummaryParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.state_for_project_path(params.project_path)?;
            let store = Self::open_store(&state)?;
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
                MCP_EVENT_ATLAS_FILE_SUMMARY,
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
            let state = self.state_for_project_path(params.project_path.clone())?;
            let store = Self::open_store(&state)?;
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
                MCP_EVENT_ATLAS_SEARCH,
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
            let state = self.state_for_project_path(params.project_path)?;
            let file = PathBuf::from(&params.file);
            let store = Self::open_store(&state)?;
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
                        SYMBOL_DISAMBIGUATOR_WITHOUT_SYMBOL_ERROR.to_string(),
                    ));
                }
                let start_line = params
                    .start_line
                    .ok_or_else(|| CliError::InvalidInput(START_LINE_REQUIRED_ERROR.to_string()))?;
                read_indexed_code_slice(&store, &file, start_line, params.end_line)?
            };
            let toon = render_code_slice(&report);
            record_usage_text(
                &store,
                &self.session,
                MCP_EVENT_ATLAS_SLICE,
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
            let (state, path) = self.state_and_root_path(params.project_path, params.path)?;
            let mut store = Self::open_mut_store(&state)?;
            let options = SymbolBuildOptions::new(
                params.max_bytes.unwrap_or(MAX_SYMBOL_FILE_BYTES),
                params.max_workers,
                params.timeout_seconds,
            );
            let report = build_symbols_for_index(&mut store, &path, &options, None)?;
            Self::encode_named_payload(MCP_PAYLOAD_SYMBOLS_BUILD, &report)
        })())
    }

    /// List indexed symbols.
    #[tool(
        name = "atlas_symbols",
        description = "List indexed symbols by optional file and query as compact TOON."
    )]
    fn atlas_symbols(&self, Parameters(params): Parameters<AtlasSymbolsParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.state_for_project_path(params.project_path)?;
            let store = Self::open_store(&state)?;
            let file = Self::validated_optional_file_key(params.file)?;
            let symbols = store.load_symbols(
                file.as_deref(),
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
                MCP_EVENT_ATLAS_SYMBOLS,
                file,
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
            let state = self.state_for_project_path(params.project_path)?;
            let store = Self::open_store(&state)?;
            let file = Self::validated_optional_file_key(params.file)?;
            let relations = store.load_symbol_relations(
                file.as_deref(),
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
                MCP_EVENT_ATLAS_SYMBOL_RELATIONS,
                file,
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
        description = "Return a bounded ProjectAtlas structural health page with optional category, severity, and path-prefix filters."
    )]
    fn atlas_health(&self, Parameters(params): Parameters<AtlasHealthParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.state_for_project_path(params.project_path.clone())?;
            let store = Self::open_store(&state)?;
            let scope = if params.source_only.unwrap_or(false) {
                HealthScope::source_only()
            } else {
                HealthScope::all()
            };
            let query = health_query_from_params(&params, scope)?;
            let page =
                store.unresolved_health_findings_page(&store.resolved_health_ids()?, &query)?;
            let toon = render_health_page(&page, &query);
            record_directory_walk_usage_estimate(
                &store,
                &self.session,
                MCP_EVENT_ATLAS_HEALTH,
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
            let state = self.state_for_project_path(params.project_path)?;
            let store = Self::open_store(&state)?;
            let resolution = HealthResolution {
                finding_id: params.finding_id,
                category: params.category,
                path: params.path,
                related_path: params.related_path,
                rationale: params.rationale,
            };
            store.resolve_health_finding(&resolution)?;
            Self::encode_named_payload(MCP_PAYLOAD_HEALTH_RESOLUTION, &resolution)
        })())
    }

    /// Return token savings telemetry.
    #[tool(
        name = "atlas_token_report",
        description = "Return ProjectAtlas token-savings telemetry for the whole index or one session."
    )]
    fn atlas_token_report(&self, Parameters(params): Parameters<AtlasTokenParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.state_for_project_path(params.project_path.clone())?;
            let store = Self::open_store(&state)?;
            let include_chart = params.include_chart.unwrap_or(false);
            if let Some(window) = params.trend_window.as_deref() {
                let window = TokenTrendWindow::parse(window).ok_or_else(|| {
                    CliError::InvalidInput(format!(
                        "unsupported token trend window {window:?}; {TOKEN_TREND_WINDOW_ERROR_SUFFIX}"
                    ))
                })?;
                let report = store.token_trends(params.session.as_deref(), window)?;
                if include_chart {
                    let chart = render_token_trend_dashboard(&report);
                    return Self::encode_two_named_payloads(
                        MCP_PAYLOAD_TOKEN_TRENDS,
                        &report,
                        MCP_PAYLOAD_CHART,
                        &chart,
                    );
                }
                return Ok(render_token_trends(&report));
            }
            let overview = store.token_overview(params.session.as_deref())?;
            if include_chart {
                let chart = render_token_dashboard(&overview, params.session.as_deref());
                return Self::encode_two_named_payloads(
                    MCP_PAYLOAD_TOKEN_SAVINGS,
                    &overview,
                    MCP_PAYLOAD_CHART,
                    &chart,
                );
            }
            Ok(render_token_overview(&overview))
        })())
    }

    /// Return repository-intelligence parity readiness.
    #[tool(
        name = "atlas_parity_report",
        description = "Return a ProjectAtlas repository-intelligence parity gate report for release and agent-runtime readiness."
    )]
    fn atlas_parity_report(&self, Parameters(params): Parameters<AtlasParityParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.state_for_project_path(params.project_path)?;
            let store = Self::open_store(&state)?;
            let profile = params
                .profile
                .unwrap_or_else(|| crate::REPOSITORY_INTELLIGENCE_PROFILE.to_string());
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
    fn atlas_settings(&self, Parameters(params): Parameters<AtlasProjectParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.state_for_project_path(params.project_path)?;
            let report = build_settings_report(
                &state.db_path,
                state.config_path.as_deref(),
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
    fn atlas_watch_status(&self, Parameters(params): Parameters<AtlasProjectParams>) -> String {
        let state = match self.state_for_project_path(params.project_path) {
            Ok(state) => state,
            Err(error) => return Self::as_mcp_text(Err(error)),
        };
        let mut report = watcher_status_report(false);
        if !state.db_path.exists() {
            report
                .recommendation
                .push_str(WATCH_STATUS_SCAN_RECOMMENDATION);
        }
        Self::as_mcp_text(Ok(render_watch_status(&report)))
    }

    /// Run one incremental refresh pass.
    #[tool(
        name = "atlas_watch_once",
        description = "Run one MCP-safe watcher refresh pass over the repository and rebuild changed symbols, with optional worker, timeout, and text-index size controls."
    )]
    fn atlas_watch_once(&self, Parameters(params): Parameters<AtlasWatchOnceParams>) -> String {
        Self::as_mcp_text((|| {
            let (state, path) = self.state_and_root_path(params.project_path, params.path)?;
            let mut store = Self::open_mut_store(&state)?;
            let plan = ScanRuntimePlan::for_path(
                state.config_path.as_deref(),
                &path,
                params.text_index_max_bytes,
            )?;
            let symbol_options = SymbolBuildOptions::new(
                MAX_SYMBOL_FILE_BYTES,
                params.max_workers,
                params.timeout_seconds,
            );
            let report = run_watch_loop(
                &mut store,
                &plan.root,
                true,
                1,
                1,
                &symbol_options,
                &plan.scan_options,
                plan.text_options,
            )?;
            Self::encode_named_payload(MCP_PAYLOAD_WATCH, &report)
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
            let (state, path) = self.state_and_root_path(params.project_path, params.path)?;
            let report = strip_legacy_purpose(
                &path,
                state.config_path.as_deref(),
                params.apply.unwrap_or(false),
                params.dry_run.unwrap_or(false),
                params
                    .strip_source_headers
                    .unwrap_or_else(|| state.config_path.is_some()),
            )?;
            Self::encode_named_payload(MCP_PAYLOAD_LEGACY_PURPOSE_MIGRATION, &report)
        })())
    }

    /// Preview or remove local runtime index/cache files.
    #[tool(
        name = "atlas_reset_index",
        description = "Preview or clear ProjectAtlas local SQLite index/cache files for recovery."
    )]
    fn atlas_reset_index(&self, Parameters(params): Parameters<AtlasResetIndexParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.state_for_project_path(params.project_path)?;
            let report = reset_index_files(
                &state.db_path,
                params.apply.unwrap_or(false),
                params.dry_run.unwrap_or(false),
                params.include_mcp_config.unwrap_or(false),
            )?;
            Self::encode_named_payload(MCP_PAYLOAD_RESET_INDEX, &report)
        })())
    }

    /// Return a bounded purpose curation queue.
    #[tool(
        name = "atlas_purpose_queue",
        description = "Return a bounded folder-first queue of ProjectAtlas paths that need agent purpose curation."
    )]
    fn atlas_purpose_queue(&self, Parameters(params): Parameters<AtlasHealthParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.state_for_project_path(params.project_path.clone())?;
            let store = Self::open_store(&state)?;
            let query = health_query_from_params(&params, purpose_queue_scope(&params))?;
            let page = purpose_curation_page(&store, &query)?;
            let toon = render_purpose_curation_page(&page);
            record_directory_walk_usage_estimate(
                &store,
                &self.session,
                MCP_EVENT_ATLAS_PURPOSE_QUEUE,
                None,
                None,
                estimated_source_tokens_for_indexed_files(&store, None, None)?,
                &toon,
            )?;
            Ok(toon)
        })())
    }

    /// Set an agent-approved purpose in the durable index.
    #[tool(
        name = "atlas_purpose_set",
        description = "Set agent-approved ProjectAtlas purpose metadata for one indexed path."
    )]
    fn atlas_purpose_set(&self, Parameters(params): Parameters<AtlasPurposeSetParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.state_for_project_path(params.project_path)?;
            let store = Self::open_store(&state)?;
            let node_key = Self::validated_indexed_node_key(&store, &params.path)?;
            store.set_purpose(&node_key, &params.purpose, PurposeSource::Agent)?;
            Self::encode_serialized_payload(McpPurposeSetResponse {
                purpose_set: McpPurposeSetPayload {
                    path: node_key,
                    status: PurposeStatus::Approved,
                    source: PurposeSource::Agent,
                    agent_reviewed: true,
                },
            })
        })())
    }

    /// Batch-review existing purpose records through the MCP surface.
    #[tool(
        name = "atlas_purpose_review",
        description = "Preview or apply agent-reviewed ProjectAtlas purpose metadata for multiple indexed paths."
    )]
    fn atlas_purpose_review(
        &self,
        Parameters(params): Parameters<AtlasPurposeReviewParams>,
    ) -> String {
        Self::as_mcp_text((|| {
            let state = self.state_for_project_path(params.project_path)?;
            let store = Self::open_store(&state)?;
            let requests = params
                .items
                .into_iter()
                .map(|item| PurposeReviewRequest {
                    path: item.path,
                    purpose: item.purpose,
                    confirm_existing: item.confirm_existing.unwrap_or(false),
                })
                .collect::<Vec<_>>();
            let report = review_purposes(&store, &requests, params.apply.unwrap_or(false))?;
            Ok(render_purpose_review_report(&report))
        })())
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ProjectAtlasMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                MCP_SERVER_NAME,
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(MCP_SERVER_INSTRUCTIONS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io;

    fn require(condition: bool, message: &str) -> Result<(), Box<dyn std::error::Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message.to_string()).into())
        }
    }

    #[test]
    fn current_dir_alias_paths_use_active_mcp_project() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo-a");
        fs::create_dir(&repo)?;
        let db_path = repo.join(".projectatlas").join("projectatlas.db");
        let server = ProjectAtlasMcpServer::new(db_path, None, "mcp-test".to_string());
        let expected_root = canonical_project_root(&repo)?;

        let (_state, root) = server.state_and_root_path(None, Some("./".to_string()))?;
        require(
            root == expected_root,
            "current-dir alias did not use active root",
        )?;

        #[cfg(windows)]
        {
            let (_state, root) = server.state_and_root_path(None, Some(".\\".to_string()))?;
            require(
                root == expected_root,
                "windows current-dir alias did not use active root",
            )?;
        }

        Ok(())
    }

    #[test]
    fn selected_project_config_cannot_redirect_root() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let repo_a = temp.path().join("repo-a");
        let repo_b = temp.path().join("repo-b");
        fs::create_dir(&repo_a)?;
        fs::create_dir(&repo_b)?;
        fs::create_dir(repo_b.join(".projectatlas"))?;
        let escaped_repo_a = repo_a.to_string_lossy().replace('\\', "/");
        fs::write(
            repo_b.join(".projectatlas").join("config.toml"),
            format!("[project]\nroot = \"{escaped_repo_a}\"\n"),
        )?;

        let Err(error) = ProjectAtlasMcpServer::project_state_from_root(&repo_b) else {
            return Err(io::Error::other("stale selected-project config was accepted").into());
        };
        require(
            error.to_string().contains("outside selected project root"),
            "stale selected-project config error was not root-scoped",
        )?;

        Ok(())
    }

    #[test]
    fn startup_config_mismatch_cannot_bind_one_root_to_another_db()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let repo_a = temp.path().join("repo-a");
        let repo_b = temp.path().join("repo-b");
        fs::create_dir(&repo_a)?;
        fs::create_dir(&repo_b)?;
        fs::create_dir(repo_a.join(".projectatlas"))?;
        let escaped_repo_a = repo_a.to_string_lossy().replace('\\', "/");
        let config_a = repo_a.join(".projectatlas").join("config.toml");
        fs::write(
            &config_a,
            format!("[project]\nroot = \"{escaped_repo_a}\"\n"),
        )?;

        let db_b = repo_b.join(".projectatlas").join("projectatlas.db");
        let server =
            ProjectAtlasMcpServer::new(db_b.clone(), Some(config_a), "mcp-test".to_string());
        let state = server.active_project_state()?;

        require(
            state.root == canonical_project_root(&repo_b)?,
            "startup state did not fall back to the DB project root",
        )?;
        require(
            state.db_path == db_b,
            "startup state changed the selected DB path",
        )?;
        require(
            state.config_path.is_none(),
            "startup state retained a config from another project root",
        )?;

        Ok(())
    }

    #[test]
    fn read_only_store_does_not_create_missing_index() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo-a");
        fs::create_dir(&repo)?;
        let state = ProjectAtlasMcpServer::project_state_from_root(&repo)?;

        let Err(error) = ProjectAtlasMcpServer::open_store(&state) else {
            return Err(io::Error::other("missing index opened unexpectedly").into());
        };
        require(
            error.to_string().contains("index"),
            "missing index error did not mention index",
        )?;
        require(
            !repo.join(".projectatlas").exists(),
            "read-only store created .projectatlas",
        )?;

        Ok(())
    }
}
