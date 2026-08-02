//! Purpose: Provide the `ProjectAtlas` 3 command-line adapter.

mod atlas_map;
#[cfg(feature = "derived-snapshot")]
mod derived_snapshot_archive;
mod mcp;
mod runtime;
mod structural;
mod token_tui;

use atlas_map::{
    AtlasMapConfig, IgnoreEntryKind, LintOptions, add_ignore_entry, effective_config_report,
    init_gitignore, init_project_with_config, lint_map, list_ignore_entries, load_atlas_config,
    remove_ignore_entry, write_map,
};
use clap::parser::ValueSource;
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
#[cfg(feature = "optional-parser-supervisor")]
use projectatlas_cli::optional_parser_lifecycle::{
    OptionalParserPackLifecycle, OptionalParserPackLifecycleError,
};
use projectatlas_core::graph::{
    ConfidenceClass, GraphLimits, GraphRelationKind, LogicalRelation, RepositoryFilePath,
};
use projectatlas_core::health::Severity;
use projectatlas_core::outline::build_outline;
use projectatlas_core::telemetry::{
    TokenCalibrationOverview, TokenTrendWindow as CoreTokenTrendWindow, UsageInstanceOwner,
};
use projectatlas_core::toon::{
    encode_agent_payload, encode_error_text, render_outline, render_overview,
    render_ranked_node_rows, render_ranked_nodes, render_symbol_relations, render_symbols,
    render_token_overview, render_token_trends,
};
use projectatlas_core::{
    IndexWorkControl, IndexWorkStage, PurposeSource, PurposeStatus, normalize_native_path_display,
    normalize_repo_path_prefix,
};
use projectatlas_db::{
    AtlasStore, DbError, HealthQuery, HealthResolution, HealthScope, ProjectRootTransition,
    ProjectRootTransitionResult, RepositoryCoverageQuery, RepositoryGraphDirection,
    verify_project_database,
};
use projectatlas_service::{
    COVERAGE_PAGE_MAX_LIMIT, CodeSlice, CodeSliceBudget, CodeSliceDraft, CoverageDiscoveryReport,
    DetailedRelationBudget, DetailedRelationQuery, FederatedStore, FileSummaryReport,
    GitImpactSelection, RelationAnalysisMode, RelationAnalysisQuery, RelationAnchor,
    RelationDirection, RelationResolutionFilter, SearchQuery, SearchReport, SearchRetrievalMode,
    ServiceError, SymbolSliceSelector, TokenReport, TokenReportRequest,
    build_file_summary_from_source, load_coverage_discovery, load_detailed_relation_page,
    load_federated_detailed_relations, load_federated_relation_analysis, load_relation_analysis,
    load_token_report, parse_coverage_parser, parse_coverage_relation, parse_coverage_state,
    parse_symbol_kind, read_indexed_code_slice_from_source_bounded,
    read_symbol_slice_from_source_bounded, search_indexed_files_with_control,
};
use rmcp::schemars;
use runtime::{
    DEFAULT_HEALTH_LIMIT, InitBootstrapOptions, InitHostConfigStatus, InitSetupReport,
    MAX_HEALTH_LIMIT, MAX_PURPOSE_REVIEW_INPUT_FILE_BYTES, MAX_SYMBOL_FILE_BYTES, PurposeLintLevel,
    PurposeReviewRequest, ScanRuntimePlan, SettingsReport, SymbolBuildOptions,
    UsageRuntimeInstance, WatchStatusReport, absolute_path, build_settings_report,
    byte_count_to_tokens, canonical_project_root, canonical_source_project_root,
    config_root_mismatch_error, default_cli_project_root, default_mcp_project_root,
    defaultable_cli_project_root, estimated_source_tokens_for_indexed_files,
    estimated_source_tokens_for_paths, index_work_control, init_config_path, init_path_status,
    lint_database_if_present, next_step_report, next_step_report_payload, normalized_folder_filter,
    open_atlas_store_for_project, open_atlas_store_read_only_for_project,
    open_federated_atlas_stores_for_project, open_fresh_atlas_store_for_project,
    purpose_curation_page, ranked_file_nodes_with_reasons, ranked_folder_nodes_with_reasons,
    read_indexed_file_content, record_directory_walk_usage_estimate, record_usage_estimate,
    record_usage_text, render_coverage_report, render_health_page, render_purpose_curation_page,
    render_purpose_review_report, reset_index_files, resolved_mcp_config_path, review_purposes,
    run_init_bootstrap, run_scan_pipeline_controlled, run_single_watch_refresh_controlled,
    run_symbol_build_pipeline_controlled, run_watch_loop, standalone_index_work_control,
    strip_legacy_purpose, validate_purpose_review_admission, validated_indexed_file_key,
    watcher_status_report,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use token_tui::{
    TokenAtlasPreview, TokenDashboardTheme, render_token_dashboard_with_atlas,
    render_token_trend_dashboard_with_theme, token_atlas_network_relation,
    token_dashboard_wants_atlas,
};
#[cfg(test)]
use token_tui::{render_token_dashboard, render_token_dashboard_with_atlas_at_width};

/// Default relative path for the `SQLite` index.
const DEFAULT_DB_PATH: &str = ".projectatlas/projectatlas.db";
/// Whole-operation deadline for the optional live token-dashboard graph read.
const TOKEN_ATLAS_READ_TIMEOUT: Duration = Duration::from_secs(15);
/// `ProjectAtlas` major architecture version.
const PROJECTATLAS_MAJOR_VERSION: u8 = 3;
/// Default caller-visible compatibility label for token telemetry.
const DEFAULT_CALLER_LABEL: &str = "default";
/// Default maximum rows returned per structured file-summary section.
const DEFAULT_FILE_SUMMARY_LIMIT: usize = 25;
/// CLI top-level field for detailed and analysis relation responses.
const CLI_PAYLOAD_SYMBOL_RELATIONS: &str = "symbol_relations";
/// One-shot watcher refresh mode.
const WATCH_MODE_ONCE: &str = "single-refresh";
/// Event-backed watcher mode.
const WATCH_MODE_NOTIFY: &str = "notify";
/// Recovery guidance for a database whose local WAL-safe placement was rejected.
const DATABASE_FILESYSTEM_RECOVERY: &str = "Place the selected local source tree and its .projectatlas database on a supported local filesystem, resolve any mount or permission uncertainty, and retry; ProjectAtlas will not weaken the WAL durability profile.";
/// Recovery guidance for a database owned by another schema version.
const SCHEMA_VERSION_MISMATCH_RECOVERY: &str = "Use a ProjectAtlas runtime that supports this database schema; do not reset, downgrade, or repair the database with this runtime.";
/// Recovery guidance for an admitted predecessor database.
const SCHEMA_MIGRATION_REQUIRED_RECOVERY: &str = "Apply the supported database-owned migration without changing the selected database: for CLI, use the `projectatlas init` action from the selected project root while preserving the same global `--db`/`--config` selection; for MCP, call `atlas_init` through the same MCP server/database binding. Do not reset or replace the database.";
/// Existing CLI command that performs one bounded refresh pass.
const CLI_REFRESH_COMMAND: &str = "watch";
/// Existing CLI command that initializes one selected project root.
const CLI_INIT_COMMAND: &str = "init";
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
    #[cfg(feature = "derived-snapshot")]
    RequiredCliCommand::Snapshot,
    #[cfg(feature = "optional-parser-supervisor")]
    RequiredCliCommand::ParserPack,
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
    /// Cooperative index work was canceled or exceeded a declared bound.
    #[error("{0}")]
    IndexWork(#[from] projectatlas_core::IndexWorkFailure),
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
    /// Optional parser-pack lifecycle operation failed.
    #[cfg(feature = "optional-parser-supervisor")]
    #[error("{0}")]
    ParserPack(#[from] OptionalParserPackLifecycleError),
    /// Optional parsing failed and the requested process cleanup also failed.
    #[cfg(feature = "optional-parser-supervisor")]
    #[error(
        "optional parser operation failed: {operation}; mandatory cleanup also failed: {cleanup}"
    )]
    OptionalParserOperationAndCleanup {
        /// Original staging or parser failure.
        operation: Box<Self>,
        /// Cleanup failure observed before releasing process ownership.
        cleanup: Box<Self>,
    },
    /// User input was invalid.
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// The selected source root has not been initialized.
    #[error("{0}")]
    InitRequired(Box<runtime::IndexInitRequired>),
    /// A bare/common Git directory was selected instead of checked-out source.
    #[error("{0}")]
    WorktreeRequired(Box<runtime::ProjectWorktreeRequired>),
    /// Current local source differs from the durable index.
    #[error("{0}")]
    RefreshRequired(Box<runtime::IndexRefreshRequired>),
    /// Current local source could not be verified completely.
    #[error("{0}")]
    VerificationIncomplete(Box<runtime::IndexVerificationIncomplete>),
    /// The selected project root does not own the opened index.
    #[error("{0}")]
    ProjectMismatch(Box<runtime::IndexProjectMismatch>),
    /// A durable root transition committed before generated configuration failed.
    #[error(
        "root transition {transition:?} committed for {root:?}, but generated project configuration is incomplete; rerun `projectatlas root set {root:?}` with the default bind transition to repair it without repeating the transition: {source}"
    )]
    RootTransitionFollowup {
        /// Canonical root already committed to the database.
        root: String,
        /// Durable transition that already completed.
        transition: RootTransition,
        /// Follow-up configuration failure.
        #[source]
        source: Box<CliError>,
    },
}

/// Structured CLI error payload for typed agent-recoverable failures.
#[derive(Serialize)]
struct CliErrorResponse<'a> {
    /// Typed error details.
    error: CliErrorPayload<'a>,
}

/// Stable CLI error details shared by TOON and JSON output.
#[derive(Serialize)]
struct CliErrorPayload<'a> {
    /// Machine-readable error kind.
    kind: AgentErrorKind,
    /// Human-readable recovery guidance.
    message: String,
    /// Local-source mismatch details when a refresh is required.
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_required: Option<&'a runtime::IndexRefreshRequired>,
    /// Exact selected-root initialization handoff.
    #[serde(skip_serializing_if = "Option::is_none")]
    init_required: Option<&'a runtime::IndexInitRequired>,
    /// Bare/common Git root selection diagnostic.
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree_required: Option<&'a runtime::ProjectWorktreeRequired>,
    /// Source/policy diagnostic when verification cannot complete.
    #[serde(skip_serializing_if = "Option::is_none")]
    verification_incomplete: Option<&'a runtime::IndexVerificationIncomplete>,
    /// Project/index identity mismatch details.
    #[serde(skip_serializing_if = "Option::is_none")]
    project_mismatch: Option<&'a runtime::IndexProjectMismatch>,
    /// Content-free database placement details for a rejected `SQLite` profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    database_filesystem: Option<DatabaseFilesystemErrorPayload>,
    /// Optional retrieval capability state and recovery guidance.
    #[serde(skip_serializing_if = "Option::is_none")]
    search_capability: Option<SearchCapabilityErrorPayload>,
    /// Direct CLI recovery selector for a confirmed mismatch.
    #[serde(skip_serializing_if = "Option::is_none")]
    next: Option<CliNextCall<'a>>,
}

/// Stable error kinds shared by CLI and MCP agent payloads.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum AgentErrorKind {
    /// General command, input, storage, or service failure.
    Error,
    /// The selected project root needs explicit initialization.
    InitRequired,
    /// A checked-out source worktree must be selected.
    WorktreeRequired,
    /// Current saved local source differs from the durable index.
    RefreshRequired,
    /// Current saved local source could not be inspected completely.
    VerificationIncomplete,
    /// The selected project root does not own the opened index.
    ProjectMismatch,
    /// The database is on a known unsupported network or distributed filesystem.
    DatabaseFilesystemUnsupported,
    /// The database's required local filesystem guarantees could not be established.
    DatabaseFilesystemUncertain,
    /// The selected database schema is unsupported by this runtime.
    SchemaVersionMismatch,
    /// The selected database schema has a supported migration route.
    SchemaMigrationRequired,
    /// The host has no accepted optional parser containment adapter.
    #[cfg(feature = "optional-parser-supervisor")]
    UnsupportedContainment,
    /// The requested optional search mode has no ready generation.
    SearchCapabilityUnavailable,
}

/// Content-free database placement details with direct recovery guidance.
#[derive(Clone, Debug, Serialize)]
struct DatabaseFilesystemErrorPayload {
    /// Database path rejected before mutation.
    path: String,
    /// Resolved owning mount when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    mount_point: Option<String>,
    /// Normalized filesystem type when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    filesystem_type: Option<String>,
    /// Bounded reason when placement was uncertain.
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    /// Safe recovery action.
    recovery: &'static str,
}

/// Typed schema-version mismatch shared by CLI and MCP adapters.
#[derive(Clone, Debug, Serialize)]
struct SchemaVersionMismatchPayload {
    /// Schema version stored by the database owner.
    found_schema_version: i64,
    /// Schema version supported by this runtime.
    supported_schema_version: i64,
    /// Public `ProjectAtlas` package version executing the request.
    runtime_version: &'static str,
    /// Safe recovery action.
    recovery: &'static str,
}

/// CLI error details for an incompatible database schema.
#[derive(Serialize)]
struct SchemaVersionMismatchErrorPayload {
    /// Machine-readable error kind.
    kind: AgentErrorKind,
    /// Human-readable recovery guidance.
    message: String,
    /// Content-free incompatible schema details.
    schema_version_mismatch: SchemaVersionMismatchPayload,
}

/// Structured CLI response for an incompatible database schema.
#[derive(Serialize)]
struct SchemaVersionMismatchErrorResponse {
    /// Typed error details.
    error: SchemaVersionMismatchErrorPayload,
}

/// Typed supported-schema migration handoff shared by CLI and MCP adapters.
#[derive(Clone, Debug, Serialize)]
struct SchemaMigrationRequiredPayload {
    /// Schema version stored by the database owner.
    found_schema_version: i64,
    /// Schema version supported by this runtime.
    supported_schema_version: i64,
    /// Remaining ordered database-owned migration steps.
    migration_steps_remaining: u32,
    /// Public `ProjectAtlas` package version executing the request.
    runtime_version: &'static str,
    /// Safe migration action.
    recovery: &'static str,
}

impl SchemaMigrationRequiredPayload {
    /// Render one content-free explanation for a command that requires the current schema.
    fn message(&self) -> String {
        format!(
            "database schema version {} requires {} supported migration step(s) to version {} before this command can run",
            self.found_schema_version,
            self.migration_steps_remaining,
            self.supported_schema_version
        )
    }
}

/// CLI error details for an admitted predecessor schema.
#[derive(Serialize)]
struct SchemaMigrationRequiredErrorPayload {
    /// Machine-readable error kind.
    kind: AgentErrorKind,
    /// Human-readable migration handoff.
    message: String,
    /// Content-free supported migration details.
    schema_migration_required: SchemaMigrationRequiredPayload,
}

/// Structured CLI response for an admitted predecessor schema.
#[derive(Serialize)]
struct SchemaMigrationRequiredErrorResponse {
    /// Typed error details.
    error: SchemaMigrationRequiredErrorPayload,
}

/// Typed optional search-capability failure shared by CLI and MCP adapters.
#[derive(Clone, Debug, Serialize)]
struct SearchCapabilityErrorPayload {
    /// Explicit retrieval mode requested by the caller.
    requested_mode: SearchRetrievalMode,
    /// Stable optional-capability lifecycle state.
    state: &'static str,
    /// Actionable recovery guidance.
    recovery: &'static str,
}

/// Existing CLI command that repairs a confirmed stale index.
#[derive(Serialize)]
struct CliNextCall<'a> {
    /// Command family accepted by the current runtime.
    command: &'static str,
    /// Selected project root to refresh.
    project_path: &'a str,
    /// Run exactly one refresh cycle.
    #[serde(skip_serializing_if = "Option::is_none")]
    once: Option<bool>,
}

/// CLI output serialization format.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    /// Token-efficient object notation for agent-facing responses.
    Toon,
    /// Pretty JSON for scripts and external machine consumers.
    Json,
}

/// Symbol-relation response contract selected by the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum RelationViewArg {
    /// Preserve the v0.3 relation rows and ordering exactly.
    Legacy,
    /// Use bounded normalized-graph navigation.
    Detailed,
    /// Project one closed architecture, impact, or static-trace analysis.
    Analysis,
}

/// Closed relation analysis selected on the existing symbol-relations route.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum RelationAnalysisModeArg {
    /// Components, communities, cycles, purpose, complexity, and bottlenecks.
    Architecture,
    /// VCS-aware affected nodes and conservative dead-code candidates.
    Impact,
    /// One node-simple static relationship path.
    Trace,
}

impl From<RelationAnalysisModeArg> for RelationAnalysisMode {
    fn from(value: RelationAnalysisModeArg) -> Self {
        match value {
            RelationAnalysisModeArg::Architecture => Self::Architecture,
            RelationAnalysisModeArg::Impact => Self::Impact,
            RelationAnalysisModeArg::Trace => Self::Trace,
        }
    }
}

/// Detailed relation traversal direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum RelationDirectionArg {
    /// Follow relations from each source frontier.
    Outbound,
    /// Follow relations into each target frontier.
    Inbound,
}

impl From<RelationDirectionArg> for RelationDirection {
    fn from(value: RelationDirectionArg) -> Self {
        match value {
            RelationDirectionArg::Outbound => Self::Outbound,
            RelationDirectionArg::Inbound => Self::Inbound,
        }
    }
}

/// Lowest confidence retained by detailed relation navigation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum RelationConfidenceArg {
    /// Parser-proven exact relation.
    Exact,
    /// High-confidence inferred relation.
    High,
    /// Medium-confidence structural relation.
    Medium,
    /// Low-confidence fallback relation.
    Low,
}

impl From<RelationConfidenceArg> for ConfidenceClass {
    fn from(value: RelationConfidenceArg) -> Self {
        match value {
            RelationConfidenceArg::Exact => Self::Exact,
            RelationConfidenceArg::High => Self::High,
            RelationConfidenceArg::Medium => Self::Medium,
            RelationConfidenceArg::Low => Self::Low,
        }
    }
}

/// Resolution state retained by detailed relation navigation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum RelationResolutionArg {
    /// Retain every resolution state.
    Any,
    /// Retain exact local targets.
    Resolved,
    /// Retain ambiguous references.
    Ambiguous,
    /// Retain unresolved references.
    Unresolved,
    /// Retain external targets.
    External,
}

impl From<RelationResolutionArg> for RelationResolutionFilter {
    fn from(value: RelationResolutionArg) -> Self {
        match value {
            RelationResolutionArg::Any => Self::Any,
            RelationResolutionArg::Resolved => Self::Resolved,
            RelationResolutionArg::Ambiguous => Self::Ambiguous,
            RelationResolutionArg::Unresolved => Self::Unresolved,
            RelationResolutionArg::External => Self::External,
        }
    }
}

/// Additive options used only by the detailed relation view.
#[derive(Args, Debug)]
struct DetailedRelationArgs {
    /// Resume one exact generation- and purpose-bound detailed page.
    #[arg(long)]
    cursor: Option<String>,
    /// Complete ordered project-root set for one read-only federated call.
    #[arg(long = "root", value_name = "PATH")]
    roots: Vec<PathBuf>,
    /// Exact local anchor controls.
    #[command(flatten)]
    anchor: DetailedRelationAnchorArgs,
    /// Relation direction and trust filters.
    #[command(flatten)]
    filters: DetailedRelationFilterArgs,
    /// Traversal and output ceilings.
    #[command(flatten)]
    limits: DetailedRelationLimitArgs,
    /// Optional controls used only by the analysis view.
    #[command(flatten)]
    analysis: Box<RelationAnalysisArgs>,
}

/// Closed controls accepted only by `symbols relations --view analysis`.
#[derive(Args, Debug)]
struct RelationAnalysisArgs {
    /// Closed analysis projection computed over the bounded relation traversal.
    #[arg(long, value_enum)]
    analysis_mode: Option<RelationAnalysisModeArg>,
    /// Exact JSON `RelationAnchor` (`file` or fully disambiguated `symbol`) for trace mode.
    #[arg(long)]
    trace_target: Option<String>,
    /// Git scope: `working-tree`, `index`, or exact `base..head`; defaults to working-tree.
    #[arg(long)]
    vcs: Option<String>,
    /// Include relationship-derived communities with containment excluded.
    #[arg(long)]
    include_communities: bool,
    /// Include iterative SCC dependency-cycle findings.
    #[arg(long)]
    include_cycles: bool,
    /// Include conservative dead-code candidates in impact mode.
    #[arg(long)]
    include_dead_code: bool,
}

/// Exact local anchor controls for detailed relation navigation.
#[derive(Args, Debug)]
struct DetailedRelationAnchorArgs {
    /// Exact symbol name used as the detailed anchor; omit for a file anchor.
    #[arg(long)]
    symbol: Option<String>,
    /// Optional exact parent used to disambiguate the detailed symbol anchor.
    #[arg(long)]
    symbol_parent: Option<String>,
    /// Optional exact kind used to disambiguate the detailed symbol anchor.
    #[arg(long)]
    symbol_kind: Option<String>,
    /// Optional exact signature used to disambiguate the detailed symbol anchor.
    #[arg(long)]
    symbol_signature: Option<String>,
}

/// Direction and trust filters for detailed relation navigation.
#[derive(Args, Debug)]
struct DetailedRelationFilterArgs {
    /// Direction followed from every detailed frontier.
    #[arg(long, value_enum, default_value_t = RelationDirectionArg::Outbound)]
    direction: RelationDirectionArg,
    /// Optional exact legacy or extended relation family.
    #[arg(long)]
    relation: Option<String>,
    /// Lowest confidence retained by detailed navigation.
    #[arg(long, value_enum, default_value_t = RelationConfidenceArg::Low)]
    minimum_confidence: RelationConfidenceArg,
    /// Resolution state retained by detailed navigation.
    #[arg(long, value_enum, default_value_t = RelationResolutionArg::Any)]
    resolution: RelationResolutionArg,
}

/// Traversal and output ceilings for detailed relation navigation.
#[derive(Args, Debug)]
struct DetailedRelationLimitArgs {
    /// Maximum detailed traversal depth.
    #[arg(long, default_value_t = 1)]
    depth: u32,
    /// Retain bounded exact source occurrences in detailed rows.
    #[arg(long)]
    include_occurrences: bool,
    /// Maximum exact occurrences retained per detailed relation.
    #[arg(long, default_value_t = 25)]
    occurrence_limit: u32,
    /// Maximum adjacency rows inspected across the complete request.
    #[arg(long)]
    edge_limit: Option<u32>,
    /// Maximum unique traversal nodes retained across the complete request.
    #[arg(long)]
    node_limit: Option<u32>,
    /// Maximum unique visited identities retained across continuation pages.
    #[arg(long)]
    visited_limit: Option<u32>,
    /// Maximum exact occurrences retained across the complete request.
    #[arg(long)]
    occurrence_total_limit: Option<u32>,
    /// Maximum decoded, cursor, and service-composition intermediate bytes.
    #[arg(long)]
    intermediate_bytes: Option<u64>,
    /// Maximum service-owned elapsed milliseconds.
    #[arg(long)]
    deadline_ms: Option<u64>,
    /// Maximum encoded bytes admitted to the detailed response.
    #[arg(long, default_value_t = 256 * 1024)]
    output_bytes: u32,
}

/// Optional exact symbol selector shared by the top-level slice command.
#[derive(Args, Debug)]
struct OptionalSymbolSelectorArgs {
    /// Slice a symbol by name instead of passing line numbers.
    #[arg(long)]
    symbol: Option<String>,
    /// Optional parent symbol for disambiguating `--symbol`.
    #[arg(long)]
    symbol_parent: Option<String>,
    /// Optional symbol kind for disambiguating `--symbol`.
    #[arg(long)]
    symbol_kind: Option<String>,
    /// Optional exact symbol signature for disambiguating `--symbol`.
    #[arg(long)]
    symbol_signature: Option<String>,
    /// Optional source line for disambiguating `--symbol`.
    #[arg(long)]
    symbol_line: Option<usize>,
    /// Maximum encoded bytes admitted to the slice response.
    #[arg(long, default_value_t = CodeSliceBudget::DEFAULT_OUTPUT_BYTES)]
    output_bytes: u32,
}

/// Required exact symbol selector shared by the symbol slice command.
#[derive(Args, Debug)]
struct RequiredSymbolSelectorArgs {
    /// Symbol name to locate.
    symbol: String,
    /// Optional parent symbol for disambiguation.
    #[arg(long)]
    symbol_parent: Option<String>,
    /// Optional symbol kind for disambiguation.
    #[arg(long)]
    symbol_kind: Option<String>,
    /// Optional exact symbol signature for disambiguation.
    #[arg(long)]
    symbol_signature: Option<String>,
    /// Optional source line for disambiguation.
    #[arg(long)]
    symbol_line: Option<usize>,
    /// Maximum encoded bytes admitted to the slice response.
    #[arg(long, default_value_t = CodeSliceBudget::DEFAULT_OUTPUT_BYTES)]
    output_bytes: u32,
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
    /// Preserve the terminal background while retaining semantic accents.
    Terminal,
}

impl From<TokenTheme> for TokenDashboardTheme {
    fn from(theme: TokenTheme) -> Self {
        match theme {
            TokenTheme::Dark => Self::Dark,
            TokenTheme::Light => Self::Light,
            TokenTheme::Terminal => Self::Terminal,
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

/// Retrieval family accepted by CLI and MCP search adapters.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Deserialize,
    Eq,
    PartialEq,
    Serialize,
    ValueEnum,
    schemars::JsonSchema,
)]
#[schemars(inline)]
#[serde(rename_all = "lowercase")]
enum SearchRetrievalModeArg {
    /// Correctness-authoritative lexical search.
    #[default]
    Lexical,
    /// Optional semantic retrieval generation.
    Semantic,
    /// Lexical-complete search with optional semantic ranking.
    Hybrid,
}

impl From<SearchRetrievalModeArg> for SearchRetrievalMode {
    fn from(value: SearchRetrievalModeArg) -> Self {
        match value {
            SearchRetrievalModeArg::Lexical => Self::Lexical,
            SearchRetrievalModeArg::Semantic => Self::Semantic,
            SearchRetrievalModeArg::Hybrid => Self::Hybrid,
        }
    }
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

/// Explicit durable root transition selected by CLI or MCP callers.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum, schemars::JsonSchema,
)]
#[schemars(inline)]
#[serde(rename_all = "snake_case")]
enum RootTransition {
    /// Initialize a missing binding or verify an identical existing binding.
    Bind,
    /// Preserve identity only after the previously recorded root is proven absent.
    Move,
    /// Rotate identity for an independent copy, clone, or worktree.
    Detach,
}

impl From<RootTransition> for ProjectRootTransition {
    fn from(value: RootTransition) -> Self {
        match value {
            RootTransition::Bind => Self::Bind,
            RootTransition::Move => Self::Move,
            RootTransition::Detach => Self::Detach,
        }
    }
}

impl From<ProjectRootTransition> for RootTransition {
    fn from(value: ProjectRootTransition) -> Self {
        match value {
            ProjectRootTransition::Bind => Self::Bind,
            ProjectRootTransition::Move => Self::Move,
            ProjectRootTransition::Detach => Self::Detach,
        }
    }
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
    /// Whether the database path came from the command line rather than the default.
    #[arg(skip)]
    database_path_is_explicit: bool,
    /// Response format to emit.
    #[arg(long, value_enum, default_value_t = OutputFormat::Toon)]
    format: OutputFormat,
    /// Caller-visible compatibility label recorded with token telemetry.
    #[arg(long, default_value = DEFAULT_CALLER_LABEL)]
    session: String,
    /// Path to `ProjectAtlas` config.toml for map/lint/init workflows.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Require this exact runtime version before executing the selected command.
    #[arg(long)]
    require_version: Option<String>,
    /// Subcommand to execute.
    #[command(subcommand)]
    command: Box<Command>,
}

impl Cli {
    /// Resolve this invocation's selected source root before opening an implicit database.
    fn project_root(&self) -> Result<PathBuf, CliError> {
        default_cli_project_root(
            &self.db,
            self.config.as_deref(),
            self.database_path_is_explicit,
        )
    }

    /// Resolve an optional command root using this invocation's database selection.
    fn project_root_for_path(&self, path: &Path) -> Result<PathBuf, CliError> {
        defaultable_cli_project_root(
            path,
            &self.db,
            self.config.as_deref(),
            self.database_path_is_explicit,
        )
    }

    /// Validate only the implicit conventional root without changing explicit authority.
    fn preflight_implicit_project_root(&self) -> Result<(), CliError> {
        if !self.database_path_is_explicit && self.config.is_none() {
            drop(self.project_root()?);
        }
        Ok(())
    }
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
        /// Retrieval family; lexical remains the default and always-available mode.
        #[arg(long, value_enum, default_value_t)]
        retrieval_mode: SearchRetrievalModeArg,
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
        /// Optional exact declaration selector.
        #[command(flatten)]
        selector: OptionalSymbolSelectorArgs,
    },
    /// Inspect and rebuild the `ProjectAtlas` symbol graph.
    Symbols {
        /// Symbol graph subcommand to run.
        #[command(subcommand)]
        command: Box<SymbolsCommand>,
    },
    /// Print local `ProjectAtlas` settings and cache/index locations.
    Settings,
    /// Export or import a portable derived-only graph snapshot.
    #[cfg(feature = "derived-snapshot")]
    Snapshot {
        /// Snapshot operation.
        #[arg(value_enum)]
        action: SnapshotAction,
        /// Destination archive for export or source archive for import.
        path: PathBuf,
        /// Require this exact lowercase BLAKE3 digest during import.
        #[arg(long)]
        require_digest: Option<String>,
        /// Optional raw 32-byte Ed25519 secret key encoded as 64 hexadecimal characters.
        #[cfg(feature = "derived-snapshot-signatures")]
        #[arg(long)]
        signing_key: Option<PathBuf>,
        /// Require an import signature from this raw 32-byte Ed25519 public key.
        #[cfg(feature = "derived-snapshot-signatures")]
        #[arg(long)]
        trusted_public_key: Option<PathBuf>,
    },
    /// Manage the separately shipped optional parser pack.
    #[cfg(feature = "optional-parser-supervisor")]
    ParserPack {
        /// Override the user-owned pack storage root for isolated verification and tests.
        #[arg(long, hide = true)]
        storage_root: Option<PathBuf>,
        /// Explicit lifecycle operation.
        #[command(subcommand)]
        command: ParserPackCommand,
    },
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
        /// Opt in to bounded current coverage discovery instead of structural findings.
        #[arg(long)]
        coverage: bool,
        /// Optional source parser coverage filter.
        #[arg(long, requires = "coverage")]
        parser: Option<String>,
        /// Optional derived-fact provider coverage filter.
        #[arg(long, requires = "coverage")]
        provider: Option<String>,
        /// Optional relationship-family coverage filter.
        #[arg(long, requires = "coverage")]
        relation: Option<String>,
        /// Optional complete, partial, failed, ignored, oversized, quarantined, or stale filter.
        #[arg(long, requires = "coverage")]
        coverage_state: Option<String>,
        /// Optional exact coverage reason filter.
        #[arg(long, requires = "coverage")]
        reason: Option<String>,
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
        /// Optional caller-visible compatibility-label filter.
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
        /// Optional repository-relative agent-navigation benchmark result.
        #[arg(long, value_name = "PATH")]
        benchmark_results: Option<PathBuf>,
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

/// Portable derived graph snapshot operation.
#[cfg(feature = "derived-snapshot")]
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
enum SnapshotAction {
    /// Export a fresh bounded tar.zst artifact without overwriting an existing file.
    Export,
    /// Validate and atomically import one portable derived graph archive.
    Import,
}

/// Explicit optional parser-pack lifecycle commands.
#[cfg(feature = "optional-parser-supervisor")]
#[derive(Debug, Subcommand)]
enum ParserPackCommand {
    /// Validate a local completed archive without installing it.
    Verify {
        /// Completed platform archive to validate.
        #[arg(long)]
        archive: PathBuf,
    },
    /// Install a local completed archive without enabling it.
    Install {
        /// Completed platform archive to install.
        #[arg(long)]
        archive: PathBuf,
    },
    /// Enable one explicitly named installed artifact for this project.
    ///
    /// Selecting the artifact reported by `status.rollback` performs an explicit rollback.
    Enable {
        /// BLAKE3 identity of the installed artifact manifest.
        #[arg(long)]
        artifact: String,
    },
    /// Install and atomically select a replacement while retaining rollback identity.
    Update {
        /// Completed replacement platform archive.
        #[arg(long)]
        archive: PathBuf,
    },
    /// Disable the optional pack for this project without deleting installed slots.
    Disable,
    /// Disable this project and remove this logical pack's user-owned slots.
    Remove,
    /// Print bounded content-free lifecycle state.
    Status,
}

/// Project root diagnostics and binding subcommands.
#[derive(Debug, Subcommand)]
enum RootCommand {
    /// Bind a repository root and regenerate project-local MCP configs.
    Set {
        /// Repository root to bind.
        path: PathBuf,
        /// Explicit binding behavior for an existing database.
        #[arg(long, value_enum, default_value_t = RootTransition::Bind)]
        transition: RootTransition,
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
        /// Host-owned task label for deterministic curator work identity.
        #[arg(long)]
        task: Option<String>,
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
        /// Preserve legacy rows or opt in to detailed normalized-graph navigation.
        #[arg(long, value_enum, default_value_t = RelationViewArg::Legacy)]
        view: RelationViewArg,
        /// Optional repository-relative file path.
        #[arg(long)]
        file: Option<String>,
        /// Optional source, target, or context query.
        #[arg(long)]
        query: Option<String>,
        /// Additive normalized-graph traversal controls.
        #[command(flatten)]
        detailed: Box<DetailedRelationArgs>,
        /// Maximum relations to return.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Return an exact source slice for a named symbol.
    Slice {
        /// Repository-relative file path.
        file: PathBuf,
        /// Exact declaration selector.
        #[command(flatten)]
        selector: RequiredSymbolSelectorArgs,
    },
}

/// Parse arguments, execute the command, and convert failures to process exit.
fn main() {
    let cli = parse_cli();
    if let Err(error) = run(&cli) {
        let rendered =
            render_cli_error(cli.format, &error).unwrap_or_else(|_| format!("error: {error}\n"));
        if write_stderr(&rendered).is_err() {
            std::process::exit(1);
        }
        std::process::exit(1);
    }
}

/// Parse CLI arguments while retaining whether `--db` was explicitly selected.
fn parse_cli() -> Cli {
    let matches = Cli::command().get_matches();
    let database_path_is_explicit = matches.value_source("db") == Some(ValueSource::CommandLine);
    let mut cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    cli.database_path_is_explicit = database_path_is_explicit;
    cli
}

/// Load map and lint config with an explicit CLI database override when present.
fn load_cli_atlas_config(cli: &Cli) -> Result<AtlasMapConfig, CliError> {
    let config = load_atlas_config(cli.config.as_deref())?;
    if cli.database_path_is_explicit {
        return Ok(config.with_database_path(&cli.db));
    }
    Ok(config)
}

/// Execute the selected CLI command.
fn run(cli: &Cli) -> Result<(), CliError> {
    if let Some(required_version) = cli.require_version.as_deref() {
        validate_required_runtime_version(required_version)?;
    }
    let usage_instance = UsageRuntimeInstance::new(UsageInstanceOwner::CliInvocation);
    match cli.command.as_ref() {
        Command::Init {
            no_scan,
            force_rescan,
            text_index_max_bytes,
        } => {
            let current_dir = std::env::current_dir().map_err(|source| CliError::Io {
                path: PathBuf::from("."),
                source,
            })?;
            let root = canonical_project_root(&current_dir)?;
            let db_path = if cli.db.is_absolute() {
                cli.db.clone()
            } else {
                root.join(&cli.db)
            };
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
            cli.preflight_implicit_project_root()?;
            let config = load_cli_atlas_config(cli)?;
            write_map(&config, *json)?;
        }
        Command::Scan {
            path,
            text_index_max_bytes,
        } => {
            let path = cli.project_root_for_path(path)?;
            let symbol_options = SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None);
            let control = index_work_control(&symbol_options);
            let plan = ScanRuntimePlan::for_path_controlled(
                cli.config.as_deref(),
                &path,
                *text_index_max_bytes,
                &control,
            )?;
            let mut store = open_atlas_store_for_project(&cli.db, &plan.root)?;
            let report =
                run_scan_pipeline_controlled(&mut store, &plan, &symbol_options, &control)?;
            print_output(
                cli.format,
                &encode_agent_payload(&json!({ "scan": report })),
                &report,
            )?;
        }
        Command::Overview => {
            let store = open_index_for_read(cli)?;
            let overview = store.overview()?;
            let toon = render_overview(&overview);
            print_tracked_directory_output_estimate(
                cli.format,
                &store,
                usage_instance,
                &cli.session,
                "overview",
                None,
                None,
                || estimated_source_tokens_for_indexed_files(&store, None, None),
                &toon,
                &overview,
            )?;
        }
        Command::Folders { query, limit } => {
            let store = open_index_for_read(cli)?;
            let selected = ranked_folder_nodes_with_reasons(&store, query, *limit)?;
            let toon = render_ranked_nodes("folders", &selected);
            let payload = render_ranked_node_rows("folders", &selected);
            print_tracked_directory_output_estimate(
                cli.format,
                &store,
                usage_instance,
                &cli.session,
                "folders",
                None,
                Some(query.clone()),
                || estimated_source_tokens_for_indexed_files(&store, None, None),
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
            let store = open_index_for_read(cli)?;
            let query_text = query.as_deref().unwrap_or("");
            let folder_filter = folder
                .as_deref()
                .map(normalized_folder_filter)
                .transpose()?;
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
                usage_instance,
                &cli.session,
                "files",
                file_pattern.clone().or_else(|| folder_filter.clone()),
                query.clone(),
                || {
                    estimated_source_tokens_for_indexed_files(
                        &store,
                        folder_filter.as_deref(),
                        file_pattern.as_deref(),
                    )
                },
                &toon,
                &payload,
            )?;
        }
        Command::Next { query, limit } => {
            let store = open_index_for_read(cli)?;
            let report = next_step_report(&store, query, Some(*limit))?;
            let payload = next_step_report_payload(&report);
            let toon = encode_agent_payload(&json!({ "next": payload }));
            print_tracked_directory_output_estimate(
                cli.format,
                &store,
                usage_instance,
                &cli.session,
                "next",
                None,
                Some(query.clone()),
                || estimated_source_tokens_for_indexed_files(&store, None, None),
                &toon,
                &payload,
            )?;
        }
        Command::Outline { file, lines } => {
            let store = open_index_for_read(cli)?;
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
                usage_instance,
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
            let store = open_index_for_read(cli)?;
            let file_key = validated_indexed_file_key(&store, file)?;
            let content = read_indexed_file_content(&store, &file_key)?;
            let report =
                build_file_summary_from_source(&store, Path::new(&file_key), *limit, &content)?;
            let toon = render_file_summary(&report);
            print_tracked_output_text(
                cli.format,
                &store,
                usage_instance,
                &cli.session,
                "summary",
                Some(report.file_path.clone()),
                None,
                &content,
                &toon,
                &report,
            )?;
        }
        Command::Search {
            pattern,
            retrieval_mode,
            regex,
            fuzzy,
            case_sensitive,
            file_pattern,
            context_lines,
            start_index,
            limit,
        } => {
            let store = open_index_for_read(cli)?;
            let report = search_indexed_files_with_control(
                &store,
                &SearchQuery {
                    pattern,
                    regex: *regex,
                    fuzzy: *fuzzy,
                    case_sensitive: *case_sensitive,
                    file_pattern: file_pattern.as_deref(),
                    context_lines: *context_lines,
                    start_index: *start_index,
                    limit: *limit,
                    retrieval_mode: (*retrieval_mode).into(),
                },
                None,
            )?;
            let toon = render_search_report(&report);
            print_tracked_output_estimate(
                cli.format,
                &store,
                usage_instance,
                &cli.session,
                "search",
                file_pattern.clone(),
                Some(pattern.clone()),
                || Ok(byte_count_to_tokens(report.searched_bytes)),
                &toon,
                &report,
            )?;
        }
        Command::Slice {
            file,
            start_line,
            end_line,
            selector:
                OptionalSymbolSelectorArgs {
                    symbol,
                    symbol_parent,
                    symbol_kind,
                    symbol_signature,
                    symbol_line,
                    output_bytes,
                },
        } => {
            let store = open_index_for_read(cli)?;
            let file_key = validated_indexed_file_key(&store, file)?;
            let content = read_indexed_file_content(&store, &file_key)?;
            let output_budget = CodeSliceBudget::new(*output_bytes)?;
            let report = if let Some(symbol) = symbol {
                read_symbol_slice_from_source_bounded(
                    &store,
                    Path::new(&file_key),
                    &SymbolSliceSelector {
                        name: symbol,
                        parent: symbol_parent.as_deref(),
                        kind: symbol_kind.as_deref(),
                        signature: symbol_signature.as_deref(),
                        line: *symbol_line,
                    },
                    &content,
                    output_budget,
                )?
            } else {
                if symbol_parent.is_some()
                    || symbol_kind.is_some()
                    || symbol_signature.is_some()
                    || symbol_line.is_some()
                {
                    return Err(CliError::InvalidInput(
                        "symbol disambiguators require --symbol".to_string(),
                    ));
                }
                let start_line = start_line.ok_or_else(|| {
                    CliError::InvalidInput(
                        "start-line is required unless --symbol is provided".to_string(),
                    )
                })?;
                read_indexed_code_slice_from_source_bounded(
                    &store,
                    Path::new(&file_key),
                    start_line,
                    *end_line,
                    &content,
                    output_budget,
                )?
            };
            print_tracked_slice_output(
                cli.format,
                &store,
                usage_instance,
                &cli.session,
                "slice",
                Some(report.slice().path.clone()),
                None,
                &content,
                &report,
            )?;
        }
        Command::Symbols { command } => match command.as_ref() {
            SymbolsCommand::Build {
                path,
                max_bytes,
                max_workers,
                timeout_seconds,
            } => {
                let path = cli.project_root_for_path(path)?;
                let options = SymbolBuildOptions::new(*max_bytes, *max_workers, *timeout_seconds);
                let control = index_work_control(&options);
                let plan = ScanRuntimePlan::for_path_controlled(
                    cli.config.as_deref(),
                    &path,
                    None,
                    &control,
                )?;
                let mut store = open_atlas_store_for_project(&cli.db, &plan.root)?;
                let report = run_symbol_build_pipeline_controlled(
                    &mut store, &plan, &options, None, &control,
                )?;
                print_output(
                    cli.format,
                    &encode_agent_payload(&json!({ "symbols_build": report })),
                    &report,
                )?;
            }
            SymbolsCommand::List { file, query, limit } => {
                let store = open_index_for_read(cli)?;
                let symbols = store.load_symbols(file.as_deref(), query.as_deref(), *limit)?;
                let toon = render_symbols(&symbols);
                print_tracked_output_estimate(
                    cli.format,
                    &store,
                    usage_instance,
                    &cli.session,
                    "symbols",
                    file.clone(),
                    query.clone(),
                    || {
                        estimated_source_tokens_for_paths(
                            &store,
                            symbols.iter().map(|symbol| symbol.path.as_str()),
                        )
                    },
                    &toon,
                    &symbols,
                )?;
            }
            SymbolsCommand::Relations {
                view,
                file,
                query,
                detailed,
                limit,
            } => {
                let DetailedRelationArgs {
                    cursor,
                    roots,
                    anchor:
                        DetailedRelationAnchorArgs {
                            symbol,
                            symbol_parent,
                            symbol_kind,
                            symbol_signature,
                        },
                    filters:
                        DetailedRelationFilterArgs {
                            direction,
                            relation,
                            minimum_confidence,
                            resolution,
                        },
                    limits:
                        DetailedRelationLimitArgs {
                            depth,
                            include_occurrences,
                            occurrence_limit,
                            edge_limit,
                            node_limit,
                            visited_limit,
                            occurrence_total_limit,
                            intermediate_bytes,
                            deadline_ms,
                            output_bytes,
                        },
                    analysis,
                } = detailed.as_ref();
                let RelationAnalysisArgs {
                    analysis_mode,
                    trace_target,
                    vcs,
                    include_communities,
                    include_cycles,
                    include_dead_code,
                } = analysis.as_ref();
                let analysis_controls_explicit = analysis_mode.is_some()
                    || trace_target.is_some()
                    || vcs.is_some()
                    || *include_communities
                    || *include_cycles
                    || *include_dead_code;
                if *view != RelationViewArg::Analysis && analysis_controls_explicit {
                    return Err(CliError::Service(ServiceError::InvalidInput(
                        "analysis controls require --view analysis".to_string(),
                    )));
                }
                if *view == RelationViewArg::Legacy && !roots.is_empty() {
                    return Err(CliError::Service(ServiceError::InvalidInput(
                        "--root requires --view detailed or --view analysis".to_string(),
                    )));
                }
                let federation_control = (!roots.is_empty()).then(|| {
                    standalone_index_work_control()
                        .with_timeout_ceiling(Duration::from_millis(10_000))
                });
                let mut federated_stores = if let Some(control) = federation_control.as_ref() {
                    let selected_root = cli.project_root()?;
                    Some(open_federated_atlas_stores_for_project(
                        &cli.db,
                        &selected_root,
                        cli.config.as_deref(),
                        roots,
                        control,
                    )?)
                } else {
                    None
                };
                let single_store = if federated_stores.is_none() {
                    Some(open_index_for_read(cli)?)
                } else {
                    None
                };
                let store = match (&federated_stores, &single_store) {
                    (Some(stores), _) => stores.first().map(FederatedStore::store),
                    (None, store) => store.as_ref(),
                }
                .ok_or_else(|| {
                    CliError::Service(ServiceError::InvalidInput(
                        "relation request opened no project store".to_string(),
                    ))
                })?;
                if *view == RelationViewArg::Legacy {
                    let relations =
                        store.load_symbol_relations(file.as_deref(), query.as_deref(), *limit)?;
                    let toon = render_symbol_relations(&relations);
                    print_tracked_output_estimate(
                        cli.format,
                        store,
                        usage_instance,
                        &cli.session,
                        "symbol-relations",
                        file.clone(),
                        query.clone(),
                        || {
                            estimated_source_tokens_for_paths(
                                store,
                                relations.iter().map(|relation| relation.path.as_str()),
                            )
                        },
                        &toon,
                        &relations,
                    )?;
                } else {
                    if query.is_some() {
                        return Err(CliError::Service(ServiceError::InvalidInput(
                            "detailed symbol relations use exact --symbol selectors, not --query"
                                .to_string(),
                        )));
                    }
                    let file = file.as_deref().ok_or_else(|| {
                        CliError::Service(ServiceError::InvalidInput(
                            "detailed symbol relations require --file".to_string(),
                        ))
                    })?;
                    let file = validated_indexed_file_key(store, Path::new(file))?;
                    let file = RepositoryFilePath::new(Path::new(&file)).map_err(|error| {
                        CliError::Service(ServiceError::InvalidInput(error.to_string()))
                    })?;
                    let anchor = if let Some(symbol) = symbol {
                        if symbol.is_empty() {
                            return Err(CliError::Service(ServiceError::InvalidInput(
                                "detailed relation symbol must not be empty".to_string(),
                            )));
                        }
                        RelationAnchor::Symbol {
                            file,
                            name: symbol.clone(),
                            symbol_kind: symbol_kind
                                .as_deref()
                                .map(parse_symbol_kind)
                                .transpose()?,
                            parent: symbol_parent.clone(),
                            signature: symbol_signature.clone(),
                        }
                    } else {
                        if symbol_parent.is_some()
                            || symbol_kind.is_some()
                            || symbol_signature.is_some()
                        {
                            return Err(CliError::Service(ServiceError::InvalidInput(
                                "symbol disambiguators require --symbol".to_string(),
                            )));
                        }
                        RelationAnchor::File { file }
                    };
                    let rows = u32::try_from(*limit).map_err(|_overflow| {
                        CliError::Service(ServiceError::InvalidInput(
                            "detailed relation limit exceeds the u32 range".to_string(),
                        ))
                    })?;
                    let limits = GraphLimits::new(rows, *occurrence_limit, *depth, *output_bytes)
                        .map_err(|error| {
                        CliError::Service(ServiceError::InvalidInput(error.to_string()))
                    })?;
                    let relations = DetailedRelationQuery {
                        anchor,
                        direction: (*direction).into(),
                        relation: relation
                            .as_deref()
                            .map(parse_coverage_relation)
                            .transpose()?,
                        minimum_confidence: (*minimum_confidence).into(),
                        resolution: (*resolution).into(),
                        include_occurrences: *include_occurrences,
                        budget: DetailedRelationBudget::from_graph_limits(limits)
                            .with_aggregate_limits(
                                *edge_limit,
                                *node_limit,
                                *visited_limit,
                                *occurrence_total_limit,
                                *intermediate_bytes,
                                *deadline_ms,
                            )?,
                        cursor: cursor.clone(),
                    };
                    let output = if *view == RelationViewArg::Detailed {
                        if let Some(stores) = federated_stores.take() {
                            let control = federation_control.as_ref().ok_or_else(|| {
                                CliError::Service(ServiceError::InvalidInput(
                                    "federated relation control is unavailable".to_string(),
                                ))
                            })?;
                            let draft = load_federated_detailed_relations(
                                stores,
                                &relations,
                                Some(control),
                            )?;
                            let (_report, output) = draft.fit_output(Some(control), |report| {
                                let payload = json!({ "symbol_relations": report });
                                let toon = encode_agent_payload(&payload);
                                serialized_output(cli.format, &toon, &payload)
                            })?;
                            output
                        } else {
                            let draft = load_detailed_relation_page(
                                single_store.as_ref().ok_or_else(|| {
                                    CliError::Service(ServiceError::InvalidInput(
                                        "single-project relation store is unavailable".to_string(),
                                    ))
                                })?,
                                &relations,
                                None,
                            )?;
                            let (_report, output) = draft.fit_output(None, |report| {
                                let payload = json!({ "symbol_relations": report });
                                let toon = encode_agent_payload(&payload);
                                serialized_output(cli.format, &toon, &payload)
                            })?;
                            output
                        }
                    } else {
                        let vcs_explicit = vcs.is_some();
                        let vcs = match vcs.as_deref().unwrap_or("working-tree") {
                            "working-tree" => GitImpactSelection::WorkingTree,
                            "index" => GitImpactSelection::Index,
                            range => {
                                let (base, head) = range.split_once("..").ok_or_else(|| {
                                    CliError::Service(ServiceError::InvalidInput(
                                        "--vcs must be working-tree, index, or an exact base..head range"
                                            .to_string(),
                                    ))
                                })?;
                                GitImpactSelection::RevisionRange {
                                    base: base.to_string(),
                                    head: head.to_string(),
                                }
                            }
                        };
                        let mode: RelationAnalysisMode = analysis_mode
                            .unwrap_or(RelationAnalysisModeArg::Architecture)
                            .into();
                        let trace_target = trace_target
                            .as_deref()
                            .map(serde_json::from_str::<RelationAnchor>)
                            .transpose()
                            .map_err(|error| {
                                CliError::Service(ServiceError::InvalidInput(format!(
                                    "--trace-target must be an exact RelationAnchor JSON object: {error}"
                                )))
                            })?;
                        let query = RelationAnalysisQuery {
                            relations,
                            mode,
                            trace_target,
                            vcs: (mode == RelationAnalysisMode::Impact || vcs_explicit)
                                .then_some(vcs),
                            include_communities: *include_communities,
                            include_cycles: *include_cycles,
                            include_dead_code: *include_dead_code,
                        };
                        if let Some(stores) = federated_stores.take() {
                            let control = federation_control.as_ref().ok_or_else(|| {
                                CliError::Service(ServiceError::InvalidInput(
                                    "federated analysis control is unavailable".to_string(),
                                ))
                            })?;
                            let draft =
                                load_federated_relation_analysis(stores, &query, Some(control))?;
                            let (_report, output) = draft.fit_output(|report, control| {
                                controlled_named_output(
                                    cli.format,
                                    CLI_PAYLOAD_SYMBOL_RELATIONS,
                                    report,
                                    control,
                                )
                            })?;
                            output
                        } else {
                            let draft = load_relation_analysis(
                                single_store.as_ref().ok_or_else(|| {
                                    CliError::Service(ServiceError::InvalidInput(
                                        "single-project analysis store is unavailable".to_string(),
                                    ))
                                })?,
                                &query,
                                None,
                            )?;
                            let (_report, output) = draft.fit_output(|report, control| {
                                controlled_named_output(
                                    cli.format,
                                    CLI_PAYLOAD_SYMBOL_RELATIONS,
                                    report,
                                    control,
                                )
                            })?;
                            output
                        }
                    };
                    write_stdout(&output)?;
                }
            }
            SymbolsCommand::Slice {
                file,
                selector:
                    RequiredSymbolSelectorArgs {
                        symbol,
                        symbol_parent,
                        symbol_kind,
                        symbol_signature,
                        symbol_line,
                        output_bytes,
                    },
            } => {
                let store = open_index_for_read(cli)?;
                let file_key = validated_indexed_file_key(&store, file)?;
                let content = read_indexed_file_content(&store, &file_key)?;
                let report = read_symbol_slice_from_source_bounded(
                    &store,
                    Path::new(&file_key),
                    &SymbolSliceSelector {
                        name: symbol,
                        parent: symbol_parent.as_deref(),
                        kind: symbol_kind.as_deref(),
                        signature: symbol_signature.as_deref(),
                        line: *symbol_line,
                    },
                    &content,
                    CodeSliceBudget::new(*output_bytes)?,
                )?;
                print_tracked_slice_output(
                    cli.format,
                    &store,
                    usage_instance,
                    &cli.session,
                    "symbol-slice",
                    Some(report.slice().path.clone()),
                    Some(symbol.clone()),
                    &content,
                    &report,
                )?;
            }
        },
        Command::Settings => {
            cli.preflight_implicit_project_root()?;
            let report = build_settings_report(&cli.db, cli.config.as_deref(), cli.format)?;
            let toon = render_settings_report(&report);
            print_output(cli.format, &toon, &report)?;
        }
        #[cfg(feature = "derived-snapshot")]
        Command::Snapshot {
            action,
            path,
            require_digest,
            #[cfg(feature = "derived-snapshot-signatures")]
            signing_key,
            #[cfg(feature = "derived-snapshot-signatures")]
            trusted_public_key,
        } => match action {
            SnapshotAction::Export => {
                if require_digest.is_some() {
                    return Err(CliError::InvalidInput(
                        "--require-digest applies only to snapshot import".to_string(),
                    ));
                }
                #[cfg(feature = "derived-snapshot-signatures")]
                if trusted_public_key.is_some() {
                    return Err(CliError::InvalidInput(
                        "--trusted-public-key applies only to snapshot import".to_string(),
                    ));
                }
                let store = open_index_for_read(cli)?;
                let report = derived_snapshot_archive::export_snapshot_archive(
                    &store,
                    path,
                    #[cfg(feature = "derived-snapshot-signatures")]
                    signing_key.as_deref(),
                )?;
                store.finish_index_read_snapshot()?;
                print_output(
                    cli.format,
                    &encode_agent_payload(&json!({ "snapshot_export": report })),
                    &report,
                )?;
            }
            SnapshotAction::Import => {
                #[cfg(feature = "derived-snapshot-signatures")]
                if signing_key.is_some() {
                    return Err(CliError::InvalidInput(
                        "--signing-key applies only to snapshot export".to_string(),
                    ));
                }
                let fresh = open_index_for_read(cli)?;
                fresh.finish_index_read_snapshot()?;
                drop(fresh);
                let mut store = open_index_for_mutation(cli)?;
                let report = derived_snapshot_archive::import_snapshot_archive(
                    &mut store,
                    path,
                    require_digest.as_deref(),
                    #[cfg(feature = "derived-snapshot-signatures")]
                    trusted_public_key.as_deref(),
                )?;
                print_output(
                    cli.format,
                    &encode_agent_payload(&json!({ "snapshot_import": report })),
                    &report,
                )?;
            }
        },
        #[cfg(feature = "optional-parser-supervisor")]
        Command::ParserPack {
            storage_root,
            command,
        } => run_parser_pack_command(cli.format, storage_root.as_ref(), command)?,
        Command::Root { command } => match command {
            Some(RootCommand::Set {
                path,
                transition,
                nearest_project,
            }) => {
                let root = canonical_project_root(path)?;
                let report = bind_project_root(&root, *transition, *nearest_project)?;
                print_output(cli.format, &render_root_report(&report), &report)?;
            }
            None | Some(RootCommand::Show) => {
                cli.preflight_implicit_project_root()?;
                let report = build_root_report(&cli.db, cli.config.as_deref())?;
                print_output(cli.format, &render_root_report(&report), &report)?;
            }
            Some(RootCommand::Verify) => {
                cli.preflight_implicit_project_root()?;
                let report = build_root_report(&cli.db, cli.config.as_deref())?;
                let verified = report.verified;
                if verified {
                    verify_project_database(&cli.db, Path::new(&report.root))?;
                }
                print_output(cli.format, &render_root_report(&report), &report)?;
                if !verified {
                    std::process::exit(1);
                }
            }
        },
        Command::Config { print: _ } => {
            cli.preflight_implicit_project_root()?;
            let config = load_cli_atlas_config(cli)?;
            let report = effective_config_report(&config);
            print_output(
                cli.format,
                &encode_agent_payload(&json!({ "config": report })),
                &report,
            )?;
        }
        Command::Ignore { command } => match command {
            IgnoreCommand::List => {
                let project_root = cli.project_root()?;
                let report = list_ignore_entries(cli.config.as_deref(), &project_root)?;
                print_output(
                    cli.format,
                    &encode_agent_payload(&json!({ "ignore": report })),
                    &report,
                )?;
            }
            IgnoreCommand::InitGitignore => {
                let project_root = cli.project_root()?;
                let report = init_gitignore(cli.config.as_deref(), &project_root)?;
                print_output(
                    cli.format,
                    &encode_agent_payload(&json!({ "gitignore": report })),
                    &report,
                )?;
            }
            IgnoreCommand::Add { kind, value } => {
                let project_root = cli.project_root()?;
                let report =
                    add_ignore_entry(cli.config.as_deref(), &project_root, (*kind).into(), value)?;
                print_output(
                    cli.format,
                    &encode_agent_payload(&json!({ "ignore": report })),
                    &report,
                )?;
            }
            IgnoreCommand::Remove { kind, value } => {
                let project_root = cli.project_root()?;
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
            let path = cli.project_root_for_path(path)?;
            let symbol_options =
                SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, *max_workers, *timeout_seconds);
            let report = if *once {
                let control = index_work_control(&symbol_options);
                let plan = ScanRuntimePlan::for_path_controlled(
                    cli.config.as_deref(),
                    &path,
                    *text_index_max_bytes,
                    &control,
                )?;
                let mut store = open_atlas_store_for_project(&cli.db, &plan.root)?;
                run_single_watch_refresh_controlled(&mut store, &plan, &symbol_options, &control)?
            } else {
                let plan =
                    ScanRuntimePlan::for_path(cli.config.as_deref(), &path, *text_index_max_bytes)?;
                let mut store = open_atlas_store_for_project(&cli.db, &plan.root)?;
                run_watch_loop(
                    &mut store,
                    &plan,
                    false,
                    *poll_seconds,
                    *max_cycles,
                    &symbol_options,
                )?
            };
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
            coverage,
            parser,
            provider,
            relation,
            coverage_state,
            reason,
        } => {
            let store = open_index_for_read(cli)?;
            if *coverage {
                let query = coverage_query_from_cli(
                    *start_index,
                    *limit,
                    &CoverageCliFilters {
                        path_prefix: path_prefix.as_deref(),
                        parser: parser.as_deref(),
                        provider: provider.as_deref(),
                        relation: relation.as_deref(),
                        state: coverage_state.as_deref(),
                        reason: reason.as_deref(),
                    },
                )?;
                let mut report = load_coverage_discovery(&store, query)?;
                let toon = finalize_coverage_output(cli.format, &mut report)?;
                print_tracked_directory_output_estimate(
                    cli.format,
                    &store,
                    usage_instance,
                    &cli.session,
                    "health-check",
                    None,
                    None,
                    || estimated_source_tokens_for_indexed_files(&store, None, None),
                    &toon,
                    &report,
                )?;
                return Ok(());
            }
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
            let page = store.unresolved_health_findings_page_current(&query)?;
            let toon = render_health_page(&page, &query);
            print_tracked_directory_output_estimate(
                cli.format,
                &store,
                usage_instance,
                &cli.session,
                "health-check",
                None,
                None,
                || estimated_source_tokens_for_indexed_files(&store, None, None),
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
                let store = open_index_for_mutation(cli)?;
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
            cli.preflight_implicit_project_root()?;
            let config = load_cli_atlas_config(cli)?;
            let (mut report, mut exit_code) = lint_map(
                &config,
                LintOptions {
                    strict_folders: *strict_folders,
                    report_untracked: *report_untracked,
                    strict_untracked: *strict_untracked,
                },
            )?;
            let (db_report, db_exit_code) = lint_database_if_present(
                &cli.db,
                &config.root,
                cli.config.as_deref(),
                (*purpose_level).into(),
            )?;
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
            benchmark_results,
            theme,
        } => {
            let store = open_index_for_current_read(cli)?;
            if let Some(window) = trend {
                if tokenizer.is_some() {
                    return Err(CliError::InvalidInput(
                        "--tokenizer is only supported for token overview reports".to_string(),
                    ));
                }
                if benchmark_results.is_some() {
                    return Err(CliError::InvalidInput(
                        "--benchmark-results is only supported for token overview reports"
                            .to_string(),
                    ));
                }
                let report = match load_token_report(
                    &store,
                    TokenReportRequest::Trends {
                        caller_label: session.as_deref(),
                        window: (*window).into(),
                    },
                )? {
                    TokenReport::Trends(report) => report,
                    TokenReport::Overview(_) => {
                        return Err(CliError::InvalidInput(
                            "token trend request returned an overview".to_string(),
                        ));
                    }
                };
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
                let mut overview = match load_token_report(
                    &store,
                    TokenReportRequest::Overview {
                        caller_label: session.as_deref(),
                        benchmark_results: benchmark_results.as_deref(),
                    },
                )? {
                    TokenReport::Overview(overview) => overview,
                    TokenReport::Trends(_) => {
                        return Err(CliError::InvalidInput(
                            "token overview request returned trends".to_string(),
                        ));
                    }
                };
                if let Some(tokenizer) = tokenizer.as_deref() {
                    overview.set_calibration(build_token_calibration(&store, tokenizer)?);
                }
                match view {
                    TokenView::Agent => {
                        print_output(cli.format, &render_token_overview(&overview), &overview)?;
                    }
                    TokenView::Tui => {
                        let atlas = if token_dashboard_wants_atlas() {
                            load_token_atlas_preview(&store)
                        } else {
                            TokenAtlasPreview::empty()
                        };
                        write_stdout(&render_token_dashboard_with_atlas(
                            &overview,
                            session.as_deref(),
                            &atlas,
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
            let store = open_index_for_read(cli)?;
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
            let path = cli.project_root_for_path(path)?;
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
            cli.preflight_implicit_project_root()?;
            let report = reset_index_files(&cli.db, *apply, *dry_run, *include_mcp_config)?;
            print_output(
                cli.format,
                &encode_agent_payload(&json!({ "reset_index": report })),
                &report,
            )?;
        }
        Command::Mcp { nearest_project } => {
            cli.preflight_implicit_project_root()?;
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
            cli.preflight_implicit_project_root()?;
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
                let store = open_index_for_mutation(cli)?;
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
                let requests = load_purpose_review_requests(from_file)?;
                validate_purpose_review_admission(&requests)?;
                let store = if *apply {
                    open_index_for_mutation(cli)?
                } else {
                    open_index_for_read(cli)?
                };
                let report = review_purposes(&store, &requests, *apply)?;
                print_output(cli.format, &render_purpose_review_report(&report), &report)?;
                if report.failed > 0 {
                    std::process::exit(1);
                }
            }
            PurposeCommand::Queue {
                task,
                start_index,
                limit,
                category,
                severity,
                path_prefix,
                summary_only,
                include_assets,
                include_low_priority_files,
            } => {
                let store = open_index_for_read(cli)?;
                let query = health_query_from_cli(
                    *start_index,
                    *limit,
                    category.as_deref(),
                    *severity,
                    path_prefix.as_deref(),
                    *summary_only,
                    purpose_queue_scope(*include_assets, *include_low_priority_files),
                );
                let page = purpose_curation_page(
                    &store,
                    &query,
                    task.as_deref().unwrap_or("purpose-curation"),
                )?;
                let toon = render_purpose_curation_page(&page);
                store.finish_index_read_snapshot()?;
                print_output(cli.format, &toon, &page)?;
            }
        },
    }
    Ok(())
}

/// Execute one explicit optional parser-pack lifecycle command from the selected project root.
#[cfg(feature = "optional-parser-supervisor")]
fn run_parser_pack_command(
    format: OutputFormat,
    storage_root: Option<&PathBuf>,
    command: &ParserPackCommand,
) -> Result<(), CliError> {
    let project_root = std::env::current_dir().map_err(|source| CliError::Io {
        path: PathBuf::from("."),
        source,
    })?;
    let lifecycle = OptionalParserPackLifecycle::new(&project_root, storage_root.cloned())?;
    let report = match command {
        ParserPackCommand::Verify { archive } => lifecycle.verify(archive)?,
        ParserPackCommand::Install { archive } => lifecycle.install(archive)?,
        ParserPackCommand::Enable { artifact } => lifecycle.enable(artifact)?,
        ParserPackCommand::Update { archive } => lifecycle.update(archive)?,
        ParserPackCommand::Disable => lifecycle.disable()?,
        ParserPackCommand::Remove => lifecycle.remove()?,
        ParserPackCommand::Status => lifecycle.status()?,
    };
    let toon = encode_agent_payload(&json!({ "parser_pack": report }));
    print_output(format, &toon, &report)
}

/// Render typed source-state failures in the selected agent/script format.
fn render_cli_error(format: OutputFormat, error: &CliError) -> Result<String, serde_json::Error> {
    if let Some(schema_version_mismatch) = schema_version_mismatch_payload(error) {
        let response = SchemaVersionMismatchErrorResponse {
            error: SchemaVersionMismatchErrorPayload {
                kind: AgentErrorKind::SchemaVersionMismatch,
                message: error.to_string(),
                schema_version_mismatch,
            },
        };
        return match format {
            OutputFormat::Toon => {
                serde_json::to_value(response).map(|value| encode_agent_payload(&value))
            }
            OutputFormat::Json => {
                serde_json::to_string_pretty(&response).map(|text| format!("{text}\n"))
            }
        };
    }
    if let Some(schema_migration_required) = schema_migration_required_payload(error) {
        let message = schema_migration_required.message();
        let response = SchemaMigrationRequiredErrorResponse {
            error: SchemaMigrationRequiredErrorPayload {
                kind: AgentErrorKind::SchemaMigrationRequired,
                message,
                schema_migration_required,
            },
        };
        return match format {
            OutputFormat::Toon => {
                serde_json::to_value(response).map(|value| encode_agent_payload(&value))
            }
            OutputFormat::Json => {
                serde_json::to_string_pretty(&response).map(|text| format!("{text}\n"))
            }
        };
    }
    let details = match error {
        #[cfg(feature = "optional-parser-supervisor")]
        CliError::ParserPack(source) if source.is_unsupported_containment() => {
            Some(CliErrorPayload {
                kind: AgentErrorKind::UnsupportedContainment,
                message: error.to_string(),
                refresh_required: None,
                init_required: None,
                worktree_required: None,
                verification_incomplete: None,
                project_mismatch: None,
                database_filesystem: None,
                search_capability: None,
                next: None,
            })
        }
        CliError::InitRequired(report) => Some(CliErrorPayload {
            kind: AgentErrorKind::InitRequired,
            message: error.to_string(),
            refresh_required: None,
            init_required: Some(report.as_ref()),
            worktree_required: None,
            verification_incomplete: None,
            project_mismatch: None,
            database_filesystem: None,
            search_capability: None,
            next: Some(CliNextCall {
                command: CLI_INIT_COMMAND,
                project_path: &report.project_root,
                once: None,
            }),
        }),
        CliError::WorktreeRequired(report) => Some(CliErrorPayload {
            kind: AgentErrorKind::WorktreeRequired,
            message: error.to_string(),
            refresh_required: None,
            init_required: None,
            worktree_required: Some(report.as_ref()),
            verification_incomplete: None,
            project_mismatch: None,
            database_filesystem: None,
            search_capability: None,
            next: None,
        }),
        CliError::RefreshRequired(report) => Some(CliErrorPayload {
            kind: AgentErrorKind::RefreshRequired,
            message: error.to_string(),
            refresh_required: Some(report.as_ref()),
            init_required: None,
            worktree_required: None,
            verification_incomplete: None,
            project_mismatch: None,
            database_filesystem: None,
            search_capability: None,
            next: Some(CliNextCall {
                command: CLI_REFRESH_COMMAND,
                project_path: &report.project_root,
                once: Some(true),
            }),
        }),
        CliError::VerificationIncomplete(report) => Some(CliErrorPayload {
            kind: AgentErrorKind::VerificationIncomplete,
            message: error.to_string(),
            refresh_required: None,
            init_required: None,
            worktree_required: None,
            verification_incomplete: Some(report.as_ref()),
            project_mismatch: None,
            database_filesystem: None,
            search_capability: None,
            next: None,
        }),
        CliError::ProjectMismatch(report) => Some(CliErrorPayload {
            kind: AgentErrorKind::ProjectMismatch,
            message: error.to_string(),
            refresh_required: None,
            init_required: None,
            worktree_required: None,
            verification_incomplete: None,
            project_mismatch: Some(report.as_ref()),
            database_filesystem: None,
            search_capability: None,
            next: None,
        }),
        CliError::Service(ServiceError::SearchCapabilityUnavailable {
            requested_mode,
            state,
            guidance,
        }) => Some(CliErrorPayload {
            kind: AgentErrorKind::SearchCapabilityUnavailable,
            message: error.to_string(),
            refresh_required: None,
            init_required: None,
            worktree_required: None,
            verification_incomplete: None,
            project_mismatch: None,
            database_filesystem: None,
            search_capability: Some(SearchCapabilityErrorPayload {
                requested_mode: *requested_mode,
                state,
                recovery: guidance,
            }),
            next: None,
        }),
        _ => database_filesystem_error_payload(error).map(|(kind, database_filesystem)| {
            CliErrorPayload {
                kind,
                message: error.to_string(),
                refresh_required: None,
                init_required: None,
                worktree_required: None,
                verification_incomplete: None,
                project_mismatch: None,
                database_filesystem: Some(database_filesystem),
                search_capability: None,
                next: None,
            }
        }),
    };
    let Some(error) = details else {
        return Ok(format!("error: {error}\n"));
    };
    let response = CliErrorResponse { error };
    match format {
        OutputFormat::Toon => {
            serde_json::to_value(response).map(|value| encode_agent_payload(&value))
        }
        OutputFormat::Json => {
            serde_json::to_string_pretty(&response).map(|text| format!("{text}\n"))
        }
    }
}

/// Extract a stable content-free `SQLite` placement failure from a CLI error.
fn database_filesystem_error_payload(
    error: &CliError,
) -> Option<(AgentErrorKind, DatabaseFilesystemErrorPayload)> {
    let (kind, path, mount_point, filesystem_type, reason) = match error {
        CliError::Db(DbError::DatabaseFilesystemUnsupported {
            path,
            mount_point,
            filesystem_type,
        }) => (
            AgentErrorKind::DatabaseFilesystemUnsupported,
            path,
            mount_point,
            filesystem_type,
            None,
        ),
        CliError::Db(DbError::DatabaseFilesystemUncertain {
            path,
            mount_point,
            filesystem_type,
            reason,
        }) => (
            AgentErrorKind::DatabaseFilesystemUncertain,
            path,
            mount_point,
            filesystem_type,
            Some(reason.clone()),
        ),
        _ => return None,
    };
    Some((
        kind,
        DatabaseFilesystemErrorPayload {
            path: path.display().to_string(),
            mount_point: mount_point
                .as_deref()
                .map(|mount| mount.display().to_string()),
            filesystem_type: filesystem_type.clone(),
            reason,
            recovery: DATABASE_FILESYSTEM_RECOVERY,
        },
    ))
}

/// Extract one privacy-safe schema mismatch from the shared database error.
fn schema_version_mismatch_payload(error: &CliError) -> Option<SchemaVersionMismatchPayload> {
    let (CliError::Db(database_error) | CliError::Service(ServiceError::Db(database_error))) =
        error
    else {
        return None;
    };
    let (found_schema_version, supported_schema_version) =
        database_error.unsupported_schema_version()?;
    Some(SchemaVersionMismatchPayload {
        found_schema_version,
        supported_schema_version,
        runtime_version: env!("CARGO_PKG_VERSION"),
        recovery: SCHEMA_VERSION_MISMATCH_RECOVERY,
    })
}

/// Extract one privacy-safe migration handoff from the shared database error.
fn schema_migration_required_payload(error: &CliError) -> Option<SchemaMigrationRequiredPayload> {
    let (CliError::Db(database_error) | CliError::Service(ServiceError::Db(database_error))) =
        error
    else {
        return None;
    };
    let (found_schema_version, supported_schema_version, migration_steps_remaining) =
        database_error.supported_schema_migration()?;
    Some(SchemaMigrationRequiredPayload {
        found_schema_version,
        supported_schema_version,
        migration_steps_remaining,
        runtime_version: env!("CARGO_PKG_VERSION"),
        recovery: SCHEMA_MIGRATION_REQUIRED_RECOVERY,
    })
}

/// Open the selected current index through one root-bound read snapshot.
fn open_index_for_current_read(cli: &Cli) -> Result<AtlasStore, CliError> {
    let root = cli.project_root()?;
    if !cli.db.is_file() {
        return Err(runtime::index_init_required(&root, &cli.db));
    }
    open_atlas_store_read_only_for_project(&cli.db, &root)
}

/// Open and verify the durable index before a normal CLI read.
fn open_index_for_read(cli: &Cli) -> Result<AtlasStore, CliError> {
    let root = cli.project_root()?;
    if !cli.db.is_file() {
        return Err(runtime::index_init_required(&root, &cli.db));
    }
    open_fresh_atlas_store_for_project(&cli.db, &root, cli.config.as_deref())
}

/// Open a selected project database for purpose or health mutation.
fn open_index_for_mutation(cli: &Cli) -> Result<AtlasStore, CliError> {
    let root = cli.project_root()?;
    if !cli.db.is_file() {
        return Err(runtime::index_init_required(&root, &cli.db));
    }
    open_atlas_store_for_project(&cli.db, &root)
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

/// Bind, move, or detach a project root without machine-global root state.
fn bind_project_root(
    root: &Path,
    transition: RootTransition,
    nearest_project: bool,
) -> Result<RootReport, CliError> {
    let root = canonical_source_project_root(root)?;
    if !root.is_dir() {
        return Err(CliError::InvalidInput(format!(
            "project root {} is not a directory",
            root.display()
        )));
    }
    let atlas_dir = root.join(".projectatlas");
    let db_path = atlas_dir.join("projectatlas.db");
    let config_path = init_config_path(&root, None);
    if config_path.exists() {
        let config = load_atlas_config(Some(&config_path))?;
        let config_root = canonical_project_root(&config.root)?;
        if config_root != root {
            return Err(config_root_mismatch_error(
                &config_path,
                &config_root,
                &root,
            ));
        }
    }

    let database_exists = db_path.exists();
    if !database_exists && transition != RootTransition::Bind {
        return Err(CliError::Db(
            DbError::ProjectRootTransitionRequiresExistingRoot,
        ));
    }
    if !database_exists {
        init_project_with_config(&root, Some(&config_path))?;
    }
    let transition_result = AtlasStore::transition_project_root(&db_path, &root, transition.into())
        .map_err(runtime::project_store_error)?;
    let configuration_result: Result<(), CliError> = (|| {
        if database_exists {
            init_project_with_config(&root, Some(&config_path))?;
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
        Ok(())
    })();
    configuration_result.map_err(|source| CliError::RootTransitionFollowup {
        root: normalize_native_path_display(root),
        transition,
        source: Box::new(source),
    })?;
    build_root_report_with_transition(&db_path, Some(&config_path), Some(&transition_result))
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
    let metadata = fs::metadata(path).map_err(|source| CliError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > MAX_PURPOSE_REVIEW_INPUT_FILE_BYTES {
        return Err(CliError::InvalidInput(format!(
            "purpose review input file contains {} bytes; maximum is {MAX_PURPOSE_REVIEW_INPUT_FILE_BYTES}",
            metadata.len()
        )));
    }
    let file = fs::File::open(path).map_err(|source| CliError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len())
            .unwrap_or(MAX_PURPOSE_REVIEW_INPUT_FILE_BYTES as usize)
            .min(MAX_PURPOSE_REVIEW_INPUT_FILE_BYTES as usize),
    );
    file.take(MAX_PURPOSE_REVIEW_INPUT_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| CliError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() as u64 > MAX_PURPOSE_REVIEW_INPUT_FILE_BYTES {
        return Err(CliError::InvalidInput(format!(
            "purpose review input file exceeds {MAX_PURPOSE_REVIEW_INPUT_FILE_BYTES} bytes"
        )));
    }
    let text = String::from_utf8(bytes).map_err(|source| {
        CliError::InvalidInput(format!(
            "purpose review input file {} is not UTF-8: {source}",
            path.display()
        ))
    })?;
    let value: serde_json::Value = serde_json::from_str(&text)?;
    let items = value.get("items").cloned().unwrap_or(value);
    let requests: Vec<PurposeReviewRequest> = serde_json::from_value(items)?;
    Ok(requests)
}

/// Build a project-local root identity report.
fn build_root_report(db: &Path, config_path: Option<&Path>) -> Result<RootReport, CliError> {
    build_root_report_with_transition(db, config_path, None)
}

/// Build a root report with optional completed transition details.
fn build_root_report_with_transition(
    db: &Path,
    config_path: Option<&Path>,
    transition: Option<&ProjectRootTransitionResult>,
) -> Result<RootReport, CliError> {
    let settings = build_settings_report(db, config_path, OutputFormat::Toon)?;
    let db_project_root = settings
        .index
        .as_ref()
        .and_then(|index| index.project_root.clone());
    let atlas_dir = Path::new(&settings.db.path)
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let runtime = build_runtime_info();
    let project_instance_id = if db.exists() {
        AtlasStore::open_read_only(db)?
            .project_instance_id()?
            .map(|identity| identity.to_string())
    } else {
        None
    };
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
        project_instance_id,
        transition: transition.map(|result| result.transition.into()),
        previous_root: transition.and_then(|result| result.previous_root.clone()),
        identity_changed: transition.map(|result| result.identity_changed),
        publication_invalidated: transition.map(|result| result.publication_invalidated),
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

/// Maximum bytes copied between cooperative output-control checks.
const CONTROLLED_ENCODING_CHUNK_BYTES: usize = 8 * 1024;

/// One dynamic top-level payload field without an intermediate JSON value.
struct NamedPayload<'a, T: ?Sized> {
    /// Stable adapter-owned field name.
    key: &'a str,
    /// Borrowed service report.
    payload: &'a T,
}

impl<T> Serialize for NamedPayload<'_, T>
where
    T: Serialize + ?Sized,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap as _;

        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry(self.key, self.payload)?;
        map.end()
    }
}

/// Bounded output buffer that observes the request control between chunks.
struct ControlledOutput<'a> {
    /// Serialized bytes retained for the adapter result.
    bytes: Vec<u8>,
    /// Exact request control shared with service analysis.
    control: &'a IndexWorkControl,
    /// Whether a write stopped because the request became terminal.
    interrupted: bool,
}

impl<'a> ControlledOutput<'a> {
    /// Create an empty controlled output buffer.
    const fn new(control: &'a IndexWorkControl) -> Self {
        Self {
            bytes: Vec::new(),
            control,
            interrupted: false,
        }
    }

    /// Translate a terminal writer error back to the typed request failure.
    fn check_terminal(&self) -> Result<(), CliError> {
        self.control.check(IndexWorkStage::RepositoryTraversal)?;
        Ok(())
    }

    /// Convert verified encoder output into UTF-8 text.
    fn into_string(self) -> Result<String, CliError> {
        String::from_utf8(self.bytes).map_err(|source| {
            CliError::Output(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("encoded output was not UTF-8: {source}"),
            ))
        })
    }
}

impl Write for ControlledOutput<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self
            .control
            .check(IndexWorkStage::RepositoryTraversal)
            .is_err()
        {
            self.interrupted = true;
            return Err(io::Error::other("analysis output encoding interrupted"));
        }
        let retained = buffer.len().min(CONTROLLED_ENCODING_CHUNK_BYTES);
        self.bytes.extend_from_slice(&buffer[..retained]);
        Ok(retained)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Controlled reader used by the installed TOON streaming encoder.
struct ControlledInput<'a> {
    /// Compact JSON bytes consumed by the TOON encoder.
    bytes: &'a [u8],
    /// Current input offset.
    offset: usize,
    /// Exact request control shared with service analysis.
    control: &'a IndexWorkControl,
    /// Whether a read stopped because the request became terminal.
    interrupted: bool,
}

impl<'a> ControlledInput<'a> {
    /// Borrow one serialized payload as a cooperatively bounded input stream.
    const fn new(bytes: &'a [u8], control: &'a IndexWorkControl) -> Self {
        Self {
            bytes,
            offset: 0,
            control,
            interrupted: false,
        }
    }
}

impl Read for ControlledInput<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self
            .control
            .check(IndexWorkStage::RepositoryTraversal)
            .is_err()
        {
            self.interrupted = true;
            return Err(io::Error::other("analysis output encoding interrupted"));
        }
        let remaining = &self.bytes[self.offset..];
        let read = remaining
            .len()
            .min(buffer.len())
            .min(CONTROLLED_ENCODING_CHUNK_BYTES);
        buffer[..read].copy_from_slice(&remaining[..read]);
        self.offset = self.offset.saturating_add(read);
        Ok(read)
    }
}

/// Serialize one named analysis envelope while retaining deadline and cancellation.
fn controlled_named_output<T>(
    format: OutputFormat,
    key: &str,
    payload: &T,
    control: &IndexWorkControl,
) -> Result<String, CliError>
where
    T: Serialize + ?Sized,
{
    let payload = NamedPayload { key, payload };
    let mut json = ControlledOutput::new(control);
    let json_result = match format {
        OutputFormat::Toon => serde_json::to_writer(&mut json, &payload),
        OutputFormat::Json => serde_json::to_writer_pretty(&mut json, &payload),
    };
    if json.interrupted {
        json.check_terminal()?;
    }
    json_result?;
    json.check_terminal()?;
    if format == OutputFormat::Json {
        json.bytes.push(b'\n');
        return json.into_string();
    }

    let mut input = ControlledInput::new(&json.bytes, control);
    let mut output = ControlledOutput::new(control);
    let toon_result = toon_format::encode_json_stream_default(&mut input, &mut output);
    if input.interrupted || output.interrupted {
        control.check(IndexWorkStage::RepositoryTraversal)?;
    }
    control.check(IndexWorkStage::RepositoryTraversal)?;
    if let Err(error) = toon_result {
        return Ok(format!(
            "toon_error: {}\n",
            encode_error_text(&error.to_string())
        ));
    }
    output.bytes.push(b'\n');
    output.into_string()
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

/// Borrowed CLI-only coverage filters before typed service parsing.
struct CoverageCliFilters<'a> {
    /// Optional repository path prefix.
    path_prefix: Option<&'a str>,
    /// Optional source parser pass.
    parser: Option<&'a str>,
    /// Optional fact provider pass.
    provider: Option<&'a str>,
    /// Optional relation family.
    relation: Option<&'a str>,
    /// Optional coverage state.
    state: Option<&'a str>,
    /// Optional exact reason.
    reason: Option<&'a str>,
}

/// Build one typed bounded coverage query from explicit CLI filters.
fn coverage_query_from_cli(
    start_index: usize,
    limit: usize,
    filters: &CoverageCliFilters<'_>,
) -> Result<RepositoryCoverageQuery, CliError> {
    Ok(RepositoryCoverageQuery {
        start_index: u32::try_from(start_index).map_err(|error| {
            CliError::InvalidInput(format!("coverage start index is too large: {error}"))
        })?,
        limit: limit.clamp(1, COVERAGE_PAGE_MAX_LIMIT as usize) as u32,
        path_prefix: trimmed_cli_filter(filters.path_prefix)
            .map(|value| normalize_repo_path_prefix(&value)),
        parser: trimmed_cli_filter(filters.parser)
            .as_deref()
            .map(parse_coverage_parser)
            .transpose()?,
        provider: trimmed_cli_filter(filters.provider)
            .as_deref()
            .map(parse_coverage_parser)
            .transpose()?,
        relation: trimmed_cli_filter(filters.relation)
            .as_deref()
            .map(parse_coverage_relation)
            .transpose()?,
        state: trimmed_cli_filter(filters.state)
            .as_deref()
            .map(parse_coverage_state)
            .transpose()?,
        reason: trimmed_cli_filter(filters.reason),
    })
}

/// Stabilize format-specific encoded byte metadata before output and telemetry.
fn finalize_coverage_output(
    format: OutputFormat,
    report: &mut CoverageDiscoveryReport,
) -> Result<String, CliError> {
    for _ in 0..4 {
        let toon = render_coverage_report(report);
        let rendered = serialized_output(format, &toon, report)?;
        let output_bytes = u32::try_from(rendered.len()).map_err(|error| {
            CliError::InvalidInput(format!("coverage output size did not fit u32: {error}"))
        })?;
        if output_bytes > report.max_output_bytes {
            return Err(CliError::InvalidInput(format!(
                "coverage output exceeded {} bytes",
                report.max_output_bytes
            )));
        }
        if report.output_bytes == output_bytes {
            return Ok(rendered);
        }
        report.output_bytes = output_bytes;
    }
    Err(CliError::InvalidInput(
        "coverage output byte metadata did not stabilize".to_string(),
    ))
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
fn print_tracked_directory_output_estimate<T, F>(
    format: OutputFormat,
    store: &AtlasStore,
    usage_instance: Option<UsageRuntimeInstance>,
    session: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    estimate_without_projectatlas: F,
    toon: &str,
    payload: &T,
) -> Result<(), CliError>
where
    T: serde::Serialize,
    F: FnOnce() -> Result<usize, CliError>,
{
    let output = serialized_output(format, toon, payload)?;
    write_stdout(&output)?;
    if usage_instance.is_none() || runtime::telemetry_disabled() {
        return Ok(());
    }
    let Ok(estimated_without_projectatlas) = estimate_without_projectatlas() else {
        return Ok(());
    };
    drop(record_directory_walk_usage_estimate(
        store,
        usage_instance,
        session,
        command,
        path,
        query,
        estimated_without_projectatlas,
        &output,
    ));
    Ok(())
}

/// Record candidate-set telemetry for the exact emitted CLI payload.
fn print_tracked_output_estimate<T, F>(
    format: OutputFormat,
    store: &AtlasStore,
    usage_instance: Option<UsageRuntimeInstance>,
    session: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    estimate_without_projectatlas: F,
    toon: &str,
    payload: &T,
) -> Result<(), CliError>
where
    T: serde::Serialize,
    F: FnOnce() -> Result<usize, CliError>,
{
    let output = serialized_output(format, toon, payload)?;
    write_stdout(&output)?;
    if usage_instance.is_none() || runtime::telemetry_disabled() {
        return Ok(());
    }
    let Ok(estimated_without_projectatlas) = estimate_without_projectatlas() else {
        return Ok(());
    };
    drop(record_usage_estimate(
        store,
        usage_instance,
        session,
        command,
        path,
        query,
        estimated_without_projectatlas,
        &output,
    ));
    Ok(())
}

/// Record baseline-text telemetry for the exact emitted CLI payload.
fn print_tracked_output_text<T: serde::Serialize>(
    format: OutputFormat,
    store: &AtlasStore,
    usage_instance: Option<UsageRuntimeInstance>,
    session: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    baseline_text: &str,
    toon: &str,
    payload: &T,
) -> Result<(), CliError> {
    let output = serialized_output(format, toon, payload)?;
    write_stdout(&output)?;
    drop(record_usage_text(
        store,
        usage_instance,
        session,
        command,
        path,
        query,
        baseline_text,
        &output,
    ));
    Ok(())
}

/// Emit a bounded exact slice and record telemetry for the accepted bytes.
fn print_tracked_slice_output(
    format: OutputFormat,
    store: &AtlasStore,
    usage_instance: Option<UsageRuntimeInstance>,
    session: &str,
    command: &str,
    path: Option<String>,
    query: Option<String>,
    baseline_text: &str,
    report: &CodeSliceDraft,
) -> Result<(), CliError> {
    let output = report.fit_output(|report| {
        let toon = render_code_slice(report);
        serialized_output(format, &toon, report)
    })?;
    write_stdout(&output)?;
    drop(record_usage_text(
        store,
        usage_instance,
        session,
        command,
        path,
        query,
        baseline_text,
        &output,
    ));
    Ok(())
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
    /// `projectatlas snapshot`.
    #[cfg(feature = "derived-snapshot")]
    Snapshot,
    /// `projectatlas parser-pack`.
    #[cfg(feature = "optional-parser-supervisor")]
    ParserPack,
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
            #[cfg(feature = "derived-snapshot")]
            Self::Snapshot => "snapshot",
            #[cfg(feature = "optional-parser-supervisor")]
            Self::ParserPack => "parser-pack",
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
                retrieval_mode: SearchRetrievalModeArg::Lexical,
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
                selector: OptionalSymbolSelectorArgs {
                    symbol: None,
                    symbol_parent: None,
                    symbol_kind: None,
                    symbol_signature: None,
                    symbol_line: None,
                    output_bytes: CodeSliceBudget::DEFAULT_OUTPUT_BYTES,
                },
            },
            Self::Symbols => Command::Symbols {
                command: Box::new(SymbolsCommand::List {
                    file: None,
                    query: None,
                    limit: 1,
                }),
            },
            Self::Settings => Command::Settings,
            #[cfg(feature = "derived-snapshot")]
            Self::Snapshot => Command::Snapshot {
                action: SnapshotAction::Export,
                path: PathBuf::from("snapshot.tar.zst"),
                require_digest: None,
                #[cfg(feature = "derived-snapshot-signatures")]
                signing_key: None,
                #[cfg(feature = "derived-snapshot-signatures")]
                trusted_public_key: None,
            },
            #[cfg(feature = "optional-parser-supervisor")]
            Self::ParserPack => Command::ParserPack {
                storage_root: None,
                command: ParserPackCommand::Status,
            },
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
                coverage: false,
                parser: None,
                provider: None,
                relation: None,
                coverage_state: None,
                reason: None,
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
                benchmark_results: None,
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
                    task: None,
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
    /// Durable identity of this local project instance, when initialized.
    project_instance_id: Option<String>,
    /// Explicit transition completed by this request.
    #[serde(skip_serializing_if = "Option::is_none")]
    transition: Option<RootTransition>,
    /// Previously recorded root for move or detach.
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_root: Option<String>,
    /// Whether the transition created or rotated project identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    identity_changed: Option<bool>,
    /// Whether the transition invalidated derived publication trust.
    #[serde(skip_serializing_if = "Option::is_none")]
    publication_invalidated: Option<bool>,
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

/// Load the tiny optional atlas preview through existing indexed relation-family reads.
fn load_token_atlas_preview(store: &AtlasStore) -> TokenAtlasPreview {
    let control = IndexWorkControl::new(
        projectatlas_core::IndexCancellation::new(),
        Some(TOKEN_ATLAS_READ_TIMEOUT),
    );
    let Some((relations, truncated)) = load_token_atlas_relations(store, &control) else {
        return TokenAtlasPreview::unavailable();
    };
    TokenAtlasPreview::from_relations(&relations, truncated)
}

/// Load the bounded resolved-relation input owned by the optional atlas preview.
fn load_token_atlas_relations(
    store: &AtlasStore,
    control: &IndexWorkControl,
) -> Option<(Vec<LogicalRelation>, bool)> {
    const ADJACENCY_ROWS_PER_ROUND: usize = 512;
    const ADJACENCY_ROUNDS: usize = 2;
    const ADJACENCY_FRONTIER_MAX: usize = 128;
    const SEEDS_PER_RELATION_FAMILY: usize = 4;

    let network_relation_kinds = GraphRelationKind::ALL
        .into_iter()
        .filter(|relation| token_atlas_network_relation(*relation))
        .collect::<Vec<_>>();
    let mut relations = Vec::new();
    let mut adjacency_relation_kinds = Vec::new();
    let mut seeds = BTreeMap::new();
    let mut truncated = false;
    for &relation in &network_relation_kinds {
        let Ok(page) = store.repository_graph_resolved_relation_hubs(
            relation,
            u32::try_from(SEEDS_PER_RELATION_FAMILY).unwrap_or(1),
            Some(control),
        ) else {
            return None;
        };
        if !page.rows.is_empty() {
            adjacency_relation_kinds.push(relation);
        }
        truncated |= page.truncated;
        for seed in page.rows {
            seeds.insert(seed.key().digest().to_string(), seed.key().clone());
        }
    }
    let mut frontier = seeds.into_values().collect::<Vec<_>>();
    let mut seen = frontier
        .iter()
        .map(|seed| seed.digest().to_string())
        .collect::<BTreeSet<_>>();
    for _ in 0..ADJACENCY_ROUNDS {
        if frontier.is_empty() {
            break;
        }
        let mut next = BTreeMap::new();
        for direction in [
            RepositoryGraphDirection::Outbound,
            RepositoryGraphDirection::Inbound,
        ] {
            let page_limit = ((GraphLimits::MAX_ROWS as usize + 1) / frontier.len())
                .saturating_sub(1)
                .clamp(1, ADJACENCY_ROWS_PER_ROUND);
            let mut remaining_rows = page_limit;
            for (index, &relation_kind) in adjacency_relation_kinds.iter().enumerate() {
                if remaining_rows == 0 {
                    truncated = true;
                    break;
                }
                let remaining_families = adjacency_relation_kinds.len() - index;
                let family_limit = remaining_rows.div_ceil(remaining_families);
                let Ok(page) = store.repository_graph_resolved_adjacency_page(
                    &frontier,
                    direction,
                    relation_kind,
                    None,
                    u32::try_from(family_limit).unwrap_or(1),
                    Some(control),
                ) else {
                    return None;
                };
                truncated |= page.truncated;
                remaining_rows = remaining_rows.saturating_sub(page.rows.len());
                for row in page.rows {
                    let relation = row.detail.relation;
                    if let Some(target) = relation.resolution().resolved_target() {
                        for endpoint in [relation.source(), target] {
                            if seen.insert(endpoint.digest().to_string()) {
                                next.insert(endpoint.digest().to_string(), endpoint.clone());
                            }
                        }
                    }
                    relations.push(relation);
                }
            }
        }
        frontier = next.into_values().take(ADJACENCY_FRONTIER_MAX).collect();
    }
    Some((relations, truncated))
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
    let health_findings = store.unresolved_health_finding_count_current()?;
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
        #[cfg(feature = "derived-snapshot")]
        Command::Snapshot { .. } => "snapshot",
        #[cfg(feature = "optional-parser-supervisor")]
        Command::ParserPack { .. } => "parser-pack",
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
        Cli, CliError, Command, GraphRelationKind, OutputFormat,
        SCHEMA_MIGRATION_REQUIRED_RECOVERY, SCHEMA_VERSION_MISMATCH_RECOVERY, SearchRetrievalMode,
        SearchRetrievalModeArg, ServiceError, build_runtime_info, controlled_named_output,
        load_token_atlas_preview, load_token_atlas_relations, render_cli_error,
        render_token_dashboard, render_token_dashboard_with_atlas_at_width,
        schema_migration_required_payload, schema_version_mismatch_payload, serialized_output,
        token_atlas_network_relation, truthy_env,
    };
    #[cfg(feature = "optional-parser-supervisor")]
    use super::{OptionalParserPackLifecycleError, ParserPackCommand};
    use clap::Parser as _;
    use notify::EventKind;
    use projectatlas_core::graph::{
        Completeness, ConfidenceClass, EntitySelector, GraphEntity, GraphIdentityText,
        LogicalRelation, RelationResolution, RepositoryFilePath,
    };
    use projectatlas_core::symbols::{
        CodeSymbol, ParserKind, RelationKind, SymbolGraph, SymbolKind, SymbolRelation,
    };
    use projectatlas_core::telemetry::TokenOverview;
    use projectatlas_core::{
        IndexCancellation, IndexGeneration, IndexWorkControl, IndexWorkFailure, IndexWorkStage,
        Node, NodeKind, normalize_native_path_display,
    };
    use projectatlas_db::{AtlasStore, DbError, RepositoryGraphRelationQuery};
    use projectatlas_fs::ScanOptions;
    use rmcp::model::{CallToolRequestParams, ClientInfo};
    use rmcp::{ClientHandler, ServiceExt};
    use serde_json::{Map, Value, json};
    use std::collections::BTreeMap;
    use std::error::Error;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

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
                    .output()?
                    .status;
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
        let db_display = normalize_native_path_display(db);
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
            language_overrides: BTreeMap::new(),
            admit_optional_languages: false,
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
    fn cli_database_filesystem_failures_are_typed_in_json_and_toon() -> Result<(), Box<dyn Error>> {
        let database = PathBuf::from("project")
            .join(".projectatlas")
            .join("projectatlas.db");
        let error = CliError::Db(DbError::DatabaseFilesystemUncertain {
            path: database,
            mount_point: None,
            filesystem_type: Some("unknown-local".to_string()),
            reason: "filesystem type is not in the supported local profile".to_string(),
        });

        let json_text = render_cli_error(OutputFormat::Json, &error)?;
        let json: Value = serde_json::from_str(&json_text)?;
        require_condition(
            json.pointer("/error/kind").and_then(Value::as_str)
                == Some("database_filesystem_uncertain"),
            "CLI JSON lost the typed filesystem error kind",
        )?;
        require_condition(
            json.pointer("/error/database_filesystem/path")
                .and_then(Value::as_str)
                .is_some_and(|path| path.ends_with("projectatlas.db")),
            "CLI JSON lost the rejected database path",
        )?;
        require_condition(
            json.pointer("/error/database_filesystem/recovery")
                .and_then(Value::as_str)
                .is_some_and(|recovery| recovery.contains("supported local filesystem")),
            "CLI JSON lost database recovery guidance",
        )?;

        let toon = render_cli_error(OutputFormat::Toon, &error)?;
        require_condition(
            toon.contains("database_filesystem_uncertain")
                && toon.contains("unknown-local")
                && toon.contains("supported local filesystem"),
            "CLI TOON lost typed filesystem details",
        )?;
        Ok(())
    }

    #[test]
    fn cli_schema_version_mismatches_are_typed_and_content_free() -> Result<(), Box<dyn Error>> {
        for (error, found) in [
            (
                CliError::Db(DbError::SchemaVersion {
                    found: 17,
                    expected: 16,
                }),
                17,
            ),
            (
                CliError::Service(ServiceError::Db(DbError::SchemaVersion {
                    found: 17,
                    expected: 16,
                })),
                17,
            ),
            (
                CliError::Db(DbError::SchemaVersion {
                    found: 7,
                    expected: 16,
                }),
                7,
            ),
            (
                CliError::Service(ServiceError::Db(DbError::SchemaVersion {
                    found: 7,
                    expected: 16,
                })),
                7,
            ),
        ] {
            let expected_message = format!("unsupported schema version {found}, expected 16");
            let json_text = render_cli_error(OutputFormat::Json, &error)?;
            let json: Value = serde_json::from_str(&json_text)?;
            require_condition(
                json.pointer("/error/kind").and_then(Value::as_str)
                    == Some("schema_version_mismatch")
                    && json
                        .pointer("/error/schema_version_mismatch/found_schema_version")
                        .and_then(Value::as_i64)
                        == Some(found)
                    && json
                        .pointer("/error/schema_version_mismatch/supported_schema_version")
                        .and_then(Value::as_i64)
                        == Some(16)
                    && json
                        .pointer("/error/schema_version_mismatch/runtime_version")
                        .and_then(Value::as_str)
                        == Some(env!("CARGO_PKG_VERSION"))
                    && json
                        .pointer("/error/schema_version_mismatch/recovery")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value.contains("do not reset"))
                    && json.pointer("/error/message").and_then(Value::as_str)
                        == Some(expected_message.as_str()),
                "CLI JSON lost the typed schema-version mismatch contract",
            )?;
            require_condition(
                !json_text.contains(".projectatlas")
                    && !json_text.contains("session_id")
                    && !json_text.contains("project_root"),
                "CLI schema mismatch exposed private database context",
            )?;

            let toon = render_cli_error(OutputFormat::Toon, &error)?;
            require_condition(
                toon.contains("kind: schema_version_mismatch")
                    && toon.contains(&format!("found_schema_version: {found}"))
                    && toon.contains("supported_schema_version: 16")
                    && toon.contains(env!("CARGO_PKG_VERSION"))
                    && toon.contains("do not reset"),
                "CLI TOON lost typed schema-version details or recovery guidance",
            )?;
        }
        for error in [
            CliError::Db(DbError::SchemaVersion {
                found: 8,
                expected: 16,
            }),
            CliError::Service(ServiceError::Db(DbError::SchemaVersion {
                found: 15,
                expected: 16,
            })),
        ] {
            require_condition(
                schema_version_mismatch_payload(&error).is_none(),
                "CLI treated an admitted predecessor as unsupported",
            )?;
            let migration = schema_migration_required_payload(&error).ok_or_else(|| {
                std::io::Error::other("CLI omitted the admitted-predecessor migration handoff")
            })?;
            let expected_steps = u32::try_from(16 - migration.found_schema_version)?;
            require_condition(
                migration.supported_schema_version == 16
                    && migration.migration_steps_remaining == expected_steps,
                "CLI migration handoff drifted from the database migration inventory",
            )?;
            let rendered = render_cli_error(OutputFormat::Json, &error)?;
            let json: Value = serde_json::from_str(&rendered)?;
            require_condition(
                json.pointer("/error/kind").and_then(Value::as_str)
                    == Some("schema_migration_required")
                    && json
                        .pointer("/error/schema_migration_required/migration_steps_remaining")
                        .and_then(Value::as_u64)
                        == Some(u64::from(expected_steps))
                    && json
                        .pointer("/error/schema_migration_required/recovery")
                        .and_then(Value::as_str)
                        == Some(SCHEMA_MIGRATION_REQUIRED_RECOVERY)
                    && rendered.contains("same global `--db`/`--config` selection")
                    && rendered.contains("same MCP server/database binding")
                    && json
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .is_some_and(|message| message.contains("supported migration step"))
                    && !rendered.contains("schema_version_mismatch")
                    && !rendered.contains(SCHEMA_VERSION_MISMATCH_RECOVERY)
                    && !rendered.contains(".projectatlas")
                    && !rendered.contains("project_root"),
                "CLI did not return a private, actionable supported-migration handoff",
            )?;
        }
        Ok(())
    }

    #[test]
    fn cli_search_modes_parse_and_unavailable_state_is_typed() -> Result<(), Box<dyn Error>> {
        let cli = Cli::try_parse_from([
            "projectatlas",
            "search",
            "needle",
            "--retrieval-mode",
            "semantic",
        ])?;
        require_condition(
            matches!(
                *cli.command,
                Command::Search {
                    retrieval_mode: SearchRetrievalModeArg::Semantic,
                    ..
                }
            ),
            "CLI did not parse explicit semantic retrieval",
        )?;

        let error = CliError::Service(ServiceError::SearchCapabilityUnavailable {
            requested_mode: SearchRetrievalMode::Semantic,
            state: "not-installed",
            guidance: "install a compatible semantic generation",
        });
        let json_text = render_cli_error(OutputFormat::Json, &error)?;
        let json: Value = serde_json::from_str(&json_text)?;
        require_condition(
            json.pointer("/error/kind").and_then(Value::as_str)
                == Some("search_capability_unavailable")
                && json
                    .pointer("/error/search_capability/requested_mode")
                    .and_then(Value::as_str)
                    == Some("semantic")
                && json
                    .pointer("/error/search_capability/state")
                    .and_then(Value::as_str)
                    == Some("not-installed")
                && json
                    .pointer("/error/search_capability/recovery")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value.contains("compatible semantic")),
            "CLI JSON lost typed semantic capability state",
        )?;
        let toon = render_cli_error(OutputFormat::Toon, &error)?;
        require_condition(
            toon.contains("search_capability_unavailable")
                && toon.contains("requested_mode")
                && toon.contains("semantic")
                && toon.contains("state")
                && toon.contains("not-installed")
                && toon.contains("compatible semantic"),
            "CLI TOON lost typed semantic capability state",
        )?;
        Ok(())
    }

    #[cfg(feature = "optional-parser-supervisor")]
    #[test]
    fn parser_pack_cli_exposes_every_explicit_lifecycle_operation() -> Result<(), Box<dyn Error>> {
        let artifact = "a".repeat(64);
        let commands = [
            vec![
                "projectatlas",
                "parser-pack",
                "verify",
                "--archive",
                "pack.tar.zst",
            ],
            vec![
                "projectatlas",
                "parser-pack",
                "install",
                "--archive",
                "pack.tar.zst",
            ],
            vec![
                "projectatlas",
                "parser-pack",
                "enable",
                "--artifact",
                artifact.as_str(),
            ],
            vec![
                "projectatlas",
                "parser-pack",
                "update",
                "--archive",
                "pack.tar.zst",
            ],
            vec!["projectatlas", "parser-pack", "disable"],
            vec!["projectatlas", "parser-pack", "remove"],
            vec!["projectatlas", "parser-pack", "status"],
        ];
        for arguments in commands {
            let parsed = Cli::try_parse_from(arguments)?;
            require_condition(
                matches!(
                    *parsed.command,
                    Command::ParserPack {
                        command: ParserPackCommand::Verify { .. }
                            | ParserPackCommand::Install { .. }
                            | ParserPackCommand::Enable { .. }
                            | ParserPackCommand::Update { .. }
                            | ParserPackCommand::Disable
                            | ParserPackCommand::Remove
                            | ParserPackCommand::Status,
                        ..
                    }
                ),
                "parser-pack command did not route to an explicit lifecycle operation",
            )?;
        }
        Ok(())
    }

    #[cfg(feature = "optional-parser-supervisor")]
    #[test]
    fn parser_pack_unsupported_containment_is_typed() -> Result<(), Box<dyn Error>> {
        let error =
            CliError::ParserPack(OptionalParserPackLifecycleError::UnsupportedContainment {
                os: "test-os",
                architecture: "test-arch",
            });
        let json_text = render_cli_error(OutputFormat::Json, &error)?;
        let json: Value = serde_json::from_str(&json_text)?;
        require_condition(
            json.pointer("/error/kind").and_then(Value::as_str) == Some("unsupported_containment"),
            "parser-pack unsupported host did not retain its typed error kind",
        )
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
                content_hash: Some(blake3::hash(b"small").to_hex().to_string()),
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
            content_hash: Some(
                blake3::hash(b"[project]\nroot = \".\"\n")
                    .to_hex()
                    .to_string(),
            ),
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
        assert!(dashboard.contains("N A V I G A T I O N   W O R K   A V O I D E D"));
        assert!(
            dashboard
                .to_ascii_lowercase()
                .contains("file reads avoided")
        );
        assert!(!dashboard.contains("Broad folder walks skipped"));
        assert!(!dashboard.contains("Candidate files not opened"));
        assert!(!dashboard.contains("source steps account for"));
        assert!(dashboard.contains("S A V I N G S   C O M P O S I T I O N"));
        assert!(dashboard.contains("S I G N A L"));
        assert!(dashboard.contains("W H E R E   T H E   S A V I N G S   C A M E   F R O M"));
        assert!(dashboard.contains("C A L I B R A T I O N   &   N O T E S"));
        assert!(dashboard.contains("Confidence"));
        assert!(dashboard.contains("Tokenizer audit"));
        assert!(
            dashboard
                .chars()
                .any(|character| matches!(character, '█' | '\u{2801}'..='\u{28ff}'))
        );
        assert!(!dashboard.contains("Gross tokens: without vs with ProjectAtlas"));
        assert!(!dashboard.contains("REQUESTED BENCHMARK EVIDENCE"));
        assert!(!dashboard.contains("How ProjectAtlas helped"));
        assert!(!dashboard.contains("Saved-token trends"));
    }

    #[test]
    fn token_atlas_network_excludes_containment() {
        assert!(!token_atlas_network_relation(GraphRelationKind::Legacy(
            RelationKind::Contains,
        )));
        assert!(token_atlas_network_relation(GraphRelationKind::Legacy(
            RelationKind::Imports,
        )));
    }

    #[test]
    fn token_atlas_loader_ranks_resolved_hubs_before_bounded_rendering()
    -> Result<(), Box<dyn Error>> {
        const UNRESOLVED_PREFIX_ROWS: usize = 129;
        const BRANCHES: usize = 16;
        const LEAVES_PER_BRANCH: usize = 3;
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("token-atlas-loader");
        fs::create_dir_all(root.join("src"))?;
        let mut store = AtlasStore::open_for_project(&root.join("projectatlas.db"), &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("token atlas fixture project identity is missing")?;
        let generation = IndexGeneration::new(1);
        let entity = |path: &str| {
            Ok::<_, Box<dyn Error>>(GraphEntity::new(
                project,
                EntitySelector::File {
                    path: RepositoryFilePath::new(Path::new(path))?,
                },
                generation,
            )?)
        };
        let node = |path: &str, kind: NodeKind, hash: Option<&str>| {
            let is_file = kind == NodeKind::File;
            Node {
                path: path.to_string(),
                kind,
                parent_path: is_file.then(|| "src".to_string()),
                extension: is_file.then(|| ".rs".to_string()),
                language: is_file.then(|| "rust".to_string()),
                size_bytes: is_file.then_some(17),
                mtime_ns: is_file.then_some(1),
                content_hash: hash.map(str::to_string),
            }
        };
        let mut nodes = vec![node("src", NodeKind::Folder, None)];
        let mut entities = Vec::new();
        let mut add_file_entity = |path: String| -> Result<GraphEntity, Box<dyn Error>> {
            let graph_entity = entity(&path)?;
            nodes.push(node(&path, NodeKind::File, Some(&path)));
            entities.push(graph_entity.clone());
            Ok(graph_entity)
        };
        let source = add_file_entity("src/source.rs".to_string())?;
        let mut resolved_calls = Vec::new();
        for branch in 0..BRANCHES {
            let branch_root = add_file_entity(format!("src/branch-{branch:02}-root.rs"))?;
            resolved_calls.push(LogicalRelation::new(
                &source,
                GraphRelationKind::Legacy(RelationKind::Calls),
                RelationResolution::resolved(&branch_root)?,
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?);
            let mut leaves = Vec::new();
            for leaf in 0..LEAVES_PER_BRANCH {
                let entity = add_file_entity(format!("src/branch-{branch:02}-leaf-{leaf}.rs"))?;
                resolved_calls.push(LogicalRelation::new(
                    &branch_root,
                    GraphRelationKind::Legacy(RelationKind::Calls),
                    RelationResolution::resolved(&entity)?,
                    ConfidenceClass::Exact,
                    Completeness::Complete,
                    generation,
                )?);
                leaves.push(entity);
            }
            for (left, right) in [(0, 1), (1, 2), (2, 0), (1, 0), (2, 1), (0, 2)] {
                resolved_calls.push(LogicalRelation::new(
                    &leaves[left],
                    GraphRelationKind::Legacy(RelationKind::Calls),
                    RelationResolution::resolved(&leaves[right])?,
                    ConfidenceClass::Exact,
                    Completeness::Complete,
                    generation,
                )?);
            }
        }
        let mut resolved_import = None;
        for index in 0..1_000 {
            let target = add_file_entity(format!("src/import-target-{index:03}.rs"))?;
            let relation = LogicalRelation::new(
                &source,
                GraphRelationKind::Legacy(RelationKind::Imports),
                RelationResolution::resolved(&target)?,
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?;
            if relation.key().digest().starts_with('f') {
                resolved_import = Some(relation);
                break;
            }
        }
        let resolved_import = resolved_import.ok_or(
            "could not construct a deterministic resolved relation after the prefix boundary",
        )?;
        let mut unresolved_imports = Vec::with_capacity(UNRESOLVED_PREFIX_ROWS);
        for index in 0..1_000 {
            let relation = LogicalRelation::new(
                &source,
                GraphRelationKind::Legacy(RelationKind::Imports),
                RelationResolution::Unresolved {
                    reference: GraphIdentityText::new(format!("missing-import-{index:04}"))?,
                },
                ConfidenceClass::Low,
                Completeness::Partial,
                generation,
            )?;
            if relation.key().digest() < resolved_import.key().digest() {
                unresolved_imports.push(relation);
                if unresolved_imports.len() == UNRESOLVED_PREFIX_ROWS {
                    break;
                }
            }
        }
        if unresolved_imports.len() != UNRESOLVED_PREFIX_ROWS {
            return Err(io::Error::other(
                "could not construct the deterministic unresolved relation-key prefix",
            )
            .into());
        }
        let contained = add_file_entity("src/contained.rs".to_string())?;
        let containment = LogicalRelation::new(
            &source,
            GraphRelationKind::Legacy(RelationKind::Contains),
            RelationResolution::resolved(&contained)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation,
        )?;
        let mut graph_relations = unresolved_imports;
        graph_relations.push(resolved_import.clone());
        graph_relations.extend(resolved_calls);
        graph_relations.push(containment);
        let mut publication = store.begin_index_publication("token-atlas-loader")?;
        publication.begin_scan_replacement()?;
        publication.upsert_scan_node_batch(&nodes)?;
        publication.finish_scan_replacement()?;
        publication.replace_repository_graph(project, &entities, &graph_relations, &[], &[])?;
        publication.complete()?;

        let raw = store.repository_graph_relations(
            RepositoryGraphRelationQuery::Family {
                relation: GraphRelationKind::Legacy(RelationKind::Imports),
            },
            128,
        )?;
        if !raw.truncated {
            return Err(io::Error::other("raw relation-family fixture was not truncated").into());
        }
        if raw
            .rows
            .iter()
            .any(|relation| relation.resolution().resolved_target().is_some())
        {
            return Err(io::Error::other(
                "raw relation-key prefix unexpectedly reached a resolved relation",
            )
            .into());
        }
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let (relations, truncated) = load_token_atlas_relations(&store, &control)
            .ok_or("token atlas relation loader unexpectedly failed")?;
        if !truncated {
            return Err(io::Error::other("token atlas omitted bounded-source state").into());
        }
        if !relations.iter().all(|relation| {
            relation.resolution().resolved_target().is_some()
                && token_atlas_network_relation(relation.kind())
        }) {
            return Err(io::Error::other(
                "token atlas retained unresolved or containment relations",
            )
            .into());
        }
        if !relations
            .iter()
            .any(|relation| relation.key() == resolved_import.key())
        {
            return Err(io::Error::other(
                "token atlas did not recover the resolved relation behind the unresolved prefix",
            )
            .into());
        }
        let atlas = load_token_atlas_preview(&store);
        let dashboard = render_token_dashboard_with_atlas_at_width(
            &TokenOverview::from_estimated_totals(4, 16_000, 4_000),
            Some("resolved-loader"),
            &atlas,
            200,
        );
        if !dashboard.contains("48 nodes •") {
            let status = dashboard
                .lines()
                .find(|line| line.contains(" nodes • "))
                .unwrap_or("atlas status line missing");
            return Err(io::Error::other(format!(
                "resolved full-family atlas did not fill the bounded 200x50 node preview: {status}"
            ))
            .into());
        }
        let cancellation = IndexCancellation::new();
        cancellation.cancel();
        let cancelled = IndexWorkControl::new(cancellation, None);
        if load_token_atlas_relations(&store, &cancelled).is_some() {
            return Err(io::Error::other(
                "token atlas loader ignored its shared cancellation boundary",
            )
            .into());
        }
        Ok(())
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

    #[test]
    fn analysis_output_encoding_is_equivalent_and_cancellable() -> Result<(), Box<dyn Error>> {
        struct CancelDuringSerialize(IndexCancellation);

        impl serde::Serialize for CancelDuringSerialize {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                use serde::ser::SerializeSeq as _;

                let mut sequence = serializer.serialize_seq(Some(2))?;
                sequence.serialize_element(&1_u8)?;
                self.0.cancel();
                sequence.serialize_element(&2_u8)?;
                sequence.end()
            }
        }

        let payload = json!({ "mode": "impact", "findings": [1, 2, 3] });
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let expected =
            projectatlas_core::toon::encode_agent_payload(&json!({ "symbol_relations": payload }));
        let encoded =
            controlled_named_output(OutputFormat::Toon, "symbol_relations", &payload, &control)?;
        if encoded != expected {
            return Err(io::Error::other("controlled TOON output changed its wire format").into());
        }

        let cancellation = IndexCancellation::new();
        let control = IndexWorkControl::new(cancellation.clone(), None);
        let result = controlled_named_output(
            OutputFormat::Json,
            "symbol_relations",
            &CancelDuringSerialize(cancellation),
            &control,
        );
        if !matches!(
            result,
            Err(CliError::IndexWork(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::RepositoryTraversal
            }))
        ) {
            return Err(
                io::Error::other("analysis adapter continued encoding after cancellation").into(),
            );
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
        if tools.tools.len() != REQUIRED_MCP_TOOL_NAMES.len() {
            return Err(format!(
                "MCP inventory grew outside the closed required surface: {} != {}",
                tools.tools.len(),
                REQUIRED_MCP_TOOL_NAMES.len()
            )
            .into());
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
        if !schema_has_property("atlas_root_set", "transition")? {
            return Err("atlas_root_set did not advertise the transition selector".into());
        }
        if schema_has_property("atlas_health", "task")? {
            return Err("atlas_health advertised the purpose-queue task parameter".into());
        }
        if !schema_has_property("atlas_purpose_queue", "task")? {
            return Err("atlas_purpose_queue did not advertise the curator task parameter".into());
        }
        if !schema_has_property("atlas_session_brief", "purpose_task")?
            || !schema_has_property("atlas_session_brief", "purpose_limit")?
        {
            return Err("atlas_session_brief did not advertise purpose handoff controls".into());
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

        let mut purpose_queue_args = Map::new();
        purpose_queue_args.insert("task".to_string(), json!("mcp-smoke-purpose"));
        let purpose_queue = client
            .peer()
            .call_tool(
                CallToolRequestParams::new("atlas_purpose_queue")
                    .with_arguments(purpose_queue_args),
            )
            .await?;
        let purpose_queue_text = purpose_queue
            .content
            .first()
            .and_then(|content| content.raw.as_text())
            .map(|text| text.text.as_str())
            .ok_or_else(|| std::io::Error::other("purpose queue result did not contain text"))?;
        if !purpose_queue_text.contains("purpose_curation:")
            || !purpose_queue_text.contains("project_instance_id:")
            || !purpose_queue_text.contains("active_generation:")
            || !purpose_queue_text.contains("task: \"mcp-smoke-purpose\"")
            || !purpose_queue_text.contains("work_key:")
            || !purpose_queue_text.contains("actionable: true")
            || !purpose_queue_text.contains("curation_scope: low")
            || !purpose_queue_text.contains("source_only: true")
            || !purpose_queue_text.contains("folder_scope: all")
            || !purpose_queue_text.contains("file_scope: high_impact")
            || !purpose_queue_text.contains("purpose_curation_items[")
            || !purpose_queue_text.contains("work_key,state_token")
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

        for changed_repo in [&repo_a, &repo_b] {
            let mut refresh_args = Map::new();
            refresh_args.insert(
                "project_path".to_string(),
                json!(changed_repo.to_string_lossy().to_string()),
            );
            let refresh = call_text!("atlas_scan", refresh_args);
            if !refresh.contains("scan:") {
                return Err(format!(
                    "routing fixture refresh failed for {}: {refresh}",
                    changed_repo.display()
                )
                .into());
            }
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

        let mut rejected_move_args = Map::new();
        rejected_move_args.insert(
            "root".to_string(),
            json!(repo_b.to_string_lossy().to_string()),
        );
        rejected_move_args.insert("transition".to_string(), json!("move"));
        let rejected_move = call_text!("atlas_root_set", rejected_move_args);
        if !rejected_move.contains("move destination") {
            return Err(
                format!("atlas_root_set did not reject a same-root move: {rejected_move}").into(),
            );
        }
        let after_failed_transition = call_text!("atlas_root", Map::new());
        if !after_failed_transition.contains(&normalize_native_path_display(&repo_a)) {
            return Err("failed durable root transition changed active MCP routing".into());
        }

        let mut bind_b_args = Map::new();
        bind_b_args.insert(
            "root".to_string(),
            json!(repo_b.to_string_lossy().to_string()),
        );
        let bind_b = call_text!("atlas_root_set", bind_b_args);
        if !bind_b.contains("transition: bind")
            || !bind_b.contains("project_instance_id:")
            || !bind_b.contains("verified: true")
        {
            return Err(format!(
                "atlas_root_set omitted transition did not preserve bind behavior: {bind_b}"
            )
            .into());
        }
        let bound_root_b = call_text!("atlas_root", Map::new());
        if !bound_root_b.contains(&normalize_native_path_display(&repo_b)) {
            return Err("successful durable root bind did not change active MCP routing".into());
        }
        let refreshed_bound_b = call_text!("atlas_scan", Map::new());
        if !refreshed_bound_b.contains("scan:") {
            return Err("durably bound project could not refresh after config generation".into());
        }

        let mut set_a_args = Map::new();
        set_a_args.insert(
            "project_path".to_string(),
            json!(repo_a.to_string_lossy().to_string()),
        );
        let set_a = call_text!("atlas_set_project_path", set_a_args);
        if !set_a.contains("project:") || !set_a.contains("status: active") {
            return Err("atlas_set_project_path did not restore repo A routing".into());
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
        if !missing_index_overview.contains("kind: init_required")
            || !missing_index_overview.contains("tool: atlas_init")
            || !missing_index_overview.contains(&normalize_native_path_display(
                super::runtime::canonical_project_root(&empty_repo)?,
            ))
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

        let mut detach_b_args = Map::new();
        detach_b_args.insert(
            "root".to_string(),
            json!(repo_b.to_string_lossy().to_string()),
        );
        detach_b_args.insert("transition".to_string(), json!("detach"));
        let detach_b = call_text_on!("atlas_root_set", detach_b_args);
        if !detach_b.contains("transition: detach")
            || !detach_b.contains("identity_changed: true")
            || !detach_b.contains("publication_invalidated: true")
        {
            return Err(
                format!("atlas_root_set did not accept explicit detach: {detach_b}").into(),
            );
        }
        let root_after_detach = call_text_on!("atlas_root", Map::new());
        if !root_after_detach.contains(&normalize_native_path_display(&repo_b))
            || !root_after_detach.contains("verified: true")
        {
            return Err("explicit detach did not activate the transitioned root".into());
        }

        client.cancel().await?;
        server_handle.await?.map_err(std::io::Error::other)?;
        Ok(())
    }
}
