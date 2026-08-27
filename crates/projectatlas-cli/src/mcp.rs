//! Purpose: Serve `ProjectAtlas` repository intelligence over MCP.
//! Native MCP adapter for `ProjectAtlas` agent integrations.

use crate::atlas_map::{
    AtlasMapConfig, IgnoreEntryKind, LintOptions, add_ignore_entry, effective_config_report,
    init_gitignore, init_project_with_config, list_ignore_entries, load_atlas_config,
    load_atlas_config_for_root, remove_ignore_entry, write_map,
};
use crate::runtime::{
    DEFAULT_HEALTH_LIMIT, INDEX_WORKER_SAFE_CEILING, IndexInitRequired, IndexProjectMismatch,
    IndexRefreshRequired, IndexVerificationIncomplete, InitBootstrapOptions, InitHydrationPhase,
    InitHydrationStatus, InitPhaseStatus, InitScanPhase, InitSetupReport, MAX_HEALTH_LIMIT,
    MAX_SYMBOL_FILE_BYTES, ProjectWorktreeRequired, PurposeCuratorHandoff, PurposeLintLevel,
    PurposeReviewRequest, ResetIndexReport, ScanReport, ScanRuntimePlan,
    SettingsClassifiedNavigationReport, SourceObservationRegistry, SymbolBuildOptions,
    UsageRuntimeInstance, VerifiedReadOutcome, VerifiedReadStamp, build_settings_report,
    byte_count_to_tokens, canonical_project_root, canonical_source_project_root,
    classified_navigation_capabilities, classified_ranked_file_nodes_with_reasons,
    config_root_mismatch_error, default_mcp_project_root,
    estimated_source_tokens_for_indexed_files, estimated_source_tokens_for_paths,
    federated_worktree_error, index_init_required, index_work_control, init_config_path,
    init_next_steps, lint_project, load_synchronized_repository_token_report,
    lossless_native_path_display, lossless_project_root_display, next_step_report_payload,
    next_step_report_with_selection, normalized_folder_filter, open_atlas_store_for_project,
    open_atlas_store_read_only_for_project, open_federated_atlas_stores_for_project,
    purpose_curation_page, purpose_curator_handoff, ranked_file_nodes_with_reasons,
    ranked_folder_nodes_with_reasons, read_indexed_file_content,
    reconcile_hydrated_index_controlled, record_directory_walk_usage_estimate,
    record_usage_estimate, record_usage_text, render_classified_ranked_file_rows,
    render_classified_symbol_rows, render_health_page, render_purpose_curation_page,
    render_purpose_review_report, require_current_worktree_usage_snapshot,
    require_registered_worktree_lifecycle, reset_index_files, reset_index_files_with_revalidation,
    review_purposes, run_init_bootstrap, run_scan_pipeline_controlled,
    run_single_watch_refresh_controlled, run_symbol_build_pipeline_controlled,
    strip_legacy_purpose, telemetry_disabled, validate_purpose_review_admission,
    validated_indexed_file_key, watcher_status_report,
};
#[cfg(all(test, unix))]
use crate::runtime::{IndexReadStatus, IndexRefreshReason, IndexRefreshScope};
#[cfg(test)]
use crate::runtime::{
    PURPOSE_CURATOR_RECOMMENDED_REASONING, db_sidecar_path, mcp_config_path_for_db,
    run_scan_pipeline, synchronize_registered_worktree_usage,
};
use crate::token_tui::{
    TokenDashboardTheme, render_token_dashboard_plain_with_theme,
    render_token_trend_dashboard_plain_with_theme,
};
use crate::{
    AgentErrorKind, CliError, DEFAULT_FILE_SUMMARY_LIMIT, DatabaseFilesystemErrorPayload,
    HarnessConfig, OutputFormat, RootTransition, RuntimeInfoReport, SchemaMigrationRequiredPayload,
    SchemaVersionMismatchPayload, SearchRetrievalModeArg, build_harness_mcp_config_report,
    build_parity_report, build_repository_control_report, build_root_report, build_runtime_info,
    controlled_named_output, database_filesystem_error_payload, finalize_coverage_output,
    render_code_slice, render_file_summary, render_parity_report, render_repository_control_report,
    render_root_report, render_runtime_info, render_search_report, render_watch_status,
    schema_migration_required_payload, schema_version_mismatch_payload,
};
use projectatlas_core::graph::{
    Completeness, ConfidenceClass, CoverageRecord, DocumentTargetUnresolvedReason, EntitySelector,
    ExternalSelector, GraphIdentityText, GraphLimitKind, GraphLimits, GraphRelationKind,
    ProjectInstanceId, RelationOccurrence, RelationResolution, RepositoryFilePath,
    ReusableTargetSelector, SourceSpan,
};
use projectatlas_core::health::Severity;
use projectatlas_core::language::{ContentClassification, ContentSelection};
use projectatlas_core::outline::build_outline;
use projectatlas_core::symbols::ParserKind;
use projectatlas_core::telemetry::{
    TOKEN_BASELINE_DIRECTORY_WALK, TOKEN_BASELINE_SELECTED_CANDIDATES,
    TOKEN_BUCKET_NAVIGATION_AVOIDANCE, TOKEN_CONFIDENCE_INFERRED, TOKEN_CONFIDENCE_POLICY_ESTIMATE,
    TokenTrendWindow, UsageInstanceOwner, usage_from_estimates_with_context, usage_from_text,
};
use projectatlas_core::toon::{
    encode_agent_payload, render_outline, render_overview, render_ranked_nodes,
    render_symbol_relations, render_token_overview, render_token_trends,
};
use projectatlas_core::{
    CanonicalProjectRoot, IndexGeneration, IndexWorkControl, IndexWorkFailure, IndexWorkStage,
    MAX_GIT_WORKTREE_REGISTRATIONS, NavigationNextCall, NavigationNextCapability, Overview,
    PurposeSource, PurposeStatus, RankedConnection, RankedConnectionCount, RankedConnectionKind,
    RankedConnectionTarget, RankedNode, RankedReasonCode, normalize_native_path_display,
    normalize_native_path_display_str, normalize_repo_path, normalize_repo_path_prefix,
    validated_repo_file_key, validated_repo_node_key,
};
use projectatlas_db::{
    ActiveWorktreeRegistrationGuard, AtlasStore, DbError, HealthQuery, HealthResolution,
    HealthScope, PreparedWorktreeHydrationCandidate, RepositoryCoverageQuery, WorktreeAlias,
    WorktreeHydrationActivation, WorktreeRegistration, WorktreeRegistrationState,
    WorktreeUsageSnapshot, WorktreeUsageSyncState, read_legacy_project_root_candidate_read_only,
    read_project_root_identity_read_only, verify_project_database,
};
use projectatlas_fs::worktree::{
    GitRepositoryStructure, GitWorktreeEntry, GitWorktreeRole, GitWorktreeState,
    RepositoryStructure, discover_repository_structure, git_administrative_identity,
};
#[cfg(test)]
use projectatlas_service::build_file_summary_from_source;
use projectatlas_service::{
    COVERAGE_PAGE_MAX_LIMIT, CodeSliceBudget, CoverageDigest, CoverageTrustState,
    DetailedRelationBudget, DetailedRelationNode, DetailedRelationQuery, DetailedRelationReport,
    DetailedRelationRow, DetailedRelationWork, FederatedDetailedRelationReport,
    FederatedParticipant, FederatedRelationWork, FederatedRendezvous, FederatedStore,
    FileCallSummary, FileSummaryReport, FileSymbolSummary, GitImpactSelection,
    RelationAnalysisMode, RelationAnalysisQuery, RelationAnchor, RelationDirection,
    RelationNextCall, RelationPurpose, RelationTotalState, SearchQuery, ServiceError,
    SymbolSliceSelector, TokenReport, TokenReportRequest,
    build_file_summary_from_source_with_selection, load_coverage_discovery_controlled,
    load_detailed_relation_page, load_federated_detailed_relations,
    load_federated_relation_analysis, load_relation_analysis, load_token_report,
    parse_coverage_parser, parse_coverage_relation, parse_coverage_state,
    parse_relation_confidence, parse_relation_direction, parse_relation_resolution,
    parse_symbol_kind, read_indexed_code_slice_from_source_bounded_with_selection,
    read_symbol_slice_from_source_bounded_with_selection, search_indexed_files_with_control,
    validate_federated_root_count,
};
use rmcp::handler::server::{
    router::tool::ToolRouter, tool::IntoCallToolResult, wrapper::Parameters,
};
use rmcp::model::{CallToolResponse, Implementation, ServerCapabilities, ServerInfo};
use rmcp::schemars;
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Arc, Mutex, RwLock,
    atomic::{AtomicU64, Ordering},
};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Text response routed through `rmcp`'s native tool-success/tool-error conversion.
#[derive(Debug, PartialEq, Eq)]
struct McpToolTextResult(Result<String, String>);

impl std::ops::Deref for McpToolTextResult {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        match &self.0 {
            Ok(text) | Err(text) => text,
        }
    }
}

impl std::fmt::Display for McpToolTextResult {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self)
    }
}

impl IntoCallToolResult for McpToolTextResult {
    fn into_call_tool_result(self) -> Result<CallToolResponse, rmcp::ErrorData> {
        self.0.into_call_tool_result()
    }
}

/// MCP tools required for the agent-first repository-intelligence surface.
pub(crate) const REQUIRED_MCP_TOOL_NAMES: &[&str] = &[
    MCP_TOOL_ATLAS_SET_PROJECT_PATH,
    MCP_TOOL_ATLAS_WORKTREE_LIST,
    MCP_TOOL_ATLAS_WORKTREE_ADD,
    MCP_TOOL_ATLAS_WORKTREE_REMOVE,
    MCP_TOOL_ATLAS_INIT,
    MCP_TOOL_ATLAS_MAP,
    MCP_TOOL_ATLAS_ROOT,
    MCP_TOOL_ATLAS_ROOT_SET,
    MCP_TOOL_ATLAS_CONFIG,
    MCP_TOOL_ATLAS_IGNORE_LIST,
    MCP_TOOL_ATLAS_IGNORE_INIT_GITIGNORE,
    MCP_TOOL_ATLAS_IGNORE_ADD,
    MCP_TOOL_ATLAS_IGNORE_REMOVE,
    MCP_TOOL_ATLAS_SCAN,
    MCP_TOOL_ATLAS_OVERVIEW,
    MCP_TOOL_ATLAS_FOLDERS,
    MCP_TOOL_ATLAS_FILES,
    MCP_TOOL_ATLAS_NEXT,
    MCP_TOOL_ATLAS_OUTLINE,
    MCP_TOOL_ATLAS_FILE_SUMMARY,
    MCP_TOOL_ATLAS_SEARCH,
    MCP_TOOL_ATLAS_SLICE,
    MCP_TOOL_ATLAS_SYMBOLS_BUILD,
    MCP_TOOL_ATLAS_SYMBOLS,
    MCP_TOOL_ATLAS_SYMBOL_RELATIONS,
    MCP_TOOL_ATLAS_HEALTH,
    MCP_TOOL_ATLAS_HEALTH_RESOLVE,
    MCP_TOOL_ATLAS_LINT,
    MCP_TOOL_ATLAS_TOKEN_REPORT,
    MCP_TOOL_ATLAS_PARITY_REPORT,
    MCP_TOOL_ATLAS_SETTINGS,
    MCP_TOOL_ATLAS_WATCH_STATUS,
    MCP_TOOL_ATLAS_WATCH_ONCE,
    MCP_TOOL_ATLAS_STRIP_LEGACY_PURPOSE,
    MCP_TOOL_ATLAS_RESET_INDEX,
    MCP_TOOL_ATLAS_MCP_CONFIG,
    MCP_TOOL_ATLAS_RUNTIME_INFO,
    MCP_TOOL_ATLAS_SESSION_BRIEF,
    MCP_TOOL_ATLAS_TASK_STATUS,
    MCP_TOOL_ATLAS_TASK_CANCEL,
    MCP_TOOL_ATLAS_PURPOSE_QUEUE,
    MCP_TOOL_ATLAS_PURPOSE_SET,
    MCP_TOOL_ATLAS_PURPOSE_REVIEW,
];

/// MCP tool name for active project selection.
const MCP_TOOL_ATLAS_SET_PROJECT_PATH: &str = "atlas_set_project_path";
/// MCP tool name for bounded structural worktree inventory.
const MCP_TOOL_ATLAS_WORKTREE_LIST: &str = "atlas_worktree_list";
/// MCP tool name for control-atlas worktree registration.
const MCP_TOOL_ATLAS_WORKTREE_ADD: &str = "atlas_worktree_add";
/// MCP tool name for control-atlas worktree retirement.
const MCP_TOOL_ATLAS_WORKTREE_REMOVE: &str = "atlas_worktree_remove";
/// MCP tool name for project initialization.
const MCP_TOOL_ATLAS_INIT: &str = "atlas_init";
/// MCP tool name for compatibility map exports.
const MCP_TOOL_ATLAS_MAP: &str = "atlas_map";
/// MCP tool name for root diagnostics.
const MCP_TOOL_ATLAS_ROOT: &str = "atlas_root";
/// MCP tool name for binding a project root.
const MCP_TOOL_ATLAS_ROOT_SET: &str = "atlas_root_set";
/// MCP tool name for effective config reports.
const MCP_TOOL_ATLAS_CONFIG: &str = "atlas_config";
/// MCP tool name for ignore policy reports.
const MCP_TOOL_ATLAS_IGNORE_LIST: &str = "atlas_ignore_list";
/// MCP tool name for project `.gitignore` initialization.
const MCP_TOOL_ATLAS_IGNORE_INIT_GITIGNORE: &str = "atlas_ignore_init_gitignore";
/// MCP tool name for adding manual ignore entries.
const MCP_TOOL_ATLAS_IGNORE_ADD: &str = "atlas_ignore_add";
/// MCP tool name for removing manual ignore entries.
const MCP_TOOL_ATLAS_IGNORE_REMOVE: &str = "atlas_ignore_remove";
/// MCP tool name for repository scans.
const MCP_TOOL_ATLAS_SCAN: &str = "atlas_scan";
/// MCP tool name for repository overviews.
const MCP_TOOL_ATLAS_OVERVIEW: &str = "atlas_overview";
/// MCP tool name for folder ranking.
const MCP_TOOL_ATLAS_FOLDERS: &str = "atlas_folders";
/// MCP tool name for file ranking.
const MCP_TOOL_ATLAS_FILES: &str = "atlas_files";
/// MCP tool name for next-step recommendations.
const MCP_TOOL_ATLAS_NEXT: &str = "atlas_next";
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
/// MCP tool name for lint reports.
const MCP_TOOL_ATLAS_LINT: &str = "atlas_lint";
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
/// MCP tool name for generated MCP config documents.
const MCP_TOOL_ATLAS_MCP_CONFIG: &str = "atlas_mcp_config";
/// MCP tool name for runtime identity reports.
const MCP_TOOL_ATLAS_RUNTIME_INFO: &str = "atlas_runtime_info";
/// MCP tool name for compact agent startup briefs.
const MCP_TOOL_ATLAS_SESSION_BRIEF: &str = "atlas_session_brief";
/// MCP tool name for task-progress status lookup.
const MCP_TOOL_ATLAS_TASK_STATUS: &str = "atlas_task_status";
/// MCP tool name for task-progress cancellation.
const MCP_TOOL_ATLAS_TASK_CANCEL: &str = "atlas_task_cancel";
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
/// Maximum distinct project bindings retained before inactive telemetry rotation.
const MCP_TELEMETRY_PROJECT_BINDING_LIMIT: usize = 64;
/// Hard ceiling for the default agent-facing settings diagnostic.
const MCP_SETTINGS_RESPONSE_MAX_BYTES: usize = 64_000;
/// Prefix for an oversized settings-response diagnostic.
const MCP_SETTINGS_RESPONSE_LIMIT_PREFIX: &str = "settings response requires ";
/// Separator for the observed and permitted settings-response byte counts.
const MCP_SETTINGS_RESPONSE_LIMIT_SEPARATOR: &str = " bytes, exceeding the ";
/// Suffix for an oversized settings-response diagnostic.
const MCP_SETTINGS_RESPONSE_LIMIT_SUFFIX: &str = "-byte diagnostic limit";
/// Maximum generation/filter token baselines retained by one MCP process.
const MCP_SOURCE_TOKEN_BASELINE_LIMIT: usize = 128;
/// Default MCP config server key.
const MCP_DEFAULT_CONFIG_SERVER_NAME: &str = "projectatlas";
/// Recovery guidance when a path names a subfolder rather than another selected root.
const SELECTED_ROOT_ASSERTION_GUIDANCE: &str = "pass project_path or call atlas_set_project_path for another repository, or use normal filesystem tools such as Get-Content or rg for files inside the selected project";
/// Recovery guidance when a path escapes the selected `ProjectAtlas` root.
const OUTSIDE_SELECTED_PROJECT_GUIDANCE: &str = "pass project_path or call atlas_set_project_path for that repository, or use normal filesystem tools such as Get-Content or rg for files outside the selected ProjectAtlas project";
/// Current-directory root alias.
const CURRENT_DIR_ALIAS: &str = ".";
/// `ProjectAtlas` MCP server display name.
const MCP_SERVER_NAME: &str = "ProjectAtlas";
/// Request-local MCP cancellation monitor thread name.
const MCP_CANCELLATION_MONITOR_THREAD_NAME: &str = "projectatlas-mcp-cancel";
/// Prefix for cancellation-monitor startup failures.
const MCP_CANCELLATION_MONITOR_START_ERROR_PREFIX: &str =
    "MCP request cancellation monitor could not start: ";
/// Prefix for invalid registered worktree roots.
const MCP_ERROR_REGISTERED_WORKTREE_ROOT_INVALID_PREFIX: &str =
    "registered worktree root is invalid: ";
/// MCP error lock-poison message.
const MCP_PROJECT_STATE_LOCK_POISONED: &str = "MCP project state lock poisoned";
/// Prefix used only if structured MCP error serialization fails.
const MCP_ERROR_SERIALIZATION_FALLBACK_PREFIX: &str = "error: ";
/// MCP payload key for scan reports.
const MCP_PAYLOAD_SCAN: &str = "scan";
/// MCP payload key for project initialization reports.
const MCP_PAYLOAD_INIT: &str = "init";
/// MCP payload key for structural worktree inventory.
const MCP_PAYLOAD_WORKTREES: &str = "worktrees";
/// MCP payload key for one worktree registration transition.
const MCP_PAYLOAD_WORKTREE: &str = "worktree";
/// MCP payload key for compatibility map reports.
const MCP_PAYLOAD_MAP: &str = "map";
/// MCP payload key for effective config reports.
const MCP_PAYLOAD_CONFIG: &str = "config";
/// MCP payload key for ignore policy reports.
const MCP_PAYLOAD_IGNORE: &str = "ignore";
/// MCP payload key for `.gitignore` initialization reports.
const MCP_PAYLOAD_GITIGNORE: &str = "gitignore";
/// MCP payload key for lint reports.
const MCP_PAYLOAD_LINT: &str = "lint";
/// MCP payload key for symbol-build reports.
const MCP_PAYLOAD_SYMBOLS_BUILD: &str = "symbols_build";
/// MCP payload key for detailed symbol-relation reports.
const MCP_PAYLOAD_SYMBOL_RELATIONS: &str = "symbol_relations";
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
/// MCP payload key for generated MCP config reports.
const MCP_PAYLOAD_MCP_CONFIG: &str = "mcp_config";
/// MCP payload key for next-step recommendation reports.
const MCP_PAYLOAD_NEXT: &str = "next";
/// MCP payload key for selected-project audit metadata on routed reads.
const MCP_PAYLOAD_SELECTED_PROJECT: &str = "selected_project";
/// MCP payload key for settings reports.
const MCP_PAYLOAD_SETTINGS: &str = "settings";
/// MCP payload key for agent startup briefs.
const MCP_PAYLOAD_SESSION_BRIEF: &str = "session_brief";
/// Default source status omitted from compact file-summary payloads.
const MCP_FILE_SOURCE_STATUS_LIVE: &str = "live-source";
/// MCP payload key for accepted background tasks.
const MCP_PAYLOAD_TASK_START: &str = "task_start";
/// MCP payload key for task status lookups.
const MCP_PAYLOAD_TASK_STATUS: &str = "task_status";
/// MCP payload key for task cancellation responses.
const MCP_PAYLOAD_TASK_CANCEL: &str = "task_cancel";
/// MCP session capability payload key.
const MCP_PAYLOAD_SESSION_CAPABILITIES: &str = "mcp_session";
/// Session-brief argument key for per-call project roots.
const MCP_BRIEF_ARG_PROJECT_PATH: &str = "project_path";
/// Session-brief argument key for registered worktree aliases.
const MCP_BRIEF_ARG_WORKTREE: &str = "worktree";
/// Session-brief argument key for exact repository-relative files.
const MCP_BRIEF_ARG_FILE: &str = "file";
/// Session-brief argument key for indexed search patterns.
const MCP_BRIEF_ARG_PATTERN: &str = "pattern";
/// Session-brief argument key for relation view selection.
const MCP_BRIEF_ARG_VIEW: &str = "view";
/// Session-brief argument key for row limits.
const MCP_BRIEF_ARG_LIMIT: &str = "limit";
/// Session-brief argument key for host-owned purpose-curation tasks.
const MCP_BRIEF_ARG_TASK: &str = "task";
/// Compact-response argument key used by typed startup recommendations.
const MCP_BRIEF_ARG_COMPACT: &str = "compact";
/// Session-brief recommendation target for normal filesystem reads.
const MCP_BRIEF_TARGET_FILESYSTEM_TOOLS: &str = "filesystem_tools";
/// Session-brief reason for missing selected indexes.
const MCP_BRIEF_REASON_SELECTED_INDEX_MISSING: &str = "selected_index_missing";
/// Session-brief reason for filesystem fallback before an index exists.
const MCP_BRIEF_REASON_FILESYSTEM_UNTIL_INDEX: &str =
    "use_filesystem_until_projectatlas_index_exists";
/// Session-brief reason for following the selected file into its summary.
const MCP_BRIEF_REASON_RANKED_FILE_SUMMARY: &str = "ranked_file_ready_for_summary";
/// Session-brief reason for following truncated graph evidence into detailed relations.
const MCP_BRIEF_REASON_RANKED_FILE_RELATIONS: &str = "ranked_file_ready_for_relations";
/// Session-brief reason for searching when ranking found no directly navigable file.
const MCP_BRIEF_REASON_SEARCH_FALLBACK: &str = "no_ranked_file_candidate_search_index";
/// Session-brief reason for normal filesystem orientation in an indexed empty project.
const MCP_BRIEF_REASON_NO_FILE_CANDIDATE: &str = "no_ranked_file_candidate";
/// Session-brief reason for health follow-up.
const MCP_BRIEF_REASON_HEALTH_BLOCKERS: &str = "unresolved_health_blockers_present";
/// Session-brief reason for following an actionable purpose-curator handoff.
const MCP_BRIEF_REASON_PURPOSE_QUEUE: &str = "purpose_queue_ready";
/// Built-in task-progress contract message.
const MCP_TASK_PROGRESS_CONTRACT_MESSAGE: &str = "task progress contract available";
/// MCP telemetry event for overview calls.
const MCP_EVENT_ATLAS_OVERVIEW: &str = "mcp.atlas_overview";
/// MCP telemetry event for folder calls.
const MCP_EVENT_ATLAS_FOLDERS: &str = "mcp.atlas_folders";
/// MCP telemetry event for file calls.
const MCP_EVENT_ATLAS_FILES: &str = "mcp.atlas_files";
/// MCP telemetry event for next-step recommendation calls.
const MCP_EVENT_ATLAS_NEXT: &str = "mcp.atlas_next";
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
/// Compatible MCP symbol-relation view name.
const MCP_SYMBOL_RELATION_VIEW_LEGACY: &str = "legacy";
/// Additive detailed MCP symbol-relation view name.
const MCP_SYMBOL_RELATION_VIEW_DETAILED: &str = "detailed";
/// Additive closed analysis view on the existing relation route.
const MCP_SYMBOL_RELATION_VIEW_ANALYSIS: &str = "analysis";
/// Default closed relation-analysis mode.
const MCP_RELATION_ANALYSIS_MODE_ARCHITECTURE: &str = "architecture";
/// VCS impact relation-analysis mode.
const MCP_RELATION_ANALYSIS_MODE_IMPACT: &str = "impact";
/// Static trace relation-analysis mode.
const MCP_RELATION_ANALYSIS_MODE_TRACE: &str = "trace";
/// Default working-tree VCS impact selection.
const MCP_RELATION_ANALYSIS_VCS_WORKING_TREE: &str = "working_tree";
/// Staged-index VCS impact selection.
const MCP_RELATION_ANALYSIS_VCS_INDEX: &str = "index";
/// Explicit revision-range VCS impact selection.
const MCP_RELATION_ANALYSIS_VCS_REVISION_RANGE: &str = "revision_range";
/// Default direction for detailed MCP symbol-relation requests.
const MCP_SYMBOL_RELATION_DIRECTION_DEFAULT: &str = "outbound";
/// Default minimum confidence for detailed MCP symbol-relation requests.
const MCP_SYMBOL_RELATION_CONFIDENCE_DEFAULT: &str = "low";
/// Default resolution filter for detailed MCP symbol-relation requests.
const MCP_SYMBOL_RELATION_RESOLUTION_DEFAULT: &str = "any";
/// MCP validation error for an unsupported relation view.
const MCP_ERROR_SYMBOL_RELATION_VIEW: &str = "unsupported symbol relation view";
/// MCP validation error for legacy query fields in detailed requests.
const MCP_ERROR_DETAILED_RELATION_QUERY: &str =
    "detailed symbol relations use exact symbol selectors, not query";
/// MCP validation error for a detailed request without a file anchor.
const MCP_ERROR_DETAILED_RELATION_FILE: &str = "detailed symbol relations require file";
/// MCP validation error for an empty detailed symbol anchor.
const MCP_ERROR_DETAILED_RELATION_SYMBOL: &str = "detailed relation symbol must not be empty";
/// MCP validation error for symbol disambiguators without a symbol.
const MCP_ERROR_DETAILED_RELATION_DISAMBIGUATOR: &str = "symbol disambiguators require symbol";
/// MCP validation error for a relation limit outside the service range.
const MCP_ERROR_DETAILED_RELATION_LIMIT: &str = "detailed relation limit exceeds the u32 range";
/// MCP validation error for compact projection on a non-detailed relation view.
const MCP_ERROR_COMPACT_DETAILED_RELATION_VIEW: &str =
    "compact symbol relations require view=detailed";
/// MCP validation error for classified traversal on the legacy relation view.
const MCP_ERROR_CONTENT_SELECTION_RELATION_VIEW: &str =
    "content_selection requires view=detailed or view=analysis";
/// MCP validation error for analysis controls on another relation view.
const MCP_ERROR_ANALYSIS_VIEW_REQUIRED: &str = "analysis controls require view=analysis";
/// MCP validation error for federation on the legacy relation view.
const MCP_ERROR_FEDERATED_RELATION_VIEW: &str =
    "roots or worktrees require the detailed or analysis relation view";
/// MCP validation error for two federation selector families in one request.
const MCP_ERROR_FEDERATED_SELECTOR_CONFLICT: &str =
    "roots and worktrees are mutually exclusive federation selectors";
/// MCP validation error for combining alias federation with a legacy root path.
const MCP_ERROR_FEDERATED_PROJECT_PATH_CONFLICT: &str =
    "worktrees federation cannot be combined with project_path";
/// MCP validation error for a mismatched explicit primary alias.
const MCP_ERROR_FEDERATED_PRIMARY_CONFLICT: &str =
    "worktree must match the first ordered worktrees alias";
/// MCP validation error for a symbol trace without a symbol-kind selector.
const MCP_ERROR_TRACE_TARGET_KIND_REQUIRED: &str = "symbol trace targets require trace_target_kind";
/// MCP validation error for a symbol trace without a signature selector.
const MCP_ERROR_TRACE_TARGET_SIGNATURE_REQUIRED: &str =
    "symbol trace targets require trace_target_signature";
/// MCP validation error for symbol disambiguators without a trace symbol.
const MCP_ERROR_TRACE_TARGET_REQUIRED: &str =
    "trace target symbol disambiguators require trace_target";
/// MCP validation error for a trace target without its owning file.
const MCP_ERROR_TRACE_TARGET_FILE_REQUIRED: &str = "trace_target requires trace_target_file";
/// MCP validation error for revision fields outside revision-range selection.
const MCP_ERROR_VCS_REVISION_FIELDS: &str = "vcs_base and vcs_head require vcs=revision_range";
/// MCP validation error for a revision-range selection without its base.
const MCP_ERROR_VCS_BASE_REQUIRED: &str = "vcs=revision_range requires vcs_base";
/// MCP validation error for a revision-range selection without its head.
const MCP_ERROR_VCS_HEAD_REQUIRED: &str = "vcs=revision_range requires vcs_head";
/// MCP validation error for an unsupported VCS impact selector.
const MCP_ERROR_UNSUPPORTED_ANALYSIS_VCS: &str = "unsupported analysis VCS selection";
/// MCP validation error for an unsupported closed relation-analysis mode.
const MCP_ERROR_UNSUPPORTED_ANALYSIS_MODE: &str = "unsupported relation analysis mode";
/// MCP telemetry event for health calls.
const MCP_EVENT_ATLAS_HEALTH: &str = "mcp.atlas_health";
/// MCP telemetry event for purpose-queue calls.
const MCP_EVENT_ATLAS_PURPOSE_QUEUE: &str = "mcp.atlas_purpose_queue";
/// MCP ignore kind token for directory names.
const MCP_IGNORE_KIND_DIR_NAME: &str = "dir-name";
/// MCP alternate ignore kind token for directory names.
const MCP_IGNORE_KIND_DIR_NAME_ALIAS: &str = "dir_name";
/// MCP ignore kind token for path prefixes.
const MCP_IGNORE_KIND_PATH_PREFIX: &str = "path-prefix";
/// MCP alternate ignore kind token for path prefixes.
const MCP_IGNORE_KIND_PATH_PREFIX_ALIAS: &str = "path_prefix";
/// MCP purpose lint level token for low strictness.
const MCP_PURPOSE_LEVEL_LOW: &str = "low";
/// MCP purpose lint level token for medium strictness.
const MCP_PURPOSE_LEVEL_MEDIUM: &str = "medium";
/// MCP purpose lint level token for strict mode.
const MCP_PURPOSE_LEVEL_STRICT: &str = "strict";
/// Default purpose-curation task used by session startup briefs.
const MCP_PURPOSE_TASK_SESSION_STARTUP: &str = "session-startup";
/// Default purpose-curation task used by direct queue requests.
const MCP_PURPOSE_TASK_QUEUE: &str = "purpose-curation";
/// MCP harness token for standard MCP JSON config.
const MCP_HARNESS_MCP_JSON: &str = "mcp-json";
/// MCP alternate harness token for standard MCP JSON config.
const MCP_HARNESS_MCP_JSON_ALIAS: &str = "mcp_json";
/// MCP harness token for Codex config.
const MCP_HARNESS_CODEX: &str = "codex";
/// MCP harness token for Claude Code config.
const MCP_HARNESS_CLAUDE_CODE: &str = "claude-code";
/// MCP alternate harness token for Claude Code config.
const MCP_HARNESS_CLAUDE_CODE_ALIAS: &str = "claude_code";
/// MCP harness token for `OpenCode` config.
const MCP_HARNESS_OPENCODE: &str = "opencode";
/// Required ignore kind diagnostic.
const MCP_ERROR_IGNORE_KIND_REQUIRED: &str =
    "ignore kind is required; expected dir-name or path-prefix";
/// Required ignore kind diagnostic for mutation tools.
const MCP_ERROR_IGNORE_KIND_REQUIRED_FOR_ADD: &str = "ignore kind is required for atlas_ignore_add";
/// Error for mutually exclusive exact-root and structural-control selection.
const MCP_ERROR_ROOT_CONTROL_CONFLICT: &str =
    "control_root cannot be combined with project_path or verify";
/// Invalid coverage start-index diagnostic prefix.
const MCP_ERROR_COVERAGE_START_INDEX_TOO_LARGE_PREFIX: &str = "coverage start index is too large: ";
/// Invalid coverage limit diagnostic prefix.
const MCP_ERROR_COVERAGE_LIMIT_TOO_LARGE_PREFIX: &str = "coverage limit is too large: ";
/// Coverage-filter diagnostic for the default structural-health mode.
const MCP_ERROR_COVERAGE_FILTERS_REQUIRE_COVERAGE: &str = "coverage filters require coverage=true";
/// CI environment variable used by MCP map export safeguards.
const MCP_ENV_CI: &str = "CI";
/// GitHub Actions environment variable used by MCP map export safeguards.
const MCP_ENV_GITHUB_ACTIONS: &str = "GITHUB_ACTIONS";
/// Compatibility map skip reason in CI.
const MCP_MAP_SKIPPED_IN_CI_REASON: &str =
    "skipped in CI; pass force=true to write the compatibility map";
/// Placeholder when no routed root is available for a diagnostic.
const MCP_NO_ROOT_PLACEHOLDER: &str = "none";
/// Invalid ignore kind diagnostic prefix.
const MCP_ERROR_INVALID_IGNORE_KIND_PREFIX: &str = "invalid ignore kind '";
/// Invalid ignore kind diagnostic suffix.
const MCP_ERROR_INVALID_IGNORE_KIND_SUFFIX: &str = "'; expected dir-name or path-prefix";
/// Invalid purpose lint level diagnostic prefix.
const MCP_ERROR_INVALID_PURPOSE_LEVEL_PREFIX: &str = "invalid purpose_level '";
/// Invalid purpose lint level diagnostic suffix.
const MCP_ERROR_INVALID_PURPOSE_LEVEL_SUFFIX: &str = "'; expected low, medium, or strict";
/// Invalid harness diagnostic prefix.
const MCP_ERROR_INVALID_HARNESS_PREFIX: &str = "invalid harness '";
/// Invalid harness diagnostic suffix.
const MCP_ERROR_INVALID_HARNESS_SUFFIX: &str =
    "'; expected mcp-json, codex, claude-code, or opencode";
/// Ambiguous route diagnostic fragment.
const MCP_ERROR_FOR_PATH_FRAGMENT: &str = " for '";
/// Ambiguous route diagnostic fragment.
const MCP_ERROR_LEXICAL_ROOT_FRAGMENT: &str = "'; lexical root: '";
/// Ambiguous route diagnostic fragment.
const MCP_ERROR_RESOLVED_ROOT_FRAGMENT: &str = "'; resolved root: '";
/// Ambiguous route diagnostic fragment.
const MCP_ERROR_GUIDANCE_FRAGMENT: &str = "'; ";
/// Node payload label for rendered folder rows.
const NODE_LABEL_FOLDERS: &str = "folders";
/// Node payload label for rendered file rows.
const NODE_LABEL_FILES: &str = "files";
/// Stable MCP payload key for classified symbol rows.
const NODE_LABEL_SYMBOLS: &str = "symbols";
/// Error when a symbol disambiguator is supplied without a symbol name.
const SYMBOL_DISAMBIGUATOR_WITHOUT_SYMBOL_ERROR: &str = "symbol disambiguators require symbol";
/// Error when a line slice omits its start line.
const START_LINE_REQUIRED_ERROR: &str = "start_line is required unless symbol is provided";
/// Error when an absolute MCP file path has no valid indexed project ancestor.
const PATH_NOT_INSIDE_INDEXED_PROJECT_ERROR: &str =
    "path is not inside an indexed ProjectAtlas project";
/// Error when an absolute MCP folder path has no valid indexed project ancestor.
const FOLDER_NOT_INSIDE_INDEXED_PROJECT_ERROR: &str =
    "folder is not inside an indexed ProjectAtlas project";
/// Error when lexical and canonical path routing disagree.
const AMBIGUOUS_NEAREST_PROJECT_PATH_ERROR: &str = "absolute MCP path resolves through a symlink or junction with multiple plausible ProjectAtlas roots";
/// Separator for diagnostic lists of accepted severity names.
const SEVERITY_EXPECTED_SEPARATOR: &str = ", ";
/// Final separator for diagnostic lists of accepted severity names.
const SEVERITY_EXPECTED_FINAL_SEPARATOR: &str = ", or ";
/// Token trend validation error suffix.
const TOKEN_TREND_WINDOW_ERROR_SUFFIX: &str = "expected day, week, month, or year";
/// Validation error for benchmark evidence on token trend requests.
const TOKEN_TREND_BENCHMARK_ERROR: &str =
    "benchmark_results is only supported for token overview reports";
/// Internal mismatch when a trend request returns the overview variant.
const TOKEN_TRENDS_RESULT_VARIANT_MISMATCH: &str = "token trend request returned an overview";
/// Internal mismatch when an overview request returns the trends variant.
const TOKEN_OVERVIEW_RESULT_VARIANT_MISMATCH: &str = "token overview request returned trends";
/// Token chart theme validation error prefix.
const TOKEN_CHART_THEME_ERROR_PREFIX: &str = "unsupported token chart theme ";
/// Token chart theme validation error suffix.
const TOKEN_CHART_THEME_ERROR_SUFFIX: &str = "; expected dark or light";
/// Watch-status recommendation when no index exists.
const WATCH_STATUS_SCAN_RECOMMENDATION: &str =
    " Run `atlas_scan` first when no ProjectAtlas index exists for this project.";
/// Default number of rows in an agent startup brief section.
const SESSION_BRIEF_DEFAULT_LIMIT: usize = 5;
/// Default number of rows in an explicitly compact startup brief section.
const COMPACT_SESSION_BRIEF_DEFAULT_LIMIT: usize = 3;
/// Maximum number of rows in an agent startup brief section.
const SESSION_BRIEF_MAX_LIMIT: usize = 8;
/// Bounded MCP task registry capacity.
const MCP_TASK_REGISTRY_CAPACITY: usize = 32;
/// Maximum concise task failure text retained in the session registry.
const MCP_TASK_ERROR_MAX_CHARS: usize = 512;
/// Built-in task id that exposes the task-progress contract itself.
const MCP_TASK_CONTRACT_ID: &str = "task-progress-contract";
/// Task-registry synchronization failure diagnostic.
const MCP_TASK_REGISTRY_LOCK_POISONED: &str = "MCP task registry lock is poisoned";
/// Prefix for generated background index task identifiers.
const MCP_INDEX_TASK_ID_PREFIX: &str = "index-";
/// Prefix for named background index worker threads.
const MCP_INDEX_WORKER_NAME_PREFIX: &str = "projectatlas-";
/// Prefix for active background task limit diagnostics.
const MCP_INDEX_TASK_LIMIT_PREFIX: &str = "background indexing task limit ";
/// Suffix for active background task limit diagnostics.
const MCP_INDEX_TASK_LIMIT_SUFFIX: &str = " is already active";
/// Maximum background indexing tasks admitted by one MCP server session.
const MCP_BACKGROUND_TASK_SAFE_CEILING: usize = 4;
/// Terminal diagnostic when a background worker panics.
const MCP_INDEX_WORKER_PANIC_ERROR: &str = "background indexing worker panicked";
/// Prefix for background worker spawn failures.
const MCP_INDEX_WORKER_SPAWN_ERROR_PREFIX: &str = "failed to start background indexing: ";
/// Progress message recorded after task admission.
const MCP_TASK_PROGRESS_ACCEPTED: &str = "accepted";
/// Progress message recorded while task work is active.
const MCP_TASK_PROGRESS_RUNNING: &str = "running";
/// Progress message recorded after successful task completion.
const MCP_TASK_PROGRESS_COMPLETE: &str = "complete";
/// Progress message recorded after task failure.
const MCP_TASK_PROGRESS_FAILED: &str = "failed";
/// Progress message recorded after cooperative cancellation completes.
const MCP_TASK_PROGRESS_CANCELED: &str = "canceled";
/// Progress message recorded after cancellation is requested.
const MCP_TASK_PROGRESS_CANCELLATION_REQUESTED: &str = "cancellation_requested";
/// Agent-facing MCP server instructions.
const MCP_SERVER_INSTRUCTIONS: &str = "ProjectAtlas provides TOON-first repository orientation, folder/file ranking, structured file summaries, symbol graph lookup, exact slices, health checks, and token telemetry for coding agents.";
/// Root-scoped selector conflict returned before filesystem or database access.
const MCP_WORKTREE_PROJECT_PATH_CONFLICT: &str =
    "worktree and project_path are mutually exclusive; choose one target selector";
/// Reserved alias for the immutable MCP control authority.
const MCP_MAIN_WORKTREE_ALIAS: &str = "main";
/// Maximum combined structural and unmatched registered rows in one MCP response.
const MCP_WORKTREE_LIST_MAX_ROWS: usize = (MAX_GIT_WORKTREE_REGISTRATIONS * 2) + 1;
/// Stable prefix for structural worktree candidates returned to agents.
const MCP_WORKTREE_SELECTOR_PREFIX: &str = "wt-";
/// Hex characters retained from the administrative-path digest.
const MCP_WORKTREE_SELECTOR_DIGEST_CHARS: usize = 16;
/// Init-owned non-source metadata file copied or removed during hydration fallback.
const MCP_NONSOURCE_FILE_NAME: &str = "projectatlas-nonsource-files.toon";
/// Typed fallback when a caller explicitly suppresses the reconciliation scan.
const MCP_HYDRATION_NO_SCAN_REASON: &str =
    "hydration requires source reconciliation; ordinary no-scan init was requested";
/// Worktree aliases require one structurally verified Git control repository.
const MCP_ERROR_WORKTREE_CONTROL_REPOSITORY_REQUIRED: &str =
    "worktree aliases require a structurally valid Git control repository";
/// `SQLite` metadata and MCP JSON require lossless UTF-8 worktree identity paths.
const MCP_ERROR_WORKTREE_PATH_NON_UTF8: &str =
    "ProjectAtlas worktree registration requires UTF-8 common, administrative, and source paths";
/// Active registrations must agree with their local atlas identity.
const MCP_ERROR_WORKTREE_IDENTITY_CONFLICT: &str =
    "local atlas identity conflicts with its active registration";
/// Deferred alias work must retain the exact control catalog that owns its registration ID.
const MCP_ERROR_WORKTREE_CONTROL_IDENTITY_CONFLICT: &str =
    "control atlas identity changed after worktree alias selection";
/// Recovery guidance when an initialized registration loses its exact atlas.
const MCP_ERROR_BOUND_WORKTREE_ATLAS_MISSING: &str = "registered worktree atlas is missing; restore it before retrying or retiring the alias so final token totals can be synchronized";
/// Resetting a bound atlas would strand its control-catalog identity.
const MCP_ERROR_BOUND_WORKTREE_RESET_UNSUPPORTED: &str = "bound worktree atlas cannot be reset in place; retire the alias to synchronize final totals, then register and initialize it again";
/// A reused administrative path cannot inherit an earlier registration.
const MCP_ERROR_WORKTREE_LIFECYCLE_CHANGED: &str =
    "registered worktree administrative lifecycle changed; unregister and register it again";
/// Federation must retain the exact alias captured during target resolution.
const MCP_ERROR_FEDERATED_ALIAS_MISSING: &str =
    "federated worktree resolution lost its captured alias";
/// Federation cannot address the same alias or root twice.
const MCP_ERROR_FEDERATED_TARGET_DUPLICATE: &str =
    "federated worktree aliases and roots must be unique";
/// The immutable control checkout is addressed only through its reserved alias.
const MCP_ERROR_CONTROL_ALIAS_REQUIRED: &str =
    "the control checkout is selected through reserved alias main";
/// Worktree registration requires one non-empty structural selector.
const MCP_ERROR_WORKTREE_SELECTOR_EMPTY: &str = "worktree selector must not be empty";
/// Structural evidence can disappear between selection and registration.
const MCP_ERROR_WORKTREE_NO_LONGER_ACTIVE: &str = "selected worktree is no longer active";
/// Retiring a missing registration preserves its last accepted aggregate.
const MCP_WORKTREE_MISSING_RETENTION_REASON: &str =
    "worktree is structurally missing; the last accepted telemetry total is retained";

/// Optional active-project override accepted by MCP tools.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasProjectParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
}

/// MCP parameter payload for compact agent startup briefs.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasSessionBriefParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Optional task query used for folder and file ranking.
    query: Option<String>,
    /// Optional host-owned task label for the purpose-curator handoff.
    purpose_task: Option<String>,
    /// Return the additive compact startup projection when true.
    compact: Option<bool>,
    /// Maximum folder candidates to return.
    folder_limit: Option<usize>,
    /// Maximum file candidates to return.
    file_limit: Option<usize>,
    /// Maximum health blockers to return.
    blocker_limit: Option<usize>,
    /// Maximum actionable low-scope purpose rows in the startup handoff.
    purpose_limit: Option<usize>,
}

/// MCP parameter payload for task-progress tools.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasTaskParams {
    /// Opaque MCP-session-local task id.
    task_id: String,
}

/// MCP parameter payload for selecting the active project.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasSetProjectPathParams {
    /// Project root to make active for calls that omit `project_path`.
    project_path: String,
}

/// MCP parameter payload for bounded structural worktree inventory.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasWorktreeListParams {
    /// Include retired `ProjectAtlas` registrations after active structural rows.
    include_retired: Option<bool>,
}

/// MCP parameter payload for registering one structurally discovered worktree.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasWorktreeAddParams {
    /// Stable or uniquely matching short selector returned by `atlas_worktree_list`.
    worktree: String,
    /// Optional short alias; defaults to the selected worktree directory name when valid.
    alias: Option<String>,
}

/// MCP parameter payload for retiring one active `ProjectAtlas` worktree alias.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasWorktreeRemoveParams {
    /// Active registered alias to remove from `ProjectAtlas` selection.
    worktree: String,
}

/// MCP parameter payload for initializing a project.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasInitParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Create/verify the project surface without running the scan/index pipeline.
    no_scan: Option<bool>,
    /// Run the scan/index phase even when a future freshness check could skip it.
    force_rescan: Option<bool>,
    /// Maximum UTF-8 file size persisted into `SQLite` text search during the init scan.
    text_index_max_bytes: Option<u64>,
}

/// MCP parameter payload for compatibility map exports.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasMapParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Write JSON compatibility content when true.
    json: Option<bool>,
    /// Force map generation even in CI-like environments.
    force: Option<bool>,
}

/// MCP parameter payload for root diagnostics.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasRootParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Optional checkout or Git common directory for mutation-free worktree status.
    control_root: Option<String>,
    /// Return the same report shape with `verified` available for gating.
    verify: Option<bool>,
}

/// MCP parameter payload for binding a root.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasRootSetParams {
    /// Project root to bind and make active for later calls.
    root: String,
    /// Explicit durable transition. Omitted requests retain bind behavior.
    transition: Option<RootTransition>,
    /// Include mcp --nearest-project in generated project-local MCP configs.
    nearest_project: Option<bool>,
}

/// MCP parameter payload for ignore mutations.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasIgnoreMutationParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Ignore kind: dir-name or path-prefix. Omit only for broad remove.
    kind: Option<String>,
    /// Directory name or repository-relative path prefix.
    value: String,
}

/// MCP parameter payload for lint reports.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasLintParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Deprecated compatibility flag matching the CLI.
    strict_folders: Option<bool>,
    /// Purpose lint strictness: low, medium, or strict.
    purpose_level: Option<String>,
    /// Include untracked-file report.
    report_untracked: Option<bool>,
    /// Make untracked-file findings fail lint.
    strict_untracked: Option<bool>,
}

/// MCP parameter payload for generated MCP config documents.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasMcpConfigParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// MCP server name to emit. Defaults to projectatlas.
    server_name: Option<String>,
    /// Harness config shape: mcp-json, codex, claude-code, or opencode.
    harness: Option<String>,
    /// Include mcp --nearest-project in generated startup args.
    nearest_project: Option<bool>,
}

/// Run the official RMCP stdio server.
pub(crate) fn run_mcp_server(
    db_path: PathBuf,
    config_path: Option<PathBuf>,
    session: String,
    allow_nearest_project: bool,
) -> Result<(), CliError> {
    let server = ProjectAtlasMcpServer::new(db_path, config_path, session, allow_nearest_project);
    let shutdown_server = server.clone();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|source| CliError::Mcp(source.to_string()))?;
    let result = runtime.block_on(async move {
        server
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|source| CliError::Mcp(source.to_string()))?
            .waiting()
            .await
            .map_err(|source| CliError::Mcp(source.to_string()))
            .map(|_| ())
    });
    shutdown_server.seal_usage_instances_for_projects();
    result
}

/// Return whether the generated RMCP router contains required tool families.
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
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Repository root path. Defaults to the configured or indexed project root.
    path: Option<String>,
    /// Opt in to nearest indexed `ProjectAtlas` project discovery for absolute paths.
    nearest_project: Option<bool>,
    /// Maximum file size to parse for symbols.
    max_bytes: Option<u64>,
    /// Maximum parser worker threads.
    max_workers: Option<usize>,
    /// Stop starting parser work after this many seconds.
    timeout_seconds: Option<u64>,
    /// Maximum UTF-8 file size persisted into `SQLite` text search.
    text_index_max_bytes: Option<u64>,
    /// Run indexing in the bounded session task registry and return immediately.
    background: Option<bool>,
}

/// MCP parameter payload for one-shot watcher refresh.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasWatchOnceParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Repository root path. Defaults to the configured or indexed project root.
    path: Option<String>,
    /// Opt in to nearest indexed `ProjectAtlas` project discovery for absolute paths.
    nearest_project: Option<bool>,
    /// Maximum parser worker threads.
    max_workers: Option<usize>,
    /// Stop starting parser work after this many seconds.
    timeout_seconds: Option<u64>,
    /// Maximum UTF-8 file size persisted into `SQLite` text search.
    text_index_max_bytes: Option<u64>,
    /// Run indexing in the bounded session task registry and return immediately.
    background: Option<bool>,
}

/// MCP parameter payload for ranked node lookup.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasQueryParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Search query for path and purpose matching.
    query: Option<String>,
    /// Maximum number of rows to return.
    limit: Option<usize>,
}

/// MCP parameter payload for classified next-step recommendations.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasNextParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Search query for path and purpose matching.
    query: Option<String>,
    /// Optional classified-content selection: source, documentation, or both.
    content_selection: Option<String>,
    /// Maximum number of rows to return.
    limit: Option<usize>,
}

/// MCP parameter payload for ranked file lookup with optional absolute folder routing.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasFilesParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Search query for path and purpose matching.
    query: Option<String>,
    /// Folder path to constrain file lookup.
    folder: Option<String>,
    /// Opt in to nearest indexed `ProjectAtlas` project discovery for absolute folder paths.
    nearest_project: Option<bool>,
    /// Optional repository-relative glob filter.
    file_pattern: Option<String>,
    /// Include indexed file text as a bounded fallback ranking signal.
    include_content: Option<bool>,
    /// Optional classified-content selection: source, documentation, or both.
    content_selection: Option<String>,
    /// Maximum number of rows to return.
    limit: Option<usize>,
}

/// MCP parameter payload for outlining a file.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasOutlineParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Repository-relative file path.
    file: String,
    /// Opt in to nearest indexed `ProjectAtlas` project discovery for absolute file paths.
    nearest_project: Option<bool>,
    /// Number of non-empty preview lines to include.
    lines: Option<usize>,
}

/// MCP parameter payload for deterministic file summaries.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasFileSummaryParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Repository-relative file path.
    file: String,
    /// Opt in to nearest indexed `ProjectAtlas` project discovery for absolute file paths.
    nearest_project: Option<bool>,
    /// Return the additive compact summary projection when true.
    compact: Option<bool>,
    /// Optional classified-content selection: source, documentation, or both.
    content_selection: Option<String>,
    /// Maximum rows per functions/methods/classes/types/calls section.
    limit: Option<usize>,
}

/// MCP parameter payload for text search.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasSearchParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Literal, regex, or fuzzy pattern to search for.
    pattern: String,
    /// Retrieval family; lexical is the default and always-available mode.
    retrieval_mode: Option<SearchRetrievalModeArg>,
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
    /// Optional classified-content selection: source, documentation, or both.
    content_selection: Option<String>,
    /// Maximum matches to return.
    limit: Option<usize>,
}

/// MCP parameter payload for exact source slices.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasSliceParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Repository-relative file path.
    file: String,
    /// Opt in to nearest indexed `ProjectAtlas` project discovery for absolute file paths.
    nearest_project: Option<bool>,
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
    /// Optional exact signature for disambiguating `symbol`.
    symbol_signature: Option<String>,
    /// Optional source line for disambiguating `symbol`.
    symbol_line: Option<usize>,
    /// Optional classified-content selection: source, documentation, or both.
    content_selection: Option<String>,
    /// Maximum encoded bytes admitted to the slice response.
    output_bytes: Option<u32>,
}

/// MCP parameter payload for symbol and relation lookup.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasSymbolsParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Optional repository-relative file path.
    file: Option<String>,
    /// Opt in to nearest indexed `ProjectAtlas` project discovery for absolute file paths.
    nearest_project: Option<bool>,
    /// Optional symbol, signature, relation, or path query.
    query: Option<String>,
    /// Optional classified-content selection: source, documentation, or both.
    content_selection: Option<String>,
    /// Maximum rows to return.
    limit: Option<usize>,
}

/// MCP parameters for legacy, detailed, or closed-analysis relation navigation.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct AtlasSymbolRelationsParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Optional repository-relative file path.
    file: Option<String>,
    /// Opt in to nearest indexed `ProjectAtlas` project discovery for absolute file paths.
    nearest_project: Option<bool>,
    /// Optional symbol, signature, relation, or path query.
    query: Option<String>,
    /// Preserve `legacy`, opt in to `detailed`, or select closed `analysis`.
    view: Option<String>,
    /// Optional classified-content selection: source, documentation, or both.
    content_selection: Option<String>,
    /// Return the opt-in compact detailed projection while preserving exact selectors and trust.
    compact: Option<bool>,
    /// Resume one exact generation- and purpose-bound detailed page.
    cursor: Option<String>,
    /// Complete ordered project-root set for one read-only federated call.
    roots: Option<Vec<String>>,
    /// Complete ordered registered worktree aliases for one read-only federated call.
    worktrees: Option<Vec<String>>,
    /// Exact symbol name used as the detailed anchor; omit for a file anchor.
    symbol: Option<String>,
    /// Optional exact parent used to disambiguate the detailed symbol anchor.
    symbol_parent: Option<String>,
    /// Optional exact kind used to disambiguate the detailed symbol anchor.
    symbol_kind: Option<String>,
    /// Optional exact signature used to disambiguate the detailed symbol anchor.
    symbol_signature: Option<String>,
    /// Detailed traversal direction: `outbound` or `inbound`.
    direction: Option<String>,
    /// Optional exact legacy or extended relation family.
    relation: Option<String>,
    /// Detailed confidence floor: `exact`, `high`, `medium`, or `low`.
    minimum_confidence: Option<String>,
    /// Detailed resolution filter.
    resolution: Option<String>,
    /// Maximum detailed traversal depth.
    depth: Option<u32>,
    /// Retain bounded exact source occurrences in detailed rows.
    include_occurrences: Option<bool>,
    /// Maximum exact occurrences retained per detailed relation.
    occurrence_limit: Option<u32>,
    /// Maximum adjacency rows inspected by one detailed page.
    edge_limit: Option<u32>,
    /// Maximum unique traversal nodes retained across continuation state.
    node_limit: Option<u32>,
    /// Maximum unique visited-node state retained across continuation state.
    visited_limit: Option<u32>,
    /// Maximum exact occurrences retained across the complete page.
    occurrence_total_limit: Option<u32>,
    /// Maximum decoded, cursor, and service-composition intermediate bytes.
    intermediate_bytes: Option<u64>,
    /// Maximum service-owned elapsed milliseconds.
    deadline_ms: Option<u64>,
    /// Maximum encoded bytes admitted to the detailed response.
    output_bytes: Option<u32>,
    /// Closed analysis mode: `architecture`, `impact`, or `trace`.
    analysis_mode: Option<String>,
    /// Exact target symbol name for trace mode.
    trace_target: Option<String>,
    /// Exact target file for trace mode; alone selects a file target.
    trace_target_file: Option<String>,
    /// Exact target parent for symbol trace mode.
    trace_target_parent: Option<String>,
    /// Exact target kind required by symbol trace mode.
    trace_target_kind: Option<String>,
    /// Exact target signature required by symbol trace mode.
    trace_target_signature: Option<String>,
    /// Impact VCS scope: `working_tree`, `index`, or `revision_range`.
    vcs: Option<String>,
    /// Older Git revision used by `revision_range`.
    vcs_base: Option<String>,
    /// Newer Git revision used by `revision_range`.
    vcs_head: Option<String>,
    /// Include communities with containment excluded.
    include_communities: Option<bool>,
    /// Include dependency SCC findings.
    include_cycles: Option<bool>,
    /// Include conservative dead-code candidates.
    include_dead_code: Option<bool>,
    /// Maximum rows to return.
    limit: Option<usize>,
}

/// Return whether any closed analysis-only control was supplied.
fn relation_analysis_controls_present(params: &AtlasSymbolRelationsParams) -> bool {
    params.analysis_mode.is_some()
        || params.trace_target.is_some()
        || params.trace_target_file.is_some()
        || params.trace_target_parent.is_some()
        || params.trace_target_kind.is_some()
        || params.trace_target_signature.is_some()
        || params.vcs.is_some()
        || params.vcs_base.is_some()
        || params.vcs_head.is_some()
        || params.include_communities.is_some()
        || params.include_cycles.is_some()
        || params.include_dead_code.is_some()
}

/// Decode and validate the optional exact trace target.
fn relation_analysis_trace_target(
    store: &AtlasStore,
    params: &AtlasSymbolRelationsParams,
) -> Result<Option<RelationAnchor>, CliError> {
    match (&params.trace_target, &params.trace_target_file) {
        (Some(name), Some(file)) => {
            let file = validated_indexed_file_key(store, Path::new(file))?;
            let kind = params.trace_target_kind.as_deref().ok_or_else(|| {
                CliError::Service(ServiceError::InvalidInput(
                    MCP_ERROR_TRACE_TARGET_KIND_REQUIRED.to_string(),
                ))
            })?;
            let signature = params.trace_target_signature.clone().ok_or_else(|| {
                CliError::Service(ServiceError::InvalidInput(
                    MCP_ERROR_TRACE_TARGET_SIGNATURE_REQUIRED.to_string(),
                ))
            })?;
            Ok(Some(RelationAnchor::Symbol {
                file: RepositoryFilePath::new(Path::new(&file)).map_err(|error| {
                    CliError::Service(ServiceError::InvalidInput(error.to_string()))
                })?,
                name: name.clone(),
                symbol_kind: Some(parse_symbol_kind(kind)?),
                parent: params.trace_target_parent.clone(),
                signature: Some(signature),
            }))
        }
        (None, Some(file)) => {
            if params.trace_target_parent.is_some()
                || params.trace_target_kind.is_some()
                || params.trace_target_signature.is_some()
            {
                return Err(CliError::Service(ServiceError::InvalidInput(
                    MCP_ERROR_TRACE_TARGET_REQUIRED.to_string(),
                )));
            }
            let file = validated_indexed_file_key(store, Path::new(file))?;
            Ok(Some(RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new(&file)).map_err(|error| {
                    CliError::Service(ServiceError::InvalidInput(error.to_string()))
                })?,
            }))
        }
        (Some(_), None) => Err(CliError::Service(ServiceError::InvalidInput(
            MCP_ERROR_TRACE_TARGET_FILE_REQUIRED.to_string(),
        ))),
        (None, None) => Ok(None),
    }
}

/// Decode and validate the optional VCS impact selector.
fn relation_analysis_vcs(
    params: &AtlasSymbolRelationsParams,
) -> Result<GitImpactSelection, CliError> {
    match params
        .vcs
        .as_deref()
        .unwrap_or(MCP_RELATION_ANALYSIS_VCS_WORKING_TREE)
    {
        MCP_RELATION_ANALYSIS_VCS_WORKING_TREE => {
            if params.vcs_base.is_some() || params.vcs_head.is_some() {
                return Err(CliError::Service(ServiceError::InvalidInput(
                    MCP_ERROR_VCS_REVISION_FIELDS.to_string(),
                )));
            }
            Ok(GitImpactSelection::WorkingTree)
        }
        MCP_RELATION_ANALYSIS_VCS_INDEX => {
            if params.vcs_base.is_some() || params.vcs_head.is_some() {
                return Err(CliError::Service(ServiceError::InvalidInput(
                    MCP_ERROR_VCS_REVISION_FIELDS.to_string(),
                )));
            }
            Ok(GitImpactSelection::Index)
        }
        MCP_RELATION_ANALYSIS_VCS_REVISION_RANGE => Ok(GitImpactSelection::RevisionRange {
            base: params.vcs_base.clone().ok_or_else(|| {
                CliError::Service(ServiceError::InvalidInput(
                    MCP_ERROR_VCS_BASE_REQUIRED.to_string(),
                ))
            })?,
            head: params.vcs_head.clone().ok_or_else(|| {
                CliError::Service(ServiceError::InvalidInput(
                    MCP_ERROR_VCS_HEAD_REQUIRED.to_string(),
                ))
            })?,
        }),
        _unsupported => Err(CliError::Service(ServiceError::InvalidInput(
            MCP_ERROR_UNSUPPORTED_ANALYSIS_VCS.to_string(),
        ))),
    }
}

/// MCP parameter payload for token savings reports.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasTokenParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Optional session id filter.
    session: Option<String>,
    /// Include a readable ASCII chart in the MCP result.
    include_chart: Option<bool>,
    /// Optional trend grouping window: day, week, month, or year.
    trend_window: Option<String>,
    /// Optional repository-relative agent-navigation benchmark result.
    benchmark_results: Option<String>,
    /// Optional chart theme for TUI output: dark or light.
    theme: Option<String>,
}

/// MCP parameter payload for bounded health finding lookup.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasHealthParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
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
    /// Opt in to bounded current coverage discovery instead of structural findings.
    coverage: Option<bool>,
    /// Optional source parser coverage filter.
    parser: Option<String>,
    /// Optional derived-fact provider coverage filter.
    provider: Option<String>,
    /// Optional relationship-family coverage filter.
    relation: Option<String>,
    /// Optional complete, partial, failed, ignored, oversized, quarantined, or stale filter.
    coverage_state: Option<String>,
    /// Optional exact coverage reason filter.
    reason: Option<String>,
}

/// MCP parameter payload for bounded task-scoped purpose curation.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasPurposeQueueParams {
    /// Shared health paging and purpose-scope filters.
    #[serde(flatten)]
    health: AtlasHealthParams,
    /// Host-owned task label for deterministic purpose-curator work identity.
    task: Option<String>,
}

/// MCP parameter payload for parity reports.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasParityParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Parity profile. Defaults to repository-intelligence.
    profile: Option<String>,
}

/// MCP parameter payload for legacy purpose cleanup.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasStripLegacyParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Repository root path. Defaults to the configured or indexed project root.
    path: Option<String>,
    /// Opt in to nearest indexed `ProjectAtlas` project discovery for absolute paths.
    nearest_project: Option<bool>,
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
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
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
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
    /// Indexed repository-relative path.
    path: String,
    /// Agent-approved purpose one-liner.
    purpose: String,
}

/// MCP payload for one batch purpose review item.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(inline)]
struct AtlasPurposeReviewItem {
    /// Indexed repository-relative path.
    path: String,
    /// Agent-reviewed purpose one-liner. Required for generated suggestions.
    purpose: Option<String>,
    /// Confirm the existing non-generated purpose after inspection.
    confirm_existing: Option<bool>,
    /// Queue task copied from the purpose-curation item.
    task: Option<String>,
    /// Queue work key copied from the purpose-curation item.
    work_key: Option<String>,
    /// Queue state token copied from the purpose-curation item.
    state_token: Option<String>,
}

/// MCP parameter payload for batch purpose review.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasPurposeReviewParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
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
    /// Optional registered worktree alias for this call. Mutually exclusive with `project_path`.
    worktree: Option<String>,
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
    /// Registered alias captured for this call, when alias routing selected the state.
    worktree: Option<McpWorktreeSelection>,
}

/// Stable control-catalog identity captured with one alias-routed project state.
#[derive(Debug, Clone, Eq, PartialEq)]
struct McpWorktreeSelection {
    /// Reserved `main` or registered short alias.
    alias: String,
    /// Durable registration identity; absent only for reserved `main`.
    registration_id: Option<i64>,
    /// Project identity captured for the selected alias target, when present.
    project_instance_id: Option<ProjectInstanceId>,
    /// Control-atlas identity that owns the captured registration ID.
    control_project_instance_id: Option<ProjectInstanceId>,
}

/// Outcome of evaluating the control atlas as an init hydration source.
enum McpWorktreeHydration {
    /// One target-exact candidate was reconciled and activated.
    Activated {
        /// Public init diagnostic.
        hydration: InitHydrationPhase,
        /// Normal scan result produced against the private candidate.
        scan: Box<ScanReport>,
    },
    /// Ordinary init must be used and disclose this reason.
    Fallback(String),
}

/// Store ownership selected by the existing relation tool request shape.
enum SymbolRelationStores<'a> {
    /// Compatibility-preserving selected-project query.
    Single(&'a AtlasStore),
    /// Explicit call-owned ordered read snapshots.
    Federated(Vec<FederatedStore>),
}

impl SymbolRelationStores<'_> {
    /// Borrow the selected first project while validating local selectors.
    fn primary(&self) -> &AtlasStore {
        match self {
            Self::Single(store) => store,
            Self::Federated(stores) => stores[0].store(),
        }
    }
}

/// Exact root/database/project identity where this MCP process recorded telemetry.
#[derive(Debug, Clone, Eq, Hash, PartialEq)]
struct McpUsageProjectBinding {
    /// Canonical selected repository root.
    root: PathBuf,
    /// Exact authoritative database used for the telemetry write.
    db_path: PathBuf,
    /// Project identity captured by the already-open selected store.
    project_instance_id: ProjectInstanceId,
    /// Captured routed origin; absent for native events and source baselines.
    worktree_registration_id: Option<i64>,
}

/// One bounded broad-source token baseline keyed to a complete generation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct McpSourceTokenBaselineKey {
    /// Exact project binding whose source files supplied the baseline.
    binding: McpUsageProjectBinding,
    /// Complete publication generation represented by the baseline.
    generation: projectatlas_core::IndexGeneration,
    /// Optional repository folder filter applied to the baseline.
    folder: Option<String>,
    /// Optional repository file-pattern filter applied to the baseline.
    file_pattern: Option<String>,
}

/// Deferred telemetry payload recorded only after a verified result is accepted.
#[derive(Debug)]
struct McpUsageIntent {
    /// Stable MCP event family.
    command: &'static str,
    /// Optional selected path or filter.
    path: Option<String>,
    /// Optional caller query.
    query: Option<String>,
    /// Baseline used by the existing usage accounting contract.
    baseline: McpUsageBaseline,
}

/// Existing telemetry baseline variants retained across verified-read acceptance.
#[derive(Debug)]
enum McpUsageBaseline {
    /// Modeled selected-candidate token count.
    Estimate(usize),
    /// Modeled avoided directory-walk token count.
    DirectoryWalk(usize),
    /// Exact source text replaced by the accepted response.
    Text(String),
}

impl McpUsageIntent {
    /// Defer one selected-candidate token estimate until result acceptance.
    fn estimate(
        command: &'static str,
        path: Option<String>,
        query: Option<String>,
        baseline_tokens: usize,
    ) -> Self {
        Self {
            command,
            path,
            query,
            baseline: McpUsageBaseline::Estimate(baseline_tokens),
        }
    }

    /// Defer one avoided directory-walk estimate until result acceptance.
    fn directory_walk(
        command: &'static str,
        path: Option<String>,
        query: Option<String>,
        baseline_tokens: usize,
    ) -> Self {
        Self {
            command,
            path,
            query,
            baseline: McpUsageBaseline::DirectoryWalk(baseline_tokens),
        }
    }

    /// Defer one exact source-text replacement event until result acceptance.
    fn text(command: &'static str, path: Option<String>, baseline_text: String) -> Self {
        Self {
            command,
            path,
            query: None,
            baseline: McpUsageBaseline::Text(baseline_text),
        }
    }
}

impl McpUsageProjectBinding {
    /// Capture one native telemetry binding in focused tests.
    #[cfg(test)]
    fn capture(state: &McpProjectState, store: &AtlasStore) -> Result<Self, DbError> {
        Self::capture_with_origin(state, store, None)
    }

    /// Capture one exact telemetry authority and optional registered origin.
    fn capture_with_origin(
        state: &McpProjectState,
        store: &AtlasStore,
        worktree_registration_id: Option<i64>,
    ) -> Result<Self, DbError> {
        let captured = store.captured_project_binding()?;
        Ok(Self {
            root: state.root.clone(),
            db_path: state.db_path.clone(),
            project_instance_id: captured.project_instance_id,
            worktree_registration_id,
        })
    }

    /// Return whether two origin bindings share one exact project database authority.
    fn same_project(&self, other: &Self) -> bool {
        self.root == other.root
            && self.db_path == other.db_path
            && self.project_instance_id == other.project_instance_id
    }
}

/// Current telemetry identity for one exact selected project binding.
#[derive(Clone, Debug)]
struct McpUsageProjectRuntime {
    /// Exact root/database authority that owns this identity.
    binding: McpUsageProjectBinding,
    /// Current bounded identity for this process/project pair.
    instance: Arc<Mutex<UsageRuntimeInstance>>,
}

/// Mutable bounded telemetry lifecycles shared by all MCP server clones.
#[derive(Debug, Default)]
struct McpUsageRuntime {
    /// Distinct selected-project identities owned by this MCP process.
    entries: Vec<McpUsageProjectRuntime>,
    /// Bounded modeled baselines reused without decoding every indexed file per call.
    source_token_baselines: VecDeque<(McpSourceTokenBaselineKey, usize)>,
}

impl McpUsageRuntime {
    /// Return or create the identity for one selected project binding.
    fn instance_for_binding(
        &mut self,
        binding: McpUsageProjectBinding,
        selected_store: &AtlasStore,
    ) -> Option<Arc<Mutex<UsageRuntimeInstance>>> {
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.binding == binding)
        {
            let entry = self.entries.remove(index);
            let instance = Arc::clone(&entry.instance);
            self.entries.push(entry);
            return Some(instance);
        }
        let instance = Arc::new(Mutex::new(UsageRuntimeInstance::new(
            UsageInstanceOwner::McpProcess,
        )?));
        if self.entries.len() >= MCP_TELEMETRY_PROJECT_BINDING_LIMIT {
            let index = (0..self.entries.len()).find(|index| {
                Self::seal_inactive_entry(&self.entries[*index], &binding, selected_store)
            })?;
            self.entries.remove(index);
        }
        self.entries.push(McpUsageProjectRuntime {
            binding,
            instance: Arc::clone(&instance),
        });
        Some(instance)
    }

    /// Seal one unborrowed least-recent binding before bounded replacement.
    fn seal_inactive_entry(
        entry: &McpUsageProjectRuntime,
        selected_binding: &McpUsageProjectBinding,
        selected_store: &AtlasStore,
    ) -> bool {
        if Arc::strong_count(&entry.instance) != 1 {
            return false;
        }
        let Ok(instance) = entry.instance.try_lock() else {
            return false;
        };
        let seal = |store: &AtlasStore| {
            store.captured_project_binding().is_ok_and(|binding| {
                binding.project_instance_id == entry.binding.project_instance_id
            }) && matches!(
                (*instance).seal(store),
                Ok(()) | Err(CliError::Db(DbError::TelemetryInstanceInactive))
            )
        };
        if entry.binding.same_project(selected_binding) {
            return seal(selected_store);
        }
        open_atlas_store_for_project(&entry.binding.db_path, &entry.binding.root)
            .is_ok_and(|store| seal(&store))
    }

    /// Clone the bounded project/runtime set before shutdown database I/O.
    fn snapshot(&self) -> Vec<McpUsageProjectRuntime> {
        self.entries.clone()
    }

    /// Return a cached generation-bound broad-source token baseline.
    fn source_token_baseline(&self, key: &McpSourceTokenBaselineKey) -> Option<usize> {
        self.source_token_baselines
            .iter()
            .find_map(|(candidate, value)| (candidate == key).then_some(*value))
    }

    /// Retain one baseline without allowing arbitrary filter keys to grow memory.
    fn insert_source_token_baseline(&mut self, key: McpSourceTokenBaselineKey, value: usize) {
        if let Some(index) = self
            .source_token_baselines
            .iter()
            .position(|(candidate, _value)| candidate == &key)
        {
            let _removed = self.source_token_baselines.remove(index);
        }
        while self.source_token_baselines.len() >= MCP_SOURCE_TOKEN_BASELINE_LIMIT {
            let _removed = self.source_token_baselines.pop_front();
        }
        self.source_token_baselines.push_back((key, value));
    }
}

/// Select whether project-state discovery validates configuration content immediately.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum McpConfigValidation {
    /// Validate configuration before returning selected project state.
    Immediate,
    /// Defer configuration reads to the admitted operation-owned work boundary.
    Deferred,
}

/// MCP response for compatibility map export.
#[derive(Debug, Serialize)]
struct McpMapReport {
    /// Canonical project root used for map generation.
    root: Option<String>,
    /// Map path from the effective config.
    map_path: Option<String>,
    /// Whether a map file was written by this call.
    written: bool,
    /// Whether JSON compatibility output was requested.
    json: bool,
    /// Human-readable reason when no file was written.
    skipped_reason: Option<String>,
}

/// Bounded control-atlas view of Git and `ProjectAtlas` worktree state.
#[derive(Debug, Serialize)]
struct McpWorktreeListReport {
    /// Reserved alias for the selected control checkout.
    control_alias: &'static str,
    /// Exact selected control checkout root.
    control_root: Option<String>,
    /// Structurally validated Git common directory.
    common_directory: Option<String>,
    /// Active Git inventory, bounded for one agent response.
    worktrees: Vec<McpWorktreeRow>,
    /// Optional retired `ProjectAtlas` registrations retained for telemetry history.
    retired: Vec<McpRetiredWorktreeRow>,
    /// Complete structural row count before response truncation.
    total_worktrees: usize,
    /// Whether structural rows exceeded the public response bound.
    truncated: bool,
}

/// One structurally discovered Git worktree joined to `ProjectAtlas` state.
#[derive(Debug, Serialize)]
struct McpWorktreeRow {
    /// Stable selector when the structural row is representable and registrable.
    selector: Option<String>,
    /// Reserved `main` or active registered alias when present.
    alias: Option<String>,
    /// Structural primary/linked role.
    role: McpGitWorktreeRole,
    /// Current reciprocal Git state.
    git_state: McpGitWorktreeState,
    /// `ProjectAtlas` registration relationship.
    registration: McpWorktreeRegistrationState,
    /// Stable Git administrative identity.
    administrative_directory: Option<String>,
    /// Exact current source root when active.
    root: Option<String>,
    /// Exact atlas availability for the current source root.
    atlas_state: McpWorktreeAtlasState,
    /// Local aggregate relationship to the accepted control snapshot.
    telemetry_state: McpWorktreeTelemetryState,
    /// Last local aggregate revision accepted by the control atlas.
    accepted_telemetry_revision: Option<u64>,
    /// Current local aggregate revision when safely readable.
    local_telemetry_revision: Option<u64>,
    /// Exact initialized project identity when safely readable.
    project_instance_id: Option<String>,
    /// Typed structural or atlas diagnostic for unavailable rows.
    blocker: Option<String>,
}

/// Retired `ProjectAtlas` registration retained outside source selection.
#[derive(Debug, Serialize)]
struct McpRetiredWorktreeRow {
    /// Last human/agent-facing alias.
    alias: String,
    /// Last structurally validated source root.
    last_root: Option<String>,
    /// Exact initialized project identity when one was bound.
    project_instance_id: Option<String>,
    /// Last local aggregate revision accepted before retirement.
    accepted_telemetry_revision: u64,
}

/// Compact candidate returned for ambiguous or missing add selection.
#[derive(Debug, Serialize)]
struct McpWorktreeCandidate {
    /// Stable selector accepted by a later add call.
    selector: String,
    /// Exact active source root.
    root: Option<String>,
    /// Structural primary/linked role.
    role: McpGitWorktreeRole,
}

/// Result of one ProjectAtlas-only worktree registration transition.
#[derive(Debug, Serialize)]
struct McpWorktreeMutationReport {
    /// Registration operation that was requested.
    operation: McpWorktreeMutationOperation,
    /// Mutation or bounded selection outcome.
    status: McpWorktreeMutationStatus,
    /// Stable structural selector when one candidate was selected.
    selector: Option<String>,
    /// Active or retired `ProjectAtlas` alias when known.
    alias: Option<String>,
    /// Exact source root when known.
    root: Option<String>,
    /// Stable control-database registration identity when committed.
    registration_id: Option<i64>,
    /// Final local telemetry synchronization outcome when attempted.
    telemetry_sync: Option<WorktreeUsageSyncState>,
    /// Bounded candidates when a human selector was ambiguous.
    candidates: Vec<McpWorktreeCandidate>,
    /// Typed synchronization or structural note when the transition is partial by design.
    blocker: Option<String>,
    /// Git lifecycle is never changed by these operations.
    git_unchanged: bool,
    /// Source, `.projectatlas`, and database files are never deleted by unregister.
    files_unchanged: bool,
}

/// Structural role serialized without leaking debug vocabulary.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpGitWorktreeRole {
    /// Primary checkout for the Git common directory.
    Primary,
    /// Linked checkout with reciprocal registration evidence.
    Linked,
}

impl From<GitWorktreeRole> for McpGitWorktreeRole {
    fn from(value: GitWorktreeRole) -> Self {
        match value {
            GitWorktreeRole::Primary => Self::Primary,
            GitWorktreeRole::Linked => Self::Linked,
        }
    }
}

/// Git state exposed by structural worktree inventory.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpGitWorktreeState {
    /// Reciprocal evidence resolves an exact source root.
    Active,
    /// Git retains registration evidence but the checkout is absent.
    Missing,
    /// Structural evidence is malformed or unsafe.
    Invalid,
}

/// `ProjectAtlas` registration relationship for one structural row.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpWorktreeRegistrationState {
    /// Reserved selected control checkout.
    Control,
    /// Active short alias is stored in the control atlas.
    Registered,
    /// Git knows the worktree but `ProjectAtlas` does not.
    Unregistered,
}

/// Exact local atlas availability without modifying its database.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpWorktreeAtlasState {
    /// A compatible atlas is bound to the exact current root.
    Initialized,
    /// No target-local database exists.
    Missing,
    /// A database exists but cannot be admitted for this exact root.
    Invalid,
    /// Git does not expose an active source root.
    Unavailable,
}

/// Relationship between independent local and durable control telemetry.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpWorktreeTelemetryState {
    /// Native telemetry belongs to the selected control atlas.
    Control,
    /// The latest readable local snapshot has been accepted.
    Current,
    /// A newer readable local snapshot awaits synchronization.
    Pending,
    /// No local atlas exists yet.
    MissingAtlas,
    /// No `ProjectAtlas` registration owns this structural row.
    Unregistered,
    /// Structural or atlas state prevents a trustworthy comparison.
    Unavailable,
}

/// ProjectAtlas-only registry operation.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpWorktreeMutationOperation {
    /// Add or reactivate one alias.
    Add,
    /// Retire one active alias after final synchronization.
    Remove,
}

/// Result state for one worktree registry operation.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpWorktreeMutationStatus {
    /// One active registration was committed.
    Registered,
    /// One active registration was retired.
    Retired,
    /// No active structural candidate matched.
    NotFound,
    /// More than one active structural candidate matched.
    Ambiguous,
}

/// Read-only local atlas facts used by list/add/remove orchestration.
struct LocalWorktreeAtlas {
    /// Exact atlas identity.
    project_instance_id: ProjectInstanceId,
    /// Bounded local aggregate snapshot.
    snapshot: WorktreeUsageSnapshot,
}

/// Role-typed selected root for MCP path routing.
#[derive(Debug, Clone)]
struct McpSelectedRoot(PathBuf);

impl McpSelectedRoot {
    /// Build the selected root wrapper from active project state.
    fn from_state(state: &McpProjectState) -> Self {
        Self(state.root.clone())
    }

    /// Convert an absolute path inside this root into a repository key.
    fn repo_key_for(&self, path: &McpAbsolutePath) -> Result<Option<McpRepoKey>, CliError> {
        if !path.as_path().starts_with(&self.0) {
            return Ok(None);
        }
        normalize_repo_path(&self.0, path.as_path())
            .map(McpRepoKey)
            .map(Some)
            .map_err(ProjectAtlasMcpServer::selected_project_path_error)
    }
}

/// Role-typed indexed project root discovered by nearest-project routing.
#[derive(Debug, Clone)]
struct McpIndexedRoot {
    /// Canonical indexed project root.
    root: PathBuf,
    /// Durable index path that records the same root.
    db_path: PathBuf,
}

/// Canonical absolute path supplied to an MCP path-bearing tool.
#[derive(Debug, Clone)]
struct McpAbsolutePath(PathBuf);

impl McpAbsolutePath {
    /// Canonicalize an absolute path through its nearest existing ancestor.
    ///
    /// File selectors may refer to an indexed path that was deleted offline.
    /// Canonicalizing the existing ancestor preserves symlink/root-escape
    /// checks without requiring the selected leaf to still exist.
    fn canonicalize(path: &Path) -> Result<Self, CliError> {
        let mut existing = path;
        let mut missing_suffix = Vec::new();
        while !existing.exists() {
            let file_name = existing
                .file_name()
                .ok_or_else(|| missing_ancestor_error(path))?;
            missing_suffix.push(PathBuf::from(file_name));
            existing = existing
                .parent()
                .ok_or_else(|| missing_ancestor_error(path))?;
        }
        let mut canonical = canonical_project_root(existing)?;
        for component in missing_suffix.into_iter().rev() {
            canonical.push(component);
        }
        Ok(Self(canonical))
    }

    /// Borrow the canonical path.
    fn as_path(&self) -> &Path {
        &self.0
    }

    /// Return the directory where nearest-indexed-root discovery should begin.
    fn nearest_search_start(&self) -> &Path {
        if self.0.is_dir() {
            &self.0
        } else {
            self.0.parent().unwrap_or(self.as_path())
        }
    }
}

/// Build the adapter error for an absolute selector without an inspectable ancestor.
fn missing_ancestor_error(path: &Path) -> CliError {
    CliError::InvalidInput(format!(
        "absolute path '{}' has no existing ancestor",
        path.display()
    ))
}

/// Repository-relative path key derived from a typed root/path conversion.
#[derive(Debug, Clone)]
struct McpRepoKey(String);

impl McpRepoKey {
    /// Consume the wrapper into the repository key string.
    fn into_string(self) -> String {
        self.0
    }
}

/// MCP path resolution result with selected-project audit state.
#[derive(Debug, Clone)]
struct McpResolvedRepoPath {
    /// Project state selected for the request.
    state: McpProjectState,
    /// Repository-relative path key inside the selected project.
    key: String,
    /// Whether nearest-project routing changed the selected root/DB.
    routed_project: bool,
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
    /// Stable machine-readable error kind.
    kind: AgentErrorKind,
    /// Human-readable error and recovery guidance.
    message: String,
    /// Bounded local-source mismatch details when refresh is required.
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_required: Option<IndexRefreshRequired>,
    /// Exact selected-root initialization handoff.
    #[serde(skip_serializing_if = "Option::is_none")]
    init_required: Option<IndexInitRequired>,
    /// Bare/common Git root selection diagnostic.
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree_required: Option<ProjectWorktreeRequired>,
    /// Bounded source/policy diagnostic when verification cannot complete.
    #[serde(skip_serializing_if = "Option::is_none")]
    verification_incomplete: Option<IndexVerificationIncomplete>,
    /// Project/index identity mismatch details.
    #[serde(skip_serializing_if = "Option::is_none")]
    project_mismatch: Option<IndexProjectMismatch>,
    /// Content-free database placement details for a rejected `SQLite` profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    database_filesystem: Option<DatabaseFilesystemErrorPayload>,
    /// Content-free incompatible schema details.
    #[serde(skip_serializing_if = "Option::is_none")]
    schema_version_mismatch: Option<SchemaVersionMismatchPayload>,
    /// Content-free supported migration handoff.
    #[serde(skip_serializing_if = "Option::is_none")]
    schema_migration_required: Option<SchemaMigrationRequiredPayload>,
    /// Optional retrieval capability state and recovery guidance.
    #[serde(skip_serializing_if = "Option::is_none")]
    search_capability: Option<crate::SearchCapabilityErrorPayload>,
    /// Reusable recovery call when refresh is required.
    #[serde(skip_serializing_if = "Option::is_none")]
    next: Option<McpNextCall>,
}

/// Direct MCP recovery selector.
#[derive(Debug, Serialize)]
struct McpNextCall {
    /// Existing MCP tool that safely recovers the selected project.
    tool: &'static str,
    /// Canonical project root for legacy per-call isolation.
    #[serde(skip_serializing_if = "Option::is_none")]
    project_path: Option<String>,
    /// Registered alias preserved by worktree-routed recovery.
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree: Option<String>,
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
    /// Registered alias used to capture this project state.
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree: Option<String>,
    /// Durable control-catalog registration identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    registration_id: Option<i64>,
    /// Canonical repository root.
    root: Option<String>,
    /// Selected durable `SQLite` index path.
    db: Option<String>,
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
    /// Registry-owned content role when the selected path is a file.
    #[serde(skip_serializing_if = "Option::is_none")]
    classification: Option<ContentClassification>,
    /// Durable purpose status after the update.
    status: PurposeStatus,
    /// Source of the durable purpose after the update.
    source: PurposeSource,
    /// Whether the purpose has been agent-reviewed.
    agent_reviewed: bool,
}

/// MCP-session capability/settings payload.
#[derive(Debug, Serialize)]
struct McpSessionCapabilities {
    /// Runtime identity and compiled tool surface.
    runtime: RuntimeInfoReport,
    /// Selected project identity and index status.
    selected_project: McpSelectedProjectCapability,
    /// Route-affecting startup policy.
    startup_policy: McpStartupPolicy,
    /// Absolute-path routing scope.
    path_scope: McpPathScope,
    /// Scan behavior visible to harnesses.
    scan_policy: McpScanPolicy,
    /// Closed classified-content navigation surface.
    classified_navigation: SettingsClassifiedNavigationReport,
    /// Token telemetry write mode for this process.
    telemetry: McpTelemetryPolicy,
    /// Privacy guarantees for this payload.
    privacy: McpPrivacyPolicy,
}

/// Selected project identity inside capability/settings payloads.
#[derive(Debug, Serialize)]
struct McpSelectedProjectCapability {
    /// Registered alias used to capture this project state.
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree: Option<String>,
    /// Durable control-catalog registration identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    registration_id: Option<i64>,
    /// Canonical repository root.
    root: Option<String>,
    /// Selected durable `SQLite` index path.
    db: Option<String>,
    /// Selected configuration path when present.
    config: Option<String>,
    /// Whether the selected durable index exists.
    index_status: McpIndexStatus,
}

/// Startup policy fields for MCP sessions.
#[derive(Debug, Serialize)]
struct McpStartupPolicy {
    /// Whether nearest indexed project routing is enabled by default.
    nearest_project: McpPolicyState,
}

/// Scan policy fields relevant before source reads.
#[derive(Debug, Serialize)]
struct McpScanPolicy {
    /// Settings calls and session briefs never scan implicitly.
    implicit_scan: McpPolicyState,
    /// Maximum `UTF-8` file size persisted into `SQLite` text search.
    text_index_max_bytes: u64,
}

/// Telemetry write policy for this MCP process.
#[derive(Debug, Serialize)]
struct McpTelemetryPolicy {
    /// Whether token telemetry writes are enabled.
    mode: McpPolicyState,
}

/// Privacy contract for capability/settings payloads.
#[derive(Debug, Serialize)]
struct McpPrivacyPolicy {
    /// No arbitrary process environment dump is included.
    environment_dump: bool,
    /// No secret or token values are included.
    secret_values: bool,
    /// Host paths are limited to `ProjectAtlas` runtime/root/DB/config paths.
    projectatlas_paths_only: bool,
}

/// Two-state policy enum serialized for MCP contracts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpPolicyState {
    /// Policy is enabled.
    Enabled,
    /// Policy is disabled.
    Disabled,
}

/// Selected index availability.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpIndexStatus {
    /// The selected index file exists.
    Available,
    /// The selected index file is missing.
    Missing,
}

/// Absolute path routing scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpPathScope {
    /// Calls stay within the selected project.
    SelectedProject,
    /// Absolute paths may route to the nearest indexed project.
    NearestIndexedProject,
}

/// Explicit compact file-summary payload with actionable facts and redundant state removed.
#[derive(Debug, Serialize)]
struct McpFileSummaryPayload<'a> {
    /// Explicit compact file intelligence.
    file_summary: McpFileSummary<'a>,
}

/// Compact projection used when an agent follows a default startup recommendation.
#[derive(Debug, Serialize)]
struct McpFileSummary<'a> {
    /// Repository-relative file path.
    file_path: &'a str,
    /// Persisted content role for the selected file.
    classification: projectatlas_core::language::ContentClassification,
    /// Detected language or file family.
    language: &'a str,
    /// Source line count.
    line_count: usize,
    /// Non-default source state when live source was unavailable.
    #[serde(skip_serializing_if = "Option::is_none")]
    source_status: Option<&'a str>,
    /// Source read diagnostic when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    source_error: Option<&'a str>,
    /// Parser family that produced the summary.
    parser_kind: &'a str,
    /// Summary quality state agents must inspect before trusting generated prose.
    summary_status: &'a str,
    /// Durable file responsibility when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    file_purpose: Option<&'a str>,
    /// Purpose status for suggestions or other unreviewed rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    file_purpose_status: Option<&'a str>,
    /// Purpose source for suggestions or other unreviewed rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    file_purpose_source: Option<&'a str>,
    /// Whether an agent approved the retained responsibility.
    #[serde(skip_serializing_if = "is_false")]
    file_purpose_agent_reviewed: bool,
    /// Current deterministic content summary.
    content_summary: &'a str,
    /// Package or module name when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    package: Option<&'a str>,
    /// File documentation when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    docstring: Option<&'a str>,
    /// Whether the default bounded repeated sections omitted rows.
    #[serde(skip_serializing_if = "is_false")]
    truncated: bool,
    /// Indexed functions when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    functions: Option<Vec<McpFileSymbolSummary<'a>>>,
    /// Indexed methods when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    methods: Option<Vec<McpFileSymbolSummary<'a>>>,
    /// Indexed classes when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    classes: Option<Vec<McpFileSymbolSummary<'a>>>,
    /// Indexed type declarations when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    types: Option<Vec<McpFileSymbolSummary<'a>>>,
    /// Imports when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    imports: Option<&'a [String]>,
    /// Manifest dependencies when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    dependencies: Option<&'a [String]>,
    /// Exported declarations when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    exports: Option<&'a [String]>,
    /// Call rows when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    calls: Option<&'a [FileCallSummary]>,
    /// Coverage details only when they require agent attention.
    #[serde(skip_serializing_if = "Option::is_none")]
    coverage: Option<McpCompactCoverageDigest<'a>>,
}

/// Compact symbol row that omits empty legacy fields.
#[derive(Debug, Serialize)]
struct McpFileSymbolSummary<'a> {
    /// Symbol name.
    name: &'a str,
    /// Symbol kind.
    kind: &'a str,
    /// One-based start line.
    line: usize,
    /// One-based end line.
    end_line: usize,
    /// Declaration signature.
    signature: &'a str,
    /// Whether the declaration is externally visible.
    exported: bool,
    /// Extracted documentation when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    documentation: Option<&'a str>,
    /// Parent declaration when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<&'a str>,
    /// Indexed callers when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    called_by: Option<&'a [String]>,
}

impl<'a> From<&'a FileSymbolSummary> for McpFileSymbolSummary<'a> {
    fn from(symbol: &'a FileSymbolSummary) -> Self {
        Self {
            name: &symbol.name,
            kind: &symbol.kind,
            line: symbol.line,
            end_line: symbol.end_line,
            signature: &symbol.signature,
            exported: symbol.exported,
            documentation: nonempty_str(&symbol.documentation),
            parent: nonempty_str(&symbol.parent),
            called_by: nonempty_slice(&symbol.called_by),
        }
    }
}

/// Project nonempty symbol rows into their compact MCP representation.
fn compact_file_symbols(symbols: &[FileSymbolSummary]) -> Option<Vec<McpFileSymbolSummary<'_>>> {
    (!symbols.is_empty()).then(|| symbols.iter().map(McpFileSymbolSummary::from).collect())
}

impl<'a> From<&'a FileSummaryReport> for McpFileSummary<'a> {
    fn from(report: &'a FileSummaryReport) -> Self {
        let reviewed_purpose = report.file_purpose_agent_reviewed;
        let coverage_requires_attention = !report.coverage.available
            || report.coverage.trust != CoverageTrustState::Trusted
            || report.coverage.omitted > 0
            || report.coverage.truncated;
        Self {
            file_path: &report.file_path,
            classification: report.classification,
            language: &report.language,
            line_count: report.line_count,
            source_status: (report.source_status != MCP_FILE_SOURCE_STATUS_LIVE)
                .then_some(report.source_status.as_str()),
            source_error: nonempty_str(&report.source_error),
            parser_kind: &report.parser_kind,
            summary_status: &report.summary_status,
            file_purpose: nonempty_str(&report.file_purpose),
            file_purpose_status: (!reviewed_purpose).then_some(report.file_purpose_status.as_str()),
            file_purpose_source: (!reviewed_purpose).then_some(report.file_purpose_source.as_str()),
            file_purpose_agent_reviewed: reviewed_purpose,
            content_summary: &report.content_summary,
            package: nonempty_str(&report.package),
            docstring: nonempty_str(&report.docstring),
            truncated: report.truncated,
            functions: compact_file_symbols(&report.functions),
            methods: compact_file_symbols(&report.methods),
            classes: compact_file_symbols(&report.classes),
            types: compact_file_symbols(&report.types),
            imports: nonempty_slice(&report.imports),
            dependencies: nonempty_slice(&report.dependencies),
            exports: nonempty_slice(&report.exports),
            calls: nonempty_slice(&report.calls),
            coverage: coverage_requires_attention
                .then(|| McpCompactCoverageDigest::from(&report.coverage)),
        }
    }
}

/// Adapter-local sparse coverage counts for the explicit compact summary.
#[derive(Debug, Serialize)]
struct McpCompactCoverageStateCounts {
    /// Complete coverage rows.
    #[serde(skip_serializing_if = "is_zero_u32")]
    complete: u32,
    /// Complete extraction scopes containing no supported candidates.
    #[serde(skip_serializing_if = "is_zero_u32")]
    no_candidates: u32,
    /// Partial coverage rows.
    #[serde(skip_serializing_if = "is_zero_u32")]
    partial: u32,
    /// Failed coverage rows.
    #[serde(skip_serializing_if = "is_zero_u32")]
    failed: u32,
    /// Intentionally ignored coverage rows.
    #[serde(skip_serializing_if = "is_zero_u32")]
    ignored: u32,
    /// Oversized coverage rows.
    #[serde(skip_serializing_if = "is_zero_u32")]
    oversized: u32,
    /// Quarantined coverage rows.
    #[serde(skip_serializing_if = "is_zero_u32")]
    quarantined: u32,
    /// Stale coverage rows.
    #[serde(skip_serializing_if = "is_zero_u32")]
    stale: u32,
}

/// Adapter-local compact coverage projection that leaves shared serialization unchanged.
#[derive(Debug, Serialize)]
struct McpCompactCoverageDigest<'a> {
    /// False when no current coverage rows exist.
    #[serde(skip_serializing_if = "is_true")]
    available: bool,
    /// Active generation shared by retained rows.
    active_generation: &'a IndexGeneration,
    /// Source parser pass recorded for the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    parser: Option<&'a ParserKind>,
    /// Fact provider pass recorded for the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<&'a ParserKind>,
    /// Sparse per-state counts.
    states: McpCompactCoverageStateCounts,
    /// Total items declared by retained rows.
    total: u64,
    /// Covered items declared by retained rows.
    covered: u64,
    /// Omitted or untrusted items declared by retained rows.
    #[serde(skip_serializing_if = "is_zero_u64")]
    omitted: u64,
    /// Number of retained relation-family rows.
    #[serde(skip_serializing_if = "is_zero_u32")]
    relation_rows: u32,
    /// Whether digest bounds omitted additional selected-file rows.
    #[serde(skip_serializing_if = "is_false")]
    truncated: bool,
    /// Conservative trust state across retained rows.
    trust: &'a CoverageTrustState,
    /// Existing opt-in health call for deeper coverage discovery.
    next_call: &'a NavigationNextCall,
}

impl<'a> From<&'a CoverageDigest> for McpCompactCoverageDigest<'a> {
    fn from(coverage: &'a CoverageDigest) -> Self {
        Self {
            available: coverage.available,
            active_generation: &coverage.active_generation,
            parser: coverage.parser.as_ref(),
            provider: coverage.provider.as_ref(),
            states: McpCompactCoverageStateCounts {
                complete: coverage.states.complete,
                no_candidates: coverage.states.no_candidates,
                partial: coverage.states.partial,
                failed: coverage.states.failed,
                ignored: coverage.states.ignored,
                oversized: coverage.states.oversized,
                quarantined: coverage.states.quarantined,
                stale: coverage.states.stale,
            },
            total: coverage.total,
            covered: coverage.covered,
            omitted: coverage.omitted,
            relation_rows: coverage.relation_rows,
            truncated: coverage.truncated,
            trust: &coverage.trust,
            next_call: &coverage.next_call,
        }
    }
}

/// Adapter-local compact detailed-relation node without stable-key duplication.
#[derive(Debug, Serialize)]
struct McpCompactDetailedRelationNode<'a> {
    /// Exact selector accepted by later relation, summary, and slice calls.
    selector: &'a EntitySelector,
    /// Accepted, unavailable, or non-local purpose state.
    purpose: &'a RelationPurpose,
    /// Authoritative coverage rows for the selected local owner.
    #[serde(skip_serializing_if = "Option::is_none")]
    coverage: Option<&'a [CoverageRecord]>,
}

impl<'a> From<&'a DetailedRelationNode> for McpCompactDetailedRelationNode<'a> {
    fn from(node: &'a DetailedRelationNode) -> Self {
        Self {
            selector: node.entity.selector(),
            purpose: &node.purpose,
            coverage: nonempty_slice(&node.coverage),
        }
    }
}

/// Resolution facts retained by the compact relation projection.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum McpCompactRelationResolution<'a> {
    /// Exactly one reusable local target was resolved.
    Resolved {
        /// Exact selector accepted by later calls.
        selector: &'a ReusableTargetSelector,
        /// Complete generation containing the target.
        generation: IndexGeneration,
    },
    /// More than one valid target remains.
    Ambiguous {
        /// Original normalized reference.
        reference: &'a GraphIdentityText,
        /// Number of retained candidates before limits.
        candidates: u32,
    },
    /// No supported static target was found.
    Unresolved {
        /// Original normalized reference.
        reference: &'a GraphIdentityText,
    },
    /// The target is intentionally outside the selected project.
    External {
        /// Typed external identity.
        external: &'a ExternalSelector,
        /// Complete generation containing the external record.
        generation: IndexGeneration,
    },
}

impl<'a> From<&'a RelationResolution> for McpCompactRelationResolution<'a> {
    fn from(resolution: &'a RelationResolution) -> Self {
        match resolution {
            RelationResolution::Resolved {
                selector,
                generation,
                ..
            } => Self::Resolved {
                selector,
                generation: *generation,
            },
            RelationResolution::Ambiguous {
                reference,
                candidates,
            } => Self::Ambiguous {
                reference,
                candidates: candidates.get(),
            },
            RelationResolution::Unresolved { reference } => Self::Unresolved { reference },
            RelationResolution::External {
                external,
                generation,
                ..
            } => Self::External {
                external,
                generation: *generation,
            },
        }
    }
}

/// Compact relation facts required for trust and direct navigation.
#[derive(Debug, Serialize)]
struct McpCompactLogicalRelation<'a> {
    /// Typed legacy or extended relation family.
    kind: GraphRelationKind,
    /// Resolution state and reusable target when local.
    resolution: McpCompactRelationResolution<'a>,
    /// Coarse trust class.
    confidence: ConfidenceClass,
    /// Producer completeness for this relation scope.
    completeness: Completeness,
    /// Complete generation containing the relation.
    generation: IndexGeneration,
}

/// One compact detailed-relation row.
#[derive(Debug, Serialize)]
struct McpCompactDetailedRelationRow<'a> {
    /// One-based traversal depth.
    depth: u32,
    /// Direction relative to the selected frontier.
    direction: RelationDirection,
    /// Typed relation facts without stable-key duplication.
    relation: McpCompactLogicalRelation<'a>,
    /// Closed reason retained only for an unresolved canonical document relation.
    #[serde(skip_serializing_if = "Option::is_none")]
    document_unresolved_reason: Option<DocumentTargetUnresolvedReason>,
    /// Exact source selector, purpose, and coverage.
    source: McpCompactDetailedRelationNode<'a>,
    /// Retained local or external target when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<McpCompactDetailedRelationNode<'a>>,
    /// Purpose disposition when no target node exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    target_purpose: Option<&'a RelationPurpose>,
    /// Exact selectors for a multi-hop path; direct source/target rows omit this duplicate.
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<Vec<&'a EntitySelector>>,
    /// Exact supporting occurrences when requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    occurrences: Option<Vec<McpCompactRelationOccurrence<'a>>>,
    /// Whether the per-relation occurrence ceiling omitted rows.
    #[serde(skip_serializing_if = "is_false")]
    occurrences_truncated: bool,
    /// Existing exact call that consumes the selected local endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    next_call: Option<&'a RelationNextCall>,
}

/// Compact source occurrence without another copy of the owning relation key.
#[derive(Debug, Serialize)]
struct McpCompactRelationOccurrence<'a> {
    /// Repository-local file containing the evidence.
    file: &'a RepositoryFilePath,
    /// Exact supporting source range.
    span: SourceSpan,
    /// Complete generation containing the occurrence.
    generation: IndexGeneration,
}

impl<'a> From<&'a RelationOccurrence> for McpCompactRelationOccurrence<'a> {
    fn from(occurrence: &'a RelationOccurrence) -> Self {
        Self {
            file: occurrence.file(),
            span: occurrence.span(),
            generation: occurrence.generation(),
        }
    }
}

impl<'a> From<&'a DetailedRelationRow> for McpCompactDetailedRelationRow<'a> {
    fn from(row: &'a DetailedRelationRow) -> Self {
        Self {
            depth: row.depth,
            direction: row.direction,
            relation: McpCompactLogicalRelation {
                kind: row.relation.kind(),
                resolution: McpCompactRelationResolution::from(row.relation.resolution()),
                confidence: row.relation.confidence(),
                completeness: row.relation.completeness(),
                generation: row.relation.generation(),
            },
            document_unresolved_reason: row.document_unresolved_reason,
            source: McpCompactDetailedRelationNode::from(&row.source),
            target: row
                .target
                .as_ref()
                .map(McpCompactDetailedRelationNode::from),
            target_purpose: row.target.is_none().then_some(&row.target_purpose),
            path: (row.path.len() > 2)
                .then(|| row.path.iter().map(|node| node.entity.selector()).collect()),
            occurrences: (!row.occurrences.is_empty()).then(|| {
                row.occurrences
                    .iter()
                    .map(McpCompactRelationOccurrence::from)
                    .collect()
            }),
            occurrences_truncated: row.occurrences_truncated,
            next_call: row.next_call.as_ref(),
        }
    }
}

/// Opt-in compact projection of one detailed relation page.
#[derive(Debug, Serialize)]
struct McpCompactDetailedRelationReport<'a> {
    /// Exact selected anchor.
    anchor: McpCompactDetailedRelationNode<'a>,
    /// Complete graph generation captured by the page.
    generation: IndexGeneration,
    /// Accepted authored-purpose revision captured by the page.
    authored_purpose_revision: u64,
    /// Direction followed from the anchor.
    direction: RelationDirection,
    /// Number of retained relation steps.
    returned: u32,
    /// Number of cyclic or duplicate-node paths pruned.
    #[serde(skip_serializing_if = "is_zero_u64")]
    pruned_paths: u64,
    /// Whether a declared boundary stopped traversal.
    #[serde(skip_serializing_if = "is_false")]
    truncated: bool,
    /// Directly reusable continuation call with the exact original query and budget.
    #[serde(skip_serializing_if = "Option::is_none")]
    next_call: Option<McpCompactRelationContinuationCall<'a>>,
    /// Exact, lower-bound, or unknown cardinality.
    total: &'a RelationTotalState,
    /// Stable hard limits reached while constructing the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    reached_limits: Option<&'a [GraphLimitKind]>,
    /// Aggregate page and retained-state work.
    work: &'a DetailedRelationWork,
    /// Ranked node-simple relation steps.
    rows: Vec<McpCompactDetailedRelationRow<'a>>,
}

/// Existing MCP relation call that resumes one exact compact page.
#[derive(Debug, Serialize)]
struct McpCompactRelationContinuationCall<'a> {
    /// Existing MCP tool that owns relation continuation.
    tool: &'static str,
    /// Exact original request plus its generation-bound cursor.
    arguments: McpCompactRelationContinuationArguments<'a>,
}

/// Exact result-defining arguments required to resume a detailed relation page.
#[derive(Debug, Serialize)]
struct McpCompactRelationContinuationArguments<'a> {
    /// Explicit project root when the original request supplied one.
    #[serde(skip_serializing_if = "Option::is_none")]
    project_path: Option<&'a str>,
    /// Registered primary worktree when the original request supplied one.
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree: Option<&'a str>,
    /// Selected repository-relative anchor file.
    file: &'a str,
    /// Original nearest-project policy when explicitly supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    nearest_project: Option<bool>,
    /// Original ordered federated roots when supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    roots: Option<&'a [String]>,
    /// Original ordered registered worktree aliases when supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    worktrees: Option<&'a [String]>,
    /// Detailed relation view.
    view: &'static str,
    /// Preserve the compact response projection.
    compact: bool,
    /// Generation-, purpose-, query-, order-, and budget-bound cursor.
    cursor: &'a str,
    /// Exact symbol name when the anchor is a declaration.
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol: Option<&'a str>,
    /// Exact nonempty symbol parent when supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol_parent: Option<&'a str>,
    /// Exact nonempty symbol kind when supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol_kind: Option<&'a str>,
    /// Exact nonempty symbol signature when supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol_signature: Option<&'a str>,
    /// Original traversal direction when explicitly supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    direction: Option<&'a str>,
    /// Original relation filter when explicitly supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    relation: Option<&'a str>,
    /// Original confidence floor when explicitly supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    minimum_confidence: Option<&'a str>,
    /// Original resolution filter when explicitly supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    resolution: Option<&'a str>,
    /// Original traversal depth when explicitly supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    depth: Option<u32>,
    /// Original occurrence-retention choice when explicitly supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    include_occurrences: Option<bool>,
    /// Original returned-row limit when explicitly supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<usize>,
    /// Original per-relation occurrence limit when explicitly supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    occurrence_limit: Option<u32>,
    /// Original edge budget when explicitly supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    edge_limit: Option<u32>,
    /// Original node budget when explicitly supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    node_limit: Option<u32>,
    /// Original visited-node budget when explicitly supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    visited_limit: Option<u32>,
    /// Original aggregate occurrence budget when explicitly supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    occurrence_total_limit: Option<u32>,
    /// Original intermediate-byte budget when explicitly supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    intermediate_bytes: Option<u64>,
    /// Original service deadline when explicitly supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    deadline_ms: Option<u64>,
    /// Original rendered-output budget when explicitly supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    output_bytes: Option<u32>,
}

impl<'a> McpCompactDetailedRelationReport<'a> {
    /// Project one detailed page and its directly reusable continuation.
    fn new(
        report: &'a DetailedRelationReport,
        file: &'a str,
        params: &'a AtlasSymbolRelationsParams,
    ) -> Self {
        Self {
            anchor: McpCompactDetailedRelationNode::from(&report.anchor),
            generation: report.generation,
            authored_purpose_revision: report.authored_purpose_revision,
            direction: report.direction,
            returned: report.returned,
            pruned_paths: report.pruned_paths,
            truncated: report.truncated,
            next_call: report.continuation.as_deref().map(|cursor| {
                McpCompactRelationContinuationCall {
                    tool: MCP_TOOL_ATLAS_SYMBOL_RELATIONS,
                    arguments: McpCompactRelationContinuationArguments {
                        project_path: params.project_path.as_deref(),
                        worktree: params.worktree.as_deref(),
                        file,
                        nearest_project: params.nearest_project,
                        roots: params.roots.as_deref(),
                        worktrees: params.worktrees.as_deref(),
                        view: MCP_SYMBOL_RELATION_VIEW_DETAILED,
                        compact: true,
                        cursor,
                        symbol: params.symbol.as_deref(),
                        symbol_parent: params.symbol_parent.as_deref().and_then(nonempty_str),
                        symbol_kind: params.symbol_kind.as_deref().and_then(nonempty_str),
                        symbol_signature: params.symbol_signature.as_deref().and_then(nonempty_str),
                        direction: params.direction.as_deref(),
                        relation: params.relation.as_deref(),
                        minimum_confidence: params.minimum_confidence.as_deref(),
                        resolution: params.resolution.as_deref(),
                        depth: params.depth,
                        include_occurrences: params.include_occurrences,
                        limit: params.limit,
                        occurrence_limit: params.occurrence_limit,
                        edge_limit: params.edge_limit,
                        node_limit: params.node_limit,
                        visited_limit: params.visited_limit,
                        occurrence_total_limit: params.occurrence_total_limit,
                        intermediate_bytes: params.intermediate_bytes,
                        deadline_ms: params.deadline_ms,
                        output_bytes: params.output_bytes,
                    },
                }
            }),
            total: &report.total,
            reached_limits: nonempty_slice(&report.reached_limits),
            work: &report.work,
            rows: report
                .rows
                .iter()
                .map(McpCompactDetailedRelationRow::from)
                .collect(),
        }
    }
}

/// Compact federated wrapper that preserves cross-root evidence and work.
#[derive(Debug, Serialize)]
struct McpCompactFederatedDetailedRelationReport<'a> {
    /// Ordered validated participants.
    participants: &'a [FederatedParticipant],
    /// Worktree alias of the first-root result when aliases selected federation.
    #[serde(skip_serializing_if = "Option::is_none")]
    primary_worktree: Option<&'a str>,
    /// Compact first-root detailed relation page.
    primary: McpCompactDetailedRelationReport<'a>,
    /// Exact cross-root external rendezvous evidence.
    #[serde(skip_serializing_if = "Option::is_none")]
    rendezvous: Option<&'a [FederatedRendezvous]>,
    /// Whether primary or rendezvous work was truncated.
    #[serde(skip_serializing_if = "is_false")]
    truncated: bool,
    /// Stable aggregate limits reached by either stage.
    #[serde(skip_serializing_if = "Option::is_none")]
    reached_limits: Option<&'a [GraphLimitKind]>,
    /// Exact aggregate work.
    work: &'a FederatedRelationWork,
}

impl<'a> McpCompactFederatedDetailedRelationReport<'a> {
    /// Project a federated page while preserving its exact continuation call.
    fn new(
        report: &'a FederatedDetailedRelationReport,
        file: &'a str,
        params: &'a AtlasSymbolRelationsParams,
    ) -> Self {
        Self {
            participants: &report.participants,
            primary_worktree: report.primary_worktree.as_deref(),
            primary: McpCompactDetailedRelationReport::new(&report.primary, file, params),
            rendezvous: nonempty_slice(&report.rendezvous),
            truncated: report.truncated,
            reached_limits: nonempty_slice(&report.reached_limits),
            work: &report.work,
        }
    }
}

/// Borrow non-empty text into one optional compact field.
fn nonempty_str(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

/// Parse an optional adapter selection before any project or database work.
fn parse_content_selection(value: Option<&str>) -> Result<ContentSelection, CliError> {
    value
        .map(str::parse::<ContentSelection>)
        .transpose()
        .map(Option::unwrap_or_default)
        .map_err(|error| CliError::InvalidInput(error.to_string()))
}

/// Borrow non-empty rows into one optional compact section.
fn nonempty_slice<T>(value: &[T]) -> Option<&[T]> {
    (!value.is_empty()).then_some(value)
}

/// Return whether a compact unsigned count is zero.
#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}

/// Return whether a compact unsigned total is zero.
#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_zero_u64(value: &u64) -> bool {
    *value == 0
}

/// Agent startup brief payload.
#[derive(Debug, Serialize)]
struct McpSessionBrief {
    /// Selected project and index identity.
    project: McpSelectedProjectCapability,
    /// Route-affecting startup policy.
    policy: McpBriefPolicy,
    /// Overview counts when an index exists.
    overview: Option<Overview>,
    /// Indexed candidate folder rows.
    folders: Vec<McpBriefCandidate>,
    /// Indexed candidate file rows.
    files: Vec<McpBriefCandidate>,
    /// Bounded health blockers.
    blockers: McpBriefBlockers,
    /// Actionable bounded purpose-curator handoff for the selected project.
    purpose_handoff: Option<PurposeCuratorHandoff>,
    /// Recommended next calls.
    recommendations: Vec<McpBriefRecommendation>,
    /// Effective limits and truncation metadata.
    limits: McpBriefLimits,
}

/// Additive compact projection of the compatibility-preserving startup brief.
#[derive(Debug, Serialize)]
struct McpCompactSessionBrief {
    /// Selected project identity needed for routing.
    project: McpCompactBriefProject,
    /// Non-default route-affecting startup policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    policy: Option<McpBriefPolicy>,
    /// Compact overview counts when an index exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    overview: Option<McpCompactBriefOverview>,
    /// Folder candidates only when no ready file candidate exists.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    folders: Vec<McpCompactBriefCandidate>,
    /// Ready file candidates for the task.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    files: Vec<McpCompactBriefCandidate>,
    /// Unsafe health blocker count when any exist.
    #[serde(skip_serializing_if = "Option::is_none")]
    blockers: Option<McpCompactBriefBlockers>,
    /// Exact host-owned purpose-curator follow-up when work is actionable.
    #[serde(skip_serializing_if = "Option::is_none")]
    purpose_handoff: Option<McpCompactBriefPurposeHandoff>,
    /// Recommended next calls.
    recommendations: Vec<McpCompactBriefRecommendation>,
    /// Non-default limits and truncation metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    limits: Option<McpCompactBriefLimits>,
}

/// Compact selected-project state for the startup path.
#[derive(Debug, Serialize)]
struct McpCompactBriefProject {
    /// Registered alias used to capture this project state.
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree: Option<String>,
    /// Durable control-catalog registration identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    registration_id: Option<i64>,
    /// Canonical repository root.
    root: Option<String>,
    /// Whether the durable index is available.
    index_status: McpIndexStatus,
}

/// Compact project counts for task startup.
#[derive(Debug, Serialize)]
struct McpCompactBriefOverview {
    /// Number of indexed files.
    files: usize,
    /// Number of indexed folders.
    folders: usize,
}

/// Brief policy fields.
#[derive(Clone, Copy, Debug, Serialize)]
struct McpBriefPolicy {
    /// Whether nearest indexed project routing is enabled by default.
    nearest_project: McpPolicyState,
    /// Absolute-path routing scope.
    path_scope: McpPathScope,
}

impl McpBriefPolicy {
    /// Return whether routing uses the ordinary selected-project policy.
    fn is_default(self) -> bool {
        self.nearest_project == McpPolicyState::Disabled
            && self.path_scope == McpPathScope::SelectedProject
    }
}

/// Bounded ranked candidate row for startup briefs.
#[derive(Debug, Serialize)]
struct McpBriefCandidate {
    /// Repository-relative path.
    path: String,
    /// Indexed node kind.
    kind: String,
    /// Purpose lifecycle status.
    purpose_status: PurposeStatus,
    /// Purpose source.
    purpose_source: PurposeSource,
    /// Whether the purpose is current agent-approved authored responsibility state.
    purpose_agent_reviewed: bool,
    /// Purpose one-liner when present.
    purpose: Option<String>,
    /// Observed content summary when present.
    summary: Option<String>,
    /// Bounded ranking reasons.
    reasons: Vec<String>,
    /// Bounded compact ranking reason codes.
    reason_codes: Vec<RankedReasonCode>,
    /// Sparse stable-order connection counts.
    connection_counts: Vec<RankedConnectionCount>,
    /// Bounded high-value current connection sample.
    connections: Vec<RankedConnection>,
    /// Whether the bounded sample omitted any validated relation through family or global overflow.
    connections_truncated: bool,
    /// Existing navigation capability recommended after this row.
    next_call: NavigationNextCall,
}

/// Bounded compact ranked candidate row for task startup.
#[derive(Debug, Serialize)]
struct McpCompactBriefCandidate {
    /// Repository-relative path.
    path: String,
    /// Purpose lifecycle status when it is not already agent-approved.
    #[serde(skip_serializing_if = "Option::is_none")]
    purpose_status: Option<PurposeStatus>,
    /// Purpose source when it is not already agent-approved.
    #[serde(skip_serializing_if = "Option::is_none")]
    purpose_source: Option<PurposeSource>,
    /// Whether the purpose is current agent-approved authored responsibility state.
    #[serde(skip_serializing_if = "is_false")]
    purpose_agent_reviewed: bool,
    /// Purpose one-liner when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    purpose: Option<String>,
    /// One high-value current connection when available.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    connections: Vec<RankedConnection>,
    /// Whether the bounded sample omitted any validated relation through family or global overflow.
    #[serde(skip_serializing_if = "is_false")]
    connections_truncated: bool,
    /// Existing navigation capability recommended after this row.
    next_call: NavigationNextCall,
}

/// Compact host-owned purpose-curator handoff for startup briefs.
#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct McpCompactBriefPurposeHandoff {
    /// Whether this report is intended for an agent harness.
    #[serde(skip_serializing_if = "is_true")]
    agent_harness_expected: bool,
    /// Recommended host-relative reliable subagent tier selection.
    recommended_subagent_reasoning: &'static str,
    /// Single host-neutral selection instruction retained from the expanded handoff.
    instructions: Vec<String>,
    /// Whether the current main agent may process the same bounded batch.
    #[serde(skip_serializing_if = "is_true")]
    main_agent_fallback: bool,
    /// Explicitly records that `ProjectAtlas` did not spawn a host agent.
    #[serde(skip_serializing_if = "is_false")]
    server_started_curator: bool,
    /// Successful maintenance should not add ordinary conversation output.
    #[serde(skip_serializing_if = "is_true")]
    silent_on_success: bool,
    /// Whether the queue call has more rows after this bounded batch.
    #[serde(skip_serializing_if = "is_false")]
    truncated: bool,
    /// Exact existing MCP call that returns conditional-review tokens and bounded row context.
    next_call: McpCompactBriefRecommendation,
}

/// Bounded health blocker section.
#[derive(Clone, Debug, Serialize)]
struct McpBriefBlockers {
    /// Findings after filters are applied.
    total: usize,
    /// Findings returned in this brief.
    returned: usize,
    /// Whether more blockers exist.
    truncated: bool,
    /// Blocker rows.
    items: Vec<McpBriefBlocker>,
}

/// Compact blocker count; the recommendation carries the exact bounded health call.
#[derive(Debug, Serialize)]
struct McpCompactBriefBlockers {
    /// Findings after filters are applied.
    total: usize,
}

/// Return whether a serialized optional fact is false and can be omitted.
#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_false(value: &bool) -> bool {
    !*value
}

/// Return whether a serialized invariant is true and can be omitted.
#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_true(value: &bool) -> bool {
    *value
}

/// One health blocker row for startup briefs.
#[derive(Clone, Debug, Serialize)]
struct McpBriefBlocker {
    /// Stable finding id.
    id: String,
    /// Finding severity.
    severity: Severity,
    /// Finding category.
    category: String,
    /// Primary path.
    path: String,
    /// Related path when applicable.
    related_path: Option<String>,
    /// Health message.
    message: String,
    /// Recommended agent action.
    recommendation: String,
}

/// One typed startup recommendation.
#[derive(Debug, Serialize)]
struct McpBriefRecommendation {
    /// Stable recommendation kind.
    kind: McpBriefRecommendationKind,
    /// MCP tool name or filesystem/tool family.
    target: String,
    /// Concise machine-readable reason.
    reason: String,
    /// Suggested arguments for the target.
    arguments: serde_json::Value,
}

/// One typed recommendation in the compact startup projection.
#[derive(Debug, Serialize)]
struct McpCompactBriefRecommendation {
    /// Stable recommendation kind.
    kind: McpBriefRecommendationKind,
    /// MCP tool name or filesystem/tool family.
    target: String,
    /// Concise machine-readable reason.
    reason: String,
    /// Suggested arguments for the target.
    arguments: serde_json::Value,
}

/// Startup recommendation kinds.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpBriefRecommendationKind {
    /// Initialize the selected project-local atlas.
    Init,
    /// Inspect the already-ranked file summary.
    Summary,
    /// Search the index when ranking has no directly navigable file.
    Search,
    /// Inspect detailed relations for the already-ranked file.
    Relations,
    /// Inspect structural health.
    Health,
    /// Load one bounded task-scoped purpose-curation queue.
    PurposeQueue,
    /// Read exact source or non-indexed files with normal filesystem tools.
    FilesystemTools,
}

/// Effective startup brief row limits.
#[derive(Debug, Serialize)]
struct McpBriefLimits {
    /// Effective folder row limit.
    folder_limit: usize,
    /// Effective file row limit.
    file_limit: usize,
    /// Effective blocker row limit.
    blocker_limit: usize,
    /// Effective actionable purpose row limit.
    purpose_limit: usize,
    /// Whether folder candidates were truncated.
    folders_truncated: bool,
    /// Whether file candidates were truncated.
    files_truncated: bool,
    /// Whether more actionable low-scope purpose rows exist.
    purposes_truncated: bool,
}

/// Non-default limits and truncation state in the compact startup projection.
#[derive(Debug, Serialize)]
struct McpCompactBriefLimits {
    /// Effective folder row limit.
    #[serde(skip_serializing_if = "is_compact_brief_default_limit")]
    folder_limit: usize,
    /// Effective file row limit.
    #[serde(skip_serializing_if = "is_compact_brief_default_limit")]
    file_limit: usize,
    /// Effective blocker row limit.
    #[serde(skip_serializing_if = "is_compact_brief_default_limit")]
    blocker_limit: usize,
    /// Effective actionable purpose row limit.
    #[serde(skip_serializing_if = "is_compact_brief_default_limit")]
    purpose_limit: usize,
    /// Whether folder candidates were omitted or truncated.
    #[serde(skip_serializing_if = "is_false")]
    folders_truncated: bool,
    /// Whether file candidates were truncated.
    #[serde(skip_serializing_if = "is_false")]
    files_truncated: bool,
    /// Whether more actionable low-scope purpose rows exist.
    #[serde(skip_serializing_if = "is_false")]
    purposes_truncated: bool,
}

/// Return whether a startup row limit is the compact projection default.
#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_compact_brief_default_limit(value: &usize) -> bool {
    *value == COMPACT_SESSION_BRIEF_DEFAULT_LIMIT
}

/// Bounded in-memory registry for MCP task-progress records.
#[derive(Debug, Clone)]
struct McpTaskRegistry {
    /// Session-local task records.
    records: VecDeque<McpTaskRecord>,
}

impl McpTaskRegistry {
    /// Create a registry with the built-in task-progress contract record.
    fn new() -> Self {
        let now = mcp_unix_time_ms();
        let mut registry = Self {
            records: VecDeque::new(),
        };
        registry.insert(McpTaskRecord {
            task_id: MCP_TASK_CONTRACT_ID.to_string(),
            operation: McpTaskOperation::Contract,
            state: McpTaskState::Complete,
            created_at_ms: now,
            updated_at_ms: now,
            progress: Some(McpTaskProgress {
                current: Some(1),
                total: Some(1),
                message: Some(MCP_TASK_PROGRESS_CONTRACT_MESSAGE.to_string()),
            }),
            error: None,
            result_ref: Some(MCP_TOOL_ATLAS_TASK_STATUS.to_string()),
            cancelable: false,
            control: None,
        });
        registry
    }

    /// Insert or replace one task record while preserving the fixed registry capacity.
    fn insert(&mut self, record: McpTaskRecord) {
        if let Some(existing_index) = self
            .records
            .iter()
            .position(|current| current.task_id == record.task_id)
        {
            let _removed = self.records.remove(existing_index);
        }
        while self.records.len() >= MCP_TASK_REGISTRY_CAPACITY {
            if let Some(finished_index) = self
                .records
                .iter()
                .position(McpTaskRecord::is_terminal_state)
            {
                let _evicted = self.records.remove(finished_index);
            } else {
                let _evicted = self.records.pop_front();
            }
        }
        self.records.push_back(record);
    }

    /// Return a task record by id.
    fn get(&self, task_id: &str) -> Option<McpTaskRecord> {
        self.records
            .iter()
            .find(|record| record.task_id == task_id)
            .cloned()
    }

    /// Update a matching task through a bounded mutable pass.
    fn update<F>(&mut self, task_id: &str, update: F) -> Option<McpTaskRecord>
    where
        F: FnOnce(&mut McpTaskRecord),
    {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.task_id == task_id)?;
        update(record);
        Some(record.clone())
    }
}

/// One MCP task-progress record.
#[derive(Debug, Clone, Serialize)]
struct McpTaskRecord {
    /// Opaque session-local task id.
    task_id: String,
    /// Operation family.
    operation: McpTaskOperation,
    /// Current task state.
    state: McpTaskState,
    /// Creation timestamp in Unix milliseconds.
    created_at_ms: u128,
    /// Last update timestamp in Unix milliseconds.
    updated_at_ms: u128,
    /// Optional progress counters/message.
    progress: Option<McpTaskProgress>,
    /// Concise failure diagnostic when present.
    error: Option<String>,
    /// Result reference or follow-up tool when present.
    result_ref: Option<String>,
    /// Whether this task can be canceled by the current server.
    cancelable: bool,
    /// Shared cooperative cancellation boundary for active indexing work.
    #[serde(skip)]
    control: Option<IndexWorkControl>,
}

impl McpTaskRecord {
    /// Return whether this record is in a terminal state and can be evicted first.
    fn is_terminal_state(&self) -> bool {
        matches!(
            self.state,
            McpTaskState::Complete | McpTaskState::Failed | McpTaskState::Canceled
        )
    }
}

/// MCP task operation kind.
#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpTaskOperation {
    /// Contract/schema marker task.
    Contract,
    /// Repository scan and index operation.
    Scan,
    /// One-shot watch refresh operation.
    WatchOnce,
    /// Symbol projection rebuild operation.
    SymbolsBuild,
    /// Future search operation.
    Search,
}

/// MCP task lifecycle state.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpTaskState {
    /// Task has not started.
    Pending,
    /// Task is running.
    Running,
    /// Task completed successfully.
    Complete,
    /// Task failed.
    Failed,
    /// Task was canceled.
    Canceled,
}

/// Optional task progress fields.
#[derive(Debug, Clone, Serialize)]
struct McpTaskProgress {
    /// Completed unit count when known.
    current: Option<u64>,
    /// Total unit count when known.
    total: Option<u64>,
    /// Concise progress message.
    message: Option<String>,
}

/// Task status lookup response.
#[derive(Debug, Serialize)]
struct McpTaskStatusResponse {
    /// Requested task id.
    task_id: String,
    /// Lookup outcome.
    lookup: McpTaskLookupStatus,
    /// Supported task states in this contract.
    states: Vec<McpTaskState>,
    /// Supported operation families in this contract.
    operations: Vec<McpTaskOperation>,
    /// Registry capacity.
    registry_capacity: usize,
    /// Task record when found.
    task: Option<McpTaskRecord>,
}

/// Task lookup outcome.
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpTaskLookupStatus {
    /// The task was found.
    Found,
    /// The task id is unknown to this MCP session.
    NotFound,
}

/// Task cancellation response.
#[derive(Debug, Serialize)]
struct McpTaskCancelResponse {
    /// Requested task id.
    task_id: String,
    /// Cancellation outcome.
    result: McpTaskCancelResult,
    /// Registry capacity.
    registry_capacity: usize,
    /// Task record when found.
    task: Option<McpTaskRecord>,
}

/// Immediate response for one accepted background indexing task.
#[derive(Debug, Serialize)]
struct McpTaskStartResponse {
    /// Opaque session-local task id.
    task_id: String,
    /// Accepted operation family.
    operation: McpTaskOperation,
    /// Initial task state.
    state: McpTaskState,
    /// Existing MCP tool used to poll the task.
    status_tool: &'static str,
    /// Existing MCP tool used to request cancellation.
    cancel_tool: &'static str,
}

/// Task cancellation outcome.
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpTaskCancelResult {
    /// Cooperative cancellation was delivered to active work.
    CancellationRequested,
    /// The task id is unknown to this MCP session.
    NotFound,
    /// The task was already finished.
    AlreadyFinished,
    /// The task exists but cannot currently be canceled.
    NotCancelable,
}

/// Fixed worker partition shared by background indexing tasks in one MCP server.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct McpBackgroundResourceEnvelope {
    /// Maximum concurrently admitted background tasks.
    task_limit: usize,
    /// Maximum scan and parser workers available to each admitted task.
    workers_per_task: usize,
    /// Maximum aggregate workers owned by all admitted tasks.
    total_worker_limit: usize,
}

impl McpBackgroundResourceEnvelope {
    /// Derive one fixed partition from supported host availability and process policy.
    fn for_host() -> Self {
        let available_workers = thread::available_parallelism().map_or(1, usize::from);
        Self::from_available_workers(available_workers)
    }

    /// Derive a deterministic envelope from an observed host worker count.
    fn from_available_workers(available_workers: usize) -> Self {
        let total_worker_limit = available_workers.clamp(1, INDEX_WORKER_SAFE_CEILING);
        let task_limit = total_worker_limit.min(MCP_BACKGROUND_TASK_SAFE_CEILING);
        let workers_per_task = (total_worker_limit / task_limit).max(1);
        Self {
            task_limit,
            workers_per_task,
            total_worker_limit,
        }
    }
}

/// Native `ProjectAtlas` MCP server backed by the same services as the CLI.
#[derive(Debug, Clone)]
pub(crate) struct ProjectAtlasMcpServer {
    /// Immutable registry/control authority selected when the MCP process started.
    control_state: McpProjectState,
    /// Active project state for calls that omit `project_path`.
    project_state: Arc<RwLock<McpProjectState>>,
    /// Caller-visible compatibility label applied to this MCP process's events.
    session: String,
    /// Bounded telemetry lifecycle shared by every server clone and routed project.
    usage_runtime: Arc<Mutex<McpUsageRuntime>>,
    /// Whether absolute path arguments may select the nearest indexed project by default.
    allow_nearest_project: bool,
    /// Bounded MCP task-progress records for this server session.
    task_registry: Arc<RwLock<McpTaskRegistry>>,
    /// Fixed aggregate worker partition for background indexing tasks.
    background_resources: McpBackgroundResourceEnvelope,
    /// Monotonic session-local task identifier source.
    next_task_sequence: Arc<AtomicU64>,
    /// Bounded per-project source observers and verified read epochs.
    source_observations: Arc<SourceObservationRegistry>,
    /// Official RMCP tool router.
    tool_router: ToolRouter<Self>,
}

/// Bridge one RMCP request cancellation token into synchronous index work.
struct McpRequestCancellationBridge {
    /// Signal used to stop the request-local cancellation monitor.
    stop: Arc<std::sync::atomic::AtomicBool>,
    /// Join handle for the bounded request-local monitor thread.
    monitor: Option<thread::JoinHandle<()>>,
    /// Direct request probe retained for synchronous cancellation fences.
    probe: Arc<dyn Fn() -> bool + Send + Sync>,
}

impl McpRequestCancellationBridge {
    /// Start a request-local monitor from one owned RMCP context.
    fn start(
        context: &RequestContext<RoleServer>,
        control: &IndexWorkControl,
    ) -> Result<Self, CliError> {
        let token = context.ct.clone();
        Self::start_with_probe(move || token.is_cancelled(), control)
    }

    /// Start a cancellation monitor from a deterministic probe for tests.
    fn start_with_probe<P>(probe: P, control: &IndexWorkControl) -> Result<Self, CliError>
    where
        P: Fn() -> bool + Send + Sync + 'static,
    {
        let probe: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(probe);
        if probe() {
            control.cancel();
        }
        let observed_control = control.clone();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let monitor_stop = Arc::clone(&stop);
        let monitor_probe = Arc::clone(&probe);
        let monitor = thread::Builder::new()
            .name(MCP_CANCELLATION_MONITOR_THREAD_NAME.to_string())
            .spawn(move || {
                while !monitor_stop.load(Ordering::Acquire) {
                    if monitor_probe() {
                        observed_control.cancel();
                        break;
                    }
                    thread::sleep(std::time::Duration::from_millis(5));
                }
            })
            .map_err(|source| {
                let mut message = MCP_CANCELLATION_MONITOR_START_ERROR_PREFIX.to_string();
                message.push_str(&source.to_string());
                CliError::InvalidInput(message)
            })?;
        Ok(Self {
            stop,
            monitor: Some(monitor),
            probe,
        })
    }

    /// Copy the request token into synchronous index-work cancellation.
    fn synchronize(&self, control: &IndexWorkControl) {
        if (self.probe)() {
            control.cancel();
        }
    }
}

impl Drop for McpRequestCancellationBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(monitor) = self.monitor.take() {
            drop(monitor.join());
        }
    }
}

impl ProjectAtlasMcpServer {
    /// Create a `ProjectAtlas` MCP server instance.
    pub(crate) fn new(
        db_path: PathBuf,
        config_path: Option<PathBuf>,
        session: String,
        allow_nearest_project: bool,
    ) -> Self {
        let startup_state = Self::startup_project_state(db_path, config_path);
        Self {
            control_state: startup_state.clone(),
            project_state: Arc::new(RwLock::new(startup_state)),
            session,
            usage_runtime: Arc::new(Mutex::new(McpUsageRuntime::default())),
            allow_nearest_project,
            task_registry: Arc::new(RwLock::new(McpTaskRegistry::new())),
            background_resources: McpBackgroundResourceEnvelope::for_host(),
            next_task_sequence: Arc::new(AtomicU64::new(1)),
            source_observations: Arc::new(SourceObservationRegistry::default()),
            tool_router: Self::tool_router(),
        }
    }

    /// Open the durable index through one root-bound read snapshot.
    fn open_read_store(state: &McpProjectState) -> Result<AtlasStore, CliError> {
        if !state.db_path.exists() {
            return Err(Self::with_target_error_context(
                index_init_required(&state.root, &state.db_path),
                state,
            ));
        }
        let store = open_atlas_store_read_only_for_project(&state.db_path, &state.root)?;
        Self::require_captured_worktree_identity(state.worktree.as_ref(), &store)?;
        Ok(store)
    }

    /// Require registered worktree aliases to be initialized outside `atlas_init`.
    fn require_initialized_worktree_target(state: &McpProjectState) -> Result<(), CliError> {
        if state.worktree.is_some() && !state.db_path.is_file() {
            return Err(Self::with_target_error_context(
                index_init_required(&state.root, &state.db_path),
                state,
            ));
        }
        Ok(())
    }

    /// Preserve a captured worktree alias on typed init/refresh recovery state.
    fn with_target_error_context(mut error: CliError, state: &McpProjectState) -> CliError {
        let Some(selection) = state.worktree.as_ref() else {
            return error;
        };
        match &mut error {
            CliError::InitRequired(report) => report.worktree = Some(selection.alias.clone()),
            CliError::RefreshRequired(report) => report.worktree = Some(selection.alias.clone()),
            CliError::VerificationIncomplete(report) => {
                report.worktree = Some(selection.alias.clone());
            }
            CliError::ProjectMismatch(report) => {
                report.worktree = Some(selection.alias.clone());
            }
            _ => {}
        }
        error
    }

    /// Run one normal MCP query inside the verified source-epoch boundary.
    #[cfg(test)]
    fn with_fresh_store<T, F>(
        &self,
        state: &McpProjectState,
        query: F,
    ) -> Result<VerifiedReadOutcome<T>, CliError>
    where
        F: FnMut(&AtlasStore, VerifiedReadStamp) -> Result<T, CliError>,
    {
        self.with_fresh_store_for_request(state, None, query)
    }

    /// Run one verified query with optional RMCP request cancellation bridging.
    fn with_fresh_store_for_request<T, F>(
        &self,
        state: &McpProjectState,
        context: Option<RequestContext<RoleServer>>,
        mut query: F,
    ) -> Result<VerifiedReadOutcome<T>, CliError>
    where
        F: FnMut(&AtlasStore, VerifiedReadStamp) -> Result<T, CliError>,
    {
        self.with_fresh_store_controlled_for_request(state, context, |store, stamp, _control| {
            query(store, stamp)
        })
    }

    /// Run one verified query that consumes the request cancellation boundary.
    fn with_fresh_store_controlled_for_request<T, F>(
        &self,
        state: &McpProjectState,
        context: Option<RequestContext<RoleServer>>,
        mut query: F,
    ) -> Result<VerifiedReadOutcome<T>, CliError>
    where
        F: FnMut(&AtlasStore, VerifiedReadStamp, &IndexWorkControl) -> Result<T, CliError>,
    {
        if !state.db_path.is_file() {
            return Err(Self::with_target_error_context(
                index_init_required(&state.root, &state.db_path),
                state,
            ));
        }
        let control =
            index_work_control(&SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None));
        let bridge = context
            .map(|context| McpRequestCancellationBridge::start(&context, &control))
            .transpose()?;
        let result = self
            .source_observations
            .with_verified_read(
                &state.db_path,
                &state.root,
                state.config_path.as_deref(),
                &control,
                |store, stamp| {
                    Self::require_captured_worktree_identity(state.worktree.as_ref(), store)?;
                    query(store, stamp, &control)
                },
            )
            .map_err(|error| Self::with_target_error_context(error, state));
        if let Some(bridge) = bridge.as_ref() {
            bridge.synchronize(&control);
        }
        drop(bridge);
        result
    }

    /// Revalidate one alias selection against the exact opened read snapshot.
    fn require_captured_worktree_identity(
        selection: Option<&McpWorktreeSelection>,
        store: &AtlasStore,
    ) -> Result<(), CliError> {
        let Some(expected) = selection.and_then(|selection| selection.project_instance_id) else {
            return Ok(());
        };
        if store.project_instance_id()? != Some(expected) {
            return Err(CliError::InvalidInput(
                MCP_ERROR_WORKTREE_IDENTITY_CONFLICT.to_string(),
            ));
        }
        Ok(())
    }

    /// Revalidate the control catalog that owns one captured alias registration.
    fn require_captured_control_identity(
        selection: Option<&McpWorktreeSelection>,
        control: &AtlasStore,
    ) -> Result<(), CliError> {
        let Some(expected) = selection.and_then(|selection| selection.control_project_instance_id)
        else {
            return Ok(());
        };
        if control.project_instance_id()? != Some(expected) {
            return Err(CliError::InvalidInput(
                MCP_ERROR_WORKTREE_CONTROL_IDENTITY_CONFLICT.to_string(),
            ));
        }
        Ok(())
    }

    /// Admit only federation snapshots that retain their captured alias identities.
    fn require_federated_worktree_identities(
        stores: Vec<FederatedStore>,
        selections: &[McpWorktreeSelection],
    ) -> Result<Vec<FederatedStore>, CliError> {
        if stores.len() != selections.len() {
            for store in stores {
                drop(store.finish());
            }
            return Err(CliError::InvalidInput(
                MCP_ERROR_FEDERATED_ALIAS_MISSING.to_string(),
            ));
        }
        for (store, selection) in stores.iter().zip(selections) {
            if let Err(error) =
                Self::require_captured_worktree_identity(Some(selection), store.store())
            {
                let error = federated_worktree_error(error, &selection.alias);
                for store in stores {
                    drop(store.finish());
                }
                return Err(error);
            }
        }
        Ok(stores)
    }

    /// Run one rendered MCP query with request cancellation bridged into index work.
    fn with_fresh_string_for_request<F>(
        &self,
        state: &McpProjectState,
        context: Option<RequestContext<RoleServer>>,
        mut query: F,
    ) -> Result<String, CliError>
    where
        F: FnMut(&AtlasStore, VerifiedReadStamp) -> Result<String, CliError>,
    {
        self.with_fresh_string_and_usage_for_request(state, context, |store, stamp| {
            Ok((query(store, stamp)?, None))
        })
    }

    /// Record optional usage only after source and `SQLite` result acceptance succeeds.
    fn with_fresh_string_and_usage_for_request<F>(
        &self,
        state: &McpProjectState,
        context: Option<RequestContext<RoleServer>>,
        mut query: F,
    ) -> Result<String, CliError>
    where
        F: FnMut(
            &AtlasStore,
            VerifiedReadStamp,
        ) -> Result<(String, Option<McpUsageIntent>), CliError>,
    {
        self.with_fresh_string_and_usage_controlled_for_request(
            state,
            context,
            |store, stamp, _control| query(store, stamp),
        )
    }

    /// Render one verified MCP query that consumes request cancellation.
    fn with_fresh_string_and_usage_controlled_for_request<F>(
        &self,
        state: &McpProjectState,
        context: Option<RequestContext<RoleServer>>,
        query: F,
    ) -> Result<String, CliError>
    where
        F: FnMut(
            &AtlasStore,
            VerifiedReadStamp,
            &IndexWorkControl,
        ) -> Result<(String, Option<McpUsageIntent>), CliError>,
    {
        let outcome = self.with_fresh_store_controlled_for_request(state, context, query)?;
        let stamp = outcome.stamp.clone();
        let (value, usage) = outcome.value;
        let output_bytes = value.len();
        let outcome = VerifiedReadOutcome {
            value,
            stamp,
            work: outcome.work,
        }
        .with_output_bytes(output_bytes);
        if let Some(usage) = usage {
            self.record_accepted_usage(state, &outcome.stamp, &usage, &outcome.value);
        }
        Ok(outcome.value)
    }

    /// Open the durable index for mutation and bind any migrated alias identity first.
    fn open_mut_store(
        state: &McpProjectState,
        control_state: &McpProjectState,
    ) -> Result<AtlasStore, CliError> {
        let Some(selection) = state
            .worktree
            .as_ref()
            .filter(|selection| selection.registration_id.is_some())
        else {
            let store = open_atlas_store_for_project(&state.db_path, &state.root)?;
            Self::require_captured_worktree_identity(state.worktree.as_ref(), &store)?;
            return Ok(store);
        };
        Self::open_registered_worktree_mut_store(state, control_state, selection)
    }

    /// Open and, when needed, bind one alias under exact control-writer exclusion.
    fn open_registered_worktree_mut_store(
        state: &McpProjectState,
        control_state: &McpProjectState,
        selection: &McpWorktreeSelection,
    ) -> Result<AtlasStore, CliError> {
        let alias = WorktreeAlias::parse(&selection.alias)?;
        let registration_id =
            selection
                .registration_id
                .ok_or_else(|| DbError::WorktreeRegistrationNotFound {
                    alias: selection.alias.clone(),
                })?;
        let control = open_atlas_store_for_project(&control_state.db_path, &control_state.root)?;
        Self::require_captured_control_identity(Some(selection), &control)?;
        control.with_active_worktree_registration(registration_id, &alias, |guard| {
            if let Err(error) =
                require_registered_worktree_lifecycle(guard.registration(), &state.root)
            {
                return Ok(Err(error));
            }
            if !state.db_path.is_file() {
                return Ok(Err(Self::with_target_error_context(
                    index_init_required(&state.root, &state.db_path),
                    state,
                )));
            }
            let target = match open_atlas_store_for_project(&state.db_path, &state.root) {
                Ok(store) => store,
                Err(error) => return Ok(Err(error)),
            };
            let project = match target.captured_project_binding() {
                Ok(binding) => binding.project_instance_id,
                Err(error) => return Ok(Err(error.into())),
            };
            if selection
                .project_instance_id
                .is_some_and(|expected| expected != project)
            {
                return Ok(Err(CliError::InvalidInput(
                    MCP_ERROR_WORKTREE_IDENTITY_CONFLICT.to_string(),
                )));
            }
            match guard.registration().project_instance_id {
                Some(bound) if bound == project => {}
                Some(_) => {
                    return Ok(Err(CliError::InvalidInput(
                        MCP_ERROR_WORKTREE_IDENTITY_CONFLICT.to_string(),
                    )));
                }
                None => {
                    let snapshot = match target.export_worktree_usage_snapshot() {
                        Ok(snapshot) => snapshot,
                        Err(error) => return Ok(Err(error.into())),
                    };
                    if let Err(error) =
                        require_registered_worktree_lifecycle(guard.registration(), &state.root)
                    {
                        return Ok(Err(error));
                    }
                    if let Err(error) = require_current_worktree_usage_snapshot(
                        &state.db_path,
                        &state.root,
                        &snapshot,
                    ) {
                        return Ok(Err(error));
                    }
                    guard.bind_project_with_usage_snapshot(&state.root, project, &snapshot)?;
                }
            }
            Ok(Ok(target))
        })?
    }

    /// Open an existing selected-project index for purpose or health mutation.
    fn open_existing_mut_store(
        state: &McpProjectState,
        control_state: &McpProjectState,
    ) -> Result<AtlasStore, CliError> {
        if !state.db_path.is_file() {
            return Err(Self::with_target_error_context(
                index_init_required(&state.root, &state.db_path),
                state,
            ));
        }
        Self::open_mut_store(state, control_state)
    }

    /// Apply one purpose mutation under a source witness retained through commit.
    fn with_admitted_purpose_mutation<T>(
        &self,
        state: &McpProjectState,
        context: Option<RequestContext<RoleServer>>,
        mutation: impl FnOnce(&AtlasStore) -> Result<T, CliError>,
    ) -> Result<T, CliError> {
        if !state.db_path.is_file() {
            return Err(Self::with_target_error_context(
                index_init_required(&state.root, &state.db_path),
                state,
            ));
        }
        let control =
            index_work_control(&SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None));
        let bridge = context
            .map(|context| McpRequestCancellationBridge::start(&context, &control))
            .transpose()?;
        let result = self
            .with_admitted_purpose_mutation_controlled(state, &control, bridge.as_ref(), mutation)
            .map_err(|error| Self::with_target_error_context(error, state));
        drop(bridge);
        result
    }

    /// Apply one admitted mutation with explicit cancellation and rollback boundaries.
    fn with_admitted_purpose_mutation_controlled<T>(
        &self,
        state: &McpProjectState,
        control: &IndexWorkControl,
        bridge: Option<&McpRequestCancellationBridge>,
        mutation: impl FnOnce(&AtlasStore) -> Result<T, CliError>,
    ) -> Result<T, CliError> {
        if let Some(bridge) = bridge {
            bridge.synchronize(control);
        }
        let admission = self.source_observations.admit_mutation(
            &state.db_path,
            &state.root,
            state.config_path.as_deref(),
            control,
        )?;
        let store = Self::open_existing_mut_store(state, &self.control_state)?;
        Self::require_captured_worktree_identity(state.worktree.as_ref(), &store)?;
        let transaction = store.begin_purpose_mutation()?;
        let operation = (|| {
            let value = mutation(&store)?;
            if let Some(bridge) = bridge {
                bridge.synchronize(control);
            }
            admission.verify()?;
            if let Some(bridge) = bridge {
                bridge.synchronize(control);
            }
            control.check(IndexWorkStage::Publication)?;
            Ok(value)
        })();
        match operation {
            Ok(value) => {
                transaction.commit()?;
                Ok(value)
            }
            Err(operation) => Err(crate::rollback_rejected_purpose_mutation(
                transaction,
                operation,
            )),
        }
    }

    /// Return whether this MCP process can record optional telemetry.
    fn telemetry_enabled() -> bool {
        !telemetry_disabled()
    }

    /// Record telemetry and rotate only this project's scope if baselines are full.
    fn record_usage_for_state<F>(&self, state: &McpProjectState, store: &AtlasStore, record: F)
    where
        F: FnMut(UsageRuntimeInstance) -> Result<(), CliError>,
    {
        self.record_usage_for_origin(state, store, None, record);
    }

    /// Record telemetry under one exact authority and optional worktree origin.
    fn record_usage_for_origin<F>(
        &self,
        state: &McpProjectState,
        store: &AtlasStore,
        worktree_registration_id: Option<i64>,
        mut record: F,
    ) where
        F: FnMut(UsageRuntimeInstance) -> Result<(), CliError>,
    {
        if telemetry_disabled() {
            return;
        }
        let Ok(binding) =
            McpUsageProjectBinding::capture_with_origin(state, store, worktree_registration_id)
        else {
            return;
        };
        let Some(project_instance) = self
            .usage_runtime
            .lock()
            .ok()
            .and_then(|mut runtime| runtime.instance_for_binding(binding, store))
        else {
            return;
        };
        let Ok(mut usage_instance) = project_instance.lock() else {
            return;
        };
        if !matches!(
            record(*usage_instance),
            Err(CliError::Db(DbError::TelemetryBaselineCapacity))
        ) {
            return;
        }

        let Some(next_instance) = UsageRuntimeInstance::new(UsageInstanceOwner::McpProcess) else {
            return;
        };
        // Creating a candidate has no lifecycle effect; install it only after
        // the old identity is durably sealed so failure cannot leak or replace it.
        if usage_instance.seal(store).is_err() {
            return;
        }
        *usage_instance = next_instance;
        drop(record(next_instance));
    }

    /// Best-effort telemetry for one result whose source epoch has already been accepted.
    fn record_accepted_usage(
        &self,
        state: &McpProjectState,
        stamp: &VerifiedReadStamp,
        intent: &McpUsageIntent,
        output: &str,
    ) {
        if !Self::telemetry_enabled() {
            return;
        }
        let Ok(store) = Self::open_read_store(state) else {
            return;
        };
        let Ok(binding) = store.captured_project_binding() else {
            return;
        };
        if binding.project_instance_id != stamp.project_instance_id {
            return;
        }
        if let Some(selection) = state
            .worktree
            .as_ref()
            .filter(|selection| selection.registration_id.is_some())
        {
            let Some(registration_id) = selection.registration_id else {
                return;
            };
            let event = match &intent.baseline {
                McpUsageBaseline::Estimate(baseline_tokens) => usage_from_estimates_with_context(
                    &self.session,
                    intent.command,
                    intent.path.clone(),
                    intent.query.clone(),
                    *baseline_tokens,
                    projectatlas_core::outline::estimate_tokens(output),
                    TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
                    TOKEN_BASELINE_SELECTED_CANDIDATES,
                    TOKEN_CONFIDENCE_INFERRED,
                ),
                McpUsageBaseline::DirectoryWalk(baseline_tokens) => {
                    usage_from_estimates_with_context(
                        &self.session,
                        intent.command,
                        intent.path.clone(),
                        intent.query.clone(),
                        *baseline_tokens,
                        projectatlas_core::outline::estimate_tokens(output),
                        TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
                        TOKEN_BASELINE_DIRECTORY_WALK,
                        TOKEN_CONFIDENCE_POLICY_ESTIMATE,
                    )
                }
                McpUsageBaseline::Text(baseline_text) => usage_from_text(
                    &self.session,
                    intent.command,
                    intent.path.clone(),
                    intent.query.clone(),
                    baseline_text,
                    output,
                ),
            };
            let Ok(control) = Self::open_read_store(&self.control_state) else {
                return;
            };
            if Self::require_captured_control_identity(Some(selection), &control).is_err() {
                return;
            }
            if control.finish_index_read_snapshot().is_ok() {
                self.record_usage_for_origin(
                    &self.control_state,
                    &control,
                    Some(registration_id),
                    |usage_instance| {
                        usage_instance.record_for_worktree(&control, registration_id, &event)
                    },
                );
            }
            return;
        }
        self.record_usage_for_state(state, &store, |usage_instance| match &intent.baseline {
            McpUsageBaseline::Estimate(baseline_tokens) => record_usage_estimate(
                &store,
                Some(usage_instance),
                &self.session,
                intent.command,
                intent.path.clone(),
                intent.query.clone(),
                *baseline_tokens,
                output,
            ),
            McpUsageBaseline::DirectoryWalk(baseline_tokens) => {
                record_directory_walk_usage_estimate(
                    &store,
                    Some(usage_instance),
                    &self.session,
                    intent.command,
                    intent.path.clone(),
                    intent.query.clone(),
                    *baseline_tokens,
                    output,
                )
            }
            McpUsageBaseline::Text(baseline_text) => record_usage_text(
                &store,
                Some(usage_instance),
                &self.session,
                intent.command,
                intent.path.clone(),
                intent.query.clone(),
                baseline_text,
                output,
            ),
        });
    }

    /// Reuse an optional broad-source telemetry baseline within one complete generation.
    fn estimated_source_tokens_cached(
        &self,
        state: &McpProjectState,
        store: &AtlasStore,
        stamp: &VerifiedReadStamp,
        folder: Option<&str>,
        file_pattern: Option<&str>,
    ) -> Result<usize, CliError> {
        let key = McpSourceTokenBaselineKey {
            binding: McpUsageProjectBinding {
                root: state.root.clone(),
                db_path: state.db_path.clone(),
                project_instance_id: stamp.project_instance_id,
                worktree_registration_id: None,
            },
            generation: stamp.generation,
            folder: folder.map(ToOwned::to_owned),
            file_pattern: file_pattern.map(ToOwned::to_owned),
        };
        if let Some(value) = self
            .usage_runtime
            .lock()
            .ok()
            .and_then(|runtime| runtime.source_token_baseline(&key))
        {
            return Ok(value);
        }
        let value = estimated_source_tokens_for_indexed_files(store, folder, file_pattern)?;
        if let Ok(mut runtime) = self.usage_runtime.lock() {
            runtime.insert_source_token_baseline(key, value);
        }
        Ok(value)
    }

    /// Best-effort seal each selected project's current identity at shutdown.
    fn seal_usage_instances_for_projects(&self) {
        let Some(projects) = self
            .usage_runtime
            .lock()
            .ok()
            .map(|runtime| runtime.snapshot())
        else {
            return;
        };
        for project in projects {
            let Ok(instance) = project.instance.lock() else {
                continue;
            };
            if let Ok(store) =
                open_atlas_store_for_project(&project.binding.db_path, &project.binding.root)
            {
                drop(instance.seal(&store));
            }
        }
    }

    /// Load effective atlas config for the selected state.
    fn load_config_for_state(state: &McpProjectState) -> Result<AtlasMapConfig, CliError> {
        state
            .config_path
            .as_deref()
            .map_or_else(
                || load_atlas_config_for_root(&state.root).map_err(CliError::from),
                |config_path| load_atlas_config(Some(config_path)).map_err(CliError::from),
            )
            .map(|config| config.with_database_path(&state.db_path))
    }

    /// Return the selected project root used by initialization.
    fn init_project_root(
        &self,
        project_path: Option<String>,
        worktree: Option<String>,
    ) -> Result<McpProjectState, CliError> {
        self.state_for_target_with_config_validation(
            project_path,
            worktree,
            McpConfigValidation::Immediate,
        )
    }

    /// Initialize one registered worktree, preferring a reconciled control-atlas baseline.
    fn run_registered_worktree_init(
        &self,
        state: &McpProjectState,
        config_path: &Path,
        options: &InitBootstrapOptions,
    ) -> Result<InitSetupReport, CliError> {
        let Some(selection) = state
            .worktree
            .as_ref()
            .filter(|selection| selection.registration_id.is_some())
        else {
            let report =
                run_init_bootstrap(&state.root, &state.db_path, Some(config_path), options)?;
            if report.ok {
                self.bind_initialized_registration_for_root(state)?;
            }
            return Ok(report);
        };
        if state.db_path.is_file() {
            let mut report =
                run_init_bootstrap(&state.root, &state.db_path, Some(config_path), options)?;
            report.hydration = Some(InitHydrationPhase {
                status: InitHydrationStatus::Existing,
                source_root: None,
                source_project_instance_id: None,
                target_project_instance_id: None,
                baseline_generation: None,
                reconciled_generation: None,
                fallback_reason: None,
            });
            if report.ok {
                self.bind_initialized_worktree(selection, state)?;
            }
            return Ok(report);
        }

        let project_dir = state.root.join(PROJECTATLAS_DIR_NAME);
        let nonsource_file = project_dir.join(MCP_NONSOURCE_FILE_NAME);
        let project_dir_existed = project_dir.exists();
        let config_existed = config_path.exists();
        let nonsource_existed = nonsource_file.exists();
        init_project_with_config(&state.root, Some(config_path))?;

        let hydration = if options.no_scan {
            McpWorktreeHydration::Fallback(MCP_HYDRATION_NO_SCAN_REASON.to_string())
        } else {
            self.attempt_worktree_hydration(state, config_path, options)?
        };
        let mut report = match hydration {
            McpWorktreeHydration::Activated { hydration, scan } => {
                let mut report = run_init_bootstrap(
                    &state.root,
                    &state.db_path,
                    Some(config_path),
                    &InitBootstrapOptions {
                        no_scan: true,
                        force_rescan: options.force_rescan,
                        text_index_max_bytes: options.text_index_max_bytes,
                    },
                )?;
                report.scan = InitScanPhase {
                    status: InitPhaseStatus::Verified,
                    requested: true,
                    force_rescan: options.force_rescan,
                    report: Some(*scan),
                    error: None,
                };
                report.next_steps =
                    init_next_steps(false, false, report.purpose_handoff.queue.total);
                report.hydration = Some(hydration);
                report
            }
            McpWorktreeHydration::Fallback(reason) => {
                let mut report =
                    run_init_bootstrap(&state.root, &state.db_path, Some(config_path), options)?;
                report.hydration = Some(InitHydrationPhase {
                    status: InitHydrationStatus::Fallback,
                    source_root: lossless_project_root_display(&self.control_state.root),
                    source_project_instance_id: None,
                    target_project_instance_id: None,
                    baseline_generation: None,
                    reconciled_generation: None,
                    fallback_reason: Some(reason),
                });
                report
            }
        };
        report.project_dir.status = if project_dir_existed {
            InitPhaseStatus::Exists
        } else {
            InitPhaseStatus::Created
        };
        report.config.status = if config_existed {
            InitPhaseStatus::Exists
        } else {
            InitPhaseStatus::Created
        };
        report.nonsource_files.status = if nonsource_existed {
            InitPhaseStatus::Exists
        } else {
            InitPhaseStatus::Created
        };
        report.db.status = InitPhaseStatus::Created;
        if report.ok {
            self.bind_initialized_worktree(selection, state)?;
        }
        Ok(report)
    }

    /// Build, reconcile, and no-clobber activate one private hydration candidate.
    fn attempt_worktree_hydration(
        &self,
        state: &McpProjectState,
        config_path: &Path,
        options: &InitBootstrapOptions,
    ) -> Result<McpWorktreeHydration, CliError> {
        let selection = state
            .worktree
            .as_ref()
            .filter(|selection| selection.registration_id.is_some())
            .ok_or_else(|| CliError::InvalidInput(MCP_ERROR_FEDERATED_ALIAS_MISSING.to_string()))?;
        let source = match Self::open_read_store(&self.control_state) {
            Ok(source) => source,
            Err(error) if Self::hydration_can_fallback(&error) => {
                return Ok(McpWorktreeHydration::Fallback(error.to_string()));
            }
            Err(error) => return Err(error),
        };
        Self::require_captured_control_identity(state.worktree.as_ref(), &source)?;
        let symbol_options = SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None);
        let control = index_work_control(&symbol_options);
        let mut candidate =
            match source.prepare_worktree_hydration(&state.root, &state.db_path, &control) {
                Ok(candidate) => candidate,
                Err(error) => {
                    let error = Self::hydration_db_error(error);
                    if Self::hydration_can_fallback(&error) {
                        return Ok(McpWorktreeHydration::Fallback(error.to_string()));
                    }
                    return Err(error);
                }
            };
        let candidate_path = candidate
            .path()
            .map_err(Self::hydration_db_error)?
            .to_path_buf();
        let mut target = open_atlas_store_for_project(&candidate_path, &state.root)?;
        let plan = ScanRuntimePlan::for_path_controlled(
            Some(config_path),
            &state.root,
            options.text_index_max_bytes,
            &control,
        )?;
        let (scan, source_unchanged) =
            reconcile_hydrated_index_controlled(&mut target, &plan, &symbol_options, &control)?;
        drop(target);
        if source_unchanged {
            candidate
                .accept_verified_source_state(&control)
                .map_err(Self::hydration_db_error)?;
        }
        drop(source);
        let candidate = candidate
            .prepare_activation(&control)
            .map_err(Self::hydration_db_error)?;
        let activation = match self
            .activate_registered_worktree_hydration(state, selection, candidate, &control)
        {
            Ok(activation) => activation,
            Err(CliError::Db(error @ DbError::WorktreeHydrationDestinationExists { .. })) => {
                return Ok(McpWorktreeHydration::Fallback(error.to_string()));
            }
            Err(error) => return Err(error),
        };
        Ok(McpWorktreeHydration::Activated {
            hydration: InitHydrationPhase {
                status: InitHydrationStatus::Hydrated,
                source_root: lossless_project_root_display(&self.control_state.root),
                source_project_instance_id: Some(activation.source_project_instance_id.to_string()),
                target_project_instance_id: Some(activation.target_project_instance_id.to_string()),
                baseline_generation: Some(activation.baseline_generation.get()),
                reconciled_generation: Some(activation.reconciled_generation.get()),
                fallback_reason: None,
            },
            scan: Box::new(scan),
        })
    }

    /// Publish one prepared hydration candidate only for the still-exact registered lifecycle.
    fn activate_registered_worktree_hydration(
        &self,
        state: &McpProjectState,
        selection: &McpWorktreeSelection,
        candidate: PreparedWorktreeHydrationCandidate,
        work_control: &IndexWorkControl,
    ) -> Result<WorktreeHydrationActivation, CliError> {
        self.activate_registered_worktree_hydration_with_post_publication(
            state,
            selection,
            candidate,
            work_control,
            || Ok(()),
        )
    }

    /// Publish and bind a verified candidate with one deterministic final-admission seam.
    fn activate_registered_worktree_hydration_with_post_publication<F>(
        &self,
        state: &McpProjectState,
        selection: &McpWorktreeSelection,
        candidate: PreparedWorktreeHydrationCandidate,
        work_control: &IndexWorkControl,
        post_publication: F,
    ) -> Result<WorktreeHydrationActivation, CliError>
    where
        F: FnOnce() -> Result<(), CliError>,
    {
        let alias = WorktreeAlias::parse(&selection.alias)?;
        let registration_id =
            selection
                .registration_id
                .ok_or_else(|| DbError::WorktreeRegistrationNotFound {
                    alias: selection.alias.clone(),
                })?;
        let control = Self::open_existing_mut_store(&self.control_state, &self.control_state)?;
        Self::require_captured_control_identity(Some(selection), &control)?;
        control.with_active_worktree_registration(registration_id, &alias, |guard| {
            if let Err(error) =
                require_registered_worktree_lifecycle(guard.registration(), &state.root)
            {
                return Ok(Err(error));
            }
            let activation = candidate.activate(work_control)?;
            if let Err(error) = post_publication() {
                return Ok(Err(error));
            }
            let target =
                match open_atlas_store_read_only_for_project(&activation.database, &state.root) {
                    Ok(target) => target,
                    Err(error) => return Ok(Err(error)),
                };
            let snapshot = match target.export_worktree_usage_snapshot() {
                Ok(snapshot) => snapshot,
                Err(error) => return Ok(Err(error.into())),
            };
            if let Err(error) =
                require_registered_worktree_lifecycle(guard.registration(), &state.root)
            {
                return Ok(Err(error));
            }
            if let Err(error) = require_current_worktree_usage_snapshot(
                &activation.database,
                &state.root,
                &snapshot,
            ) {
                return Ok(Err(error));
            }
            guard.bind_project_with_usage_snapshot(
                &state.root,
                activation.target_project_instance_id,
                &snapshot,
            )?;
            Ok(Ok(activation))
        })?
    }

    /// Bind a successfully initialized alias to its exact local atlas identity.
    fn bind_initialized_worktree(
        &self,
        selection: &McpWorktreeSelection,
        state: &McpProjectState,
    ) -> Result<(), CliError> {
        drop(Self::open_registered_worktree_mut_store(
            state,
            &self.control_state,
            selection,
        )?);
        Ok(())
    }

    /// Bind a legacy exact-path init when its root already has an active alias.
    fn bind_initialized_registration_for_root(
        &self,
        state: &McpProjectState,
    ) -> Result<(), CliError> {
        let Some(repository) = self.control_git_repository_if_present()? else {
            return Ok(());
        };
        let Some(entry) = repository
            .worktrees
            .iter()
            .find(|entry| Self::active_worktree_root(entry).is_some_and(|root| root == state.root))
        else {
            return Ok(());
        };
        if !Self::worktree_registration_paths_are_utf8(&repository.common_directory, entry) {
            return Ok(());
        }
        let administrative_directory =
            normalize_native_path_display(&entry.administrative_directory);
        let administrative_identity = git_administrative_identity(&entry.administrative_directory)?;
        let control = Self::open_existing_mut_store(&self.control_state, &self.control_state)?;
        let control_project_instance_id = control.captured_project_binding()?.project_instance_id;
        let Some(registration) =
            control
                .worktree_registrations(false)?
                .into_iter()
                .find(|registration| {
                    registration.git_administrative_directory == administrative_directory
                        && registration.git_administrative_identity == administrative_identity
                })
        else {
            return Ok(());
        };
        self.bind_initialized_worktree(
            &McpWorktreeSelection {
                alias: registration.alias.to_string(),
                registration_id: Some(registration.registration_id),
                project_instance_id: registration.project_instance_id,
                control_project_instance_id: Some(control_project_instance_id),
            },
            state,
        )
    }

    /// Preserve cancellation/resource failures and fallback only for unusable source baselines.
    fn hydration_can_fallback(error: &CliError) -> bool {
        matches!(
            error,
            CliError::Db(
                DbError::WorktreeHydrationInvalid { .. }
                    | DbError::WorktreeHydrationDestinationExists { .. }
                    | DbError::WorktreeHydrationBackupBusy { .. }
                    | DbError::GraphPublicationUnavailable
                    | DbError::GraphProjectIdentityMismatch { .. }
                    | DbError::DerivedSnapshotInvalid { .. }
                    | DbError::DerivedSnapshotLimit { .. }
                    | DbError::SchemaVersion { .. }
                    | DbError::SchemaVersionMissing
                    | DbError::SchemaShape { .. }
                    | DbError::IntegrityCheck { .. }
                    | DbError::ProjectRootMissing
                    | DbError::ProjectInstanceIdentityMissing
            )
        )
    }

    /// Lift database-owned cancellation into the shared CLI work-control error.
    fn hydration_db_error(error: DbError) -> CliError {
        match error {
            DbError::IndexWork(failure) => failure.into(),
            error => error.into(),
        }
    }

    /// Return the initialized selected project root used by admin-style MCP calls.
    fn admin_project_root(
        &self,
        project_path: Option<String>,
        worktree: Option<String>,
    ) -> Result<McpProjectState, CliError> {
        self.state_for_target(project_path, worktree)
    }

    /// Parse an MCP ignore kind parameter.
    fn parse_ignore_kind(
        kind: Option<&str>,
        required: bool,
    ) -> Result<Option<IgnoreEntryKind>, CliError> {
        let Some(kind) = kind.map(str::trim).filter(|value| !value.is_empty()) else {
            return if required {
                Err(CliError::InvalidInput(
                    MCP_ERROR_IGNORE_KIND_REQUIRED.to_string(),
                ))
            } else {
                Ok(None)
            };
        };
        match kind {
            MCP_IGNORE_KIND_DIR_NAME | MCP_IGNORE_KIND_DIR_NAME_ALIAS => {
                Ok(Some(IgnoreEntryKind::DirName))
            }
            MCP_IGNORE_KIND_PATH_PREFIX | MCP_IGNORE_KIND_PATH_PREFIX_ALIAS => {
                Ok(Some(IgnoreEntryKind::PathPrefix))
            }
            other => Err(CliError::InvalidInput(Self::invalid_parameter_message(
                MCP_ERROR_INVALID_IGNORE_KIND_PREFIX,
                other,
                MCP_ERROR_INVALID_IGNORE_KIND_SUFFIX,
            ))),
        }
    }

    /// Parse an MCP purpose lint level parameter.
    fn parse_purpose_lint_level(value: Option<&str>) -> Result<PurposeLintLevel, CliError> {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            None | Some(MCP_PURPOSE_LEVEL_LOW) => Ok(PurposeLintLevel::Low),
            Some(MCP_PURPOSE_LEVEL_MEDIUM) => Ok(PurposeLintLevel::Medium),
            Some(MCP_PURPOSE_LEVEL_STRICT) => Ok(PurposeLintLevel::Strict),
            Some(other) => Err(CliError::InvalidInput(Self::invalid_parameter_message(
                MCP_ERROR_INVALID_PURPOSE_LEVEL_PREFIX,
                other,
                MCP_ERROR_INVALID_PURPOSE_LEVEL_SUFFIX,
            ))),
        }
    }

    /// Parse an MCP harness config parameter.
    fn parse_harness_config(value: Option<&str>) -> Result<HarnessConfig, CliError> {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            None | Some(MCP_HARNESS_MCP_JSON | MCP_HARNESS_MCP_JSON_ALIAS) => {
                Ok(HarnessConfig::McpJson)
            }
            Some(MCP_HARNESS_CODEX) => Ok(HarnessConfig::Codex),
            Some(MCP_HARNESS_CLAUDE_CODE | MCP_HARNESS_CLAUDE_CODE_ALIAS) => {
                Ok(HarnessConfig::ClaudeCode)
            }
            Some(MCP_HARNESS_OPENCODE) => Ok(HarnessConfig::OpenCode),
            Some(other) => Err(CliError::InvalidInput(Self::invalid_parameter_message(
                MCP_ERROR_INVALID_HARNESS_PREFIX,
                other,
                MCP_ERROR_INVALID_HARNESS_SUFFIX,
            ))),
        }
    }

    /// Parse the optional token chart theme parameter.
    fn parse_token_chart_theme(value: Option<&str>) -> Result<TokenDashboardTheme, CliError> {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            None => Ok(TokenDashboardTheme::Dark),
            Some(theme) => TokenDashboardTheme::parse(theme).ok_or_else(|| {
                CliError::InvalidInput(Self::invalid_parameter_message(
                    TOKEN_CHART_THEME_ERROR_PREFIX,
                    theme,
                    TOKEN_CHART_THEME_ERROR_SUFFIX,
                ))
            }),
        }
    }

    /// Build an invalid-parameter diagnostic from centralized fragments.
    fn invalid_parameter_message(prefix: &str, value: &str, suffix: &str) -> String {
        let mut message = String::with_capacity(prefix.len() + value.len() + suffix.len());
        message.push_str(prefix);
        message.push_str(value);
        message.push_str(suffix);
        message
    }

    /// Build a compatibility map report, writing the map unless CI skip policy applies.
    fn build_map_report(
        state: &McpProjectState,
        json: bool,
        force: bool,
    ) -> Result<McpMapReport, CliError> {
        let config = Self::load_config_for_state(state)?;
        let skipped_reason = if !force
            && (crate::truthy_env(MCP_ENV_CI) || crate::truthy_env(MCP_ENV_GITHUB_ACTIONS))
        {
            Some(MCP_MAP_SKIPPED_IN_CI_REASON.to_string())
        } else {
            write_map(&config, json)?;
            None
        };
        Ok(McpMapReport {
            root: lossless_project_root_display(&config.root),
            map_path: lossless_native_path_display(&config.map_path),
            written: skipped_reason.is_none(),
            json,
            skipped_reason,
        })
    }

    /// Build typed MCP session capabilities from active server state.
    fn session_capabilities(
        &self,
        state: &McpProjectState,
        text_index_max_bytes: u64,
    ) -> McpSessionCapabilities {
        McpSessionCapabilities {
            runtime: build_runtime_info(),
            selected_project: Self::selected_project_capability(state),
            startup_policy: McpStartupPolicy {
                nearest_project: Self::policy_state(self.allow_nearest_project),
            },
            path_scope: self.path_scope(),
            scan_policy: McpScanPolicy {
                implicit_scan: McpPolicyState::Disabled,
                text_index_max_bytes,
            },
            classified_navigation: classified_navigation_capabilities(),
            telemetry: McpTelemetryPolicy {
                mode: Self::policy_state(!telemetry_disabled()),
            },
            privacy: McpPrivacyPolicy {
                environment_dump: false,
                secret_values: false,
                projectatlas_paths_only: true,
            },
        }
    }

    /// Encode settings plus additive MCP-session capability fields.
    fn render_settings_with_capabilities(
        &self,
        state: &McpProjectState,
    ) -> Result<String, CliError> {
        let report = build_settings_report(
            &state.db_path,
            state.config_path.as_deref(),
            OutputFormat::Toon,
        )?;
        let capabilities = self.session_capabilities(state, report.text_index_max_bytes);
        let rendered = Self::encode_two_named_payloads(
            MCP_PAYLOAD_SETTINGS,
            &report,
            MCP_PAYLOAD_SESSION_CAPABILITIES,
            &capabilities,
        )?;
        if rendered.len() > MCP_SETTINGS_RESPONSE_MAX_BYTES {
            let mut message = MCP_SETTINGS_RESPONSE_LIMIT_PREFIX.to_string();
            message.push_str(&rendered.len().to_string());
            message.push_str(MCP_SETTINGS_RESPONSE_LIMIT_SEPARATOR);
            message.push_str(&MCP_SETTINGS_RESPONSE_MAX_BYTES.to_string());
            message.push_str(MCP_SETTINGS_RESPONSE_LIMIT_SUFFIX);
            return Err(CliError::InvalidInput(message));
        }
        Ok(rendered)
    }

    /// Build the selected-project capability row.
    fn selected_project_capability(state: &McpProjectState) -> McpSelectedProjectCapability {
        McpSelectedProjectCapability {
            worktree: state
                .worktree
                .as_ref()
                .map(|selection| selection.alias.clone()),
            registration_id: state
                .worktree
                .as_ref()
                .and_then(|selection| selection.registration_id),
            root: lossless_project_root_display(&state.root),
            db: lossless_native_path_display(&state.db_path),
            config: state
                .config_path
                .as_ref()
                .and_then(|path| lossless_native_path_display(path)),
            index_status: if state.db_path.exists() {
                McpIndexStatus::Available
            } else {
                McpIndexStatus::Missing
            },
        }
    }

    /// Return the path scope for this MCP server.
    fn path_scope(&self) -> McpPathScope {
        if self.allow_nearest_project {
            McpPathScope::NearestIndexedProject
        } else {
            McpPathScope::SelectedProject
        }
    }

    /// Convert a bool into a serialized policy state.
    fn policy_state(enabled: bool) -> McpPolicyState {
        if enabled {
            McpPolicyState::Enabled
        } else {
            McpPolicyState::Disabled
        }
    }

    /// Clamp a caller-provided brief limit to the supported range.
    fn brief_limit(value: Option<usize>) -> usize {
        value
            .unwrap_or(SESSION_BRIEF_DEFAULT_LIMIT)
            .clamp(1, SESSION_BRIEF_MAX_LIMIT)
    }

    /// Build a read-only agent startup brief.
    fn build_session_brief(
        &self,
        params: AtlasSessionBriefParams,
        context: Option<RequestContext<RoleServer>>,
    ) -> Result<McpSessionBrief, CliError> {
        let selected_project_path = params.project_path.clone();
        let selected_worktree = params.worktree.clone();
        let state =
            self.state_for_target(selected_project_path.clone(), selected_worktree.clone())?;
        let query = Self::query_or_empty(params.query);
        let purpose_task = params
            .purpose_task
            .unwrap_or_else(|| MCP_PURPOSE_TASK_SESSION_STARTUP.to_string());
        let folder_limit = Self::brief_limit(params.folder_limit);
        let file_limit = Self::brief_limit(params.file_limit);
        let blocker_limit = Self::brief_limit(params.blocker_limit);
        let purpose_limit = Self::brief_limit(params.purpose_limit);
        let project = Self::selected_project_capability(&state);
        if !state.db_path.exists() {
            let init_project_path = selected_worktree
                .is_none()
                .then(|| lossless_project_root_display(&state.root))
                .flatten();
            return Ok(McpSessionBrief {
                project,
                policy: self.brief_policy(),
                overview: None,
                folders: Vec::new(),
                files: Vec::new(),
                blockers: McpBriefBlockers {
                    total: 0,
                    returned: 0,
                    truncated: false,
                    items: Vec::new(),
                },
                purpose_handoff: None,
                recommendations: Self::missing_index_recommendations(
                    init_project_path,
                    selected_worktree,
                ),
                limits: McpBriefLimits {
                    folder_limit,
                    file_limit,
                    blocker_limit,
                    purpose_limit,
                    folders_truncated: false,
                    files_truncated: false,
                    purposes_truncated: false,
                },
            });
        }
        let outcome = self.with_fresh_store_for_request(&state, context, |store, _stamp| {
            let overview = store.overview()?;
            let folder_rows =
                ranked_folder_nodes_with_reasons(store, &query, folder_limit.saturating_add(1))?;
            let file_rows = ranked_file_nodes_with_reasons(
                store,
                &query,
                None,
                None,
                file_limit.saturating_add(1),
                false,
            )?;
            let blockers = Self::brief_blockers(store, blocker_limit)?;
            let purpose_query = HealthQuery {
                start_index: 0,
                limit: purpose_limit,
                category: None,
                severity: None,
                path_prefix: None,
                summary_only: false,
                scope: HealthScope::purpose_default(),
            };
            let purpose_queue = purpose_curation_page(store, &purpose_query, &purpose_task)?;
            let purposes_truncated = purpose_queue.truncated;
            let folders_truncated = folder_rows.len() > folder_limit;
            let files_truncated = file_rows.len() > file_limit;
            let next_navigation_call = file_rows.first().map(|row| row.next_call.clone());
            Ok(McpSessionBrief {
                project: Self::selected_project_capability(&state),
                policy: self.brief_policy(),
                overview: Some(overview),
                folders: folder_rows
                    .into_iter()
                    .take(folder_limit)
                    .map(Self::brief_candidate)
                    .collect(),
                files: file_rows
                    .into_iter()
                    .take(file_limit)
                    .map(Self::brief_candidate)
                    .collect(),
                recommendations: Self::indexed_project_recommendations(
                    &query,
                    next_navigation_call,
                    blockers.total,
                    blocker_limit,
                    selected_project_path.clone(),
                    selected_worktree.clone(),
                ),
                blockers,
                purpose_handoff: purpose_queue
                    .actionable
                    .then(|| purpose_curator_handoff(purpose_queue)),
                limits: McpBriefLimits {
                    folder_limit,
                    file_limit,
                    blocker_limit,
                    purpose_limit,
                    folders_truncated,
                    files_truncated,
                    purposes_truncated,
                },
            })
        })?;
        Ok(outcome.value)
    }

    /// Build brief policy fields.
    fn brief_policy(&self) -> McpBriefPolicy {
        McpBriefPolicy {
            nearest_project: Self::policy_state(self.allow_nearest_project),
            path_scope: self.path_scope(),
        }
    }

    /// Build the additive compact projection without changing legacy defaults or query behavior.
    fn build_compact_session_brief(
        &self,
        mut params: AtlasSessionBriefParams,
        context: Option<RequestContext<RoleServer>>,
    ) -> Result<McpCompactSessionBrief, CliError> {
        let project_path = params.project_path.clone();
        let worktree = params.worktree.clone();
        params.compact = None;
        params
            .folder_limit
            .get_or_insert(COMPACT_SESSION_BRIEF_DEFAULT_LIMIT);
        params
            .file_limit
            .get_or_insert(COMPACT_SESSION_BRIEF_DEFAULT_LIMIT);
        params
            .blocker_limit
            .get_or_insert(COMPACT_SESSION_BRIEF_DEFAULT_LIMIT);
        params
            .purpose_limit
            .get_or_insert(COMPACT_SESSION_BRIEF_DEFAULT_LIMIT);
        let brief = self.build_session_brief(params, context)?;
        Ok(Self::compact_session_brief(
            &brief,
            project_path.as_deref(),
            worktree.as_deref(),
        ))
    }

    /// Project the compatibility report into the explicit compact response shape.
    fn compact_session_brief(
        brief: &McpSessionBrief,
        project_path: Option<&str>,
        worktree: Option<&str>,
    ) -> McpCompactSessionBrief {
        let omit_folders = !brief.files.is_empty();
        let folders_truncated =
            brief.limits.folders_truncated || (omit_folders && !brief.folders.is_empty());
        let compact_limits = McpCompactBriefLimits {
            folder_limit: brief.limits.folder_limit,
            file_limit: brief.limits.file_limit,
            blocker_limit: brief.limits.blocker_limit,
            purpose_limit: brief.limits.purpose_limit,
            folders_truncated,
            files_truncated: brief.limits.files_truncated,
            purposes_truncated: brief.limits.purposes_truncated,
        };
        let limits_are_default = is_compact_brief_default_limit(&compact_limits.folder_limit)
            && is_compact_brief_default_limit(&compact_limits.file_limit)
            && is_compact_brief_default_limit(&compact_limits.blocker_limit)
            && is_compact_brief_default_limit(&compact_limits.purpose_limit)
            && !compact_limits.folders_truncated
            && !compact_limits.files_truncated
            && !compact_limits.purposes_truncated;
        McpCompactSessionBrief {
            project: McpCompactBriefProject {
                worktree: brief.project.worktree.clone(),
                registration_id: brief.project.registration_id,
                root: brief.project.root.clone(),
                index_status: brief.project.index_status,
            },
            policy: (!brief.policy.is_default()).then_some(brief.policy),
            overview: brief
                .overview
                .as_ref()
                .map(|overview| McpCompactBriefOverview {
                    files: overview.files,
                    folders: overview.folders,
                }),
            folders: if omit_folders {
                Vec::new()
            } else {
                brief
                    .folders
                    .iter()
                    .map(Self::compact_brief_candidate)
                    .collect()
            },
            files: brief
                .files
                .iter()
                .map(Self::compact_brief_candidate)
                .collect(),
            blockers: (brief.blockers.total > 0).then_some(McpCompactBriefBlockers {
                total: brief.blockers.total,
            }),
            purpose_handoff: brief.purpose_handoff.as_ref().map(|handoff| {
                Self::compact_brief_purpose_handoff(
                    handoff,
                    project_path.map(ToString::to_string),
                    worktree.map(ToString::to_string),
                )
            }),
            recommendations: brief
                .recommendations
                .iter()
                .map(Self::compact_brief_recommendation)
                .collect(),
            limits: (!limits_are_default).then_some(compact_limits),
        }
    }

    /// Convert a ranked node into the compatibility-preserving startup candidate.
    fn brief_candidate(row: RankedNode) -> McpBriefCandidate {
        let purpose_agent_reviewed = row.node.purpose.agent_reviewed();
        McpBriefCandidate {
            path: row.node.node.path,
            kind: row.node.node.kind.to_string(),
            purpose_status: row.node.purpose.status,
            purpose_source: row.node.purpose.source,
            purpose_agent_reviewed,
            purpose: row.node.purpose.purpose,
            summary: row.node.summary,
            reasons: row.reasons,
            reason_codes: row.reason_codes,
            connection_counts: row.connection_counts,
            connections: row.connections,
            connections_truncated: row.connections_truncated,
            next_call: row.next_call,
        }
    }

    /// Project one compatibility candidate into the bounded compact shape.
    fn compact_brief_candidate(row: &McpBriefCandidate) -> McpCompactBriefCandidate {
        let connections = row
            .connections
            .iter()
            .find(|connection| Self::brief_connection_is_crisp(connection))
            .cloned()
            .into_iter()
            .collect::<Vec<_>>();
        let next_call = if row.next_call.capability == NavigationNextCapability::Relations {
            NavigationNextCall {
                capability: NavigationNextCapability::Summary,
                path: row.path.clone(),
            }
        } else {
            row.next_call.clone()
        };
        McpCompactBriefCandidate {
            path: row.path.clone(),
            purpose_status: (!row.purpose_agent_reviewed).then_some(row.purpose_status),
            purpose_source: (!row.purpose_agent_reviewed).then_some(row.purpose_source),
            purpose_agent_reviewed: row.purpose_agent_reviewed,
            purpose: row.purpose.clone(),
            connections_truncated: row.connections_truncated
                || connections.len() < row.connections.len(),
            connections,
            next_call,
        }
    }

    /// Prefer a resolved non-import edge as the single default startup connection sample.
    fn brief_connection_is_crisp(connection: &RankedConnection) -> bool {
        connection.kind != RankedConnectionKind::Import
            && !matches!(
                &connection.target,
                RankedConnectionTarget::Unresolved { .. }
            )
    }

    /// Project an actionable handoff into one exact follow-up call instead of duplicating rows.
    fn compact_brief_purpose_handoff(
        handoff: &PurposeCuratorHandoff,
        project_path: Option<String>,
        worktree: Option<String>,
    ) -> McpCompactBriefPurposeHandoff {
        McpCompactBriefPurposeHandoff {
            agent_harness_expected: handoff.agent_harness_expected,
            recommended_subagent_reasoning: handoff.recommended_subagent_reasoning,
            instructions: handoff.instructions.first().cloned().into_iter().collect(),
            main_agent_fallback: handoff.main_agent_fallback,
            server_started_curator: handoff.server_started_curator,
            silent_on_success: handoff.silent_on_success,
            truncated: handoff.queue.truncated,
            next_call: McpCompactBriefRecommendation {
                kind: McpBriefRecommendationKind::PurposeQueue,
                target: MCP_TOOL_ATLAS_PURPOSE_QUEUE.to_string(),
                reason: MCP_BRIEF_REASON_PURPOSE_QUEUE.to_string(),
                arguments: Self::brief_call_arguments(
                    project_path,
                    worktree,
                    &[(MCP_BRIEF_ARG_TASK, &handoff.queue.task)],
                    Some((MCP_BRIEF_ARG_LIMIT, handoff.queue.limit)),
                ),
            },
        }
    }

    /// Add compact opt-ins to one legacy recommendation without changing its source report.
    fn compact_brief_recommendation(
        recommendation: &McpBriefRecommendation,
    ) -> McpCompactBriefRecommendation {
        let relation_to_summary =
            matches!(recommendation.kind, McpBriefRecommendationKind::Relations);
        let mut arguments = recommendation.arguments.clone();
        if let Some(object) = arguments.as_object_mut() {
            if relation_to_summary {
                object.remove(MCP_BRIEF_ARG_VIEW);
            }
            if relation_to_summary
                || matches!(recommendation.kind, McpBriefRecommendationKind::Summary)
            {
                object.insert(
                    MCP_BRIEF_ARG_COMPACT.to_string(),
                    serde_json::Value::Bool(true),
                );
            }
        }
        McpCompactBriefRecommendation {
            kind: if relation_to_summary {
                McpBriefRecommendationKind::Summary
            } else {
                recommendation.kind
            },
            target: if relation_to_summary {
                MCP_TOOL_ATLAS_FILE_SUMMARY.to_string()
            } else {
                recommendation.target.clone()
            },
            reason: if relation_to_summary {
                MCP_BRIEF_REASON_RANKED_FILE_SUMMARY.to_string()
            } else {
                recommendation.reason.clone()
            },
            arguments,
        }
    }

    /// Build bounded health blockers for a session brief.
    fn brief_blockers(
        store: &AtlasStore,
        blocker_limit: usize,
    ) -> Result<McpBriefBlockers, CliError> {
        let query = HealthQuery {
            start_index: 0,
            limit: blocker_limit,
            category: None,
            severity: None,
            path_prefix: None,
            summary_only: false,
            scope: HealthScope::all(),
        };
        let page = store.unresolved_health_findings_page_current(&query)?;
        let total = page.total;
        let returned = page.returned;
        Ok(McpBriefBlockers {
            total,
            returned,
            truncated: returned < total,
            items: page
                .findings
                .into_iter()
                .map(|finding| McpBriefBlocker {
                    id: finding.id,
                    severity: finding.severity,
                    category: finding.category,
                    path: finding.path,
                    related_path: finding.related_path,
                    message: finding.message,
                    recommendation: finding.recommendation,
                })
                .collect(),
        })
    }

    /// Recommend next calls for a missing index.
    fn missing_index_recommendations(
        project_path: Option<String>,
        worktree: Option<String>,
    ) -> Vec<McpBriefRecommendation> {
        vec![
            McpBriefRecommendation {
                kind: McpBriefRecommendationKind::Init,
                target: MCP_TOOL_ATLAS_INIT.to_string(),
                reason: MCP_BRIEF_REASON_SELECTED_INDEX_MISSING.to_string(),
                arguments: Self::target_arguments(project_path.clone(), worktree.clone()),
            },
            McpBriefRecommendation {
                kind: McpBriefRecommendationKind::FilesystemTools,
                target: MCP_BRIEF_TARGET_FILESYSTEM_TOOLS.to_string(),
                reason: MCP_BRIEF_REASON_FILESYSTEM_UNTIL_INDEX.to_string(),
                arguments: Self::target_arguments(project_path, worktree),
            },
        ]
    }

    /// Recommend next calls for an indexed project.
    fn indexed_project_recommendations(
        query: &str,
        next_navigation_call: Option<NavigationNextCall>,
        blocker_total: usize,
        blocker_limit: usize,
        project_path: Option<String>,
        worktree: Option<String>,
    ) -> Vec<McpBriefRecommendation> {
        let mut recommendations = match next_navigation_call {
            Some(next_call) if next_call.capability == NavigationNextCapability::Summary => {
                vec![McpBriefRecommendation {
                    kind: McpBriefRecommendationKind::Summary,
                    target: MCP_TOOL_ATLAS_FILE_SUMMARY.to_string(),
                    reason: MCP_BRIEF_REASON_RANKED_FILE_SUMMARY.to_string(),
                    arguments: Self::brief_call_arguments(
                        project_path.clone(),
                        worktree.clone(),
                        &[(MCP_BRIEF_ARG_FILE, &next_call.path)],
                        None,
                    ),
                }]
            }
            Some(next_call) if next_call.capability == NavigationNextCapability::Relations => {
                vec![McpBriefRecommendation {
                    kind: McpBriefRecommendationKind::Relations,
                    target: MCP_TOOL_ATLAS_SYMBOL_RELATIONS.to_string(),
                    reason: MCP_BRIEF_REASON_RANKED_FILE_RELATIONS.to_string(),
                    arguments: Self::brief_call_arguments(
                        project_path.clone(),
                        worktree.clone(),
                        &[
                            (MCP_BRIEF_ARG_FILE, &next_call.path),
                            (MCP_BRIEF_ARG_VIEW, MCP_SYMBOL_RELATION_VIEW_DETAILED),
                        ],
                        None,
                    ),
                }]
            }
            _ if !query.trim().is_empty() => vec![McpBriefRecommendation {
                kind: McpBriefRecommendationKind::Search,
                target: MCP_TOOL_ATLAS_SEARCH.to_string(),
                reason: MCP_BRIEF_REASON_SEARCH_FALLBACK.to_string(),
                arguments: Self::brief_call_arguments(
                    project_path.clone(),
                    worktree.clone(),
                    &[(MCP_BRIEF_ARG_PATTERN, query)],
                    None,
                ),
            }],
            _ => vec![McpBriefRecommendation {
                kind: McpBriefRecommendationKind::FilesystemTools,
                target: MCP_BRIEF_TARGET_FILESYSTEM_TOOLS.to_string(),
                reason: MCP_BRIEF_REASON_NO_FILE_CANDIDATE.to_string(),
                arguments: Self::target_arguments(project_path.clone(), worktree.clone()),
            }],
        };
        if blocker_total > 0 {
            recommendations.push(McpBriefRecommendation {
                kind: McpBriefRecommendationKind::Health,
                target: MCP_TOOL_ATLAS_HEALTH.to_string(),
                reason: MCP_BRIEF_REASON_HEALTH_BLOCKERS.to_string(),
                arguments: Self::brief_call_arguments(
                    project_path,
                    worktree,
                    &[],
                    Some((MCP_BRIEF_ARG_LIMIT, blocker_limit)),
                ),
            });
        }
        recommendations
    }

    /// Build a `JSON` object containing one mutually exclusive root selector.
    fn target_arguments(
        project_path: Option<String>,
        worktree: Option<String>,
    ) -> serde_json::Value {
        let mut arguments = serde_json::Map::new();
        if let Some(path) = project_path {
            arguments.insert(
                MCP_BRIEF_ARG_PROJECT_PATH.to_string(),
                serde_json::Value::String(path),
            );
        } else if let Some(alias) = worktree {
            arguments.insert(
                MCP_BRIEF_ARG_WORKTREE.to_string(),
                serde_json::Value::String(alias),
            );
        }
        serde_json::Value::Object(arguments)
    }

    /// Build recommendation call arguments with optional project path and one payload argument.
    fn brief_call_arguments(
        project_path: Option<String>,
        worktree: Option<String>,
        string_args: &[(&'static str, &str)],
        usize_arg: Option<(&'static str, usize)>,
    ) -> serde_json::Value {
        let mut arguments = serde_json::Map::new();
        if let Some(path) = project_path {
            arguments.insert(
                MCP_BRIEF_ARG_PROJECT_PATH.to_string(),
                serde_json::Value::String(path),
            );
        } else if let Some(alias) = worktree {
            arguments.insert(
                MCP_BRIEF_ARG_WORKTREE.to_string(),
                serde_json::Value::String(alias),
            );
        }
        for (key, value) in string_args {
            arguments.insert(
                (*key).to_string(),
                serde_json::Value::String((*value).to_string()),
            );
        }
        if let Some((key, value)) = usize_arg {
            arguments.insert(key.to_string(), serde_json::json!(value));
        }
        serde_json::Value::Object(arguments)
    }

    /// Return all task model states for contract discovery.
    fn task_state_values() -> Vec<McpTaskState> {
        vec![
            McpTaskState::Pending,
            McpTaskState::Running,
            McpTaskState::Complete,
            McpTaskState::Failed,
            McpTaskState::Canceled,
        ]
    }

    /// Return all task operation values for contract discovery.
    fn task_operation_values() -> Vec<McpTaskOperation> {
        vec![
            McpTaskOperation::Contract,
            McpTaskOperation::Scan,
            McpTaskOperation::WatchOnce,
            McpTaskOperation::SymbolsBuild,
            McpTaskOperation::Search,
        ]
    }

    /// Start one bounded session-local indexing task.
    fn start_index_task<F>(
        &self,
        operation: McpTaskOperation,
        options: SymbolBuildOptions,
        result_ref: &'static str,
        work: F,
    ) -> Result<McpTaskStartResponse, CliError>
    where
        F: FnOnce(&IndexWorkControl, SymbolBuildOptions) -> Result<(), CliError> + Send + 'static,
    {
        let options = options.with_worker_ceiling(self.background_resources.workers_per_task);
        let control = index_work_control(&options);
        let mut task_id = MCP_INDEX_TASK_ID_PREFIX.to_string();
        task_id.push_str(
            &self
                .next_task_sequence
                .fetch_add(1, Ordering::Relaxed)
                .to_string(),
        );
        let now = mcp_unix_time_ms();
        {
            let mut registry = self
                .task_registry
                .write()
                .map_err(|_poisoned| CliError::Mcp(MCP_PROJECT_STATE_LOCK_POISONED.to_string()))?;
            let active = registry
                .records
                .iter()
                .filter(|record| !record.is_terminal_state())
                .count();
            if active >= self.background_resources.task_limit {
                let mut message = MCP_INDEX_TASK_LIMIT_PREFIX.to_string();
                message.push_str(&self.background_resources.task_limit.to_string());
                message.push_str(MCP_INDEX_TASK_LIMIT_SUFFIX);
                return Err(CliError::Mcp(message));
            }
            registry.insert(McpTaskRecord {
                task_id: task_id.clone(),
                operation: operation.clone(),
                state: McpTaskState::Pending,
                created_at_ms: now,
                updated_at_ms: now,
                progress: Some(McpTaskProgress {
                    current: None,
                    total: None,
                    message: Some(MCP_TASK_PROGRESS_ACCEPTED.to_string()),
                }),
                error: None,
                result_ref: None,
                cancelable: true,
                control: Some(control.clone()),
            });
        }

        let registry = Arc::clone(&self.task_registry);
        let worker_task_id = task_id.clone();
        let mut worker_name = MCP_INDEX_WORKER_NAME_PREFIX.to_string();
        worker_name.push_str(&task_id);
        let spawn_result = thread::Builder::new().name(worker_name).spawn(move || {
            if let Ok(mut registry) = registry.write() {
                registry.update(&worker_task_id, |record| {
                    record.state = McpTaskState::Running;
                    record.updated_at_ms = mcp_unix_time_ms();
                    record.progress = Some(McpTaskProgress {
                        current: None,
                        total: None,
                        message: Some(MCP_TASK_PROGRESS_RUNNING.to_string()),
                    });
                });
            }
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&control, options)));
            let (state, progress, error, completed_result_ref) = match outcome {
                Ok(Ok(())) => (
                    McpTaskState::Complete,
                    MCP_TASK_PROGRESS_COMPLETE,
                    None,
                    Some(result_ref.to_string()),
                ),
                Ok(Err(error)) => {
                    let state = if task_error_is_canceled(&error) {
                        McpTaskState::Canceled
                    } else {
                        McpTaskState::Failed
                    };
                    (
                        state,
                        if state == McpTaskState::Canceled {
                            MCP_TASK_PROGRESS_CANCELED
                        } else {
                            MCP_TASK_PROGRESS_FAILED
                        },
                        Some(bounded_task_error(&error)),
                        None,
                    )
                }
                Err(_panic) => (
                    McpTaskState::Failed,
                    MCP_TASK_PROGRESS_FAILED,
                    Some(MCP_INDEX_WORKER_PANIC_ERROR.to_string()),
                    None,
                ),
            };
            if let Ok(mut registry) = registry.write() {
                registry.update(&worker_task_id, |record| {
                    record.state = state;
                    record.updated_at_ms = mcp_unix_time_ms();
                    record.progress = Some(McpTaskProgress {
                        current: None,
                        total: None,
                        message: Some(progress.to_string()),
                    });
                    record.error = error;
                    record.result_ref = completed_result_ref;
                    record.cancelable = false;
                    record.control = None;
                });
            }
        });
        if let Err(source) = spawn_result {
            let mut spawn_error = MCP_INDEX_WORKER_SPAWN_ERROR_PREFIX.to_string();
            spawn_error.push_str(&source.to_string());
            if let Ok(mut registry) = self.task_registry.write() {
                registry.update(&task_id, |record| {
                    record.state = McpTaskState::Failed;
                    record.updated_at_ms = mcp_unix_time_ms();
                    record.progress = Some(McpTaskProgress {
                        current: None,
                        total: None,
                        message: Some(MCP_TASK_PROGRESS_FAILED.to_string()),
                    });
                    record.error = Some(spawn_error.clone());
                    record.cancelable = false;
                    record.control = None;
                });
            }
            return Err(CliError::Mcp(spawn_error));
        }

        Ok(McpTaskStartResponse {
            task_id,
            operation,
            state: McpTaskState::Pending,
            status_tool: MCP_TOOL_ATLAS_TASK_STATUS,
            cancel_tool: MCP_TOOL_ATLAS_TASK_CANCEL,
        })
    }

    /// Look up one MCP task status.
    fn task_status(&self, task_id: String) -> Result<McpTaskStatusResponse, CliError> {
        let registry = self
            .task_registry
            .read()
            .map_err(|_poisoned| CliError::Mcp(MCP_TASK_REGISTRY_LOCK_POISONED.to_string()))?;
        let task = registry.get(&task_id);
        Ok(McpTaskStatusResponse {
            task_id,
            lookup: if task.is_some() {
                McpTaskLookupStatus::Found
            } else {
                McpTaskLookupStatus::NotFound
            },
            states: Self::task_state_values(),
            operations: Self::task_operation_values(),
            registry_capacity: MCP_TASK_REGISTRY_CAPACITY,
            task,
        })
    }

    /// Cancel one MCP task when cancellation is supported.
    fn task_cancel(&self, task_id: String) -> Result<McpTaskCancelResponse, CliError> {
        let mut registry = self
            .task_registry
            .write()
            .map_err(|_poisoned| CliError::Mcp(MCP_TASK_REGISTRY_LOCK_POISONED.to_string()))?;
        let Some(record) = registry.get(&task_id) else {
            return Ok(McpTaskCancelResponse {
                task_id,
                result: McpTaskCancelResult::NotFound,
                registry_capacity: MCP_TASK_REGISTRY_CAPACITY,
                task: None,
            });
        };
        if matches!(
            record.state,
            McpTaskState::Complete | McpTaskState::Failed | McpTaskState::Canceled
        ) {
            return Ok(McpTaskCancelResponse {
                task_id,
                result: McpTaskCancelResult::AlreadyFinished,
                registry_capacity: MCP_TASK_REGISTRY_CAPACITY,
                task: Some(record),
            });
        }
        if !record.cancelable {
            return Ok(McpTaskCancelResponse {
                task_id,
                result: McpTaskCancelResult::NotCancelable,
                registry_capacity: MCP_TASK_REGISTRY_CAPACITY,
                task: Some(record),
            });
        }
        let task = registry.update(&task_id, |record| {
            if let Some(control) = &record.control {
                control.cancel();
            }
            record.updated_at_ms = mcp_unix_time_ms();
            record.progress = Some(McpTaskProgress {
                current: None,
                total: None,
                message: Some(MCP_TASK_PROGRESS_CANCELLATION_REQUESTED.to_string()),
            });
            record.cancelable = false;
        });
        Ok(McpTaskCancelResponse {
            task_id,
            result: McpTaskCancelResult::CancellationRequested,
            registry_capacity: MCP_TASK_REGISTRY_CAPACITY,
            task,
        })
    }

    /// Build a CLI-compatible lint report for MCP callers.
    fn lint_report_for_state(
        state: &McpProjectState,
        params: &AtlasLintParams,
    ) -> Result<crate::runtime::LintReport, CliError> {
        let config = Self::load_config_for_state(state)?;
        let purpose_level = Self::parse_purpose_lint_level(params.purpose_level.as_deref())?;
        lint_project(
            &config,
            &state.db_path,
            state.config_path.as_deref(),
            LintOptions {
                strict_folders: params.strict_folders.unwrap_or(false),
                report_untracked: params.report_untracked.unwrap_or(false),
                strict_untracked: params.strict_untracked.unwrap_or(false),
            },
            purpose_level,
        )
    }

    /// Build startup state from CLI-supplied DB/config paths.
    fn startup_project_state(db_path: PathBuf, config_path: Option<PathBuf>) -> McpProjectState {
        let root = Self::startup_project_root(&db_path, config_path.as_deref());
        let config_path = config_path.filter(|path| Self::config_matches_project_root(&root, path));
        McpProjectState {
            root,
            db_path,
            config_path,
            worktree: None,
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
        let state = self
            .project_state
            .read()
            .map(|state| state.clone())
            .map_err(|_poisoned| CliError::Mcp(MCP_PROJECT_STATE_LOCK_POISONED.to_string()))?;
        canonical_source_project_root(&state.root)?;
        Ok(state)
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

    /// Return the bounded reciprocal Git inventory for the immutable control checkout.
    fn control_git_repository(&self) -> Result<GitRepositoryStructure, CliError> {
        self.control_git_repository_if_present()?.ok_or_else(|| {
            CliError::InvalidInput(MCP_ERROR_WORKTREE_CONTROL_REPOSITORY_REQUIRED.to_string())
        })
    }

    /// Return the control Git inventory, or none for a true non-Git control root.
    fn control_git_repository_if_present(
        &self,
    ) -> Result<Option<GitRepositoryStructure>, CliError> {
        let repository = match discover_repository_structure(&self.control_state.root)? {
            RepositoryStructure::Git(repository) => repository,
            RepositoryStructure::NonGit { .. } => return Ok(None),
            RepositoryStructure::InvalidGit { issue, .. } => {
                return Err(CliError::InvalidInput(format!(
                    "invalid control-repository Git evidence at '{}': {:?}",
                    normalize_native_path_display(issue.path),
                    issue.kind
                )));
            }
        };
        let control_present = repository.worktrees.iter().any(|entry| {
            matches!(
                &entry.state,
                GitWorktreeState::Active { root, .. } if root == &self.control_state.root
            )
        });
        if !control_present {
            return Err(CliError::InvalidInput(format!(
                "selected control root '{}' is not one active worktree in its reciprocal Git inventory",
                normalize_native_path_display(&self.control_state.root)
            )));
        }
        Ok(Some(repository))
    }

    /// Derive one stable, short candidate selector from Git administrative identity.
    fn worktree_candidate_selector(entry: &GitWorktreeEntry) -> String {
        if let Some(identity) = entry.administrative_directory.to_str() {
            return Self::worktree_candidate_selector_from_identity(
                &normalize_native_path_display_str(identity),
            );
        }
        Self::worktree_candidate_selector_from_native_identity(
            entry
                .administrative_directory
                .as_os_str()
                .as_encoded_bytes(),
        )
    }

    /// Derive one stable, short candidate selector from normalized Git identity text.
    fn worktree_candidate_selector_from_identity(identity: &str) -> String {
        Self::worktree_candidate_selector_from_native_identity(identity.as_bytes())
    }

    /// Derive a selector from exact native administrative-directory bytes.
    fn worktree_candidate_selector_from_native_identity(identity: &[u8]) -> String {
        let digest = blake3::hash(identity).to_hex();
        let mut selector = String::with_capacity(
            MCP_WORKTREE_SELECTOR_PREFIX.len() + MCP_WORKTREE_SELECTOR_DIGEST_CHARS,
        );
        selector.push_str(MCP_WORKTREE_SELECTOR_PREFIX);
        selector.push_str(&digest.as_str()[..MCP_WORKTREE_SELECTOR_DIGEST_CHARS]);
        selector
    }

    /// Borrow one active source root from structural worktree evidence.
    fn active_worktree_root(entry: &GitWorktreeEntry) -> Option<&Path> {
        match &entry.state {
            GitWorktreeState::Active { root, .. } => Some(root),
            GitWorktreeState::Missing { .. } | GitWorktreeState::Invalid { .. } => None,
        }
    }

    /// Return whether `SQLite` text and MCP JSON can preserve this identity exactly.
    fn worktree_registration_paths_are_utf8(
        common_directory: &Path,
        entry: &GitWorktreeEntry,
    ) -> bool {
        common_directory.to_str().is_some()
            && entry.administrative_directory.to_str().is_some()
            && Self::active_worktree_root(entry).is_none_or(|root| root.to_str().is_some())
    }

    /// Return whether one short selector identifies this structural candidate.
    fn worktree_candidate_matches(entry: &GitWorktreeEntry, selector: &str) -> bool {
        if Self::worktree_candidate_selector(entry) == selector {
            return true;
        }
        let root_name = Self::active_worktree_root(entry)
            .and_then(Path::file_name)
            .and_then(std::ffi::OsStr::to_str);
        let administrative_name = entry
            .administrative_directory
            .file_name()
            .and_then(std::ffi::OsStr::to_str);
        root_name.is_some_and(|name| name.eq_ignore_ascii_case(selector))
            || administrative_name.is_some_and(|name| name.eq_ignore_ascii_case(selector))
    }

    /// Return bounded active non-control candidates for one stable or human selector.
    fn matching_worktree_candidates<'a>(
        &'a self,
        repository: &'a GitRepositoryStructure,
        selector: &str,
    ) -> Vec<&'a GitWorktreeEntry> {
        repository
            .worktrees
            .iter()
            .filter(|entry| {
                Self::worktree_registration_paths_are_utf8(&repository.common_directory, entry)
            })
            .filter(|entry| {
                Self::active_worktree_root(entry)
                    .is_some_and(|root| root != self.control_state.root)
            })
            .filter(|entry| Self::worktree_candidate_matches(entry, selector))
            .collect()
    }

    /// Revalidate one selected candidate and its filesystem lifecycle before registration.
    fn revalidate_worktree_candidate(
        &self,
        expected_repository: &GitRepositoryStructure,
        expected_entry: &GitWorktreeEntry,
        expected_identity: &str,
    ) -> Result<(GitRepositoryStructure, GitWorktreeEntry), CliError> {
        let expected_root = Self::active_worktree_root(expected_entry).ok_or_else(|| {
            CliError::InvalidInput(MCP_ERROR_WORKTREE_NO_LONGER_ACTIVE.to_string())
        })?;
        let repository = self.control_git_repository()?;
        let entry = repository
            .worktrees
            .iter()
            .find(|entry| entry.administrative_directory == expected_entry.administrative_directory)
            .cloned()
            .ok_or_else(|| {
                CliError::InvalidInput(MCP_ERROR_WORKTREE_LIFECYCLE_CHANGED.to_string())
            })?;
        let root = Self::active_worktree_root(&entry).ok_or_else(|| {
            CliError::InvalidInput(MCP_ERROR_WORKTREE_LIFECYCLE_CHANGED.to_string())
        })?;
        let identity = git_administrative_identity(&entry.administrative_directory)?;
        if repository.common_directory != expected_repository.common_directory
            || entry.role != expected_entry.role
            || root != expected_root
            || identity != expected_identity
        {
            return Err(CliError::InvalidInput(
                MCP_ERROR_WORKTREE_LIFECYCLE_CHANGED.to_string(),
            ));
        }
        Ok((repository, entry))
    }

    /// Build one concise add-candidate row from active structural evidence.
    fn worktree_candidate(
        common_directory: &Path,
        entry: &GitWorktreeEntry,
    ) -> Option<McpWorktreeCandidate> {
        if !Self::worktree_registration_paths_are_utf8(common_directory, entry) {
            return None;
        }
        Some(McpWorktreeCandidate {
            selector: Self::worktree_candidate_selector(entry),
            root: lossless_project_root_display(Self::active_worktree_root(entry)?),
            role: entry.role.into(),
        })
    }

    /// Derive a valid default alias from one selected active worktree root.
    fn default_worktree_alias(root: &Path) -> Result<WorktreeAlias, CliError> {
        let name = root
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or_else(|| CliError::InvalidInput(MCP_ERROR_WORKTREE_PATH_NON_UTF8.to_string()))?;
        WorktreeAlias::parse(&name.to_ascii_lowercase()).map_err(|source| {
            CliError::InvalidInput(format!(
                "selected worktree directory cannot be used as an alias; provide alias explicitly: {source}"
            ))
        })
    }

    /// Open one exact local worktree atlas without creating or migrating it.
    fn open_local_worktree_atlas(root: &Path) -> Result<Option<AtlasStore>, CliError> {
        let db_path = Self::projectatlas_db_path(root);
        if !db_path.exists() {
            return Ok(None);
        }
        Ok(Some(open_atlas_store_read_only_for_project(
            &db_path, root,
        )?))
    }

    /// Require the exact project identity from one already-open local atlas.
    fn local_worktree_project_instance_id(
        store: &AtlasStore,
        db_path: &Path,
    ) -> Result<ProjectInstanceId, CliError> {
        let project_instance_id = store.project_instance_id()?.ok_or_else(|| {
            CliError::InvalidInput(format!(
                "worktree atlas '{}' has no exact project identity",
                normalize_native_path_display(db_path)
            ))
        })?;
        Ok(project_instance_id)
    }

    /// Export one local usage snapshot bound to an independently read project identity.
    fn local_worktree_usage_snapshot(
        store: &AtlasStore,
        db_path: &Path,
        project_instance_id: ProjectInstanceId,
    ) -> Result<WorktreeUsageSnapshot, CliError> {
        let snapshot = store.export_worktree_usage_snapshot()?;
        if snapshot.project_instance_id() != project_instance_id {
            return Err(CliError::InvalidInput(format!(
                "worktree atlas '{}' telemetry identity does not match its project identity",
                normalize_native_path_display(db_path)
            )));
        }
        Ok(snapshot)
    }

    /// Read one exact local worktree atlas and its bounded telemetry snapshot without migration.
    fn local_worktree_atlas(root: &Path) -> Result<Option<LocalWorktreeAtlas>, CliError> {
        let db_path = Self::projectatlas_db_path(root);
        let Some(store) = Self::open_local_worktree_atlas(root)? else {
            return Ok(None);
        };
        let project_instance_id = Self::local_worktree_project_instance_id(&store, &db_path)?;
        let snapshot = Self::local_worktree_usage_snapshot(&store, &db_path, project_instance_id)?;
        Ok(Some(LocalWorktreeAtlas {
            project_instance_id,
            snapshot,
        }))
    }

    /// Reopen an identity-bound local atlas before its registration is committed.
    fn revalidate_local_worktree_atlas_identity(
        root: &Path,
        expected: Option<ProjectInstanceId>,
    ) -> Result<(), CliError> {
        let Some(expected) = expected else {
            return Ok(());
        };
        let db_path = Self::projectatlas_db_path(root);
        let Some(store) = Self::open_local_worktree_atlas(root)? else {
            return Err(CliError::InvalidInput(
                MCP_ERROR_WORKTREE_IDENTITY_CONFLICT.to_string(),
            ));
        };
        if Self::local_worktree_project_instance_id(&store, &db_path)? != expected {
            return Err(CliError::InvalidInput(
                MCP_ERROR_WORKTREE_IDENTITY_CONFLICT.to_string(),
            ));
        }
        Ok(())
    }

    /// Finalize one retirement under control-first writer exclusion.
    fn retire_registered_worktree(
        control: &AtlasStore,
        registration: &WorktreeRegistration,
        root: Option<&Path>,
        retired_at_epoch: u64,
        initial_blocker: Option<String>,
    ) -> Result<
        (
            WorktreeRegistration,
            Option<WorktreeUsageSyncState>,
            Option<String>,
        ),
        CliError,
    > {
        Self::retire_registered_worktree_with_pre_open(
            control,
            registration,
            root,
            retired_at_epoch,
            initial_blocker,
            || Ok(()),
        )
    }

    /// Finalize retirement with one deterministic seam before the local atlas is opened.
    fn retire_registered_worktree_with_pre_open<F>(
        control: &AtlasStore,
        registration: &WorktreeRegistration,
        root: Option<&Path>,
        retired_at_epoch: u64,
        initial_blocker: Option<String>,
        pre_open: F,
    ) -> Result<
        (
            WorktreeRegistration,
            Option<WorktreeUsageSyncState>,
            Option<String>,
        ),
        CliError,
    >
    where
        F: FnOnce() -> Result<(), CliError>,
    {
        control.with_active_worktree_registration(
            registration.registration_id,
            &registration.alias,
            |guard| {
                let Some(root) = root else {
                    let retired = guard.retire(retired_at_epoch)?;
                    return Ok(Ok((retired, None, initial_blocker)));
                };
                if require_registered_worktree_lifecycle(guard.registration(), root).is_err() {
                    return Self::retire_changed_worktree_lifecycle(guard, retired_at_epoch)
                        .map(Ok);
                }
                if let Err(error) = pre_open() {
                    return Ok(Err(error));
                }
                let local = match Self::open_local_worktree_atlas(root) {
                    Ok(local) => local,
                    Err(error) => {
                        return Self::classify_retirement_failure(
                            guard,
                            root,
                            retired_at_epoch,
                            error,
                        );
                    }
                };
                let Some(local) = local else {
                    if require_registered_worktree_lifecycle(guard.registration(), root).is_err() {
                        return Self::retire_changed_worktree_lifecycle(guard, retired_at_epoch)
                            .map(Ok);
                    }
                    if guard.registration().project_instance_id.is_some() {
                        return Ok(Err(CliError::InvalidInput(
                            MCP_ERROR_BOUND_WORKTREE_ATLAS_MISSING.to_string(),
                        )));
                    }
                    let retired = guard.retire(retired_at_epoch)?;
                    return Ok(Ok((retired, None, initial_blocker)));
                };
                let db_path = Self::projectatlas_db_path(root);
                let project_instance_id =
                    match Self::local_worktree_project_instance_id(&local, &db_path) {
                        Ok(project_instance_id) => project_instance_id,
                        Err(error) => {
                            return Self::classify_retirement_failure(
                                guard,
                                root,
                                retired_at_epoch,
                                error,
                            );
                        }
                    };
                let finalization = match local.with_exclusive_worktree_usage_snapshot(|snapshot| {
                    if require_registered_worktree_lifecycle(guard.registration(), root).is_err() {
                        return Ok(None);
                    }
                    guard
                        .retire_with_usage_snapshot(
                            root,
                            project_instance_id,
                            snapshot,
                            retired_at_epoch,
                        )
                        .map(Some)
                }) {
                    Ok(finalization) => finalization,
                    Err(error) => {
                        return Self::classify_retirement_failure(
                            guard,
                            root,
                            retired_at_epoch,
                            error.into(),
                        );
                    }
                };
                if let Some((retired, synchronized)) = finalization {
                    return Ok(Ok((retired, Some(synchronized), initial_blocker)));
                }
                Self::retire_changed_worktree_lifecycle(guard, retired_at_epoch).map(Ok)
            },
        )?
    }

    /// Preserve an unchanged database error or retire a replaced Git lifecycle.
    fn classify_retirement_failure(
        guard: &mut ActiveWorktreeRegistrationGuard<'_>,
        root: &Path,
        retired_at_epoch: u64,
        error: CliError,
    ) -> Result<
        Result<
            (
                WorktreeRegistration,
                Option<WorktreeUsageSyncState>,
                Option<String>,
            ),
            CliError,
        >,
        DbError,
    > {
        if require_registered_worktree_lifecycle(guard.registration(), root).is_err() {
            Self::retire_changed_worktree_lifecycle(guard, retired_at_epoch).map(Ok)
        } else {
            Ok(Err(error))
        }
    }

    /// Retire a stale registration without binding, importing, or modifying replacement state.
    fn retire_changed_worktree_lifecycle(
        guard: &mut ActiveWorktreeRegistrationGuard<'_>,
        retired_at_epoch: u64,
    ) -> Result<
        (
            WorktreeRegistration,
            Option<WorktreeUsageSyncState>,
            Option<String>,
        ),
        DbError,
    > {
        let retired = guard.retire(retired_at_epoch)?;
        Ok((
            retired,
            None,
            Some(MCP_ERROR_WORKTREE_LIFECYCLE_CHANGED.to_string()),
        ))
    }

    /// Reset one unbound alias while excluding concurrent bind or retirement publication.
    fn reset_registered_worktree_index(
        &self,
        state: &McpProjectState,
        selection: &McpWorktreeSelection,
        include_mcp_config: bool,
    ) -> Result<ResetIndexReport, CliError> {
        self.reset_registered_worktree_index_with_post_validation(
            state,
            selection,
            include_mcp_config,
            || Ok(()),
        )
    }

    /// Reset one unbound alias with a deterministic post-validation seam.
    fn reset_registered_worktree_index_with_post_validation<F>(
        &self,
        state: &McpProjectState,
        selection: &McpWorktreeSelection,
        include_mcp_config: bool,
        post_validation: F,
    ) -> Result<ResetIndexReport, CliError>
    where
        F: FnOnce() -> Result<(), CliError>,
    {
        let alias = WorktreeAlias::parse(&selection.alias)?;
        let registration_id =
            selection
                .registration_id
                .ok_or_else(|| DbError::WorktreeRegistrationNotFound {
                    alias: selection.alias.clone(),
                })?;
        let control = Self::open_existing_mut_store(&self.control_state, &self.control_state)?;
        Self::require_captured_control_identity(Some(selection), &control)?;
        match control.with_unbound_worktree_registration(registration_id, &alias, |registration| {
            require_registered_worktree_lifecycle(registration, &state.root)?;
            post_validation()?;
            reset_index_files_with_revalidation(&state.db_path, include_mcp_config, || {
                require_registered_worktree_lifecycle(registration, &state.root)
            })
        }) {
            Ok(result) => result,
            Err(DbError::WorktreeRegistrationConflict { .. }) => Err(CliError::InvalidInput(
                MCP_ERROR_BOUND_WORKTREE_RESET_UNSUPPORTED.to_string(),
            )),
            Err(error) => Err(error.into()),
        }
    }

    /// Join one structural worktree entry to registry, atlas, and telemetry state.
    fn worktree_list_row(
        &self,
        common_directory: &Path,
        entry: &GitWorktreeEntry,
        registrations: &[WorktreeRegistration],
    ) -> McpWorktreeRow {
        let administrative_directory =
            lossless_native_path_display(&entry.administrative_directory);
        let registration_paths_are_utf8 =
            Self::worktree_registration_paths_are_utf8(common_directory, entry);
        let registration = registration_paths_are_utf8
            .then(|| {
                administrative_directory.as_deref().and_then(|directory| {
                    registrations.iter().find(|registration| {
                        registration.state == WorktreeRegistrationState::Active
                            && registration.git_administrative_directory == directory
                    })
                })
            })
            .flatten();
        let administrative_identity = git_administrative_identity(&entry.administrative_directory);
        let lifecycle_matches = registration.is_none_or(|registration| {
            administrative_identity
                .as_ref()
                .is_ok_and(|identity| identity == &registration.git_administrative_identity)
        });
        let root = Self::active_worktree_root(entry);
        let control = root.is_some_and(|root| root == self.control_state.root);
        let alias = if control {
            Some(MCP_MAIN_WORKTREE_ALIAS.to_string())
        } else {
            registration.map(|registration| registration.alias.to_string())
        };
        let registration_state = if control {
            McpWorktreeRegistrationState::Control
        } else if registration.is_some() {
            McpWorktreeRegistrationState::Registered
        } else {
            McpWorktreeRegistrationState::Unregistered
        };
        let mut atlas_state = McpWorktreeAtlasState::Unavailable;
        let mut telemetry_state = McpWorktreeTelemetryState::Unavailable;
        let mut local_telemetry_revision = None;
        let mut project_instance_id = None;
        let mut blocker = if registration_paths_are_utf8 {
            (!lifecycle_matches)
                .then(|| MCP_ERROR_WORKTREE_LIFECYCLE_CHANGED.to_string())
                .or_else(|| administrative_identity.err().map(|error| error.to_string()))
        } else {
            Some(MCP_ERROR_WORKTREE_PATH_NON_UTF8.to_string())
        };
        let git_state = match &entry.state {
            GitWorktreeState::Active { root, .. } => {
                if registration_paths_are_utf8 && lifecycle_matches {
                    match Self::local_worktree_atlas(root) {
                        Ok(Some(local)) => {
                            let identity_matches = registration
                                .and_then(|registration| registration.project_instance_id)
                                .is_none_or(|expected| expected == local.project_instance_id);
                            if identity_matches {
                                atlas_state = McpWorktreeAtlasState::Initialized;
                                local_telemetry_revision = Some(local.snapshot.revision());
                                project_instance_id = Some(local.project_instance_id.to_string());
                                telemetry_state = if control {
                                    McpWorktreeTelemetryState::Control
                                } else if let Some(registration) = registration {
                                    if local.snapshot.revision()
                                        > registration.accepted_telemetry_revision
                                    {
                                        McpWorktreeTelemetryState::Pending
                                    } else {
                                        McpWorktreeTelemetryState::Current
                                    }
                                } else {
                                    McpWorktreeTelemetryState::Unregistered
                                };
                            } else {
                                atlas_state = McpWorktreeAtlasState::Invalid;
                                blocker = Some(MCP_ERROR_WORKTREE_IDENTITY_CONFLICT.to_string());
                            }
                        }
                        Ok(None) => {
                            atlas_state = McpWorktreeAtlasState::Missing;
                            telemetry_state = if registration.is_some() {
                                McpWorktreeTelemetryState::MissingAtlas
                            } else {
                                McpWorktreeTelemetryState::Unregistered
                            };
                        }
                        Err(error) => {
                            atlas_state = McpWorktreeAtlasState::Invalid;
                            blocker = Some(error.to_string());
                        }
                    }
                } else {
                    atlas_state = McpWorktreeAtlasState::Invalid;
                    telemetry_state = McpWorktreeTelemetryState::Unavailable;
                }
                McpGitWorktreeState::Active
            }
            GitWorktreeState::Missing { git_control_path } => {
                blocker = Some(format!(
                    "Git registration target is missing at '{}'",
                    normalize_native_path_display(git_control_path)
                ));
                McpGitWorktreeState::Missing
            }
            GitWorktreeState::Invalid { issue } => {
                blocker = Some(format!(
                    "invalid Git evidence at '{}': {:?}",
                    normalize_native_path_display(&issue.path),
                    issue.kind
                ));
                McpGitWorktreeState::Invalid
            }
        };
        McpWorktreeRow {
            selector: registration_paths_are_utf8.then(|| Self::worktree_candidate_selector(entry)),
            alias,
            role: entry.role.into(),
            git_state,
            registration: registration_state,
            administrative_directory,
            root: root
                .filter(|_| registration_paths_are_utf8)
                .and_then(lossless_project_root_display),
            atlas_state,
            telemetry_state,
            accepted_telemetry_revision: registration
                .map(|registration| registration.accepted_telemetry_revision),
            local_telemetry_revision,
            project_instance_id,
            blocker,
        }
    }

    /// Preserve one active alias when Git no longer reports its worktree registration.
    fn missing_registered_worktree_row(registration: &WorktreeRegistration) -> McpWorktreeRow {
        McpWorktreeRow {
            selector: Some(Self::worktree_candidate_selector_from_identity(
                &registration.git_administrative_directory,
            )),
            alias: Some(registration.alias.to_string()),
            role: McpGitWorktreeRole::Linked,
            git_state: McpGitWorktreeState::Missing,
            registration: McpWorktreeRegistrationState::Registered,
            administrative_directory: Some(registration.git_administrative_directory.clone()),
            root: Some(registration.last_root.clone()),
            atlas_state: McpWorktreeAtlasState::Unavailable,
            telemetry_state: McpWorktreeTelemetryState::Unavailable,
            accepted_telemetry_revision: Some(registration.accepted_telemetry_revision),
            local_telemetry_revision: None,
            project_instance_id: registration
                .project_instance_id
                .map(|identity| identity.to_string()),
            blocker: Some(MCP_WORKTREE_MISSING_RETENTION_REASON.to_string()),
        }
    }

    /// Return the current Unix epoch seconds within the persisted `SQLite` domain.
    fn current_epoch_seconds() -> Result<u64, CliError> {
        u64::try_from(mcp_unix_time_ms().saturating_div(1_000)).map_err(|source| {
            CliError::InvalidInput(format!(
                "current Unix epoch exceeds the supported worktree registry range: {source}"
            ))
        })
    }

    /// Return active/legacy state or one captured registered alias target.
    fn state_for_target(
        &self,
        project_path: Option<String>,
        worktree: Option<String>,
    ) -> Result<McpProjectState, CliError> {
        let state = self.state_for_target_with_config_validation(
            project_path,
            worktree,
            McpConfigValidation::Immediate,
        )?;
        Self::require_initialized_worktree_target(&state)?;
        Ok(state)
    }

    /// Resolve an ordered alias federation into exact initialized roots and identities.
    fn federated_worktree_roots(
        &self,
        worktrees: &[String],
    ) -> Result<(Vec<PathBuf>, Vec<McpWorktreeSelection>), CliError> {
        validate_federated_root_count(worktrees.len()).map_err(CliError::Service)?;
        let mut roots = Vec::with_capacity(worktrees.len());
        let mut selections = Vec::with_capacity(worktrees.len());
        for worktree in worktrees {
            let state = self.state_for_target(None, Some(worktree.clone()))?;
            let selection = state.worktree.ok_or_else(|| {
                CliError::InvalidInput(MCP_ERROR_FEDERATED_ALIAS_MISSING.to_string())
            })?;
            if selections
                .iter()
                .any(|captured: &McpWorktreeSelection| captured.alias == selection.alias)
                || roots.contains(&state.root)
            {
                return Err(CliError::Service(ServiceError::InvalidInput(
                    MCP_ERROR_FEDERATED_TARGET_DUPLICATE.to_string(),
                )));
            }
            selections.push(selection);
            roots.push(state.root);
        }
        Ok((roots, selections))
    }

    /// Resolve one mutually exclusive target under the selected config-validation timing.
    fn state_for_target_with_config_validation(
        &self,
        project_path: Option<String>,
        worktree: Option<String>,
        validation: McpConfigValidation,
    ) -> Result<McpProjectState, CliError> {
        let project_path = Self::normalized_optional_path(project_path);
        let worktree = worktree.map(|alias| alias.trim().to_string());
        if project_path.is_some() && worktree.is_some() {
            return Err(CliError::InvalidInput(
                MCP_WORKTREE_PROJECT_PATH_CONFLICT.to_string(),
            ));
        }
        let Some(alias) = worktree else {
            return self.state_for_project_path_with_config_validation(project_path, validation);
        };
        if alias.is_empty() {
            return Err(CliError::InvalidInput(
                MCP_ERROR_WORKTREE_SELECTOR_EMPTY.to_string(),
            ));
        }
        if alias == MCP_MAIN_WORKTREE_ALIAS {
            let mut state = self.control_state.clone();
            let project_instance_id = if state.db_path.is_file() {
                Self::open_read_store(&state)?.project_instance_id()?
            } else {
                None
            };
            state.worktree = Some(McpWorktreeSelection {
                alias,
                registration_id: None,
                project_instance_id,
                control_project_instance_id: project_instance_id,
            });
            return Ok(state);
        }
        let alias = WorktreeAlias::parse(&alias)?;
        self.resolve_registered_worktree(&alias, validation)
    }

    /// Resolve one active registration through current reciprocal Git structure.
    fn resolve_registered_worktree(
        &self,
        alias: &WorktreeAlias,
        validation: McpConfigValidation,
    ) -> Result<McpProjectState, CliError> {
        let control = Self::open_existing_mut_store(&self.control_state, &self.control_state)?;
        let control_project_instance_id = control.captured_project_binding()?.project_instance_id;
        let registration = control.worktree_registration(alias)?;
        let repository = self.control_git_repository()?;
        if repository.common_directory.to_str().is_none() {
            return Err(CliError::InvalidInput(
                MCP_ERROR_WORKTREE_PATH_NON_UTF8.to_string(),
            ));
        }
        if normalize_native_path_display(&repository.common_directory)
            != registration.git_common_directory
        {
            return Err(CliError::InvalidInput(format!(
                "registered worktree '{}' belongs to a different Git common directory",
                alias.as_str()
            )));
        }
        let entry = repository
            .worktrees
            .iter()
            .find(|entry| {
                Self::worktree_registration_paths_are_utf8(&repository.common_directory, entry)
                    && normalize_native_path_display(&entry.administrative_directory)
                        == registration.git_administrative_directory
            })
            .ok_or_else(|| {
                CliError::InvalidInput(format!(
                    "registered worktree '{}' is no longer present in the bounded Git inventory",
                    alias.as_str()
                ))
            })?;
        let administrative_identity = git_administrative_identity(&entry.administrative_directory)?;
        if administrative_identity != registration.git_administrative_identity {
            return Err(CliError::InvalidInput(
                MCP_ERROR_WORKTREE_LIFECYCLE_CHANGED.to_string(),
            ));
        }
        let root = match &entry.state {
            GitWorktreeState::Active { root, .. } => root.clone(),
            GitWorktreeState::Missing { .. } => {
                return Err(CliError::InvalidInput(format!(
                    "registered worktree '{}' is missing; restore it through Git or unregister it",
                    alias.as_str()
                )));
            }
            GitWorktreeState::Invalid { issue } => {
                return Err(CliError::InvalidInput(format!(
                    "registered worktree '{}' has invalid Git evidence at '{}': {:?}",
                    alias.as_str(),
                    normalize_native_path_display(&issue.path),
                    issue.kind
                )));
            }
        };
        if root == self.control_state.root {
            return Err(CliError::InvalidInput(
                MCP_ERROR_CONTROL_ALIAS_REQUIRED.to_string(),
            ));
        }
        CanonicalProjectRoot::from_path(&root).map_err(|source| {
            let mut message = MCP_ERROR_REGISTERED_WORKTREE_ROOT_INVALID_PREFIX.to_string();
            message.push_str(&source.to_string());
            CliError::InvalidInput(message)
        })?;
        let current_registry_root = root
            .to_str()
            .map(normalize_native_path_display_str)
            .ok_or_else(|| CliError::InvalidInput(MCP_ERROR_WORKTREE_PATH_NON_UTF8.to_string()))?;
        // Validate a bound local atlas before any control-catalog refresh. A Git
        // move alone is not enough evidence to rewrite `last_root`; the local
        // atlas must first be opened through its native admission boundary and
        // retain the captured project identity.
        if let Some(expected) = registration.project_instance_id {
            let store = Self::open_local_worktree_atlas(&root)?.ok_or_else(|| {
                CliError::InvalidInput(MCP_ERROR_BOUND_WORKTREE_ATLAS_MISSING.to_string())
            })?;
            let db_path = Self::projectatlas_db_path(&root);
            let observed = Self::local_worktree_project_instance_id(&store, &db_path)?;
            if observed != expected {
                return Err(CliError::InvalidInput(
                    MCP_ERROR_WORKTREE_IDENTITY_CONFLICT.to_string(),
                ));
            }
        }
        let registration_root_matches = current_registry_root == registration.last_root;
        let registration = if registration_root_matches {
            registration
        } else {
            control.refresh_worktree_root(&registration, &root)?
        };
        let mut state = Self::project_state_from_root_with_config_validation(&root, validation)?;
        state.worktree = Some(McpWorktreeSelection {
            alias: alias.to_string(),
            registration_id: Some(registration.registration_id),
            project_instance_id: registration.project_instance_id,
            control_project_instance_id: Some(control_project_instance_id),
        });
        Ok(state)
    }

    /// Return project state under the requested configuration-validation timing.
    fn state_for_project_path_with_config_validation(
        &self,
        project_path: Option<String>,
        validation: McpConfigValidation,
    ) -> Result<McpProjectState, CliError> {
        let project_path = Self::normalized_optional_path(project_path);
        project_path.map_or_else(
            || self.active_project_state(),
            |path| {
                Self::project_state_from_root_with_config_validation(Path::new(&path), validation)
            },
        )
    }

    /// Return the nearest-project policy for one call, honoring explicit overrides.
    fn nearest_project_enabled(&self, override_value: Option<bool>) -> bool {
        override_value.unwrap_or(self.allow_nearest_project)
    }

    /// Return selected state and validate an optional root assertion.
    fn state_and_root_path(
        &self,
        project_path: Option<String>,
        worktree: Option<String>,
        path: Option<String>,
        nearest_project: bool,
    ) -> Result<(McpProjectState, PathBuf), CliError> {
        self.state_and_root_path_with_config_validation(
            project_path,
            worktree,
            path,
            nearest_project,
            McpConfigValidation::Immediate,
        )
    }

    /// Select a background project without reading configuration before task admission.
    fn background_state_and_root_path(
        &self,
        project_path: Option<String>,
        worktree: Option<String>,
        path: Option<String>,
        nearest_project: bool,
    ) -> Result<(McpProjectState, PathBuf), CliError> {
        self.state_and_root_path_with_config_validation(
            project_path,
            worktree,
            path,
            nearest_project,
            McpConfigValidation::Deferred,
        )
    }

    /// Select project state and validate a root assertion under one config timing policy.
    fn state_and_root_path_with_config_validation(
        &self,
        project_path: Option<String>,
        worktree: Option<String>,
        path: Option<String>,
        nearest_project: bool,
        validation: McpConfigValidation,
    ) -> Result<(McpProjectState, PathBuf), CliError> {
        let explicit_target = project_path.is_some() || worktree.is_some();
        let state = self.state_for_target_with_config_validation(
            project_path.clone(),
            worktree,
            validation,
        )?;
        Self::require_initialized_worktree_target(&state)?;
        let root = match (
            Self::normalized_optional_path(project_path),
            Self::normalized_optional_path(path),
        ) {
            (None, Some(path)) if !explicit_target => {
                match Self::path_or_project_root(&state, Some(path.clone())) {
                    Ok(root) => root,
                    Err(active_error) => {
                        if !nearest_project {
                            return Err(active_error);
                        }
                        if Self::absolute_path_inside_selected_root(&state, &path)? {
                            return Err(active_error);
                        }
                        let Some(indexed_state) =
                            Self::nearest_root_state_for_root_argument_with_config_validation(
                                Path::new(&path),
                                validation,
                            )?
                        else {
                            return Err(active_error);
                        };
                        let root = indexed_state.root.clone();
                        return Ok((indexed_state, root));
                    }
                }
            }
            (_, path) => Self::path_or_project_root(&state, path)?,
        };
        Ok((state, root))
    }

    /// Return whether a root assertion path is inside the selected root but not root-equivalent.
    fn absolute_path_inside_selected_root(
        state: &McpProjectState,
        path: &str,
    ) -> Result<bool, CliError> {
        let candidate = PathBuf::from(path);
        if !candidate.is_absolute() {
            return Ok(false);
        }
        let resolved = canonical_project_root(&candidate)?;
        Ok(resolved != state.root && resolved.starts_with(&state.root))
    }

    /// Return nearest indexed root under one configuration-validation timing policy.
    fn nearest_root_state_for_root_argument_with_config_validation(
        path: &Path,
        validation: McpConfigValidation,
    ) -> Result<Option<McpProjectState>, CliError> {
        let Ok(addressed_root) = canonical_project_root(path) else {
            return Ok(None);
        };
        let Some(indexed_state) =
            Self::project_state_from_nearest_indexed_path_with_config_validation(path, validation)?
        else {
            return Ok(None);
        };
        if addressed_root == indexed_state.root {
            Ok(Some(indexed_state))
        } else {
            Ok(None)
        }
    }

    /// Return state and a repository-relative file key for an MCP file argument.
    fn state_and_file_key(
        &self,
        project_path: Option<&str>,
        worktree: Option<&str>,
        file: &str,
        nearest_project: bool,
    ) -> Result<McpResolvedRepoPath, CliError> {
        let state = self.state_for_target(
            project_path.map(ToString::to_string),
            worktree.map(ToString::to_string),
        )?;
        let file_path = PathBuf::from(&file);
        if !file_path.is_absolute() {
            let file_key = validated_repo_file_key(&file_path)
                .map_err(|source| CliError::InvalidInput(source.to_string()))?;
            return Ok(McpResolvedRepoPath {
                state,
                key: file_key,
                routed_project: false,
            });
        }
        if nearest_project && project_path.is_none() && worktree.is_none() {
            let resolved = Self::nearest_state_and_repo_key(&state, file)?.ok_or_else(|| {
                Self::selected_project_path_error(PATH_NOT_INSIDE_INDEXED_PROJECT_ERROR)
            })?;
            let file_key = validated_repo_file_key(Path::new(&resolved.key))
                .map_err(|source| CliError::InvalidInput(source.to_string()))?;
            return Ok(McpResolvedRepoPath {
                key: file_key,
                ..resolved
            });
        }
        if let Some(file_key) = Self::absolute_path_key_in_selected_project(&state, &file_path)? {
            let file_key = validated_repo_file_key(Path::new(&file_key))
                .map_err(|source| CliError::InvalidInput(source.to_string()))?;
            return Ok(McpResolvedRepoPath {
                state,
                key: file_key,
                routed_project: false,
            });
        }
        if project_path.is_some() || worktree.is_some() {
            return Err(Self::selected_project_path_error(
                PATH_NOT_INSIDE_INDEXED_PROJECT_ERROR,
            ));
        }
        if !nearest_project {
            return Err(Self::selected_project_path_error(
                PATH_NOT_INSIDE_INDEXED_PROJECT_ERROR,
            ));
        }
        Err(Self::selected_project_path_error(
            PATH_NOT_INSIDE_INDEXED_PROJECT_ERROR,
        ))
    }

    /// Return state and an optional repository-relative file key for MCP symbol arguments.
    fn state_and_optional_file_key(
        &self,
        project_path: Option<&str>,
        worktree: Option<&str>,
        file: Option<&str>,
        nearest_project: bool,
    ) -> Result<(McpProjectState, Option<String>, bool), CliError> {
        let Some(file) = file else {
            return self
                .state_for_target(
                    project_path.map(ToString::to_string),
                    worktree.map(ToString::to_string),
                )
                .map(|state| (state, None, false));
        };
        let resolved = self.state_and_file_key(project_path, worktree, file, nearest_project)?;
        Ok((resolved.state, Some(resolved.key), resolved.routed_project))
    }

    /// Return state and an optional folder filter for MCP file-ranking arguments.
    fn state_and_optional_folder_filter(
        &self,
        project_path: Option<&str>,
        worktree: Option<&str>,
        folder: Option<&str>,
        nearest_project: bool,
    ) -> Result<(McpProjectState, Option<String>, bool), CliError> {
        let state = self.state_for_target(
            project_path.map(ToString::to_string),
            worktree.map(ToString::to_string),
        )?;
        let Some(folder) = folder.map(str::trim).filter(|folder| !folder.is_empty()) else {
            return Ok((state, None, false));
        };
        let folder_path = PathBuf::from(&folder);
        if !folder_path.is_absolute() {
            let folder_filter = normalized_folder_filter(folder)?;
            return Ok((state, Some(folder_filter), false));
        }
        if nearest_project && project_path.is_none() && worktree.is_none() {
            let resolved = Self::nearest_state_and_repo_key(&state, folder)?.ok_or_else(|| {
                Self::selected_project_path_error(FOLDER_NOT_INSIDE_INDEXED_PROJECT_ERROR)
            })?;
            let folder_filter = normalized_folder_filter(&resolved.key)?;
            return Ok((resolved.state, Some(folder_filter), resolved.routed_project));
        }
        if let Some(folder_filter) =
            Self::absolute_path_key_in_selected_project(&state, &folder_path)?
        {
            let folder_filter = normalized_folder_filter(&folder_filter)?;
            return Ok((state, Some(folder_filter), false));
        }
        if project_path.is_some() || worktree.is_some() {
            return Err(Self::selected_project_path_error(
                FOLDER_NOT_INSIDE_INDEXED_PROJECT_ERROR,
            ));
        }
        if !nearest_project {
            return Err(Self::selected_project_path_error(
                FOLDER_NOT_INSIDE_INDEXED_PROJECT_ERROR,
            ));
        }
        Err(Self::selected_project_path_error(
            FOLDER_NOT_INSIDE_INDEXED_PROJECT_ERROR,
        ))
    }

    /// Resolve an absolute addressed path to the nearest indexed project and repo key.
    fn nearest_state_and_repo_key(
        active_state: &McpProjectState,
        path: &str,
    ) -> Result<Option<McpResolvedRepoPath>, CliError> {
        let path = Path::new(path);
        let absolute_path = McpAbsolutePath::canonicalize(path)?;
        let lexical_state = Self::project_state_from_nearest_lexical_indexed_path(path)?;
        let canonical_state = Self::project_state_from_nearest_indexed_path(path)?;
        Self::reject_ambiguous_nearest_project_path(
            path,
            lexical_state.as_ref(),
            canonical_state.as_ref(),
            &absolute_path,
        )?;
        let Some(state) = canonical_state else {
            return Ok(None);
        };
        if state.root == active_state.root {
            let key = McpSelectedRoot::from_state(active_state)
                .repo_key_for(&absolute_path)?
                .ok_or_else(|| {
                    Self::selected_project_path_error(PATH_NOT_INSIDE_INDEXED_PROJECT_ERROR)
                })?;
            return Ok(Some(McpResolvedRepoPath {
                state: active_state.clone(),
                key: key.into_string(),
                routed_project: false,
            }));
        }
        let key = McpSelectedRoot::from_state(&state)
            .repo_key_for(&absolute_path)?
            .ok_or_else(|| {
                Self::selected_project_path_error(PATH_NOT_INSIDE_INDEXED_PROJECT_ERROR)
            })?;
        Ok(Some(McpResolvedRepoPath {
            state,
            key: key.into_string(),
            routed_project: true,
        }))
    }

    /// Return a selected-project repository key for an absolute path inside the active root.
    fn absolute_path_key_in_selected_project(
        state: &McpProjectState,
        path: &Path,
    ) -> Result<Option<String>, CliError> {
        let absolute_path = McpAbsolutePath::canonicalize(path)?;
        McpSelectedRoot::from_state(state)
            .repo_key_for(&absolute_path)
            .map(|key| key.map(McpRepoKey::into_string))
    }

    /// Normalize optional project/root path text from MCP payloads.
    fn normalized_optional_path(path: Option<String>) -> Option<String> {
        path.map(|path| path.trim().to_string())
            .filter(|path| !path.is_empty())
    }

    /// Build `ProjectAtlas` state for one project root.
    fn project_state_from_root(root: &Path) -> Result<McpProjectState, CliError> {
        Self::project_state_from_root_with_config_validation(root, McpConfigValidation::Immediate)
    }

    /// Build project state while controlling when configuration content is validated.
    fn project_state_from_root_with_config_validation(
        root: &Path,
        validation: McpConfigValidation,
    ) -> Result<McpProjectState, CliError> {
        let root = canonical_source_project_root(root)?;
        if !root.is_dir() {
            return Err(CliError::InvalidInput(format!(
                "project path '{}' is not a directory",
                root.display()
            )));
        }
        let db_path = Self::projectatlas_db_path(&root);
        let config_path = Self::config_path_for_project_root(&root, validation)?;
        Ok(McpProjectState {
            root,
            db_path,
            config_path,
            worktree: None,
        })
    }

    /// Build project state from the nearest indexed ancestor of an addressed path.
    fn project_state_from_nearest_indexed_path(
        path: &Path,
    ) -> Result<Option<McpProjectState>, CliError> {
        Self::project_state_from_nearest_indexed_path_with_config_validation(
            path,
            McpConfigValidation::Immediate,
        )
    }

    /// Build nearest canonical project state under one config-validation policy.
    fn project_state_from_nearest_indexed_path_with_config_validation(
        path: &Path,
        validation: McpConfigValidation,
    ) -> Result<Option<McpProjectState>, CliError> {
        let Ok(absolute_path) = McpAbsolutePath::canonicalize(path) else {
            return Ok(None);
        };
        let mut candidate = absolute_path.nearest_search_start();
        loop {
            if let Some(indexed_root) = Self::indexed_root_from_candidate(candidate) {
                let config_path =
                    Self::config_path_for_project_root(&indexed_root.root, validation)?;
                return Ok(Some(McpProjectState {
                    root: indexed_root.root,
                    db_path: indexed_root.db_path,
                    config_path,
                    worktree: None,
                }));
            }
            let Some(parent) = candidate.parent() else {
                return Ok(None);
            };
            candidate = parent;
        }
    }

    /// Build project state from the nearest lexical indexed ancestor of an addressed path.
    fn project_state_from_nearest_lexical_indexed_path(
        path: &Path,
    ) -> Result<Option<McpProjectState>, CliError> {
        Self::project_state_from_nearest_lexical_indexed_path_with_config_validation(
            path,
            McpConfigValidation::Immediate,
        )
    }

    /// Build nearest lexical project state under one config-validation policy.
    fn project_state_from_nearest_lexical_indexed_path_with_config_validation(
        path: &Path,
        validation: McpConfigValidation,
    ) -> Result<Option<McpProjectState>, CliError> {
        if !path.is_absolute() {
            return Ok(None);
        }
        let lexical_path = Self::lexically_normalized_absolute_path(path);
        let mut candidate = if lexical_path.is_dir() {
            lexical_path
        } else {
            lexical_path
                .parent()
                .unwrap_or(lexical_path.as_path())
                .to_path_buf()
        };
        loop {
            if let Some(indexed_root) = Self::indexed_root_from_lexical_candidate(&candidate) {
                let config_path =
                    Self::config_path_for_project_root(&indexed_root.root, validation)?;
                return Ok(Some(McpProjectState {
                    root: indexed_root.root,
                    db_path: indexed_root.db_path,
                    config_path,
                    worktree: None,
                }));
            }
            let Some(parent) = candidate.parent() else {
                return Ok(None);
            };
            candidate = parent.to_path_buf();
        }
    }

    /// Return a role-typed indexed root when a candidate folder has a matching DB.
    fn indexed_root_from_candidate(candidate: &Path) -> Option<McpIndexedRoot> {
        let Ok(root) = canonical_project_root(candidate) else {
            return None;
        };
        let Ok(root_identity) = CanonicalProjectRoot::from_path(&root) else {
            return None;
        };
        let db_path = Self::projectatlas_db_path(&root);
        if !db_path.is_file() || !Self::nearest_indexed_db_matches_root(&db_path, &root_identity) {
            return None;
        }
        Some(McpIndexedRoot { root, db_path })
    }

    /// Return an indexed lexical root without treating symlinked descendants as the lexical owner.
    fn indexed_root_from_lexical_candidate(candidate: &Path) -> Option<McpIndexedRoot> {
        if Self::path_has_symlink_component(candidate) {
            return None;
        }
        let Ok(root) = canonical_project_root(candidate) else {
            return None;
        };
        let Ok(candidate_identity) = CanonicalProjectRoot::from_path(candidate) else {
            return None;
        };
        let Ok(root_identity) = CanonicalProjectRoot::from_path(&root) else {
            return None;
        };
        if candidate_identity != root_identity {
            return None;
        }
        let db_path = Self::projectatlas_db_path(&root);
        if !db_path.is_file() || !Self::nearest_indexed_db_matches_root(&db_path, &root_identity) {
            return None;
        }
        Some(McpIndexedRoot { root, db_path })
    }

    /// Return a normalized absolute path without resolving symlinks or junctions.
    fn lexically_normalized_absolute_path(path: &Path) -> PathBuf {
        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
                Component::RootDir => normalized.push(component.as_os_str()),
                Component::CurDir => {}
                Component::ParentDir => {
                    normalized.pop();
                }
                Component::Normal(segment) => normalized.push(segment),
            }
        }
        normalized
    }

    /// Return whether a path contains a symlink component in its lexical ancestry.
    fn path_has_symlink_component(path: &Path) -> bool {
        let mut current = PathBuf::new();
        for component in path.components() {
            match component {
                Component::Prefix(prefix) => current.push(prefix.as_os_str()),
                Component::RootDir => current.push(component.as_os_str()),
                Component::CurDir => {}
                Component::ParentDir => {
                    current.pop();
                }
                Component::Normal(segment) => {
                    current.push(segment);
                    if fs::symlink_metadata(&current)
                        .is_ok_and(|metadata| Self::metadata_is_symlink_or_reparse_point(&metadata))
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Return whether metadata represents a symlink or Windows reparse point.
    fn metadata_is_symlink_or_reparse_point(metadata: &fs::Metadata) -> bool {
        if metadata.file_type().is_symlink() {
            return true;
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
            metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    /// Reject nearest-project routing when lexical and canonical roots disagree.
    fn reject_ambiguous_nearest_project_path(
        path: &Path,
        lexical_state: Option<&McpProjectState>,
        canonical_state: Option<&McpProjectState>,
        canonical_path: &McpAbsolutePath,
    ) -> Result<(), CliError> {
        if let (Some(lexical_state), Some(canonical_state)) = (lexical_state, canonical_state) {
            if lexical_state.root != canonical_state.root {
                return Err(Self::ambiguous_nearest_project_path_error(
                    path,
                    Some(lexical_state),
                    Some(canonical_state),
                ));
            }
            return Ok(());
        }
        let lexical_path = Self::lexically_normalized_absolute_path(path);
        if Self::path_has_symlink_component(&lexical_path)
            || lexical_state.is_some_and(|state| !canonical_path.as_path().starts_with(&state.root))
        {
            return Err(Self::ambiguous_nearest_project_path_error(
                path,
                lexical_state,
                canonical_state,
            ));
        }
        Ok(())
    }

    /// Build a clear MCP error for ambiguous symlink or junction routing.
    fn ambiguous_nearest_project_path_error(
        path: &Path,
        lexical_state: Option<&McpProjectState>,
        canonical_state: Option<&McpProjectState>,
    ) -> CliError {
        let lexical_root = lexical_state.map_or_else(
            || MCP_NO_ROOT_PLACEHOLDER.to_string(),
            |state| normalize_native_path_display(&state.root),
        );
        let resolved_root = canonical_state.map_or_else(
            || MCP_NO_ROOT_PLACEHOLDER.to_string(),
            |state| normalize_native_path_display(&state.root),
        );
        let path_display = normalize_native_path_display(path);
        let mut message = String::new();
        message.push_str(AMBIGUOUS_NEAREST_PROJECT_PATH_ERROR);
        message.push_str(MCP_ERROR_FOR_PATH_FRAGMENT);
        message.push_str(&path_display);
        message.push_str(MCP_ERROR_LEXICAL_ROOT_FRAGMENT);
        message.push_str(&lexical_root);
        message.push_str(MCP_ERROR_RESOLVED_ROOT_FRAGMENT);
        message.push_str(&resolved_root);
        message.push_str(MCP_ERROR_GUIDANCE_FRAGMENT);
        message.push_str(OUTSIDE_SELECTED_PROJECT_GUIDANCE);
        CliError::InvalidInput(message)
    }

    /// Return whether nearest routing can safely associate one DB with a live root.
    fn nearest_indexed_db_matches_root(db_path: &Path, root: &CanonicalProjectRoot) -> bool {
        match read_project_root_identity_read_only(db_path) {
            Ok(Some(stored_root)) => Self::nearest_existing_roots_match(&stored_root, root),
            Ok(None) => {
                let Ok(Some(legacy_root)) = read_legacy_project_root_candidate_read_only(db_path)
                else {
                    return false;
                };
                if legacy_root.contains('\u{fffd}') {
                    return false;
                }
                let Ok(legacy_root) = CanonicalProjectRoot::from_path(Path::new(&legacy_root))
                else {
                    return false;
                };
                Self::nearest_existing_roots_match(&legacy_root, root)
            }
            Err(_) => false,
        }
    }

    /// Re-resolve both existing roots before nearest-routing comparison.
    fn nearest_existing_roots_match(
        persisted_root: &CanonicalProjectRoot,
        selected_root: &CanonicalProjectRoot,
    ) -> bool {
        let Ok(persisted_root) = CanonicalProjectRoot::from_path(persisted_root.as_path()) else {
            return false;
        };
        let Ok(selected_root) = CanonicalProjectRoot::from_path(selected_root.as_path()) else {
            return false;
        };
        persisted_root == selected_root
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

    /// Find a project-local config and optionally reject one pointing at another root.
    fn config_path_for_project_root(
        root: &Path,
        validation: McpConfigValidation,
    ) -> Result<Option<PathBuf>, CliError> {
        for config_path in [
            Self::projectatlas_nested_config_path(root),
            Self::projectatlas_flat_config_path(root),
        ] {
            if config_path.exists() {
                if validation == McpConfigValidation::Immediate {
                    Self::validate_project_config_root(root, &config_path)?;
                }
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
            project: Self::project_state_payload(state),
        };
        Self::encode_serialized_payload(payload)
    }

    /// Build selected-project payload fields.
    fn project_state_payload(state: &McpProjectState) -> McpProjectStatePayload {
        McpProjectStatePayload {
            worktree: state
                .worktree
                .as_ref()
                .map(|selection| selection.alias.clone()),
            registration_id: state
                .worktree
                .as_ref()
                .and_then(|selection| selection.registration_id),
            root: lossless_project_root_display(&state.root),
            db: lossless_native_path_display(&state.db_path),
            config: state
                .config_path
                .as_ref()
                .and_then(|path| lossless_native_path_display(path)),
            status: McpProjectStatus::Active,
        }
    }

    /// Prefix routed cross-project read payloads with selected root/DB metadata.
    fn with_selected_project_audit(
        state: &McpProjectState,
        routed_project: bool,
        toon: String,
    ) -> Result<String, CliError> {
        if !routed_project {
            return Ok(toon);
        }
        let prefix = Self::encode_named_payload(
            MCP_PAYLOAD_SELECTED_PROJECT,
            &Self::project_state_payload(state),
        )?;
        let mut audited = String::with_capacity(prefix.len() + 1 + toon.len());
        audited.push_str(&prefix);
        audited.push('\n');
        audited.push_str(&toon);
        Ok(audited)
    }

    /// Prefix one controlled analysis payload with selected root/DB metadata.
    fn with_selected_project_audit_controlled(
        state: &McpProjectState,
        routed_project: bool,
        toon: String,
        control: &IndexWorkControl,
    ) -> Result<String, CliError> {
        control.check(projectatlas_core::IndexWorkStage::RepositoryTraversal)?;
        if !routed_project {
            return Ok(toon);
        }
        let prefix = controlled_named_output(
            OutputFormat::Toon,
            MCP_PAYLOAD_SELECTED_PROJECT,
            &Self::project_state_payload(state),
            control,
        )?;
        let mut audited = String::with_capacity(prefix.len() + 1 + toon.len());
        audited.push_str(&prefix);
        audited.push('\n');
        audited.push_str(&toon);
        control.check(projectatlas_core::IndexWorkStage::RepositoryTraversal)?;
        Ok(audited)
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
        let schema_version_mismatch = schema_version_mismatch_payload(error);
        let schema_migration_required = schema_migration_required_payload(error);
        let message = schema_migration_required.as_ref().map_or_else(
            || error.to_string(),
            SchemaMigrationRequiredPayload::message,
        );
        let (
            kind,
            refresh_required,
            init_required,
            worktree_required,
            verification_incomplete,
            project_mismatch,
            database_filesystem,
            search_capability,
            next,
        ) = match error {
            CliError::InitRequired(report) => (
                AgentErrorKind::InitRequired,
                None,
                Some(report.as_ref().clone()),
                None,
                None,
                None,
                None,
                None,
                Some(McpNextCall {
                    tool: MCP_TOOL_ATLAS_INIT,
                    project_path: report
                        .worktree
                        .is_none()
                        .then(|| report.project_root.clone())
                        .flatten(),
                    worktree: report.worktree.clone(),
                }),
            ),
            CliError::WorktreeRequired(report) => (
                AgentErrorKind::WorktreeRequired,
                None,
                None,
                Some(report.as_ref().clone()),
                None,
                None,
                None,
                None,
                None,
            ),
            CliError::RefreshRequired(report) => (
                AgentErrorKind::RefreshRequired,
                Some(report.as_ref().clone()),
                None,
                None,
                None,
                None,
                None,
                None,
                Some(McpNextCall {
                    tool: MCP_TOOL_ATLAS_WATCH_ONCE,
                    project_path: report
                        .worktree
                        .is_none()
                        .then(|| report.project_root.clone())
                        .flatten(),
                    worktree: report.worktree.clone(),
                }),
            ),
            CliError::VerificationIncomplete(report) => (
                AgentErrorKind::VerificationIncomplete,
                None,
                None,
                None,
                Some(report.as_ref().clone()),
                None,
                None,
                None,
                None,
            ),
            CliError::ProjectMismatch(report) => (
                AgentErrorKind::ProjectMismatch,
                None,
                None,
                None,
                None,
                Some(report.as_ref().clone()),
                None,
                None,
                None,
            ),
            CliError::Service(ServiceError::SearchCapabilityUnavailable {
                requested_mode,
                state,
                guidance,
            }) => (
                AgentErrorKind::SearchCapabilityUnavailable,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(crate::SearchCapabilityErrorPayload {
                    requested_mode: *requested_mode,
                    state,
                    recovery: guidance,
                }),
                None,
            ),
            _ if schema_version_mismatch.is_some() => (
                AgentErrorKind::SchemaVersionMismatch,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            _ if schema_migration_required.is_some() => (
                AgentErrorKind::SchemaMigrationRequired,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            _ => database_filesystem_error_payload(error).map_or(
                (
                    AgentErrorKind::Error,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
                |(kind, database_filesystem)| {
                    (
                        kind,
                        None,
                        None,
                        None,
                        None,
                        None,
                        Some(database_filesystem),
                        None,
                        None,
                    )
                },
            ),
        };
        let payload = McpErrorResponse {
            error: McpErrorPayload {
                kind,
                message,
                refresh_required,
                init_required,
                worktree_required,
                verification_incomplete,
                project_mismatch,
                database_filesystem,
                schema_version_mismatch,
                schema_migration_required,
                search_capability,
                next,
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
    fn as_mcp_text(result: Result<String, CliError>) -> McpToolTextResult {
        match result {
            Ok(text) => McpToolTextResult(Ok(text)),
            Err(error) => {
                let payload = Self::encode_error_payload(&error);
                if schema_version_mismatch_payload(&error).is_some() {
                    McpToolTextResult(Err(payload))
                } else {
                    McpToolTextResult(Ok(payload))
                }
            }
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

/// Return whether MCP parameters contain coverage-only filters.
fn has_coverage_filters(params: &AtlasHealthParams) -> bool {
    params.parser.is_some()
        || params.provider.is_some()
        || params.relation.is_some()
        || params.coverage_state.is_some()
        || params.reason.is_some()
}

/// Convert explicit MCP coverage parameters into one typed bounded DB query.
fn coverage_query_from_params(
    params: &AtlasHealthParams,
) -> Result<RepositoryCoverageQuery, CliError> {
    let limit = params
        .limit
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_HEALTH_LIMIT)
        .min(COVERAGE_PAGE_MAX_LIMIT as usize);
    Ok(RepositoryCoverageQuery {
        start_index: u32::try_from(params.start_index.unwrap_or(0)).map_err(|error| {
            let mut message = String::from(MCP_ERROR_COVERAGE_START_INDEX_TOO_LARGE_PREFIX);
            message.push_str(&error.to_string());
            CliError::InvalidInput(message)
        })?,
        limit: u32::try_from(limit).map_err(|error| {
            let mut message = String::from(MCP_ERROR_COVERAGE_LIMIT_TOO_LARGE_PREFIX);
            message.push_str(&error.to_string());
            CliError::InvalidInput(message)
        })?,
        path_prefix: trimmed_filter(params.path_prefix.as_deref())
            .map(|value| normalize_repo_path_prefix(&value)),
        parser: trimmed_filter(params.parser.as_deref())
            .as_deref()
            .map(parse_coverage_parser)
            .transpose()?,
        provider: trimmed_filter(params.provider.as_deref())
            .as_deref()
            .map(parse_coverage_parser)
            .transpose()?,
        relation: trimmed_filter(params.relation.as_deref())
            .as_deref()
            .map(parse_coverage_relation)
            .transpose()?,
        state: trimmed_filter(params.coverage_state.as_deref())
            .as_deref()
            .map(parse_coverage_state)
            .transpose()?,
        reason: trimmed_filter(params.reason.as_deref()),
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
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state = Self::project_state_from_root(Path::new(&params.project_path))?;
            self.set_active_project_state(state.clone())?;
            Self::render_project_state(&state)
        })())
    }

    /// Return bounded structural and `ProjectAtlas` worktree state without mutation.
    #[tool(
        name = "atlas_worktree_list",
        description = "List structurally discovered Git worktrees, short ProjectAtlas aliases, atlas availability, and telemetry synchronization state without changing Git or files."
    )]
    fn atlas_worktree_list(
        &self,
        Parameters(params): Parameters<AtlasWorktreeListParams>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let repository = self.control_git_repository()?;
            let store = open_atlas_store_read_only_for_project(
                &self.control_state.db_path,
                &self.control_state.root,
            )?;
            let include_retired = params.include_retired.unwrap_or(false);
            let registrations = store.worktree_registrations(include_retired)?;
            let structural_identities = repository
                .worktrees
                .iter()
                .filter(|entry| {
                    Self::worktree_registration_paths_are_utf8(&repository.common_directory, entry)
                })
                .map(|entry| normalize_native_path_display(&entry.administrative_directory))
                .collect::<HashSet<_>>();
            let (mut worktrees, unregistered): (Vec<_>, Vec<_>) = repository
                .worktrees
                .iter()
                .map(|entry| {
                    self.worktree_list_row(&repository.common_directory, entry, &registrations)
                })
                .partition(|row| {
                    !matches!(row.registration, McpWorktreeRegistrationState::Unregistered)
                });
            worktrees.extend(
                registrations
                    .iter()
                    .filter(|registration| {
                        registration.state == WorktreeRegistrationState::Active
                            && !structural_identities
                                .contains(&registration.git_administrative_directory)
                    })
                    .map(Self::missing_registered_worktree_row),
            );
            let total_worktrees = worktrees.len() + unregistered.len();
            worktrees.extend(
                unregistered
                    .into_iter()
                    .take(MCP_WORKTREE_LIST_MAX_ROWS.saturating_sub(worktrees.len())),
            );
            let truncated = total_worktrees > worktrees.len();
            let retired = registrations
                .iter()
                .filter(|registration| registration.state == WorktreeRegistrationState::Retired)
                .map(|registration| McpRetiredWorktreeRow {
                    alias: registration.alias.to_string(),
                    last_root: Some(registration.last_root.clone()),
                    project_instance_id: registration
                        .project_instance_id
                        .map(|identity| identity.to_string()),
                    accepted_telemetry_revision: registration.accepted_telemetry_revision,
                })
                .collect();
            Self::encode_named_payload(
                MCP_PAYLOAD_WORKTREES,
                &McpWorktreeListReport {
                    control_alias: MCP_MAIN_WORKTREE_ALIAS,
                    control_root: lossless_project_root_display(&self.control_state.root),
                    common_directory: lossless_native_path_display(&repository.common_directory),
                    worktrees,
                    retired,
                    total_worktrees,
                    truncated,
                },
            )
        })())
    }

    /// Register one active structural worktree in the immutable control atlas.
    #[tool(
        name = "atlas_worktree_add",
        description = "Register one structurally discovered Git worktree under a short ProjectAtlas alias without creating, moving, or switching Git worktrees."
    )]
    fn atlas_worktree_add(
        &self,
        Parameters(params): Parameters<AtlasWorktreeAddParams>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let requested = params.worktree.trim();
            if requested.is_empty() {
                return Err(CliError::InvalidInput(
                    MCP_ERROR_WORKTREE_SELECTOR_EMPTY.to_string(),
                ));
            }
            let repository = self.control_git_repository()?;
            let candidates = self.matching_worktree_candidates(&repository, requested);
            if candidates.len() != 1 {
                let ambiguous = !candidates.is_empty();
                let candidate_rows: Vec<McpWorktreeCandidate> = if candidates.is_empty() {
                    repository
                        .worktrees
                        .iter()
                        .filter(|entry| {
                            Self::active_worktree_root(entry)
                                .is_some_and(|root| root != self.control_state.root)
                        })
                        .filter_map(|entry| {
                            Self::worktree_candidate(&repository.common_directory, entry)
                        })
                        .take(MCP_WORKTREE_LIST_MAX_ROWS)
                        .collect()
                } else {
                    candidates
                        .into_iter()
                        .filter_map(|entry| {
                            Self::worktree_candidate(&repository.common_directory, entry)
                        })
                        .take(MCP_WORKTREE_LIST_MAX_ROWS)
                        .collect()
                };
                return Self::encode_named_payload(
                    MCP_PAYLOAD_WORKTREE,
                    &McpWorktreeMutationReport {
                        operation: McpWorktreeMutationOperation::Add,
                        status: if ambiguous {
                            McpWorktreeMutationStatus::Ambiguous
                        } else {
                            McpWorktreeMutationStatus::NotFound
                        },
                        selector: None,
                        alias: None,
                        root: None,
                        registration_id: None,
                        telemetry_sync: None,
                        candidates: candidate_rows,
                        blocker: Some(format!(
                            "selector {requested:?} did not identify exactly one active non-control worktree"
                        )),
                        git_unchanged: true,
                        files_unchanged: true,
                    },
                );
            }
            let entry = candidates[0];
            let root = Self::active_worktree_root(entry).ok_or_else(|| {
                CliError::InvalidInput(MCP_ERROR_WORKTREE_NO_LONGER_ACTIVE.to_string())
            })?;
            let administrative_identity =
                git_administrative_identity(&entry.administrative_directory)?;
            let alias = match params.alias.as_deref() {
                Some(alias) => WorktreeAlias::parse(alias.trim())?,
                None => Self::default_worktree_alias(root)?,
            };
            let db_path = Self::projectatlas_db_path(root);
            let mut project_instance_id = None;
            let local = (|| {
                let Some(store) = Self::open_local_worktree_atlas(root)? else {
                    return Ok(None);
                };
                let identity = Self::local_worktree_project_instance_id(&store, &db_path)?;
                project_instance_id = Some(identity);
                let snapshot = Self::local_worktree_usage_snapshot(&store, &db_path, identity)?;
                Ok::<_, CliError>(Some(LocalWorktreeAtlas {
                    project_instance_id: identity,
                    snapshot,
                }))
            })();
            let (local, blocker) = match local {
                Ok(local) => (local, None),
                Err(error) if project_instance_id.is_some() => return Err(error),
                Err(error) => (
                    None,
                    Some(format!(
                        "registration committed without local telemetry import: {error}"
                    )),
                ),
            };
            let (repository, entry) =
                self.revalidate_worktree_candidate(&repository, entry, &administrative_identity)?;
            let root = Self::active_worktree_root(&entry).ok_or_else(|| {
                CliError::InvalidInput(MCP_ERROR_WORKTREE_NO_LONGER_ACTIVE.to_string())
            })?;
            Self::revalidate_local_worktree_atlas_identity(root, project_instance_id)?;
            if let Some(local) = local.as_ref() {
                require_current_worktree_usage_snapshot(&db_path, root, &local.snapshot)?;
            }
            let control = Self::open_existing_mut_store(&self.control_state, &self.control_state)?;
            let created_at_epoch = Self::current_epoch_seconds()?;
            let (registration, telemetry_sync) = if let Some(local) = local.as_ref() {
                match control.register_worktree_with_usage_snapshot(
                    &alias,
                    &repository.common_directory,
                    &entry.administrative_directory,
                    &administrative_identity,
                    root,
                    local.project_instance_id,
                    &local.snapshot,
                    created_at_epoch,
                ) {
                    Ok((registration, state)) => (registration, Some(state)),
                    Err(error) => return Err(error.into()),
                }
            } else {
                (
                    control.register_worktree(
                        &alias,
                        &repository.common_directory,
                        &entry.administrative_directory,
                        &administrative_identity,
                        root,
                        project_instance_id,
                        created_at_epoch,
                    )?,
                    None,
                )
            };
            Self::encode_named_payload(
                MCP_PAYLOAD_WORKTREE,
                &McpWorktreeMutationReport {
                    operation: McpWorktreeMutationOperation::Add,
                    status: McpWorktreeMutationStatus::Registered,
                    selector: Some(Self::worktree_candidate_selector(&entry)),
                    alias: Some(alias.to_string()),
                    root: lossless_project_root_display(root),
                    registration_id: Some(registration.registration_id),
                    telemetry_sync,
                    candidates: Vec::new(),
                    blocker,
                    git_unchanged: true,
                    files_unchanged: true,
                },
            )
        })())
    }

    /// Final-sync and retire one `ProjectAtlas` alias without touching Git or files.
    #[tool(
        name = "atlas_worktree_remove",
        description = "Final-sync and retire one ProjectAtlas worktree alias while preserving retained token totals and leaving Git, source files, .projectatlas, and SQLite files untouched."
    )]
    fn atlas_worktree_remove(
        &self,
        Parameters(params): Parameters<AtlasWorktreeRemoveParams>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let alias = WorktreeAlias::parse(params.worktree.trim())?;
            let repository = self.control_git_repository()?;
            let control = Self::open_existing_mut_store(&self.control_state, &self.control_state)?;
            let registration = control.worktree_registration(&alias)?;
            let mut blocker = None;
            let entry = repository.worktrees.iter().find(|entry| {
                Self::worktree_registration_paths_are_utf8(&repository.common_directory, entry)
                    && normalize_native_path_display(&entry.administrative_directory)
                        == registration.git_administrative_directory
            });
            let entry = match entry {
                Some(entry) => match git_administrative_identity(&entry.administrative_directory) {
                    Ok(identity) if identity == registration.git_administrative_identity => {
                        Some(entry)
                    }
                    Ok(_) => {
                        blocker = Some(MCP_ERROR_WORKTREE_LIFECYCLE_CHANGED.to_string());
                        None
                    }
                    Err(error) => {
                        blocker = Some(error.to_string());
                        None
                    }
                },
                None => None,
            };
            let retired_at_epoch = Self::current_epoch_seconds()?;
            let (root_display, active_root) = match entry.map(|entry| &entry.state) {
                Some(GitWorktreeState::Active { root, .. }) => {
                    (lossless_project_root_display(root), Some(root.as_path()))
                }
                Some(GitWorktreeState::Invalid { issue }) => {
                    return Err(CliError::InvalidInput(format!(
                        "cannot retire worktree '{}' while its Git evidence is invalid at '{}': {:?}",
                        alias,
                        normalize_native_path_display(&issue.path),
                        issue.kind
                    )));
                }
                Some(GitWorktreeState::Missing { .. }) | None => {
                    blocker
                        .get_or_insert_with(|| MCP_WORKTREE_MISSING_RETENTION_REASON.to_string());
                    (Some(registration.last_root.clone()), None)
                }
            };
            let (retired, telemetry_sync, final_blocker) = Self::retire_registered_worktree(
                &control,
                &registration,
                active_root,
                retired_at_epoch,
                blocker,
            )?;
            Self::encode_named_payload(
                MCP_PAYLOAD_WORKTREE,
                &McpWorktreeMutationReport {
                    operation: McpWorktreeMutationOperation::Remove,
                    status: McpWorktreeMutationStatus::Retired,
                    selector: entry.map(Self::worktree_candidate_selector),
                    alias: Some(alias.to_string()),
                    root: root_display,
                    registration_id: Some(retired.registration_id),
                    telemetry_sync,
                    candidates: Vec::new(),
                    blocker: final_blocker,
                    git_unchanged: true,
                    files_unchanged: true,
                },
            )
        })())
    }

    /// Initialize a `ProjectAtlas` project-local config surface.
    #[tool(
        name = "atlas_init",
        description = "Initialize ProjectAtlas project-local config, database, host MCP configs, scan/index, and purpose handoff."
    )]
    fn atlas_init(&self, Parameters(params): Parameters<AtlasInitParams>) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state = self.init_project_root(params.project_path, params.worktree)?;
            let config_path = init_config_path(&state.root, state.config_path.as_deref());
            let mut report = self.run_registered_worktree_init(
                &state,
                &config_path,
                &InitBootstrapOptions {
                    no_scan: params.no_scan.unwrap_or(false),
                    force_rescan: params.force_rescan.unwrap_or(false),
                    text_index_max_bytes: params.text_index_max_bytes,
                },
            )?;
            crate::write_init_mcp_config_files(
                &mut report,
                &state.root.join(PROJECTATLAS_DIR_NAME),
                &state.db_path,
                &config_path,
                false,
            );
            Self::encode_named_payload(MCP_PAYLOAD_INIT, &report)
        })())
    }

    /// Write an explicit compatibility map export.
    #[tool(
        name = "atlas_map",
        description = "Write the explicit compatibility ProjectAtlas map export for older workflows."
    )]
    fn atlas_map(&self, Parameters(params): Parameters<AtlasMapParams>) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state = self.admin_project_root(params.project_path, params.worktree)?;
            let report = Self::build_map_report(
                &state,
                params.json.unwrap_or(false),
                params.force.unwrap_or(false),
            )?;
            Self::encode_named_payload(MCP_PAYLOAD_MAP, &report)
        })())
    }

    /// Return root/DB/config diagnostics.
    #[tool(
        name = "atlas_root",
        description = "Show or verify ProjectAtlas root, DB, config, and runtime identity."
    )]
    fn atlas_root(&self, Parameters(params): Parameters<AtlasRootParams>) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            if let Some(control_root) = params.control_root.as_deref() {
                if params.project_path.is_some()
                    || params.worktree.is_some()
                    || params.verify.unwrap_or(false)
                {
                    return Err(CliError::InvalidInput(
                        MCP_ERROR_ROOT_CONTROL_CONFLICT.to_owned(),
                    ));
                }
                let report = build_repository_control_report(Path::new(control_root))?;
                return Ok(render_repository_control_report(&report));
            }
            let state = self.admin_project_root(params.project_path, params.worktree)?;
            let report = build_root_report(&state.db_path, state.config_path.as_deref())?;
            if params.verify.unwrap_or(false) && report.verified {
                verify_project_database(&state.db_path, &state.root)?;
            }
            Self::with_selected_project_audit(
                &state,
                state.worktree.is_some(),
                render_root_report(&report),
            )
        })())
    }

    /// Bind, move, or detach a project root and then make it active.
    #[tool(
        name = "atlas_root_set",
        description = "Bind a repository root, generate project-local MCP configs, and make it active for later MCP calls."
    )]
    fn atlas_root_set(
        &self,
        Parameters(params): Parameters<AtlasRootSetParams>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let root = canonical_project_root(Path::new(&params.root))?;
            let report = crate::bind_project_root(
                &root,
                params.transition.unwrap_or(RootTransition::Bind),
                params.nearest_project.unwrap_or(false),
            )?;
            let state = Self::project_state_from_root(&root)?;
            self.set_active_project_state(state)?;
            Ok(render_root_report(&report))
        })())
    }

    /// Return the effective `ProjectAtlas` config.
    #[tool(
        name = "atlas_config",
        description = "Return the effective ProjectAtlas scan, purpose, and output configuration."
    )]
    fn atlas_config(
        &self,
        Parameters(params): Parameters<AtlasProjectParams>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state = self.admin_project_root(params.project_path, params.worktree)?;
            let report = effective_config_report(&Self::load_config_for_state(&state)?);
            Self::encode_named_payload(MCP_PAYLOAD_CONFIG, &report)
        })())
    }

    /// Return the effective `ProjectAtlas` ignore policy.
    #[tool(
        name = "atlas_ignore_list",
        description = "List effective ProjectAtlas manual ignore policy and inherited .gitignore status."
    )]
    fn atlas_ignore_list(
        &self,
        Parameters(params): Parameters<AtlasProjectParams>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state = self.admin_project_root(params.project_path, params.worktree)?;
            let report = list_ignore_entries(state.config_path.as_deref(), &state.root)?;
            Self::encode_named_payload(MCP_PAYLOAD_IGNORE, &report)
        })())
    }

    /// Create a project-root `.gitignore` when it is absent.
    #[tool(
        name = "atlas_ignore_init_gitignore",
        description = "Create a project-root .gitignore when it is missing."
    )]
    fn atlas_ignore_init_gitignore(
        &self,
        Parameters(params): Parameters<AtlasProjectParams>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state = self.admin_project_root(params.project_path, params.worktree)?;
            let report = init_gitignore(state.config_path.as_deref(), &state.root)?;
            Self::encode_named_payload(MCP_PAYLOAD_GITIGNORE, &report)
        })())
    }

    /// Add a manual `ProjectAtlas` ignore entry.
    #[tool(
        name = "atlas_ignore_add",
        description = "Add one manual ProjectAtlas ignore entry to the selected project's config."
    )]
    fn atlas_ignore_add(
        &self,
        Parameters(params): Parameters<AtlasIgnoreMutationParams>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state = self.admin_project_root(params.project_path, params.worktree)?;
            let kind = Self::parse_ignore_kind(params.kind.as_deref(), true)?.ok_or_else(|| {
                CliError::InvalidInput(MCP_ERROR_IGNORE_KIND_REQUIRED_FOR_ADD.to_owned())
            })?;
            let report = add_ignore_entry(
                state.config_path.as_deref(),
                &state.root,
                kind,
                &params.value,
            )?;
            Self::encode_named_payload(MCP_PAYLOAD_IGNORE, &report)
        })())
    }

    /// Remove a manual `ProjectAtlas` ignore entry.
    #[tool(
        name = "atlas_ignore_remove",
        description = "Remove one manual ProjectAtlas ignore entry from the selected project's config."
    )]
    fn atlas_ignore_remove(
        &self,
        Parameters(params): Parameters<AtlasIgnoreMutationParams>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state = self.admin_project_root(params.project_path, params.worktree)?;
            let kind = Self::parse_ignore_kind(params.kind.as_deref(), false)?;
            let report = remove_ignore_entry(
                state.config_path.as_deref(),
                &state.root,
                kind,
                &params.value,
            )?;
            Self::encode_named_payload(MCP_PAYLOAD_IGNORE, &report)
        })())
    }

    /// Scan a repository, import purpose metadata, rebuild symbols, and return an overview.
    #[tool(
        name = "atlas_scan",
        description = "Scan repository structure, import ProjectAtlas purpose metadata, rebuild symbols, and return a TOON overview."
    )]
    fn atlas_scan(&self, Parameters(params): Parameters<AtlasScanParams>) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let background = params.background.unwrap_or(false);
            let (state, path) = if background {
                self.background_state_and_root_path(
                    params.project_path,
                    params.worktree,
                    params.path,
                    nearest_project,
                )?
            } else {
                self.state_and_root_path(
                    params.project_path,
                    params.worktree,
                    params.path,
                    nearest_project,
                )?
            };
            let symbol_options = SymbolBuildOptions::new(
                params.max_bytes.unwrap_or(MAX_SYMBOL_FILE_BYTES),
                params.max_workers,
                params.timeout_seconds,
            );
            let text_index_max_bytes = params.text_index_max_bytes;
            if background {
                let control_state = self.control_state.clone();
                let task = self.start_index_task(
                    McpTaskOperation::Scan,
                    symbol_options,
                    MCP_TOOL_ATLAS_OVERVIEW,
                    move |control, symbol_options| {
                        let plan = ScanRuntimePlan::for_path_controlled(
                            state.config_path.as_deref(),
                            &path,
                            text_index_max_bytes,
                            control,
                        )?;
                        let mut store = Self::open_mut_store(&state, &control_state)?;
                        run_scan_pipeline_controlled(&mut store, &plan, &symbol_options, control)?;
                        Ok(())
                    },
                )?;
                return Self::encode_named_payload(MCP_PAYLOAD_TASK_START, &task);
            }
            let control = index_work_control(&symbol_options);
            let plan = ScanRuntimePlan::for_path_controlled(
                state.config_path.as_deref(),
                &path,
                text_index_max_bytes,
                &control,
            )?;
            let mut store = Self::open_mut_store(&state, &self.control_state)?;
            let report =
                run_scan_pipeline_controlled(&mut store, &plan, &symbol_options, &control)?;
            Self::encode_named_payload(MCP_PAYLOAD_SCAN, &report)
        })())
    }

    /// Render one verified overview response with optional request cancellation.
    fn atlas_overview_response(
        &self,
        params: AtlasProjectParams,
        context: Option<RequestContext<RoleServer>>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state = self.state_for_target(params.project_path, params.worktree)?;
            self.with_fresh_string_and_usage_for_request(&state, context, |store, stamp| {
                let overview = store.overview()?;
                let toon = render_overview(&overview);
                let usage = Self::telemetry_enabled()
                    .then(|| self.estimated_source_tokens_cached(&state, store, &stamp, None, None))
                    .and_then(Result::ok)
                    .map(|baseline_tokens| {
                        McpUsageIntent::directory_walk(
                            MCP_EVENT_ATLAS_OVERVIEW,
                            None,
                            None,
                            baseline_tokens,
                        )
                    });
                Ok((toon, usage))
            })
        })())
    }

    /// Return the indexed repository overview.
    #[tool(
        name = "atlas_overview",
        description = "Return a compact TOON overview of indexed files, folders, and purpose coverage."
    )]
    fn atlas_overview(
        &self,
        Parameters(params): Parameters<AtlasProjectParams>,
        context: RequestContext<RoleServer>,
    ) -> McpToolTextResult {
        self.atlas_overview_response(params, Some(context))
    }

    /// Rank folders before an agent chooses a work area.
    #[tool(
        name = "atlas_folders",
        description = "Rank repository folders by query and purpose so agents choose a work area before opening files."
    )]
    fn atlas_folders(
        &self,
        Parameters(params): Parameters<AtlasQueryParams>,
        context: RequestContext<RoleServer>,
    ) -> McpToolTextResult {
        self.atlas_folders_response(params, Some(context))
    }

    /// Build the folders response with optional request telemetry context.
    fn atlas_folders_response(
        &self,
        params: AtlasQueryParams,
        context: Option<RequestContext<RoleServer>>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state = self.state_for_target(params.project_path, params.worktree)?;
            let query = Self::query_or_empty(params.query);
            self.with_fresh_string_and_usage_for_request(&state, context, |store, stamp| {
                let selected =
                    ranked_folder_nodes_with_reasons(store, &query, params.limit.unwrap_or(10))?;
                let toon = render_ranked_nodes(NODE_LABEL_FOLDERS, &selected);
                let usage = Self::telemetry_enabled()
                    .then(|| self.estimated_source_tokens_cached(&state, store, &stamp, None, None))
                    .and_then(Result::ok)
                    .map(|baseline_tokens| {
                        McpUsageIntent::directory_walk(
                            MCP_EVENT_ATLAS_FOLDERS,
                            None,
                            Some(query.clone()),
                            baseline_tokens,
                        )
                    });
                Ok((toon, usage))
            })
        })())
    }

    /// Rank files after an agent has chosen a folder or query.
    #[tool(
        name = "atlas_files",
        description = "Rank repository files by query, purpose, optional folder, and optional indexed text fallback before an agent opens source."
    )]
    fn atlas_files(
        &self,
        Parameters(params): Parameters<AtlasFilesParams>,
        context: RequestContext<RoleServer>,
    ) -> McpToolTextResult {
        self.atlas_files_response(params, Some(context))
    }

    /// Build the files response with optional request telemetry context.
    fn atlas_files_response(
        &self,
        params: AtlasFilesParams,
        context: Option<RequestContext<RoleServer>>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let content_selection = parse_content_selection(params.content_selection.as_deref())?;
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let (state, folder_filter, routed_project) = self.state_and_optional_folder_filter(
                params.project_path.as_deref(),
                params.worktree.as_deref(),
                params.folder.as_deref(),
                nearest_project,
            )?;
            let query = Self::query_or_empty(params.query);
            self.with_fresh_string_and_usage_for_request(&state, context, |store, stamp| {
                let selected = classified_ranked_file_nodes_with_reasons(
                    store,
                    &query,
                    folder_filter.as_deref(),
                    params.file_pattern.as_deref(),
                    params.limit.unwrap_or(10),
                    params.include_content.unwrap_or(false),
                    content_selection,
                )?;
                let toon = Self::with_selected_project_audit(
                    &state,
                    routed_project,
                    encode_agent_payload(&serde_json::json!({
                        NODE_LABEL_FILES: render_classified_ranked_file_rows(&selected),
                    })),
                )?;
                let usage = Self::telemetry_enabled()
                    .then(|| {
                        self.estimated_source_tokens_cached(
                            &state,
                            store,
                            &stamp,
                            folder_filter.as_deref(),
                            params.file_pattern.as_deref(),
                        )
                    })
                    .and_then(Result::ok)
                    .map(|baseline_tokens| {
                        McpUsageIntent::estimate(
                            MCP_EVENT_ATLAS_FILES,
                            params
                                .file_pattern
                                .clone()
                                .or_else(|| folder_filter.clone()),
                            Some(query.clone()),
                            baseline_tokens,
                        )
                    });
                Ok((toon, usage))
            })
        })())
    }

    /// Recommend the next indexed folders, files, and inspection commands.
    #[tool(
        name = "atlas_next",
        description = "Recommend top indexed folders/files with reasons and deterministic follow-up commands for a task query."
    )]
    fn atlas_next(
        &self,
        Parameters(params): Parameters<AtlasNextParams>,
        context: RequestContext<RoleServer>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let content_selection = parse_content_selection(params.content_selection.as_deref())?;
            let state = self.state_for_target(params.project_path, params.worktree)?;
            let query = Self::query_or_empty(params.query);
            self.with_fresh_string_and_usage_for_request(&state, Some(context), |store, stamp| {
                let report = next_step_report_with_selection(
                    store,
                    &query,
                    params.limit,
                    content_selection,
                )?;
                let payload = next_step_report_payload(&report);
                let toon = Self::encode_named_payload(MCP_PAYLOAD_NEXT, &payload)?;
                let usage = Self::telemetry_enabled()
                    .then(|| self.estimated_source_tokens_cached(&state, store, &stamp, None, None))
                    .and_then(Result::ok)
                    .map(|baseline_tokens| {
                        McpUsageIntent::directory_walk(
                            MCP_EVENT_ATLAS_NEXT,
                            None,
                            Some(query.clone()),
                            baseline_tokens,
                        )
                    });
                Ok((toon, usage))
            })
        })())
    }

    /// Build a compact file outline.
    #[tool(
        name = "atlas_outline",
        description = "Return compact TOON outline and preview context for a selected file."
    )]
    fn atlas_outline(
        &self,
        Parameters(params): Parameters<AtlasOutlineParams>,
        context: RequestContext<RoleServer>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let resolved = self.state_and_file_key(
                params.project_path.as_deref(),
                params.worktree.as_deref(),
                &params.file,
                nearest_project,
            )?;
            let state = resolved.state;
            self.with_fresh_string_and_usage_for_request(&state, Some(context), |store, _stamp| {
                let file_key = validated_indexed_file_key(store, Path::new(&resolved.key))?;
                let content = read_indexed_file_content(store, &file_key)?;
                let language = store
                    .load_node_by_path(&file_key)?
                    .and_then(|node| node.node.language);
                let outline =
                    build_outline(&file_key, language, &content, params.lines.unwrap_or(12));
                let toon = Self::with_selected_project_audit(
                    &state,
                    resolved.routed_project,
                    render_outline(&outline),
                )?;
                let usage = Some(McpUsageIntent::text(
                    MCP_EVENT_ATLAS_OUTLINE,
                    Some(file_key),
                    content,
                ));
                Ok((toon, usage))
            })
        })())
    }

    /// Render one verified structured file-summary response.
    fn atlas_file_summary_response(
        &self,
        params: &AtlasFileSummaryParams,
        context: Option<RequestContext<RoleServer>>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let content_selection = parse_content_selection(params.content_selection.as_deref())?;
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let resolved = self.state_and_file_key(
                params.project_path.as_deref(),
                params.worktree.as_deref(),
                &params.file,
                nearest_project,
            )?;
            let state = resolved.state;
            self.with_fresh_string_and_usage_for_request(&state, context, |store, _stamp| {
                let file_key = validated_indexed_file_key(store, Path::new(&resolved.key))?;
                let content = read_indexed_file_content(store, &file_key)?;
                let report = build_file_summary_from_source_with_selection(
                    store,
                    Path::new(&file_key),
                    params.limit.unwrap_or(DEFAULT_FILE_SUMMARY_LIMIT),
                    &content,
                    content_selection,
                )?;
                let rendered = if params.compact.unwrap_or(false) {
                    encode_agent_payload(&McpFileSummaryPayload {
                        file_summary: McpFileSummary::from(&report),
                    })
                } else {
                    render_file_summary(&report)
                };
                let toon =
                    Self::with_selected_project_audit(&state, resolved.routed_project, rendered)?;
                let usage = Some(McpUsageIntent::text(
                    MCP_EVENT_ATLAS_FILE_SUMMARY,
                    Some(report.file_path),
                    content,
                ));
                Ok((toon, usage))
            })
        })())
    }

    /// Return deterministic structured file intelligence from the deep index.
    #[tool(
        name = "atlas_file_summary",
        description = "Return structured TOON file intelligence: file purpose, content summary, imports, symbols, line ranges, and calls."
    )]
    fn atlas_file_summary(
        &self,
        Parameters(params): Parameters<AtlasFileSummaryParams>,
        context: RequestContext<RoleServer>,
    ) -> McpToolTextResult {
        self.atlas_file_summary_response(&params, Some(context))
    }

    /// Search selected indexed files with optional context lines.
    #[tool(
        name = "atlas_search",
        description = "Search indexed files with literal, regex, or fuzzy matching, file filters, pagination, and TOON results."
    )]
    fn atlas_search(
        &self,
        Parameters(params): Parameters<AtlasSearchParams>,
        context: RequestContext<RoleServer>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let content_selection = parse_content_selection(params.content_selection.as_deref())?;
            let state =
                self.state_for_target(params.project_path.clone(), params.worktree.clone())?;
            self.with_fresh_string_and_usage_controlled_for_request(
                &state,
                Some(context),
                |store, _stamp, control| {
                    let report = search_indexed_files_with_control(
                        store,
                        &SearchQuery {
                            pattern: &params.pattern,
                            regex: params.regex.unwrap_or(false),
                            fuzzy: params.fuzzy.unwrap_or(false),
                            case_sensitive: params.case_sensitive.unwrap_or(false),
                            file_pattern: params.file_pattern.as_deref(),
                            context_lines: params.context_lines.unwrap_or(0),
                            start_index: params.start_index.unwrap_or(0),
                            limit: params.limit.unwrap_or(20),
                            content_selection,
                            retrieval_mode: params.retrieval_mode.unwrap_or_default().into(),
                        },
                        Some(control),
                    )?;
                    let toon = render_search_report(&report);
                    let usage = Some(McpUsageIntent::estimate(
                        MCP_EVENT_ATLAS_SEARCH,
                        params.file_pattern.clone(),
                        Some(params.pattern.clone()),
                        byte_count_to_tokens(report.searched_bytes),
                    ));
                    Ok((toon, usage))
                },
            )
        })())
    }

    /// Return an exact line or symbol slice from a selected file.
    #[tool(
        name = "atlas_slice",
        description = "Return exact source for a selected line range or indexed symbol, after folder/file orientation."
    )]
    fn atlas_slice(
        &self,
        Parameters(params): Parameters<AtlasSliceParams>,
        context: RequestContext<RoleServer>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let content_selection = parse_content_selection(params.content_selection.as_deref())?;
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let resolved = self.state_and_file_key(
                params.project_path.as_deref(),
                params.worktree.as_deref(),
                &params.file,
                nearest_project,
            )?;
            let state = resolved.state;
            self.with_fresh_string_and_usage_for_request(&state, Some(context), |store, _stamp| {
                let file_key = validated_indexed_file_key(store, Path::new(&resolved.key))?;
                let file = PathBuf::from(&file_key);
                let content = read_indexed_file_content(store, &file_key)?;
                let output_budget = CodeSliceBudget::new(
                    params
                        .output_bytes
                        .unwrap_or(CodeSliceBudget::DEFAULT_OUTPUT_BYTES),
                )?;
                let report = if let Some(symbol) = params.symbol.as_ref() {
                    read_symbol_slice_from_source_bounded_with_selection(
                        store,
                        &file,
                        &SymbolSliceSelector {
                            name: symbol,
                            parent: params.symbol_parent.as_deref().and_then(nonempty_str),
                            kind: params.symbol_kind.as_deref().and_then(nonempty_str),
                            signature: params.symbol_signature.as_deref().and_then(nonempty_str),
                            line: params.symbol_line,
                        },
                        &content,
                        output_budget,
                        content_selection,
                    )?
                } else {
                    if params
                        .symbol_parent
                        .as_deref()
                        .and_then(nonempty_str)
                        .is_some()
                        || params
                            .symbol_kind
                            .as_deref()
                            .and_then(nonempty_str)
                            .is_some()
                        || params
                            .symbol_signature
                            .as_deref()
                            .and_then(nonempty_str)
                            .is_some()
                        || params.symbol_line.is_some()
                    {
                        return Err(CliError::InvalidInput(
                            SYMBOL_DISAMBIGUATOR_WITHOUT_SYMBOL_ERROR.to_string(),
                        ));
                    }
                    let start_line = params.start_line.ok_or_else(|| {
                        CliError::InvalidInput(START_LINE_REQUIRED_ERROR.to_string())
                    })?;
                    read_indexed_code_slice_from_source_bounded_with_selection(
                        store,
                        &file,
                        start_line,
                        params.end_line,
                        &content,
                        output_budget,
                        content_selection,
                    )?
                };
                let toon = report.fit_output(|report| {
                    Self::with_selected_project_audit(
                        &state,
                        resolved.routed_project,
                        render_code_slice(report),
                    )
                })?;
                let usage = Some(McpUsageIntent::text(
                    MCP_EVENT_ATLAS_SLICE,
                    Some(report.slice().path.clone()),
                    content,
                ));
                Ok((toon, usage))
            })
        })())
    }

    /// Rebuild symbol graphs for indexed files.
    #[tool(
        name = "atlas_symbols_build",
        description = "Rebuild ProjectAtlas symbol graphs for indexed files and return a TOON build report."
    )]
    fn atlas_symbols_build(
        &self,
        Parameters(params): Parameters<AtlasScanParams>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let background = params.background.unwrap_or(false);
            let (state, path) = if background {
                self.background_state_and_root_path(
                    params.project_path,
                    params.worktree,
                    params.path,
                    nearest_project,
                )?
            } else {
                self.state_and_root_path(
                    params.project_path,
                    params.worktree,
                    params.path,
                    nearest_project,
                )?
            };
            let options = SymbolBuildOptions::new(
                params.max_bytes.unwrap_or(MAX_SYMBOL_FILE_BYTES),
                params.max_workers,
                params.timeout_seconds,
            );
            let text_index_max_bytes = params.text_index_max_bytes;
            if background {
                let control_state = self.control_state.clone();
                let task = self.start_index_task(
                    McpTaskOperation::SymbolsBuild,
                    options,
                    MCP_TOOL_ATLAS_SYMBOLS,
                    move |control, options| {
                        let plan = ScanRuntimePlan::for_path_controlled(
                            state.config_path.as_deref(),
                            &path,
                            text_index_max_bytes,
                            control,
                        )?;
                        let mut store = Self::open_mut_store(&state, &control_state)?;
                        run_symbol_build_pipeline_controlled(
                            &mut store, &plan, &options, None, control,
                        )?;
                        Ok(())
                    },
                )?;
                return Self::encode_named_payload(MCP_PAYLOAD_TASK_START, &task);
            }
            let control = index_work_control(&options);
            let plan = ScanRuntimePlan::for_path_controlled(
                state.config_path.as_deref(),
                &path,
                text_index_max_bytes,
                &control,
            )?;
            let mut store = Self::open_mut_store(&state, &self.control_state)?;
            let report =
                run_symbol_build_pipeline_controlled(&mut store, &plan, &options, None, &control)?;
            Self::encode_named_payload(MCP_PAYLOAD_SYMBOLS_BUILD, &report)
        })())
    }

    /// Render one verified symbol-list response.
    fn atlas_symbols_response(
        &self,
        params: &AtlasSymbolsParams,
        context: Option<RequestContext<RoleServer>>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let content_selection = parse_content_selection(params.content_selection.as_deref())?;
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let (state, file, routed_project) = self.state_and_optional_file_key(
                params.project_path.as_deref(),
                params.worktree.as_deref(),
                params.file.as_deref(),
                nearest_project,
            )?;
            self.with_fresh_string_and_usage_for_request(&state, context, |store, _stamp| {
                let file = file
                    .as_deref()
                    .map(|path| validated_indexed_file_key(store, Path::new(path)))
                    .transpose()?;
                let symbols = store.load_classified_symbols(
                    file.as_deref(),
                    params.query.as_deref(),
                    content_selection,
                    params.limit.unwrap_or(50),
                )?;
                let toon = Self::with_selected_project_audit(
                    &state,
                    routed_project,
                    encode_agent_payload(&serde_json::json!({
                        NODE_LABEL_SYMBOLS: render_classified_symbol_rows(&symbols),
                    })),
                )?;
                let usage = Self::telemetry_enabled()
                    .then(|| {
                        estimated_source_tokens_for_paths(
                            store,
                            symbols
                                .iter()
                                .map(|classified| classified.symbol.path.as_str()),
                        )
                    })
                    .and_then(Result::ok)
                    .map(|baseline_tokens| {
                        McpUsageIntent::estimate(
                            MCP_EVENT_ATLAS_SYMBOLS,
                            file.clone(),
                            params.query.clone(),
                            baseline_tokens,
                        )
                    });
                Ok((toon, usage))
            })
        })())
    }

    /// List indexed symbols.
    #[tool(
        name = "atlas_symbols",
        description = "List indexed symbols by optional file and query as compact TOON."
    )]
    fn atlas_symbols(
        &self,
        Parameters(params): Parameters<AtlasSymbolsParams>,
        context: RequestContext<RoleServer>,
    ) -> McpToolTextResult {
        self.atlas_symbols_response(&params, Some(context))
    }

    /// Render the shared detailed/analysis contract through one selected store set.
    fn detailed_symbol_relations_response(
        state: &McpProjectState,
        routed_project: bool,
        file: &str,
        params: &AtlasSymbolRelationsParams,
        content_selection: ContentSelection,
        analysis: bool,
        stores: SymbolRelationStores<'_>,
        control: &IndexWorkControl,
    ) -> Result<(String, Option<McpUsageIntent>), CliError> {
        if params.query.is_some() {
            return Err(CliError::Service(ServiceError::InvalidInput(
                MCP_ERROR_DETAILED_RELATION_QUERY.to_string(),
            )));
        }
        let primary = stores.primary();
        let file = validated_indexed_file_key(primary, Path::new(file))?;
        let graph_file = RepositoryFilePath::new(Path::new(&file))
            .map_err(|error| CliError::Service(ServiceError::InvalidInput(error.to_string())))?;
        let anchor = if let Some(symbol) = params.symbol.as_ref() {
            if symbol.is_empty() {
                return Err(CliError::Service(ServiceError::InvalidInput(
                    MCP_ERROR_DETAILED_RELATION_SYMBOL.to_string(),
                )));
            }
            RelationAnchor::Symbol {
                file: graph_file,
                name: symbol.clone(),
                symbol_kind: params
                    .symbol_kind
                    .as_deref()
                    .and_then(nonempty_str)
                    .map(parse_symbol_kind)
                    .transpose()?,
                parent: params
                    .symbol_parent
                    .as_deref()
                    .and_then(nonempty_str)
                    .map(ToString::to_string),
                signature: params
                    .symbol_signature
                    .as_deref()
                    .and_then(nonempty_str)
                    .map(ToString::to_string),
            }
        } else {
            if params
                .symbol_parent
                .as_deref()
                .and_then(nonempty_str)
                .is_some()
                || params
                    .symbol_kind
                    .as_deref()
                    .and_then(nonempty_str)
                    .is_some()
                || params
                    .symbol_signature
                    .as_deref()
                    .and_then(nonempty_str)
                    .is_some()
            {
                return Err(CliError::Service(ServiceError::InvalidInput(
                    MCP_ERROR_DETAILED_RELATION_DISAMBIGUATOR.to_string(),
                )));
            }
            RelationAnchor::File { file: graph_file }
        };
        let rows = u32::try_from(params.limit.unwrap_or(50)).map_err(|_overflow| {
            CliError::Service(ServiceError::InvalidInput(
                MCP_ERROR_DETAILED_RELATION_LIMIT.to_string(),
            ))
        })?;
        let limits = GraphLimits::new(
            rows,
            params.occurrence_limit.unwrap_or(25),
            params.depth.unwrap_or(1),
            params.output_bytes.unwrap_or(256 * 1024),
        )
        .map_err(|error| CliError::Service(ServiceError::InvalidInput(error.to_string())))?;
        let relations = DetailedRelationQuery {
            anchor,
            direction: parse_relation_direction(
                params
                    .direction
                    .as_deref()
                    .unwrap_or(MCP_SYMBOL_RELATION_DIRECTION_DEFAULT),
            )?,
            relation: params
                .relation
                .as_deref()
                .map(parse_coverage_relation)
                .transpose()?,
            minimum_confidence: parse_relation_confidence(
                params
                    .minimum_confidence
                    .as_deref()
                    .unwrap_or(MCP_SYMBOL_RELATION_CONFIDENCE_DEFAULT),
            )?,
            resolution: parse_relation_resolution(
                params
                    .resolution
                    .as_deref()
                    .unwrap_or(MCP_SYMBOL_RELATION_RESOLUTION_DEFAULT),
            )?,
            content_selection,
            include_occurrences: params.include_occurrences.unwrap_or(false),
            budget: DetailedRelationBudget::from_graph_limits(limits).with_aggregate_limits(
                params.edge_limit,
                params.node_limit,
                params.visited_limit,
                params.occurrence_total_limit,
                params.intermediate_bytes,
                params.deadline_ms,
            )?,
            cursor: params.cursor.clone(),
        };
        let usage = matches!(&stores, SymbolRelationStores::Single(_))
            .then(|| {
                Self::telemetry_enabled()
                    .then(|| {
                        estimated_source_tokens_for_paths(primary, std::iter::once(file.as_str()))
                    })
                    .and_then(Result::ok)
                    .map(|baseline_tokens| {
                        McpUsageIntent::estimate(
                            MCP_EVENT_ATLAS_SYMBOL_RELATIONS,
                            Some(file.clone()),
                            params.symbol.clone(),
                            baseline_tokens,
                        )
                    })
            })
            .flatten();

        if analysis {
            let mode = match params
                .analysis_mode
                .as_deref()
                .unwrap_or(MCP_RELATION_ANALYSIS_MODE_ARCHITECTURE)
            {
                MCP_RELATION_ANALYSIS_MODE_ARCHITECTURE => RelationAnalysisMode::Architecture,
                MCP_RELATION_ANALYSIS_MODE_IMPACT => RelationAnalysisMode::Impact,
                MCP_RELATION_ANALYSIS_MODE_TRACE => RelationAnalysisMode::Trace,
                _unsupported => {
                    return Err(CliError::Service(ServiceError::InvalidInput(
                        MCP_ERROR_UNSUPPORTED_ANALYSIS_MODE.to_string(),
                    )));
                }
            };
            let trace_target = relation_analysis_trace_target(primary, params)?;
            let vcs_explicit =
                params.vcs.is_some() || params.vcs_base.is_some() || params.vcs_head.is_some();
            let vcs = relation_analysis_vcs(params)?;
            let query = RelationAnalysisQuery {
                relations,
                mode,
                trace_target,
                vcs: (mode == RelationAnalysisMode::Impact || vcs_explicit).then_some(vcs),
                include_communities: params.include_communities.unwrap_or(false),
                include_cycles: params.include_cycles.unwrap_or(false),
                include_dead_code: params.include_dead_code.unwrap_or(false),
            };
            let toon = match stores {
                SymbolRelationStores::Single(store) => {
                    let draft = load_relation_analysis(store, &query, Some(control))?;
                    draft
                        .fit_output(|report, control| {
                            Self::with_selected_project_audit_controlled(
                                state,
                                routed_project,
                                controlled_named_output(
                                    OutputFormat::Toon,
                                    MCP_PAYLOAD_SYMBOL_RELATIONS,
                                    report,
                                    control,
                                )?,
                                control,
                            )
                        })?
                        .1
                }
                SymbolRelationStores::Federated(stores) => {
                    let draft = load_federated_relation_analysis(stores, &query, Some(control))?;
                    draft
                        .fit_output(|report, control| {
                            Self::with_selected_project_audit_controlled(
                                state,
                                routed_project,
                                controlled_named_output(
                                    OutputFormat::Toon,
                                    MCP_PAYLOAD_SYMBOL_RELATIONS,
                                    report,
                                    control,
                                )?,
                                control,
                            )
                        })?
                        .1
                }
            };
            return Ok((toon, usage));
        }

        let toon = match stores {
            SymbolRelationStores::Single(store) => {
                let draft = load_detailed_relation_page(store, &relations, Some(control))?;
                draft
                    .fit_output(Some(control), |report| {
                        let payload = if params.compact.unwrap_or(false) {
                            Self::encode_named_payload(
                                MCP_PAYLOAD_SYMBOL_RELATIONS,
                                &McpCompactDetailedRelationReport::new(report, &file, params),
                            )?
                        } else {
                            Self::encode_named_payload(MCP_PAYLOAD_SYMBOL_RELATIONS, report)?
                        };
                        Self::with_selected_project_audit(state, routed_project, payload)
                    })?
                    .1
            }
            SymbolRelationStores::Federated(stores) => {
                let draft = load_federated_detailed_relations(stores, &relations, Some(control))?;
                draft
                    .fit_output(Some(control), |report| {
                        let payload = if params.compact.unwrap_or(false) {
                            Self::encode_named_payload(
                                MCP_PAYLOAD_SYMBOL_RELATIONS,
                                &McpCompactFederatedDetailedRelationReport::new(
                                    report, &file, params,
                                ),
                            )?
                        } else {
                            Self::encode_named_payload(MCP_PAYLOAD_SYMBOL_RELATIONS, report)?
                        };
                        Self::with_selected_project_audit(state, routed_project, payload)
                    })?
                    .1
            }
        };
        Ok((toon, usage))
    }

    /// Render one verified legacy or detailed symbol-relation response.
    fn atlas_symbol_relations_response(
        &self,
        params: &AtlasSymbolRelationsParams,
        context: Option<RequestContext<RoleServer>>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let (detailed, analysis) = match params
                .view
                .as_deref()
                .unwrap_or(MCP_SYMBOL_RELATION_VIEW_LEGACY)
            {
                MCP_SYMBOL_RELATION_VIEW_LEGACY => (false, false),
                MCP_SYMBOL_RELATION_VIEW_DETAILED => (true, false),
                MCP_SYMBOL_RELATION_VIEW_ANALYSIS => (true, true),
                _unsupported => {
                    return Err(CliError::Service(ServiceError::InvalidInput(
                        MCP_ERROR_SYMBOL_RELATION_VIEW.to_string(),
                    )));
                }
            };
            if params.compact.unwrap_or(false) && (!detailed || analysis) {
                return Err(CliError::Service(ServiceError::InvalidInput(
                    MCP_ERROR_COMPACT_DETAILED_RELATION_VIEW.to_string(),
                )));
            }
            if !analysis && relation_analysis_controls_present(params) {
                return Err(CliError::Service(ServiceError::InvalidInput(
                    MCP_ERROR_ANALYSIS_VIEW_REQUIRED.to_string(),
                )));
            }
            let content_selection = parse_content_selection(params.content_selection.as_deref())?;
            if !detailed && params.content_selection.is_some() {
                return Err(CliError::Service(ServiceError::InvalidInput(
                    MCP_ERROR_CONTENT_SELECTION_RELATION_VIEW.to_string(),
                )));
            }
            if params.roots.is_some() && params.worktrees.is_some() {
                return Err(CliError::Service(ServiceError::InvalidInput(
                    MCP_ERROR_FEDERATED_SELECTOR_CONFLICT.to_string(),
                )));
            }
            let federated_worktrees = params.worktrees.as_deref();
            if let Some(worktrees) = federated_worktrees {
                validate_federated_root_count(worktrees.len()).map_err(CliError::Service)?;
                if params.project_path.is_some() {
                    return Err(CliError::Service(ServiceError::InvalidInput(
                        MCP_ERROR_FEDERATED_PROJECT_PATH_CONFLICT.to_string(),
                    )));
                }
                if params.worktree.as_deref().is_some_and(|primary| {
                    worktrees
                        .first()
                        .is_none_or(|first| primary.trim() != first.trim())
                }) {
                    return Err(CliError::Service(ServiceError::InvalidInput(
                        MCP_ERROR_FEDERATED_PRIMARY_CONFLICT.to_string(),
                    )));
                }
            }
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let selected_worktree = federated_worktrees
                .and_then(|worktrees| worktrees.first())
                .map(String::as_str)
                .or(params.worktree.as_deref());
            let (state, file, routed_project) = self.state_and_optional_file_key(
                params.project_path.as_deref(),
                selected_worktree,
                params.file.as_deref(),
                nearest_project,
            )?;
            if params.roots.is_some() || federated_worktrees.is_some() {
                if !detailed {
                    return Err(CliError::Service(ServiceError::InvalidInput(
                        MCP_ERROR_FEDERATED_RELATION_VIEW.to_string(),
                    )));
                }
                let file = file.as_deref().ok_or_else(|| {
                    CliError::Service(ServiceError::InvalidInput(
                        MCP_ERROR_DETAILED_RELATION_FILE.to_string(),
                    ))
                })?;
                let control =
                    index_work_control(&SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None))
                        .with_timeout_ceiling(Duration::from_millis(
                            params.deadline_ms.unwrap_or(10_000).clamp(1, 60_000),
                        ));
                let bridge = context
                    .map(|context| McpRequestCancellationBridge::start(&context, &control))
                    .transpose()?;
                let (roots, worktree_selections) = if let Some(worktrees) = federated_worktrees {
                    let (roots, selections) = self.federated_worktree_roots(worktrees)?;
                    (roots, Some(selections))
                } else {
                    (
                        params
                            .roots
                            .as_deref()
                            .unwrap_or_default()
                            .iter()
                            .map(PathBuf::from)
                            .collect::<Vec<_>>(),
                        None,
                    )
                };
                let worktree_labels = worktree_selections.as_ref().map(|selections| {
                    selections
                        .iter()
                        .map(|selection| selection.alias.clone())
                        .collect::<Vec<_>>()
                });
                let stores = open_federated_atlas_stores_for_project(
                    &state.db_path,
                    &state.root,
                    state.config_path.as_deref(),
                    &roots,
                    worktree_labels.as_deref(),
                    &control,
                )?;
                let stores = if let Some(selections) = worktree_selections.as_deref() {
                    Self::require_federated_worktree_identities(stores, selections)?
                } else {
                    stores
                };
                let result = Self::detailed_symbol_relations_response(
                    &state,
                    routed_project,
                    file,
                    params,
                    content_selection,
                    analysis,
                    SymbolRelationStores::Federated(stores),
                    &control,
                )
                .map(|(toon, _usage)| toon);
                if let Some(bridge) = bridge.as_ref() {
                    bridge.synchronize(&control);
                }
                drop(bridge);
                return result;
            }
            self.with_fresh_string_and_usage_controlled_for_request(
                &state,
                context,
                |store, _stamp, control| {
                    let file = file
                        .as_deref()
                        .map(|path| validated_indexed_file_key(store, Path::new(path)))
                        .transpose()?;
                    if detailed {
                        let file = file.as_deref().ok_or_else(|| {
                            CliError::Service(ServiceError::InvalidInput(
                                MCP_ERROR_DETAILED_RELATION_FILE.to_string(),
                            ))
                        })?;
                        return Self::detailed_symbol_relations_response(
                            &state,
                            routed_project,
                            file,
                            params,
                            content_selection,
                            analysis,
                            SymbolRelationStores::Single(store),
                            control,
                        );
                    }

                    let relations = store.load_symbol_relations(
                        file.as_deref(),
                        params.query.as_deref(),
                        params.limit.unwrap_or(50),
                    )?;
                    let toon = Self::with_selected_project_audit(
                        &state,
                        routed_project,
                        render_symbol_relations(&relations),
                    )?;
                    let usage = Self::telemetry_enabled()
                        .then(|| {
                            estimated_source_tokens_for_paths(
                                store,
                                relations.iter().map(|relation| relation.path.as_str()),
                            )
                        })
                        .and_then(Result::ok)
                        .map(|baseline_tokens| {
                            McpUsageIntent::estimate(
                                MCP_EVENT_ATLAS_SYMBOL_RELATIONS,
                                file.clone(),
                                params.query.clone(),
                                baseline_tokens,
                            )
                        });
                    Ok((toon, usage))
                },
            )
        })())
    }

    /// List indexed symbol relations.
    #[tool(
        name = "atlas_symbol_relations",
        description = "List imports, calls, dependencies, and containment edges as compact TOON."
    )]
    fn atlas_symbol_relations(
        &self,
        Parameters(params): Parameters<AtlasSymbolRelationsParams>,
        context: RequestContext<RoleServer>,
    ) -> McpToolTextResult {
        self.atlas_symbol_relations_response(&params, Some(context))
    }

    /// Return structural health findings.
    #[tool(
        name = "atlas_health",
        description = "Return a bounded ProjectAtlas structural health page with optional category, severity, and path-prefix filters."
    )]
    fn atlas_health(
        &self,
        Parameters(params): Parameters<AtlasHealthParams>,
        context: RequestContext<RoleServer>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state =
                self.state_for_target(params.project_path.clone(), params.worktree.clone())?;
            if params.coverage.unwrap_or(false) {
                let query = coverage_query_from_params(&params)?;
                return self.with_fresh_string_and_usage_controlled_for_request(
                    &state,
                    Some(context),
                    |store, stamp, control| {
                        let mut report =
                            load_coverage_discovery_controlled(store, query.clone(), control)?;
                        let toon = finalize_coverage_output(OutputFormat::Toon, &mut report)?;
                        let usage = Self::telemetry_enabled()
                            .then(|| {
                                self.estimated_source_tokens_cached(
                                    &state, store, &stamp, None, None,
                                )
                            })
                            .and_then(Result::ok)
                            .map(|baseline_tokens| {
                                McpUsageIntent::directory_walk(
                                    MCP_EVENT_ATLAS_HEALTH,
                                    None,
                                    None,
                                    baseline_tokens,
                                )
                            });
                        Ok((toon, usage))
                    },
                );
            }
            if has_coverage_filters(&params) {
                return Err(CliError::InvalidInput(
                    MCP_ERROR_COVERAGE_FILTERS_REQUIRE_COVERAGE.to_string(),
                ));
            }
            let scope = if params.source_only.unwrap_or(false) {
                HealthScope::source_only()
            } else {
                HealthScope::all()
            };
            let query = health_query_from_params(&params, scope)?;
            self.with_fresh_string_and_usage_for_request(&state, Some(context), |store, stamp| {
                let page = store.unresolved_health_findings_page_current(&query)?;
                let toon = render_health_page(&page, &query);
                let usage = Self::telemetry_enabled()
                    .then(|| self.estimated_source_tokens_cached(&state, store, &stamp, None, None))
                    .and_then(Result::ok)
                    .map(|baseline_tokens| {
                        McpUsageIntent::directory_walk(
                            MCP_EVENT_ATLAS_HEALTH,
                            None,
                            None,
                            baseline_tokens,
                        )
                    });
                Ok((toon, usage))
            })
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
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state = self.state_for_target(params.project_path, params.worktree)?;
            let store = Self::open_existing_mut_store(&state, &self.control_state)?;
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

    /// Run `ProjectAtlas` lint checks without terminating the MCP transport.
    #[tool(
        name = "atlas_lint",
        description = "Run ProjectAtlas lint checks and return an ok flag, CLI-compatible exit code, and report text."
    )]
    fn atlas_lint(&self, Parameters(params): Parameters<AtlasLintParams>) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state =
                self.admin_project_root(params.project_path.clone(), params.worktree.clone())?;
            let report = Self::lint_report_for_state(&state, &params)?;
            Self::encode_named_payload(MCP_PAYLOAD_LINT, &report)
        })())
    }

    /// Return token savings telemetry.
    #[tool(
        name = "atlas_token_report",
        description = "Return ProjectAtlas token-savings telemetry for the whole index or one session."
    )]
    fn atlas_token_report(
        &self,
        Parameters(params): Parameters<AtlasTokenParams>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state =
                self.state_for_target(params.project_path.clone(), params.worktree.clone())?;
            let repository_scope = params.session.is_none()
                && state.root == self.control_state.root
                && state.db_path == self.control_state.db_path;
            let load_report = |request: TokenReportRequest<'_>| {
                if repository_scope {
                    load_synchronized_repository_token_report(
                        &state.db_path,
                        &state.root,
                        state
                            .worktree
                            .as_ref()
                            .and_then(|selection| selection.control_project_instance_id),
                        request,
                    )
                } else {
                    let store = Self::open_read_store(&state)?;
                    load_token_report(&store, request).map_err(CliError::from)
                }
            };
            let include_chart = params.include_chart.unwrap_or(false);
            let chart_theme = Self::parse_token_chart_theme(params.theme.as_deref())?;
            if let Some(window) = params.trend_window.as_deref() {
                if params.benchmark_results.is_some() {
                    return Err(CliError::InvalidInput(
                        TOKEN_TREND_BENCHMARK_ERROR.to_string(),
                    ));
                }
                let window = TokenTrendWindow::parse(window).ok_or_else(|| {
                    CliError::InvalidInput(format!(
                        "unsupported token trend window {window:?}; {TOKEN_TREND_WINDOW_ERROR_SUFFIX}"
                    ))
                })?;
                let request = if repository_scope {
                    TokenReportRequest::RepositoryTrends { window }
                } else {
                    TokenReportRequest::Trends {
                        caller_label: params.session.as_deref(),
                        window,
                    }
                };
                let report = match load_report(request)? {
                    TokenReport::Trends(report) => report,
                    TokenReport::Overview(_) => {
                        return Err(CliError::InvalidInput(
                            TOKEN_TRENDS_RESULT_VARIANT_MISMATCH.to_string(),
                        ));
                    }
                };
                if include_chart {
                    let chart = render_token_trend_dashboard_plain_with_theme(&report, chart_theme);
                    let output = Self::encode_two_named_payloads(
                        MCP_PAYLOAD_TOKEN_TRENDS,
                        &report,
                        MCP_PAYLOAD_CHART,
                        &chart,
                    )?;
                    return Self::with_selected_project_audit(
                        &state,
                        state.worktree.is_some(),
                        output,
                    );
                }
                return Self::with_selected_project_audit(
                    &state,
                    state.worktree.is_some(),
                    render_token_trends(&report),
                );
            }
            let request = if repository_scope {
                TokenReportRequest::RepositoryOverview {
                    benchmark_results: params.benchmark_results.as_deref().map(Path::new),
                }
            } else {
                TokenReportRequest::Overview {
                    caller_label: params.session.as_deref(),
                    benchmark_results: params.benchmark_results.as_deref().map(Path::new),
                }
            };
            let overview = match load_report(request)? {
                TokenReport::Overview(overview) => overview,
                TokenReport::Trends(_) => {
                    return Err(CliError::InvalidInput(
                        TOKEN_OVERVIEW_RESULT_VARIANT_MISMATCH.to_string(),
                    ));
                }
            };
            if include_chart {
                let chart = render_token_dashboard_plain_with_theme(
                    &overview,
                    params.session.as_deref(),
                    chart_theme,
                );
                let output = Self::encode_two_named_payloads(
                    MCP_PAYLOAD_TOKEN_SAVINGS,
                    &overview,
                    MCP_PAYLOAD_CHART,
                    &chart,
                )?;
                return Self::with_selected_project_audit(&state, state.worktree.is_some(), output);
            }
            Self::with_selected_project_audit(
                &state,
                state.worktree.is_some(),
                render_token_overview(&overview),
            )
        })())
    }

    /// Return repository-intelligence parity readiness.
    #[tool(
        name = "atlas_parity_report",
        description = "Return a ProjectAtlas repository-intelligence parity gate report for release and agent-runtime readiness."
    )]
    fn atlas_parity_report(
        &self,
        Parameters(params): Parameters<AtlasParityParams>,
        context: RequestContext<RoleServer>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state = self.state_for_target(params.project_path, params.worktree)?;
            let profile = params
                .profile
                .unwrap_or_else(|| crate::REPOSITORY_INTELLIGENCE_PROFILE.to_string());
            self.with_fresh_string_for_request(&state, Some(context), |store, _stamp| {
                Ok(render_parity_report(&build_parity_report(store, &profile)?))
            })
        })())
    }

    /// Return local settings and cache/index locations.
    #[tool(
        name = "atlas_settings",
        description = "Return ProjectAtlas local settings, config, and durable index paths."
    )]
    fn atlas_settings(
        &self,
        Parameters(params): Parameters<AtlasProjectParams>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state = self.state_for_target(params.project_path, params.worktree)?;
            self.render_settings_with_capabilities(&state)
        })())
    }

    /// Return watcher availability and operating mode.
    #[tool(
        name = "atlas_watch_status",
        description = "Return ProjectAtlas watcher availability and current operating mode."
    )]
    fn atlas_watch_status(
        &self,
        Parameters(params): Parameters<AtlasProjectParams>,
    ) -> McpToolTextResult {
        let state = match self.state_for_target(params.project_path, params.worktree) {
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
    fn atlas_watch_once(
        &self,
        Parameters(params): Parameters<AtlasWatchOnceParams>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let background = params.background.unwrap_or(false);
            let (state, path) = if background {
                self.background_state_and_root_path(
                    params.project_path,
                    params.worktree,
                    params.path,
                    nearest_project,
                )?
            } else {
                self.state_and_root_path(
                    params.project_path,
                    params.worktree,
                    params.path,
                    nearest_project,
                )?
            };
            let symbol_options = SymbolBuildOptions::new(
                MAX_SYMBOL_FILE_BYTES,
                params.max_workers,
                params.timeout_seconds,
            );
            let text_index_max_bytes = params.text_index_max_bytes;
            if background {
                let control_state = self.control_state.clone();
                let task = self.start_index_task(
                    McpTaskOperation::WatchOnce,
                    symbol_options,
                    MCP_TOOL_ATLAS_OVERVIEW,
                    move |control, symbol_options| {
                        let plan = ScanRuntimePlan::for_path_controlled(
                            state.config_path.as_deref(),
                            &path,
                            text_index_max_bytes,
                            control,
                        )?;
                        let mut store = Self::open_mut_store(&state, &control_state)?;
                        run_single_watch_refresh_controlled(
                            &mut store,
                            &plan,
                            &symbol_options,
                            control,
                        )?;
                        Ok(())
                    },
                )?;
                return Self::encode_named_payload(MCP_PAYLOAD_TASK_START, &task);
            }
            let control = index_work_control(&symbol_options);
            let plan = ScanRuntimePlan::for_path_controlled(
                state.config_path.as_deref(),
                &path,
                text_index_max_bytes,
                &control,
            )?;
            let mut store = Self::open_mut_store(&state, &self.control_state)?;
            let report =
                run_single_watch_refresh_controlled(&mut store, &plan, &symbol_options, &control)?;
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
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let (state, path) = self.state_and_root_path(
                params.project_path,
                params.worktree,
                params.path,
                nearest_project,
            )?;
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
    fn atlas_reset_index(
        &self,
        Parameters(params): Parameters<AtlasResetIndexParams>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state = self.state_for_target(params.project_path, params.worktree)?;
            let apply = params.apply.unwrap_or(false);
            let dry_run = params.dry_run.unwrap_or(false);
            let include_mcp_config = params.include_mcp_config.unwrap_or(false);
            let report = if apply && !dry_run {
                if let Some(selection) = state
                    .worktree
                    .as_ref()
                    .filter(|selection| selection.registration_id.is_some())
                {
                    self.reset_registered_worktree_index(&state, selection, include_mcp_config)?
                } else {
                    reset_index_files(&state.db_path, true, false, include_mcp_config)?
                }
            } else {
                reset_index_files(&state.db_path, apply, dry_run, include_mcp_config)?
            };
            Self::encode_named_payload(MCP_PAYLOAD_RESET_INDEX, &report)
        })())
    }

    /// Generate a project-local MCP config document.
    #[tool(
        name = "atlas_mcp_config",
        description = "Return a generated ProjectAtlas MCP config document for mcp-json, codex, claude-code, or opencode hosts."
    )]
    fn atlas_mcp_config(
        &self,
        Parameters(params): Parameters<AtlasMcpConfigParams>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state = self.admin_project_root(params.project_path, params.worktree)?;
            let harness = Self::parse_harness_config(params.harness.as_deref())?;
            let server_name = params
                .server_name
                .unwrap_or_else(|| MCP_DEFAULT_CONFIG_SERVER_NAME.to_string());
            let report = build_harness_mcp_config_report(
                harness,
                &server_name,
                &state.db_path,
                state.config_path.as_deref(),
                params.nearest_project.unwrap_or(false),
            )?;
            Self::encode_named_payload(MCP_PAYLOAD_MCP_CONFIG, &report)
        })())
    }

    /// Return runtime identity and compiled MCP tool surface.
    #[tool(
        name = "atlas_runtime_info",
        description = "Return ProjectAtlas runtime identity, version, capabilities, and compiled MCP tool names."
    )]
    fn atlas_runtime_info(
        &self,
        Parameters(_params): Parameters<AtlasProjectParams>,
    ) -> McpToolTextResult {
        let _session_scope = self.session.as_str();
        Self::as_mcp_text(Ok(render_runtime_info(&build_runtime_info())))
    }

    /// Return a compact startup brief for agents.
    #[tool(
        name = "atlas_session_brief",
        description = "Return selected project identity, index state, ranked candidates, blockers, and typed next-call recommendations for agent startup."
    )]
    fn atlas_session_brief(
        &self,
        Parameters(params): Parameters<AtlasSessionBriefParams>,
        context: RequestContext<RoleServer>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            if params.compact.unwrap_or(false) {
                let brief = self.build_compact_session_brief(params, Some(context))?;
                Self::encode_named_payload(MCP_PAYLOAD_SESSION_BRIEF, &brief)
            } else {
                let brief = self.build_session_brief(params, Some(context))?;
                Self::encode_named_payload(MCP_PAYLOAD_SESSION_BRIEF, &brief)
            }
        })())
    }

    /// Return typed status for one MCP task-progress record.
    #[tool(
        name = "atlas_task_status",
        description = "Return typed status for a bounded MCP task-progress record."
    )]
    fn atlas_task_status(
        &self,
        Parameters(params): Parameters<AtlasTaskParams>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let status = self.task_status(params.task_id)?;
            Self::encode_named_payload(MCP_PAYLOAD_TASK_STATUS, &status)
        })())
    }

    /// Request cancellation for one MCP task-progress record.
    #[tool(
        name = "atlas_task_cancel",
        description = "Request cancellation for a bounded MCP task-progress record."
    )]
    fn atlas_task_cancel(
        &self,
        Parameters(params): Parameters<AtlasTaskParams>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let cancel = self.task_cancel(params.task_id)?;
            Self::encode_named_payload(MCP_PAYLOAD_TASK_CANCEL, &cancel)
        })())
    }

    /// Return a bounded purpose curation queue.
    #[tool(
        name = "atlas_purpose_queue",
        description = "Return a bounded folder-first queue of ProjectAtlas paths that need agent purpose curation."
    )]
    fn atlas_purpose_queue(
        &self,
        Parameters(params): Parameters<AtlasPurposeQueueParams>,
        context: RequestContext<RoleServer>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state = self.state_for_target(
                params.health.project_path.clone(),
                params.health.worktree.clone(),
            )?;
            let query =
                health_query_from_params(&params.health, purpose_queue_scope(&params.health))?;
            let task = params
                .task
                .as_deref()
                .unwrap_or(MCP_PURPOSE_TASK_QUEUE)
                .to_string();
            self.with_fresh_string_and_usage_for_request(&state, Some(context), |store, stamp| {
                let page = purpose_curation_page(store, &query, &task)?;
                let toon = render_purpose_curation_page(&page);
                let usage = Self::telemetry_enabled()
                    .then(|| self.estimated_source_tokens_cached(&state, store, &stamp, None, None))
                    .and_then(Result::ok)
                    .map(|baseline_tokens| {
                        McpUsageIntent::directory_walk(
                            MCP_EVENT_ATLAS_PURPOSE_QUEUE,
                            None,
                            None,
                            baseline_tokens,
                        )
                    });
                Ok((toon, usage))
            })
        })())
    }

    /// Set an agent-approved purpose in the durable index.
    #[tool(
        name = "atlas_purpose_set",
        description = "Set agent-approved ProjectAtlas purpose metadata for one indexed path."
    )]
    fn atlas_purpose_set(
        &self,
        Parameters(params): Parameters<AtlasPurposeSetParams>,
        context: RequestContext<RoleServer>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let state = self.state_for_target(params.project_path, params.worktree)?;
            self.with_admitted_purpose_mutation(&state, Some(context), |store| {
                let node_key = Self::validated_indexed_node_key(store, &params.path)?;
                store.set_purpose(&node_key, &params.purpose, PurposeSource::Agent)?;
                let classification = if store
                    .load_node_by_path(&node_key)?
                    .is_some_and(|node| node.node.kind == projectatlas_core::NodeKind::File)
                {
                    store
                        .file_content_classifications_for_paths(std::slice::from_ref(&node_key))?
                        .first()
                        .map(|row| row.classification)
                } else {
                    None
                };
                Self::encode_serialized_payload(McpPurposeSetResponse {
                    purpose_set: McpPurposeSetPayload {
                        path: node_key,
                        classification,
                        status: PurposeStatus::Approved,
                        source: PurposeSource::Agent,
                        agent_reviewed: true,
                    },
                })
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
        context: RequestContext<RoleServer>,
    ) -> McpToolTextResult {
        Self::as_mcp_text((|| {
            let apply = params.apply.unwrap_or(false);
            let requests = params
                .items
                .into_iter()
                .map(|item| PurposeReviewRequest {
                    path: item.path,
                    purpose: item.purpose,
                    confirm_existing: item.confirm_existing.unwrap_or(false),
                    task: item.task,
                    work_key: item.work_key,
                    state_token: item.state_token,
                })
                .collect::<Vec<_>>();
            validate_purpose_review_admission(&requests)?;
            let state = self.state_for_target(params.project_path, params.worktree)?;
            if apply {
                return self.with_admitted_purpose_mutation(&state, Some(context), |store| {
                    let report = review_purposes(store, &requests, true)?;
                    Ok(render_purpose_review_report(&report))
                });
            }
            self.with_fresh_string_for_request(&state, Some(context), |store, _stamp| {
                let report = review_purposes(store, &requests, false)?;
                Ok(render_purpose_review_report(&report))
            })
        })())
    }
}

#[allow(clippy::unused_async_trait_impl)]
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

/// Return current Unix time in milliseconds for MCP task status records.
fn mcp_unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

/// Recognize cooperative cancellation through each typed adapter error layer.
fn task_error_is_canceled(error: &CliError) -> bool {
    std::iter::successors(
        Some(error as &(dyn std::error::Error + 'static)),
        |source| source.source(),
    )
    .any(|source| {
        matches!(
            source.downcast_ref::<IndexWorkFailure>(),
            Some(IndexWorkFailure::Cancelled { .. })
        )
    })
}

/// Retain one concise task failure without letting diagnostics grow unbounded.
fn bounded_task_error(error: &CliError) -> String {
    error
        .to_string()
        .chars()
        .take(MCP_TASK_ERROR_MAX_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas_map::init_project_with_config;
    use notify::{Event, EventKind, event::ModifyKind};
    use projectatlas_core::graph::{
        Completeness, ConfidenceClass, EntitySelector, ExtendedRelationKind, ExternalSelector,
        GraphEntity, GraphIdentityText, GraphRelationKind, LogicalRelation, PackageSelector,
        RelationResolution, RepositoryFilePath,
    };
    use projectatlas_core::symbols::RelationKind;
    use projectatlas_core::{
        CanonicalProjectRoot, IndexCancellation, IndexWorkStage, RankedConnectionDirection,
        RankedConnectionKind, RankedConnectionTarget,
    };
    use projectatlas_db::ProjectRootTransition;
    use std::collections::BTreeSet;
    use std::fs;
    use std::io;
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt;
    use std::process::Command as StdCommand;
    use std::time::{Duration, Instant};

    fn require(condition: bool, message: &str) -> Result<(), Box<dyn std::error::Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message.to_string()).into())
        }
    }

    /// Execute one test-owned Git fixture command and retain exact diagnostics on failure.
    fn run_fixture_command(command: &mut StdCommand) -> Result<String, Box<dyn std::error::Error>> {
        let output = command.output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "fixture command failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        Ok(String::from_utf8(output.stdout)?)
    }

    struct RegisteredWorktreeRaceFixture {
        _temp: tempfile::TempDir,
        primary: PathBuf,
        control_root: PathBuf,
        linked: PathBuf,
        control_db: PathBuf,
        target_db: PathBuf,
        server: ProjectAtlasMcpServer,
        alias: WorktreeAlias,
        registration: WorktreeRegistration,
        selection: McpWorktreeSelection,
        state: McpProjectState,
        administrative_directory: PathBuf,
        administrative_identity: String,
    }

    fn registered_worktree_race_fixture(
        alias: &str,
    ) -> Result<RegisteredWorktreeRaceFixture, Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let primary = temp.path().join("control");
        let linked = temp.path().join("linked");
        fs::create_dir_all(primary.join("src"))?;
        run_fixture_command(StdCommand::new("git").current_dir(&primary).arg("init"))?;
        for (key, value) in [
            ("user.name", "ProjectAtlas Test"),
            ("user.email", "projectatlas@example.invalid"),
            ("commit.gpgsign", "false"),
            ("core.autocrlf", "false"),
        ] {
            run_fixture_command(
                StdCommand::new("git")
                    .current_dir(&primary)
                    .args(["config", key, value]),
            )?;
        }
        fs::write(primary.join("src/lib.rs"), "pub fn control() {}\n")?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["add", "."]),
        )?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["commit", "-m", "fixture"]),
        )?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["worktree", "add", "-b", "captured"])
                .arg(&linked),
        )?;

        let control_root = primary.canonicalize()?;
        let control_config = control_root.join(PROJECTATLAS_DIR_NAME).join("config.toml");
        let control_db = control_root
            .join(PROJECTATLAS_DIR_NAME)
            .join(PROJECTATLAS_DB_FILE_NAME);
        init_project_with_config(&control_root, Some(&control_config))?;
        let mut control = AtlasStore::open_for_project(&control_db, &control_root)?;
        let plan = ScanRuntimePlan::for_path(Some(&control_config), &control_root, None)?;
        run_scan_pipeline(
            &mut control,
            &plan,
            &SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None),
        )?;
        let control_project_instance_id = control
            .project_instance_id()?
            .ok_or(DbError::ProjectInstanceIdentityMissing)?;
        let server = ProjectAtlasMcpServer::new(
            control_db.clone(),
            Some(control_config),
            "worktree-race".to_string(),
            false,
        );
        let repository = server.control_git_repository()?;
        let canonical_linked = linked.canonicalize()?;
        let entry = repository
            .worktrees
            .iter()
            .find(|entry| {
                ProjectAtlasMcpServer::active_worktree_root(entry)
                    == Some(canonical_linked.as_path())
            })
            .ok_or_else(|| io::Error::other("linked worktree was not discovered"))?;
        let administrative_directory = entry.administrative_directory.clone();
        let administrative_identity = git_administrative_identity(&administrative_directory)?;
        let alias = WorktreeAlias::parse(alias)?;
        let registration = control.register_worktree(
            &alias,
            &repository.common_directory,
            &administrative_directory,
            &administrative_identity,
            &canonical_linked,
            None,
            1,
        )?;
        drop(control);
        let target_db = canonical_linked
            .join(PROJECTATLAS_DIR_NAME)
            .join(PROJECTATLAS_DB_FILE_NAME);
        let selection = McpWorktreeSelection {
            alias: alias.to_string(),
            registration_id: Some(registration.registration_id),
            project_instance_id: None,
            control_project_instance_id: Some(control_project_instance_id),
        };
        let state = McpProjectState {
            root: canonical_linked,
            db_path: target_db.clone(),
            config_path: Some(linked.join(PROJECTATLAS_DIR_NAME).join("config.toml")),
            worktree: Some(selection.clone()),
        };
        Ok(RegisteredWorktreeRaceFixture {
            _temp: temp,
            primary: control_root.clone(),
            control_root,
            linked,
            control_db,
            target_db,
            server,
            alias,
            registration,
            selection,
            state,
            administrative_directory,
            administrative_identity,
        })
    }

    fn replace_registered_worktree(
        fixture: &RegisteredWorktreeRaceFixture,
        branch: &str,
    ) -> Result<GitWorktreeEntry, Box<dyn std::error::Error>> {
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&fixture.primary)
                .args(["worktree", "remove", "--force"])
                .arg(&fixture.linked),
        )?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&fixture.primary)
                .args(["worktree", "add", "-b", branch])
                .arg(&fixture.linked),
        )?;
        let repository = fixture.server.control_git_repository()?;
        repository
            .worktrees
            .into_iter()
            .find(|entry| {
                ProjectAtlasMcpServer::active_worktree_root(entry)
                    == Some(fixture.state.root.as_path())
            })
            .ok_or_else(|| io::Error::other("replacement worktree was not discovered").into())
    }

    /// Prepare one fully verified hydration candidate without publishing it.
    fn prepared_hydration_candidate(
        fixture: &RegisteredWorktreeRaceFixture,
    ) -> Result<
        (
            PreparedWorktreeHydrationCandidate,
            PathBuf,
            IndexWorkControl,
        ),
        Box<dyn std::error::Error>,
    > {
        fs::create_dir_all(
            fixture
                .target_db
                .parent()
                .ok_or_else(|| io::Error::other("target database has no parent"))?,
        )?;
        let source =
            open_atlas_store_read_only_for_project(&fixture.control_db, &fixture.control_root)?;
        let work_control =
            index_work_control(&SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None));
        let mut candidate = source.prepare_worktree_hydration(
            &fixture.state.root,
            &fixture.target_db,
            &work_control,
        )?;
        let candidate_path = candidate.path()?.to_path_buf();
        candidate.accept_verified_source_state(&work_control)?;
        let candidate = candidate.prepare_activation(&work_control)?;
        Ok((candidate, candidate_path, work_control))
    }

    #[test]
    fn task_errors_classify_only_typed_cancellation_as_canceled() {
        let stage = IndexWorkStage::RepositoryTraversal;
        assert!(task_error_is_canceled(&CliError::IndexWork(
            IndexWorkFailure::Cancelled { stage },
        )));
        assert!(task_error_is_canceled(&CliError::Fs(
            projectatlas_fs::FsError::IndexWork(IndexWorkFailure::Cancelled { stage }),
        )));
        assert!(task_error_is_canceled(&CliError::Db(DbError::IndexWork(
            IndexWorkFailure::Cancelled { stage },
        ))));
        assert!(task_error_is_canceled(&CliError::Service(
            ServiceError::Db(DbError::IndexWork(IndexWorkFailure::Cancelled { stage })),
        )));
        assert!(!task_error_is_canceled(&CliError::IndexWork(
            IndexWorkFailure::DeadlineExceeded { stage },
        )));
        assert!(!task_error_is_canceled(&CliError::Db(DbError::IndexWork(
            IndexWorkFailure::DeadlineExceeded { stage },
        ))));
        assert!(!task_error_is_canceled(&CliError::Fs(
            projectatlas_fs::FsError::IndexWork(IndexWorkFailure::ResourceLimitExceeded {
                stage,
                resource: projectatlas_core::IndexWorkResource::Entries,
                limit: 1,
                observed: 2,
            }),
        )));
    }

    fn usage_test_project(
        parent: &Path,
        name: &str,
    ) -> Result<(McpProjectState, AtlasStore), Box<dyn std::error::Error>> {
        let root = parent.join(name);
        fs::create_dir_all(root.join(".projectatlas"))?;
        let db_path = root.join(".projectatlas").join("projectatlas.db");
        let store = AtlasStore::open_for_project(&db_path, &root)?;
        Ok((
            McpProjectState {
                root,
                db_path,
                config_path: None,
                worktree: None,
            },
            store,
        ))
    }

    fn usage_runtime_identity(
        server: &ProjectAtlasMcpServer,
        state: &McpProjectState,
        store: &AtlasStore,
    ) -> Result<UsageRuntimeInstance, Box<dyn std::error::Error>> {
        let binding = McpUsageProjectBinding::capture(state, store)?;
        let project_instance = server
            .usage_runtime
            .lock()
            .map_err(|_poisoned| io::Error::other("usage runtime lock poisoned"))?
            .entries
            .iter()
            .find(|entry| entry.binding == binding)
            .map(|entry| Arc::clone(&entry.instance))
            .ok_or_else(|| io::Error::other("operating-system entropy was unavailable"))?;
        let identity = *project_instance
            .lock()
            .map_err(|_poisoned| io::Error::other("project usage lock poisoned"))?;
        Ok(identity)
    }

    #[test]
    fn mcp_server_clones_share_one_runtime_identity() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let (state, store) = usage_test_project(temp.path(), "selected-project")?;
        let first = ProjectAtlasMcpServer::new(
            state.db_path.clone(),
            None,
            "shared-label".to_string(),
            false,
        );
        first.record_usage_for_state(&state, &store, |_usage_instance| Ok(()));
        let first_identity = usage_runtime_identity(&first, &state, &store)?;
        let cloned = first.clone();
        require(
            usage_runtime_identity(&first, &state, &store)? == first_identity,
            "cloning an MCP server changed the original telemetry identity",
        )?;
        let restarted = ProjectAtlasMcpServer::new(
            state.db_path.clone(),
            None,
            "shared-label".to_string(),
            false,
        );
        restarted.record_usage_for_state(&state, &store, |_usage_instance| Ok(()));
        let restarted_identity = usage_runtime_identity(&restarted, &state, &store)?;

        require(
            usage_runtime_identity(&cloned, &state, &store)? == first_identity,
            "cloning an MCP server changed its process-scoped telemetry identity",
        )?;
        require(
            restarted_identity != first_identity,
            "a separately constructed MCP server reused the prior runtime identity",
        )
    }

    #[test]
    fn mcp_request_cancellation_bridge_reaches_index_work() -> Result<(), Box<dyn std::error::Error>>
    {
        let cancellation = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = Arc::clone(&cancellation);
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let bridge = McpRequestCancellationBridge::start_with_probe(
            move || observed.load(Ordering::Acquire),
            &control,
        )?;

        cancellation.store(true, Ordering::Release);
        let mut canceled = false;
        for _attempt in 0..100 {
            if matches!(
                control.check(IndexWorkStage::Publication),
                Err(IndexWorkFailure::Cancelled { .. })
            ) {
                canceled = true;
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        drop(bridge);

        require(
            canceled,
            "RMCP cancellation probe did not reach the shared index work control",
        )
    }

    #[test]
    fn purpose_mutation_synchronously_rolls_back_request_cancellation()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("purpose-cancellation");
        fs::create_dir_all(root.join(".projectatlas"))?;
        fs::write(root.join("source.rs"), "fn current() {}\n")?;
        let db_path = root.join(".projectatlas/projectatlas.db");
        let plan = ScanRuntimePlan::for_path(None, &root, None)?;
        let mut store = open_atlas_store_for_project(&db_path, &plan.root)?;
        run_scan_pipeline(
            &mut store,
            &plan,
            &SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None),
        )?;
        let before_revision = store.authored_purpose_revision()?;
        let before_purpose = store
            .load_node_by_path("source.rs")?
            .ok_or_else(|| io::Error::other("indexed cancellation source missing"))?
            .purpose;
        drop(store);

        let server = ProjectAtlasMcpServer::new(
            db_path.clone(),
            None,
            "purpose-cancellation".to_string(),
            false,
        );
        let state = server.state_for_target(Some(normalize_native_path_display(&root)), None)?;
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancellation_probe = Arc::clone(&cancelled);
        let bridge = McpRequestCancellationBridge::start_with_probe(
            move || cancellation_probe.load(Ordering::Acquire),
            &control,
        )?;
        let result = server.with_admitted_purpose_mutation_controlled(
            &state,
            &control,
            Some(&bridge),
            |store| {
                store.set_purpose("source.rs", "Canceled purpose", PurposeSource::Agent)?;
                cancelled.store(true, Ordering::Release);
                Ok(())
            },
        );
        drop(bridge);

        require(
            matches!(result, Err(CliError::IndexWork(_))),
            "request cancellation did not reject the purpose transaction",
        )?;
        let store = open_atlas_store_for_project(&db_path, &state.root)?;
        require(
            store.authored_purpose_revision()? == before_revision,
            "request cancellation advanced the authored-purpose revision",
        )?;
        require(
            store
                .load_node_by_path("source.rs")?
                .is_some_and(|node| node.purpose == before_purpose),
            "request cancellation persisted the rejected purpose",
        )
    }

    #[test]
    fn mcp_same_path_project_identity_rotation_starts_a_distinct_runtime_entry()
    -> Result<(), Box<dyn std::error::Error>> {
        if telemetry_disabled() {
            return Ok(());
        }
        let temp = tempfile::tempdir()?;
        let (state, store) = usage_test_project(temp.path(), "selected-project")?;
        let server = ProjectAtlasMcpServer::new(
            state.db_path.clone(),
            None,
            "shared-label".to_string(),
            false,
        );
        let old_project_identity = store.captured_project_binding()?.project_instance_id;
        server.record_usage_for_state(&state, &store, |_usage_instance| Ok(()));
        let old_runtime_identity = usage_runtime_identity(&server, &state, &store)?;
        drop(store);

        AtlasStore::transition_project_root(
            &state.db_path,
            &state.root,
            projectatlas_db::ProjectRootTransition::Detach,
        )?;
        let detached_store = AtlasStore::open_for_project(&state.db_path, &state.root)?;
        let detached_project_identity = detached_store
            .captured_project_binding()?
            .project_instance_id;
        server.record_usage_for_state(&state, &detached_store, |_usage_instance| Ok(()));
        let detached_runtime_identity = usage_runtime_identity(&server, &state, &detached_store)?;
        let tracked = server
            .usage_runtime
            .lock()
            .map_err(|_poisoned| io::Error::other("usage runtime lock poisoned"))?
            .entries
            .len();

        require(
            detached_project_identity != old_project_identity,
            "detach did not rotate the captured project identity",
        )?;
        require(
            detached_runtime_identity != old_runtime_identity,
            "same-path detach reused the previous project's telemetry identity",
        )?;
        require(
            tracked == 2,
            "same-path project identities did not retain distinct bounded runtime entries",
        )
    }

    #[test]
    fn mcp_telemetry_rotates_inactive_bindings_without_dropping_later_worktrees()
    -> Result<(), Box<dyn std::error::Error>> {
        if telemetry_disabled() {
            return Ok(());
        }
        let temp = tempfile::tempdir()?;
        let (state, control) = usage_test_project(temp.path(), "control")?;
        let server =
            ProjectAtlasMcpServer::new(state.db_path, None, "worktree-capacity".to_string(), false);
        let common = temp.path().join("common.git");
        let event = usage_from_estimates_with_context(
            "worktree-capacity",
            "atlas_overview",
            None,
            None,
            100,
            10,
            TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
            TOKEN_BASELINE_SELECTED_CANDIDATES,
            TOKEN_CONFIDENCE_INFERRED,
        );
        for index in 0..=MCP_TELEMETRY_PROJECT_BINDING_LIMIT {
            let registration = control.register_worktree(
                &WorktreeAlias::parse(&format!("worktree-{index:03}"))?,
                &common,
                &common.join(format!("worktrees/{index:03}")),
                &format!("{:064x}", index + 1),
                &temp.path().join(format!("worktree-{index:03}")),
                Some(ProjectInstanceId::from_bytes(
                    [u8::try_from(index + 1)?; 16],
                )?),
                u64::try_from(index + 1)?,
            )?;
            server.record_usage_for_origin(
                &server.control_state,
                &control,
                Some(registration.registration_id),
                |usage_instance| {
                    usage_instance.record_for_worktree(
                        &control,
                        registration.registration_id,
                        &event,
                    )
                },
            );
        }

        require(
            control.repository_token_overview()?.calls == MCP_TELEMETRY_PROJECT_BINDING_LIMIT + 1,
            "a worktree beyond the in-memory telemetry bound lost its accepted event",
        )?;
        let runtime = server
            .usage_runtime
            .lock()
            .map_err(|_poisoned| io::Error::other("usage runtime lock poisoned"))?;
        require(
            runtime.entries.len() == MCP_TELEMETRY_PROJECT_BINDING_LIMIT,
            "telemetry project binding registry exceeded its hard bound",
        )?;
        drop(runtime);
        require(
            control.telemetry_retention_state()?.active_instance_rows
                == MCP_TELEMETRY_PROJECT_BINDING_LIMIT,
            "inactive binding rotation did not seal before replacement",
        )?;
        server.seal_usage_instances_for_projects();
        require(
            control.telemetry_retention_state()?.active_instance_rows == 0,
            "rotated telemetry bindings were not sealed at MCP shutdown",
        )
    }

    #[test]
    fn routed_worktree_telemetry_preserves_session_baseline_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        if telemetry_disabled() {
            return Ok(());
        }
        let temp = tempfile::tempdir()?;
        let (state, control) = usage_test_project(temp.path(), "control")?;
        let server =
            ProjectAtlasMcpServer::new(state.db_path, None, "worktree-scale".to_string(), false);
        let common = temp.path().join("common.git");
        let event = usage_from_estimates_with_context(
            "worktree-scale",
            "atlas_overview",
            None,
            None,
            100,
            10,
            TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
            TOKEN_BASELINE_SELECTED_CANDIDATES,
            TOKEN_CONFIDENCE_INFERRED,
        );
        let registration = control.register_worktree(
            &WorktreeAlias::parse("worktree-001")?,
            &common,
            &common.join("worktrees/001"),
            &format!("{:064x}", 1),
            &temp.path().join("worktree-001"),
            Some(ProjectInstanceId::from_bytes([1; 16])?),
            1,
        )?;
        for _ in 0..2 {
            server.record_usage_for_origin(
                &server.control_state,
                &control,
                Some(registration.registration_id),
                |usage_instance| {
                    usage_instance.record_for_worktree(
                        &control,
                        registration.registration_id,
                        &event,
                    )
                },
            );
        }

        let overview = control.repository_token_overview()?;

        require(
            overview.calls == 2,
            "alias-routed modeled usage did not retain both accepted calls",
        )?;
        require(
            overview.deduped_modeled_tokens_avoided == 80,
            "alias-routed modeled usage did not reuse the session baseline witness",
        )?;
        require(
            overview.repeated_baselines_deduped == 1,
            "alias-routed modeled usage did not classify the repeated baseline",
        )?;
        require(
            control.telemetry_retention_state()?.active_instance_rows == 1,
            "alias-routed usage did not retain one bounded session identity",
        )?;
        server.seal_usage_instances_for_projects();
        require(
            control.telemetry_retention_state()?.active_instance_rows == 0,
            "alias-routed session identity was not sealed at MCP shutdown",
        )
    }

    #[test]
    fn mcp_telemetry_busy_project_does_not_block_another_project()
    -> Result<(), Box<dyn std::error::Error>> {
        if telemetry_disabled() {
            return Ok(());
        }
        let server = ProjectAtlasMcpServer::new(
            PathBuf::from("startup.db"),
            None,
            "shared-label".to_string(),
            false,
        );
        let temp = tempfile::tempdir()?;
        let (state_a, store_a) = usage_test_project(temp.path(), "project-a")?;
        let (state_b, store_b) = usage_test_project(temp.path(), "project-b")?;
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let server_a = server.clone();
        let a_handle = std::thread::spawn(move || -> Result<(), String> {
            server_a.record_usage_for_state(&state_a, &store_a, |_usage_instance| {
                entered_tx.send(()).map_err(|error| {
                    CliError::InvalidInput(format!("test coordination failed: {error}"))
                })?;
                release_rx
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|error| {
                        CliError::InvalidInput(format!("test coordination failed: {error}"))
                    })?;
                Ok(())
            });
            Ok(())
        });
        entered_rx.recv_timeout(Duration::from_secs(2))?;

        let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
        let server_b = server;
        let b_handle = std::thread::spawn(move || -> Result<(), String> {
            server_b.record_usage_for_state(&state_b, &store_b, |_usage_instance| {
                done_tx.send(()).map_err(|error| {
                    CliError::InvalidInput(format!("test coordination failed: {error}"))
                })?;
                Ok(())
            });
            Ok(())
        });
        let project_b_completed = done_rx.recv_timeout(Duration::from_secs(1)).is_ok();
        release_tx.send(())?;
        a_handle
            .join()
            .map_err(|_panic| io::Error::other("project A telemetry thread panicked"))?
            .map_err(io::Error::other)?;
        b_handle
            .join()
            .map_err(|_panic| io::Error::other("project B telemetry thread panicked"))?
            .map_err(io::Error::other)?;

        require(
            project_b_completed,
            "one project's blocked telemetry delayed another project",
        )
    }

    #[test]
    fn mcp_telemetry_keeps_identity_when_capacity_seal_fails()
    -> Result<(), Box<dyn std::error::Error>> {
        if telemetry_disabled() {
            return Ok(());
        }
        let temp = tempfile::tempdir()?;
        let (state, store) = usage_test_project(temp.path(), "selected-project")?;
        let server = ProjectAtlasMcpServer::new(
            state.db_path.clone(),
            None,
            "shared-label".to_string(),
            false,
        );
        server.record_usage_for_state(&state, &store, |usage_instance| {
            record_usage_estimate(
                &store,
                Some(usage_instance),
                "seal-failure-test",
                MCP_EVENT_ATLAS_OVERVIEW,
                None,
                None,
                8,
                "overview:\n  files: 1\n",
            )
        });
        let initial_identity = usage_runtime_identity(&server, &state, &store)?;
        let busy_connection = rusqlite::Connection::open(&state.db_path)?;
        busy_connection.execute_batch("BEGIN IMMEDIATE")?;
        let mut calls = 0usize;

        server.record_usage_for_state(&state, &store, |_usage_instance| {
            calls += 1;
            Err(CliError::Db(DbError::TelemetryBaselineCapacity))
        });
        busy_connection.execute_batch("ROLLBACK")?;

        require(calls == 1, "failed sealing unexpectedly retried the event")?;
        require(
            usage_runtime_identity(&server, &state, &store)? == initial_identity,
            "failed sealing replaced the still-active project identity",
        )
    }

    #[test]
    fn mcp_telemetry_rotates_and_retries_once_when_baselines_reach_capacity()
    -> Result<(), Box<dyn std::error::Error>> {
        if telemetry_disabled() {
            return Ok(());
        }
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("selected-project");
        fs::create_dir_all(root.join(".projectatlas"))?;
        let db_path = root.join(".projectatlas").join("projectatlas.db");
        let store = AtlasStore::open_for_project(&db_path, &root)?;
        let state = McpProjectState {
            root,
            db_path: db_path.clone(),
            config_path: None,
            worktree: None,
        };
        let other_root = temp.path().join("other-project");
        fs::create_dir_all(other_root.join(".projectatlas"))?;
        let other_db_path = other_root.join(".projectatlas").join("projectatlas.db");
        let other_store = AtlasStore::open_for_project(&other_db_path, &other_root)?;
        let other_state = McpProjectState {
            root: other_root,
            db_path: other_db_path,
            config_path: None,
            worktree: None,
        };
        let server = ProjectAtlasMcpServer::new(db_path, None, "shared-label".to_string(), false);
        server.record_usage_for_state(&state, &store, |usage_instance| {
            record_usage_estimate(
                &store,
                Some(usage_instance),
                "rotation-test",
                MCP_EVENT_ATLAS_OVERVIEW,
                None,
                None,
                8,
                "overview:\n  files: 1\n",
            )
        });
        server.record_usage_for_state(&other_state, &other_store, |_usage_instance| Ok(()));
        let initial_identity = usage_runtime_identity(&server, &state, &store)?;
        let other_identity = usage_runtime_identity(&server, &other_state, &other_store)?;
        let mut calls = 0usize;

        server.record_usage_for_state(&state, &store, |_usage_instance| {
            calls += 1;
            if calls == 1 {
                Err(CliError::Db(DbError::TelemetryBaselineCapacity))
            } else {
                Ok(())
            }
        });

        let tracked = server
            .usage_runtime
            .lock()
            .map_err(|_poisoned| io::Error::other("usage runtime lock poisoned"))?
            .entries
            .len();
        let rotated_identity = usage_runtime_identity(&server, &state, &store)?;
        require(calls == 2, "capacity handling did not retry exactly once")?;
        require(
            rotated_identity != initial_identity,
            "capacity handling did not rotate the bounded runtime identity",
        )?;
        require(
            tracked == 2,
            "rotation changed the bounded project binding inventory",
        )?;
        require(
            usage_runtime_identity(&server, &other_state, &other_store)? == other_identity,
            "one project's capacity rotation changed another project's identity",
        )
    }

    #[test]
    fn navigation_result_survives_telemetry_write_failure() -> Result<(), Box<dyn std::error::Error>>
    {
        if telemetry_disabled() {
            return Ok(());
        }
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join("src"))?;
        fs::write(repo.join("src").join("lib.rs"), "pub fn owner() {}\n")?;
        let config_path = repo.join(".projectatlas").join("config.toml");
        init_project_with_config(&repo, Some(&config_path))?;
        let db_path = repo.join(".projectatlas").join("projectatlas.db");
        let plan = ScanRuntimePlan::for_path(Some(&config_path), &repo, None)?;
        let mut store = open_atlas_store_for_project(&db_path, &repo)?;
        run_scan_pipeline(
            &mut store,
            &plan,
            &SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, Some(1), Some(30)),
        )?;
        drop(store);

        let connection = rusqlite::Connection::open(&db_path)?;
        connection.execute_batch("BEGIN IMMEDIATE")?;

        let server = ProjectAtlasMcpServer::new(
            db_path,
            Some(config_path),
            "shared-label".to_string(),
            false,
        );
        let result = server.atlas_overview_response(
            AtlasProjectParams {
                project_path: None,
                worktree: None,
            },
            None,
        );
        connection.execute_batch("ROLLBACK")?;

        if result.contains("overview:") && result.contains("files:") {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "telemetry failure replaced an already-built navigation result: {result}"
            ))
            .into())
        }
    }

    #[test]
    fn mcp_calls_share_server_identity_and_new_server_uses_another()
    -> Result<(), Box<dyn std::error::Error>> {
        if telemetry_disabled() {
            return Ok(());
        }
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join("src"))?;
        fs::write(repo.join("src").join("lib.rs"), "pub fn owner() {}\n")?;
        let config_path = repo.join(".projectatlas").join("config.toml");
        init_project_with_config(&repo, Some(&config_path))?;
        let db_path = repo.join(".projectatlas").join("projectatlas.db");
        let plan = ScanRuntimePlan::for_path(Some(&config_path), &repo, None)?;
        let mut store = open_atlas_store_for_project(&db_path, &repo)?;
        run_scan_pipeline(
            &mut store,
            &plan,
            &SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, Some(1), Some(30)),
        )?;
        drop(store);

        let call_overview = |server: &ProjectAtlasMcpServer| {
            server.atlas_overview_response(
                AtlasProjectParams {
                    project_path: None,
                    worktree: None,
                },
                None,
            )
        };
        let first = ProjectAtlasMcpServer::new(
            db_path.clone(),
            Some(config_path.clone()),
            "shared-label".to_string(),
            false,
        );
        require(
            call_overview(&first).contains("overview:"),
            "first MCP call failed",
        )?;
        require(
            call_overview(&first).contains("overview:"),
            "second MCP call failed",
        )?;
        let restarted = ProjectAtlasMcpServer::new(
            db_path.clone(),
            Some(config_path),
            "shared-label".to_string(),
            false,
        );
        require(
            call_overview(&restarted).contains("overview:"),
            "restarted MCP call failed",
        )?;

        let connection = rusqlite::Connection::open(db_path)?;
        let instances: i64 = connection.query_row(
            "SELECT COUNT(*) FROM usage_instances WHERE owner = 'mcp_process' AND caller_label = 'shared-label'",
            [],
            |row| row.get(0),
        )?;
        let events: i64 =
            connection.query_row("SELECT COUNT(*) FROM usage_events", [], |row| row.get(0))?;
        require(
            instances == 2 && events == 3,
            "MCP calls did not reuse one identity per server construction",
        )
    }

    #[test]
    fn mcp_database_filesystem_failures_are_typed_and_actionable()
    -> Result<(), Box<dyn std::error::Error>> {
        let error = CliError::Db(projectatlas_db::DbError::DatabaseFilesystemUnsupported {
            path: PathBuf::from("project")
                .join(".projectatlas")
                .join("projectatlas.db"),
            mount_point: Some(PathBuf::from("project")),
            filesystem_type: Some("nfs".to_string()),
        });
        let payload = ProjectAtlasMcpServer::encode_error_payload(&error);
        require(
            payload.contains("kind: database_filesystem_unsupported")
                && payload.contains("filesystem_type: nfs")
                && payload.contains("supported local filesystem"),
            "MCP TOON lost typed filesystem details or recovery guidance",
        )
    }

    #[cfg(unix)]
    #[test]
    fn mcp_project_mismatch_keeps_native_display_unavailable_typed()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let raw_root = temp
            .path()
            .join(std::ffi::OsString::from_vec(b"raw-root-\x80".to_vec()));
        let replacement_root = temp.path().join("raw-root-�");
        fs::create_dir(&raw_root)?;
        fs::create_dir(&replacement_root)?;
        let raw_identity = CanonicalProjectRoot::from_path(&raw_root)?;
        let replacement_identity = CanonicalProjectRoot::from_path(&replacement_root)?;
        let error = CliError::ProjectMismatch(Box::new(IndexProjectMismatch::from_native_roots(
            &raw_identity,
            &replacement_identity,
        )));
        let payload = ProjectAtlasMcpServer::encode_error_payload(&error);
        require(
            payload.contains("selected_project_root: null")
                && payload.contains("indexed_project_root:")
                && payload.contains("raw-root-�"),
            "MCP promoted a lossy native root into an ambiguous structured value",
        )?;

        let mapped = crate::runtime::project_store_error(DbError::ProjectRootMismatch {
            expected: raw_root.to_string_lossy().into_owned(),
            found: replacement_root.to_string_lossy().into_owned(),
        });
        let mapped_payload = ProjectAtlasMcpServer::encode_error_payload(&mapped);
        require(
            mapped_payload.contains("selected_project_root: null")
                && mapped_payload.contains("indexed_project_root: null")
                && mapped_payload.contains("does not match"),
            "MCP promoted lossy store mismatch text into structured roots",
        )
    }

    #[cfg(unix)]
    #[test]
    fn mcp_recovery_and_session_reports_omit_unavailable_native_root_selectors()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let raw_root = temp
            .path()
            .join(std::ffi::OsString::from_vec(b"repo-\x80".to_vec()));
        let replacement_root = temp.path().join("repo-�");
        fs::create_dir(&raw_root)?;
        fs::create_dir(&replacement_root)?;
        let raw_db = raw_root.join(".projectatlas").join("projectatlas.db");
        let replacement_db = replacement_root
            .join(".projectatlas")
            .join("projectatlas.db");
        let replacement_display = lossless_project_root_display(&replacement_root)
            .ok_or_else(|| io::Error::other("replacement root lost its UTF-8 display"))?;

        let init_errors = [
            (
                crate::runtime::index_init_required(&raw_root, &raw_db),
                false,
            ),
            (
                crate::runtime::index_init_required(&replacement_root, &replacement_db),
                true,
            ),
        ];
        for (error, displayable) in init_errors {
            let payload = ProjectAtlasMcpServer::encode_error_payload(&error);
            let value: serde_json::Value = toon_format::decode_default(&payload)?;
            if displayable {
                let expected = serde_json::Value::String(replacement_display.clone());
                require(
                    value.pointer("/error/init_required/project_root") == Some(&expected)
                        && value.pointer("/error/next/project_path") == Some(&expected),
                    "MCP init recovery lost a displayable root selector",
                )?;
            } else {
                require(
                    value
                        .pointer("/error/init_required/project_root")
                        .is_some_and(serde_json::Value::is_null)
                        && value.pointer("/error/next/project_path").is_none()
                        && !payload.contains("repo-�"),
                    "MCP init recovery exposed a lossy raw-root selector",
                )?;
            }
        }

        let refresh_errors = [
            (
                CliError::RefreshRequired(Box::new(IndexRefreshRequired {
                    project_root: lossless_project_root_display(&raw_root),
                    worktree: None,
                    status: IndexReadStatus::RefreshRequired,
                    reason: IndexRefreshReason::SourceChanged,
                    scope: IndexRefreshScope::Incremental,
                    changed: 1,
                    added: 0,
                    removed: 0,
                    modified: 1,
                    sample_paths: vec!["src/lib.rs".to_string()],
                })),
                false,
            ),
            (
                CliError::RefreshRequired(Box::new(IndexRefreshRequired {
                    project_root: lossless_project_root_display(&replacement_root),
                    worktree: None,
                    status: IndexReadStatus::RefreshRequired,
                    reason: IndexRefreshReason::SourceChanged,
                    scope: IndexRefreshScope::Incremental,
                    changed: 1,
                    added: 0,
                    removed: 0,
                    modified: 1,
                    sample_paths: vec!["src/lib.rs".to_string()],
                })),
                true,
            ),
        ];
        for (error, displayable) in refresh_errors {
            let payload = ProjectAtlasMcpServer::encode_error_payload(&error);
            let value: serde_json::Value = toon_format::decode_default(&payload)?;
            if displayable {
                let expected = serde_json::Value::String(replacement_display.clone());
                require(
                    value.pointer("/error/refresh_required/project_root") == Some(&expected)
                        && value.pointer("/error/next/project_path") == Some(&expected),
                    "MCP refresh recovery lost a displayable root selector",
                )?;
            } else {
                require(
                    value
                        .pointer("/error/refresh_required/project_root")
                        .is_some_and(serde_json::Value::is_null)
                        && value.pointer("/error/next/project_path").is_none()
                        && !payload.contains("repo-�"),
                    "MCP refresh recovery exposed a lossy raw-root selector",
                )?;
            }
        }

        let raw_server = ProjectAtlasMcpServer::new(raw_db, None, "raw-session".to_string(), false);
        let raw_project = serde_json::to_value(ProjectAtlasMcpServer::project_state_payload(
            &raw_server.control_state,
        ))?;
        let raw_capability = serde_json::to_value(
            ProjectAtlasMcpServer::selected_project_capability(&raw_server.control_state),
        )?;
        require(
            raw_project.get("root") == Some(&serde_json::Value::Null)
                && raw_project.get("db") == Some(&serde_json::Value::Null)
                && raw_capability.get("root") == Some(&serde_json::Value::Null)
                && raw_capability.get("db") == Some(&serde_json::Value::Null)
                && !raw_project.to_string().contains("repo-�")
                && !raw_capability.to_string().contains("repo-�"),
            "MCP selected-project reports exposed a lossy raw-root projection",
        )?;
        let raw_brief = raw_server.build_session_brief(
            AtlasSessionBriefParams {
                project_path: None,
                worktree: None,
                query: None,
                purpose_task: None,
                compact: None,
                folder_limit: None,
                file_limit: None,
                blocker_limit: None,
                purpose_limit: None,
            },
            None,
        )?;
        let raw_value = serde_json::to_value(&raw_brief)?;
        require(
            raw_value.pointer("/project/root") == Some(&serde_json::Value::Null)
                && raw_value
                    .pointer("/recommendations/0/arguments/project_path")
                    .is_none()
                && raw_value
                    .pointer("/recommendations/0/arguments/worktree")
                    .is_none()
                && !raw_value.to_string().contains("repo-�"),
            "MCP session brief offered a lossy sibling selector for a raw root",
        )?;

        let replacement_server = ProjectAtlasMcpServer::new(
            replacement_db,
            None,
            "replacement-session".to_string(),
            false,
        );
        let replacement_project = serde_json::to_value(
            ProjectAtlasMcpServer::project_state_payload(&replacement_server.control_state),
        )?;
        let replacement_capability = serde_json::to_value(
            ProjectAtlasMcpServer::selected_project_capability(&replacement_server.control_state),
        )?;
        require(
            replacement_project
                .get("root")
                .and_then(|value| value.as_str())
                == Some(replacement_display.as_str())
                && replacement_capability
                    .get("root")
                    .and_then(|value| value.as_str())
                    == Some(replacement_display.as_str()),
            "MCP selected-project reports lost the displayable root",
        )?;
        let replacement_brief = replacement_server.build_session_brief(
            AtlasSessionBriefParams {
                project_path: None,
                worktree: None,
                query: None,
                purpose_task: None,
                compact: None,
                folder_limit: None,
                file_limit: None,
                blocker_limit: None,
                purpose_limit: None,
            },
            None,
        )?;
        let replacement_value = serde_json::to_value(&replacement_brief)?;
        require(
            replacement_value
                .pointer("/project/root")
                .and_then(|value| value.as_str())
                == Some(replacement_display.as_str())
                && replacement_value
                    .pointer("/recommendations/0/arguments/project_path")
                    .and_then(|value| value.as_str())
                    == Some(replacement_display.as_str()),
            "MCP session brief lost the displayable root recovery selector",
        )?;
        require(
            !raw_root.join(".projectatlas").exists()
                && !replacement_root.join(".projectatlas").exists(),
            "MCP missing-index recovery mutated a project root",
        )
    }

    #[test]
    fn mcp_schema_version_mismatches_are_typed_and_content_free()
    -> Result<(), Box<dyn std::error::Error>> {
        let supported = projectatlas_db::CURRENT_SCHEMA_VERSION;
        let future = supported + 1;
        let error = CliError::Db(DbError::SchemaVersion {
            found: future,
            expected: supported,
        });
        let payload = ProjectAtlasMcpServer::encode_error_payload(&error);
        require(
            payload.contains("kind: schema_version_mismatch")
                && payload.contains(&format!("found_schema_version: {future}"))
                && payload.contains(&format!("supported_schema_version: {supported}"))
                && payload.contains(env!("CARGO_PKG_VERSION"))
                && payload.contains("do not reset")
                && !payload.contains(".projectatlas")
                && !payload.contains("session_id")
                && !payload.contains("project_root"),
            "MCP TOON lost typed schema-version details or exposed database context",
        )?;

        let predecessor = CliError::Service(ServiceError::Db(DbError::SchemaVersion {
            found: 8,
            expected: supported,
        }));
        let McpToolTextResult(predecessor_result) =
            ProjectAtlasMcpServer::as_mcp_text(Err(predecessor));
        let predecessor_payload = predecessor_result.map_err(std::io::Error::other)?;
        require(
            predecessor_payload.contains("kind: schema_migration_required")
                && predecessor_payload.contains("found_schema_version: 8")
                && predecessor_payload.contains(&format!("supported_schema_version: {supported}"))
                && predecessor_payload
                    .contains(&format!("migration_steps_remaining: {}", supported - 8))
                && predecessor_payload.contains("projectatlas init")
                && predecessor_payload.contains("atlas_init")
                && predecessor_payload.contains("same global `--db`/`--config` selection")
                && predecessor_payload.contains("same MCP server/database binding")
                && !predecessor_payload.contains("schema_version_mismatch")
                && !predecessor_payload.contains(crate::SCHEMA_VERSION_MISMATCH_RECOVERY),
            "MCP omitted the supported-predecessor migration handoff",
        )
    }

    #[test]
    fn mcp_search_capability_failures_are_typed_and_actionable()
    -> Result<(), Box<dyn std::error::Error>> {
        let error = CliError::Service(ServiceError::SearchCapabilityUnavailable {
            requested_mode: projectatlas_service::SearchRetrievalMode::Hybrid,
            state: "not-installed",
            guidance: "install and build a compatible semantic generation",
        });
        let payload = ProjectAtlasMcpServer::encode_error_payload(&error);
        require(
            payload.contains("kind: search_capability_unavailable")
                && payload.contains("requested_mode")
                && payload.contains("hybrid")
                && payload.contains("state")
                && payload.contains("not-installed")
                && payload.contains("compatible semantic generation"),
            "MCP TOON lost typed search-capability state or recovery guidance",
        )
    }

    #[test]
    fn mcp_records_usage_only_for_the_accepted_verified_attempt()
    -> Result<(), Box<dyn std::error::Error>> {
        if telemetry_disabled() {
            return Ok(());
        }
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        let source = repo.join("src").join("lib.rs");
        fs::create_dir_all(
            source
                .parent()
                .ok_or_else(|| io::Error::other("missing parent"))?,
        )?;
        let original = "pub fn original() {}\n";
        let revised = "pub fn revised() {}\n";
        fs::write(&source, original)?;
        let config_path = repo.join(".projectatlas").join("config.toml");
        init_project_with_config(&repo, Some(&config_path))?;
        let db_path = repo.join(".projectatlas").join("projectatlas.db");
        let plan = ScanRuntimePlan::for_path(Some(&config_path), &repo, None)?;
        let mut store = open_atlas_store_for_project(&db_path, &repo)?;
        run_scan_pipeline(
            &mut store,
            &plan,
            &SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, Some(1), Some(30)),
        )?;
        drop(store);
        let state = McpProjectState {
            root: repo.clone(),
            db_path: db_path.clone(),
            config_path: Some(config_path.clone()),
            worktree: None,
        };
        let server = ProjectAtlasMcpServer::new(
            db_path.clone(),
            Some(config_path.clone()),
            "accepted-attempt".to_string(),
            false,
        );
        let mut attempts = 0_u64;

        let response =
            server.with_fresh_string_and_usage_for_request(&state, None, |store, _stamp| {
                attempts = attempts.saturating_add(1);
                let hash = store
                    .load_node_by_path("src/lib.rs")?
                    .and_then(|node| node.node.content_hash)
                    .ok_or_else(|| CliError::InvalidInput("source hash missing".to_string()))?;
                if attempts == 1 {
                    fs::write(&source, revised).map_err(|source_error| CliError::Io {
                        path: source.clone(),
                        source: source_error,
                    })?;
                    server.source_observations.inject_test_event(
                        &db_path,
                        &repo,
                        Some(&config_path),
                        Event::new(EventKind::Modify(ModifyKind::Any)).add_path(source.clone()),
                    )?;
                }
                Ok((
                    hash,
                    Some(McpUsageIntent::estimate(
                        MCP_EVENT_ATLAS_OVERVIEW,
                        None,
                        None,
                        1,
                    )),
                ))
            })?;

        require(
            attempts >= 2,
            "mid-query source edit did not retry the query",
        )?;
        require(
            response == blake3::hash(revised.as_bytes()).to_hex().to_string(),
            "MCP returned the provisional pre-edit result",
        )?;
        let connection = rusqlite::Connection::open(db_path)?;
        let events: i64 =
            connection.query_row("SELECT COUNT(*) FROM usage_events", [], |row| row.get(0))?;
        require(
            events == 1,
            "MCP telemetry recorded a discarded provisional attempt",
        )
    }

    /// Wait for one admitted task to reach a terminal state.
    fn wait_for_background_task(
        server: &ProjectAtlasMcpServer,
        task_id: &str,
    ) -> Result<McpTaskRecord, Box<dyn std::error::Error>> {
        for _attempt in 0..5_000 {
            let status = server.task_status(task_id.to_string())?;
            if let Some(record) = status.task.filter(McpTaskRecord::is_terminal_state) {
                return Ok(record);
            }
            thread::sleep(Duration::from_millis(1));
        }
        Err(io::Error::other("background task did not reach a terminal state").into())
    }

    /// Admit one successful task and wait for completion.
    fn run_successful_background_task(
        server: &ProjectAtlasMcpServer,
    ) -> Result<McpTaskRecord, Box<dyn std::error::Error>> {
        let task = server.start_index_task(
            McpTaskOperation::Scan,
            SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None),
            MCP_TOOL_ATLAS_OVERVIEW,
            |_control, _options| Ok(()),
        )?;
        wait_for_background_task(server, &task.task_id)
    }

    /// Wait for the latest task of one operation to reach a terminal state.
    fn wait_for_background_operation(
        server: &ProjectAtlasMcpServer,
        operation: &McpTaskOperation,
    ) -> Result<McpTaskRecord, Box<dyn std::error::Error>> {
        let task_id = server
            .task_registry
            .read()
            .map_err(|_poisoned| io::Error::other("task registry lock poisoned"))?
            .records
            .iter()
            .rev()
            .find(|record| &record.operation == operation)
            .map(|record| record.task_id.clone())
            .ok_or_else(|| io::Error::other("background task was not admitted"))?;
        wait_for_background_task(server, &task_id)
    }

    /// Assert that persisted index work is visible through normal agent reads.
    fn require_agent_index_reads(
        server: &ProjectAtlasMcpServer,
        project_path: &str,
        expected_symbol: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let overview = server.atlas_overview_response(
            AtlasProjectParams {
                project_path: Some(project_path.to_string()),
                worktree: None,
            },
            None,
        );
        require(
            overview.contains("overview:"),
            "agent overview did not read the published index",
        )?;

        let summary = server.atlas_file_summary_response(
            &AtlasFileSummaryParams {
                project_path: Some(project_path.to_string()),
                worktree: None,
                file: "src/lib.rs".to_string(),
                nearest_project: Some(false),
                compact: None,
                content_selection: None,
                limit: Some(25),
            },
            None,
        );
        require(
            summary.contains("file_summary:")
                && summary.contains("src/lib.rs")
                && summary.contains(expected_symbol),
            "agent file summary omitted published source facts",
        )?;

        let symbols = server.atlas_symbols_response(
            &AtlasSymbolsParams {
                project_path: Some(project_path.to_string()),
                worktree: None,
                file: Some("src/lib.rs".to_string()),
                nearest_project: Some(false),
                query: None,
                content_selection: None,
                limit: Some(50),
            },
            None,
        );
        require(
            symbols.contains("symbols[") && symbols.contains(expected_symbol),
            "agent symbol read omitted published parser output",
        )?;

        let relations = server.atlas_symbol_relations_response(
            &AtlasSymbolRelationsParams {
                project_path: Some(project_path.to_string()),
                file: Some("src/lib.rs".to_string()),
                nearest_project: Some(false),
                query: None,
                limit: Some(50),
                ..AtlasSymbolRelationsParams::default()
            },
            None,
        );
        require(
            relations.contains("relations[") && relations.contains(expected_symbol),
            "agent relation read omitted published graph output",
        )?;
        let explicit_legacy = server.atlas_symbol_relations_response(
            &AtlasSymbolRelationsParams {
                project_path: Some(project_path.to_string()),
                file: Some("src/lib.rs".to_string()),
                nearest_project: Some(false),
                view: Some("legacy".to_string()),
                query: None,
                limit: Some(50),
                ..AtlasSymbolRelationsParams::default()
            },
            None,
        );
        require(
            relations == explicit_legacy,
            "explicit MCP legacy relation view changed default response bytes or ordering",
        )?;
        let compact_legacy = server.atlas_symbol_relations_response(
            &AtlasSymbolRelationsParams {
                project_path: Some(project_path.to_string()),
                file: Some("src/lib.rs".to_string()),
                nearest_project: Some(false),
                compact: Some(true),
                limit: Some(50),
                ..AtlasSymbolRelationsParams::default()
            },
            None,
        );
        require(
            compact_legacy.contains(MCP_ERROR_COMPACT_DETAILED_RELATION_VIEW),
            "compact relation projection did not reject the legacy view",
        )?;
        let zero_limit_legacy = server.atlas_symbol_relations_response(
            &AtlasSymbolRelationsParams {
                project_path: Some(project_path.to_string()),
                file: Some("src/lib.rs".to_string()),
                nearest_project: Some(false),
                limit: Some(0),
                ..AtlasSymbolRelationsParams::default()
            },
            None,
        );
        require(
            zero_limit_legacy.contains("relations[1]"),
            "MCP legacy zero limit no longer preserves its one-row compatibility behavior",
        )?;
        let detailed = server.atlas_symbol_relations_response(
            &AtlasSymbolRelationsParams {
                project_path: Some(project_path.to_string()),
                file: Some("src/lib.rs".to_string()),
                nearest_project: Some(false),
                view: Some("detailed".to_string()),
                direction: Some("outbound".to_string()),
                limit: Some(50),
                ..AtlasSymbolRelationsParams::default()
            },
            None,
        );
        require(
            detailed.contains("symbol_relations:") && detailed.contains("anchor:"),
            "detailed MCP relation route did not return the bounded graph envelope",
        )?;
        if expected_symbol == "third" {
            let compact_detailed = server.atlas_symbol_relations_response(
                &AtlasSymbolRelationsParams {
                    project_path: Some(project_path.to_string()),
                    file: Some("src/lib.rs".to_string()),
                    nearest_project: Some(false),
                    view: Some("detailed".to_string()),
                    compact: Some(true),
                    symbol: Some("first".to_string()),
                    direction: Some("outbound".to_string()),
                    include_occurrences: Some(true),
                    limit: Some(1),
                    output_bytes: Some(8 * 1_024),
                    ..AtlasSymbolRelationsParams::default()
                },
                None,
            );
            require(
                compact_detailed.len() <= 8 * 1_024
                    && compact_detailed.contains("returned: 1")
                    && compact_detailed.contains("status: resolved")
                    && compact_detailed.contains("confidence: exact")
                    && compact_detailed.contains("completeness: complete")
                    && compact_detailed.contains("Own café λ relation navigation")
                    && compact_detailed.contains("next_call:")
                    && compact_detailed.contains("occurrences[1]:")
                    && !compact_detailed.contains("occurrences[1]:\n        - relation:"),
                "compact detailed relation omitted trust, purpose, occurrence, next-call, or bounded-output behavior",
            )?;
            let first_detailed_page = server.atlas_symbol_relations_response(
                &AtlasSymbolRelationsParams {
                    project_path: Some(project_path.to_string()),
                    file: Some("src/lib.rs".to_string()),
                    nearest_project: Some(false),
                    view: Some("detailed".to_string()),
                    symbol: Some("first".to_string()),
                    direction: Some("outbound".to_string()),
                    depth: Some(2),
                    limit: Some(1),
                    output_bytes: Some(64 * 1024),
                    ..AtlasSymbolRelationsParams::default()
                },
                None,
            );
            let first_detailed_value: serde_json::Value =
                toon_format::decode_default(&first_detailed_page)?;
            let first_detailed_report = first_detailed_value
                .get("symbol_relations")
                .ok_or_else(|| io::Error::other("first detailed MCP page omitted its envelope"))?;
            let continuation = first_detailed_report
                .get("continuation")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| io::Error::other("first detailed MCP page omitted its cursor"))?;
            require(
                first_detailed_report
                    .get("returned")
                    .and_then(serde_json::Value::as_u64)
                    == Some(1)
                    && first_detailed_report
                        .get("rows")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|rows| rows.len() == 1),
                "first detailed MCP page was not a nonempty bounded symbol result",
            )?;
            let second_detailed_page = server.atlas_symbol_relations_response(
                &AtlasSymbolRelationsParams {
                    project_path: Some(project_path.to_string()),
                    file: Some("src/lib.rs".to_string()),
                    nearest_project: Some(false),
                    view: Some("detailed".to_string()),
                    cursor: Some(continuation.to_string()),
                    symbol: Some("first".to_string()),
                    direction: Some("outbound".to_string()),
                    depth: Some(2),
                    limit: Some(1),
                    output_bytes: Some(64 * 1024),
                    ..AtlasSymbolRelationsParams::default()
                },
                None,
            );
            let second_detailed_value: serde_json::Value =
                toon_format::decode_default(&second_detailed_page)?;
            let second_detailed_report = second_detailed_value
                .get("symbol_relations")
                .ok_or_else(|| io::Error::other("second detailed MCP page omitted its envelope"))?;
            require(
                second_detailed_report
                    .get("returned")
                    .and_then(serde_json::Value::as_u64)
                    == Some(1)
                    && second_detailed_report.get("rows") != first_detailed_report.get("rows")
                    && second_detailed_page.contains("Own café λ relation navigation"),
                "detailed MCP cursor did not resume a distinct Unicode-safe symbol row",
            )?;
            let compact_continuation_page = server.atlas_symbol_relations_response(
                &AtlasSymbolRelationsParams {
                    project_path: Some(project_path.to_string()),
                    file: Some("src/lib.rs".to_string()),
                    nearest_project: Some(false),
                    view: Some("detailed".to_string()),
                    compact: Some(true),
                    symbol: Some("first".to_string()),
                    symbol_parent: Some(String::new()),
                    direction: Some("outbound".to_string()),
                    depth: Some(2),
                    limit: Some(1),
                    output_bytes: Some(64 * 1024),
                    ..AtlasSymbolRelationsParams::default()
                },
                None,
            );
            let compact_continuation_value: serde_json::Value =
                toon_format::decode_default(&compact_continuation_page)?;
            let compact_continuation_report = compact_continuation_value
                .get("symbol_relations")
                .ok_or_else(|| io::Error::other("compact relation page omitted its envelope"))?;
            let compact_next_call = compact_continuation_report
                .get("next_call")
                .ok_or_else(|| io::Error::other("compact relation page omitted its next call"))?;
            require(
                compact_next_call
                    .get("tool")
                    .and_then(serde_json::Value::as_str)
                    == Some(MCP_TOOL_ATLAS_SYMBOL_RELATIONS),
                "compact relation continuation did not name its owning MCP tool",
            )?;
            let compact_next_arguments = compact_next_call
                .get("arguments")
                .cloned()
                .ok_or_else(|| io::Error::other("compact next call omitted its arguments"))?;
            require(
                compact_next_arguments.get("cursor").is_some()
                    && compact_next_arguments.get("symbol_parent").is_none(),
                "compact next call did not preserve its cursor or normalize an empty parent",
            )?;
            let compact_next_params: AtlasSymbolRelationsParams =
                serde_json::from_value(compact_next_arguments)?;
            let compact_resumed_page =
                server.atlas_symbol_relations_response(&compact_next_params, None);
            require(
                compact_resumed_page.contains("symbol_relations:")
                    && !compact_resumed_page.contains("cursor does not match query")
                    && !compact_resumed_page.contains("graph symbol anchor is not available"),
                "compact relation next call was not directly reusable",
            )?;
        }
        let bounded_output_bytes = 4 * 1024_u32;
        let bounded = server.atlas_symbol_relations_response(
            &AtlasSymbolRelationsParams {
                project_path: Some(project_path.to_string()),
                file: Some("src/lib.rs".to_string()),
                nearest_project: Some(false),
                view: Some("detailed".to_string()),
                direction: Some("outbound".to_string()),
                limit: Some(50),
                edge_limit: Some(50),
                node_limit: Some(50),
                visited_limit: Some(50),
                occurrence_total_limit: Some(50),
                intermediate_bytes: Some(128 * 1024),
                deadline_ms: Some(2_000),
                output_bytes: Some(bounded_output_bytes),
                ..AtlasSymbolRelationsParams::default()
            },
            None,
        );
        require(
            bounded.contains("symbol_relations:")
                && bounded.len() <= bounded_output_bytes as usize
                && bounded.contains("Own café λ relation navigation")
                && bounded.contains(&format!("rendered_output_bytes: {}", bounded.len())),
            "detailed MCP relation output did not enforce or report the exact routed envelope bytes",
        )?;

        let analysis = server.atlas_symbol_relations_response(
            &AtlasSymbolRelationsParams {
                project_path: Some(project_path.to_string()),
                file: Some("src/lib.rs".to_string()),
                nearest_project: Some(false),
                view: Some("analysis".to_string()),
                symbol: Some("first".to_string()),
                direction: Some("outbound".to_string()),
                depth: Some(2),
                limit: Some(50),
                output_bytes: Some(64 * 1024),
                include_communities: Some(true),
                include_cycles: Some(true),
                ..AtlasSymbolRelationsParams::default()
            },
            None,
        );
        require(
            analysis.contains("symbol_relations:")
                && analysis.contains("mode: architecture")
                && analysis.contains("findings[")
                && analysis.contains("next_call:")
                && analysis.contains("work:"),
            "MCP relation analysis omitted its closed mode, findings, work, or reusable next call",
        )?;

        let state = ProjectAtlasMcpServer::project_state_from_root(Path::new(project_path))?;
        let publication_before = ProjectAtlasMcpServer::open_read_store(&state)?
            .index_publication()?
            .ok_or_else(|| io::Error::other("MCP impact fixture publication missing"))?;
        let task_records_before = server
            .task_registry
            .read()
            .map_err(|_poisoned| io::Error::other("task registry lock poisoned"))?
            .records
            .len();
        let impact_started = Instant::now();
        let impact = server.atlas_symbol_relations_response(
            &AtlasSymbolRelationsParams {
                project_path: Some(project_path.to_string()),
                file: Some("src/lib.rs".to_string()),
                nearest_project: Some(false),
                view: Some("analysis".to_string()),
                symbol: Some("first".to_string()),
                direction: Some("outbound".to_string()),
                depth: Some(2),
                limit: Some(8),
                edge_limit: Some(8),
                node_limit: Some(16),
                visited_limit: Some(16),
                occurrence_total_limit: Some(16),
                intermediate_bytes: Some(128 * 1_024),
                deadline_ms: Some(1_000),
                output_bytes: Some(64 * 1_024),
                analysis_mode: Some("impact".to_string()),
                vcs: Some("working_tree".to_string()),
                include_dead_code: Some(true),
                ..AtlasSymbolRelationsParams::default()
            },
            None,
        );
        let impact_elapsed = impact_started.elapsed();
        require(
            impact_elapsed <= Duration::from_secs(5)
                && impact.contains("mode: impact")
                && (impact.contains("state: available") || impact.contains("state: unavailable")),
            "MCP impact analysis exceeded its elapsed tolerance or omitted typed mode/VCS state",
        )?;
        let impact_value: serde_json::Value = toon_format::decode_default(&impact)?;
        let impact_report = impact_value
            .get(MCP_PAYLOAD_SYMBOL_RELATIONS)
            .ok_or_else(|| io::Error::other("MCP impact response omitted its envelope"))?;
        let bounded_work = [
            ("/returned", 8_u64),
            ("/work/relations/inspected_edges", 8),
            ("/work/relations/active_nodes", 16),
            ("/work/relations/visited_nodes", 16),
            ("/work/analyzed_nodes", 16),
            ("/work/analyzed_edges", 8),
            ("/work/peak_intermediate_bytes", 128 * 1_024),
            ("/work/rendered_output_bytes", 64 * 1_024),
        ];
        require(
            impact.len() <= 64 * 1_024
                && bounded_work.iter().all(|(path, limit)| {
                    impact_report
                        .pointer(path)
                        .and_then(serde_json::Value::as_u64)
                        .is_some_and(|observed| observed <= *limit)
                }),
            "MCP impact analysis crossed or omitted a declared row/node/edge/visited/intermediate/output budget",
        )?;
        require(
            ProjectAtlasMcpServer::open_read_store(&state)?
                .index_publication()?
                .as_ref()
                == Some(&publication_before)
                && server
                    .task_registry
                    .read()
                    .map_err(|_poisoned| io::Error::other("task registry lock poisoned"))?
                    .records
                    .len()
                    == task_records_before,
            "read-only MCP impact analysis changed publication or retained a task record",
        )?;
        let follow_up_started = Instant::now();
        let follow_up = server.atlas_overview_response(
            AtlasProjectParams {
                project_path: Some(project_path.to_string()),
                worktree: None,
            },
            None,
        );
        require(
            follow_up_started.elapsed() <= Duration::from_secs(2)
                && follow_up.contains("overview:"),
            "immediate MCP follow-up read was not responsive after bounded impact analysis",
        )?;

        let trace = server.atlas_symbol_relations_response(
            &AtlasSymbolRelationsParams {
                project_path: Some(project_path.to_string()),
                file: Some("src/lib.rs".to_string()),
                nearest_project: Some(false),
                view: Some("analysis".to_string()),
                symbol: Some("first".to_string()),
                direction: Some("outbound".to_string()),
                depth: Some(2),
                limit: Some(50),
                analysis_mode: Some("trace".to_string()),
                trace_target: Some("second".to_string()),
                trace_target_file: Some("src/lib.rs".to_string()),
                trace_target_kind: Some("function".to_string()),
                trace_target_signature: Some("fn second ( )".to_string()),
                ..AtlasSymbolRelationsParams::default()
            },
            None,
        );
        require(
            trace.contains("mode: trace")
                && trace.contains("kind: static_trace")
                && trace.contains("status: confirmed")
                && trace.contains("name: second")
                && trace.contains("capability: symbol_slice"),
            "MCP trace analysis omitted its confirmed path or reusable exact selector",
        )?;

        let misplaced = server.atlas_symbol_relations_response(
            &AtlasSymbolRelationsParams {
                project_path: Some(project_path.to_string()),
                file: Some("src/lib.rs".to_string()),
                nearest_project: Some(false),
                view: Some("detailed".to_string()),
                analysis_mode: Some("impact".to_string()),
                ..AtlasSymbolRelationsParams::default()
            },
            None,
        );
        require(
            misplaced.contains("analysis controls require view=analysis"),
            "MCP detailed relation view accepted analysis-only controls",
        )?;

        let missing_trace_target = server.atlas_symbol_relations_response(
            &AtlasSymbolRelationsParams {
                project_path: Some(project_path.to_string()),
                file: Some("src/lib.rs".to_string()),
                nearest_project: Some(false),
                view: Some("analysis".to_string()),
                analysis_mode: Some("trace".to_string()),
                ..AtlasSymbolRelationsParams::default()
            },
            None,
        );
        require(
            missing_trace_target.contains("analysis trace requires an exact file or symbol target"),
            "MCP trace analysis accepted a missing exact target",
        )?;

        let misplaced_vcs = server.atlas_symbol_relations_response(
            &AtlasSymbolRelationsParams {
                project_path: Some(project_path.to_string()),
                file: Some("src/lib.rs".to_string()),
                nearest_project: Some(false),
                view: Some("analysis".to_string()),
                analysis_mode: Some("architecture".to_string()),
                vcs: Some("working_tree".to_string()),
                ..AtlasSymbolRelationsParams::default()
            },
            None,
        );
        require(
            misplaced_vcs.contains("VCS selection is valid only for impact analysis"),
            "MCP silently dropped an explicit VCS selector outside impact mode",
        )
    }

    #[test]
    fn current_dir_alias_paths_use_active_mcp_project() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo-a");
        fs::create_dir(&repo)?;
        let db_path = repo.join(".projectatlas").join("projectatlas.db");
        let server = ProjectAtlasMcpServer::new(db_path, None, "mcp-test".to_string(), false);
        let expected_root = canonical_project_root(&repo)?;

        let (_state, root) =
            server.state_and_root_path(None, None, Some("./".to_string()), false)?;
        require(
            root == expected_root,
            "current-dir alias did not use active root",
        )?;

        #[cfg(windows)]
        {
            let (_state, root) =
                server.state_and_root_path(None, None, Some(".\\".to_string()), false)?;
            require(
                root == expected_root,
                "windows current-dir alias did not use active root",
            )?;
        }

        Ok(())
    }

    #[test]
    fn worktree_list_retains_retired_rows_at_structural_capacity()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let primary = temp.path().join("control");
        fs::create_dir(&primary)?;
        run_fixture_command(StdCommand::new("git").current_dir(&primary).arg("init"))?;
        let git_directory = primary.join(".git");
        let structural_registrations = git_directory.join("worktrees");
        fs::create_dir(&structural_registrations)?;
        for index in 0..MAX_GIT_WORKTREE_REGISTRATIONS {
            let administrative_directory =
                structural_registrations.join(format!("missing-{index:04}"));
            fs::create_dir(&administrative_directory)?;
            fs::write(
                administrative_directory.join("gitdir"),
                temp.path()
                    .join(format!("missing-{index:04}"))
                    .join(".git")
                    .to_string_lossy()
                    .as_bytes(),
            )?;
        }

        let database = primary.join(".projectatlas").join("projectatlas.db");
        fs::create_dir_all(
            database
                .parent()
                .ok_or_else(|| io::Error::other("control database has no parent"))?,
        )?;
        let store = AtlasStore::open_for_project(&database, &primary)?;
        let retired_alias = WorktreeAlias::parse("retired-at-capacity")?;
        let retired_registration = store.register_worktree(
            &retired_alias,
            &git_directory,
            &structural_registrations.join("retired"),
            &"ab".repeat(32),
            &temp.path().join("retired"),
            None,
            1,
        )?;
        drop(store);

        let server =
            ProjectAtlasMcpServer::new(database.clone(), None, "worktree-list".to_string(), false);
        let active = server.atlas_worktree_list(Parameters(AtlasWorktreeListParams {
            include_retired: Some(false),
        }));
        require(
            active.contains("total_worktrees: 1026")
                && active.contains("truncated: false")
                && active.contains("retired-at-capacity")
                && active.contains("missing-1023"),
            "combined capacity starved a structural or active missing registration",
        )?;

        let store = AtlasStore::open_for_project(&database, &primary)?;
        store.retire_worktree(retired_registration.registration_id, &retired_alias, 2)?;
        drop(store);

        let listed = server.atlas_worktree_list(Parameters(AtlasWorktreeListParams {
            include_retired: Some(true),
        }));
        require(
            listed.contains("total_worktrees: 1025") && listed.contains("retired-at-capacity"),
            "full structural inventory starved the requested retired registration",
        )
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_worktree_identities_are_blocked_before_join_or_selection()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let control_root = temp.path().join("control");
        let common_directory = control_root.join(".git");
        fs::create_dir_all(&common_directory)?;
        let database = control_root.join(".projectatlas").join("projectatlas.db");
        fs::create_dir_all(
            database
                .parent()
                .ok_or_else(|| io::Error::other("control database has no parent"))?,
        )?;
        let store = AtlasStore::open_for_project(&database, &control_root)?;
        let server =
            ProjectAtlasMcpServer::new(database, None, "non-utf8-worktrees".to_string(), false);
        let mut entries = Vec::new();

        for (index, terminal_byte) in [0xff, 0xfe].into_iter().enumerate() {
            let name = std::ffi::OsString::from_vec(vec![b'w', b't', b'-', terminal_byte]);
            let administrative_directory = common_directory.join("worktrees").join(&name);
            let root = temp.path().join(name);
            fs::create_dir_all(&administrative_directory)?;
            fs::create_dir_all(&root)?;
            let entry = GitWorktreeEntry {
                role: GitWorktreeRole::Linked,
                administrative_directory: administrative_directory.clone(),
                state: GitWorktreeState::Active {
                    git_control_path: root.join(".git"),
                    root: root.clone(),
                },
            };
            let shadow = WorktreeRegistration {
                registration_id: i64::try_from(index + 1)?,
                alias: WorktreeAlias::parse(&format!("shadow-{index}"))?,
                state: WorktreeRegistrationState::Active,
                git_common_directory: normalize_native_path_display(&common_directory),
                git_administrative_directory: normalize_native_path_display(
                    &administrative_directory,
                ),
                git_administrative_identity: "ab".repeat(32),
                last_root: normalize_native_path_display(&root),
                project_instance_id: None,
                accepted_telemetry_revision: 0,
                created_at_epoch: 1,
                retired_at_epoch: None,
            };
            let row = server.worktree_list_row(&common_directory, &entry, &[shadow]);
            require(
                row.selector.is_none()
                    && row.alias.is_none()
                    && matches!(row.registration, McpWorktreeRegistrationState::Unregistered)
                    && matches!(row.atlas_state, McpWorktreeAtlasState::Invalid)
                    && row.root.is_none()
                    && row.blocker.as_deref() == Some(MCP_ERROR_WORKTREE_PATH_NON_UTF8),
                "non-UTF-8 structural row was advertised or joined as registrable",
            )?;
            entries.push(entry);
        }

        let first_selector = ProjectAtlasMcpServer::worktree_candidate_selector(&entries[0]);
        require(
            first_selector != ProjectAtlasMcpServer::worktree_candidate_selector(&entries[1]),
            "native administrative identities collapsed into one selector",
        )?;
        let repository = GitRepositoryStructure {
            common_directory: common_directory.clone(),
            selection: projectatlas_fs::worktree::GitRepositorySelection::CommonManager {
                source_selection: projectatlas_fs::worktree::GitManagerSourceSelection::Ambiguous {
                    worktree_count: entries.len(),
                },
            },
            worktrees: entries,
        };
        require(
            server
                .matching_worktree_candidates(&repository, &first_selector)
                .is_empty()
                && repository.worktrees.iter().all(|entry| {
                    ProjectAtlasMcpServer::worktree_candidate(&repository.common_directory, entry)
                        .is_none()
                }),
            "non-UTF-8 structural identity remained selectable for registration",
        )?;
        require(
            store.worktree_registrations(true)?.is_empty(),
            "non-UTF-8 structural identity reached the registration store",
        )?;

        let non_utf8_common = temp
            .path()
            .join(std::ffi::OsString::from_vec(b"common-\xff".to_vec()));
        let utf8_entry = GitWorktreeEntry {
            role: GitWorktreeRole::Linked,
            administrative_directory: common_directory.join("worktrees").join("utf8"),
            state: GitWorktreeState::Active {
                git_control_path: temp.path().join("utf8").join(".git"),
                root: temp.path().join("utf8"),
            },
        };
        let row = server.worktree_list_row(&non_utf8_common, &utf8_entry, &[]);
        require(
            row.selector.is_none()
                && row.root.is_none()
                && row.blocker.as_deref() == Some(MCP_ERROR_WORKTREE_PATH_NON_UTF8)
                && ProjectAtlasMcpServer::worktree_candidate(&non_utf8_common, &utf8_entry)
                    .is_none(),
            "non-UTF-8 common-directory identity remained registrable",
        )
    }

    #[test]
    fn worktree_registration_revalidates_captured_local_atlas_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("worktree");
        fs::create_dir(&root)?;
        let state = root.join(PROJECTATLAS_DIR_NAME);
        let db_path = state.join(PROJECTATLAS_DB_FILE_NAME);
        let config_path = state.join("config.toml");
        init_project_with_config(&root, Some(&config_path))?;
        drop(AtlasStore::open_for_project(&db_path, &root)?);
        let captured = ProjectAtlasMcpServer::local_worktree_atlas(&root)?
            .ok_or_else(|| io::Error::other("captured worktree atlas is missing"))?;
        ProjectAtlasMcpServer::revalidate_local_worktree_atlas_identity(
            &root,
            Some(captured.project_instance_id),
        )?;

        let preserved = root.join(".projectatlas-captured-registration");
        fs::rename(&state, &preserved)?;
        fs::create_dir(&state)?;
        let replacement = AtlasStore::open_for_project(&db_path, &root)?;
        require(
            replacement.project_instance_id()? != Some(captured.project_instance_id),
            "replacement atlas reused the captured registration identity",
        )?;
        drop(replacement);
        let rejected = ProjectAtlasMcpServer::revalidate_local_worktree_atlas_identity(
            &root,
            Some(captured.project_instance_id),
        );
        require(
            rejected.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains(MCP_ERROR_WORKTREE_IDENTITY_CONFLICT)
            }),
            "registration guard accepted a replacement local atlas",
        )?;
        fs::remove_dir_all(&state)?;
        let missing = ProjectAtlasMcpServer::revalidate_local_worktree_atlas_identity(
            &root,
            Some(captured.project_instance_id),
        );
        require(
            missing.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains(MCP_ERROR_WORKTREE_IDENTITY_CONFLICT)
            }),
            "registration guard accepted a missing captured local atlas",
        )?;

        let uninitialized = temp.path().join("uninitialized");
        fs::create_dir(&uninitialized)?;
        ProjectAtlasMcpServer::revalidate_local_worktree_atlas_identity(&uninitialized, None)
            .map_err(Into::into)
    }

    #[test]
    fn registered_init_rejects_replaced_git_lifecycle_before_or_during_activation_and_binding()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = registered_worktree_race_fixture("init-race")?;
        let (candidate, candidate_path, work_control) = prepared_hydration_candidate(&fixture)?;

        let replacement_entry = replace_registered_worktree(&fixture, "replacement-init")?;
        require(
            replacement_entry.administrative_directory == fixture.administrative_directory
                && git_administrative_identity(&replacement_entry.administrative_directory)?
                    != fixture.administrative_identity,
            "replacement fixture did not reuse the administrative path with a new lifecycle",
        )?;
        fs::create_dir_all(
            fixture
                .target_db
                .parent()
                .ok_or_else(|| io::Error::other("replacement database has no parent"))?,
        )?;
        let replacement = AtlasStore::open_for_project(&fixture.target_db, &fixture.state.root)?;
        let replacement_project = replacement
            .project_instance_id()?
            .ok_or(DbError::ProjectInstanceIdentityMissing)?;
        drop(replacement);

        let activation = fixture.server.activate_registered_worktree_hydration(
            &fixture.state,
            &fixture.selection,
            candidate,
            &work_control,
        );
        require(
            activation.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains("administrative lifecycle changed")
            }),
            "hydration candidate activated into a replacement Git lifecycle",
        )?;
        require(
            !candidate_path.exists()
                && open_atlas_store_read_only_for_project(&fixture.target_db, &fixture.state.root)?
                    .project_instance_id()?
                    == Some(replacement_project),
            "rejected hydration changed the replacement atlas or retained its candidate",
        )?;

        let binding = fixture
            .server
            .bind_initialized_worktree(&fixture.selection, &fixture.state);
        require(
            binding.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains("administrative lifecycle changed")
            }),
            "final init binding accepted a replacement Git lifecycle",
        )?;
        let control =
            open_atlas_store_read_only_for_project(&fixture.control_db, &fixture.control_root)?;
        require(
            control
                .worktree_registration(&fixture.alias)?
                .project_instance_id
                .is_none(),
            "failed activation or binding attached the replacement atlas",
        )?;

        let during = registered_worktree_race_fixture("init-publish-race")?;
        let (candidate, _candidate_path, work_control) = prepared_hydration_candidate(&during)?;
        let replacement_project = std::cell::Cell::new(None);
        let activation = during
            .server
            .activate_registered_worktree_hydration_with_post_publication(
                &during.state,
                &during.selection,
                candidate,
                &work_control,
                || {
                    replace_registered_worktree(&during, "replacement-during-publish")
                        .map_err(|error| CliError::InvalidInput(error.to_string()))?;
                    fs::create_dir_all(during.target_db.parent().ok_or_else(|| {
                        CliError::InvalidInput("replacement database has no parent".to_string())
                    })?)
                    .map_err(|source| CliError::Io {
                        path: during.state.root.clone(),
                        source,
                    })?;
                    let replacement =
                        AtlasStore::open_for_project(&during.target_db, &during.state.root)?;
                    replacement_project.set(Some(
                        replacement
                            .project_instance_id()?
                            .ok_or(DbError::ProjectInstanceIdentityMissing)?,
                    ));
                    Ok(())
                },
            );
        let activation_error = activation.as_ref().err().map(ToString::to_string);
        require(
            activation_error
                .as_deref()
                .is_some_and(|error| error.contains("administrative lifecycle changed")),
            &format!(
                "hydration bound a lifecycle replaced after candidate publication: {activation_error:?}"
            ),
        )?;
        let replacement =
            open_atlas_store_read_only_for_project(&during.target_db, &during.state.root)?;
        let control =
            open_atlas_store_read_only_for_project(&during.control_db, &during.control_root)?;
        require(
            replacement.project_instance_id()? == replacement_project.get()
                && control
                    .worktree_registration(&during.alias)?
                    .project_instance_id
                    .is_none(),
            "post-publication lifecycle rejection changed or bound the replacement atlas",
        )
    }

    #[test]
    fn final_retirement_does_not_import_replacement_lifecycle_telemetry()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = registered_worktree_race_fixture("retire-race")?;
        let replacement_entry = replace_registered_worktree(&fixture, "replacement-retire")?;
        fs::create_dir_all(
            fixture
                .target_db
                .parent()
                .ok_or_else(|| io::Error::other("replacement database has no parent"))?,
        )?;
        let replacement = AtlasStore::open_for_project(&fixture.target_db, &fixture.state.root)?;
        replacement.record_usage(&usage_from_text(
            "replacement",
            "atlas_overview",
            None,
            None,
            "pub fn replacement() {}",
            "repository overview",
        ))?;
        let replacement_project = replacement
            .project_instance_id()?
            .ok_or(DbError::ProjectInstanceIdentityMissing)?;
        drop(replacement);
        let control = open_atlas_store_for_project(&fixture.control_db, &fixture.control_root)?;
        let (retired, synchronized, blocker) = ProjectAtlasMcpServer::retire_registered_worktree(
            &control,
            &fixture.registration,
            Some(&fixture.state.root),
            2,
            None,
        )?;
        require(
            retired.state == WorktreeRegistrationState::Retired
                && retired.project_instance_id.is_none()
                && retired.accepted_telemetry_revision == 0
                && synchronized.is_none()
                && blocker.as_deref() == Some(MCP_ERROR_WORKTREE_LIFECYCLE_CHANGED),
            "replacement lifecycle telemetry was bound, imported, or hidden during retirement",
        )?;

        let repository = fixture.server.control_git_repository()?;
        let replacement_alias = WorktreeAlias::parse("replacement-owner")?;
        let replacement_registration = control.register_worktree(
            &replacement_alias,
            &repository.common_directory,
            &replacement_entry.administrative_directory,
            &git_administrative_identity(&replacement_entry.administrative_directory)?,
            &fixture.state.root,
            Some(replacement_project),
            3,
        )?;
        require(
            replacement_registration.project_instance_id == Some(replacement_project),
            "retired origin stranded the replacement atlas identity",
        )?;

        let open_race = registered_worktree_race_fixture("retire-open-race")?;
        let control = open_atlas_store_for_project(&open_race.control_db, &open_race.primary)?;
        let sentinel = b"replacement atlas must remain untouched";
        let (retired, synchronized, blocker) =
            ProjectAtlasMcpServer::retire_registered_worktree_with_pre_open(
                &control,
                &open_race.registration,
                Some(&open_race.state.root),
                2,
                None,
                || {
                    replace_registered_worktree(&open_race, "replacement-before-local-open")
                        .map_err(|error| CliError::InvalidInput(error.to_string()))?;
                    fs::create_dir_all(open_race.target_db.parent().ok_or_else(|| {
                        CliError::InvalidInput("replacement database has no parent".to_string())
                    })?)
                    .map_err(|source| CliError::Io {
                        path: open_race.state.root.clone(),
                        source,
                    })?;
                    fs::write(&open_race.target_db, sentinel).map_err(|source| CliError::Io {
                        path: open_race.target_db.clone(),
                        source,
                    })
                },
            )?;
        require(
            retired.state == WorktreeRegistrationState::Retired
                && retired.project_instance_id.is_none()
                && synchronized.is_none()
                && blocker.as_deref() == Some(MCP_ERROR_WORKTREE_LIFECYCLE_CHANGED)
                && fs::read(&open_race.target_db)? == sentinel,
            "replacement database failure blocked stale retirement or changed replacement bytes",
        )
    }

    #[test]
    fn retirement_reclassifies_identity_and_snapshot_failures_after_lifecycle_replacement()
    -> Result<(), Box<dyn std::error::Error>> {
        for (suffix, failure) in [
            ("retire-identity-failure", "project identity read failed"),
            ("retire-snapshot-failure", "usage snapshot export failed"),
        ] {
            let fixture = registered_worktree_race_fixture(suffix)?;
            let replacement_entry = replace_registered_worktree(&fixture, suffix)?;
            fs::create_dir_all(
                fixture
                    .target_db
                    .parent()
                    .ok_or_else(|| io::Error::other("replacement database has no parent"))?,
            )?;
            let replacement =
                AtlasStore::open_for_project(&fixture.target_db, &fixture.state.root)?;
            replacement.record_usage(&usage_from_text(
                suffix,
                "atlas_token_report",
                None,
                None,
                "pub fn replacement() {}",
                "replacement telemetry",
            ))?;
            let replacement_project = replacement
                .project_instance_id()?
                .ok_or(DbError::ProjectInstanceIdentityMissing)?;
            drop(replacement);

            let control = open_atlas_store_for_project(&fixture.control_db, &fixture.control_root)?;
            let classified = control.with_active_worktree_registration(
                fixture.registration.registration_id,
                &fixture.alias,
                |guard| {
                    ProjectAtlasMcpServer::classify_retirement_failure(
                        guard,
                        &fixture.state.root,
                        2,
                        CliError::InvalidInput(failure.to_string()),
                    )
                },
            )?;
            let (retired, synchronized, blocker) = classified?;
            let replacement =
                open_atlas_store_read_only_for_project(&fixture.target_db, &fixture.state.root)?;
            require(
                retired.state == WorktreeRegistrationState::Retired
                    && retired.project_instance_id.is_none()
                    && synchronized.is_none()
                    && blocker.as_deref() == Some(MCP_ERROR_WORKTREE_LIFECYCLE_CHANGED)
                    && replacement.project_instance_id()? == Some(replacement_project)
                    && replacement.token_overview(Some(suffix))?.calls == 1,
                "retirement failure classification changed or imported replacement state",
            )?;

            let repository = fixture.server.control_git_repository()?;
            let replacement_alias = WorktreeAlias::parse(&format!("{suffix}-owner"))?;
            let replacement_registration = control.register_worktree(
                &replacement_alias,
                &repository.common_directory,
                &replacement_entry.administrative_directory,
                &git_administrative_identity(&replacement_entry.administrative_directory)?,
                &fixture.state.root,
                Some(replacement_project),
                3,
            )?;
            require(
                replacement_registration.project_instance_id == Some(replacement_project),
                "retirement failure classification stranded replacement ownership",
            )?;
        }
        Ok(())
    }

    #[test]
    fn registered_reset_and_binding_linearize_without_recreating_a_reset_winner()
    -> Result<(), Box<dyn std::error::Error>> {
        let bind_wins = registered_worktree_race_fixture("reset-bind-wins")?;
        fs::create_dir_all(
            bind_wins
                .target_db
                .parent()
                .ok_or_else(|| io::Error::other("bind-wins database has no parent"))?,
        )?;
        let target = AtlasStore::open_for_project(&bind_wins.target_db, &bind_wins.state.root)?;
        let project = target
            .project_instance_id()?
            .ok_or(DbError::ProjectInstanceIdentityMissing)?;
        drop(target);
        let before = fs::read(&bind_wins.target_db)?;
        let control = open_atlas_store_for_project(&bind_wins.control_db, &bind_wins.primary)?;
        control.bind_worktree_project(
            bind_wins.registration.registration_id,
            &bind_wins.alias,
            &bind_wins.state.root,
            project,
        )?;
        let rejected = bind_wins.server.reset_registered_worktree_index(
            &bind_wins.state,
            &bind_wins.selection,
            false,
        );
        require(
            rejected.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains(MCP_ERROR_BOUND_WORKTREE_RESET_UNSUPPORTED)
            }) && fs::read(&bind_wins.target_db)? == before,
            "reset deleted a target after a concurrent binding won",
        )?;

        let reset_wins = registered_worktree_race_fixture("reset-delete-wins")?;
        fs::create_dir_all(
            reset_wins
                .target_db
                .parent()
                .ok_or_else(|| io::Error::other("reset-wins database has no parent"))?,
        )?;
        drop(AtlasStore::open_for_project(
            &reset_wins.target_db,
            &reset_wins.state.root,
        )?);
        reset_wins.server.reset_registered_worktree_index(
            &reset_wins.state,
            &reset_wins.selection,
            false,
        )?;
        require(
            !reset_wins.target_db.exists(),
            "winning reset did not delete the unbound target atlas",
        )?;
        let late_bind = ProjectAtlasMcpServer::open_registered_worktree_mut_store(
            &reset_wins.state,
            &reset_wins.server.control_state,
            &reset_wins.selection,
        );
        let control =
            open_atlas_store_read_only_for_project(&reset_wins.control_db, &reset_wins.primary)?;
        require(
            late_bind.is_err()
                && !reset_wins.target_db.exists()
                && control
                    .worktree_registration(&reset_wins.alias)?
                    .project_instance_id
                    .is_none(),
            "late binding recreated or attached the atlas deleted by reset",
        )?;

        let replaced = registered_worktree_race_fixture("reset-replaced-lifecycle")?;
        let replacement_files = [
            replaced.target_db.clone(),
            db_sidecar_path(&replaced.target_db, "wal"),
            db_sidecar_path(&replaced.target_db, "shm"),
            db_sidecar_path(&replaced.target_db, "journal"),
            mcp_config_path_for_db(&replaced.target_db),
        ];
        let replacement_bytes = replacement_files
            .iter()
            .enumerate()
            .map(|(index, _)| format!("replacement-owned-{index}").into_bytes())
            .collect::<Vec<_>>();
        let rejected = replaced
            .server
            .reset_registered_worktree_index_with_post_validation(
                &replaced.state,
                &replaced.selection,
                true,
                || {
                    replace_registered_worktree(&replaced, "replacement-during-reset")
                        .map_err(|error| CliError::InvalidInput(error.to_string()))?;
                    fs::create_dir_all(replaced.target_db.parent().ok_or_else(|| {
                        CliError::InvalidInput("replacement database has no parent".to_string())
                    })?)
                    .map_err(|source| CliError::Io {
                        path: replaced.state.root.clone(),
                        source,
                    })?;
                    for (path, bytes) in replacement_files.iter().zip(&replacement_bytes) {
                        fs::write(path, bytes).map_err(|source| CliError::Io {
                            path: path.clone(),
                            source,
                        })?;
                    }
                    Ok(())
                },
            );
        let control =
            open_atlas_store_read_only_for_project(&replaced.control_db, &replaced.primary)?;
        require(
            rejected.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains("administrative lifecycle changed")
            }) && replacement_files
                .iter()
                .zip(&replacement_bytes)
                .all(|(path, bytes)| {
                    fs::read(path).is_ok_and(|found| found.as_slice() == bytes.as_slice())
                })
                && control
                    .worktree_registration(&replaced.alias)?
                    .project_instance_id
                    .is_none(),
            "guarded reset deleted replacement lifecycle database, sidecars, or MCP config",
        )
    }

    #[test]
    fn registered_worktree_missing_atlas_does_not_refresh_control_root()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = registered_worktree_race_fixture("missing-atlas")?;
        let expected_project = ProjectInstanceId::from_bytes([0xA; 16])?;
        let control = AtlasStore::open_for_project(&fixture.control_db, &fixture.control_root)?;
        control.bind_worktree_project(
            fixture.registration.registration_id,
            &fixture.alias,
            &fixture.state.root,
            expected_project,
        )?;
        drop(control);

        let sidecars = ["-wal", "-shm", "-journal"].map(|suffix| {
            let mut path = fixture.control_db.as_os_str().to_os_string();
            path.push(suffix);
            PathBuf::from(path)
        });
        let before = std::iter::once(&fixture.control_db)
            .chain(sidecars.iter())
            .map(|path| fs::read(path).ok())
            .collect::<Vec<_>>();
        let result = fixture
            .server
            .state_for_target(None, Some(fixture.alias.to_string()));
        require(
            result.is_err(),
            "missing bound atlas was accepted during registered resolution",
        )?;
        let after = std::iter::once(&fixture.control_db)
            .chain(sidecars.iter())
            .map(|path| fs::read(path).ok())
            .collect::<Vec<_>>();
        require(
            before == after,
            "failed atlas validation refreshed the control registration",
        )?;
        Ok(())
    }

    #[test]
    fn unbound_registered_worktree_move_refreshes_root_before_missing_retirement()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = registered_worktree_race_fixture("unbound-move")?;
        let original_root_display = normalize_native_path_display(&fixture.state.root);
        let moved = fixture
            .primary
            .parent()
            .ok_or_else(|| io::Error::other("moved worktree has no parent"))?
            .join("moved-unbound-worktree");
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&fixture.primary)
                .args(["worktree", "move"])
                .arg(&fixture.linked)
                .arg(&moved),
        )?;
        let moved_root = moved.canonicalize()?;
        let unresolved = fixture
            .server
            .state_for_target(None, Some(fixture.alias.to_string()));
        require(
            unresolved.is_err(),
            "unbound moved worktree without an atlas was accepted as initialized",
        )?;

        let control = AtlasStore::open_for_project(&fixture.control_db, &fixture.control_root)?;
        let refreshed = control.worktree_registration(&fixture.alias)?;
        require(
            refreshed.project_instance_id.is_none()
                && refreshed.last_root == normalize_native_path_display(&moved_root),
            "unbound Git move did not refresh its registered root",
        )?;
        drop(control);

        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&fixture.primary)
                .args(["worktree", "remove", "--force"])
                .arg(&moved),
        )?;
        let missing = fixture
            .server
            .atlas_worktree_list(Parameters(AtlasWorktreeListParams {
                include_retired: Some(false),
            }));
        require(
            missing.contains(&normalize_native_path_display(&moved_root))
                && !missing.contains(&original_root_display)
                && missing.contains("\"unbound-move\",linked,missing,registered"),
            &format!("missing worktree reporting retained the pre-move root: {missing}"),
        )?;
        let retired = fixture
            .server
            .atlas_worktree_remove(Parameters(AtlasWorktreeRemoveParams {
                worktree: fixture.alias.to_string(),
            }));
        require(
            retired.contains("status: retired")
                && retired.contains(&normalize_native_path_display(&moved_root))
                && !retired.contains(&original_root_display),
            &format!("retirement reporting retained the pre-move root: {retired}"),
        )
    }

    #[test]
    fn registered_worktree_move_refreshes_registry_root_after_local_rebind()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = registered_worktree_race_fixture("moved-root")?;
        let original_root = fixture.state.root.clone();
        let original_root_display = normalize_native_path_display(&original_root);
        let target_config = fixture
            .linked
            .join(PROJECTATLAS_DIR_NAME)
            .join(PROJECTATLAS_CONFIG_FILE_NAME);
        init_project_with_config(&fixture.linked, Some(&target_config))?;
        let target_store = AtlasStore::open_for_project(&fixture.target_db, &fixture.linked)?;
        let target_project = target_store
            .project_instance_id()?
            .ok_or(DbError::ProjectInstanceIdentityMissing)?;
        drop(target_store);

        let registered = {
            let control = AtlasStore::open_for_project(&fixture.control_db, &fixture.control_root)?;
            control.register_worktree(
                &fixture.alias,
                &fixture.control_root.join(".git"),
                &fixture.administrative_directory,
                &fixture.registration.git_administrative_identity,
                &original_root,
                Some(target_project),
                1,
            )?
        };
        require(
            registered.project_instance_id == Some(target_project),
            "moved worktree fixture remained unbound",
        )?;
        let original_registration = registered;

        let moved = fixture
            .primary
            .parent()
            .ok_or_else(|| io::Error::other("moved worktree has no parent"))?
            .join("moved-worktree");
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&fixture.primary)
                .args(["worktree", "move"])
                .arg(&fixture.linked)
                .arg(&moved),
        )?;
        let moved_root = moved.canonicalize()?;
        let moved_db = moved_root
            .join(PROJECTATLAS_DIR_NAME)
            .join(PROJECTATLAS_DB_FILE_NAME);
        let moved_binding = AtlasStore::transition_project_root(
            &moved_db,
            &moved_root,
            ProjectRootTransition::Move,
        )?;
        require(
            moved_binding.project_instance_id == target_project,
            "local root rebind changed the registered project identity",
        )?;
        require(
            read_project_root_identity_read_only(&moved_db)?
                == Some(CanonicalProjectRoot::from_path(&moved_root)?),
            "local root rebind did not establish the moved native identity",
        )?;

        let resolved = fixture
            .server
            .state_for_target(None, Some(fixture.alias.to_string()))?;
        require(
            resolved.root == moved_root,
            "alias resolution did not use the moved Git root",
        )?;
        let control = AtlasStore::open_for_project(&fixture.control_db, &fixture.control_root)?;
        let refreshed = control.worktree_registration(&fixture.alias)?;
        require(
            refreshed.last_root == normalize_native_path_display(&moved_root)
                && refreshed.last_root != original_root_display,
            "alias resolution retained the stale registry root",
        )?;
        require(
            refreshed.registration_id == original_registration.registration_id
                && refreshed.alias == original_registration.alias
                && refreshed.git_common_directory == original_registration.git_common_directory
                && refreshed.git_administrative_directory
                    == original_registration.git_administrative_directory
                && refreshed.git_administrative_identity
                    == original_registration.git_administrative_identity
                && refreshed.project_instance_id == original_registration.project_instance_id
                && refreshed.accepted_telemetry_revision
                    == original_registration.accepted_telemetry_revision,
            "moved root refresh changed administrative identity or telemetry state",
        )?;
        drop(control);

        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&fixture.primary)
                .args(["worktree", "remove", "--force"])
                .arg(&moved),
        )?;
        let missing = fixture
            .server
            .atlas_worktree_list(Parameters(AtlasWorktreeListParams {
                include_retired: Some(false),
            }));
        require(
            missing.contains(&normalize_native_path_display(&moved_root))
                && !missing.contains(&original_root_display)
                && missing.contains("\"moved-root\",linked,missing,registered"),
            &format!("missing worktree reporting regressed to the pre-move root: {missing}"),
        )?;
        let retired = fixture
            .server
            .atlas_worktree_remove(Parameters(AtlasWorktreeRemoveParams {
                worktree: fixture.alias.to_string(),
            }));
        require(
            retired.contains("status: retired")
                && retired.contains(&normalize_native_path_display(&moved_root))
                && !retired.contains(&original_root_display),
            &format!("retirement did not preserve the moved root: {retired}"),
        )
    }

    #[test]
    fn worktree_tools_register_route_and_retire_without_git_or_file_lifecycle_mutation()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let primary = temp.path().join("selected control");
        let worktree_a = temp.path().join("unrelated one").join("checkout");
        let worktree_b = temp.path().join("unrelated two").join("checkout");
        fs::create_dir_all(&primary)?;
        fs::create_dir_all(
            worktree_a
                .parent()
                .ok_or_else(|| io::Error::other("worktree A has no parent"))?,
        )?;
        fs::create_dir_all(
            worktree_b
                .parent()
                .ok_or_else(|| io::Error::other("worktree B has no parent"))?,
        )?;
        run_fixture_command(StdCommand::new("git").current_dir(&primary).arg("init"))?;
        for (key, value) in [
            ("user.name", "ProjectAtlas Test"),
            ("user.email", "projectatlas@example.invalid"),
            ("commit.gpgsign", "false"),
            ("core.autocrlf", "false"),
        ] {
            run_fixture_command(
                StdCommand::new("git")
                    .current_dir(&primary)
                    .args(["config", key, value]),
            )?;
        }
        fs::create_dir(primary.join("src"))?;
        fs::write(
            primary.join("src").join("lib.rs"),
            "mod child;\npub fn main_only() { child::helper(); }\n",
        )?;
        fs::write(primary.join("src").join("child.rs"), "pub fn helper() {}\n")?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["add", "."]),
        )?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["commit", "-m", "fixture"]),
        )?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["worktree", "add", "-b", "issue-430-a"])
                .arg(&worktree_a),
        )?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["worktree", "add", "-b", "issue-430-b"])
                .arg(&worktree_b),
        )?;
        fs::write(
            worktree_a.join("src").join("branch.rs"),
            "pub fn worktree_only() {}\n",
        )?;
        let git_before = run_fixture_command(StdCommand::new("git").current_dir(&primary).args([
            "worktree",
            "list",
            "--porcelain",
        ]))?;

        let control_db = primary.join(".projectatlas").join("projectatlas.db");
        let control_config = primary.join(".projectatlas").join("config.toml");
        init_project_with_config(&primary, Some(&control_config))?;
        let mut control_store = AtlasStore::open_for_project(&control_db, &primary)?;
        let control_plan = ScanRuntimePlan::for_path(Some(&control_config), &primary, None)?;
        run_scan_pipeline(
            &mut control_store,
            &control_plan,
            &SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None),
        )?;
        control_store.set_purpose(
            "src/lib.rs",
            "Own the shared library contract.",
            PurposeSource::Agent,
        )?;
        drop(control_store);
        let server = ProjectAtlasMcpServer::new(
            control_db.clone(),
            Some(control_config),
            "worktree-tools".to_string(),
            false,
        );
        let control_before_blank_selector = fs::read(&control_db)?;
        let blank_selector = server.atlas_reset_index(Parameters(AtlasResetIndexParams {
            project_path: None,
            worktree: Some("   ".to_string()),
            apply: Some(true),
            dry_run: Some(false),
            include_mcp_config: Some(true),
        }));
        require(
            blank_selector.contains(MCP_ERROR_WORKTREE_SELECTOR_EMPTY),
            "blank worktree selector fell back to the control atlas",
        )?;
        require(
            fs::read(&control_db)? == control_before_blank_selector,
            "blank worktree selector changed the control database",
        )?;
        let repository = server.control_git_repository()?;
        let canonical_a = worktree_a.canonicalize()?;
        let entry_a = repository
            .worktrees
            .iter()
            .find(|entry| {
                ProjectAtlasMcpServer::active_worktree_root(entry) == Some(canonical_a.as_path())
            })
            .ok_or_else(|| io::Error::other("worktree A was not structurally discovered"))?;
        let selector_a = ProjectAtlasMcpServer::worktree_candidate_selector(entry_a);
        let canonical_b = worktree_b.canonicalize()?;
        let entry_b = repository
            .worktrees
            .iter()
            .find(|entry| {
                ProjectAtlasMcpServer::active_worktree_root(entry) == Some(canonical_b.as_path())
            })
            .ok_or_else(|| io::Error::other("worktree B was not structurally discovered"))?;
        let selector_b = ProjectAtlasMcpServer::worktree_candidate_selector(entry_b);

        let listed = server.atlas_worktree_list(Parameters(AtlasWorktreeListParams {
            include_retired: Some(false),
        }));
        require(
            listed.contains("control_alias: main")
                && listed.contains(&selector_a)
                && listed.contains(&normalize_native_path_display(&canonical_a)),
            "worktree list omitted control, stable selector, or arbitrary exact root",
        )?;
        let ambiguous = server.atlas_worktree_add(Parameters(AtlasWorktreeAddParams {
            worktree: "checkout".to_string(),
            alias: Some("issue-430".to_string()),
        }));
        require(
            ambiguous.contains("status: ambiguous")
                && ambiguous.matches(MCP_WORKTREE_SELECTOR_PREFIX).count() >= 2,
            "ambiguous human selector guessed or omitted bounded stable candidates",
        )?;
        let control_before_blank_alias = fs::read(&control_db)?;
        let blank_alias = server.atlas_worktree_add(Parameters(AtlasWorktreeAddParams {
            worktree: selector_a.clone(),
            alias: Some("   ".to_string()),
        }));
        require(
            blank_alias.contains("alias is empty"),
            "blank explicit alias fell back to the selected directory name",
        )?;
        require(
            fs::read(&control_db)? == control_before_blank_alias,
            "blank explicit alias changed the control database",
        )?;

        let target_b_config = worktree_b.join(PROJECTATLAS_DIR_NAME).join("config.toml");
        init_project_with_config(&worktree_b, Some(&target_b_config))?;
        let target_b_db = worktree_b
            .join(PROJECTATLAS_DIR_NAME)
            .join(PROJECTATLAS_DB_FILE_NAME);
        let target_b_store = AtlasStore::open_for_project(&target_b_db, &worktree_b)?;
        target_b_store.record_usage(&usage_from_text(
            "snapshot-blocked",
            "atlas_overview",
            None,
            None,
            "pub fn main_only() {}",
            "repository overview",
        ))?;
        let target_b_project = target_b_store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("worktree B project identity is missing"))?;
        drop(target_b_store);
        let corrupt = rusqlite::Connection::open(&target_b_db)?;
        corrupt.execute_batch("PRAGMA ignore_check_constraints = ON;")?;
        let corrupted = corrupt.execute("UPDATE usage_global_aggregates SET calls = -1", [])?;
        require(
            corrupted > 0,
            "worktree B fixture did not invalidate an aggregate row",
        )?;
        drop(corrupt);
        let rejected_snapshot = server.atlas_worktree_add(Parameters(AtlasWorktreeAddParams {
            worktree: selector_b.clone(),
            alias: Some("snapshot-blocked".to_string()),
        }));
        require(
            rejected_snapshot.contains("telemetry integer overflow")
                && !rejected_snapshot.contains("status: registered"),
            &format!(
                "worktree registration survived a failed local usage snapshot: {rejected_snapshot}"
            ),
        )?;
        let control_after_rejection =
            open_atlas_store_read_only_for_project(&control_db, &primary)?;
        require(
            matches!(
                control_after_rejection
                    .worktree_registration(&WorktreeAlias::parse("snapshot-blocked")?),
                Err(DbError::WorktreeRegistrationNotFound { .. })
            ),
            "telemetry export failure committed a partial worktree registration",
        )?;
        drop(control_after_rejection);
        let repaired = rusqlite::Connection::open(&target_b_db)?;
        repaired.execute("UPDATE usage_global_aggregates SET calls = 1", [])?;
        drop(repaired);
        let registered_snapshot = server.atlas_worktree_add(Parameters(AtlasWorktreeAddParams {
            worktree: selector_b.clone(),
            alias: Some("snapshot-blocked".to_string()),
        }));
        require(
            registered_snapshot.contains("status: registered"),
            &format!(
                "repaired local usage snapshot did not register atomically: {registered_snapshot}"
            ),
        )?;
        let control_after_snapshot = open_atlas_store_read_only_for_project(&control_db, &primary)?;
        require(
            control_after_snapshot
                .worktree_registration(&WorktreeAlias::parse("snapshot-blocked")?)?
                .project_instance_id
                == Some(target_b_project)
                && control_after_snapshot
                    .registered_worktree_token_overview(&WorktreeAlias::parse("snapshot-blocked")?)?
                    .calls
                    == 1,
            "successful registration did not bind identity and import telemetry atomically",
        )?;
        drop(control_after_snapshot);
        let target_b_state = worktree_b.join(PROJECTATLAS_DIR_NAME);
        let preserved_target_b_state = worktree_b.join(".projectatlas-snapshot-blocked");
        fs::rename(&target_b_state, &preserved_target_b_state)?;
        fs::create_dir(&target_b_state)?;
        let replacement_b = AtlasStore::open_for_project(&target_b_db, &worktree_b)?;
        require(
            replacement_b.project_instance_id()? != Some(target_b_project),
            "worktree B replacement reused the registered project identity",
        )?;
        drop(replacement_b);
        let replacement_b_error =
            server.state_for_target(None, Some("snapshot-blocked".to_string()));
        require(
            replacement_b_error.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains(MCP_ERROR_WORKTREE_IDENTITY_CONFLICT)
            }),
            "snapshot-backed registration routed to a replacement atlas",
        )?;
        fs::remove_dir_all(&target_b_state)?;
        let refused_snapshot =
            server.atlas_worktree_remove(Parameters(AtlasWorktreeRemoveParams {
                worktree: "snapshot-blocked".to_string(),
            }));
        require(
            refused_snapshot.contains(MCP_ERROR_BOUND_WORKTREE_ATLAS_MISSING),
            &format!(
                "bound registration retired without its required final snapshot: {refused_snapshot}"
            ),
        )?;
        let control_after_refusal = open_atlas_store_read_only_for_project(&control_db, &primary)?;
        require(
            control_after_refusal
                .worktree_registration(&WorktreeAlias::parse("snapshot-blocked")?)?
                .state
                == WorktreeRegistrationState::Active,
            "missing bound atlas retirement changed the active registration",
        )?;
        drop(control_after_refusal);
        fs::rename(&preserved_target_b_state, &target_b_state)?;
        let retired_snapshot =
            server.atlas_worktree_remove(Parameters(AtlasWorktreeRemoveParams {
                worktree: "snapshot-blocked".to_string(),
            }));
        require(
            retired_snapshot.contains("status: retired"),
            &format!(
                "restored bound atlas did not permit final-sync retirement: {retired_snapshot}"
            ),
        )?;
        fs::remove_dir_all(&target_b_state)?;

        let legacy_added = server.atlas_worktree_add(Parameters(AtlasWorktreeAddParams {
            worktree: selector_b.clone(),
            alias: Some("legacy-init".to_string()),
        }));
        require(
            legacy_added.contains("status: registered"),
            &format!("legacy-init fixture registration failed: {legacy_added}"),
        )?;
        init_project_with_config(&worktree_b, Some(&target_b_config))?;
        let control_before_legacy_sync =
            open_atlas_store_read_only_for_project(&control_db, &primary)?;
        require(
            control_before_legacy_sync
                .worktree_registration(&WorktreeAlias::parse("legacy-init")?)?
                .project_instance_id
                .is_none(),
            "independent exact-path init unexpectedly mutated the control registration",
        )?;
        drop(control_before_legacy_sync);
        let legacy_target = AtlasStore::open_for_project(&target_b_db, &worktree_b)?;
        legacy_target.record_usage(&usage_from_text(
            "legacy-init",
            "atlas_overview",
            None,
            None,
            "pub fn independent() {}",
            "repository overview",
        ))?;
        let legacy_project = legacy_target
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("legacy-init target identity is missing"))?;
        drop(legacy_target);
        let legacy_state = server.state_for_target(None, Some("legacy-init".to_string()))?;
        let legacy_selection = legacy_state
            .worktree
            .as_ref()
            .ok_or_else(|| io::Error::other("legacy-init selection is missing"))?;
        let corrupt = rusqlite::Connection::open(&target_b_db)?;
        corrupt.execute_batch("PRAGMA ignore_check_constraints = ON;")?;
        corrupt.execute("UPDATE usage_global_aggregates SET calls = -1", [])?;
        drop(corrupt);
        let rejected_bind = ProjectAtlasMcpServer::open_registered_worktree_mut_store(
            &legacy_state,
            &server.control_state,
            legacy_selection,
        );
        let control_after_rejected_bind =
            open_atlas_store_read_only_for_project(&control_db, &primary)?;
        require(
            rejected_bind
                .as_ref()
                .is_err_and(|error| error.to_string().contains("telemetry integer overflow"))
                && control_after_rejected_bind
                    .worktree_registration(&WorktreeAlias::parse("legacy-init")?)?
                    .project_instance_id
                    .is_none()
                && control_after_rejected_bind
                    .registered_worktree_token_overview(&WorktreeAlias::parse("legacy-init")?)?
                    .calls
                    == 0,
            "failed deferred snapshot synchronization committed its project binding or aggregate",
        )?;
        drop(control_after_rejected_bind);
        let repaired = rusqlite::Connection::open(&target_b_db)?;
        repaired.execute("UPDATE usage_global_aggregates SET calls = 1", [])?;
        drop(repaired);
        drop(ProjectAtlasMcpServer::open_registered_worktree_mut_store(
            &legacy_state,
            &server.control_state,
            legacy_selection,
        )?);
        let control_after_legacy_init =
            open_atlas_store_read_only_for_project(&control_db, &primary)?;
        require(
            control_after_legacy_init
                .worktree_registration(&WorktreeAlias::parse("legacy-init")?)?
                .project_instance_id
                == Some(legacy_project),
            "first alias mutation did not bind the independently initialized alias",
        )?;
        require(
            control_after_legacy_init
                .registered_worktree_token_overview(&WorktreeAlias::parse("legacy-init")?)?
                .calls
                == 1,
            "first alias mutation omitted independently initialized worktree usage",
        )?;
        drop(control_after_legacy_init);
        let preserved_legacy_state = worktree_b.join(".projectatlas-legacy-init");
        fs::rename(&target_b_state, &preserved_legacy_state)?;
        fs::create_dir(&target_b_state)?;
        let legacy_replacement = AtlasStore::open_for_project(&target_b_db, &worktree_b)?;
        require(
            legacy_replacement.project_instance_id()? != Some(legacy_project),
            "legacy-init replacement reused the registered project identity",
        )?;
        drop(legacy_replacement);
        let legacy_replacement_error =
            server.state_for_target(None, Some("legacy-init".to_string()));
        require(
            legacy_replacement_error.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains(MCP_ERROR_WORKTREE_IDENTITY_CONFLICT)
            }),
            "legacy exact-path init left its alias unbound to a replacement atlas",
        )?;
        fs::remove_dir_all(&target_b_state)?;
        let refused_legacy = server.atlas_worktree_remove(Parameters(AtlasWorktreeRemoveParams {
            worktree: "legacy-init".to_string(),
        }));
        require(
            refused_legacy.contains(MCP_ERROR_BOUND_WORKTREE_ATLAS_MISSING),
            &format!("legacy-init registration retired without its atlas: {refused_legacy}"),
        )?;
        fs::rename(&preserved_legacy_state, &target_b_state)?;
        let retired_legacy = server.atlas_worktree_remove(Parameters(AtlasWorktreeRemoveParams {
            worktree: "legacy-init".to_string(),
        }));
        require(
            retired_legacy.contains("status: retired"),
            &format!("restored legacy-init registration could not be retired: {retired_legacy}"),
        )?;
        fs::remove_dir_all(&target_b_state)?;

        init_project_with_config(&worktree_b, Some(&target_b_config))?;
        let migratable_store = AtlasStore::open_for_project(&target_b_db, &worktree_b)?;
        let migratable_project = migratable_store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("migratable target identity is missing"))?;
        drop(migratable_store);
        let predecessor = rusqlite::Connection::open(&target_b_db)?;
        let current_limits = GraphLimitKind::ALL
            .iter()
            .map(|limit| format!("'{}'", limit.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        let current_coverage_schema: String = predecessor.query_row(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'graph_coverage'",
            [],
            |row| row.get(0),
        )?;
        let predecessor_coverage_schema = current_coverage_schema.replace(
            &current_limits,
            "'rows', 'occurrences', 'depth', 'output_bytes'",
        );
        require(
            predecessor_coverage_schema != current_coverage_schema,
            "predecessor fixture did not replace the current graph limit domain",
        )?;
        // This isolated fixture needs the exact released schema-17 declaration; rebuilding the
        // table here would duplicate the complete production graph DDL in the CLI crate.
        predecessor.execute_batch("PRAGMA writable_schema = ON;")?;
        predecessor.execute(
            "UPDATE sqlite_schema SET sql = ?1 WHERE type = 'table' AND name = 'graph_coverage'",
            [predecessor_coverage_schema],
        )?;
        predecessor.execute_batch("PRAGMA writable_schema = OFF;")?;
        predecessor.execute_batch(
            "DROP TABLE usage_instance_worktree_origins;
             DROP TABLE worktree_usage_aggregates;
            DROP TABLE worktree_registrations;
            DROP TABLE usage_aggregate_revisions;
            DROP TABLE project_root_identity;
            UPDATE metadata SET value = '17' WHERE key = 'schema_version';",
        )?;
        drop(predecessor);
        let migratable_added = server.atlas_worktree_add(Parameters(AtlasWorktreeAddParams {
            worktree: selector_b,
            alias: Some("migratable-atlas".to_string()),
        }));
        require(
            migratable_added.contains("status: registered")
                && migratable_added
                    .contains("registration committed without local telemetry import"),
            &format!(
                "supported predecessor atlas did not retain its explicit unbound registration: {migratable_added}"
            ),
        )?;
        let control_before_migration =
            open_atlas_store_read_only_for_project(&control_db, &primary)?;
        require(
            control_before_migration
                .worktree_registration(&WorktreeAlias::parse("migratable-atlas")?)?
                .project_instance_id
                .is_none(),
            "supported predecessor fixture unexpectedly bound before migration",
        )?;
        drop(control_before_migration);
        let migrated = server.atlas_scan(Parameters(AtlasScanParams {
            project_path: None,
            worktree: Some("migratable-atlas".to_string()),
            path: None,
            nearest_project: Some(false),
            max_bytes: None,
            max_workers: Some(1),
            timeout_seconds: None,
            text_index_max_bytes: None,
            background: Some(false),
        }));
        require(
            migrated.contains("scan:"),
            &format!("alias-routed predecessor migration did not complete its scan: {migrated}"),
        )?;
        let migrated_target = open_atlas_store_read_only_for_project(&target_b_db, &worktree_b)?;
        require(
            migrated_target.project_instance_id()? == Some(migratable_project),
            "supported predecessor migration changed the worktree project identity",
        )?;
        drop(migrated_target);
        let control_after_migration =
            open_atlas_store_read_only_for_project(&control_db, &primary)?;
        require(
            control_after_migration
                .worktree_registration(&WorktreeAlias::parse("migratable-atlas")?)?
                .project_instance_id
                == Some(migratable_project),
            "alias-routed predecessor migration left its registration unbound",
        )?;
        drop(control_after_migration);
        let retired_migratable =
            server.atlas_worktree_remove(Parameters(AtlasWorktreeRemoveParams {
                worktree: "migratable-atlas".to_string(),
            }));
        require(
            retired_migratable.contains("status: retired"),
            &format!("migrated registration could not be retired: {retired_migratable}"),
        )?;

        let added = server.atlas_worktree_add(Parameters(AtlasWorktreeAddParams {
            worktree: selector_a,
            alias: Some("issue-430".to_string()),
        }));
        require(
            added.contains("status: registered")
                && added.contains("alias: \"issue-430\"")
                && added.contains("git_unchanged: true")
                && added.contains("files_unchanged: true"),
            &format!("stable selector did not create one lifecycle-neutral registration: {added}"),
        )?;
        require(
            !worktree_a.join(".projectatlas").exists(),
            "registration created a target atlas before explicit init",
        )?;

        let missing = server.atlas_overview_response(
            AtlasProjectParams {
                project_path: None,
                worktree: Some("issue-430".to_string()),
            },
            None,
        );
        require(
            missing.contains("init_required")
                && missing.contains("tool: atlas_init")
                && missing.contains("worktree: \"issue-430\"")
                && !missing.contains("project_path:"),
            "missing alias target did not preserve its short selector in typed init guidance",
        )?;
        let conflict = server.atlas_overview_response(
            AtlasProjectParams {
                project_path: Some(primary.to_string_lossy().to_string()),
                worktree: Some("issue-430".to_string()),
            },
            None,
        );
        require(
            conflict.contains(MCP_WORKTREE_PROJECT_PATH_CONFLICT),
            "worktree/project_path conflict was not rejected by the shared resolver",
        )?;

        let initialized = server.atlas_init(Parameters(AtlasInitParams {
            project_path: None,
            worktree: Some("issue-430".to_string()),
            no_scan: Some(false),
            force_rescan: Some(false),
            text_index_max_bytes: None,
        }));
        require(
            initialized.contains("status: hydrated")
                && initialized.contains("source_project_instance_id:")
                && initialized.contains("target_project_instance_id:")
                && initialized.contains("reconciled_generation:")
                && initialized.contains("parsed: 1"),
            &format!("registered alias init did not expose one completed hydration: {initialized}"),
        )?;
        let target_db = worktree_a.join(".projectatlas").join("projectatlas.db");
        let target_store = open_atlas_store_read_only_for_project(&target_db, &worktree_a)?;
        require(
            target_store.load_node_by_path("src/branch.rs")?.is_some(),
            "hydration reconciliation omitted a target-only dirty source file",
        )?;
        require(
            target_store
                .load_node_by_path("src/lib.rs")?
                .is_some_and(|node| {
                    node.purpose.purpose.as_deref() == Some("Own the shared library contract.")
                        && node.purpose.source == PurposeSource::Agent
                }),
            "hydration did not preserve an applicable approved main purpose",
        )?;
        let target_project = target_store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("hydrated target identity is missing"))?;
        drop(target_store);
        let captured_alias_state = server.state_for_target(None, Some("issue-430".to_string()))?;
        let captured_federated_aliases = ["main".to_string(), "issue-430".to_string()];
        let (captured_federated_roots, captured_federated_selections) =
            server.federated_worktree_roots(&captured_federated_aliases)?;
        let captured_main_state = server.state_for_target(None, Some("main".to_string()))?;
        let control_after_init = open_atlas_store_read_only_for_project(&control_db, &primary)?;
        let control_project = control_after_init
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("control project identity is missing"))?;
        require(
            captured_main_state
                .worktree
                .as_ref()
                .and_then(|selection| selection.project_instance_id)
                == Some(control_project),
            "main alias did not capture the current control atlas identity",
        )?;
        require(
            [&captured_main_state, &captured_alias_state]
                .iter()
                .all(|state| {
                    state
                        .worktree
                        .as_ref()
                        .and_then(|selection| selection.control_project_instance_id)
                        == Some(control_project)
                }),
            "alias selection did not capture its control atlas identity",
        )?;
        require(
            control_after_init
                .load_node_by_path("src/branch.rs")?
                .is_none(),
            "target-only source state bled into the control graph",
        )?;
        require(
            control_after_init
                .worktree_registration(&WorktreeAlias::parse("issue-430")?)?
                .project_instance_id
                == Some(target_project),
            "successful alias init did not bind the exact target atlas identity",
        )?;
        drop(control_after_init);
        let target_before_reset = fs::read(&target_db)?;
        let bound_reset = server.atlas_reset_index(Parameters(AtlasResetIndexParams {
            project_path: None,
            worktree: Some("issue-430".to_string()),
            apply: Some(true),
            dry_run: Some(false),
            include_mcp_config: Some(true),
        }));
        require(
            bound_reset.contains(MCP_ERROR_BOUND_WORKTREE_RESET_UNSUPPORTED),
            "applied reset did not reject a bound worktree alias",
        )?;
        require(
            fs::read(&target_db)? == target_before_reset,
            "rejected bound worktree reset changed its database",
        )?;
        let control_state = primary.join(PROJECTATLAS_DIR_NAME);
        let preserved_control_state = primary.join(".projectatlas-captured-main");
        fs::rename(&control_state, &preserved_control_state)?;
        fs::create_dir(&control_state)?;
        init_project_with_config(&primary, captured_main_state.config_path.as_deref())?;
        let mut replacement_control = AtlasStore::open_for_project(&control_db, &primary)?;
        let replacement_control_plan =
            ScanRuntimePlan::for_path(captured_main_state.config_path.as_deref(), &primary, None)?;
        run_scan_pipeline(
            &mut replacement_control,
            &replacement_control_plan,
            &SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None),
        )?;
        require(
            replacement_control.project_instance_id()? != Some(control_project),
            "replacement control atlas reused the captured main identity",
        )?;
        let captured_registration_id = captured_alias_state
            .worktree
            .as_ref()
            .and_then(|selection| selection.registration_id)
            .ok_or_else(|| io::Error::other("captured alias registration identity is missing"))?;
        let common = primary.join(".git");
        let mut replacement_alias = None;
        for index in 1..=captured_registration_id {
            let alias = WorktreeAlias::parse(&format!("replacement-{index}"))?;
            let registration = replacement_control.register_worktree(
                &alias,
                &common,
                &common
                    .join("worktrees")
                    .join(format!("replacement-{index}")),
                &format!("{index:064x}"),
                &temp.path().join(format!("replacement-worktree-{index}")),
                Some(ProjectInstanceId::from_bytes([u8::try_from(index)?; 16])?),
                u64::try_from(index)?,
            )?;
            require(
                registration.registration_id == index,
                "replacement control did not reuse the expected registration identity",
            )?;
            if index == captured_registration_id {
                replacement_alias = Some(alias);
            }
        }
        let replacement_alias = replacement_alias
            .ok_or_else(|| io::Error::other("replacement alias was not created"))?;
        let replacement_calls_before = replacement_control
            .registered_worktree_token_overview(&replacement_alias)?
            .calls;
        require(
            ProjectAtlasMcpServer::require_captured_control_identity(
                captured_alias_state.worktree.as_ref(),
                &replacement_control,
            )
            .as_ref()
            .is_err_and(|error| {
                error
                    .to_string()
                    .contains(MCP_ERROR_WORKTREE_CONTROL_IDENTITY_CONFLICT)
            }),
            "captured alias accepted a replacement control atlas",
        )?;
        drop(replacement_control);
        let accepted = server.with_fresh_string_and_usage_for_request(
            &captured_alias_state,
            None,
            |_store, _stamp| {
                Ok((
                    "accepted result".to_string(),
                    Some(McpUsageIntent::estimate(
                        MCP_EVENT_ATLAS_OVERVIEW,
                        None,
                        None,
                        1,
                    )),
                ))
            },
        )?;
        require(
            accepted == "accepted result",
            "accepted target read was lost",
        )?;
        let replacement_control = open_atlas_store_read_only_for_project(&control_db, &primary)?;
        require(
            replacement_control
                .registered_worktree_token_overview(&replacement_alias)?
                .calls
                == replacement_calls_before,
            "deferred telemetry was attributed through a replacement control catalog",
        )?;
        drop(replacement_control);
        require(
            matches!(
                synchronize_registered_worktree_usage(
                    &captured_main_state.db_path,
                    &captured_main_state.root,
                    Some(control_project),
                ),
                Err(CliError::InvalidInput(message))
                    if message.contains("control atlas identity changed")
            ),
            "main token synchronization did not revalidate its captured project identity",
        )?;
        let captured_main_direct_read =
            ProjectAtlasMcpServer::open_read_store(&captured_main_state);
        require(
            captured_main_direct_read.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains(MCP_ERROR_WORKTREE_IDENTITY_CONFLICT)
            }),
            "main direct token read did not revalidate its captured project identity",
        )?;
        let captured_main_read =
            server.with_fresh_store(&captured_main_state, |_store, _stamp| Ok(()));
        require(
            captured_main_read.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains(MCP_ERROR_WORKTREE_IDENTITY_CONFLICT)
            }),
            "main read snapshot did not revalidate its captured project identity",
        )?;
        let captured_main_write = ProjectAtlasMcpServer::open_existing_mut_store(
            &captured_main_state,
            &server.control_state,
        );
        require(
            captured_main_write.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains(MCP_ERROR_WORKTREE_IDENTITY_CONFLICT)
            }),
            "main mutation did not revalidate its captured project identity",
        )?;
        let captured_federated_labels = captured_federated_selections
            .iter()
            .map(|selection| selection.alias.clone())
            .collect::<Vec<_>>();
        let federated_control =
            index_work_control(&SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None));
        let captured_federated_stores = open_federated_atlas_stores_for_project(
            &captured_main_state.db_path,
            &captured_main_state.root,
            captured_main_state.config_path.as_deref(),
            &captured_federated_roots,
            Some(&captured_federated_labels),
            &federated_control,
        )?;
        let captured_main_federation = ProjectAtlasMcpServer::require_federated_worktree_identities(
            captured_federated_stores,
            &captured_federated_selections,
        );
        require(
            captured_main_federation.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains(MCP_ERROR_WORKTREE_IDENTITY_CONFLICT)
                    && error.to_string().contains(MCP_MAIN_WORKTREE_ALIAS)
            }),
            "federation did not revalidate the captured main identity",
        )?;
        fs::remove_dir_all(&control_state)?;
        fs::rename(&preserved_control_state, &control_state)?;
        let target_state = worktree_a.join(PROJECTATLAS_DIR_NAME);
        let preserved_target_state = worktree_a.join(".projectatlas-registered");
        fs::rename(&target_state, &preserved_target_state)?;
        fs::create_dir(&target_state)?;
        init_project_with_config(&worktree_a, captured_alias_state.config_path.as_deref())?;
        let mut replacement_store = AtlasStore::open_for_project(&target_db, &worktree_a)?;
        let replacement_plan = ScanRuntimePlan::for_path(
            captured_alias_state.config_path.as_deref(),
            &worktree_a,
            None,
        )?;
        run_scan_pipeline(
            &mut replacement_store,
            &replacement_plan,
            &SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None),
        )?;
        let replacement_project = replacement_store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("replacement target identity is missing"))?;
        require(
            replacement_project != target_project,
            "replacement atlas reused the registered project identity",
        )?;
        drop(replacement_store);
        let captured_read = server.with_fresh_store(&captured_alias_state, |_store, _stamp| Ok(()));
        require(
            captured_read.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains(MCP_ERROR_WORKTREE_IDENTITY_CONFLICT)
            }),
            "alias read snapshot did not revalidate its captured project identity",
        )?;
        let captured_federated_labels = captured_federated_selections
            .iter()
            .map(|selection| selection.alias.clone())
            .collect::<Vec<_>>();
        let federated_control =
            index_work_control(&SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None));
        let captured_federated_stores = open_federated_atlas_stores_for_project(
            &captured_main_state.db_path,
            &captured_main_state.root,
            captured_main_state.config_path.as_deref(),
            &captured_federated_roots,
            Some(&captured_federated_labels),
            &federated_control,
        )?;
        let captured_federation = ProjectAtlasMcpServer::require_federated_worktree_identities(
            captured_federated_stores,
            &captured_federated_selections,
        );
        require(
            captured_federation.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains(MCP_ERROR_WORKTREE_IDENTITY_CONFLICT)
                    && error.to_string().contains("issue-430")
            }),
            "federated snapshots did not retain their captured alias identities",
        )?;
        let replacement_error = server.state_for_target(None, Some("issue-430".to_string()));
        require(
            replacement_error.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains(MCP_ERROR_WORKTREE_IDENTITY_CONFLICT)
            }),
            "alias routing accepted a replacement atlas at the registered root",
        )?;
        fs::remove_dir_all(&target_state)?;
        fs::rename(&preserved_target_state, &target_state)?;
        let selected = server.state_for_target(None, Some("issue-430".to_string()))?;
        require(
            selected.root == canonical_a
                && selected
                    .worktree
                    .as_ref()
                    .is_some_and(|selection| selection.alias == "issue-430"),
            "short alias did not capture the exact target root and identity",
        )?;
        let selected_diagnostics = ProjectAtlasMcpServer::render_project_state(&selected)?;
        require(
            selected_diagnostics.contains("worktree: \"issue-430\"")
                && selected_diagnostics.contains("registration_id:"),
            &format!(
                "selected-project diagnostics omitted the captured alias or registration identity: {selected_diagnostics}"
            ),
        )?;
        let settings = server.atlas_settings(Parameters(AtlasProjectParams {
            project_path: None,
            worktree: Some("issue-430".to_string()),
        }));
        require(
            settings.contains("worktree: \"issue-430\"") && settings.contains("registration_id:"),
            &format!(
                "alias-routed settings omitted the captured alias or registration identity: {settings}"
            ),
        )?;
        let root_report = server.atlas_root(Parameters(AtlasRootParams {
            project_path: None,
            worktree: Some("issue-430".to_string()),
            verify: Some(false),
            control_root: None,
        }));
        require(
            root_report.contains("worktree: \"issue-430\"")
                && root_report.contains("registration_id:"),
            &format!(
                "alias-routed root diagnostics omitted the captured alias or registration identity: {root_report}"
            ),
        )?;
        let routed_overview = server.atlas_overview_response(
            AtlasProjectParams {
                project_path: None,
                worktree: Some("issue-430".to_string()),
            },
            None,
        );
        require(
            routed_overview.contains("overview:") && routed_overview.contains("files:"),
            &format!("alias-routed overview failed: {routed_overview}"),
        )?;
        let alias = WorktreeAlias::parse("issue-430")?;
        let control_after_routed = open_atlas_store_read_only_for_project(&control_db, &primary)?;
        require(
            control_after_routed
                .registered_worktree_token_overview(&alias)?
                .calls
                == 1
                && control_after_routed.repository_token_overview()?.calls == 3,
            "alias-routed MCP usage or retained independently initialized usage was miscounted",
        )?;
        drop(control_after_routed);
        let local_event = usage_from_text(
            "worktree-local",
            "atlas_summary",
            Some("src/lib.rs".to_string()),
            None,
            "pub fn main_only() { child::helper(); }",
            "Own the shared library contract.",
        );
        let local_target = AtlasStore::open_for_project(&target_db, &worktree_a)?;
        local_target.record_usage(&local_event)?;
        require(
            local_target.token_overview(None)?.calls == 1,
            "independent worktree usage was not retained in its exact local atlas",
        )?;
        drop(local_target);
        let repository_tokens = server.atlas_token_report(Parameters(AtlasTokenParams {
            project_path: None,
            worktree: Some("main".to_string()),
            session: None,
            include_chart: Some(false),
            trend_window: None,
            benchmark_results: None,
            theme: None,
        }));
        require(
            repository_tokens.contains("worktree: main") && repository_tokens.contains("calls: 4"),
            &format!(
                "control token report did not combine routed and synchronized worktree usage: {repository_tokens}"
            ),
        )?;
        let worktree_tokens = server.atlas_token_report(Parameters(AtlasTokenParams {
            project_path: None,
            worktree: Some("issue-430".to_string()),
            session: None,
            include_chart: Some(false),
            trend_window: None,
            benchmark_results: None,
            theme: None,
        }));
        require(
            worktree_tokens.contains("worktree: \"issue-430\"")
                && worktree_tokens.contains("calls: 1"),
            &format!(
                "exact worktree token report included routed or sibling usage: {worktree_tokens}"
            ),
        )?;
        let federated = server.atlas_symbol_relations_response(
            &AtlasSymbolRelationsParams {
                file: Some("src/lib.rs".to_string()),
                view: Some(MCP_SYMBOL_RELATION_VIEW_DETAILED.to_string()),
                compact: Some(true),
                worktrees: Some(vec!["main".to_string(), "issue-430".to_string()]),
                limit: Some(1),
                ..AtlasSymbolRelationsParams::default()
            },
            None,
        );
        require(
            federated.contains("primary_worktree: main")
                && federated.contains("participants[2]{order,worktree")
                && federated.contains("0,main,")
                && federated.contains("1,\"issue-430\","),
            &format!(
                "alias federation did not label the primary and every participant: {federated}"
            ),
        )?;
        let federation_conflict = server.atlas_symbol_relations_response(
            &AtlasSymbolRelationsParams {
                file: Some("src/lib.rs".to_string()),
                view: Some(MCP_SYMBOL_RELATION_VIEW_DETAILED.to_string()),
                roots: Some(vec![
                    normalize_native_path_display(&primary),
                    normalize_native_path_display(&worktree_a),
                ]),
                worktrees: Some(vec!["main".to_string(), "issue-430".to_string()]),
                ..AtlasSymbolRelationsParams::default()
            },
            None,
        );
        require(
            federation_conflict.contains(MCP_ERROR_FEDERATED_SELECTOR_CONFLICT),
            "alias federation did not reject legacy roots before opening participants",
        )?;
        let target_database_before_blocker = fs::read(&target_db)?;
        fs::write(
            worktree_a.join("src").join("branch.rs"),
            "pub fn worktree_only_changed() {}\n",
        )?;
        let blocked_federation = server.atlas_symbol_relations_response(
            &AtlasSymbolRelationsParams {
                file: Some("src/lib.rs".to_string()),
                view: Some(MCP_SYMBOL_RELATION_VIEW_DETAILED.to_string()),
                worktrees: Some(vec!["main".to_string(), "issue-430".to_string()]),
                ..AtlasSymbolRelationsParams::default()
            },
            None,
        );
        require(
            blocked_federation.contains("refresh_required")
                && blocked_federation.contains("worktree: \"issue-430\""),
            &format!(
                "stale federated participant blocker omitted its exact worktree alias: {blocked_federation}"
            ),
        )?;
        require(
            fs::read(&target_db)? == target_database_before_blocker,
            "read-only alias federation repaired or changed a sibling database",
        )?;
        let main = server.state_for_target(None, Some("main".to_string()))?;
        require(
            main.root == primary.canonicalize()? && selected.root != main.root,
            "interleaved main and worktree selections bled into one target",
        )?;
        let source_before = fs::read(worktree_a.join("src").join("lib.rs"))?;
        let removed = server.atlas_worktree_remove(Parameters(AtlasWorktreeRemoveParams {
            worktree: "issue-430".to_string(),
        }));
        require(
            removed.contains("status: retired")
                && removed.contains("git_unchanged: true")
                && removed.contains("files_unchanged: true"),
            "unregister did not retire the alias with lifecycle-neutral status",
        )?;
        require(
            target_db.is_file()
                && worktree_a.join(".git").is_file()
                && fs::read(worktree_a.join("src").join("lib.rs"))? == source_before,
            "unregister deleted or modified target-owned Git, source, or atlas state",
        )?;
        let git_after = run_fixture_command(StdCommand::new("git").current_dir(&primary).args([
            "worktree",
            "list",
            "--porcelain",
        ]))?;
        require(
            git_after == git_before,
            "ProjectAtlas worktree operations changed Git lifecycle state",
        )?;
        let control = open_atlas_store_read_only_for_project(&control_db, &primary)?;
        let registrations = control.worktree_registrations(true)?;
        require(
            registrations.iter().any(|registration| {
                registration.alias.as_str() == "issue-430"
                    && registration.state == WorktreeRegistrationState::Retired
            }),
            "retired registration and retained telemetry identity were not durable",
        )?;
        require(
            server
                .state_for_target(None, Some("issue-430".to_string()))
                .is_err(),
            "retired alias remained selectable for source operations",
        )
    }

    #[test]
    fn uninitialized_alias_rejects_a_reused_git_administrative_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let primary = temp.path().join("control");
        let original = temp.path().join("first location").join("checkout");
        let replacement = temp.path().join("second location").join("checkout");
        fs::create_dir_all(&primary)?;
        fs::create_dir_all(
            original
                .parent()
                .ok_or_else(|| io::Error::other("original worktree has no parent"))?,
        )?;
        fs::create_dir_all(
            replacement
                .parent()
                .ok_or_else(|| io::Error::other("replacement worktree has no parent"))?,
        )?;
        run_fixture_command(StdCommand::new("git").current_dir(&primary).arg("init"))?;
        for (key, value) in [
            ("user.name", "ProjectAtlas Test"),
            ("user.email", "projectatlas@example.invalid"),
            ("commit.gpgsign", "false"),
        ] {
            run_fixture_command(
                StdCommand::new("git")
                    .current_dir(&primary)
                    .args(["config", key, value]),
            )?;
        }
        fs::write(primary.join("lib.rs"), "pub fn control() {}\n")?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["add", "."]),
        )?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["commit", "-m", "fixture"]),
        )?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["worktree", "add", "-b", "original"])
                .arg(&original),
        )?;

        let control_db = primary.join(".projectatlas").join("projectatlas.db");
        let control_config = primary.join(".projectatlas").join("config.toml");
        init_project_with_config(&primary, Some(&control_config))?;
        drop(AtlasStore::open_for_project(&control_db, &primary)?);
        let server = ProjectAtlasMcpServer::new(
            control_db,
            Some(control_config),
            "worktree-lifecycle".to_string(),
            false,
        );
        let repository = server.control_git_repository()?;
        let original_root = original.canonicalize()?;
        let original_entry = repository
            .worktrees
            .iter()
            .find(|entry| {
                ProjectAtlasMcpServer::active_worktree_root(entry) == Some(original_root.as_path())
            })
            .ok_or_else(|| io::Error::other("original worktree was not discovered"))?;
        let administrative_directory = original_entry.administrative_directory.clone();
        let administrative_identity = git_administrative_identity(&administrative_directory)?;
        let added = server.atlas_worktree_add(Parameters(AtlasWorktreeAddParams {
            worktree: ProjectAtlasMcpServer::worktree_candidate_selector(original_entry),
            alias: Some("replacement-check".to_string()),
        }));
        require(
            added.contains("status: registered"),
            &format!("original uninitialized worktree was not registered: {added}"),
        )?;

        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["worktree", "remove", "--force"])
                .arg(&original),
        )?;
        let missing_registration =
            server.atlas_worktree_list(Parameters(AtlasWorktreeListParams {
                include_retired: Some(false),
            }));
        require(
            missing_registration.contains("\"replacement-check\",linked,missing,registered")
                && missing_registration.contains(&normalize_native_path_display(&original))
                && missing_registration.contains(",unavailable,unavailable,0,")
                && missing_registration.contains(MCP_WORKTREE_MISSING_RETENTION_REASON),
            &format!(
                "active alias disappeared after external Git worktree removal: {missing_registration}"
            ),
        )?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["worktree", "add", "-b", "replacement"])
                .arg(&replacement),
        )?;
        let replacement_repository = server.control_git_repository()?;
        let replacement_root = replacement.canonicalize()?;
        let replacement_entry = replacement_repository
            .worktrees
            .iter()
            .find(|entry| {
                ProjectAtlasMcpServer::active_worktree_root(entry)
                    == Some(replacement_root.as_path())
            })
            .ok_or_else(|| io::Error::other("replacement worktree was not discovered"))?;
        require(
            replacement_entry.administrative_directory == administrative_directory,
            "Git fixture did not reuse the administrative path",
        )?;
        require(
            git_administrative_identity(&replacement_entry.administrative_directory)?
                != administrative_identity,
            "replacement Git worktree reused the prior lifecycle identity",
        )?;
        let add_revalidation = server.revalidate_worktree_candidate(
            &repository,
            original_entry,
            &administrative_identity,
        );
        require(
            add_revalidation.as_ref().is_err_and(|error| {
                error
                    .to_string()
                    .contains(MCP_ERROR_WORKTREE_LIFECYCLE_CHANGED)
            }),
            "add revalidation combined the old root with a replacement lifecycle",
        )?;

        let resolution_error = server
            .state_for_target(None, Some("replacement-check".to_string()))
            .err()
            .ok_or_else(|| io::Error::other("reused administrative path was accepted"))?;
        require(
            resolution_error
                .to_string()
                .contains(MCP_ERROR_WORKTREE_LIFECYCLE_CHANGED),
            "reused administrative path did not return the lifecycle blocker",
        )?;
        require(
            !replacement.join(".projectatlas").exists(),
            "failed lifecycle validation initialized the replacement worktree",
        )?;
        let before_remove =
            run_fixture_command(StdCommand::new("git").current_dir(&primary).args([
                "worktree",
                "list",
                "--porcelain",
            ]))?;
        let removed = server.atlas_worktree_remove(Parameters(AtlasWorktreeRemoveParams {
            worktree: "replacement-check".to_string(),
        }));
        require(
            removed.contains("status: retired")
                && removed.contains(MCP_ERROR_WORKTREE_LIFECYCLE_CHANGED),
            &format!("stale registration could not be retired safely: {removed}"),
        )?;
        let after_remove =
            run_fixture_command(StdCommand::new("git").current_dir(&primary).args([
                "worktree",
                "list",
                "--porcelain",
            ]))?;
        require(
            after_remove == before_remove,
            "retiring a stale registration changed Git lifecycle state",
        )
    }

    #[test]
    fn reused_git_administrative_path_cannot_synchronize_replacement_telemetry()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let primary = temp.path().join("control");
        let worktree = temp.path().join("checkout");
        fs::create_dir_all(&primary)?;
        run_fixture_command(StdCommand::new("git").current_dir(&primary).arg("init"))?;
        for (key, value) in [
            ("user.name", "ProjectAtlas Test"),
            ("user.email", "projectatlas@example.invalid"),
            ("commit.gpgsign", "false"),
        ] {
            run_fixture_command(
                StdCommand::new("git")
                    .current_dir(&primary)
                    .args(["config", key, value]),
            )?;
        }
        fs::write(primary.join("lib.rs"), "pub fn control() {}\n")?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["add", "."]),
        )?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["commit", "-m", "fixture"]),
        )?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["worktree", "add", "-b", "original-telemetry"])
                .arg(&worktree),
        )?;

        let control_db = primary.join(".projectatlas").join("projectatlas.db");
        let target_db = worktree.join(".projectatlas").join("projectatlas.db");
        fs::create_dir_all(
            control_db
                .parent()
                .ok_or_else(|| io::Error::other("control database has no parent"))?,
        )?;
        fs::create_dir_all(
            target_db
                .parent()
                .ok_or_else(|| io::Error::other("target database has no parent"))?,
        )?;
        let event = usage_from_text(
            "worktree-lifecycle",
            "atlas_overview",
            None,
            None,
            "pub fn source() {}",
            "repository overview",
        );
        let target = AtlasStore::open_for_project(&target_db, &worktree)?;
        target.record_usage(&event)?;
        let target_project = target
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("target project identity is missing"))?;
        drop(target);

        let RepositoryStructure::Git(repository) = discover_repository_structure(&primary)? else {
            return Err(io::Error::other("Git repository was not discovered").into());
        };
        let canonical_worktree = worktree.canonicalize()?;
        let entry = repository
            .worktrees
            .iter()
            .find(|entry| {
                ProjectAtlasMcpServer::active_worktree_root(entry)
                    == Some(canonical_worktree.as_path())
            })
            .ok_or_else(|| io::Error::other("worktree was not discovered"))?;
        let administrative_directory = entry.administrative_directory.clone();
        let administrative_identity = git_administrative_identity(&administrative_directory)?;
        let alias = WorktreeAlias::parse("lifecycle-telemetry")?;
        let control = AtlasStore::open_for_project(&control_db, &primary)?;
        control.register_worktree(
            &alias,
            &repository.common_directory,
            &administrative_directory,
            &administrative_identity,
            &canonical_worktree,
            Some(target_project),
            1,
        )?;
        drop(control);
        synchronize_registered_worktree_usage(&control_db, &primary, None)?;
        let control = AtlasStore::open_for_project(&control_db, &primary)?;
        require(
            control.registered_worktree_token_overview(&alias)?.calls == 1,
            "initial worktree telemetry was not synchronized",
        )?;
        drop(control);

        let saved_db = temp.path().join("saved-projectatlas.db");
        fs::copy(&target_db, &saved_db)?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["worktree", "remove", "--force"])
                .arg(&worktree),
        )?;
        require(
            matches!(
                synchronize_registered_worktree_usage(&control_db, &primary, None),
                Err(CliError::InvalidInput(message))
                    if message.contains("aggregate token totals cannot be synchronized")
            ),
            "externally removed bound worktree reported stale aggregate success",
        )?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["worktree", "add", "-b", "replacement-telemetry"])
                .arg(&worktree),
        )?;
        fs::create_dir_all(
            target_db
                .parent()
                .ok_or_else(|| io::Error::other("target database has no parent"))?,
        )?;
        fs::copy(&saved_db, &target_db)?;
        let replacement = AtlasStore::open_for_project(&target_db, &worktree)?;
        replacement.record_usage(&event)?;
        drop(replacement);

        let RepositoryStructure::Git(replacement_repository) =
            discover_repository_structure(&primary)?
        else {
            return Err(io::Error::other("replacement Git repository was not discovered").into());
        };
        let replacement_entry = replacement_repository
            .worktrees
            .iter()
            .find(|entry| {
                ProjectAtlasMcpServer::active_worktree_root(entry)
                    == Some(canonical_worktree.as_path())
            })
            .ok_or_else(|| io::Error::other("replacement worktree was not discovered"))?;
        require(
            replacement_entry.administrative_directory == administrative_directory,
            "Git fixture did not reuse the administrative path",
        )?;
        require(
            git_administrative_identity(&replacement_entry.administrative_directory)?
                != administrative_identity,
            "replacement Git worktree reused the prior lifecycle identity",
        )?;

        require(
            matches!(
                synchronize_registered_worktree_usage(&control_db, &primary, None),
                Err(CliError::InvalidInput(message))
                    if message.contains("aggregate token totals cannot be synchronized")
            ),
            "replacement lifecycle reported stale aggregate success",
        )?;
        let control = open_atlas_store_read_only_for_project(&control_db, &primary)?;
        require(
            control.registered_worktree_token_overview(&alias)?.calls == 1,
            "replacement lifecycle telemetry was imported into the retired origin",
        )
    }

    #[test]
    fn aggregate_synchronization_propagates_local_atlas_and_identity_failures()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let primary = temp.path().join("control");
        let worktree = temp.path().join("external").join("worktree");
        fs::create_dir_all(&primary)?;
        fs::create_dir_all(
            worktree
                .parent()
                .ok_or_else(|| io::Error::other("worktree has no parent"))?,
        )?;
        run_fixture_command(StdCommand::new("git").current_dir(&primary).arg("init"))?;
        for (key, value) in [
            ("user.name", "ProjectAtlas Test"),
            ("user.email", "projectatlas@example.invalid"),
            ("commit.gpgsign", "false"),
        ] {
            run_fixture_command(
                StdCommand::new("git")
                    .current_dir(&primary)
                    .args(["config", key, value]),
            )?;
        }
        fs::write(primary.join("README.md"), "# fixture\n")?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["add", "."]),
        )?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["commit", "-m", "fixture"]),
        )?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["worktree", "add", "-b", "sync-failure"])
                .arg(&worktree),
        )?;

        let control_db = primary.join(".projectatlas").join("projectatlas.db");
        let target_db = worktree.join(".projectatlas").join("projectatlas.db");
        fs::create_dir_all(
            control_db
                .parent()
                .ok_or_else(|| io::Error::other("control database has no parent"))?,
        )?;
        fs::create_dir_all(
            target_db
                .parent()
                .ok_or_else(|| io::Error::other("target database has no parent"))?,
        )?;
        let target = AtlasStore::open_for_project(&target_db, &worktree)?;
        target.record_usage(&usage_from_text(
            "worktree-sync",
            "atlas_overview",
            None,
            None,
            "pub fn source() {}",
            "repository overview",
        ))?;
        let target_project = target
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("target project identity is missing"))?;
        drop(target);

        let RepositoryStructure::Git(repository) = discover_repository_structure(&primary)? else {
            return Err(io::Error::other("Git repository was not discovered").into());
        };
        let canonical_worktree = worktree.canonicalize()?;
        let entry = repository
            .worktrees
            .iter()
            .find(|entry| {
                ProjectAtlasMcpServer::active_worktree_root(entry)
                    == Some(canonical_worktree.as_path())
            })
            .ok_or_else(|| io::Error::other("worktree was not discovered"))?;
        let alias = WorktreeAlias::parse("sync-failure")?;
        let foreign_project = if target_project == ProjectInstanceId::from_bytes([0x7f; 16])? {
            ProjectInstanceId::from_bytes([0x7e; 16])?
        } else {
            ProjectInstanceId::from_bytes([0x7f; 16])?
        };
        let control = AtlasStore::open_for_project(&control_db, &primary)?;
        control.register_worktree(
            &alias,
            &repository.common_directory,
            &entry.administrative_directory,
            &git_administrative_identity(&entry.administrative_directory)?,
            &canonical_worktree,
            Some(foreign_project),
            1,
        )?;
        drop(control);

        require(
            matches!(
                synchronize_registered_worktree_usage(&control_db, &primary, None),
                Err(CliError::Db(
                    DbError::WorktreeTelemetryProjectMismatch { .. }
                ))
            ),
            "aggregate synchronization hid the project identity failure behind stale success",
        )?;

        let git_directory = primary.join(".git");
        let unavailable_git_directory = temp.path().join("unavailable-control-git");
        fs::rename(&git_directory, &unavailable_git_directory)?;
        require(
            matches!(
                synchronize_registered_worktree_usage(&control_db, &primary, None),
                Err(CliError::InvalidInput(message))
                    if message.contains("requires Git control evidence")
            ),
            "aggregate synchronization treated missing control Git evidence as success",
        )?;
        fs::rename(&unavailable_git_directory, &git_directory)?;

        let control_head = git_directory.join("HEAD");
        let valid_control_head = temp.path().join("valid-control-head");
        fs::rename(&control_head, &valid_control_head)?;
        fs::create_dir(&control_head)?;
        require(
            matches!(
                synchronize_registered_worktree_usage(&control_db, &primary, None),
                Err(CliError::InvalidInput(message))
                    if message.contains("invalid Git evidence")
            ),
            "aggregate synchronization treated invalid control Git evidence as success",
        )?;
        fs::remove_dir(&control_head)?;
        fs::rename(&valid_control_head, &control_head)?;

        let corrupt = rusqlite::Connection::open(&target_db)?;
        corrupt.pragma_update(None, "ignore_check_constraints", true)?;
        corrupt.execute("UPDATE usage_aggregate_revisions SET revision = -1", [])?;
        drop(corrupt);
        require(
            matches!(
                synchronize_registered_worktree_usage(&control_db, &primary, None),
                Err(CliError::Db(DbError::TelemetryIntegerOverflow {
                    field: "usage_aggregate_revisions.revision"
                }))
            ),
            "aggregate synchronization hid the local snapshot export failure",
        )?;

        fs::remove_file(&target_db)?;
        require(
            matches!(
                synchronize_registered_worktree_usage(&control_db, &primary, None),
                Err(CliError::InvalidInput(message))
                    if message.contains("aggregate token totals cannot be synchronized")
            ),
            "aggregate synchronization reported stale success for a bound missing atlas",
        )?;
        fs::create_dir(&target_db)?;
        require(
            synchronize_registered_worktree_usage(&control_db, &primary, None).is_err(),
            "aggregate synchronization hid the existing local atlas open failure",
        )
    }

    #[test]
    fn registered_worktree_init_falls_back_from_incomplete_control_and_preserves_existing_atlas()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let primary = temp.path().join("control");
        let linked = temp.path().join("external").join("linked");
        fs::create_dir_all(&primary)?;
        fs::create_dir_all(
            linked
                .parent()
                .ok_or_else(|| io::Error::other("linked worktree has no parent"))?,
        )?;
        run_fixture_command(StdCommand::new("git").current_dir(&primary).arg("init"))?;
        for (key, value) in [
            ("user.name", "ProjectAtlas Test"),
            ("user.email", "projectatlas@example.invalid"),
            ("commit.gpgsign", "false"),
        ] {
            run_fixture_command(
                StdCommand::new("git")
                    .current_dir(&primary)
                    .args(["config", key, value]),
            )?;
        }
        fs::write(primary.join("lib.rs"), "pub fn control() {}\n")?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["add", "."]),
        )?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["commit", "-m", "fixture"]),
        )?;
        run_fixture_command(
            StdCommand::new("git")
                .current_dir(&primary)
                .args(["worktree", "add", "-b", "fallback"])
                .arg(&linked),
        )?;
        let git_before = run_fixture_command(StdCommand::new("git").current_dir(&primary).args([
            "worktree",
            "list",
            "--porcelain",
        ]))?;

        let control_db = primary.join(".projectatlas").join("projectatlas.db");
        fs::create_dir_all(
            control_db
                .parent()
                .ok_or_else(|| io::Error::other("control DB has no parent"))?,
        )?;
        drop(AtlasStore::open_for_project(&control_db, &primary)?);
        let server = ProjectAtlasMcpServer::new(
            control_db.clone(),
            None,
            "worktree-fallback".to_string(),
            false,
        );
        let repository = server.control_git_repository()?;
        let canonical_linked = linked.canonicalize()?;
        let entry = repository
            .worktrees
            .iter()
            .find(|entry| {
                ProjectAtlasMcpServer::active_worktree_root(entry)
                    == Some(canonical_linked.as_path())
            })
            .ok_or_else(|| io::Error::other("linked worktree was not discovered"))?;
        let added = server.atlas_worktree_add(Parameters(AtlasWorktreeAddParams {
            worktree: ProjectAtlasMcpServer::worktree_candidate_selector(entry),
            alias: Some("fallback".to_string()),
        }));
        require(
            added.contains("status: registered"),
            "fallback fixture registration failed",
        )?;

        let initialized = server.atlas_init(Parameters(AtlasInitParams {
            project_path: None,
            worktree: Some("fallback".to_string()),
            no_scan: Some(false),
            force_rescan: Some(false),
            text_index_max_bytes: None,
        }));
        require(
            initialized.contains("status: fallback")
                && initialized.contains("fallback_reason:")
                && initialized.contains("repository graph is unavailable"),
            &format!(
                "incomplete control atlas did not produce visible ordinary fallback: {initialized}"
            ),
        )?;
        let target_db = linked.join(".projectatlas").join("projectatlas.db");
        let target = open_atlas_store_read_only_for_project(&target_db, &linked)?;
        let identity = target
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("fallback target identity is missing"))?;
        require(
            target.index_publication()?.is_some_and(|publication| {
                publication.state == projectatlas_db::IndexPublicationState::Complete
            }),
            "ordinary fallback did not publish a complete target index",
        )?;
        drop(target);

        let repeated = server.atlas_init(Parameters(AtlasInitParams {
            project_path: None,
            worktree: Some("fallback".to_string()),
            no_scan: Some(true),
            force_rescan: Some(false),
            text_index_max_bytes: None,
        }));
        require(
            repeated.contains("status: existing"),
            &format!("repeat init did not preserve the valid target atlas: {repeated}"),
        )?;
        let preserved = open_atlas_store_read_only_for_project(&target_db, &linked)?;
        require(
            preserved.project_instance_id()? == Some(identity),
            "repeat init replaced the valid target atlas identity",
        )?;
        let control = open_atlas_store_read_only_for_project(&control_db, &primary)?;
        require(
            control
                .worktree_registration(&WorktreeAlias::parse("fallback")?)?
                .project_instance_id
                == Some(identity),
            "fallback init did not bind the exact target identity",
        )?;
        let git_after = run_fixture_command(StdCommand::new("git").current_dir(&primary).args([
            "worktree",
            "list",
            "--porcelain",
        ]))?;
        require(
            git_after == git_before,
            "fallback or repeat init changed Git lifecycle state",
        )?;

        let config_path = init_config_path(&linked, None);
        let config_before = fs::read(&config_path)?;
        drop(preserved);
        drop(control);
        fs::remove_file(&target_db)?;
        let refused = server.atlas_init(Parameters(AtlasInitParams {
            project_path: None,
            worktree: Some("fallback".to_string()),
            no_scan: Some(false),
            force_rescan: Some(false),
            text_index_max_bytes: None,
        }));
        require(
            refused.contains(MCP_ERROR_BOUND_WORKTREE_ATLAS_MISSING),
            &format!("bound missing atlas did not fail before initialization: {refused}"),
        )?;
        require(
            !target_db.exists() && fs::read(&config_path)? == config_before,
            "bound missing atlas refusal changed target ProjectAtlas state",
        )
    }

    #[test]
    fn bare_startup_and_root_set_preserve_worktree_required_without_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let bare = temp.path().join("repository.git");
        let output = StdCommand::new("git")
            .args(["init", "--bare"])
            .arg(&bare)
            .output()?;
        require(output.status.success(), "git init --bare failed")?;
        let db_path = bare.join(".projectatlas").join("projectatlas.db");
        let server =
            ProjectAtlasMcpServer::new(db_path, None, "mcp-bare-root-test".to_string(), false);

        let Err(error) = server.active_project_state() else {
            return Err(io::Error::other("bare MCP startup state was exposed as active").into());
        };
        if !matches!(error, CliError::WorktreeRequired(_)) {
            return Err(io::Error::other(format!(
                "bare MCP startup did not preserve typed worktree_required state: {error:?}"
            ))
            .into());
        }

        let response = server.atlas_root_set(Parameters(AtlasRootSetParams {
            root: bare.to_string_lossy().to_string(),
            transition: None,
            nearest_project: None,
        }));
        require(
            response.contains("worktree_required"),
            "atlas_root_set did not reject a bare Git control root",
        )?;
        require(
            !bare.join(".projectatlas").exists(),
            "bare MCP startup or root-set refusal created project state",
        )
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
            ProjectAtlasMcpServer::new(db_b.clone(), Some(config_a), "mcp-test".to_string(), false);
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

        let Err(error) = ProjectAtlasMcpServer::open_read_store(&state) else {
            return Err(io::Error::other("missing index opened unexpectedly").into());
        };
        require(
            matches!(error, CliError::InitRequired(_)),
            "missing index did not return typed init_required state",
        )?;
        let payload = ProjectAtlasMcpServer::encode_error_payload(&error);
        require(
            payload.contains("kind: init_required")
                && payload.contains("init_required:")
                && payload.contains("tool: atlas_init")
                && payload.contains(&normalize_native_path_display(&repo)),
            "missing index payload did not contain the exact atlas_init recovery call",
        )?;
        require(
            !repo.join(".projectatlas").exists(),
            "read-only store created .projectatlas",
        )?;

        Ok(())
    }

    #[test]
    fn atlas_init_explicit_project_path_bootstraps_without_switching_active_project()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let repo_a = temp.path().join("repo-a");
        let repo_b = temp.path().join("repo-b");
        fs::create_dir(&repo_a)?;
        fs::create_dir(&repo_b)?;
        let db_a = repo_a.join(".projectatlas").join("projectatlas.db");
        let server = ProjectAtlasMcpServer::new(db_a, None, "mcp-test".to_string(), false);
        let active_before = server.active_project_state()?;

        let text = server.atlas_init(Parameters(AtlasInitParams {
            project_path: Some(repo_b.to_string_lossy().to_string()),
            worktree: None,
            no_scan: Some(true),
            force_rescan: Some(false),
            text_index_max_bytes: None,
        }));

        let expected_b = normalize_native_path_display(canonical_project_root(&repo_b)?);
        require(
            text.contains("init:"),
            "atlas_init did not return named init payload",
        )?;
        require(
            text.contains(&expected_b),
            "atlas_init did not report the explicit project path",
        )?;
        require(
            text.contains("status: skipped"),
            "atlas_init --no-scan did not report skipped scan",
        )?;
        require(
            text.contains("purpose_handoff:")
                && text.contains("execution_owner: agent_host")
                && text.contains(&format!(
                    "recommended_subagent_reasoning: {PURPOSE_CURATOR_RECOMMENDED_REASONING}"
                ))
                && text.contains("main_agent_fallback: true")
                && text.contains("server_started_curator: false")
                && text.contains("silent_on_success: true")
                && text.contains("curation_scope: low"),
            "atlas_init did not expose the host-owned low-scope curator handoff",
        )?;
        require(
            repo_b
                .join(".projectatlas")
                .join("projectatlas.db")
                .is_file(),
            "atlas_init did not create the explicit project's DB",
        )?;
        require(
            repo_b
                .join(".projectatlas")
                .join("projectatlas.mcp.json")
                .is_file()
                && repo_b
                    .join(".projectatlas")
                    .join("projectatlas.claude.mcp.json")
                    .is_file()
                && repo_b
                    .join(".projectatlas")
                    .join("projectatlas.opencode.json")
                    .is_file(),
            "atlas_init did not generate host MCP configs",
        )?;

        let active_after = server.active_project_state()?;
        require(
            active_after.root == active_before.root,
            "atlas_init with explicit project_path changed the active default root",
        )?;
        require(
            !repo_a.join(".projectatlas").exists(),
            "explicit atlas_init mutated the active project",
        )?;

        Ok(())
    }

    #[test]
    fn session_brief_missing_index_stays_read_only() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo-a");
        fs::create_dir(&repo)?;
        let db_path = repo.join(".projectatlas").join("projectatlas.db");
        let server = ProjectAtlasMcpServer::new(db_path, None, "mcp-test".to_string(), false);

        let brief = server.build_session_brief(
            AtlasSessionBriefParams {
                project_path: None,
                worktree: None,
                query: Some("startup".to_string()),
                purpose_task: None,
                compact: None,
                folder_limit: None,
                file_limit: None,
                blocker_limit: None,
                purpose_limit: None,
            },
            None,
        )?;

        require(
            brief.project.index_status == McpIndexStatus::Missing,
            "missing index was not represented as typed state",
        )?;
        require(
            brief.overview.is_none(),
            "missing-index brief unexpectedly included overview",
        )?;
        require(
            brief.recommendations.iter().any(|recommendation| {
                matches!(recommendation.kind, McpBriefRecommendationKind::Init)
                    && recommendation.target == MCP_TOOL_ATLAS_INIT
                    && recommendation.arguments.get(MCP_BRIEF_ARG_PROJECT_PATH)
                        == Some(&serde_json::json!(normalize_native_path_display(&repo)))
            }),
            "missing-index brief did not recommend atlas_init for the exact selected root",
        )?;
        require(
            !repo.join(".projectatlas").exists(),
            "session brief created .projectatlas for a missing index",
        )?;

        Ok(())
    }

    #[test]
    fn session_brief_recommendations_preserve_per_call_project_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let project_path = "F:/example/repo-b".to_string();
        let recommendations = ProjectAtlasMcpServer::indexed_project_recommendations(
            "startup",
            Some(NavigationNextCall {
                capability: NavigationNextCapability::Summary,
                path: "src/lib.rs".to_string(),
            }),
            1,
            7,
            Some(project_path.clone()),
            None,
        );

        require(
            recommendations.iter().all(|recommendation| {
                recommendation.arguments.get(MCP_BRIEF_ARG_PROJECT_PATH)
                    == Some(&serde_json::Value::String(project_path.clone()))
            }),
            "indexed brief recommendations did not preserve project_path",
        )?;
        require(
            recommendations.iter().any(|recommendation| {
                matches!(recommendation.kind, McpBriefRecommendationKind::Summary)
                    && recommendation.target == MCP_TOOL_ATLAS_FILE_SUMMARY
                    && recommendation.arguments.get(MCP_BRIEF_ARG_FILE)
                        == Some(&serde_json::Value::String("src/lib.rs".to_string()))
            }),
            "summary recommendation did not preserve the ranked file selector",
        )?;
        require(
            recommendations.iter().all(|recommendation| {
                recommendation.target != MCP_TOOL_ATLAS_FOLDERS
                    && recommendation.target != MCP_TOOL_ATLAS_FILES
            }),
            "indexed brief recommended rerunning folder or file ranking",
        )?;
        require(
            recommendations.iter().any(|recommendation| {
                matches!(recommendation.kind, McpBriefRecommendationKind::Health)
                    && recommendation.arguments.get(MCP_BRIEF_ARG_LIMIT)
                        == Some(&serde_json::json!(7))
            }),
            "health recommendation did not preserve limit",
        )?;

        let relation_recommendations = ProjectAtlasMcpServer::indexed_project_recommendations(
            "startup",
            Some(NavigationNextCall {
                capability: NavigationNextCapability::Relations,
                path: "src/graph.rs".to_string(),
            }),
            0,
            7,
            Some(project_path),
            None,
        );
        require(
            relation_recommendations.iter().any(|recommendation| {
                matches!(recommendation.kind, McpBriefRecommendationKind::Relations)
                    && recommendation.target == MCP_TOOL_ATLAS_SYMBOL_RELATIONS
                    && recommendation.arguments.get(MCP_BRIEF_ARG_FILE)
                        == Some(&serde_json::Value::String("src/graph.rs".to_string()))
                    && recommendation.arguments.get(MCP_BRIEF_ARG_VIEW)
                        == Some(&serde_json::Value::String("detailed".to_string()))
            }),
            "relation recommendation did not preserve the ranked file and detailed view",
        )?;

        let worktree_recommendations = ProjectAtlasMcpServer::indexed_project_recommendations(
            "startup",
            None,
            1,
            7,
            None,
            Some("issue-430".to_string()),
        );
        require(
            worktree_recommendations.iter().all(|recommendation| {
                recommendation.arguments.get(MCP_BRIEF_ARG_WORKTREE)
                    == Some(&serde_json::Value::String("issue-430".to_string()))
                    && recommendation
                        .arguments
                        .get(MCP_BRIEF_ARG_PROJECT_PATH)
                        .is_none()
            }),
            "indexed brief recommendations did not preserve the mutually exclusive worktree alias",
        )?;

        Ok(())
    }

    #[test]
    fn session_brief_file_candidates_ignore_indexed_text_fallback()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo-a");
        fs::create_dir_all(repo.join("src"))?;
        fs::write(
            repo.join("src").join("owner.rs"),
            "const ROUTE: &str = \"hiddenNeedle\";\n",
        )?;
        let db_path = repo.join(".projectatlas").join("projectatlas.db");
        let plan = ScanRuntimePlan::for_path(None, &repo, None)?;
        let mut store = open_atlas_store_for_project(&db_path, &plan.root)?;
        let symbol_options = SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, Some(1), Some(30));
        run_scan_pipeline(&mut store, &plan, &symbol_options)?;
        drop(store);

        let server = ProjectAtlasMcpServer::new(db_path, None, "mcp-test".to_string(), false);
        let brief = server.build_session_brief(
            AtlasSessionBriefParams {
                project_path: None,
                worktree: None,
                query: Some("hiddenNeedle".to_string()),
                purpose_task: None,
                compact: None,
                folder_limit: Some(5),
                file_limit: Some(5),
                blocker_limit: Some(5),
                purpose_limit: Some(5),
            },
            None,
        )?;

        require(
            brief.files.is_empty(),
            "session brief returned a content-only indexed-text hit",
        )?;
        require(
            brief.recommendations.iter().any(|recommendation| {
                matches!(recommendation.kind, McpBriefRecommendationKind::Search)
                    && recommendation.target == MCP_TOOL_ATLAS_SEARCH
                    && recommendation.arguments.get(MCP_BRIEF_ARG_PATTERN)
                        == Some(&serde_json::Value::String("hiddenNeedle".to_string()))
            }),
            "session brief did not route a content-only query directly to indexed search",
        )?;

        let navigable = server.build_session_brief(
            AtlasSessionBriefParams {
                project_path: None,
                worktree: None,
                query: Some("owner".to_string()),
                purpose_task: None,
                compact: None,
                folder_limit: Some(5),
                file_limit: Some(5),
                blocker_limit: Some(5),
                purpose_limit: Some(5),
            },
            None,
        )?;
        let candidate = navigable
            .files
            .first()
            .ok_or_else(|| std::io::Error::other("navigable brief file is missing"))?;
        require(
            candidate.reason_codes.contains(&RankedReasonCode::Path)
                && candidate.next_call.capability == NavigationNextCapability::Summary
                && !candidate.purpose_agent_reviewed,
            "session brief dropped ranked navigation evidence",
        )?;
        require(
            navigable.recommendations.iter().any(|recommendation| {
                matches!(recommendation.kind, McpBriefRecommendationKind::Summary)
                    && recommendation.target == MCP_TOOL_ATLAS_FILE_SUMMARY
                    && recommendation.arguments.get(MCP_BRIEF_ARG_FILE)
                        == Some(&serde_json::Value::String(candidate.path.clone()))
            }) && navigable.recommendations.iter().all(|recommendation| {
                recommendation.target != MCP_TOOL_ATLAS_FOLDERS
                    && recommendation.target != MCP_TOOL_ATLAS_FILES
            }),
            "session brief recommendation did not follow its returned ranked file directly",
        )?;

        Ok(())
    }

    #[test]
    fn mcp_navigation_and_session_brief_propagate_typed_graph_evidence()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo-a");
        fs::create_dir_all(repo.join("src"))?;
        fs::create_dir_all(repo.join("tests"))?;
        fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname = \"adapter-navigation\"\nversion = \"0.1.0\"\n",
        )?;
        for path in [
            "src/navigation_owner.rs",
            "src/navigation_local.rs",
            "src/navigation_unresolved.rs",
            "tests/navigation_owner.rs",
        ] {
            fs::write(repo.join(path), "pub fn navigation_fixture() {}\n")?;
        }
        let db_path = repo.join(".projectatlas").join("projectatlas.db");
        let plan = ScanRuntimePlan::for_path(None, &repo, None)?;
        let mut store = open_atlas_store_for_project(&db_path, &plan.root)?;
        run_scan_pipeline(
            &mut store,
            &plan,
            &SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, Some(1), Some(30)),
        )?;
        publish_mcp_navigation_graph(&mut store)?;
        drop(store);

        let server =
            ProjectAtlasMcpServer::new(db_path, None, "mcp-navigation-test".to_string(), false);
        let folders_text = server.atlas_folders_response(
            AtlasQueryParams {
                project_path: None,
                worktree: None,
                query: Some("navigation".to_string()),
                limit: Some(10),
            },
            None,
        );
        let files_text = server.atlas_files_response(
            AtlasFilesParams {
                project_path: None,
                worktree: None,
                query: Some("navigation".to_string()),
                folder: None,
                nearest_project: Some(false),
                file_pattern: None,
                include_content: Some(false),
                content_selection: None,
                limit: Some(10),
            },
            None,
        );
        for (surface, text) in [
            ("atlas_folders", &folders_text),
            ("atlas_files", &files_text),
        ] {
            require(
                text.contains("connection_counts")
                    && text.contains("connections")
                    && text.contains("direction:")
                    && text.contains("target:")
                    && text.contains("connections_truncated: true"),
                &format!("{surface} dropped nonempty typed graph evidence: {text}"),
            )?;
        }

        let brief = server.build_session_brief(
            AtlasSessionBriefParams {
                project_path: None,
                worktree: None,
                query: Some("navigation".to_string()),
                purpose_task: None,
                compact: None,
                folder_limit: Some(10),
                file_limit: Some(10),
                blocker_limit: Some(10),
                purpose_limit: Some(10),
            },
            None,
        )?;
        let folder = brief
            .folders
            .iter()
            .find(|candidate| candidate.path == "src")
            .ok_or_else(|| io::Error::other("graph-enriched MCP folder is missing"))?;
        require(
            folder.connection_counts.len() == 7
                && folder.connections.len() == 3
                && folder.connections_truncated,
            "MCP folder lost count, sample, or global truncation evidence",
        )?;
        let owner = brief
            .files
            .iter()
            .find(|candidate| candidate.path == "src/navigation_owner.rs")
            .ok_or_else(|| io::Error::other("graph-enriched MCP owner file is missing"))?;
        require(
            owner.connection_counts.len() == 7
                && owner.connections.len() == 3
                && owner.connections_truncated
                && owner.next_call.capability
                    == projectatlas_core::NavigationNextCapability::Relations,
            "MCP file or session brief lost graph truncation or relations navigation",
        )?;
        let compact_relations = server.atlas_symbol_relations_response(
            &AtlasSymbolRelationsParams {
                project_path: None,
                file: Some("src/navigation_owner.rs".to_string()),
                nearest_project: Some(false),
                view: Some("detailed".to_string()),
                compact: Some(true),
                direction: Some("outbound".to_string()),
                include_occurrences: Some(true),
                limit: Some(10),
                output_bytes: Some(64 * 1_024),
                ..AtlasSymbolRelationsParams::default()
            },
            None,
        );
        require(
            compact_relations.contains("status: resolved")
                && compact_relations.contains("status: ambiguous")
                && compact_relations.contains("status: external")
                && compact_relations.contains("status: unresolved")
                && compact_relations.contains("reference: \"navigation-ambiguous\"")
                && compact_relations.contains("candidates: 2")
                && compact_relations.contains("next_call:"),
            &format!(
                "compact detailed relations dropped a resolution state or reusable next call: {compact_relations}"
            ),
        )?;

        let compact = server.build_compact_session_brief(
            AtlasSessionBriefParams {
                project_path: None,
                worktree: None,
                query: Some("navigation_owner".to_string()),
                purpose_task: None,
                compact: Some(true),
                folder_limit: None,
                file_limit: None,
                blocker_limit: None,
                purpose_limit: None,
            },
            None,
        )?;
        let compact_owner = compact
            .files
            .iter()
            .find(|candidate| candidate.path == "src/navigation_owner.rs")
            .ok_or_else(|| io::Error::other("compact graph owner file is missing"))?;
        require(
            compact_owner.connections.len() == 1
                && compact_owner.connections.iter().all(|connection| {
                    connection.kind != RankedConnectionKind::Import
                        && !matches!(
                            &connection.target,
                            RankedConnectionTarget::Unresolved { .. }
                        )
                })
                && compact_owner.next_call.capability == NavigationNextCapability::Summary
                && compact_owner.purpose_agent_reviewed,
            "compact session brief did not retain one crisp edge and summary-first routing",
        )?;
        let expanded_text =
            ProjectAtlasMcpServer::encode_named_payload(MCP_PAYLOAD_SESSION_BRIEF, &brief)?;
        require(
            expanded_text.contains("missing_purposes:")
                && expanded_text.contains("stale_purposes:")
                && expanded_text.contains("approved_purposes:")
                && expanded_text.contains("suggested_purposes:"),
            "compatibility session brief lost purpose lifecycle counts",
        )?;
        let compact_text =
            ProjectAtlasMcpServer::encode_named_payload(MCP_PAYLOAD_SESSION_BRIEF, &compact)?;
        require(
            compact.purpose_handoff.as_ref().is_some_and(|handoff| {
                handoff.recommended_subagent_reasoning == PURPOSE_CURATOR_RECOMMENDED_REASONING
                    && handoff.instructions.len() == 1
                    && handoff.instructions.first()
                        == brief
                            .purpose_handoff
                            .as_ref()
                            .and_then(|expanded| expanded.instructions.first())
            }) && compact_text.contains(&format!(
                "recommended_subagent_reasoning: {PURPOSE_CURATOR_RECOMMENDED_REASONING}"
            )) && compact_text
                .contains("lowest reliable reasoning and cost tier the host supports"),
            &format!(
                "compact actionable handoff lost its reliable-tier instruction: {compact_text}"
            ),
        )?;
        require(
            compact
                .blockers
                .as_ref()
                .is_some_and(|blockers| blockers.total > 0)
                && !compact_text.contains("\n    db:")
                && !compact_text.contains("\n    config:")
                && !compact_text.contains("\n  policy:")
                && !compact_text.contains("\n    items:")
                && !compact_text.contains("reason_codes")
                && !compact_text.contains("connection_counts")
                && !compact_text.contains("agent_harness_expected")
                && !compact_text.contains("server_started_curator")
                && !compact_text.contains("missing_purposes")
                && compact_text.len() <= 4_096,
            &format!("compact session brief retained default-only chatter: {compact_text}"),
        )?;

        let families = brief
            .files
            .iter()
            .flat_map(|candidate| candidate.connection_counts.iter().map(|count| count.kind))
            .collect::<BTreeSet<_>>();
        require(
            families
                == BTreeSet::from([
                    RankedConnectionKind::Package,
                    RankedConnectionKind::Import,
                    RankedConnectionKind::Call,
                    RankedConnectionKind::Reference,
                    RankedConnectionKind::Test,
                    RankedConnectionKind::Route,
                    RankedConnectionKind::Config,
                ]),
            &format!("MCP graph families were not propagated: {families:?}"),
        )?;
        let connections = brief
            .files
            .iter()
            .flat_map(|candidate| candidate.connections.iter())
            .collect::<Vec<_>>();
        require(
            connections
                .iter()
                .any(|connection| connection.direction == RankedConnectionDirection::Outbound)
                && connections
                    .iter()
                    .any(|connection| connection.direction == RankedConnectionDirection::Inbound),
            "MCP graph samples did not preserve both directions",
        )?;
        for (name, present) in [
            (
                "local",
                connections.iter().any(|connection| {
                    matches!(connection.target, RankedConnectionTarget::Local { .. })
                }),
            ),
            (
                "package",
                connections.iter().any(|connection| {
                    matches!(connection.target, RankedConnectionTarget::Package { .. })
                }),
            ),
            (
                "external",
                connections.iter().any(|connection| {
                    matches!(connection.target, RankedConnectionTarget::External { .. })
                }),
            ),
            (
                "unresolved",
                connections.iter().any(|connection| {
                    matches!(connection.target, RankedConnectionTarget::Unresolved { .. })
                }),
            ),
        ] {
            require(
                present,
                &format!("MCP graph samples omitted {name} targets"),
            )?;
        }
        Ok(())
    }

    fn publish_mcp_navigation_graph(
        store: &mut AtlasStore,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("MCP navigation project identity is missing"))?;
        let current_publication = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("MCP navigation publication is missing"))?;
        let fingerprint = current_publication
            .contract_fingerprint
            .clone()
            .ok_or_else(|| io::Error::other("MCP navigation fingerprint is missing"))?;
        let generation = current_publication
            .generation
            .checked_next()
            .ok_or_else(|| io::Error::other("MCP navigation generation overflow"))?;
        let file_entity = |path: &str| {
            GraphEntity::new(
                project,
                EntitySelector::File {
                    path: RepositoryFilePath::new(Path::new(path))?,
                },
                generation,
            )
        };
        let owner = file_entity("src/navigation_owner.rs")?;
        let local = file_entity("src/navigation_local.rs")?;
        let unresolved = file_entity("src/navigation_unresolved.rs")?;
        let test = file_entity("tests/navigation_owner.rs")?;
        let package = GraphEntity::new(
            project,
            EntitySelector::Package {
                package: PackageSelector {
                    manager: GraphIdentityText::new("cargo")?,
                    name: GraphIdentityText::new("adapter-navigation")?,
                    manifest: RepositoryFilePath::new(Path::new("Cargo.toml"))?,
                },
            },
            generation,
        )?;
        let external = GraphEntity::new(
            project,
            EntitySelector::External {
                external: ExternalSelector {
                    system: GraphIdentityText::new("crates.io")?,
                    identity: GraphIdentityText::new("serde@1")?,
                },
            },
            generation,
        )?;
        let resolved = |source: &GraphEntity, kind, target: &GraphEntity| {
            Ok::<_, Box<dyn std::error::Error>>(LogicalRelation::new(
                source,
                kind,
                RelationResolution::resolved(target)?,
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?)
        };
        let unresolved_relation = |source: &GraphEntity, kind, reference: &str| {
            Ok::<_, Box<dyn std::error::Error>>(LogicalRelation::new(
                source,
                kind,
                RelationResolution::Unresolved {
                    reference: GraphIdentityText::new(reference)?,
                },
                ConfidenceClass::High,
                Completeness::Partial,
                generation,
            )?)
        };
        let relations = vec![
            resolved(
                &package,
                GraphRelationKind::Legacy(RelationKind::DependsOn),
                &owner,
            )?,
            LogicalRelation::new(
                &owner,
                GraphRelationKind::Legacy(RelationKind::Imports),
                RelationResolution::external(&external)?,
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?,
            resolved(
                &owner,
                GraphRelationKind::Legacy(RelationKind::Calls),
                &local,
            )?,
            unresolved_relation(
                &unresolved,
                GraphRelationKind::Extended(ExtendedRelationKind::References),
                "navigation-reference",
            )?,
            resolved(
                &test,
                GraphRelationKind::Extended(ExtendedRelationKind::Tests),
                &owner,
            )?,
            resolved(
                &owner,
                GraphRelationKind::Extended(ExtendedRelationKind::RoutesTo),
                &local,
            )?,
            LogicalRelation::new(
                &owner,
                GraphRelationKind::Extended(ExtendedRelationKind::References),
                RelationResolution::Ambiguous {
                    reference: GraphIdentityText::new("navigation-ambiguous")?,
                    candidates: std::num::NonZeroU32::new(2)
                        .ok_or_else(|| io::Error::other("ambiguous fixture count is zero"))?,
                },
                ConfidenceClass::High,
                Completeness::Partial,
                generation,
            )?,
            unresolved_relation(
                &owner,
                GraphRelationKind::Extended(ExtendedRelationKind::Configures),
                "NAVIGATION_MODE",
            )?,
        ];
        let nodes = store
            .load_nodes()?
            .into_iter()
            .map(|node| node.node)
            .collect::<Vec<_>>();
        {
            let mut publication = store.begin_index_publication(&fingerprint)?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&nodes)?;
            publication.finish_scan_replacement()?;
            publication.replace_repository_graph(
                project,
                &[owner, local, unresolved, test, package, external],
                &relations,
                &[],
                &[],
            )?;
            publication.complete()?;
        }
        store.set_purpose("src", "Navigation graph folder", PurposeSource::Agent)?;
        store.set_purpose(
            "src/navigation_owner.rs",
            "Navigation graph owner",
            PurposeSource::Agent,
        )?;
        store.set_purpose(
            "src/navigation_unresolved.rs",
            "Navigation unresolved graph owner",
            PurposeSource::Agent,
        )?;
        Ok(())
    }

    #[test]
    fn session_brief_exposes_host_owned_purpose_curator_handoff()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo-a");
        fs::create_dir_all(repo.join("src"))?;
        fs::write(repo.join("src").join("main.rs"), "fn main() {}\n")?;
        let db_path = repo.join(".projectatlas").join("projectatlas.db");
        let plan = ScanRuntimePlan::for_path(None, &repo, None)?;
        let mut store = open_atlas_store_for_project(&db_path, &plan.root)?;
        let symbol_options = SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, Some(1), Some(30));
        run_scan_pipeline(&mut store, &plan, &symbol_options)?;
        drop(store);

        let server = ProjectAtlasMcpServer::new(db_path, None, "mcp-test".to_string(), false);
        let brief = server.build_session_brief(
            AtlasSessionBriefParams {
                project_path: None,
                worktree: None,
                query: Some("startup".to_string()),
                purpose_task: Some("startup-task".to_string()),
                compact: None,
                folder_limit: Some(5),
                file_limit: Some(5),
                blocker_limit: Some(5),
                purpose_limit: Some(1),
            },
            None,
        )?;
        let handoff = brief
            .purpose_handoff
            .as_ref()
            .ok_or_else(|| std::io::Error::other("actionable purpose handoff missing"))?;
        require(
            handoff.execution_owner == "agent_host",
            "session handoff was not host-owned",
        )?;
        require(
            handoff.recommended_subagent_reasoning == PURPOSE_CURATOR_RECOMMENDED_REASONING,
            "session handoff did not request the lowest reliable host-supported reasoning",
        )?;
        require(
            handoff.main_agent_fallback && !handoff.server_started_curator,
            "session handoff misrepresented curator execution ownership",
        )?;
        require(
            handoff.silent_on_success,
            "session handoff was not quiet on successful maintenance",
        )?;
        require(
            handoff.queue.task == "startup-task"
                && handoff.queue.curation_scope == "low"
                && handoff.queue.actionable
                && handoff.queue.returned == 1
                && handoff.queue.limit == 1
                && handoff.queue.truncated,
            "compatibility session handoff lost its bounded purpose queue metadata",
        )?;
        require(
            handoff.queue.items.iter().all(|item| {
                item.work_key.len() == 64
                    && item.state_token.len() == 64
                    && !item.purpose_agent_reviewed
            }),
            "session handoff item tokens or lifecycle state were incomplete",
        )?;
        let compact = server.build_compact_session_brief(
            AtlasSessionBriefParams {
                project_path: None,
                worktree: None,
                query: Some("startup".to_string()),
                purpose_task: Some("startup-task".to_string()),
                compact: Some(true),
                folder_limit: Some(5),
                file_limit: Some(5),
                blocker_limit: Some(5),
                purpose_limit: Some(1),
            },
            None,
        )?;
        let compact_handoff = compact
            .purpose_handoff
            .as_ref()
            .ok_or_else(|| std::io::Error::other("compact purpose handoff missing"))?;
        require(
            matches!(
                compact_handoff.next_call.kind,
                McpBriefRecommendationKind::PurposeQueue
            ) && compact_handoff.next_call.target == MCP_TOOL_ATLAS_PURPOSE_QUEUE
                && compact_handoff.next_call.arguments.get(MCP_BRIEF_ARG_TASK)
                    == Some(&serde_json::json!("startup-task"))
                && compact_handoff.next_call.arguments.get(MCP_BRIEF_ARG_LIMIT)
                    == Some(&serde_json::json!(1)),
            "compact handoff did not preserve the exact bounded purpose-queue call",
        )?;
        require(
            brief.limits.purpose_limit == 1 && brief.limits.purposes_truncated,
            "session brief purpose limits were not reported",
        )?;
        Ok(())
    }

    #[test]
    fn settings_capabilities_report_nearest_policy() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo-a");
        fs::create_dir(&repo)?;
        let db_path = repo.join(".projectatlas").join("projectatlas.db");
        let disabled =
            ProjectAtlasMcpServer::new(db_path.clone(), None, "mcp-test".to_string(), false);
        let enabled = ProjectAtlasMcpServer::new(db_path, None, "mcp-test".to_string(), true);
        let disabled_state = disabled.active_project_state()?;
        let enabled_state = enabled.active_project_state()?;

        let disabled_text = disabled.render_settings_with_capabilities(&disabled_state)?;
        require(
            disabled_text.contains("mcp_session:"),
            "settings did not include mcp_session capabilities",
        )?;
        require(
            disabled_text.contains("path_scope: selected_project"),
            "disabled nearest-project policy was not typed",
        )?;
        require(
            disabled_text.contains("language_registry:")
                && disabled_text.contains("accepted_set_digest:")
                && disabled_text.contains("semantic_provider_digest:")
                && disabled_text.contains("semantic_relation_contract_digest:")
                && disabled_text.contains("relation_family_inventory:")
                && disabled_text.contains("optional_disabled_families:")
                && disabled_text.contains("benchmarked:")
                && !disabled_text.contains("accepted_minimum:")
                && !disabled_text.contains("provenance_source:")
                && disabled_text.contains("optional_catalog:")
                && disabled_text.contains("database:")
                && disabled_text.contains("compile_options:")
                && disabled_text.contains("search:")
                && disabled_text.contains("default_mode: lexical")
                && disabled_text.contains("optional_parser_pack:"),
            "settings did not project compact shared language registry truth",
        )?;
        require(
            disabled_text.len() <= MCP_SETTINGS_RESPONSE_MAX_BYTES,
            "settings exceeded its agent-facing output bound",
        )?;
        let enabled_text = enabled.render_settings_with_capabilities(&enabled_state)?;
        require(
            enabled_text.contains("path_scope: nearest_indexed_project"),
            "enabled nearest-project policy was not typed",
        )?;
        require(
            !enabled_text.contains("GITHUB_TOKEN") && !enabled_text.contains("GH_TOKEN"),
            "settings capabilities leaked token environment names",
        )?;

        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn mcp_settings_reports_native_root_equivalence() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let original = temp.path().join("McpCaseRoot");
        let staging = temp.path().join("McpCaseRootStaging");
        let renamed = temp.path().join("mcpcaseroot");
        fs::create_dir(&original)?;
        let database = original.join(".projectatlas/projectatlas.db");
        fs::create_dir_all(
            database
                .parent()
                .ok_or_else(|| io::Error::other("MCP case-only database has no parent"))?,
        )?;
        drop(AtlasStore::open_for_project(&database, &original)?);
        fs::rename(&original, &staging)?;
        fs::rename(&staging, &renamed)?;
        let renamed_database = renamed.join(".projectatlas/projectatlas.db");
        let positive_config = temp.path().join("mcp-case-only-config.toml");
        fs::write(
            &positive_config,
            format!(
                "[project]\nroot = {}\n",
                serde_json::to_string(&renamed.to_string_lossy())?
            ),
        )?;
        let positive_server = ProjectAtlasMcpServer::new(
            renamed_database.clone(),
            None,
            "mcp-settings-test".to_string(),
            false,
        );
        let positive_state = McpProjectState {
            root: renamed,
            db_path: renamed_database,
            config_path: Some(positive_config),
            worktree: None,
        };
        let positive: serde_json::Value = toon_format::decode_default(
            &positive_server.render_settings_with_capabilities(&positive_state)?,
        )?;
        require(
            positive.pointer("/settings/root_verified") == Some(&serde_json::json!(true)),
            "MCP settings rejected case-only root rename",
        )?;

        let case_sensitive_parent = temp.path().join("mcp-case-sensitive-parent");
        fs::create_dir(&case_sensitive_parent)?;
        let enabled = StdCommand::new("fsutil")
            .args(["file", "SetCaseSensitiveInfo"])
            .arg(&case_sensitive_parent)
            .arg("enable")
            .status()
            .is_ok_and(|status| status.success());
        if !enabled {
            return Ok(());
        }
        let stored_root = case_sensitive_parent.join("Repo");
        let selected_root = case_sensitive_parent.join("repo");
        fs::create_dir(&stored_root)?;
        fs::create_dir(&selected_root)?;
        let database = stored_root.join(".projectatlas/projectatlas.db");
        fs::create_dir_all(
            database
                .parent()
                .ok_or_else(|| io::Error::other("MCP case-sensitive database has no parent"))?,
        )?;
        drop(AtlasStore::open_for_project(&database, &stored_root)?);
        let negative_config = temp.path().join("mcp-case-sensitive-config.toml");
        fs::write(
            &negative_config,
            format!(
                "[project]\nroot = {}\n",
                serde_json::to_string(&selected_root.to_string_lossy())?
            ),
        )?;
        let negative_server = ProjectAtlasMcpServer::new(
            database.clone(),
            None,
            "mcp-settings-test".to_string(),
            false,
        );
        let negative_state = McpProjectState {
            root: selected_root,
            db_path: database,
            config_path: Some(negative_config),
            worktree: None,
        };
        let negative: serde_json::Value = toon_format::decode_default(
            &negative_server.render_settings_with_capabilities(&negative_state)?,
        )?;
        require(
            negative.pointer("/settings/root_verified") == Some(&serde_json::json!(false)),
            "MCP settings accepted a case-sensitive sibling",
        )?;
        Ok(())
    }

    #[test]
    fn task_progress_status_and_cancel_are_typed() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo-a");
        fs::create_dir(&repo)?;
        let db_path = repo.join(".projectatlas").join("projectatlas.db");
        let server = ProjectAtlasMcpServer::new(db_path, None, "mcp-test".to_string(), false);

        let status = server.task_status(MCP_TASK_CONTRACT_ID.to_string())?;
        require(
            status.lookup == McpTaskLookupStatus::Found,
            "contract task missing",
        )?;
        require(
            status
                .task
                .as_ref()
                .is_some_and(|task| task.state == McpTaskState::Complete),
            "contract task was not complete",
        )?;
        require(
            status.states.contains(&McpTaskState::Pending)
                && status.states.contains(&McpTaskState::Canceled),
            "status response did not expose task state contract",
        )?;
        let cancel = server.task_cancel(MCP_TASK_CONTRACT_ID.to_string())?;
        require(
            cancel.result == McpTaskCancelResult::AlreadyFinished,
            "completed contract task did not return already_finished",
        )?;
        let missing = server.task_status("missing-task".to_string())?;
        require(
            missing.lookup == McpTaskLookupStatus::NotFound,
            "unknown task did not return not_found",
        )?;

        Ok(())
    }

    #[test]
    fn background_task_envelope_and_cancellation_reach_owned_control()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo-a");
        fs::create_dir(&repo)?;
        let db_path = repo.join(".projectatlas").join("projectatlas.db");
        let mut server = ProjectAtlasMcpServer::new(db_path, None, "mcp-test".to_string(), false);
        let host_envelope = server.background_resources;
        let host_workers = thread::available_parallelism().map_or(1, usize::from);
        let representative_envelope = McpBackgroundResourceEnvelope::from_available_workers(8);
        require(
            host_envelope.task_limit <= MCP_BACKGROUND_TASK_SAFE_CEILING
                && host_envelope.workers_per_task > 0
                && host_envelope.total_worker_limit
                    <= host_workers.clamp(1, INDEX_WORKER_SAFE_CEILING)
                && host_envelope.workers_per_task * host_envelope.task_limit
                    <= host_envelope.total_worker_limit,
            "background resource envelope exceeded its host or process worker budget",
        )?;
        require(
            representative_envelope
                == (McpBackgroundResourceEnvelope {
                    task_limit: 4,
                    workers_per_task: 2,
                    total_worker_limit: 8,
                })
                && McpBackgroundResourceEnvelope::from_available_workers(0).total_worker_limit == 1
                && McpBackgroundResourceEnvelope::from_available_workers(usize::MAX)
                    .total_worker_limit
                    == INDEX_WORKER_SAFE_CEILING,
            "background resource envelope did not partition representative host capacities",
        )?;

        server.background_resources = McpBackgroundResourceEnvelope::from_available_workers(4);
        let envelope = server.background_resources;
        let started = Arc::new(std::sync::Barrier::new(envelope.task_limit + 1));
        let databases_ready = Arc::new(std::sync::Barrier::new(envelope.task_limit + 1));
        let release = Arc::new(std::sync::Barrier::new(envelope.task_limit + 1));
        let observed_option_workers = Arc::new(AtomicU64::new(0));
        let observed_control_workers = Arc::new(AtomicU64::new(0));
        let mut concurrent_tasks = Vec::new();
        for task_index in 0..envelope.task_limit {
            let isolated_root = temp.path().join(format!("repo-{task_index}"));
            let isolated_db = isolated_root.join(".projectatlas").join("projectatlas.db");
            fs::create_dir_all(isolated_root.join(".projectatlas"))?;
            let worker_started = Arc::clone(&started);
            let worker_databases_ready = Arc::clone(&databases_ready);
            let worker_release = Arc::clone(&release);
            let worker_option_workers = Arc::clone(&observed_option_workers);
            let worker_control_workers = Arc::clone(&observed_control_workers);
            concurrent_tasks.push(server.start_index_task(
                McpTaskOperation::Scan,
                SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None),
                MCP_TOOL_ATLAS_OVERVIEW,
                move |control, options| {
                    worker_option_workers
                        .fetch_add(options.reported_workers() as u64, Ordering::Relaxed);
                    worker_control_workers.fetch_add(
                        control.worker_ceiling().unwrap_or_default() as u64,
                        Ordering::Relaxed,
                    );
                    worker_started.wait();
                    let store = open_atlas_store_for_project(&isolated_db, &isolated_root);
                    worker_databases_ready.wait();
                    worker_release.wait();
                    let _store = store?;
                    Ok(())
                },
            )?);
        }
        started.wait();
        let admitted_workers = envelope.workers_per_task * envelope.task_limit;
        require(
            observed_option_workers.load(Ordering::Relaxed) == admitted_workers as u64
                && observed_control_workers.load(Ordering::Relaxed) == admitted_workers as u64
                && admitted_workers <= envelope.total_worker_limit,
            "concurrent background tasks did not share the aggregate worker envelope",
        )?;
        let overflow = server.start_index_task(
            McpTaskOperation::Scan,
            SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None),
            MCP_TOOL_ATLAS_OVERVIEW,
            |_control, _options| Ok(()),
        );
        require(
            matches!(overflow, Err(CliError::Mcp(message)) if message.starts_with(MCP_INDEX_TASK_LIMIT_PREFIX)),
            "background task admission exceeded the server task limit",
        )?;
        require(
            server.task_status(MCP_TASK_CONTRACT_ID.to_string())?.lookup
                == McpTaskLookupStatus::Found,
            "task status became unresponsive while concurrent work was active",
        )?;
        databases_ready.wait();
        release.wait();
        for task in concurrent_tasks {
            require(
                wait_for_background_task(&server, &task.task_id)?.state == McpTaskState::Complete,
                "concurrent background task did not finish successfully",
            )?;
        }

        let cancel_started = Arc::new(std::sync::Barrier::new(2));
        let worker_cancel_started = Arc::clone(&cancel_started);
        let canceled_task = server.start_index_task(
            McpTaskOperation::Scan,
            SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None),
            MCP_TOOL_ATLAS_OVERVIEW,
            move |control, options| {
                if options.reported_workers() != control.worker_ceiling().unwrap_or_default() {
                    return Err(CliError::Mcp(
                        "background parser and operation worker ceilings diverged".to_string(),
                    ));
                }
                worker_cancel_started.wait();
                loop {
                    control
                        .check(projectatlas_core::IndexWorkStage::RepositoryTraversal)
                        .map_err(|failure| {
                            CliError::Fs(projectatlas_fs::FsError::IndexWork(failure))
                        })?;
                    thread::yield_now();
                }
            },
        )?;
        cancel_started.wait();

        let running = server.task_status(canceled_task.task_id.clone())?;
        require(
            running
                .task
                .as_ref()
                .is_some_and(|record| record.state == McpTaskState::Running),
            "background task did not become running",
        )?;
        let cancel = server.task_cancel(canceled_task.task_id.clone())?;
        require(
            cancel.result == McpTaskCancelResult::CancellationRequested,
            "task cancellation did not reach the active work control",
        )?;

        require(
            wait_for_background_task(&server, &canceled_task.task_id)?.state
                == McpTaskState::Canceled,
            "background task did not finish canceled after consuming the signal",
        )?;
        require(
            run_successful_background_task(&server)?.state == McpTaskState::Complete,
            "successful task was not admitted after cancellation",
        )?;

        let failed_task = server.start_index_task(
            McpTaskOperation::Scan,
            SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None),
            MCP_TOOL_ATLAS_OVERVIEW,
            |_control, _options| Err(CliError::Mcp("expected task failure".to_string())),
        )?;
        require(
            wait_for_background_task(&server, &failed_task.task_id)?.state == McpTaskState::Failed,
            "background task error did not become a terminal failure",
        )?;
        require(
            run_successful_background_task(&server)?.state == McpTaskState::Complete,
            "successful task was not admitted after failure",
        )?;

        let panicked_task = server.start_index_task(
            McpTaskOperation::Scan,
            SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, None, None),
            MCP_TOOL_ATLAS_OVERVIEW,
            |_control, _options| -> Result<(), CliError> {
                std::panic::resume_unwind(Box::new("expected background task panic"));
            },
        )?;
        let panicked = wait_for_background_task(&server, &panicked_task.task_id)?;
        require(
            panicked.state == McpTaskState::Failed
                && panicked.error.as_deref() == Some(MCP_INDEX_WORKER_PANIC_ERROR),
            "background task panic did not remain bounded and terminal",
        )?;
        require(
            run_successful_background_task(&server)?.state == McpTaskState::Complete,
            "terminal task lifecycle did not release background admission capacity",
        )?;

        Ok(())
    }

    #[test]
    fn background_scan_defers_config_validation_and_rejects_root_redirection()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo-a");
        let redirected = temp.path().join("repo-b");
        let atlas_dir = repo.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        fs::create_dir_all(&redirected)?;
        let config_path = atlas_dir.join("config.toml");
        fs::write(&config_path, "[project]\nroot = \"../../repo-b\"\n")?;
        let db_path = atlas_dir.join("projectatlas.db");
        let server =
            ProjectAtlasMcpServer::new(db_path, Some(config_path), "mcp-test".to_string(), false);

        let response = server.atlas_scan(Parameters(AtlasScanParams {
            project_path: Some(repo.to_string_lossy().into_owned()),
            worktree: None,
            path: None,
            nearest_project: Some(false),
            max_bytes: None,
            max_workers: Some(1),
            timeout_seconds: None,
            text_index_max_bytes: None,
            background: Some(true),
        }));
        require(
            response.contains(MCP_PAYLOAD_TASK_START),
            "explicit background project validated redirecting config before task admission",
        )?;
        let admitted_task_id = server
            .task_registry
            .read()
            .map_err(|_poisoned| io::Error::other("task registry lock poisoned"))?
            .records
            .iter()
            .find(|record| matches!(record.operation, McpTaskOperation::Scan))
            .map(|record| record.task_id.clone())
            .ok_or_else(|| io::Error::other("admitted background scan task missing"))?;
        let mut admitted_terminal = None;
        for _attempt in 0..1_000 {
            let status = server.task_status(admitted_task_id.clone())?;
            if status
                .task
                .as_ref()
                .is_some_and(McpTaskRecord::is_terminal_state)
            {
                admitted_terminal = status.task;
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        require(
            admitted_terminal
                .as_ref()
                .is_some_and(|record| record.state == McpTaskState::Failed),
            "redirecting background config did not fail inside the admitted task",
        )?;
        require(
            admitted_terminal
                .as_ref()
                .and_then(|record| record.error.as_deref())
                .is_some_and(|error| error.contains("outside selected project root")),
            "controlled plan loading did not preserve root-redirection refusal",
        )?;

        Ok(())
    }

    #[test]
    fn index_adapters_publish_scan_symbol_and_watch_effects()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        let source_dir = repo.join("src");
        fs::create_dir_all(&source_dir)?;
        let source_path = source_dir.join("lib.rs");
        fs::write(
            &source_path,
            "pub fn first() { second(); }\nfn second() {}\n",
        )?;
        let config_path = repo.join(".projectatlas").join("config.toml");
        init_project_with_config(&repo, Some(&config_path))?;
        let db_path = repo.join(".projectatlas").join("projectatlas.db");
        let server =
            ProjectAtlasMcpServer::new(db_path.clone(), None, "mcp-test".to_string(), false);
        let project_path = repo.to_string_lossy().into_owned();

        for operation in [
            McpTaskOperation::Scan,
            McpTaskOperation::SymbolsBuild,
            McpTaskOperation::WatchOnce,
        ] {
            match operation {
                McpTaskOperation::SymbolsBuild => {
                    let store = open_atlas_store_for_project(&db_path, &repo)?;
                    store.clear_symbol_graph_for_path("src/lib.rs")?;
                    require(
                        store.symbol_count_for_path("src/lib.rs")? == 0,
                        "symbol fixture was not cleared before background rebuild",
                    )?;
                }
                McpTaskOperation::WatchOnce => {
                    fs::write(
                        &source_path,
                        "pub fn first() { second(); third(); }\nfn second() {}\nfn third() {}\n",
                    )?;
                }
                McpTaskOperation::Scan | McpTaskOperation::Contract | McpTaskOperation::Search => {}
            }

            let response = match operation {
                McpTaskOperation::Scan | McpTaskOperation::SymbolsBuild => {
                    let params = AtlasScanParams {
                        project_path: Some(project_path.clone()),
                        worktree: None,
                        path: None,
                        nearest_project: Some(false),
                        max_bytes: None,
                        max_workers: Some(1),
                        timeout_seconds: None,
                        text_index_max_bytes: None,
                        background: Some(true),
                    };
                    if operation == McpTaskOperation::Scan {
                        server.atlas_scan(Parameters(params))
                    } else {
                        server.atlas_symbols_build(Parameters(params))
                    }
                }
                McpTaskOperation::WatchOnce => {
                    server.atlas_watch_once(Parameters(AtlasWatchOnceParams {
                        project_path: Some(project_path.clone()),
                        worktree: None,
                        path: None,
                        nearest_project: Some(false),
                        max_workers: Some(1),
                        timeout_seconds: None,
                        text_index_max_bytes: None,
                        background: Some(true),
                    }))
                }
                McpTaskOperation::Contract | McpTaskOperation::Search => unreachable!(),
            };
            require(
                response.contains(MCP_PAYLOAD_TASK_START),
                "production background adapter did not admit its task",
            )?;
            let terminal = wait_for_background_operation(&server, &operation)?;
            require(
                terminal.state == McpTaskState::Complete,
                terminal
                    .error
                    .as_deref()
                    .unwrap_or("background task failed"),
            )?;

            let expected_symbol = match operation {
                McpTaskOperation::Scan | McpTaskOperation::SymbolsBuild => "second",
                McpTaskOperation::WatchOnce => "third",
                McpTaskOperation::Contract | McpTaskOperation::Search => unreachable!(),
            };
            let store = open_atlas_store_for_project(&db_path, &repo)?;
            store.set_purpose(
                "src/lib.rs",
                "Own café λ relation navigation",
                PurposeSource::Agent,
            )?;
            drop(store);
            require_agent_index_reads(&server, &project_path, expected_symbol)?;
        }

        let store = open_atlas_store_for_project(&db_path, &repo)?;
        store.clear_symbol_graph_for_path("src/lib.rs")?;
        drop(store);
        let synchronous_symbols = server.atlas_symbols_build(Parameters(AtlasScanParams {
            project_path: Some(project_path.clone()),
            worktree: None,
            path: None,
            nearest_project: Some(false),
            max_bytes: None,
            max_workers: Some(1),
            timeout_seconds: None,
            text_index_max_bytes: None,
            background: Some(false),
        }));
        require(
            synchronous_symbols.contains(MCP_PAYLOAD_SYMBOLS_BUILD),
            "synchronous symbol adapter did not return its completed report",
        )?;
        require_agent_index_reads(&server, &project_path, "second")?;

        fs::write(
            &source_path,
            "pub fn first() { second(); fourth(); }\nfn second() {}\nfn fourth() {}\n",
        )?;
        let synchronous_watch = server.atlas_watch_once(Parameters(AtlasWatchOnceParams {
            project_path: Some(project_path.clone()),
            worktree: None,
            path: None,
            nearest_project: Some(false),
            max_workers: Some(1),
            timeout_seconds: None,
            text_index_max_bytes: None,
            background: Some(false),
        }));
        require(
            synchronous_watch.contains(MCP_PAYLOAD_WATCH),
            "synchronous watch adapter did not return its completed report",
        )?;
        require_agent_index_reads(&server, &project_path, "fourth")?;
        Ok(())
    }

    #[test]
    fn long_lived_mcp_query_families_reuse_one_verified_epoch()
    -> Result<(), Box<dyn std::error::Error>> {
        const LARGE_UNRELATED_SOURCE_FILES: usize = 256;
        const MAX_MEASURED_QUERY_OUTPUT_BYTES: usize = 64 * 1_024;
        const MAX_MEASURED_QUERY_ELAPSED: Duration = Duration::from_secs(30);

        let measure = |unrelated_source_files: usize| {
            let temp = tempfile::tempdir()?;
            let repo = temp.path().join("repo");
            let source_dir = repo.join("src");
            fs::create_dir_all(&source_dir)?;
            fs::write(
                source_dir.join("lib.rs"),
                "pub fn first() { second(); }\nfn second() {}\n",
            )?;
            for index in 0..unrelated_source_files {
                fs::write(
                    source_dir.join(format!("unrelated_{index:03}.rs")),
                    format!("pub fn unrelated_{index:03}() {{}}\n"),
                )?;
            }
            let config_path = repo.join(".projectatlas").join("config.toml");
            init_project_with_config(&repo, Some(&config_path))?;
            let db_path = repo.join(".projectatlas").join("projectatlas.db");
            let mut writer = open_atlas_store_for_project(&db_path, &repo)?;
            let plan = ScanRuntimePlan::for_path(None, &repo, None)?;
            run_scan_pipeline(
                &mut writer,
                &plan,
                &SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, Some(1), None),
            )?;
            drop(writer);

            let server = ProjectAtlasMcpServer::new(
                db_path.clone(),
                None,
                "verified-epoch-test".to_string(),
                false,
            );
            let state = McpProjectState {
                root: repo,
                db_path,
                config_path: None,
                worktree: None,
            };
            let first = server.with_fresh_store(&state, |store, _stamp| Ok(store.overview()?))?;
            require(
                first.work.exact_verifications >= 1,
                "first long-lived MCP read did not establish exact source truth",
            )?;
            require(
                first.work.filesystem_entries > u64::try_from(unrelated_source_files)?,
                "scale fixture did not exercise its complete repository source set",
            )?;
            let expected_stamp = first.stamp;

            let folder = server.with_fresh_store(&state, |store, _stamp| {
                Ok(render_ranked_nodes(
                    NODE_LABEL_FOLDERS,
                    &ranked_folder_nodes_with_reasons(store, "", 4)?,
                ))
            })?;
            let folder_output_bytes = folder.value.len();
            let folder = folder.with_output_bytes(folder_output_bytes);
            let files = server.with_fresh_store(&state, |store, _stamp| {
                Ok(render_ranked_nodes(
                    NODE_LABEL_FILES,
                    &ranked_file_nodes_with_reasons(store, "", Some("src"), None, 4, false)?,
                ))
            })?;
            let files_output_bytes = files.value.len();
            let files = files.with_output_bytes(files_output_bytes);
            let summary = server.with_fresh_store(&state, |store, _stamp| {
                let content = read_indexed_file_content(store, "src/lib.rs")?;
                let report = build_file_summary_from_source(
                    store,
                    Path::new("src/lib.rs"),
                    DEFAULT_FILE_SUMMARY_LIMIT,
                    &content,
                )?;
                Ok(render_file_summary(&report))
            })?;
            let summary_output_bytes = summary.value.len();
            let summary = summary.with_output_bytes(summary_output_bytes);
            let relations = server.with_fresh_store(&state, |store, _stamp| {
                Ok(render_symbol_relations(&store.load_symbol_relations(
                    Some("src/lib.rs"),
                    None,
                    8,
                )?))
            })?;
            let relation_output_bytes = relations.value.len();
            let relations = relations.with_output_bytes(relation_output_bytes);

            let mut measurements = Vec::new();
            for (family, outcome) in [
                ("folder", folder),
                ("file", files),
                ("summary", summary),
                ("relation", relations),
            ] {
                require(
                    outcome.stamp == expected_stamp,
                    &format!("{family} call did not remain bound to the verified epoch"),
                )?;
                require(
                    outcome.work.exact_verifications == 0
                        && outcome.work.filesystem_entries == 0
                        && outcome.work.filesystem_bytes == 0
                        && outcome.work.decoded_nodes == 0,
                    &format!("{family} call repeated repository-sized freshness work"),
                )?;
                require(
                    outcome.work.sqlite_read_statements == 1,
                    &format!("{family} freshness check used unexpected SQLite work"),
                )?;
                require(
                    outcome.work.output_bytes == u64::try_from(outcome.value.len())?,
                    &format!("{family} call did not record its accepted rendered bytes"),
                )?;
                require(
                    outcome.value.len() <= MAX_MEASURED_QUERY_OUTPUT_BYTES,
                    &format!("{family} call exceeded the focused output bound"),
                )?;
                require(
                    !outcome.work.elapsed.is_zero()
                        && outcome.work.elapsed <= MAX_MEASURED_QUERY_ELAPSED,
                    &format!("{family} call did not retain a bounded elapsed measurement"),
                )?;
                measurements.push((family, outcome.work));
            }
            Ok::<_, Box<dyn std::error::Error>>(measurements)
        };

        let small = measure(0)?;
        let large = measure(LARGE_UNRELATED_SOURCE_FILES)?;
        for ((small_family, small_work), (large_family, large_work)) in small.into_iter().zip(large)
        {
            require(
                small_family == large_family,
                "small and large query measurements used different families",
            )?;
            require(
                small_work.exact_verifications == large_work.exact_verifications
                    && small_work.filesystem_entries == large_work.filesystem_entries
                    && small_work.filesystem_bytes == large_work.filesystem_bytes
                    && small_work.sqlite_read_statements == large_work.sqlite_read_statements
                    && small_work.decoded_nodes == large_work.decoded_nodes,
                &format!("{small_family} warm freshness work changed with repository scale"),
            )?;
        }
        Ok(())
    }

    #[test]
    fn task_registry_evictions_prefer_old_terminal_records()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut registry = McpTaskRegistry {
            records: VecDeque::new(),
        };
        registry.insert(McpTaskRecord {
            task_id: "running-0".to_string(),
            operation: McpTaskOperation::Search,
            state: McpTaskState::Running,
            created_at_ms: 0,
            updated_at_ms: 0,
            progress: None,
            error: None,
            result_ref: None,
            cancelable: true,
            control: None,
        });
        for index in 1..MCP_TASK_REGISTRY_CAPACITY {
            registry.insert(McpTaskRecord {
                task_id: format!("complete-{index}"),
                operation: McpTaskOperation::Search,
                state: McpTaskState::Complete,
                created_at_ms: index as u128,
                updated_at_ms: index as u128,
                progress: None,
                error: None,
                result_ref: None,
                cancelable: false,
                control: None,
            });
        }

        registry.insert(McpTaskRecord {
            task_id: "new-complete".to_string(),
            operation: McpTaskOperation::Search,
            state: McpTaskState::Complete,
            created_at_ms: 100,
            updated_at_ms: 100,
            progress: None,
            error: None,
            result_ref: Some("atlas_search".to_string()),
            cancelable: false,
            control: None,
        });

        require(
            registry.records.len() == MCP_TASK_REGISTRY_CAPACITY,
            "task registry exceeded configured capacity",
        )?;
        require(
            registry.get("running-0").is_some(),
            "registry evicted a running record before old terminal records",
        )?;
        require(
            registry.get("complete-1").is_none(),
            "registry did not evict the oldest terminal record",
        )?;
        require(
            registry.get("new-complete").is_some(),
            "registry did not retain the newly inserted record",
        )?;

        Ok(())
    }

    #[test]
    fn selected_root_absolute_path_keys_stay_inside_selected_project()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        let outside = temp.path().join("outside");
        fs::create_dir_all(repo.join("src"))?;
        fs::create_dir_all(outside.join("src"))?;
        let inside_file = repo.join("src").join("lib.rs");
        let outside_file = outside.join("src").join("lib.rs");
        fs::write(&inside_file, "pub fn inside() {}\n")?;
        fs::write(&outside_file, "pub fn outside() {}\n")?;
        let state = McpProjectState {
            root: canonical_project_root(&repo)?,
            db_path: repo.join(".projectatlas").join("projectatlas.db"),
            config_path: None,
            worktree: None,
        };

        let inside_key =
            ProjectAtlasMcpServer::absolute_path_key_in_selected_project(&state, &inside_file)?
                .ok_or_else(|| io::Error::other("inside selected root did not produce key"))?;
        require(
            inside_key == "src/lib.rs",
            "inside selected root produced wrong repo key",
        )?;
        require(
            ProjectAtlasMcpServer::absolute_path_key_in_selected_project(&state, &outside_file)?
                .is_none(),
            "outside selected root produced a repo key",
        )?;

        Ok(())
    }

    #[test]
    fn indexed_root_candidate_requires_matching_project_root()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        let other = temp.path().join("other");
        fs::create_dir_all(repo.join(".projectatlas"))?;
        fs::create_dir_all(&other)?;

        require(
            ProjectAtlasMcpServer::indexed_root_from_candidate(&repo).is_none(),
            "candidate without DB was treated as indexed",
        )?;

        let db_path = repo.join(".projectatlas").join("projectatlas.db");
        {
            let _store = open_atlas_store_for_project(&db_path, &other)?;
        }
        require(
            ProjectAtlasMcpServer::indexed_root_from_candidate(&repo).is_none(),
            "candidate with mismatched DB root was treated as indexed",
        )?;

        reset_index_files(&db_path, true, false, false)?;
        {
            let _store = open_atlas_store_for_project(&db_path, &repo)?;
        }
        let indexed = ProjectAtlasMcpServer::indexed_root_from_candidate(&repo)
            .ok_or_else(|| io::Error::other("matching DB root was not accepted"))?;
        require(
            indexed.root == canonical_project_root(&repo)?,
            "indexed root did not preserve canonical candidate root",
        )?;
        let expected_db_path =
            ProjectAtlasMcpServer::projectatlas_db_path(&canonical_project_root(&repo)?);
        require(
            indexed.db_path == expected_db_path,
            "indexed root changed DB path",
        )?;

        Ok(())
    }

    #[test]
    fn indexed_root_predecessor_candidate_supports_nearest_routing()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("predecessor");
        let source = root.join("src").join("lib.rs");
        let database = root.join(".projectatlas").join("projectatlas.db");
        fs::create_dir_all(
            source
                .parent()
                .ok_or_else(|| io::Error::other("predecessor source path has no parent"))?,
        )?;
        fs::create_dir_all(
            database
                .parent()
                .ok_or_else(|| io::Error::other("predecessor database path has no parent"))?,
        )?;
        fs::write(&source, "pub fn predecessor() {}\n")?;
        let store = open_atlas_store_for_project(&database, &root)?;
        drop(store);
        let predecessor = rusqlite::Connection::open(&database)?;
        predecessor.execute_batch(
            "DROP TABLE project_root_identity;
             UPDATE metadata SET value = '19' WHERE key = 'schema_version';",
        )?;
        drop(predecessor);

        let expected_root = canonical_project_root(&root)?;
        let expected_database = ProjectAtlasMcpServer::projectatlas_db_path(&expected_root);
        let canonical = ProjectAtlasMcpServer::project_state_from_nearest_indexed_path(&source)?
            .ok_or_else(|| io::Error::other("canonical nearest predecessor was not found"))?;
        let lexical =
            ProjectAtlasMcpServer::project_state_from_nearest_lexical_indexed_path(&source)?
                .ok_or_else(|| io::Error::other("lexical nearest predecessor was not found"))?;
        for state in [canonical, lexical] {
            require(
                state.root == expected_root && state.db_path == expected_database,
                "nearest predecessor routing changed the canonical root or database path",
            )?;
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn nearest_routing_recanonicalizes_case_only_root_rename()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let original = temp.path().join("NearestCaseRoot");
        let staging = temp.path().join("NearestCaseRootStaging");
        let renamed = temp.path().join("nearestcaseroot");
        let source = original.join("src").join("lib.rs");
        fs::create_dir_all(
            source
                .parent()
                .ok_or_else(|| io::Error::other("case-only nearest source has no parent"))?,
        )?;
        let database = original.join(".projectatlas").join("projectatlas.db");
        fs::create_dir_all(
            database
                .parent()
                .ok_or_else(|| io::Error::other("case-only nearest database has no parent"))?,
        )?;
        fs::write(&source, "pub fn nearest_case_only() {}\n")?;
        drop(open_atlas_store_for_project(&database, &original)?);

        fs::rename(&original, &staging)?;
        fs::rename(&staging, &renamed)?;
        let renamed_source = renamed.join("src").join("lib.rs");
        let expected_root = canonical_project_root(&renamed)?;
        let expected_database = ProjectAtlasMcpServer::projectatlas_db_path(&expected_root);
        let canonical =
            ProjectAtlasMcpServer::project_state_from_nearest_indexed_path(&renamed_source)?
                .ok_or_else(|| {
                    io::Error::other("canonical nearest case-only root was not found")
                })?;
        let lexical = ProjectAtlasMcpServer::project_state_from_nearest_lexical_indexed_path(
            &renamed_source,
        )?
        .ok_or_else(|| io::Error::other("lexical nearest case-only root was not found"))?;
        for state in [canonical, lexical] {
            require(
                state.root == expected_root && state.db_path == expected_database,
                "nearest routing rejected a case-only root rename",
            )?;
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn nearest_routing_rejects_case_sensitive_sibling() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let parent = temp.path().join("nearest-case-sensitive-parent");
        fs::create_dir(&parent)?;
        let enabled = StdCommand::new("fsutil")
            .args(["file", "SetCaseSensitiveInfo"])
            .arg(&parent)
            .arg("enable")
            .status()
            .is_ok_and(|status| status.success());
        if !enabled {
            return Ok(());
        }

        let stored_root = parent.join("Repo");
        let selected_root = parent.join("repo");
        fs::create_dir(&stored_root)?;
        fs::create_dir(&selected_root)?;
        let stored_database = stored_root.join(".projectatlas").join("projectatlas.db");
        let selected_database = selected_root.join(".projectatlas").join("projectatlas.db");
        fs::create_dir_all(
            stored_database
                .parent()
                .ok_or_else(|| io::Error::other("stored nearest database has no parent"))?,
        )?;
        fs::create_dir_all(
            selected_database
                .parent()
                .ok_or_else(|| io::Error::other("selected nearest database has no parent"))?,
        )?;
        drop(open_atlas_store_for_project(
            &stored_database,
            &stored_root,
        )?);
        fs::copy(&stored_database, &selected_database)?;

        require(
            ProjectAtlasMcpServer::indexed_root_from_candidate(&selected_root).is_none()
                && ProjectAtlasMcpServer::indexed_root_from_lexical_candidate(&selected_root)
                    .is_none(),
            "nearest routing accepted a distinct case-sensitive sibling",
        )?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn mcp_init_rejects_ambiguous_predecessor_before_project_writes()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let temp = tempfile::tempdir()?;
        let raw_root = temp
            .path()
            .join(OsString::from_vec(b"mcp-raw-root-\x80".to_vec()));
        let replacement_root = PathBuf::from(raw_root.to_string_lossy().into_owned());
        let database = raw_root
            .join(PROJECTATLAS_DIR_NAME)
            .join(PROJECTATLAS_DB_FILE_NAME);
        fs::create_dir_all(&raw_root)?;
        fs::create_dir_all(
            database
                .parent()
                .ok_or_else(|| io::Error::other("predecessor database has no parent"))?,
        )?;
        let store = AtlasStore::open_for_project(&database, &raw_root)?;
        drop(store);
        let predecessor = rusqlite::Connection::open(&database)?;
        predecessor.execute_batch(
            "DROP TABLE project_root_identity;
             UPDATE metadata SET value = '19' WHERE key = 'schema_version';",
        )?;
        predecessor.execute(
            "INSERT INTO metadata(key, value) VALUES('project_root', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [&replacement_root.to_string_lossy().into_owned()],
        )?;
        drop(predecessor);
        fs::create_dir_all(&replacement_root)?;

        // The startup root resolver opens the predecessor read-only. SQLite
        // may materialize WAL sidecars for that read path, so warm the exact
        // recovery route before capturing the no-mutation baseline. An
        // ambiguous legacy root must be refused rather than selected.
        require(
            default_mcp_project_root(&database, None).is_err(),
            "ambiguous predecessor was selected during startup discovery",
        )?;
        let database_before = fs::read(&database)?;
        let sidecars_before = ["wal", "shm", "journal"]
            .map(|suffix| fs::read(db_sidecar_path(&database, suffix)).ok());
        let replacement_project_dir = replacement_root.join(PROJECTATLAS_DIR_NAME);
        let replacement_config = replacement_project_dir.join(PROJECTATLAS_CONFIG_FILE_NAME);
        let replacement_nonsource = replacement_project_dir.join(MCP_NONSOURCE_FILE_NAME);
        let server = ProjectAtlasMcpServer::new(
            database.clone(),
            None,
            "ambiguous-predecessor".to_string(),
            false,
        );
        require(
            ProjectAtlasMcpServer::startup_project_state(database.clone(), None).root
                != canonical_project_root(&replacement_root)?,
            "MCP startup selected the replacement-character candidate",
        )?;
        let result = server.atlas_init(Parameters(AtlasInitParams {
            project_path: None,
            worktree: None,
            no_scan: Some(true),
            force_rescan: Some(false),
            text_index_max_bytes: None,
        }));
        require(
            result.contains("project-root identity"),
            &format!("ambiguous predecessor init returned an unexpected result: {result}"),
        )?;
        require(
            fs::read(&database)? == database_before
                && ["wal", "shm", "journal"]
                    .map(|suffix| fs::read(db_sidecar_path(&database, suffix)).ok())
                    == sidecars_before,
            "ambiguous predecessor init changed database or sidecar state",
        )?;
        require(
            !replacement_project_dir.exists()
                && !replacement_config.exists()
                && !replacement_nonsource.exists(),
            "ambiguous predecessor init wrote replacement-root project state",
        )?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn nearest_root_rejects_ambiguous_predecessor_without_mutation()
    -> Result<(), Box<dyn std::error::Error>> {
        fn sidecar_bytes(database: &Path) -> [Option<Vec<u8>>; 3] {
            ["wal", "shm", "journal"].map(|suffix| fs::read(db_sidecar_path(database, suffix)).ok())
        }

        fn directory_inventory(path: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
            let mut names = fs::read_dir(path)?
                .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
                .collect::<Result<Vec<_>, _>>()?;
            names.sort();
            Ok(names)
        }

        fn snapshot(
            database: &Path,
        ) -> Result<(Vec<u8>, [Option<Vec<u8>>; 3], Vec<String>), Box<dyn std::error::Error>>
        {
            let parent = database
                .parent()
                .ok_or_else(|| io::Error::other("predecessor database has no parent"))?;
            Ok((
                fs::read(database)?,
                sidecar_bytes(database),
                directory_inventory(parent)?,
            ))
        }

        let temp = tempfile::tempdir()?;
        let raw_root = temp
            .path()
            .join(std::ffi::OsString::from_vec(b"nearest-raw-\x80".to_vec()));
        let replacement_root = temp.path().join("nearest-raw-�");
        let raw_database = raw_root.join(".projectatlas").join("projectatlas.db");
        let replacement_database = replacement_root
            .join(".projectatlas")
            .join("projectatlas.db");
        fs::create_dir_all(
            raw_database
                .parent()
                .ok_or_else(|| io::Error::other("raw predecessor database has no parent"))?,
        )?;
        fs::create_dir_all(
            replacement_database.parent().ok_or_else(|| {
                io::Error::other("replacement predecessor database has no parent")
            })?,
        )?;
        let store = open_atlas_store_for_project(&raw_database, &raw_root)?;
        drop(store);
        let predecessor = rusqlite::Connection::open(&raw_database)?;
        predecessor.execute_batch(
            "DROP TABLE project_root_identity;
             UPDATE metadata SET value = '19' WHERE key = 'schema_version';",
        )?;
        drop(predecessor);
        fs::copy(&raw_database, &replacement_database)?;

        // A read-only WAL opener may materialize its sidecars. Warm the exact
        // recovery path before capturing the no-mutation baseline.
        for database in [&raw_database, &replacement_database] {
            let _ = read_legacy_project_root_candidate_read_only(database)?;
        }

        for (root, database) in [
            (&raw_root, &raw_database),
            (&replacement_root, &replacement_database),
        ] {
            let before = snapshot(database)?;
            require(
                ProjectAtlasMcpServer::indexed_root_from_candidate(root).is_none()
                    && ProjectAtlasMcpServer::indexed_root_from_lexical_candidate(root).is_none(),
                "ambiguous predecessor was admitted by nearest routing",
            )?;
            require(
                snapshot(database)? == before,
                "ambiguous predecessor detection changed database or sidecar state",
            )?;
        }
        Ok(())
    }
}
