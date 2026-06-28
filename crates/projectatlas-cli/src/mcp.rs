//! Native MCP adapter for `ProjectAtlas` agent integrations.

use crate::runtime::{
    MAX_SYMBOL_FILE_BYTES, ScanRuntimePlan, SymbolBuildOptions, build_settings_report,
    build_symbols_for_index, byte_count_to_tokens, default_mcp_project_root,
    estimated_source_tokens_for_indexed_files, estimated_source_tokens_for_paths,
    file_summary_usage_baseline, normalized_folder_filter, open_atlas_store, ranked_file_nodes,
    read_indexed_file_content, record_usage_estimate, record_usage_text, reset_index_files,
    run_scan_pipeline, run_watch_loop, strip_legacy_purpose, validated_indexed_file_key,
    watcher_status_report,
};
use crate::{
    CliError, DEFAULT_FILE_SUMMARY_LIMIT, OutputFormat, build_parity_report, render_code_slice,
    render_file_summary, render_parity_report, render_search_report, render_settings_report,
    render_watch_status,
};
use projectatlas_core::outline::build_outline;
use projectatlas_core::toon::{
    encode_agent_payload, render_health, render_nodes, render_outline, render_overview,
    render_symbol_relations, render_symbols, render_token_overview,
};
use projectatlas_core::{NodeKind, PurposeSource};
use projectatlas_db::{AtlasStore, HealthResolution};
use projectatlas_service::{
    SymbolSliceSelector, build_file_summary, read_indexed_code_slice, read_symbol_slice,
    search_indexed_files,
};
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::schemars;
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

/// MCP tools required for the agent-first repository-intelligence surface.
pub(crate) const REQUIRED_MCP_TOOL_NAMES: &[&str] = &[
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
        open_atlas_store(&self.db_path)
    }

    /// Open the durable index for mutation.
    fn open_mut_store(&self) -> Result<AtlasStore, CliError> {
        open_atlas_store(&self.db_path)
    }

    /// Return a path parameter or the MCP-bound project root.
    fn path_or_project_root(&self, path: Option<String>) -> Result<PathBuf, CliError> {
        let project_root = default_mcp_project_root(&self.db_path, self.config_path.as_deref())?;
        let Some(value) = path else {
            return Ok(project_root);
        };
        if value.is_empty() || value == "." {
            return Ok(project_root);
        }
        let candidate = PathBuf::from(value);
        if candidate.is_absolute() {
            Ok(candidate)
        } else {
            Ok(project_root.join(candidate))
        }
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
            let path = self.path_or_project_root(params.path)?;
            let plan = ScanRuntimePlan::for_path(
                self.config_path.as_deref(),
                &path,
                params.text_index_max_bytes,
            )?;
            let mut store = self.open_mut_store()?;
            let symbol_options = SymbolBuildOptions::new(
                params.max_bytes.unwrap_or(MAX_SYMBOL_FILE_BYTES),
                params.max_workers,
                params.timeout_seconds,
            );
            let report = run_scan_pipeline(&mut store, &plan, &symbol_options)?;
            Ok(encode_agent_payload(&json!({
                "scan": report
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
            let path = self.path_or_project_root(params.path)?;
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
        description = "Run one MCP-safe watcher refresh pass over the repository and rebuild changed symbols, with optional worker, timeout, and text-index size controls."
    )]
    fn atlas_watch_once(&self, Parameters(params): Parameters<AtlasWatchOnceParams>) -> String {
        Self::as_mcp_text((|| {
            let mut store = self.open_mut_store()?;
            let path = self.path_or_project_root(params.path)?;
            let plan = ScanRuntimePlan::for_path(
                self.config_path.as_deref(),
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
                &self.path_or_project_root(params.path)?,
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
