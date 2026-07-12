//! Purpose: Serve `ProjectAtlas` repository intelligence over MCP.
//! Native MCP adapter for `ProjectAtlas` agent integrations.

use crate::atlas_map::{
    AtlasMapConfig, IgnoreEntryKind, LintOptions, add_ignore_entry, effective_config_report,
    init_gitignore, lint_map, list_ignore_entries, load_atlas_config, load_atlas_config_for_root,
    remove_ignore_entry, write_map,
};
use crate::runtime::{
    DEFAULT_HEALTH_LIMIT, InitBootstrapOptions, MAX_HEALTH_LIMIT, MAX_SYMBOL_FILE_BYTES,
    PurposeLintLevel, PurposeReviewRequest, ScanRuntimePlan, SymbolBuildOptions,
    build_settings_report, build_symbols_for_index, byte_count_to_tokens, canonical_project_root,
    config_root_mismatch_error, default_mcp_project_root,
    estimated_source_tokens_for_indexed_files, estimated_source_tokens_for_paths,
    file_summary_usage_baseline, init_config_path, lint_database_if_present, next_step_report,
    next_step_report_payload, normalized_folder_filter, open_atlas_store, purpose_curation_page,
    ranked_file_nodes_with_reasons, ranked_folder_nodes_with_reasons, read_indexed_file_content,
    record_directory_walk_usage_estimate, record_usage_estimate, record_usage_text,
    render_health_page, render_purpose_curation_page, render_purpose_review_report,
    reset_index_files, review_purposes, run_init_bootstrap, run_scan_pipeline, run_watch_loop,
    strip_legacy_purpose, telemetry_disabled, validated_indexed_file_key, watcher_status_report,
};
use crate::token_tui::{
    TokenDashboardTheme, render_token_dashboard_plain_with_theme,
    render_token_trend_dashboard_plain_with_theme,
};
use crate::{
    CliError, DEFAULT_FILE_SUMMARY_LIMIT, HarnessConfig, OutputFormat, RuntimeInfoReport,
    build_harness_mcp_config_report, build_parity_report, build_root_report, build_runtime_info,
    render_code_slice, render_file_summary, render_parity_report, render_root_report,
    render_runtime_info, render_search_report, render_watch_status,
};
use projectatlas_core::health::Severity;
use projectatlas_core::outline::build_outline;
use projectatlas_core::telemetry::TokenTrendWindow;
use projectatlas_core::toon::{
    encode_agent_payload, render_outline, render_overview, render_ranked_nodes,
    render_symbol_relations, render_symbols, render_token_overview, render_token_trends,
};
use projectatlas_core::{
    Overview, PurposeSource, PurposeStatus, RankedNode, normalize_native_path_display,
    normalize_repo_path, normalize_repo_path_prefix, validated_repo_node_key,
};
use projectatlas_db::{
    AtlasStore, HealthQuery, HealthResolution, HealthScope, read_project_root_read_only,
};
use projectatlas_service::{
    SymbolSliceSelector, build_file_summary, read_indexed_code_slice, read_symbol_slice,
    search_indexed_files,
};
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::schemars;
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// MCP tools required for the agent-first repository-intelligence surface.
pub(crate) const REQUIRED_MCP_TOOL_NAMES: &[&str] = &[
    MCP_TOOL_ATLAS_SET_PROJECT_PATH,
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
/// Default MCP config server key.
const MCP_DEFAULT_CONFIG_SERVER_NAME: &str = "projectatlas";
/// Missing-index recovery guidance for MCP tools.
const MISSING_INDEX_GUIDANCE: &str =
    "run atlas_scan with project_path or atlas_set_project_path first";
/// Recovery guidance when a path names a subfolder rather than another selected root.
const SELECTED_ROOT_ASSERTION_GUIDANCE: &str = "pass project_path or call atlas_set_project_path for another repository, or use normal filesystem tools such as Get-Content or rg for files inside the selected project";
/// Recovery guidance when a path escapes the selected `ProjectAtlas` root.
const OUTSIDE_SELECTED_PROJECT_GUIDANCE: &str = "pass project_path or call atlas_set_project_path for that repository, or use normal filesystem tools such as Get-Content or rg for files outside the selected ProjectAtlas project";
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
/// MCP payload key for project initialization reports.
const MCP_PAYLOAD_INIT: &str = "init";
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
/// MCP payload key for task status lookups.
const MCP_PAYLOAD_TASK_STATUS: &str = "task_status";
/// MCP payload key for task cancellation responses.
const MCP_PAYLOAD_TASK_CANCEL: &str = "task_cancel";
/// MCP session capability payload key.
const MCP_PAYLOAD_SESSION_CAPABILITIES: &str = "mcp_session";
/// Session-brief argument key for per-call project roots.
const MCP_BRIEF_ARG_PROJECT_PATH: &str = "project_path";
/// Session-brief argument key for ranked query text.
const MCP_BRIEF_ARG_QUERY: &str = "query";
/// Session-brief argument key for row limits.
const MCP_BRIEF_ARG_LIMIT: &str = "limit";
/// Session-brief recommendation target for normal filesystem reads.
const MCP_BRIEF_TARGET_FILESYSTEM_TOOLS: &str = "filesystem_tools";
/// Session-brief reason for missing selected indexes.
const MCP_BRIEF_REASON_SELECTED_INDEX_MISSING: &str = "selected_index_missing";
/// Session-brief reason for filesystem fallback before an index exists.
const MCP_BRIEF_REASON_FILESYSTEM_UNTIL_INDEX: &str =
    "use_filesystem_until_projectatlas_index_exists";
/// Session-brief reason for choosing folders first.
const MCP_BRIEF_REASON_CHOOSE_WORK_AREA: &str = "choose_work_area_before_source_reads";
/// Session-brief reason for choosing files before details.
const MCP_BRIEF_REASON_CHOOSE_FILES: &str = "choose_files_before_summary_or_slice";
/// Session-brief reason for health follow-up.
const MCP_BRIEF_REASON_HEALTH_BLOCKERS: &str = "unresolved_health_blockers_present";
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
/// Token chart theme validation error prefix.
const TOKEN_CHART_THEME_ERROR_PREFIX: &str = "unsupported token chart theme ";
/// Token chart theme validation error suffix.
const TOKEN_CHART_THEME_ERROR_SUFFIX: &str = "; expected dark or light";
/// Watch-status recommendation when no index exists.
const WATCH_STATUS_SCAN_RECOMMENDATION: &str =
    " Run `atlas_scan` first when no ProjectAtlas index exists for this project.";
/// Default number of rows in an agent startup brief section.
const SESSION_BRIEF_DEFAULT_LIMIT: usize = 5;
/// Maximum number of rows in an agent startup brief section.
const SESSION_BRIEF_MAX_LIMIT: usize = 8;
/// Bounded MCP task registry capacity.
const MCP_TASK_REGISTRY_CAPACITY: usize = 32;
/// Built-in task id that exposes the task-progress contract itself.
const MCP_TASK_CONTRACT_ID: &str = "task-progress-contract";
/// Agent-facing MCP server instructions.
const MCP_SERVER_INSTRUCTIONS: &str = "ProjectAtlas provides TOON-first repository orientation, folder/file ranking, structured file summaries, symbol graph lookup, exact slices, health checks, and token telemetry for coding agents.";

/// Optional active-project override accepted by MCP tools.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasProjectParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
}

/// MCP parameter payload for compact agent startup briefs.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasSessionBriefParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Optional task query used for folder and file ranking.
    query: Option<String>,
    /// Maximum folder candidates to return.
    folder_limit: Option<usize>,
    /// Maximum file candidates to return.
    file_limit: Option<usize>,
    /// Maximum health blockers to return.
    blocker_limit: Option<usize>,
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

/// MCP parameter payload for initializing a project.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasInitParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
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
    /// Return the same report shape with `verified` available for gating.
    verify: Option<bool>,
}

/// MCP parameter payload for binding a root.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasRootSetParams {
    /// Project root to bind and make active for later calls.
    root: String,
    /// Include mcp --nearest-project in generated project-local MCP configs.
    nearest_project: Option<bool>,
}

/// MCP parameter payload for ignore mutations.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasIgnoreMutationParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
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
}

/// MCP parameter payload for one-shot watcher refresh.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasWatchOnceParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
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
}

/// MCP parameter payload for ranked node lookup.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasQueryParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
    /// Search query for path and purpose matching.
    query: Option<String>,
    /// Maximum number of rows to return.
    limit: Option<usize>,
}

/// MCP parameter payload for ranked file lookup with optional absolute folder routing.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AtlasFilesParams {
    /// Optional project root for this call. Defaults to the active MCP project.
    project_path: Option<String>,
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
    /// Repository-relative file path.
    file: String,
    /// Opt in to nearest indexed `ProjectAtlas` project discovery for absolute file paths.
    nearest_project: Option<bool>,
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
    /// Opt in to nearest indexed `ProjectAtlas` project discovery for absolute file paths.
    nearest_project: Option<bool>,
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
    /// Optional chart theme for TUI output: dark or light.
    theme: Option<String>,
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

/// MCP response for compatibility map export.
#[derive(Debug, Serialize)]
struct McpMapReport {
    /// Canonical project root used for map generation.
    root: String,
    /// Map path from the effective config.
    map_path: String,
    /// Whether a map file was written by this call.
    written: bool,
    /// Whether JSON compatibility output was requested.
    json: bool,
    /// Human-readable reason when no file was written.
    skipped_reason: Option<String>,
}

/// MCP response for lint reports.
#[derive(Debug, Serialize)]
struct McpLintReport {
    /// Whether lint passed.
    ok: bool,
    /// CLI-compatible exit code that callers can gate on.
    exit_code: i32,
    /// Combined lint report text.
    report: String,
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
    /// Canonicalize a user-supplied absolute path argument.
    fn canonicalize(path: &Path) -> Result<Self, CliError> {
        canonical_project_root(path).map(Self)
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
    /// Token telemetry write mode for this process.
    telemetry: McpTelemetryPolicy,
    /// Privacy guarantees for this payload.
    privacy: McpPrivacyPolicy,
}

/// Selected project identity inside capability/settings payloads.
#[derive(Debug, Serialize)]
struct McpSelectedProjectCapability {
    /// Canonical repository root.
    root: String,
    /// Selected durable `SQLite` index path.
    db: String,
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
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpPolicyState {
    /// Policy is enabled.
    Enabled,
    /// Policy is disabled.
    Disabled,
}

/// Selected index availability.
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpIndexStatus {
    /// The selected index file exists.
    Available,
    /// The selected index file is missing.
    Missing,
}

/// Absolute path routing scope.
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpPathScope {
    /// Calls stay within the selected project.
    SelectedProject,
    /// Absolute paths may route to the nearest indexed project.
    NearestIndexedProject,
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
    /// Recommended next calls.
    recommendations: Vec<McpBriefRecommendation>,
    /// Effective limits and truncation metadata.
    limits: McpBriefLimits,
}

/// Brief policy fields.
#[derive(Debug, Serialize)]
struct McpBriefPolicy {
    /// Whether nearest indexed project routing is enabled by default.
    nearest_project: McpPolicyState,
    /// Absolute-path routing scope.
    path_scope: McpPathScope,
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
    /// Purpose one-liner when present.
    purpose: Option<String>,
    /// Observed content summary when present.
    summary: Option<String>,
    /// Bounded ranking reasons.
    reasons: Vec<String>,
}

/// Bounded health blocker section.
#[derive(Debug, Serialize)]
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

/// One health blocker row for startup briefs.
#[derive(Debug, Serialize)]
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

/// Startup recommendation kinds.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpBriefRecommendationKind {
    /// Refresh or create the `ProjectAtlas` index.
    Scan,
    /// Rank folders.
    Folders,
    /// Rank files.
    Files,
    /// Inspect structural health.
    Health,
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
    /// Whether folder candidates were truncated.
    folders_truncated: bool,
    /// Whether file candidates were truncated.
    files_truncated: bool,
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
    /// Future scan operation.
    Scan,
    /// Future one-shot watch refresh operation.
    WatchOnce,
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

/// Task cancellation outcome.
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum McpTaskCancelResult {
    /// The task was canceled.
    Canceled,
    /// The task id is unknown to this MCP session.
    NotFound,
    /// The task was already finished.
    AlreadyFinished,
    /// The task exists but cannot currently be canceled.
    NotCancelable,
}

/// Native `ProjectAtlas` MCP server backed by the same services as the CLI.
#[derive(Debug, Clone)]
pub(crate) struct ProjectAtlasMcpServer {
    /// Active project state for calls that omit `project_path`.
    project_state: Arc<RwLock<McpProjectState>>,
    /// Token telemetry session id.
    session: String,
    /// Whether absolute path arguments may select the nearest indexed project by default.
    allow_nearest_project: bool,
    /// Bounded MCP task-progress records for this server session.
    task_registry: Arc<RwLock<McpTaskRegistry>>,
    /// Official RMCP tool router.
    tool_router: ToolRouter<Self>,
}

impl ProjectAtlasMcpServer {
    /// Create a `ProjectAtlas` MCP server instance.
    pub(crate) fn new(
        db_path: PathBuf,
        config_path: Option<PathBuf>,
        session: String,
        allow_nearest_project: bool,
    ) -> Self {
        Self {
            project_state: Arc::new(RwLock::new(Self::startup_project_state(
                db_path,
                config_path,
            ))),
            session,
            allow_nearest_project,
            task_registry: Arc::new(RwLock::new(McpTaskRegistry::new())),
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

    /// Load effective atlas config for the selected state.
    fn load_config_for_state(state: &McpProjectState) -> Result<AtlasMapConfig, CliError> {
        state.config_path.as_deref().map_or_else(
            || load_atlas_config_for_root(&state.root).map_err(CliError::from),
            |config_path| load_atlas_config(Some(config_path)).map_err(CliError::from),
        )
    }

    /// Return the selected project root used by admin-style MCP calls.
    fn admin_project_root(
        &self,
        project_path: Option<String>,
    ) -> Result<McpProjectState, CliError> {
        self.state_for_project_path(project_path)
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
            root: normalize_native_path_display(&config.root),
            map_path: normalize_native_path_display(&config.map_path),
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
        Self::encode_two_named_payloads(
            MCP_PAYLOAD_SETTINGS,
            &report,
            MCP_PAYLOAD_SESSION_CAPABILITIES,
            &capabilities,
        )
    }

    /// Build the selected-project capability row.
    fn selected_project_capability(state: &McpProjectState) -> McpSelectedProjectCapability {
        McpSelectedProjectCapability {
            root: normalize_native_path_display(&state.root),
            db: normalize_native_path_display(&state.db_path),
            config: state
                .config_path
                .as_ref()
                .map(normalize_native_path_display),
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
    ) -> Result<McpSessionBrief, CliError> {
        let selected_project_path = params.project_path.clone();
        let state = self.state_for_project_path(selected_project_path.clone())?;
        let query = Self::query_or_empty(params.query);
        let folder_limit = Self::brief_limit(params.folder_limit);
        let file_limit = Self::brief_limit(params.file_limit);
        let blocker_limit = Self::brief_limit(params.blocker_limit);
        let project = Self::selected_project_capability(&state);
        if !state.db_path.exists() {
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
                recommendations: Self::missing_index_recommendations(params.project_path),
                limits: McpBriefLimits {
                    folder_limit,
                    file_limit,
                    blocker_limit,
                    folders_truncated: false,
                    files_truncated: false,
                },
            });
        }
        let store = Self::open_store(&state)?;
        let overview = store.overview()?;
        let folder_rows =
            ranked_folder_nodes_with_reasons(&store, &query, folder_limit.saturating_add(1))?;
        let file_rows = ranked_file_nodes_with_reasons(
            &store,
            &query,
            None,
            None,
            file_limit.saturating_add(1),
            false,
        )?;
        let blockers = Self::brief_blockers(&store, blocker_limit)?;
        let folders_truncated = folder_rows.len() > folder_limit;
        let files_truncated = file_rows.len() > file_limit;
        Ok(McpSessionBrief {
            project,
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
                blockers.total,
                blocker_limit,
                selected_project_path,
            ),
            blockers,
            limits: McpBriefLimits {
                folder_limit,
                file_limit,
                blocker_limit,
                folders_truncated,
                files_truncated,
            },
        })
    }

    /// Build brief policy fields.
    fn brief_policy(&self) -> McpBriefPolicy {
        McpBriefPolicy {
            nearest_project: Self::policy_state(self.allow_nearest_project),
            path_scope: self.path_scope(),
        }
    }

    /// Convert a ranked node into a compact startup candidate.
    fn brief_candidate(row: RankedNode) -> McpBriefCandidate {
        McpBriefCandidate {
            path: row.node.node.path,
            kind: row.node.node.kind.to_string(),
            purpose_status: row.node.purpose.status,
            purpose_source: row.node.purpose.source,
            purpose: row.node.purpose.purpose,
            summary: row.node.summary,
            reasons: row.reasons,
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
        let page = store.unresolved_health_findings_page(&store.resolved_health_ids()?, &query)?;
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
    fn missing_index_recommendations(project_path: Option<String>) -> Vec<McpBriefRecommendation> {
        vec![
            McpBriefRecommendation {
                kind: McpBriefRecommendationKind::Scan,
                target: MCP_TOOL_ATLAS_SCAN.to_string(),
                reason: MCP_BRIEF_REASON_SELECTED_INDEX_MISSING.to_string(),
                arguments: Self::project_path_arguments(project_path.clone()),
            },
            McpBriefRecommendation {
                kind: McpBriefRecommendationKind::FilesystemTools,
                target: MCP_BRIEF_TARGET_FILESYSTEM_TOOLS.to_string(),
                reason: MCP_BRIEF_REASON_FILESYSTEM_UNTIL_INDEX.to_string(),
                arguments: Self::project_path_arguments(project_path),
            },
        ]
    }

    /// Recommend next calls for an indexed project.
    fn indexed_project_recommendations(
        query: &str,
        blocker_total: usize,
        blocker_limit: usize,
        project_path: Option<String>,
    ) -> Vec<McpBriefRecommendation> {
        let mut recommendations = vec![
            McpBriefRecommendation {
                kind: McpBriefRecommendationKind::Folders,
                target: MCP_TOOL_ATLAS_FOLDERS.to_string(),
                reason: MCP_BRIEF_REASON_CHOOSE_WORK_AREA.to_string(),
                arguments: Self::brief_call_arguments(
                    project_path.clone(),
                    Some((MCP_BRIEF_ARG_QUERY, query)),
                    None,
                ),
            },
            McpBriefRecommendation {
                kind: McpBriefRecommendationKind::Files,
                target: MCP_TOOL_ATLAS_FILES.to_string(),
                reason: MCP_BRIEF_REASON_CHOOSE_FILES.to_string(),
                arguments: Self::brief_call_arguments(
                    project_path.clone(),
                    Some((MCP_BRIEF_ARG_QUERY, query)),
                    None,
                ),
            },
        ];
        if blocker_total > 0 {
            recommendations.push(McpBriefRecommendation {
                kind: McpBriefRecommendationKind::Health,
                target: MCP_TOOL_ATLAS_HEALTH.to_string(),
                reason: MCP_BRIEF_REASON_HEALTH_BLOCKERS.to_string(),
                arguments: Self::brief_call_arguments(
                    project_path,
                    None,
                    Some((MCP_BRIEF_ARG_LIMIT, blocker_limit)),
                ),
            });
        }
        recommendations
    }

    /// Build a `JSON` object containing `project_path` when present.
    fn project_path_arguments(project_path: Option<String>) -> serde_json::Value {
        project_path.map_or_else(
            || serde_json::Value::Object(serde_json::Map::new()),
            |path| Self::string_argument(MCP_BRIEF_ARG_PROJECT_PATH, path),
        )
    }

    /// Build recommendation call arguments with optional project path and one payload argument.
    fn brief_call_arguments(
        project_path: Option<String>,
        string_arg: Option<(&'static str, &str)>,
        usize_arg: Option<(&'static str, usize)>,
    ) -> serde_json::Value {
        let mut arguments = serde_json::Map::new();
        if let Some(path) = project_path {
            arguments.insert(
                MCP_BRIEF_ARG_PROJECT_PATH.to_string(),
                serde_json::Value::String(path),
            );
        }
        if let Some((key, value)) = string_arg {
            arguments.insert(
                key.to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
        if let Some((key, value)) = usize_arg {
            arguments.insert(key.to_string(), serde_json::json!(value));
        }
        serde_json::Value::Object(arguments)
    }

    /// Build a one-field string argument object for typed recommendations.
    fn string_argument(key: &'static str, value: impl Into<String>) -> serde_json::Value {
        let mut arguments = serde_json::Map::new();
        arguments.insert(key.to_string(), serde_json::Value::String(value.into()));
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
            McpTaskOperation::Search,
        ]
    }

    /// Look up one MCP task status.
    fn task_status(&self, task_id: String) -> Result<McpTaskStatusResponse, CliError> {
        let registry = self
            .task_registry
            .read()
            .map_err(|_poisoned| CliError::Mcp(MCP_PROJECT_STATE_LOCK_POISONED.to_string()))?;
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
            .map_err(|_poisoned| CliError::Mcp(MCP_PROJECT_STATE_LOCK_POISONED.to_string()))?;
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
            record.state = McpTaskState::Canceled;
            record.updated_at_ms = mcp_unix_time_ms();
        });
        Ok(McpTaskCancelResponse {
            task_id,
            result: McpTaskCancelResult::Canceled,
            registry_capacity: MCP_TASK_REGISTRY_CAPACITY,
            task,
        })
    }

    /// Build a CLI-compatible lint report for MCP callers.
    fn lint_report_for_state(
        state: &McpProjectState,
        params: &AtlasLintParams,
    ) -> Result<McpLintReport, CliError> {
        let config = Self::load_config_for_state(state)?;
        let (mut report, mut exit_code) = lint_map(
            &config,
            LintOptions {
                strict_folders: params.strict_folders.unwrap_or(false),
                report_untracked: params.report_untracked.unwrap_or(false),
                strict_untracked: params.strict_untracked.unwrap_or(false),
            },
        )?;
        let purpose_level = Self::parse_purpose_lint_level(params.purpose_level.as_deref())?;
        let (db_report, db_exit_code) = lint_database_if_present(&state.db_path, purpose_level)?;
        if !db_report.is_empty() {
            if !report.ends_with('\n') {
                report.push('\n');
            }
            report.push_str(&db_report);
        }
        exit_code = exit_code.max(db_exit_code);
        Ok(McpLintReport {
            ok: exit_code == 0,
            exit_code,
            report,
        })
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

    /// Return the nearest-project policy for one call, honoring explicit overrides.
    fn nearest_project_enabled(&self, override_value: Option<bool>) -> bool {
        override_value.unwrap_or(self.allow_nearest_project)
    }

    /// Return selected state and validate an optional root assertion.
    fn state_and_root_path(
        &self,
        project_path: Option<String>,
        path: Option<String>,
        nearest_project: bool,
    ) -> Result<(McpProjectState, PathBuf), CliError> {
        let state = self.state_for_project_path(project_path.clone())?;
        let root = match (
            Self::normalized_optional_path(project_path),
            Self::normalized_optional_path(path),
        ) {
            (None, Some(path)) => match Self::path_or_project_root(&state, Some(path.clone())) {
                Ok(root) => root,
                Err(active_error) => {
                    if !nearest_project {
                        return Err(active_error);
                    }
                    if Self::absolute_path_inside_selected_root(&state, &path)? {
                        return Err(active_error);
                    }
                    let Some(indexed_state) =
                        Self::nearest_root_state_for_root_argument(Path::new(&path))?
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

    /// Return nearest indexed state only when the addressed path is the project root itself.
    fn nearest_root_state_for_root_argument(
        path: &Path,
    ) -> Result<Option<McpProjectState>, CliError> {
        let Ok(addressed_root) = canonical_project_root(path) else {
            return Ok(None);
        };
        let Some(indexed_state) = Self::project_state_from_nearest_indexed_path(path)? else {
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
        file: &str,
        nearest_project: bool,
    ) -> Result<McpResolvedRepoPath, CliError> {
        let state = self.state_for_project_path(project_path.map(ToString::to_string))?;
        let file_path = PathBuf::from(&file);
        if !file_path.is_absolute() {
            let store = Self::open_store(&state)?;
            let file_key = validated_indexed_file_key(&store, &file_path)?;
            return Ok(McpResolvedRepoPath {
                state,
                key: file_key,
                routed_project: false,
            });
        }
        if nearest_project && project_path.is_none() {
            let resolved = Self::nearest_state_and_repo_key(&state, file)?.ok_or_else(|| {
                Self::selected_project_path_error(PATH_NOT_INSIDE_INDEXED_PROJECT_ERROR)
            })?;
            let store = Self::open_store(&resolved.state)?;
            let file_key = validated_indexed_file_key(&store, Path::new(&resolved.key))?;
            return Ok(McpResolvedRepoPath {
                key: file_key,
                ..resolved
            });
        }
        if let Some(file_key) = Self::absolute_path_key_in_selected_project(&state, &file_path)? {
            let store = Self::open_store(&state)?;
            let file_key = validated_indexed_file_key(&store, Path::new(&file_key))?;
            return Ok(McpResolvedRepoPath {
                state,
                key: file_key,
                routed_project: false,
            });
        }
        if project_path.is_some() {
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
        file: Option<&str>,
        nearest_project: bool,
    ) -> Result<(McpProjectState, Option<String>, bool), CliError> {
        let Some(file) = file else {
            return self
                .state_for_project_path(project_path.map(ToString::to_string))
                .map(|state| (state, None, false));
        };
        let resolved = self.state_and_file_key(project_path, file, nearest_project)?;
        Ok((resolved.state, Some(resolved.key), resolved.routed_project))
    }

    /// Return state and an optional folder filter for MCP file-ranking arguments.
    fn state_and_optional_folder_filter(
        &self,
        project_path: Option<&str>,
        folder: Option<&str>,
        nearest_project: bool,
    ) -> Result<(McpProjectState, Option<String>, bool), CliError> {
        let state = self.state_for_project_path(project_path.map(ToString::to_string))?;
        let Some(folder) = folder.map(str::trim).filter(|folder| !folder.is_empty()) else {
            return Ok((state, None, false));
        };
        let folder_path = PathBuf::from(&folder);
        if !folder_path.is_absolute() {
            let folder_filter = normalized_folder_filter(folder)?;
            return Ok((state, Some(folder_filter), false));
        }
        if nearest_project && project_path.is_none() {
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
        if project_path.is_some() {
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

    /// Build project state from the nearest indexed ancestor of an addressed path.
    fn project_state_from_nearest_indexed_path(
        path: &Path,
    ) -> Result<Option<McpProjectState>, CliError> {
        let Ok(absolute_path) = McpAbsolutePath::canonicalize(path) else {
            return Ok(None);
        };
        let mut candidate = absolute_path.nearest_search_start();
        loop {
            if let Some(indexed_root) = Self::indexed_root_from_candidate(candidate) {
                let config_path = Self::config_path_for_project_root(&indexed_root.root)?;
                return Ok(Some(McpProjectState {
                    root: indexed_root.root,
                    db_path: indexed_root.db_path,
                    config_path,
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
                let config_path = Self::config_path_for_project_root(&indexed_root.root)?;
                return Ok(Some(McpProjectState {
                    root: indexed_root.root,
                    db_path: indexed_root.db_path,
                    config_path,
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
        let db_path = Self::projectatlas_db_path(&root);
        if !db_path.is_file() || !Self::indexed_db_matches_root(&db_path, &root) {
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
        if normalize_native_path_display(candidate) != normalize_native_path_display(&root) {
            return None;
        }
        let db_path = Self::projectatlas_db_path(&root);
        if !db_path.is_file() || !Self::indexed_db_matches_root(&db_path, &root) {
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

    /// Return whether an existing DB records the same canonical project root.
    fn indexed_db_matches_root(db_path: &Path, root: &Path) -> bool {
        let Ok(stored_root) = read_project_root_read_only(db_path) else {
            return false;
        };
        let Some(stored_root) = stored_root else {
            return false;
        };
        let Ok(stored_root) = canonical_project_root(Path::new(&stored_root)) else {
            return false;
        };
        let Ok(candidate_root) = canonical_project_root(root) else {
            return false;
        };
        stored_root == candidate_root
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
            project: Self::project_state_payload(state),
        };
        Self::encode_serialized_payload(payload)
    }

    /// Build selected-project payload fields.
    fn project_state_payload(state: &McpProjectState) -> McpProjectStatePayload {
        McpProjectStatePayload {
            root: normalize_native_path_display(&state.root),
            db: normalize_native_path_display(&state.db_path),
            config: state
                .config_path
                .as_ref()
                .map(normalize_native_path_display),
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

    /// Initialize a `ProjectAtlas` project-local config surface.
    #[tool(
        name = "atlas_init",
        description = "Initialize ProjectAtlas project-local config, database, host MCP configs, scan/index, and purpose handoff."
    )]
    fn atlas_init(&self, Parameters(params): Parameters<AtlasInitParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.admin_project_root(params.project_path)?;
            let config_path = init_config_path(&state.root, state.config_path.as_deref());
            let mut report = run_init_bootstrap(
                &state.root,
                &state.db_path,
                Some(&config_path),
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
    fn atlas_map(&self, Parameters(params): Parameters<AtlasMapParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.admin_project_root(params.project_path)?;
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
    fn atlas_root(&self, Parameters(params): Parameters<AtlasRootParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.admin_project_root(params.project_path)?;
            let report = build_root_report(&state.db_path, state.config_path.as_deref())?;
            if params.verify.unwrap_or(false) && !report.verified {
                return Ok(render_root_report(&report));
            }
            Ok(render_root_report(&report))
        })())
    }

    /// Bind a project root and make it active for subsequent defaulted calls.
    #[tool(
        name = "atlas_root_set",
        description = "Bind a repository root, generate project-local MCP configs, and make it active for later MCP calls."
    )]
    fn atlas_root_set(&self, Parameters(params): Parameters<AtlasRootSetParams>) -> String {
        Self::as_mcp_text((|| {
            let root = canonical_project_root(Path::new(&params.root))?;
            let report = crate::bind_project_root(&root, params.nearest_project.unwrap_or(false))?;
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
    fn atlas_config(&self, Parameters(params): Parameters<AtlasProjectParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.admin_project_root(params.project_path)?;
            let report = effective_config_report(&Self::load_config_for_state(&state)?);
            Self::encode_named_payload(MCP_PAYLOAD_CONFIG, &report)
        })())
    }

    /// Return the effective `ProjectAtlas` ignore policy.
    #[tool(
        name = "atlas_ignore_list",
        description = "List effective ProjectAtlas manual ignore policy and inherited .gitignore status."
    )]
    fn atlas_ignore_list(&self, Parameters(params): Parameters<AtlasProjectParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.admin_project_root(params.project_path)?;
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
    ) -> String {
        Self::as_mcp_text((|| {
            let state = self.admin_project_root(params.project_path)?;
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
    ) -> String {
        Self::as_mcp_text((|| {
            let state = self.admin_project_root(params.project_path)?;
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
    ) -> String {
        Self::as_mcp_text((|| {
            let state = self.admin_project_root(params.project_path)?;
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
    fn atlas_scan(&self, Parameters(params): Parameters<AtlasScanParams>) -> String {
        Self::as_mcp_text((|| {
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let (state, path) =
                self.state_and_root_path(params.project_path, params.path, nearest_project)?;
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
            let selected =
                ranked_folder_nodes_with_reasons(&store, &query, params.limit.unwrap_or(10))?;
            let toon = render_ranked_nodes(NODE_LABEL_FOLDERS, &selected);
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
    fn atlas_files(&self, Parameters(params): Parameters<AtlasFilesParams>) -> String {
        Self::as_mcp_text((|| {
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let (state, folder_filter, routed_project) = self.state_and_optional_folder_filter(
                params.project_path.as_deref(),
                params.folder.as_deref(),
                nearest_project,
            )?;
            let store = Self::open_store(&state)?;
            let query = Self::query_or_empty(params.query);
            let selected = ranked_file_nodes_with_reasons(
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
            let toon = Self::with_selected_project_audit(
                &state,
                routed_project,
                render_ranked_nodes(NODE_LABEL_FILES, &selected),
            )?;
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

    /// Recommend the next indexed folders, files, and inspection commands.
    #[tool(
        name = "atlas_next",
        description = "Recommend top indexed folders/files with reasons and deterministic follow-up commands for a task query."
    )]
    fn atlas_next(&self, Parameters(params): Parameters<AtlasQueryParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.state_for_project_path(params.project_path)?;
            let store = Self::open_store(&state)?;
            let query = Self::query_or_empty(params.query);
            let report = next_step_report(&store, &query, params.limit)?;
            let payload = next_step_report_payload(&report);
            let toon = Self::encode_named_payload(MCP_PAYLOAD_NEXT, &payload)?;
            record_directory_walk_usage_estimate(
                &store,
                &self.session,
                MCP_EVENT_ATLAS_NEXT,
                None,
                Some(query),
                estimated_source_tokens_for_indexed_files(&store, None, None)?,
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
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let resolved = self.state_and_file_key(
                params.project_path.as_deref(),
                &params.file,
                nearest_project,
            )?;
            let state = resolved.state;
            let file_key = resolved.key;
            let store = Self::open_store(&state)?;
            let content = read_indexed_file_content(&store, &file_key)?;
            let language = store
                .load_node_by_path(&file_key)?
                .and_then(|node| node.node.language);
            let outline = build_outline(&file_key, language, &content, params.lines.unwrap_or(12));
            let toon = Self::with_selected_project_audit(
                &state,
                resolved.routed_project,
                render_outline(&outline),
            )?;
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
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let resolved = self.state_and_file_key(
                params.project_path.as_deref(),
                &params.file,
                nearest_project,
            )?;
            let state = resolved.state;
            let file_key = resolved.key;
            let store = Self::open_store(&state)?;
            let report = build_file_summary(
                &store,
                Path::new(&file_key),
                params.limit.unwrap_or(DEFAULT_FILE_SUMMARY_LIMIT),
            )?;
            let toon = Self::with_selected_project_audit(
                &state,
                resolved.routed_project,
                render_file_summary(&report),
            )?;
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
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let resolved = self.state_and_file_key(
                params.project_path.as_deref(),
                &params.file,
                nearest_project,
            )?;
            let state = resolved.state;
            let file_key = resolved.key;
            let file = PathBuf::from(&file_key);
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
            let toon = Self::with_selected_project_audit(
                &state,
                resolved.routed_project,
                render_code_slice(&report),
            )?;
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
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let (state, path) =
                self.state_and_root_path(params.project_path, params.path, nearest_project)?;
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
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let (state, file, routed_project) = self.state_and_optional_file_key(
                params.project_path.as_deref(),
                params.file.as_deref(),
                nearest_project,
            )?;
            let store = Self::open_store(&state)?;
            let symbols = store.load_symbols(
                file.as_deref(),
                params.query.as_deref(),
                params.limit.unwrap_or(50),
            )?;
            let toon = Self::with_selected_project_audit(
                &state,
                routed_project,
                render_symbols(&symbols),
            )?;
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
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let (state, file, routed_project) = self.state_and_optional_file_key(
                params.project_path.as_deref(),
                params.file.as_deref(),
                nearest_project,
            )?;
            let store = Self::open_store(&state)?;
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

    /// Run `ProjectAtlas` lint checks without terminating the MCP transport.
    #[tool(
        name = "atlas_lint",
        description = "Run ProjectAtlas lint checks and return an ok flag, CLI-compatible exit code, and report text."
    )]
    fn atlas_lint(&self, Parameters(params): Parameters<AtlasLintParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.admin_project_root(params.project_path.clone())?;
            let report = Self::lint_report_for_state(&state, &params)?;
            Self::encode_named_payload(MCP_PAYLOAD_LINT, &report)
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
            let chart_theme = Self::parse_token_chart_theme(params.theme.as_deref())?;
            if let Some(window) = params.trend_window.as_deref() {
                let window = TokenTrendWindow::parse(window).ok_or_else(|| {
                    CliError::InvalidInput(format!(
                        "unsupported token trend window {window:?}; {TOKEN_TREND_WINDOW_ERROR_SUFFIX}"
                    ))
                })?;
                let report = store.token_trends(params.session.as_deref(), window)?;
                if include_chart {
                    let chart = render_token_trend_dashboard_plain_with_theme(&report, chart_theme);
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
                let chart = render_token_dashboard_plain_with_theme(
                    &overview,
                    params.session.as_deref(),
                    chart_theme,
                );
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
            self.render_settings_with_capabilities(&state)
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
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let (state, path) =
                self.state_and_root_path(params.project_path, params.path, nearest_project)?;
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
            let nearest_project = self.nearest_project_enabled(params.nearest_project);
            let (state, path) =
                self.state_and_root_path(params.project_path, params.path, nearest_project)?;
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

    /// Generate a project-local MCP config document.
    #[tool(
        name = "atlas_mcp_config",
        description = "Return a generated ProjectAtlas MCP config document for mcp-json, codex, claude-code, or opencode hosts."
    )]
    fn atlas_mcp_config(&self, Parameters(params): Parameters<AtlasMcpConfigParams>) -> String {
        Self::as_mcp_text((|| {
            let state = self.admin_project_root(params.project_path)?;
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
    fn atlas_runtime_info(&self, Parameters(_params): Parameters<AtlasProjectParams>) -> String {
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
    ) -> String {
        Self::as_mcp_text((|| {
            let brief = self.build_session_brief(params)?;
            Self::encode_named_payload(MCP_PAYLOAD_SESSION_BRIEF, &brief)
        })())
    }

    /// Return typed status for one MCP task-progress record.
    #[tool(
        name = "atlas_task_status",
        description = "Return typed status for a bounded MCP task-progress record."
    )]
    fn atlas_task_status(&self, Parameters(params): Parameters<AtlasTaskParams>) -> String {
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
    fn atlas_task_cancel(&self, Parameters(params): Parameters<AtlasTaskParams>) -> String {
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

/// Return current Unix time in milliseconds for MCP task status records.
fn mcp_unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
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
        let server = ProjectAtlasMcpServer::new(db_path, None, "mcp-test".to_string(), false);
        let expected_root = canonical_project_root(&repo)?;

        let (_state, root) = server.state_and_root_path(None, Some("./".to_string()), false)?;
        require(
            root == expected_root,
            "current-dir alias did not use active root",
        )?;

        #[cfg(windows)]
        {
            let (_state, root) =
                server.state_and_root_path(None, Some(".\\".to_string()), false)?;
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

        let brief = server.build_session_brief(AtlasSessionBriefParams {
            project_path: None,
            query: Some("startup".to_string()),
            folder_limit: None,
            file_limit: None,
            blocker_limit: None,
        })?;

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
                matches!(recommendation.kind, McpBriefRecommendationKind::Scan)
            }),
            "missing-index brief did not recommend scan",
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
            1,
            7,
            Some(project_path.clone()),
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
                matches!(recommendation.kind, McpBriefRecommendationKind::Folders)
                    && recommendation.arguments.get(MCP_BRIEF_ARG_QUERY)
                        == Some(&serde_json::Value::String("startup".to_string()))
            }),
            "folder recommendation did not preserve query",
        )?;
        require(
            recommendations.iter().any(|recommendation| {
                matches!(recommendation.kind, McpBriefRecommendationKind::Health)
                    && recommendation.arguments.get(MCP_BRIEF_ARG_LIMIT)
                        == Some(&serde_json::json!(7))
            }),
            "health recommendation did not preserve limit",
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
        let mut store = open_atlas_store(&db_path)?;
        let plan = ScanRuntimePlan::for_path(None, &repo, None)?;
        let symbol_options = SymbolBuildOptions::new(MAX_SYMBOL_FILE_BYTES, Some(1), Some(30));
        run_scan_pipeline(&mut store, &plan, &symbol_options)?;
        drop(store);

        let server = ProjectAtlasMcpServer::new(db_path, None, "mcp-test".to_string(), false);
        let brief = server.build_session_brief(AtlasSessionBriefParams {
            project_path: None,
            query: Some("hiddenNeedle".to_string()),
            folder_limit: Some(5),
            file_limit: Some(5),
            blocker_limit: Some(5),
        })?;

        require(
            brief.files.is_empty(),
            "session brief returned a content-only indexed-text hit",
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
        let store = open_atlas_store(&db_path)?;
        store.set_project_root(&other)?;
        require(
            ProjectAtlasMcpServer::indexed_root_from_candidate(&repo).is_none(),
            "candidate with mismatched DB root was treated as indexed",
        )?;
        store.set_project_root(&repo)?;
        drop(store);
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
}
