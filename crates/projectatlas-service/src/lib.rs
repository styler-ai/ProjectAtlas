//! Purpose: Provide shared `ProjectAtlas` query services for CLI and MCP adapters.

mod agent_efficiency;
mod analysis;
mod federation;
mod import_aliases;
mod relations;

pub use analysis::{
    AnalysisFinding, AnalysisFindingKind, AnalysisNode, AnalysisStatus, GitImpactSelection,
    RelationAnalysisDraft, RelationAnalysisMode, RelationAnalysisQuery, RelationAnalysisReport,
    RelationAnalysisWork, VcsImpact, load_relation_analysis,
};
pub use federation::{
    FederatedAnalysisDraft, FederatedAnalysisReport, FederatedDetailedRelationDraft,
    FederatedDetailedRelationReport, FederatedInputWork, FederatedParticipant,
    FederatedRelationEvidence, FederatedRelationWork, FederatedRendezvous, FederatedStore,
    MAX_FEDERATED_DATABASE_BYTES, MAX_FEDERATED_INPUT_BYTES, load_federated_detailed_relations,
    load_federated_relation_analysis, validate_federated_root_count,
};
pub use relations::{
    DetailedRelationBudget, DetailedRelationNode, DetailedRelationPageDraft, DetailedRelationQuery,
    DetailedRelationReport, DetailedRelationRow, DetailedRelationWork, RelationAnchor,
    RelationDirection, RelationNextCall, RelationPurpose, RelationResolutionFilter,
    RelationTotalState, load_detailed_relation_page, load_detailed_relations,
    parse_relation_confidence, parse_relation_direction, parse_relation_resolution,
};

use agent_efficiency::load_agent_efficiency_comparison as load_agent_efficiency_for_binding;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use import_aliases::{ImportAliasMap, load_import_alias_map};
use projectatlas_core::graph::{
    CoverageScope, CoverageState, ExtendedRelationKind, GraphIdentityRejection, GraphLimitKind,
    GraphLimits, GraphRelationKind,
};
use projectatlas_core::language::{ContentClassification, ContentSelection};
use projectatlas_core::outline::estimate_tokens;
use projectatlas_core::symbols::{
    CodeSymbol, ParserKind, RelationKind, SourceParseMetadata, SymbolKind, SymbolRelation,
};
use projectatlas_core::telemetry::{
    AgentEfficiencyComparison, TokenOverview, TokenTrendReport, TokenTrendWindow,
};
use projectatlas_core::{
    CanonicalProjectRoot, IndexCancellation, IndexGeneration, IndexWorkControl, IndexWorkFailure,
    IndexWorkStage, IndexedNode, NavigationNextCall, NavigationNextCapability, NodeKind,
    RankedConnectionKind, RankedConnectionTarget, RankedNode, RankedReasonCode,
    repo_path_to_native, validated_repo_file_key,
};
use projectatlas_db::{
    AtlasStore, CapturedProjectBinding, DbError, FileTextAdmission, FileTextFtsQuery,
    IndexedFileText, MAX_FILE_CONTENT_CLASSIFICATION_PATHS, MAX_FILE_TEXT_FTS_CANDIDATES,
    RepositoryCoverageQuery, RepositoryCoverageRow, RepositoryNavigationConnections,
    RepositoryNavigationNode,
};
use projectatlas_symbols::module_aliases_for_path;
use regex::RegexBuilder;
use serde::Serialize;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

/// Maximum caller references retained for one summarized symbol.
const CALLERS_PER_SYMBOL_LIMIT: usize = 20;
/// Relation query limit multiplier used for called-by lookup.
const CALLER_RELATION_LIMIT_PER_TARGET: usize = 20;
/// Maximum package/module symbols read for file-level metadata.
const FILE_METADATA_SYMBOL_LIMIT: usize = 20;
/// Maximum concise reasons attached to one ranked result.
const RANKED_REASON_LIMIT: usize = 6;
/// Maximum selected candidates considered when service-side ranking enriches DB output.
const RANKED_CANDIDATE_LIMIT: usize = 100;
/// Maximum validated relationships retained for one navigation family.
const RANKED_CONNECTION_FAMILY_LIMIT: u32 = 4;
/// Maximum high-value connections sampled into one ranked row.
const RANKED_CONNECTION_SAMPLE_LIMIT: usize = 3;
/// Default number of folders and files returned by `next`.
const NEXT_REPORT_DEFAULT_LIMIT: usize = 3;
/// Maximum number of folders and files returned by `next`.
const NEXT_REPORT_MAX_LIMIT: usize = 10;
/// Maximum rows returned by one agent-facing coverage page.
pub const COVERAGE_PAGE_MAX_LIMIT: u32 = 200;
/// Maximum elapsed work for one project-wide coverage discovery query.
const COVERAGE_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(2);
/// Maximum rows retained by one selected-file coverage digest.
const COVERAGE_DIGEST_ROW_LIMIT: u32 = 16;
/// Status emitted when live source was read successfully.
const SOURCE_STATUS_LIVE: &str = "live-source";
/// Status emitted when indexed metadata had to stand in for live source.
const SOURCE_STATUS_INDEXED: &str = "indexed-metadata";
/// Maximum selected persisted-text files inspected by one lexical search.
const SEARCH_MAX_SELECTED_FILES: usize = 50_000;
/// Maximum selected persisted-text bytes inspected by one lexical search.
const SEARCH_MAX_SELECTED_BYTES: usize = 128 * 1024 * 1024;
/// Maximum wall time available to one lexical search.
const SEARCH_MAX_ELAPSED: Duration = Duration::from_secs(10);
/// Maximum context lines retained on either side of one match.
const SEARCH_MAX_CONTEXT_LINES: usize = 20;
/// Maximum result rows retained by one lexical search.
const SEARCH_MAX_RESULT_ROWS: usize = 1_000;
/// Maximum approximate payload bytes retained before adapter serialization.
const SEARCH_MAX_RETAINED_BYTES: usize = 2 * 1024 * 1024;
/// Maximum UTF-8 bytes accepted in one literal, regex, or fuzzy pattern.
const SEARCH_MAX_PATTERN_BYTES: usize = 64 * 1024;
/// Maximum UTF-8 bytes accepted in one repository path glob.
const SEARCH_MAX_FILE_PATTERN_BYTES: usize = 4 * 1024;
/// Stable state reported until the optional semantic lifecycle lands in task 6.3.
const SEARCH_SEMANTIC_UNAVAILABLE_STATE: &str = "not-installed";
/// Stable recovery guidance for an explicitly unavailable retrieval mode.
const SEARCH_SEMANTIC_RECOVERY: &str =
    "install and enable a compatible semantic retrieval pack, then build a ready generation";

/// Internal resource ceilings for one lexical search execution.
#[derive(Clone, Copy, Debug)]
struct SearchBounds {
    /// Maximum persisted-text rows admitted for decoding.
    selected_files: usize,
    /// Maximum persisted-text bytes admitted for decoding.
    selected_bytes: usize,
    /// Maximum elapsed search duration.
    elapsed: Duration,
    /// Maximum approximate bytes retained before serialization.
    retained_bytes: usize,
}

/// Product search ceilings applied identically to CLI and MCP calls.
const DEFAULT_SEARCH_BOUNDS: SearchBounds = SearchBounds {
    selected_files: SEARCH_MAX_SELECTED_FILES,
    selected_bytes: SEARCH_MAX_SELECTED_BYTES,
    elapsed: SEARCH_MAX_ELAPSED,
    retained_bytes: SEARCH_MAX_RETAINED_BYTES,
};
/// Service-layer failures.
#[derive(Debug, Error)]
pub enum ServiceError {
    /// Database operation failed.
    #[error("{0}")]
    Db(#[from] DbError),
    /// User input or stored metadata was invalid.
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// Filesystem operation failed.
    #[error("io error for {path:?}: {source}")]
    Io {
        /// Path involved in the IO failure.
        path: PathBuf,
        /// Source IO error.
        source: std::io::Error,
    },
    /// Serialization failed while building a telemetry baseline.
    #[error("serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
    /// The selected database has no complete project binding.
    #[error("selected project binding is unavailable")]
    SelectedProjectUnavailable,
    /// The selected project binding changed while the report was being read.
    #[error("selected project binding changed while loading the token report")]
    SelectedProjectChanged,
    /// An explicitly requested optional search capability has no ready generation.
    #[error("search retrieval mode {requested_mode:?} is unavailable ({state}); {guidance}")]
    SearchCapabilityUnavailable {
        /// Caller-selected retrieval mode.
        requested_mode: SearchRetrievalMode,
        /// Stable optional-capability lifecycle state.
        state: &'static str,
        /// Actionable recovery guidance.
        guidance: &'static str,
    },
    /// A detailed-relation cursor is malformed or violates its bounded state invariants.
    #[error("invalid detailed relation cursor: {reason}; restart the relation request")]
    RelationCursorInvalid {
        /// Bounded validation reason safe to expose to the caller.
        reason: &'static str,
    },
    /// A detailed-relation cursor belongs to another normalized request.
    #[error("detailed relation cursor does not match {field}; restart the relation request")]
    RelationCursorMismatched {
        /// Result-defining request field that changed.
        field: &'static str,
    },
    /// A detailed-relation cursor belongs to stale repository or purpose state.
    #[error("detailed relation cursor is stale for {field}; restart the relation request")]
    RelationCursorStale {
        /// Captured state field that changed.
        field: &'static str,
    },
}

/// Convenient result alias for service operations.
pub type ServiceResult<T> = Result<T, ServiceError>;

/// Hash a native canonical root for an opaque service continuation identity.
pub(crate) fn canonical_root_digest(
    domain: &str,
    root: &CanonicalProjectRoot,
) -> ServiceResult<[u8; 32]> {
    let canonical_root = CanonicalProjectRoot::from_path(root.as_path())
        .map_err(|error| ServiceError::InvalidInput(error.to_string()))?;
    let encoded = canonical_root
        .encode()
        .map_err(|error| ServiceError::InvalidInput(error.to_string()))?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain.as_bytes());
    hasher.update(&[0]);
    hasher.update(&encoded);
    Ok(*hasher.finalize().as_bytes())
}

/// Closed token-report request selected by CLI and MCP adapters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenReportRequest<'a> {
    /// Load the all-time token overview for an optional caller label.
    Overview {
        /// Optional caller-visible label filter.
        caller_label: Option<&'a str>,
        /// Optional repository-relative controlled benchmark artifact.
        benchmark_results: Option<&'a Path>,
    },
    /// Load retained token trends for an optional caller label and window.
    Trends {
        /// Optional caller-visible label filter.
        caller_label: Option<&'a str>,
        /// Calendar grouping requested by the adapter.
        window: TokenTrendWindow,
    },
    /// Load the control atlas's combined native-main and worktree overview.
    RepositoryOverview {
        /// Optional repository-relative controlled benchmark artifact.
        benchmark_results: Option<&'a Path>,
    },
    /// Load combined native-main and worktree trends.
    RepositoryTrends {
        /// Calendar grouping requested by the adapter.
        window: TokenTrendWindow,
    },
}

/// Typed token-report result returned without transport rendering.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenReport {
    /// All-time token overview.
    Overview(Box<TokenOverview>),
    /// Retained token trend periods.
    Trends(TokenTrendReport),
}

/// Capture the root and identity validated when the selected store opened.
fn selected_project_binding(store: &AtlasStore) -> ServiceResult<CapturedProjectBinding> {
    match store.captured_project_binding() {
        Ok(binding) => Ok(binding),
        Err(DbError::ProjectRootMissing | DbError::ProjectInstanceIdentityMissing) => {
            Err(ServiceError::SelectedProjectUnavailable)
        }
        Err(error) => Err(ServiceError::Db(error)),
    }
}

/// Revalidate the selected binding on a fresh snapshot after the report read.
fn revalidate_selected_project_binding(store: &AtlasStore) -> ServiceResult<()> {
    match store.revalidate_captured_project_binding() {
        Ok(()) => Ok(()),
        Err(
            DbError::ProjectRootMissing
            | DbError::ProjectInstanceIdentityMissing
            | DbError::ProjectRootMismatch { .. }
            | DbError::ProjectRootTransitionChanged { .. },
        ) => Err(ServiceError::SelectedProjectChanged),
        Err(error) => Err(ServiceError::Db(error)),
    }
}

/// Load persisted classifications for exact paths through bounded set queries.
fn file_content_classifications_by_path(
    store: &AtlasStore,
    paths: impl IntoIterator<Item = String>,
) -> ServiceResult<HashMap<String, ContentClassification>> {
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    let mut classifications = HashMap::with_capacity(paths.len());
    for chunk in paths.chunks(MAX_FILE_CONTENT_CLASSIFICATION_PATHS) {
        classifications.extend(
            store
                .file_content_classifications_for_paths(chunk)?
                .into_iter()
                .map(|row| (row.path, row.classification)),
        );
    }
    Ok(classifications)
}

/// Load one exact file classification and enforce an explicit selection.
fn selected_file_classification(
    store: &AtlasStore,
    path: &str,
    selection: ContentSelection,
) -> ServiceResult<ContentClassification> {
    let mut classifications = file_content_classifications_by_path(store, [path.to_string()])?;
    let classification = classifications.remove(path).ok_or_else(|| {
        ServiceError::InvalidInput(format!("file {path:?} has no content classification"))
    })?;
    if !selection.includes(classification) {
        return Err(ServiceError::InvalidInput(format!(
            "file {path:?} is classified as {classification} and is outside the selected content"
        )));
    }
    Ok(classification)
}

/// Load one token report through the selected-project service boundary.
///
/// # Errors
///
/// Returns an error when the selected project is unavailable or changes during
/// the bounded database read, or when the report query fails.
pub fn load_token_report(
    store: &AtlasStore,
    request: TokenReportRequest<'_>,
) -> ServiceResult<TokenReport> {
    let selected_project = selected_project_binding(store)?;
    let report = match request {
        TokenReportRequest::Overview {
            caller_label,
            benchmark_results,
        } => {
            let mut overview = store.token_overview(caller_label)?;
            overview.set_agent_efficiency(load_agent_efficiency_for_binding(
                &selected_project,
                benchmark_results,
            )?);
            TokenReport::Overview(Box::new(overview))
        }
        TokenReportRequest::Trends {
            caller_label,
            window,
        } => TokenReport::Trends(store.token_trends(caller_label, window)?),
        TokenReportRequest::RepositoryOverview { benchmark_results } => {
            let mut overview = store.repository_token_overview()?;
            overview.set_agent_efficiency(load_agent_efficiency_for_binding(
                &selected_project,
                benchmark_results,
            )?);
            TokenReport::Overview(Box::new(overview))
        }
        TokenReportRequest::RepositoryTrends { window } => {
            TokenReport::Trends(store.repository_token_trends(window)?)
        }
    };
    revalidate_selected_project_binding(store)?;
    Ok(report)
}

/// Load optional benchmark evidence for one exact selected project.
///
/// # Errors
///
/// Returns an error when the selected project is unavailable or changes while
/// the bounded artifact is loaded.
pub fn load_agent_efficiency_comparison(
    store: &AtlasStore,
    benchmark_results: Option<&Path>,
) -> ServiceResult<AgentEfficiencyComparison> {
    let selected_project = selected_project_binding(store)?;
    let comparison = load_agent_efficiency_for_binding(&selected_project, benchmark_results)?;
    revalidate_selected_project_binding(store)?;
    Ok(comparison)
}

/// Closed trust projection for one normalized coverage state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageTrustState {
    /// The selected producer reported complete coverage.
    Trusted,
    /// Some current facts are available while omissions remain explicit.
    Partial,
    /// The selected facts are unavailable or not current enough to trust.
    Untrusted,
}

/// Closed producer family represented by one coverage row.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageExtractionPass {
    /// File parse and fact projection coverage.
    GraphProjection,
    /// One normalized relationship-family extraction pass.
    Relationship,
}

/// Typed cardinality knowledge for one bounded coverage page.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "state", content = "value", rename_all = "snake_case")]
pub enum CoverageTotalState {
    /// The bounded page proves the exact filtered total.
    Exact(u32),
    /// At least this many matching rows exist.
    AtLeast(u32),
    /// An exact or lower-bound total is unavailable at this continuation.
    Unknown,
}

/// Per-state counts retained by one selected-file coverage digest.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct CoverageStateCounts {
    /// Complete coverage rows.
    pub complete: u32,
    /// Complete extraction scopes containing no supported candidates.
    pub no_candidates: u32,
    /// Partial coverage rows.
    pub partial: u32,
    /// Failed coverage rows.
    pub failed: u32,
    /// Intentionally ignored coverage rows.
    pub ignored: u32,
    /// Oversized coverage rows.
    pub oversized: u32,
    /// Quarantined coverage rows.
    pub quarantined: u32,
    /// Stale coverage rows.
    pub stale: u32,
}

/// Compact relationship and parse coverage attached to one selected-file summary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CoverageDigest {
    /// Whether current coverage rows exist for the selected file.
    pub available: bool,
    /// Active generation shared by every retained row, or zero when unavailable.
    pub active_generation: IndexGeneration,
    /// Source parser pass recorded for the file.
    pub parser: Option<ParserKind>,
    /// Fact provider pass recorded for the file.
    pub provider: Option<ParserKind>,
    /// Bounded per-state coverage counts.
    pub states: CoverageStateCounts,
    /// Total items declared by retained coverage rows.
    pub total: u64,
    /// Covered items declared by retained coverage rows.
    pub covered: u64,
    /// Omitted or untrusted items declared by retained coverage rows.
    pub omitted: u64,
    /// Number of retained relation-family rows.
    pub relation_rows: u32,
    /// Whether additional selected-file rows were omitted by the digest bound.
    pub truncated: bool,
    /// Conservative trust state across the retained digest.
    pub trust: CoverageTrustState,
    /// Existing opt-in health surface for deeper coverage discovery.
    pub next_call: NavigationNextCall,
}

/// One actionable row in an opt-in coverage page.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CoverageDiscoveryRow {
    /// Exact repository-relative path, or `.` for project-scoped coverage.
    pub path: String,
    /// Stable extraction-pass owner for parse or relationship facts.
    pub extraction_pass: CoverageExtractionPass,
    /// Optional normalized relation family.
    pub relation: Option<GraphRelationKind>,
    /// Current coverage lifecycle state.
    pub state: CoverageState,
    /// Conservative trust projection.
    pub trust: CoverageTrustState,
    /// Total items represented by this row.
    pub total: u64,
    /// Successfully covered items.
    pub covered: u64,
    /// Omitted or untrusted items.
    pub omitted: u64,
    /// Actionable explanation when coverage is not complete.
    pub reason: Option<String>,
    /// Reached product limit when applicable.
    pub reached_limit: Option<GraphLimitKind>,
    /// Active complete index generation.
    pub active_generation: IndexGeneration,
    /// Source parser pass for path-scoped coverage.
    pub parser: Option<ParserKind>,
    /// Fact provider pass for path-scoped coverage.
    pub provider: Option<ParserKind>,
    /// Bounded typed identity rejections for this path.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub identity_rejections: Vec<GraphIdentityRejection>,
    /// Existing selected-file summary or health surface to call next.
    pub next_call: NavigationNextCall,
}

/// Agent-facing bounded coverage discovery report.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CoverageDiscoveryReport {
    /// Zero-based result offset after filters.
    pub start_index: u32,
    /// Requested page limit after the service ceiling is applied.
    pub limit: u32,
    /// Maximum service page size.
    pub max_limit: u32,
    /// Number of rows returned.
    pub returned: u32,
    /// Whether at least one additional validated row exists.
    pub truncated: bool,
    /// Next zero-based continuation when another row exists.
    pub continuation: Option<u32>,
    /// Product ceiling reached when further continuation is intentionally unavailable.
    pub reached_limit: Option<GraphLimitKind>,
    /// Typed knowledge of the filtered total.
    pub total: CoverageTotalState,
    /// Encoded output bytes, filled by the selected adapter.
    pub output_bytes: u32,
    /// Absolute encoded-output ceiling.
    pub max_output_bytes: u32,
    /// Fully validated actionable rows.
    pub rows: Vec<CoverageDiscoveryRow>,
}

/// Parse one public parser/provider coverage filter.
///
/// # Errors
///
/// Returns an error when the value is not a supported parser pass.
pub fn parse_coverage_parser(value: &str) -> ServiceResult<ParserKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "tree-sitter" | "tree_sitter" => Ok(ParserKind::TreeSitter),
        "manifest" => Ok(ParserKind::Manifest),
        "structural" => Ok(ParserKind::Structural),
        "fallback" => Ok(ParserKind::Fallback),
        _ => Err(ServiceError::InvalidInput(format!(
            "invalid coverage parser/provider '{value}'; expected tree-sitter, manifest, structural, or fallback"
        ))),
    }
}

/// Parse one public relation-family coverage filter.
///
/// # Errors
///
/// Returns an error when the value is not a supported legacy or extended family.
pub fn parse_coverage_relation(value: &str) -> ServiceResult<GraphRelationKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "contains" => Ok(GraphRelationKind::Legacy(RelationKind::Contains)),
        "imports" => Ok(GraphRelationKind::Legacy(RelationKind::Imports)),
        "calls" => Ok(GraphRelationKind::Legacy(RelationKind::Calls)),
        "depends-on" | "depends_on" => Ok(GraphRelationKind::Legacy(RelationKind::DependsOn)),
        "references" => Ok(GraphRelationKind::Extended(
            ExtendedRelationKind::References,
        )),
        "tests" => Ok(GraphRelationKind::Extended(ExtendedRelationKind::Tests)),
        "routes-to" | "routes_to" => {
            Ok(GraphRelationKind::Extended(ExtendedRelationKind::RoutesTo))
        }
        "configures" => Ok(GraphRelationKind::Extended(
            ExtendedRelationKind::Configures,
        )),
        "deploys" => Ok(GraphRelationKind::Extended(ExtendedRelationKind::Deploys)),
        "reads" => Ok(GraphRelationKind::Extended(ExtendedRelationKind::Reads)),
        "writes" => Ok(GraphRelationKind::Extended(ExtendedRelationKind::Writes)),
        "documents" => Ok(GraphRelationKind::Extended(ExtendedRelationKind::Documents)),
        _ => Err(ServiceError::InvalidInput(format!(
            "invalid coverage relation '{value}'"
        ))),
    }
}

/// Parse one public coverage lifecycle filter.
///
/// # Errors
///
/// Returns an error when the value is not one of the eight closed states.
pub fn parse_coverage_state(value: &str) -> ServiceResult<CoverageState> {
    match value.trim().to_ascii_lowercase().as_str() {
        "complete" => Ok(CoverageState::Complete),
        "no-candidates" | "no_candidates" => Ok(CoverageState::NoCandidates),
        "partial" => Ok(CoverageState::Partial),
        "failed" => Ok(CoverageState::Failed),
        "ignored" => Ok(CoverageState::Ignored),
        "oversized" => Ok(CoverageState::Oversized),
        "quarantined" => Ok(CoverageState::Quarantined),
        "stale" => Ok(CoverageState::Stale),
        _ => Err(ServiceError::InvalidInput(format!(
            "invalid coverage state '{value}'"
        ))),
    }
}

/// Load one bounded opt-in coverage page without starting index work.
///
/// # Errors
///
/// Returns an error when the selected project has no identity, the query bounds
/// are invalid, or persisted coverage/provenance is inconsistent.
pub fn load_coverage_discovery(
    store: &AtlasStore,
    query: RepositoryCoverageQuery,
) -> ServiceResult<CoverageDiscoveryReport> {
    let control = IndexWorkControl::new(IndexCancellation::new(), Some(COVERAGE_DISCOVERY_TIMEOUT));
    load_coverage_discovery_controlled(store, query, &control)
}

/// Load one coverage page under caller cancellation and a fixed elapsed ceiling.
///
/// # Errors
///
/// Returns the same errors as [`load_coverage_discovery`] plus typed
/// cancellation or deadline failure.
pub fn load_coverage_discovery_controlled(
    store: &AtlasStore,
    mut query: RepositoryCoverageQuery,
    control: &IndexWorkControl,
) -> ServiceResult<CoverageDiscoveryReport> {
    if query.start_index >= GraphLimits::MAX_ROWS {
        return Err(ServiceError::InvalidInput(format!(
            "coverage start index must be below {}",
            GraphLimits::MAX_ROWS
        )));
    }
    query.limit = query
        .limit
        .clamp(1, COVERAGE_PAGE_MAX_LIMIT)
        .min(GraphLimits::MAX_ROWS - query.start_index);
    let project = store
        .project_instance_id()?
        .ok_or(ServiceError::SelectedProjectUnavailable)?;
    let control = control.with_timeout_ceiling(COVERAGE_DISCOVERY_TIMEOUT);
    let page = store.repository_coverage_page_controlled(project, &query, Some(&control))?;
    let page_rows = page.rows;
    let paths = page_rows
        .iter()
        .filter_map(|row| match row.coverage.scope() {
            CoverageScope::Path { path } => Some(path.clone()),
            CoverageScope::Project => None,
        })
        .collect::<Vec<_>>();
    let rejections = store.repository_graph_identity_rejections(
        project,
        &paths,
        GraphLimits::MAX_ROWS,
        Some(&control),
    )?;
    let mut rejections_by_path = HashMap::<String, Vec<GraphIdentityRejection>>::new();
    for rejection in rejections {
        rejections_by_path
            .entry(rejection.path.as_str().to_owned())
            .or_default()
            .push(rejection);
    }
    let rows = page_rows
        .into_iter()
        .map(|row| coverage_discovery_row(row, &mut rejections_by_path))
        .collect::<Vec<_>>();
    let returned = u32::try_from(rows.len()).map_err(|error| {
        ServiceError::InvalidInput(format!("coverage row count did not fit u32: {error}"))
    })?;
    let proved = query.start_index.saturating_add(returned);
    let total = if page.truncated {
        CoverageTotalState::AtLeast(proved.saturating_add(1))
    } else if query.start_index == 0 || returned > 0 {
        CoverageTotalState::Exact(proved)
    } else {
        CoverageTotalState::Unknown
    };
    let next_index = query.start_index.saturating_add(returned);
    let continuation = (page.truncated && next_index < GraphLimits::MAX_ROWS).then_some(next_index);
    let reached_limit = (page.truncated && continuation.is_none()).then_some(GraphLimitKind::Rows);
    Ok(CoverageDiscoveryReport {
        start_index: query.start_index,
        limit: query.limit,
        max_limit: COVERAGE_PAGE_MAX_LIMIT,
        returned,
        truncated: page.truncated,
        continuation,
        reached_limit,
        total,
        output_bytes: 0,
        max_output_bytes: GraphLimits::MAX_OUTPUT_BYTES,
        rows,
    })
}

/// Project one validated storage row into the agent-facing coverage contract.
fn coverage_discovery_row(
    row: RepositoryCoverageRow,
    rejections_by_path: &mut HashMap<String, Vec<GraphIdentityRejection>>,
) -> CoverageDiscoveryRow {
    let coverage = row.coverage;
    let path = match coverage.scope() {
        CoverageScope::Project => ".".to_string(),
        CoverageScope::Path { path } => path.as_str().to_string(),
    };
    let relation = coverage.relation();
    let identity_rejections = rejections_by_path.remove(&path).unwrap_or_default();
    CoverageDiscoveryRow {
        next_call: NavigationNextCall {
            capability: if matches!(coverage.scope(), CoverageScope::Path { .. }) {
                NavigationNextCapability::Summary
            } else {
                NavigationNextCapability::Health
            },
            path: path.clone(),
        },
        path,
        extraction_pass: if relation.is_some() {
            CoverageExtractionPass::Relationship
        } else {
            CoverageExtractionPass::GraphProjection
        },
        relation,
        state: coverage.state(),
        trust: coverage_trust(coverage.state()),
        total: coverage.total(),
        covered: coverage.covered(),
        omitted: coverage.omitted(),
        reason: coverage.reason().map(|reason| reason.as_str().to_string()),
        reached_limit: coverage.reached_limit(),
        active_generation: coverage.generation(),
        parser: row.parser,
        provider: row.provider,
        identity_rejections,
    }
}

/// Return the conservative trust state for one coverage lifecycle state.
const fn coverage_trust(state: CoverageState) -> CoverageTrustState {
    match state {
        CoverageState::Complete | CoverageState::NoCandidates => CoverageTrustState::Trusted,
        CoverageState::Partial => CoverageTrustState::Partial,
        CoverageState::Failed
        | CoverageState::Ignored
        | CoverageState::Oversized
        | CoverageState::Quarantined
        | CoverageState::Stale => CoverageTrustState::Untrusted,
    }
}

/// Build the bounded selected-file coverage digest used by normal summaries.
fn load_coverage_digest(
    store: &AtlasStore,
    path: &str,
    parse_metadata: Option<&SourceParseMetadata>,
) -> ServiceResult<CoverageDigest> {
    let project = store
        .project_instance_id()?
        .ok_or(ServiceError::SelectedProjectUnavailable)?;
    let path_page = store.repository_coverage_page(
        project,
        &RepositoryCoverageQuery {
            start_index: 0,
            limit: COVERAGE_DIGEST_ROW_LIMIT,
            path_prefix: Some(path.to_string()),
            parser: None,
            provider: None,
            relation: None,
            state: None,
            reason: None,
        },
    )?;
    let rows = path_page
        .rows
        .into_iter()
        .filter(|row| {
            matches!(
                row.coverage.scope(),
                CoverageScope::Path { path: row_path } if row_path.as_str() == path
            )
        })
        .collect::<Vec<_>>();
    let mut states = CoverageStateCounts::default();
    let mut total = 0_u64;
    let mut covered = 0_u64;
    let mut omitted = 0_u64;
    let mut relation_rows = 0_u32;
    let mut trust = CoverageTrustState::Trusted;
    for row in &rows {
        let record = &row.coverage;
        increment_coverage_state(&mut states, record.state());
        total = checked_coverage_sum(total, record.total(), "total")?;
        covered = checked_coverage_sum(covered, record.covered(), "covered")?;
        omitted = checked_coverage_sum(omitted, record.omitted(), "omitted")?;
        if record.relation().is_some() {
            relation_rows = relation_rows.saturating_add(1);
        }
        trust = match (trust, coverage_trust(record.state())) {
            (_, CoverageTrustState::Untrusted) => CoverageTrustState::Untrusted,
            (CoverageTrustState::Trusted, CoverageTrustState::Partial) => {
                CoverageTrustState::Partial
            }
            (current, _) => current,
        };
    }
    let available = !rows.is_empty();
    if !available {
        trust = CoverageTrustState::Untrusted;
    }
    Ok(CoverageDigest {
        available,
        active_generation: rows
            .first()
            .map_or(IndexGeneration::ZERO, |row| row.coverage.generation()),
        parser: rows
            .iter()
            .find_map(|row| row.parser)
            .or_else(|| parse_metadata.map(|metadata| metadata.parser)),
        provider: rows.iter().find_map(|row| row.provider),
        states,
        total,
        covered,
        omitted,
        relation_rows,
        truncated: path_page.truncated,
        trust,
        next_call: NavigationNextCall {
            capability: NavigationNextCapability::Health,
            path: path.to_string(),
        },
    })
}

/// Add one persisted non-negative coverage count without silent overflow.
fn checked_coverage_sum(current: u64, value: u64, field: &str) -> ServiceResult<u64> {
    current.checked_add(value).ok_or_else(|| {
        ServiceError::InvalidInput(format!("selected-file coverage {field} overflowed u64"))
    })
}

/// Increment the closed selected-file state counter.
fn increment_coverage_state(counts: &mut CoverageStateCounts, state: CoverageState) {
    let count = match state {
        CoverageState::Complete => &mut counts.complete,
        CoverageState::NoCandidates => &mut counts.no_candidates,
        CoverageState::Partial => &mut counts.partial,
        CoverageState::Failed => &mut counts.failed,
        CoverageState::Ignored => &mut counts.ignored,
        CoverageState::Oversized => &mut counts.oversized,
        CoverageState::Quarantined => &mut counts.quarantined,
        CoverageState::Stale => &mut counts.stale,
    };
    *count = count.saturating_add(1);
}

/// Structured deterministic intelligence for one indexed file.
#[derive(Debug, Serialize)]
pub struct FileSummaryReport {
    /// Repository-relative file path.
    pub file_path: String,
    /// Detected language or file family.
    pub language: String,
    /// Registry-owned role of this admitted file.
    pub classification: ContentClassification,
    /// Source line count when the file can be read.
    pub line_count: usize,
    /// Whether source-derived fields came from live source or indexed metadata.
    pub source_status: String,
    /// Error text when live source could not be read.
    pub source_error: String,
    /// Parser family that produced the stored content summary.
    pub parser_kind: String,
    /// Summary quality status: `ok`, `fallback`, or `missing`.
    pub summary_status: String,
    /// Durable one-line reason this file exists, if approved or suggested.
    pub file_purpose: String,
    /// File-purpose lifecycle status.
    pub file_purpose_status: String,
    /// File-purpose source.
    pub file_purpose_source: String,
    /// Whether an agent explicitly reviewed or set this purpose.
    pub file_purpose_agent_reviewed: bool,
    /// Current one-line content summary from scan and deep index facts.
    pub content_summary: String,
    /// Package, module, or manifest name when indexed.
    pub package: String,
    /// File or primary symbol documentation when indexed.
    pub docstring: String,
    /// Total indexed symbols.
    pub symbol_count: usize,
    /// Maximum rows returned per repeated section.
    pub limit: usize,
    /// Total indexed functions before limiting.
    pub total_functions: usize,
    /// Total indexed methods before limiting.
    pub total_methods: usize,
    /// Total indexed classes before limiting.
    pub total_classes: usize,
    /// Total indexed type-like declarations before limiting.
    pub total_types: usize,
    /// Total call relationships before limiting.
    pub total_calls: usize,
    /// Total import relationships before limiting.
    pub total_imports: usize,
    /// Total manifest dependency relationships before limiting.
    pub total_dependencies: usize,
    /// Total exported/public symbols before limiting.
    pub total_exports: usize,
    /// Whether any repeated section was truncated.
    pub truncated: bool,
    /// Indexed functions.
    pub functions: Vec<FileSymbolSummary>,
    /// Indexed methods.
    pub methods: Vec<FileSymbolSummary>,
    /// Indexed classes or class-like types.
    pub classes: Vec<FileSymbolSummary>,
    /// Indexed structs, enums, traits, interfaces, and type aliases.
    pub types: Vec<FileSymbolSummary>,
    /// Imported modules and include-like dependencies.
    pub imports: Vec<String>,
    /// Manifest package dependencies.
    pub dependencies: Vec<String>,
    /// Exported or publicly visible declarations.
    pub exports: Vec<String>,
    /// Call relationships discovered inside this file.
    pub calls: Vec<FileCallSummary>,
    /// Compact current relationship and parse coverage.
    pub coverage: CoverageDigest,
}

/// Compact file-summary symbol row.
#[derive(Debug, Serialize)]
pub struct FileSymbolSummary {
    /// Symbol name.
    pub name: String,
    /// Symbol kind.
    pub kind: String,
    /// One-based start line.
    pub line: usize,
    /// One-based end line.
    pub end_line: usize,
    /// Declaration signature.
    pub signature: String,
    /// Whether the symbol is exported or publicly visible.
    pub exported: bool,
    /// Extracted doc comment or docstring.
    pub documentation: String,
    /// Optional parent symbol.
    pub parent: String,
    /// Symbols that call this symbol across the indexed graph.
    pub called_by: Vec<String>,
}

/// Compact file-summary call row.
#[derive(Debug, Serialize)]
pub struct FileCallSummary {
    /// Calling symbol name.
    pub source: String,
    /// Called symbol name.
    pub target: String,
    /// One-based call line.
    pub line: usize,
    /// Compact call-site context.
    pub context: String,
}

/// Result row for indexed text search.
#[derive(Debug, Serialize)]
pub struct SearchMatch {
    /// Repository-relative path.
    pub path: String,
    /// Registry-owned role of the matched file.
    pub classification: ContentClassification,
    /// One-based line number.
    pub line: usize,
    /// Context before the matching line.
    pub context_before: Vec<String>,
    /// Matching line text.
    pub text: String,
    /// Context after the matching line.
    pub context_after: Vec<String>,
}

/// Caller-selected retrieval family for repository search.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchRetrievalMode {
    /// Correctness-authoritative persisted lexical search.
    #[default]
    Lexical,
    /// Optional semantic retrieval generation.
    Semantic,
    /// Lexical-complete ranking with optional semantic enrichment.
    Hybrid,
}

impl SearchRetrievalMode {
    /// Return the stable adapter-facing mode name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lexical => "lexical",
            Self::Semantic => "semantic",
            Self::Hybrid => "hybrid",
        }
    }
}

/// Complete typed request for one bounded indexed-text search.
#[derive(Clone, Copy, Debug)]
pub struct SearchQuery<'query> {
    /// Literal, regex, or fuzzy source pattern.
    pub pattern: &'query str,
    /// Whether the source pattern is a regular expression.
    pub regex: bool,
    /// Whether the source pattern is a fuzzy subsequence.
    pub fuzzy: bool,
    /// Whether exact matching preserves source case.
    pub case_sensitive: bool,
    /// Optional repository-relative glob.
    pub file_pattern: Option<&'query str>,
    /// Context lines retained before and after each match.
    pub context_lines: usize,
    /// Number of exact matches skipped before result retention.
    pub start_index: usize,
    /// Maximum exact matches returned.
    pub limit: usize,
    /// Optional classified-content restriction; the default preserves legacy candidates.
    pub content_selection: ContentSelection,
    /// Explicit retrieval family; omitted adapters use lexical.
    pub retrieval_mode: SearchRetrievalMode,
}

/// Search report returned by CLI and MCP adapters.
#[derive(Debug, Serialize)]
pub struct SearchReport {
    /// Search pattern.
    pub query: String,
    /// Search mode: `literal`, `regex`, or `fuzzy`.
    pub mode: String,
    /// Retrieval family selected by the caller.
    pub retrieval_mode: String,
    /// Explicit classified-content restriction, or `None` for legacy behavior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_selection: Option<ContentSelection>,
    /// Source used for broad repository search.
    pub source: String,
    /// Candidate strategy used while preserving exact lexical semantics.
    pub strategy: String,
    /// Pagination start index.
    pub start_index: usize,
    /// Matches observed before pagination and bounded early stop.
    pub total: usize,
    /// Alias for `total` that makes bounded search semantics explicit.
    pub observed_total: usize,
    /// Whether `total`/`observed_total` is known to be the exhaustive match count.
    pub total_is_complete: bool,
    /// Returned matches after pagination.
    pub returned: usize,
    /// Indexed files opened while serving the query.
    pub searched_files: usize,
    /// Source bytes read while serving the query.
    pub searched_bytes: usize,
    /// Metadata-only FTS candidates considered before exact verification.
    pub candidate_files: usize,
    /// Approximate retained result bytes before adapter serialization.
    pub retained_bytes: usize,
    /// Whether the search stopped after satisfying the requested page.
    pub truncated: bool,
    /// Stable first bound that stopped exhaustive search, when applicable.
    pub truncation_reason: Option<String>,
    /// Search matches.
    pub results: Vec<SearchMatch>,
}

/// Agent-facing next-step recommendation report built from indexed metadata.
#[derive(Debug, Serialize)]
pub struct NextStepReport {
    /// Task/navigation query.
    pub query: String,
    /// Top matching folders with concise ranking evidence.
    pub folders: Vec<RankedNode>,
    /// Top matching files with concise ranking evidence.
    pub files: Vec<ClassifiedRankedNode>,
    /// Deterministic follow-up commands for the selected index targets.
    pub suggestions: Vec<String>,
}

/// One ranked file row with its persisted content role.
#[derive(Debug, Serialize)]
pub struct ClassifiedRankedNode {
    /// Existing compatibility-preserving ranked node payload.
    #[serde(flatten)]
    pub ranked: RankedNode,
    /// Registry-owned role of the ranked file.
    pub classification: ContentClassification,
}

impl Deref for ClassifiedRankedNode {
    type Target = RankedNode;

    fn deref(&self) -> &Self::Target {
        &self.ranked
    }
}

/// Exact code slice returned after orientation.
#[derive(Debug, Serialize)]
pub struct CodeSlice {
    /// Repository-relative path.
    pub path: String,
    /// Persisted role of the indexed file, when an index-backed entry point was used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classification: Option<ContentClassification>,
    /// One-based start line.
    pub start_line: usize,
    /// One-based end line.
    pub end_line: usize,
    /// Total source line count.
    pub line_count: usize,
    /// Estimated tokens for the slice.
    pub estimated_tokens: usize,
    /// Slice content.
    pub content: String,
}

/// Unrendered exact slice with its selected adapter-output ceiling.
#[derive(Debug)]
pub struct CodeSliceDraft {
    /// Compatibility-preserving exact slice payload.
    slice: CodeSlice,
    /// Exact adapter-output ceiling selected for this slice.
    output_budget: CodeSliceBudget,
}

impl CodeSliceDraft {
    /// Borrow the compatibility-preserving slice payload.
    #[must_use]
    pub const fn slice(&self) -> &CodeSlice {
        &self.slice
    }

    /// Encode this slice and enforce the selected adapter-output ceiling.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when encoding fails or the exact encoded
    /// output exceeds the selected byte ceiling.
    pub fn fit_output<F, E, O>(&self, encode: F) -> Result<O, E>
    where
        F: FnOnce(&CodeSlice) -> Result<O, E>,
        E: From<ServiceError>,
        O: AsRef<[u8]>,
    {
        let output = encode(&self.slice)?;
        if output.as_ref().len() > self.output_budget.output_bytes() as usize {
            return Err(E::from(ServiceError::InvalidInput(format!(
                "slice output exceeds the requested {}-byte ceiling; narrow the line or symbol range or raise output-bytes",
                self.output_budget.output_bytes()
            ))));
        }
        Ok(output)
    }
}

/// Encoded-output ceiling shared by line and symbol slices.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeSliceBudget {
    /// Maximum bytes emitted by the selected adapter.
    output_bytes: u32,
}

impl CodeSliceBudget {
    /// Compatibility-preserving default for callers that omit a byte ceiling.
    pub const DEFAULT_OUTPUT_BYTES: u32 = 256 * 1_024;

    /// Validate one requested exact-output ceiling.
    ///
    /// # Errors
    ///
    /// Returns an error when the limit is zero or above the shared product
    /// output ceiling.
    pub fn new(output_bytes: u32) -> ServiceResult<Self> {
        if output_bytes == 0 || output_bytes > GraphLimits::MAX_OUTPUT_BYTES {
            return Err(ServiceError::InvalidInput(format!(
                "slice output byte limit must be between 1 and {}",
                GraphLimits::MAX_OUTPUT_BYTES
            )));
        }
        Ok(Self { output_bytes })
    }

    /// Return the exact adapter-output ceiling.
    #[must_use]
    pub const fn output_bytes(self) -> u32 {
        self.output_bytes
    }
}

impl Default for CodeSliceBudget {
    fn default() -> Self {
        Self {
            output_bytes: Self::DEFAULT_OUTPUT_BYTES,
        }
    }
}

/// Optional selectors for disambiguating a symbol slice.
#[derive(Debug, Default)]
pub struct SymbolSliceSelector<'a> {
    /// Symbol name to locate.
    pub name: &'a str,
    /// Optional parent symbol, such as a class or struct name.
    pub parent: Option<&'a str>,
    /// Optional symbol kind, such as `function`, `method`, or `struct`.
    pub kind: Option<&'a str>,
    /// Optional exact normalized declaration signature.
    pub signature: Option<&'a str>,
    /// Optional line that must fall inside the selected symbol range.
    pub line: Option<usize>,
}

impl From<&SymbolRelation> for FileCallSummary {
    fn from(relation: &SymbolRelation) -> Self {
        Self {
            source: relation.source_name.clone(),
            target: relation.target_name.clone(),
            line: relation.line,
            context: relation.context.clone(),
        }
    }
}

/// Build structured file intelligence from the durable index.
///
/// # Errors
///
/// Returns an error when the file path is invalid, not indexed, or indexed
/// metadata cannot be read.
pub fn build_file_summary(
    store: &AtlasStore,
    file: &Path,
    limit: usize,
) -> ServiceResult<FileSummaryReport> {
    build_file_summary_with_selection(store, file, limit, ContentSelection::UnspecifiedLegacy)
}

/// Build structured file intelligence after enforcing classified-content selection.
///
/// # Errors
///
/// Returns an error when the file is outside the explicit selection or the
/// ordinary summary read fails.
pub fn build_file_summary_with_selection(
    store: &AtlasStore,
    file: &Path,
    limit: usize,
    content_selection: ContentSelection,
) -> ServiceResult<FileSummaryReport> {
    let file_key = validated_indexed_file_key(store, file)?;
    let classification = selected_file_classification(store, &file_key, content_selection)?;
    let source_read =
        indexed_native_path(store, &file_key).and_then(|path| read_file_content(&path));
    match source_read {
        Ok(content) => build_file_summary_with_source_state(
            store,
            file_key,
            limit,
            Some(&content),
            SOURCE_STATUS_LIVE.to_string(),
            String::new(),
            classification,
        ),
        Err(error) => build_file_summary_with_source_state(
            store,
            file_key,
            limit,
            None,
            SOURCE_STATUS_INDEXED.to_string(),
            error.to_string(),
            classification,
        ),
    }
}

/// Build structured file intelligence from caller-verified source bytes.
///
/// # Errors
///
/// Returns an error when the file path is invalid, not indexed, or indexed
/// metadata cannot be read.
pub fn build_file_summary_from_source(
    store: &AtlasStore,
    file: &Path,
    limit: usize,
    source: &str,
) -> ServiceResult<FileSummaryReport> {
    build_file_summary_from_source_with_selection(
        store,
        file,
        limit,
        source,
        ContentSelection::UnspecifiedLegacy,
    )
}

/// Build structured file intelligence from verified source after selection.
///
/// # Errors
///
/// Returns an error when the file is outside the explicit selection or the
/// ordinary summary read fails.
pub fn build_file_summary_from_source_with_selection(
    store: &AtlasStore,
    file: &Path,
    limit: usize,
    source: &str,
    content_selection: ContentSelection,
) -> ServiceResult<FileSummaryReport> {
    let file_key = validated_indexed_file_key(store, file)?;
    let classification = selected_file_classification(store, &file_key, content_selection)?;
    build_file_summary_with_source_state(
        store,
        file_key,
        limit,
        Some(source),
        SOURCE_STATUS_LIVE.to_string(),
        String::new(),
        classification,
    )
}

/// Build one summary from optional already-selected live source.
fn build_file_summary_with_source_state(
    store: &AtlasStore,
    file_key: String,
    limit: usize,
    file_content: Option<&str>,
    source_status: String,
    source_error: String,
    classification: ContentClassification,
) -> ServiceResult<FileSummaryReport> {
    let effective_limit = limit.max(1);
    let indexed = store
        .load_node_by_path(&file_key)?
        .ok_or_else(|| ServiceError::InvalidInput(format!("file {file_key:?} is not indexed")))?;
    let metadata_symbols = store.load_symbols_by_kinds(
        &file_key,
        &metadata_symbol_kinds(),
        FILE_METADATA_SYMBOL_LIMIT,
    )?;
    let line_count = file_content.map_or_else(
        || store.max_symbol_end_line_for_path(&file_key),
        |content| Ok(line_count_from_content(content)),
    )?;
    let docstring = file_content
        .and_then(file_level_docstring)
        .unwrap_or_else(|| file_docstring(&metadata_symbols));
    let function_symbols =
        store.load_symbols_by_kinds(&file_key, &[SymbolKind::Function], effective_limit)?;
    let method_symbols =
        store.load_symbols_by_kinds(&file_key, &[SymbolKind::Method], effective_limit)?;
    let class_symbols =
        store.load_symbols_by_kinds(&file_key, &[SymbolKind::Class], effective_limit)?;
    let type_kinds = type_symbol_kinds();
    let type_symbols = store.load_symbols_by_kinds(&file_key, &type_kinds, effective_limit)?;
    let summarized_symbols = summarized_symbol_set(
        &function_symbols,
        &method_symbols,
        &class_symbols,
        &type_symbols,
    );
    let summarized_names = symbol_names(&summarized_symbols);
    let symbol_name_counts = store.symbol_name_counts(&summarized_names)?;
    let alias_scope_symbols = store.load_symbols_by_names(&summarized_names)?;
    let alias_counts = symbol_alias_counts(&alias_scope_symbols);
    let import_aliases = load_import_alias_map(store, &summarized_symbols, &alias_counts)?;
    let caller_targets = caller_target_names(&summarized_symbols, &import_aliases);
    let caller_relations =
        store.load_call_relations_to_targets(&caller_targets, CALLER_RELATION_LIMIT_PER_TARGET)?;
    let called_by = called_by_map(
        &summarized_symbols,
        &caller_relations,
        &symbol_name_counts,
        &alias_counts,
        &import_aliases,
    );
    let functions = summarize_symbols(&function_symbols, &called_by);
    let methods = summarize_symbols(&method_symbols, &called_by);
    let classes = summarize_symbols(&class_symbols, &called_by);
    let types = summarize_symbols(&type_symbols, &called_by);
    let imports = store.load_distinct_relation_targets_by_kind(
        &file_key,
        RelationKind::Imports,
        effective_limit,
    )?;
    let dependencies = store.load_distinct_relation_targets_by_kind(
        &file_key,
        RelationKind::DependsOn,
        effective_limit,
    )?;
    let exports = store.load_exported_symbol_names_for_path(&file_key, effective_limit)?;
    let calls = store
        .load_symbol_relations_by_kind(&file_key, RelationKind::Calls, effective_limit)?
        .iter()
        .map(FileCallSummary::from)
        .collect::<Vec<_>>();
    let total_functions = store.count_symbols_by_kinds(&file_key, &[SymbolKind::Function])?;
    let total_methods = store.count_symbols_by_kinds(&file_key, &[SymbolKind::Method])?;
    let total_classes = store.count_symbols_by_kinds(&file_key, &[SymbolKind::Class])?;
    let total_types = store.count_symbols_by_kinds(&file_key, &type_kinds)?;
    let total_calls = store.count_symbol_relations_by_kind(&file_key, RelationKind::Calls)?;
    let total_imports =
        store.count_distinct_relation_targets_by_kind(&file_key, RelationKind::Imports)?;
    let total_dependencies =
        store.count_distinct_relation_targets_by_kind(&file_key, RelationKind::DependsOn)?;
    let total_exports = store.exported_symbol_count_for_path(&file_key)?;
    let symbol_count = store.symbol_count_for_path(&file_key)?;
    let symbol_parser_kinds = store.symbol_parser_kinds_for_path(&file_key)?;
    let parse_metadata = store.load_source_parse_metadata(&file_key)?;
    let coverage = load_coverage_digest(store, &file_key, parse_metadata.as_ref())?;
    let truncated = [
        total_functions,
        total_methods,
        total_classes,
        total_types,
        total_calls,
        total_imports,
        total_dependencies,
        total_exports,
    ]
    .iter()
    .any(|total| *total > effective_limit);

    let content_summary = indexed.summary.unwrap_or_default();
    let parser_kind = summary_parser_kind(
        &content_summary,
        symbol_count,
        &symbol_parser_kinds,
        parse_metadata.as_ref(),
    )
    .to_string();
    let summary_status = summary_status(
        &content_summary,
        symbol_count,
        &symbol_parser_kinds,
        parse_metadata.as_ref(),
    )
    .to_string();

    Ok(FileSummaryReport {
        file_path: file_key,
        language: indexed
            .node
            .language
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        classification,
        line_count,
        file_purpose: indexed.purpose.purpose.clone().unwrap_or_default(),
        file_purpose_status: indexed.purpose.status.to_string(),
        file_purpose_source: indexed.purpose.source.to_string(),
        file_purpose_agent_reviewed: indexed.purpose.agent_reviewed(),
        content_summary,
        package: package_name(&metadata_symbols),
        docstring,
        symbol_count,
        source_status,
        source_error,
        parser_kind,
        summary_status,
        limit: effective_limit,
        total_functions,
        total_methods,
        total_classes,
        total_types,
        total_calls,
        total_imports,
        total_dependencies,
        total_exports,
        truncated,
        functions,
        methods,
        classes,
        types,
        imports,
        dependencies,
        exports,
        calls,
        coverage,
    })
}

/// Serialize the exact file summary payload for token telemetry.
///
/// # Errors
///
/// Returns an error when the summary payload cannot be serialized.
pub fn file_summary_baseline_text(report: &FileSummaryReport) -> ServiceResult<String> {
    Ok(serde_json::to_string(report)?)
}

/// Return the parser family implied by stored summary and parser metadata.
fn summary_parser_kind(
    summary: &str,
    symbol_count: usize,
    parser_kinds: &[ParserKind],
    parse_metadata: Option<&SourceParseMetadata>,
) -> &'static str {
    if symbol_count > 0 && !parser_kinds.is_empty() {
        return symbol_parser_kind(parser_kinds);
    }
    if let Some(metadata) = parse_metadata {
        return parser_kind_label(metadata.parser);
    }
    if is_symbol_graph_empty_summary(summary) {
        "symbol-graph"
    } else if summary.is_empty() {
        "missing"
    } else if is_scanner_fallback_summary(summary) {
        "scanner-metadata"
    } else {
        "structural"
    }
}

/// Return a summary quality status for agent consumers.
fn summary_status(
    summary: &str,
    symbol_count: usize,
    parser_kinds: &[ParserKind],
    parse_metadata: Option<&SourceParseMetadata>,
) -> &'static str {
    if summary.is_empty() {
        "missing"
    } else if is_scanner_fallback_summary(summary)
        || parse_metadata.is_some_and(|metadata| metadata.parser == ParserKind::Fallback)
        || fallback_only_symbols(symbol_count, parser_kinds)
    {
        "fallback"
    } else {
        "ok"
    }
}

/// Return the public parser label for one file-level parser strategy.
fn parser_kind_label(parser: ParserKind) -> &'static str {
    match parser {
        ParserKind::TreeSitter => "tree-sitter-symbol-graph",
        ParserKind::Manifest => "manifest-symbol-graph",
        ParserKind::Structural => "structural-symbol-graph",
        ParserKind::Fallback => "fallback-symbol-graph",
    }
}

/// Return the parser family for a non-empty symbol graph.
fn symbol_parser_kind(parser_kinds: &[ParserKind]) -> &'static str {
    let has_tree_sitter = parser_kinds.contains(&ParserKind::TreeSitter);
    let has_manifest = parser_kinds.contains(&ParserKind::Manifest);
    let has_structural = parser_kinds.contains(&ParserKind::Structural);
    let has_fallback = parser_kinds.contains(&ParserKind::Fallback);
    let family_count = usize::from(has_tree_sitter)
        .saturating_add(usize::from(has_manifest))
        .saturating_add(usize::from(has_structural))
        .saturating_add(usize::from(has_fallback));
    match (
        family_count,
        has_tree_sitter,
        has_manifest,
        has_structural,
        has_fallback,
    ) {
        (1, true, false, false, false) => "tree-sitter-symbol-graph",
        (1, false, true, false, false) => "manifest-symbol-graph",
        (1, false, false, true, false) => "structural-symbol-graph",
        (1, false, false, false, true) => "fallback-symbol-graph",
        _ => "mixed-symbol-graph",
    }
}

/// Return whether the only available symbol graph was created by fallback parsing.
fn fallback_only_symbols(symbol_count: usize, parser_kinds: &[ParserKind]) -> bool {
    symbol_count > 0
        && !parser_kinds.is_empty()
        && parser_kinds
            .iter()
            .all(|parser_kind| *parser_kind == ParserKind::Fallback)
}

/// Return whether a no-declaration source summary came from the symbol graph.
fn is_symbol_graph_empty_summary(summary: &str) -> bool {
    summary
        .trim()
        .ends_with("source file with no declarations found.")
}

/// Return whether a summary is only the filesystem byte-count fallback.
fn is_scanner_fallback_summary(summary: &str) -> bool {
    let trimmed = summary.trim_end_matches('.');
    let Some((_, tail)) = trimmed.rsplit_once(", ") else {
        return false;
    };
    let Some(number) = tail.strip_suffix(" bytes") else {
        return false;
    };
    !number.is_empty() && number.chars().all(|character| character.is_ascii_digit())
}

/// Search indexed project files with bounded source reads and `globset` filters.
///
/// # Errors
///
/// Returns an error when the index is unavailable, the regex or glob is
/// invalid, or an indexed file cannot be read.
pub fn search_indexed_files(
    store: &AtlasStore,
    pattern: &str,
    regex: bool,
    fuzzy: bool,
    case_sensitive: bool,
    file_pattern: Option<&str>,
    context_lines: usize,
    start_index: usize,
    limit: usize,
) -> ServiceResult<SearchReport> {
    search_indexed_files_with_control(
        store,
        &SearchQuery {
            pattern,
            regex,
            fuzzy,
            case_sensitive,
            file_pattern,
            context_lines,
            start_index,
            limit,
            content_selection: ContentSelection::UnspecifiedLegacy,
            retrieval_mode: SearchRetrievalMode::Lexical,
        },
        None,
    )
}

/// Search indexed project files through one bounded, cancellable retrieval request.
///
/// Safe ASCII literal tokens may use the rebuildable FTS5 projection only as a
/// complete metadata candidate superset. Persisted `file_texts` remains the
/// authority and every candidate is exact-verified in deterministic path order.
/// All other shapes use the persisted-text fallback with path admission before
/// content decoding.
///
/// # Errors
///
/// Returns a typed capability error for unavailable semantic/hybrid requests,
/// or an error when input, storage, cancellation, or persisted text is invalid.
pub fn search_indexed_files_with_control(
    store: &AtlasStore,
    query: &SearchQuery<'_>,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<SearchReport> {
    search_indexed_files_with_bounds(store, query, control, DEFAULT_SEARCH_BOUNDS)
}

/// Execute one search under an explicit internal resource envelope.
fn search_indexed_files_with_bounds(
    store: &AtlasStore,
    query: &SearchQuery<'_>,
    control: Option<&IndexWorkControl>,
    bounds: SearchBounds,
) -> ServiceResult<SearchReport> {
    if query.retrieval_mode != SearchRetrievalMode::Lexical {
        return Err(ServiceError::SearchCapabilityUnavailable {
            requested_mode: query.retrieval_mode,
            state: SEARCH_SEMANTIC_UNAVAILABLE_STATE,
            guidance: SEARCH_SEMANTIC_RECOVERY,
        });
    }
    if query.regex && query.fuzzy {
        return Err(ServiceError::InvalidInput(
            "search cannot combine regex and fuzzy modes".to_string(),
        ));
    }
    if query.pattern.len() > SEARCH_MAX_PATTERN_BYTES {
        return Err(ServiceError::InvalidInput(format!(
            "search pattern cannot exceed {SEARCH_MAX_PATTERN_BYTES} UTF-8 bytes"
        )));
    }
    if query
        .file_pattern
        .is_some_and(|pattern| pattern.len() > SEARCH_MAX_FILE_PATTERN_BYTES)
    {
        return Err(ServiceError::InvalidInput(format!(
            "search file pattern cannot exceed {SEARCH_MAX_FILE_PATTERN_BYTES} UTF-8 bytes"
        )));
    }
    if query.context_lines > SEARCH_MAX_CONTEXT_LINES {
        return Err(ServiceError::InvalidInput(format!(
            "search context lines cannot exceed {SEARCH_MAX_CONTEXT_LINES}"
        )));
    }
    if query.limit > SEARCH_MAX_RESULT_ROWS {
        return Err(ServiceError::InvalidInput(format!(
            "search result limit cannot exceed {SEARCH_MAX_RESULT_ROWS}"
        )));
    }
    let path_matcher = build_path_matcher(query.file_pattern)?;
    let matcher = if query.regex {
        LineMatcher::Regex(
            RegexBuilder::new(query.pattern)
                .case_insensitive(!query.case_sensitive)
                .build()
                .map_err(|source| ServiceError::InvalidInput(source.to_string()))?,
        )
    } else if query.fuzzy {
        LineMatcher::Fuzzy {
            needle: normalized_search_text(query.pattern, query.case_sensitive),
            case_sensitive: query.case_sensitive,
        }
    } else {
        LineMatcher::Literal {
            needle: normalized_search_text(query.pattern, query.case_sensitive),
            case_sensitive: query.case_sensitive,
        }
    };
    let mut report = SearchReport {
        query: query.pattern.to_string(),
        mode: matcher.mode().to_string(),
        retrieval_mode: query.retrieval_mode.as_str().to_string(),
        content_selection: query
            .content_selection
            .explicit_value()
            .map(|_| query.content_selection),
        source: "sqlite-file-text".to_string(),
        strategy: "persisted-text-fallback".to_string(),
        start_index: query.start_index,
        total: 0,
        observed_total: 0,
        total_is_complete: true,
        returned: 0,
        searched_files: 0,
        searched_bytes: 0,
        candidate_files: 0,
        retained_bytes: 0,
        truncated: false,
        truncation_reason: None,
        results: Vec::new(),
    };
    let bounded_control = control.map_or_else(
        || IndexWorkControl::new(IndexCancellation::new(), Some(bounds.elapsed)),
        |control| control.with_timeout_ceiling(bounds.elapsed),
    );
    if let Err(failure) = bounded_control.check(IndexWorkStage::TextIndex) {
        if matches!(failure, IndexWorkFailure::DeadlineExceeded { .. }) {
            mark_search_truncated(&mut report, "elapsed-time-limit");
            return Ok(finalize_search_report(report));
        }
        return Err(DbError::from(failure).into());
    }
    if query.limit == 0 {
        return Ok(report);
    }
    let needed = query.start_index.saturating_add(query.limit);
    let path_prefix = search_path_prefix(query.file_pattern);
    let mut used_fts = false;
    if let Some(literal_token) = matcher.fts_literal_token()
        && store.file_text_fts_ready()?
    {
        let mut page = match store.query_file_text_fts_candidates(
            &FileTextFtsQuery {
                literal_token,
                path_prefix: path_prefix.as_deref(),
                limit: MAX_FILE_TEXT_FTS_CANDIDATES,
            },
            Some(&bounded_control),
        ) {
            Ok(page) => page,
            Err(error) if is_search_deadline(&error) => {
                mark_search_truncated(&mut report, "elapsed-time-limit");
                return Ok(finalize_search_report(report));
            }
            Err(error) => return Err(error.into()),
        };
        report.candidate_files = page.candidates.len();
        if !page.overflow {
            page.candidates
                .sort_by(|left, right| left.path.cmp(&right.path));
            let classifications = file_content_classifications_by_path(
                store,
                page.candidates
                    .iter()
                    .map(|candidate| candidate.path.clone()),
            )?;
            report.strategy = "fts5-bm25-candidates-exact-verified".to_string();
            used_fts = true;
            for candidate in page.candidates {
                if !path_matches(&candidate.path, path_matcher.as_ref()) {
                    continue;
                }
                let classification =
                    classifications
                        .get(&candidate.path)
                        .copied()
                        .ok_or_else(|| {
                            ServiceError::InvalidInput(format!(
                                "FTS candidate {:?} has no content classification",
                                candidate.path
                            ))
                        })?;
                if !query.content_selection.includes(classification) {
                    continue;
                }
                if let Err(failure) = bounded_control.check(IndexWorkStage::RepositoryTraversal) {
                    if matches!(failure, IndexWorkFailure::DeadlineExceeded { .. }) {
                        mark_search_truncated(&mut report, "elapsed-time-limit");
                        break;
                    }
                    return Err(DbError::from(failure).into());
                }
                if !search_metadata_within_bounds(
                    &mut report,
                    candidate.byte_count,
                    bounds.selected_files,
                    bounds.selected_bytes,
                ) {
                    break;
                }
                let text = store.load_file_text(&candidate.path)?.ok_or_else(|| {
                    ServiceError::InvalidInput(format!(
                        "FTS candidate {:?} has no authoritative persisted text",
                        candidate.path
                    ))
                })?;
                if text.byte_count != candidate.byte_count
                    || text.line_count != candidate.line_count
                    || text.content_hash != candidate.content_hash
                {
                    return Err(ServiceError::InvalidInput(format!(
                        "FTS candidate metadata changed for {:?}",
                        candidate.path
                    )));
                }
                report.searched_files += 1;
                report.searched_bytes += candidate.byte_count;
                if let Err(failure) = inspect_search_text(
                    &mut report,
                    &text,
                    classification,
                    &matcher,
                    query.context_lines,
                    needed,
                    bounds.retained_bytes,
                    &bounded_control,
                ) {
                    if matches!(failure, IndexWorkFailure::DeadlineExceeded { .. }) {
                        mark_search_truncated(&mut report, "elapsed-time-limit");
                        break;
                    }
                    return Err(DbError::from(failure).into());
                }
                if report.truncated {
                    break;
                }
            }
        }
    }
    if !used_fts {
        let mut selected_files = 0usize;
        let mut selected_bytes = 0usize;
        let mut searched_files = 0usize;
        let mut searched_bytes = 0usize;
        let mut admission_truncation = None;
        let admitted_classification = Cell::new(None);
        let fallback_result = store.visit_file_texts_for_fallback(
            path_prefix.as_deref(),
            Some(&bounded_control),
            |metadata| {
                if !path_matches(&metadata.path, path_matcher.as_ref()) {
                    return Ok(FileTextAdmission::Skip);
                }
                if !query.content_selection.includes(metadata.classification) {
                    return Ok(FileTextAdmission::Skip);
                }
                if selected_files >= bounds.selected_files {
                    admission_truncation = Some("selected-file-limit");
                    return Ok(FileTextAdmission::Stop);
                }
                let Some(next_bytes) = selected_bytes.checked_add(metadata.byte_count) else {
                    admission_truncation = Some("selected-byte-limit");
                    return Ok(FileTextAdmission::Stop);
                };
                if next_bytes > bounds.selected_bytes {
                    admission_truncation = Some("selected-byte-limit");
                    return Ok(FileTextAdmission::Stop);
                }
                selected_files += 1;
                selected_bytes = next_bytes;
                admitted_classification.set(Some(metadata.classification));
                Ok(FileTextAdmission::Read)
            },
            |text| {
                let classification = admitted_classification.take().ok_or_else(|| {
                    DbError::FileContentClassificationMissing {
                        path: text.path.clone(),
                    }
                })?;
                searched_files += 1;
                searched_bytes += text.byte_count;
                inspect_search_text(
                    &mut report,
                    &text,
                    classification,
                    &matcher,
                    query.context_lines,
                    needed,
                    bounds.retained_bytes,
                    &bounded_control,
                )
                .map_err(DbError::from)?;
                Ok(!report.truncated)
            },
        );
        report.searched_files = searched_files;
        report.searched_bytes = searched_bytes;
        match fallback_result {
            Ok(()) => {}
            Err(error) if is_search_deadline(&error) => {
                mark_search_truncated(&mut report, "elapsed-time-limit");
            }
            Err(error) => return Err(error.into()),
        }
        if let Some(reason) = admission_truncation {
            mark_search_truncated(&mut report, reason);
        }
    }
    Ok(finalize_search_report(report))
}

/// Finalize counters that describe the bounded work observed by one search.
fn finalize_search_report(mut report: SearchReport) -> SearchReport {
    report.returned = report.results.len();
    report.observed_total = report.total;
    report.total_is_complete = !report.truncated;
    report
}

/// Return whether a database read stopped only because its deadline elapsed.
fn is_search_deadline(error: &DbError) -> bool {
    matches!(
        error,
        DbError::IndexWork(IndexWorkFailure::DeadlineExceeded { .. })
    )
}

/// Filter file nodes through a repository-relative glob.
///
/// # Errors
///
/// Returns an error when `file_pattern` is not a valid repository glob.
pub fn filter_files_by_glob(
    nodes: Vec<IndexedNode>,
    file_pattern: Option<&str>,
) -> ServiceResult<Vec<IndexedNode>> {
    let matcher = FilePathMatcher::new(file_pattern)?;
    Ok(nodes
        .into_iter()
        .filter(|node| node.node.kind == NodeKind::File)
        .filter(|node| matcher.is_match(&node.node.path))
        .collect())
}

/// Load ranked file nodes and apply the shared repository-relative glob policy.
///
/// # Errors
///
/// Returns an error when the file pattern is invalid or indexed nodes cannot be
/// loaded.
pub fn load_ranked_file_nodes(
    store: &AtlasStore,
    query: &str,
    folder: Option<&str>,
    file_pattern: Option<&str>,
    limit: usize,
    include_content: bool,
) -> ServiceResult<Vec<IndexedNode>> {
    let target = limit.max(1);
    let selected = load_ranked_file_node_candidates(
        store,
        query,
        folder,
        file_pattern,
        target,
        include_content,
    )?;
    Ok(ranked_nodes_with_reasons(store, query, selected)?
        .into_iter()
        .take(target)
        .map(|ranked| ranked.node)
        .collect())
}

/// Load the bounded file candidate set before final graph-aware truncation.
fn load_ranked_file_node_candidates(
    store: &AtlasStore,
    query: &str,
    folder: Option<&str>,
    file_pattern: Option<&str>,
    target: usize,
    include_content: bool,
) -> ServiceResult<Vec<IndexedNode>> {
    let matcher = FilePathMatcher::new(file_pattern)?;
    let candidate_target = ranked_candidate_target(query, target);
    let mut selected = if matcher.filters() {
        load_ranked_file_nodes_matching_glob(store, query, folder, &matcher, candidate_target)?
    } else {
        store.load_ranked_nodes(query, NodeKind::File, folder, candidate_target, 0)?
    };
    if include_content && !query.trim().is_empty() && selected.len() < candidate_target {
        append_content_ranked_file_nodes(
            store,
            query,
            folder,
            &matcher,
            candidate_target,
            &mut selected,
        )?;
    }
    append_paired_file_nodes(store, &matcher, candidate_target, &mut selected)?;
    Ok(selected)
}

/// Load the bounded eligible file candidate set before final ranking.
fn load_ranked_file_node_candidates_with_selection(
    store: &AtlasStore,
    query: &str,
    folder: Option<&str>,
    file_pattern: Option<&str>,
    target: usize,
    include_content: bool,
    content_selection: ContentSelection,
) -> ServiceResult<Vec<IndexedNode>> {
    if content_selection == ContentSelection::UnspecifiedLegacy {
        return load_ranked_file_node_candidates(
            store,
            query,
            folder,
            file_pattern,
            target,
            include_content,
        );
    }
    let matcher = FilePathMatcher::new(file_pattern)?;
    let candidate_target = ranked_candidate_target(query, target);
    let mut selected = load_ranked_file_nodes_matching_selection(
        store,
        query,
        folder,
        &matcher,
        candidate_target,
        content_selection,
    )?;
    if include_content && !query.trim().is_empty() && selected.len() < candidate_target {
        append_content_ranked_file_nodes_selected(
            store,
            query,
            folder,
            &matcher,
            candidate_target,
            content_selection,
            &mut selected,
        )?;
    }
    append_paired_file_nodes_selected(
        store,
        &matcher,
        candidate_target,
        content_selection,
        &mut selected,
    )?;
    Ok(selected)
}

/// Load ranked folders with concise reasons.
///
/// # Errors
///
/// Returns an error when indexed folder metadata cannot be loaded.
pub fn load_ranked_folder_nodes_with_reasons(
    store: &AtlasStore,
    query: &str,
    limit: usize,
) -> ServiceResult<Vec<RankedNode>> {
    let target = limit.max(1);
    let candidate_target = ranked_candidate_target(query, target);
    let selected = store.load_ranked_nodes(query, NodeKind::Folder, None, candidate_target, 0)?;
    let mut ranked = ranked_nodes_with_reasons(store, query, selected)?;
    ranked.truncate(target);
    Ok(ranked)
}

/// Load ranked files with concise reasons.
///
/// # Errors
///
/// Returns an error when indexed file metadata cannot be loaded or filters are invalid.
pub fn load_ranked_file_nodes_with_reasons(
    store: &AtlasStore,
    query: &str,
    folder: Option<&str>,
    file_pattern: Option<&str>,
    limit: usize,
    include_content: bool,
) -> ServiceResult<Vec<RankedNode>> {
    let target = limit.max(1);
    let selected = load_ranked_file_node_candidates(
        store,
        query,
        folder,
        file_pattern,
        target,
        include_content,
    )?;
    let mut ranked = ranked_nodes_with_reasons(store, query, selected)?;
    ranked.truncate(target);
    Ok(ranked)
}

/// Load ranked files with persisted classification and pre-ranking selection.
///
/// # Errors
///
/// Returns an error when indexed metadata, classification, or filters are invalid.
pub fn load_classified_ranked_file_nodes_with_reasons(
    store: &AtlasStore,
    query: &str,
    folder: Option<&str>,
    file_pattern: Option<&str>,
    limit: usize,
    include_content: bool,
    content_selection: ContentSelection,
) -> ServiceResult<Vec<ClassifiedRankedNode>> {
    let target = limit.max(1);
    let selected = load_ranked_file_node_candidates_with_selection(
        store,
        query,
        folder,
        file_pattern,
        target,
        include_content,
        content_selection,
    )?;
    let mut ranked = ranked_nodes_with_reasons(store, query, selected)?;
    ranked.truncate(target);
    let classifications = file_content_classifications_by_path(
        store,
        ranked.iter().map(|node| node.node.node.path.clone()),
    )?;
    ranked
        .into_iter()
        .map(|ranked| {
            let classification = classifications
                .get(&ranked.node.node.path)
                .copied()
                .ok_or_else(|| {
                    ServiceError::InvalidInput(format!(
                        "ranked file {:?} has no content classification",
                        ranked.node.node.path
                    ))
                })?;
            Ok(ClassifiedRankedNode {
                ranked,
                classification,
            })
        })
        .collect()
}

/// Build an indexed-metadata recommendation report for the next inspection step.
///
/// # Errors
///
/// Returns an error when indexed folder or file metadata cannot be loaded.
pub fn build_next_report(
    store: &AtlasStore,
    query: &str,
    limit: Option<usize>,
) -> ServiceResult<NextStepReport> {
    build_next_report_with_selection(store, query, limit, ContentSelection::UnspecifiedLegacy)
}

/// Build an indexed recommendation report with classified file selection.
///
/// # Errors
///
/// Returns an error when indexed folder, file, or classification metadata cannot be loaded.
pub fn build_next_report_with_selection(
    store: &AtlasStore,
    query: &str,
    limit: Option<usize>,
    content_selection: ContentSelection,
) -> ServiceResult<NextStepReport> {
    let target = limit
        .unwrap_or(NEXT_REPORT_DEFAULT_LIMIT)
        .clamp(1, NEXT_REPORT_MAX_LIMIT);
    let folders = load_ranked_folder_nodes_with_reasons(store, query, target)?;
    let files = load_classified_ranked_file_nodes_with_reasons(
        store,
        query,
        None,
        None,
        target,
        true,
        content_selection,
    )?;
    let suggestions = next_report_suggestions(query, &folders, &files, content_selection);
    Ok(NextStepReport {
        query: query.to_string(),
        folders,
        files,
        suggestions,
    })
}

#[derive(Debug)]
/// Score and evidence computed for one ranked node.
struct RankedEvidence {
    /// Exact normalized full-path dominance tier.
    exact_path: bool,
    /// Exact normalized basename dominance tier.
    exact_name: bool,
    /// Reviewed responsibility-purpose dominance tier.
    reviewed_purpose: bool,
    /// Bounded lexical and query-relevant graph context score.
    context_score: usize,
    /// Concise evidence strings emitted to the agent-facing result.
    reasons: Vec<String>,
    /// Compact stable evidence emitted to programmatic consumers.
    reason_codes: Vec<RankedReasonCode>,
}

/// Return the bounded candidate count used before final ranking truncation.
fn ranked_candidate_target(query: &str, target: usize) -> usize {
    if query.trim().is_empty() {
        target
    } else {
        target
            .saturating_mul(3)
            .clamp(target, RANKED_CANDIDATE_LIMIT)
    }
}

/// Rank and enrich one bounded candidate set through a single graph batch call.
fn ranked_nodes_with_reasons(
    store: &AtlasStore,
    query: &str,
    selected: Vec<IndexedNode>,
) -> ServiceResult<Vec<RankedNode>> {
    let terms = normalize_ranking_terms(query);
    let text_hit_paths = indexed_text_hit_paths(store, &selected, &terms)?;
    let owners = selected
        .iter()
        .map(|node| RepositoryNavigationNode {
            path: node.node.path.clone(),
            kind: node.node.kind,
        })
        .collect::<Vec<_>>();
    let connections = store.repository_navigation_connections(
        &owners,
        RANKED_CONNECTION_FAMILY_LIMIT,
        RANKED_CONNECTION_SAMPLE_LIMIT,
    )?;
    let mut connections_by_path = connections
        .into_iter()
        .map(|page| (page.path.clone(), page))
        .collect::<HashMap<_, _>>();
    let exact_query = normalize_exact_ranking_query(query);
    let mut scored = selected
        .into_iter()
        .enumerate()
        .map(|(index, node)| {
            let page = connections_by_path.remove(&node.node.path).ok_or_else(|| {
                ServiceError::InvalidInput(format!(
                    "graph navigation batch omitted indexed path {:?}",
                    node.node.path
                ))
            })?;
            let evidence =
                ranked_node_evidence(store, &node, &terms, &exact_query, &text_hit_paths, &page)?;
            let next_capability = match node.node.kind {
                NodeKind::Folder => NavigationNextCapability::Files,
                NodeKind::File if page.truncated => NavigationNextCapability::Relations,
                NodeKind::File => NavigationNextCapability::Summary,
            };
            Ok((
                index,
                RankedEvidence {
                    exact_path: evidence.exact_path,
                    exact_name: evidence.exact_name,
                    reviewed_purpose: evidence.reviewed_purpose,
                    context_score: evidence.context_score,
                    reasons: Vec::new(),
                    reason_codes: Vec::new(),
                },
                RankedNode {
                    node,
                    reasons: evidence.reasons,
                    reason_codes: evidence.reason_codes,
                    connection_counts: page.counts,
                    connections: page.connections,
                    connections_truncated: page.truncated,
                    next_call: NavigationNextCall {
                        capability: next_capability,
                        path: owners[index].path.clone(),
                    },
                },
            ))
        })
        .collect::<ServiceResult<Vec<_>>>()?;
    scored.sort_by(|left, right| {
        ranked_evidence_order(&left.1, &right.1)
            .then_with(|| left.0.cmp(&right.0))
            .then_with(|| left.2.node.node.path.cmp(&right.2.node.node.path))
    });
    Ok(scored.into_iter().map(|(_, _, node)| node).collect())
}

/// Compare ranking tiers before stable candidate order and path tie-breakers.
fn ranked_evidence_order(left: &RankedEvidence, right: &RankedEvidence) -> std::cmp::Ordering {
    right
        .exact_path
        .cmp(&left.exact_path)
        .then_with(|| right.exact_name.cmp(&left.exact_name))
        .then_with(|| right.reviewed_purpose.cmp(&left.reviewed_purpose))
        .then_with(|| right.context_score.cmp(&left.context_score))
}

/// Compute score and reasons for one node from indexed metadata.
fn ranked_node_evidence(
    store: &AtlasStore,
    node: &IndexedNode,
    terms: &[String],
    exact_query: &str,
    text_hit_paths: &HashSet<String>,
    connections: &RepositoryNavigationConnections,
) -> ServiceResult<RankedEvidence> {
    let normalized_path = node.node.path.replace('\\', "/").to_lowercase();
    let normalized_name = normalized_path
        .rsplit('/')
        .next()
        .unwrap_or(&normalized_path);
    let exact_path = !exact_query.is_empty() && normalized_path == exact_query;
    let exact_name = !exact_query.is_empty() && normalized_name == exact_query;
    let mut reviewed_purpose = false;
    let mut context_score = 0usize;
    let mut reasons = Vec::new();
    let mut reason_codes = Vec::new();

    if exact_path {
        push_ranked_reason(&mut reasons, "exact path".to_string());
        push_ranked_reason_code(&mut reason_codes, RankedReasonCode::ExactPath);
    }
    if exact_name {
        push_ranked_reason(&mut reasons, "exact name".to_string());
        push_ranked_reason_code(&mut reason_codes, RankedReasonCode::ExactName);
    }

    if let Some(term) = first_matching_term(&node.node.path, terms)
        && !exact_path
    {
        context_score = context_score.saturating_add(40);
        push_ranked_reason(&mut reasons, format!("path matched {term}"));
        push_ranked_reason_code(&mut reason_codes, RankedReasonCode::Path);
    }
    if node.purpose.agent_reviewed()
        && let Some(term) = node
            .purpose
            .purpose
            .as_deref()
            .and_then(|purpose| first_matching_term(purpose, terms))
    {
        reviewed_purpose = true;
        push_ranked_reason(&mut reasons, format!("purpose matched {term}"));
        push_ranked_reason_code(&mut reason_codes, RankedReasonCode::ReviewedPurpose);
    }
    if let Some(term) = node
        .summary
        .as_deref()
        .and_then(|summary| first_matching_term(summary, terms))
    {
        context_score = context_score.saturating_add(20);
        push_ranked_reason(&mut reasons, format!("summary matched {term}"));
        push_ranked_reason_code(&mut reason_codes, RankedReasonCode::Summary);
    }
    if node.node.kind == NodeKind::File {
        if let Some((symbol_name, term)) = first_symbol_match(store, &node.node.path, terms)? {
            context_score = context_score.saturating_add(35);
            push_ranked_reason(&mut reasons, format!("symbol {symbol_name} matched {term}"));
            push_ranked_reason_code(&mut reason_codes, RankedReasonCode::Symbol);
        }
        if text_hit_paths.contains(&node.node.path)
            && let Some(term) = indexed_text_match_term(store, &node.node.path, terms)?
        {
            context_score = context_score.saturating_add(15);
            push_ranked_reason(&mut reasons, format!("indexed text matched {term}"));
            push_ranked_reason_code(&mut reason_codes, RankedReasonCode::IndexedText);
        }
        if let Some(reason) = paired_path_reason(store, &node.node.path)? {
            context_score = context_score.saturating_add(10);
            push_ranked_reason(&mut reasons, reason);
            push_ranked_reason_code(&mut reason_codes, RankedReasonCode::PairedFile);
        }
    }

    let mut graph_context_score = 0usize;
    for count in &connections.counts {
        if count.count == 0 {
            continue;
        }
        graph_context_score = graph_context_score.saturating_add(2);
        push_ranked_reason_code(&mut reason_codes, graph_reason_code(count.kind));
    }
    for connection in &connections.connections {
        if ranked_connection_matches_terms(&connection.target, terms) {
            graph_context_score = graph_context_score.saturating_add(18);
            push_ranked_reason_code(&mut reason_codes, graph_reason_code(connection.kind));
        }
    }
    context_score = context_score.saturating_add(graph_context_score.min(32));

    Ok(RankedEvidence {
        exact_path,
        exact_name,
        reviewed_purpose,
        context_score,
        reasons,
        reason_codes,
    })
}

/// Normalize a complete query for exact path and basename dominance.
fn normalize_exact_ranking_query(query: &str) -> String {
    query.trim().replace('\\', "/").to_lowercase()
}

/// Return whether one compact graph endpoint matches any normalized query term.
fn ranked_connection_matches_terms(target: &RankedConnectionTarget, terms: &[String]) -> bool {
    let fields = match target {
        RankedConnectionTarget::Local { path, symbol } => {
            [Some(path.as_str()), symbol.as_deref(), None]
        }
        RankedConnectionTarget::Package {
            manager,
            name,
            manifest,
        } => [
            Some(manager.as_str()),
            Some(name.as_str()),
            Some(manifest.as_str()),
        ],
        RankedConnectionTarget::External { system, identity } => {
            [Some(system.as_str()), Some(identity.as_str()), None]
        }
        RankedConnectionTarget::Unresolved { reference } => [Some(reference.as_str()), None, None],
    };
    fields
        .into_iter()
        .flatten()
        .any(|field| first_matching_term(field, terms).is_some())
}

/// Map one connection family to its compact ranking signal.
const fn graph_reason_code(kind: RankedConnectionKind) -> RankedReasonCode {
    match kind {
        RankedConnectionKind::Package => RankedReasonCode::GraphPackage,
        RankedConnectionKind::Import => RankedReasonCode::GraphImport,
        RankedConnectionKind::Call => RankedReasonCode::GraphCall,
        RankedConnectionKind::Reference => RankedReasonCode::GraphReference,
        RankedConnectionKind::Test => RankedReasonCode::GraphTest,
        RankedConnectionKind::Route => RankedReasonCode::GraphRoute,
        RankedConnectionKind::Config => RankedReasonCode::GraphConfig,
    }
}

/// Split a query into unique lowercase terms used by ranking evidence.
fn normalize_ranking_terms(query: &str) -> Vec<String> {
    let mut terms = query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    terms
}

/// Return the first normalized query term contained in a text field.
fn first_matching_term(text: &str, terms: &[String]) -> Option<String> {
    let haystack = normalized_search_text(text, false);
    terms
        .iter()
        .find(|term| haystack.contains(term.as_str()))
        .cloned()
}

/// Append a reason when it is unique and the per-result cap allows it.
fn push_ranked_reason(reasons: &mut Vec<String>, reason: String) {
    if reasons.len() < RANKED_REASON_LIMIT && !reasons.contains(&reason) {
        reasons.push(reason);
    }
}

/// Append one unique compact reason code in stable discovery order.
fn push_ranked_reason_code(codes: &mut Vec<RankedReasonCode>, code: RankedReasonCode) {
    if !codes.contains(&code) {
        codes.push(code);
    }
}

/// Return selected file paths whose persisted indexed text matches any term.
fn indexed_text_hit_paths(
    store: &AtlasStore,
    selected: &[IndexedNode],
    terms: &[String],
) -> ServiceResult<HashSet<String>> {
    let mut hits = HashSet::new();
    if terms.is_empty() {
        return Ok(hits);
    }
    for node in selected
        .iter()
        .filter(|node| node.node.kind == NodeKind::File)
    {
        if indexed_text_match_term(store, &node.node.path, terms)?.is_some() {
            hits.insert(node.node.path.clone());
        }
    }
    Ok(hits)
}

/// Return the first query term found in one file's persisted indexed text.
fn indexed_text_match_term(
    store: &AtlasStore,
    path: &str,
    terms: &[String],
) -> ServiceResult<Option<String>> {
    let Some(text) = store.load_file_text(path)? else {
        return Ok(None);
    };
    Ok(first_matching_term(&text.content, terms))
}

/// Return the first indexed symbol match for one file and query term set.
fn first_symbol_match(
    store: &AtlasStore,
    path: &str,
    terms: &[String],
) -> ServiceResult<Option<(String, String)>> {
    const RANKING_SYMBOL_KINDS: &[SymbolKind] = &[
        SymbolKind::Function,
        SymbolKind::Method,
        SymbolKind::Class,
        SymbolKind::Struct,
        SymbolKind::Enum,
        SymbolKind::Trait,
        SymbolKind::Interface,
        SymbolKind::Type,
        SymbolKind::Module,
        SymbolKind::Value,
    ];
    for symbol in store.load_symbols_by_kinds(path, RANKING_SYMBOL_KINDS, 50)? {
        if let Some(term) = first_matching_term(&symbol.name, terms)
            .or_else(|| first_matching_term(&symbol.signature, terms))
        {
            return Ok(Some((symbol.name, term)));
        }
    }
    Ok(None)
}

/// Append conventional source/test counterpart files to a candidate set.
fn append_paired_file_nodes(
    store: &AtlasStore,
    matcher: &FilePathMatcher,
    target: usize,
    selected: &mut Vec<IndexedNode>,
) -> ServiceResult<()> {
    if selected.len() >= target {
        return Ok(());
    }
    let mut seen = selected
        .iter()
        .map(|node| node.node.path.clone())
        .collect::<HashSet<_>>();
    let seed_paths = selected
        .iter()
        .map(|node| node.node.path.clone())
        .collect::<Vec<_>>();
    for path in seed_paths {
        for candidate in paired_path_candidates(&path) {
            if selected.len() >= target {
                return Ok(());
            }
            if seen.contains(&candidate) || !matcher.is_match(&candidate) {
                continue;
            }
            if let Some(node) = store.load_node_by_path(&candidate)? {
                seen.insert(candidate);
                selected.push(node);
            }
        }
    }
    Ok(())
}

/// Append classification-eligible source/test counterparts in one exact-path batch.
fn append_paired_file_nodes_selected(
    store: &AtlasStore,
    matcher: &FilePathMatcher,
    target: usize,
    content_selection: ContentSelection,
    selected: &mut Vec<IndexedNode>,
) -> ServiceResult<()> {
    if selected.len() >= target {
        return Ok(());
    }
    let seen = selected
        .iter()
        .map(|node| node.node.path.clone())
        .collect::<HashSet<_>>();
    let mut candidates = selected
        .iter()
        .flat_map(|node| paired_path_candidates(&node.node.path))
        .filter(|path| !seen.contains(path) && matcher.is_match(path))
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    let classifications = file_content_classifications_by_path(store, candidates.clone())?;
    let hydrated = store
        .load_nodes_by_paths(&candidates)?
        .into_iter()
        .map(|node| (node.node.path.clone(), node))
        .collect::<HashMap<_, _>>();
    for path in candidates {
        if selected.len() >= target {
            break;
        }
        if classifications
            .get(&path)
            .is_some_and(|classification| content_selection.includes(*classification))
            && let Some(node) = hydrated.get(&path)
        {
            selected.push(node.clone());
        }
    }
    Ok(())
}

/// Build a concise reason when a source/test counterpart is indexed.
fn paired_path_reason(store: &AtlasStore, path: &str) -> ServiceResult<Option<String>> {
    for candidate in paired_path_candidates(path) {
        if store.load_node_by_path(&candidate)?.is_some() {
            let relation = if is_test_path(path) {
                "paired source file"
            } else {
                "paired test file"
            };
            return Ok(Some(format!("{relation} {candidate}")));
        }
    }
    Ok(None)
}

/// Return conventional source/test counterpart path candidates.
fn paired_path_candidates(path: &str) -> Vec<String> {
    let Some((stem_path, extension)) = path.rsplit_once('.') else {
        return Vec::new();
    };
    let extension = format!(".{extension}");
    let file_stem = stem_path.rsplit('/').next().unwrap_or(stem_path);
    let mut candidates = Vec::new();
    if let Some(source_name) = file_stem.strip_suffix("_test") {
        let prefix = stem_path
            .strip_suffix(file_stem)
            .unwrap_or("")
            .trim_end_matches('/');
        candidates.push(join_repo_path(prefix, &format!("{source_name}{extension}")));
    } else if let Some(source_name) = file_stem.strip_suffix(".test") {
        let prefix = stem_path
            .strip_suffix(file_stem)
            .unwrap_or("")
            .trim_end_matches('/');
        candidates.push(join_repo_path(prefix, &format!("{source_name}{extension}")));
    }
    if let Some(test_name) = stem_path.strip_prefix("tests/") {
        candidates.push(format!("src/{test_name}{extension}"));
    } else if let Some(source_name) = stem_path.strip_prefix("src/") {
        candidates.push(format!("tests/{source_name}{extension}"));
        candidates.push(format!("src/{source_name}_test{extension}"));
    } else {
        candidates.push(format!("tests/{file_stem}{extension}"));
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

/// Return whether a path is conventionally test-owned.
fn is_test_path(path: &str) -> bool {
    path.starts_with("tests/")
        || path.contains("/tests/")
        || path.contains("_test.")
        || path.contains(".test.")
}

/// Join repository path segments without introducing platform separators.
fn join_repo_path(prefix: &str, leaf: &str) -> String {
    if prefix.is_empty() {
        leaf.to_string()
    } else {
        format!("{prefix}/{leaf}")
    }
}

/// Build deterministic follow-up commands for a next-step report.
fn next_report_suggestions(
    query: &str,
    folders: &[RankedNode],
    files: &[ClassifiedRankedNode],
    content_selection: ContentSelection,
) -> Vec<String> {
    let mut suggestions = Vec::new();
    let selection = content_selection
        .explicit_value()
        .map_or_else(String::new, |value| format!(" --content-selection {value}"));
    if let Some(file) = files.first() {
        let path = quoted_command_arg(&file.node.node.path);
        suggestions.push(format!("projectatlas summary {path} --limit 25{selection}"));
        suggestions.push(format!("projectatlas outline {path}{selection}"));
    }
    if let Some(folder) = folders.first() {
        let query_arg = quoted_command_arg(query);
        let folder_arg = quoted_command_arg(&folder.node.node.path);
        suggestions.push(format!(
            "projectatlas files {query_arg} --folder {folder_arg} --limit 5{selection}"
        ));
    }
    if !query.trim().is_empty() {
        let query_arg = quoted_command_arg(query);
        suggestions.push(format!(
            "projectatlas search {query_arg} --file-pattern **/* --context-lines 2{selection}"
        ));
    }
    suggestions.truncate(4);
    suggestions
}

/// Quote a command argument when whitespace or quotes require it.
fn quoted_command_arg(value: &str) -> String {
    if value.is_empty() {
        "\"\"".to_string()
    } else if value
        .chars()
        .any(|character| character.is_whitespace() || matches!(character, '"' | '\''))
    {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

/// Load ranked files while applying a compiled repository-relative glob.
fn load_ranked_file_nodes_matching_glob(
    store: &AtlasStore,
    query: &str,
    folder: Option<&str>,
    matcher: &FilePathMatcher,
    target: usize,
) -> ServiceResult<Vec<IndexedNode>> {
    if !matcher.filters() {
        return Ok(store.load_ranked_nodes(query, NodeKind::File, folder, target, 0)?);
    }
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
            if matcher.is_match(&node.node.path) {
                selected.push(node);
                if selected.len() >= target {
                    return Ok(selected);
                }
            }
        }
    }
    Ok(selected)
}

/// Load ranked DB pages until enough classification-eligible files are found.
fn load_ranked_file_nodes_matching_selection(
    store: &AtlasStore,
    query: &str,
    folder: Option<&str>,
    matcher: &FilePathMatcher,
    target: usize,
    content_selection: ContentSelection,
) -> ServiceResult<Vec<IndexedNode>> {
    let batch_size = target.saturating_mul(20).clamp(50, 500);
    let mut offset = 0usize;
    let mut selected = Vec::new();
    loop {
        let batch = store.load_ranked_nodes(query, NodeKind::File, folder, batch_size, offset)?;
        if batch.is_empty() {
            break;
        }
        offset = offset.saturating_add(batch.len());
        let candidates = batch
            .into_iter()
            .filter(|node| matcher.is_match(&node.node.path))
            .collect::<Vec<_>>();
        let classifications = file_content_classifications_by_path(
            store,
            candidates.iter().map(|node| node.node.path.clone()),
        )?;
        for node in candidates {
            if classifications
                .get(&node.node.path)
                .is_some_and(|classification| content_selection.includes(*classification))
            {
                selected.push(node);
                if selected.len() >= target {
                    return Ok(selected);
                }
            }
        }
    }
    Ok(selected)
}

/// Append indexed-text hits after ordinary ranked file results.
fn append_content_ranked_file_nodes(
    store: &AtlasStore,
    query: &str,
    folder: Option<&str>,
    matcher: &FilePathMatcher,
    target: usize,
    selected: &mut Vec<IndexedNode>,
) -> ServiceResult<()> {
    let terms = normalize_ranking_terms(query);
    if terms.is_empty() {
        return Ok(());
    }
    let mut seen = selected
        .iter()
        .map(|node| node.node.path.clone())
        .collect::<HashSet<_>>();
    store.visit_file_texts_for_search(None, false, |text| {
        if selected.len() >= target {
            return Ok(false);
        }
        let indexed_text = normalized_search_text(&text.content, false);
        if !seen.contains(&text.path)
            && path_is_inside_folder(&text.path, folder)
            && matcher.is_match(&text.path)
            && terms.iter().any(|term| indexed_text.contains(term))
            && let Some(node) = store.load_node_by_path(&text.path)?
        {
            seen.insert(text.path);
            selected.push(node);
        }
        Ok(selected.len() < target)
    })?;
    Ok(())
}

/// Append selected indexed-text hits after ordinary ranked file results.
fn append_content_ranked_file_nodes_selected(
    store: &AtlasStore,
    query: &str,
    folder: Option<&str>,
    matcher: &FilePathMatcher,
    target: usize,
    content_selection: ContentSelection,
    selected: &mut Vec<IndexedNode>,
) -> ServiceResult<()> {
    let terms = normalize_ranking_terms(query);
    if terms.is_empty() {
        return Ok(());
    }
    let mut seen = selected
        .iter()
        .map(|node| node.node.path.clone())
        .collect::<HashSet<_>>();
    store.visit_file_texts_for_fallback(
        None,
        None,
        |metadata| {
            Ok(
                if path_is_inside_folder(&metadata.path, folder)
                    && matcher.is_match(&metadata.path)
                    && content_selection.includes(metadata.classification)
                {
                    FileTextAdmission::Read
                } else {
                    FileTextAdmission::Skip
                },
            )
        },
        |text| {
            if selected.len() >= target {
                return Ok(false);
            }
            let indexed_text = normalized_search_text(&text.content, false);
            if !seen.contains(&text.path)
                && terms.iter().any(|term| indexed_text.contains(term))
                && let Some(node) = store.load_node_by_path(&text.path)?
            {
                seen.insert(text.path);
                selected.push(node);
            }
            Ok(selected.len() < target)
        },
    )?;
    Ok(())
}

/// Return whether a file path is inside an optional repository folder filter.
fn path_is_inside_folder(path: &str, folder: Option<&str>) -> bool {
    let Some(folder) = folder
        .map(|folder| folder.trim_matches('/').trim_matches('\\'))
        .filter(|folder| !folder.is_empty() && *folder != ".")
    else {
        return true;
    };
    let folder = folder.replace('\\', "/");
    path == folder
        || path
            .strip_prefix(&folder)
            .is_some_and(|tail| tail.starts_with('/'))
}

/// Return whether one repository-relative path matches an optional file glob.
///
/// # Errors
///
/// Returns an error when `file_pattern` is not a valid repository glob.
pub fn file_path_matches_glob(path: &str, file_pattern: Option<&str>) -> ServiceResult<bool> {
    Ok(FilePathMatcher::new(file_pattern)?.is_match(path))
}

/// Reusable repository-relative file path matcher.
pub struct FilePathMatcher {
    /// Compiled optional glob matcher.
    matcher: Option<GlobSet>,
}

impl FilePathMatcher {
    /// Compile a repository-relative glob matcher once for many path checks.
    ///
    /// # Errors
    ///
    /// Returns an error when `file_pattern` is not a valid repository glob.
    pub fn new(file_pattern: Option<&str>) -> ServiceResult<Self> {
        Ok(Self {
            matcher: build_path_matcher(file_pattern)?,
        })
    }

    /// Return whether this matcher has an active filtering glob.
    #[must_use]
    pub fn filters(&self) -> bool {
        self.matcher.is_some()
    }

    /// Return whether `path` matches the compiled repository-relative glob.
    #[must_use]
    pub fn is_match(&self, path: &str) -> bool {
        path_matches(path, self.matcher.as_ref())
    }
}

/// Borrow indexed text content as line slices for context extraction.
fn indexed_text_lines(text: &IndexedFileText) -> Vec<&str> {
    text.content.lines().collect()
}

/// Read an exact line slice from an indexed project file.
///
/// # Errors
///
/// Returns an error when the file is not an indexed project file, line numbers
/// are invalid, or source cannot be read.
pub fn read_indexed_code_slice(
    store: &AtlasStore,
    file: &Path,
    start_line: usize,
    end_line: Option<usize>,
) -> ServiceResult<CodeSlice> {
    read_indexed_code_slice_with_selection(
        store,
        file,
        start_line,
        end_line,
        ContentSelection::UnspecifiedLegacy,
    )
}

/// Read an exact indexed line slice after enforcing classified-content selection.
///
/// # Errors
///
/// Returns an error when the file is outside the explicit selection, is not an
/// indexed project file, has invalid line numbers, or source cannot be read.
pub fn read_indexed_code_slice_with_selection(
    store: &AtlasStore,
    file: &Path,
    start_line: usize,
    end_line: Option<usize>,
    content_selection: ContentSelection,
) -> ServiceResult<CodeSlice> {
    let file_key = validated_indexed_file_key(store, file)?;
    let classification = selected_file_classification(store, &file_key, content_selection)?;
    let native_file = indexed_native_path(store, &file_key)?;
    let content = read_file_content(&native_file)?;
    let mut draft = read_code_slice(
        &content,
        &file_key,
        start_line,
        end_line,
        CodeSliceBudget::default(),
    )?;
    draft.slice.classification = Some(classification);
    Ok(draft.slice)
}

/// Read an exact line slice from caller-verified source bytes.
///
/// # Errors
///
/// Returns an error when the file is not indexed or line numbers are invalid.
pub fn read_indexed_code_slice_from_source(
    store: &AtlasStore,
    file: &Path,
    start_line: usize,
    end_line: Option<usize>,
    source: &str,
) -> ServiceResult<CodeSlice> {
    read_indexed_code_slice_from_source_bounded_with_selection(
        store,
        file,
        start_line,
        end_line,
        source,
        CodeSliceBudget::default(),
        ContentSelection::UnspecifiedLegacy,
    )
    .map(|draft| draft.slice)
}

/// Read an exact line slice from verified source after classified selection.
///
/// # Errors
///
/// Returns an error when the file is outside the explicit selection, is not
/// indexed, or the requested line range is invalid.
pub fn read_indexed_code_slice_from_source_with_selection(
    store: &AtlasStore,
    file: &Path,
    start_line: usize,
    end_line: Option<usize>,
    source: &str,
    content_selection: ContentSelection,
) -> ServiceResult<CodeSlice> {
    read_indexed_code_slice_from_source_bounded_with_selection(
        store,
        file,
        start_line,
        end_line,
        source,
        CodeSliceBudget::default(),
        content_selection,
    )
    .map(|draft| draft.slice)
}

/// Read a byte-bounded exact line slice from caller-verified source bytes.
///
/// # Errors
///
/// Returns an error when the file is not indexed, line numbers are invalid,
/// or the verbatim slice cannot fit the requested output budget.
pub fn read_indexed_code_slice_from_source_bounded(
    store: &AtlasStore,
    file: &Path,
    start_line: usize,
    end_line: Option<usize>,
    source: &str,
    output_budget: CodeSliceBudget,
) -> ServiceResult<CodeSliceDraft> {
    read_indexed_code_slice_from_source_bounded_with_selection(
        store,
        file,
        start_line,
        end_line,
        source,
        output_budget,
        ContentSelection::UnspecifiedLegacy,
    )
}

/// Read a bounded line slice from verified source after classified selection.
///
/// # Errors
///
/// Returns an error when the file is outside the explicit selection, is not
/// indexed, has invalid line numbers, or cannot fit the output budget.
pub fn read_indexed_code_slice_from_source_bounded_with_selection(
    store: &AtlasStore,
    file: &Path,
    start_line: usize,
    end_line: Option<usize>,
    source: &str,
    output_budget: CodeSliceBudget,
    content_selection: ContentSelection,
) -> ServiceResult<CodeSliceDraft> {
    let file_key = validated_indexed_file_key(store, file)?;
    let classification = selected_file_classification(store, &file_key, content_selection)?;
    let mut draft = read_code_slice(source, &file_key, start_line, end_line, output_budget)?;
    draft.slice.classification = Some(classification);
    Ok(draft)
}

/// Read a symbol body by exact symbol name and optional disambiguators.
///
/// # Errors
///
/// Returns an error when the symbol is absent, ambiguous, filtered out by the
/// selector, or source cannot be read.
pub fn read_symbol_slice(
    store: &AtlasStore,
    file: &Path,
    selector: &SymbolSliceSelector<'_>,
) -> ServiceResult<CodeSlice> {
    read_symbol_slice_with_selection(store, file, selector, ContentSelection::UnspecifiedLegacy)
}

/// Read an exact indexed symbol body after classified-content selection.
///
/// # Errors
///
/// Returns an error when the file is outside the explicit selection, the
/// symbol is absent or ambiguous, its selector rejects it, or source fails.
pub fn read_symbol_slice_with_selection(
    store: &AtlasStore,
    file: &Path,
    selector: &SymbolSliceSelector<'_>,
    content_selection: ContentSelection,
) -> ServiceResult<CodeSlice> {
    let file_key = validated_indexed_file_key(store, file)?;
    let classification = selected_file_classification(store, &file_key, content_selection)?;
    let native_file = indexed_native_path(store, &file_key)?;
    let content = read_file_content(&native_file)?;
    read_symbol_slice_from_source_for_file(
        store,
        &file_key,
        selector,
        &content,
        CodeSliceBudget::default(),
        classification,
    )
    .map(|draft| draft.slice)
}

/// Read a symbol body from caller-verified source bytes.
///
/// # Errors
///
/// Returns an error when the symbol is absent, ambiguous, filtered out by the
/// selector, or its indexed range is invalid for the supplied source.
pub fn read_symbol_slice_from_source(
    store: &AtlasStore,
    file: &Path,
    selector: &SymbolSliceSelector<'_>,
    source: &str,
) -> ServiceResult<CodeSlice> {
    read_symbol_slice_from_source_bounded_with_selection(
        store,
        file,
        selector,
        source,
        CodeSliceBudget::default(),
        ContentSelection::UnspecifiedLegacy,
    )
    .map(|draft| draft.slice)
}

/// Read an exact symbol body from verified source after classified selection.
///
/// # Errors
///
/// Returns an error when the file is outside the explicit selection, or the
/// symbol is absent, ambiguous, rejected by the selector, or out of range.
pub fn read_symbol_slice_from_source_with_selection(
    store: &AtlasStore,
    file: &Path,
    selector: &SymbolSliceSelector<'_>,
    source: &str,
    content_selection: ContentSelection,
) -> ServiceResult<CodeSlice> {
    read_symbol_slice_from_source_bounded_with_selection(
        store,
        file,
        selector,
        source,
        CodeSliceBudget::default(),
        content_selection,
    )
    .map(|draft| draft.slice)
}

/// Read a byte-bounded symbol body from caller-verified source bytes.
///
/// # Errors
///
/// Returns an error when the symbol is absent, ambiguous, filtered out by the
/// selector, its indexed range is invalid, or its verbatim body cannot fit the
/// requested output budget.
pub fn read_symbol_slice_from_source_bounded(
    store: &AtlasStore,
    file: &Path,
    selector: &SymbolSliceSelector<'_>,
    source: &str,
    output_budget: CodeSliceBudget,
) -> ServiceResult<CodeSliceDraft> {
    read_symbol_slice_from_source_bounded_with_selection(
        store,
        file,
        selector,
        source,
        output_budget,
        ContentSelection::UnspecifiedLegacy,
    )
}

/// Read a bounded symbol body from verified source after classified selection.
///
/// # Errors
///
/// Returns an error when the file is outside the explicit selection, the
/// symbol cannot be selected exactly, or its body cannot fit the output budget.
pub fn read_symbol_slice_from_source_bounded_with_selection(
    store: &AtlasStore,
    file: &Path,
    selector: &SymbolSliceSelector<'_>,
    source: &str,
    output_budget: CodeSliceBudget,
    content_selection: ContentSelection,
) -> ServiceResult<CodeSliceDraft> {
    let file_key = validated_indexed_file_key(store, file)?;
    let classification = selected_file_classification(store, &file_key, content_selection)?;
    read_symbol_slice_from_source_for_file(
        store,
        &file_key,
        selector,
        source,
        output_budget,
        classification,
    )
}

/// Select and slice one symbol after its owning file classification is admitted.
fn read_symbol_slice_from_source_for_file(
    store: &AtlasStore,
    file_key: &str,
    selector: &SymbolSliceSelector<'_>,
    source: &str,
    output_budget: CodeSliceBudget,
    classification: ContentClassification,
) -> ServiceResult<CodeSliceDraft> {
    let requested_kind = selector.kind.map(parse_symbol_kind).transpose()?;
    let mut symbols = store.load_symbols_by_exact_file_and_name(file_key, selector.name)?;
    if let Some(parent) = selector.parent {
        symbols.retain(|symbol| symbol.parent.as_deref() == Some(parent));
    }
    if let Some(kind) = requested_kind {
        symbols.retain(|symbol| symbol.kind == kind);
    }
    if let Some(signature) = selector.signature {
        symbols.retain(|symbol| symbol.signature == signature);
    }
    if let Some(line) = selector.line {
        symbols.retain(|symbol| symbol.line_start <= line && line <= symbol.line_end);
    }
    let symbol = match symbols.as_slice() {
        [symbol] => symbol,
        [] => {
            return Err(ServiceError::InvalidInput(format!(
                "symbol {:?} was not found in indexed file {file_key}",
                selector.name
            )));
        }
        _ => {
            return Err(ServiceError::InvalidInput(format!(
                "symbol {:?} is ambiguous in {file_key}; pass symbol_parent, symbol_kind, symbol_signature, or symbol_line. candidates: {}",
                selector.name,
                describe_symbol_candidates(&symbols)
            )));
        }
    };
    let mut draft = read_code_slice(
        source,
        file_key,
        symbol.line_start,
        Some(symbol.line_end),
        output_budget,
    )?;
    draft.slice.classification = Some(classification);
    Ok(draft)
}

/// Normalize and validate a user-supplied path as a repository-relative file key.
fn validated_file_key(file: &Path) -> ServiceResult<String> {
    validated_repo_file_key(file).map_err(|source| ServiceError::InvalidInput(source.to_string()))
}

/// Validate that a path belongs to the indexed project file set.
fn validated_indexed_file_key(store: &AtlasStore, file: &Path) -> ServiceResult<String> {
    let file_key = validated_file_key(file)?;
    let indexed = store
        .load_node_by_path(&file_key)?
        .ok_or_else(|| ServiceError::InvalidInput(format!("file {file_key:?} is not indexed")))?;
    if indexed.node.kind != NodeKind::File {
        return Err(ServiceError::InvalidInput(format!(
            "path {file_key:?} is not an indexed file"
        )));
    }
    Ok(file_key)
}

/// Load the project root recorded by the latest scan.
fn indexed_project_root(store: &AtlasStore) -> ServiceResult<CanonicalProjectRoot> {
    store.project_root_identity()?.ok_or_else(|| {
        ServiceError::InvalidInput(
            "indexed project root is missing; run projectatlas scan <project-root> first"
                .to_string(),
        )
    })
}

/// Build an absolute native path for a previously validated indexed file key.
fn indexed_native_path(store: &AtlasStore, file_key: &str) -> ServiceResult<PathBuf> {
    Ok(indexed_project_root(store)?
        .as_path()
        .join(repo_path_to_native(file_key)))
}

/// Read source text for a selected file.
fn read_file_content(file: &Path) -> ServiceResult<String> {
    fs::read_to_string(file).map_err(|source| ServiceError::Io {
        path: file.to_path_buf(),
        source,
    })
}

/// Build a path matcher from an optional repository glob.
fn build_path_matcher(pattern: Option<&str>) -> ServiceResult<Option<GlobSet>> {
    let Some(pattern) = pattern else {
        return Ok(None);
    };
    let normalized = pattern.trim().replace('\\', "/");
    if normalized.is_empty() || normalized == "*" {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    add_glob(&mut builder, &normalized)?;
    if !normalized.contains('/') {
        add_glob(&mut builder, &format!("**/{normalized}"))?;
    }
    builder
        .build()
        .map(Some)
        .map_err(|source| ServiceError::InvalidInput(source.to_string()))
}

/// Return an exact path-or-descendant prefix that safely narrows a glob.
fn search_path_prefix(pattern: Option<&str>) -> Option<String> {
    let normalized = pattern?.trim().replace('\\', "/");
    if normalized.is_empty() || normalized == "*" {
        return None;
    }
    let wildcard = normalized
        .char_indices()
        .find_map(|(index, character)| "*?[{".contains(character).then_some(index));
    let prefix = wildcard.map_or(normalized.as_str(), |index| &normalized[..index]);
    let prefix = if wildcard.is_some() {
        prefix.rsplit_once('/').map_or("", |(parent, _)| parent)
    } else {
        prefix.trim_end_matches('/')
    };
    (!prefix.is_empty()).then(|| prefix.to_string())
}

/// Check whether one more hydrated source row fits file and byte ceilings.
fn search_metadata_within_bounds(
    report: &mut SearchReport,
    byte_count: usize,
    max_files: usize,
    max_bytes: usize,
) -> bool {
    if report.searched_files >= max_files {
        mark_search_truncated(report, "selected-file-limit");
        return false;
    }
    let Some(next_bytes) = report.searched_bytes.checked_add(byte_count) else {
        mark_search_truncated(report, "selected-byte-limit");
        return false;
    };
    if next_bytes > max_bytes {
        mark_search_truncated(report, "selected-byte-limit");
        return false;
    }
    true
}

/// Preserve the first stable reason that made exhaustive search impossible.
fn mark_search_truncated(report: &mut SearchReport, reason: &'static str) {
    report.truncated = true;
    if report.truncation_reason.is_none() {
        report.truncation_reason = Some(reason.to_string());
    }
}

/// Exact-verify one admitted authoritative text row.
fn inspect_search_text(
    report: &mut SearchReport,
    text: &IndexedFileText,
    classification: ContentClassification,
    matcher: &LineMatcher,
    context_lines: usize,
    needed: usize,
    max_retained_bytes: usize,
    control: &IndexWorkControl,
) -> Result<(), IndexWorkFailure> {
    let lines = indexed_text_lines(text);
    append_line_matches(
        report,
        &text.path,
        classification,
        &lines,
        matcher,
        context_lines,
        needed,
        max_retained_bytes,
        control,
    )
}

/// Add one normalized glob to a builder.
fn add_glob(builder: &mut GlobSetBuilder, pattern: &str) -> ServiceResult<()> {
    let glob = GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map_err(|source| ServiceError::InvalidInput(source.to_string()))?;
    builder.add(glob);
    Ok(())
}

/// Return whether a repository path matches an optional compiled glob.
fn path_matches(path: &str, matcher: Option<&GlobSet>) -> bool {
    matcher.is_none_or(|matcher| matcher.is_match(path))
}

/// Line-level search mode.
enum LineMatcher {
    /// Regex-backed line matching.
    Regex(regex::Regex),
    /// Literal substring matching.
    Literal {
        /// Normalized literal needle.
        needle: String,
        /// Whether matching is case-sensitive.
        case_sensitive: bool,
    },
    /// Fuzzy subsequence matching.
    Fuzzy {
        /// Normalized fuzzy needle.
        needle: String,
        /// Whether matching is case-sensitive.
        case_sensitive: bool,
    },
}

impl LineMatcher {
    /// Return the serialized search mode name.
    fn mode(&self) -> &'static str {
        match self {
            Self::Regex(_) => "regex",
            Self::Literal { .. } => "literal",
            Self::Fuzzy { .. } => "fuzzy",
        }
    }

    /// Return one FTS-safe token whose candidates remain a complete superset.
    fn fts_literal_token(&self) -> Option<&str> {
        match self {
            Self::Literal { needle, .. }
                if needle.len() >= 3
                    && needle
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric()) =>
            {
                Some(needle.as_str())
            }
            Self::Regex(_) | Self::Fuzzy { .. } | Self::Literal { .. } => None,
        }
    }

    /// Return whether this matcher accepts one source line.
    fn is_match(&self, line: &str) -> bool {
        match self {
            Self::Regex(regex) => regex.is_match(line),
            Self::Literal {
                needle,
                case_sensitive,
            } => normalized_search_text(line, *case_sensitive).contains(needle),
            Self::Fuzzy {
                needle,
                case_sensitive,
            } => fuzzy_subsequence_matches(needle, &normalized_search_text(line, *case_sensitive)),
        }
    }
}

/// Append bounded line matches from one source file.
fn append_line_matches(
    report: &mut SearchReport,
    path: &str,
    classification: ContentClassification,
    lines: &[&str],
    matcher: &LineMatcher,
    context_lines: usize,
    needed: usize,
    max_retained_bytes: usize,
    control: &IndexWorkControl,
) -> Result<(), IndexWorkFailure> {
    let result_limit = needed.saturating_sub(report.start_index);
    for (index, line) in lines.iter().enumerate() {
        control.check(IndexWorkStage::TextIndex)?;
        if !matcher.is_match(line) {
            continue;
        }
        report.total += 1;
        if report.total <= report.start_index {
            continue;
        }
        if report.results.len() >= result_limit {
            mark_search_truncated(report, "result-limit");
            control.check(IndexWorkStage::TextIndex)?;
            return Ok(());
        }
        let row = SearchMatch {
            path: path.to_string(),
            classification,
            line: index + 1,
            context_before: context_before(lines, index, context_lines),
            text: (*line).to_string(),
            context_after: context_after(lines, index, context_lines),
        };
        let retained_bytes = row
            .path
            .len()
            .saturating_add(row.text.len())
            .saturating_add(
                row.context_before
                    .iter()
                    .chain(&row.context_after)
                    .map(String::len)
                    .sum::<usize>(),
            );
        let Some(next_retained_bytes) = report.retained_bytes.checked_add(retained_bytes) else {
            mark_search_truncated(report, "retained-byte-limit");
            control.check(IndexWorkStage::TextIndex)?;
            return Ok(());
        };
        if next_retained_bytes > max_retained_bytes {
            mark_search_truncated(report, "retained-byte-limit");
            control.check(IndexWorkStage::TextIndex)?;
            return Ok(());
        }
        report.retained_bytes = next_retained_bytes;
        report.results.push(row);
        if report.results.len() >= result_limit {
            mark_search_truncated(report, "result-limit");
            control.check(IndexWorkStage::TextIndex)?;
            return Ok(());
        }
    }
    control.check(IndexWorkStage::TextIndex)
}

/// Normalize search text for case-sensitive or insensitive matching.
fn normalized_search_text(text: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        text.to_string()
    } else {
        text.to_ascii_lowercase()
    }
}

/// Return whether every needle character appears in candidate order.
fn fuzzy_subsequence_matches(needle: &str, candidate: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut needle = needle.chars();
    let Some(mut expected) = needle.next() else {
        return true;
    };
    for character in candidate.chars() {
        if character == expected {
            let Some(next) = needle.next() else {
                return true;
            };
            expected = next;
        }
    }
    false
}

/// Return context lines before a match.
fn context_before(lines: &[&str], index: usize, context_lines: usize) -> Vec<String> {
    let start = index.saturating_sub(context_lines);
    lines[start..index]
        .iter()
        .map(|line| (*line).to_string())
        .collect()
}

/// Return context lines after a match.
fn context_after(lines: &[&str], index: usize, context_lines: usize) -> Vec<String> {
    let start = index.saturating_add(1);
    let end = lines.len().min(start.saturating_add(context_lines));
    lines[start..end]
        .iter()
        .map(|line| (*line).to_string())
        .collect()
}

/// Read an exact line slice from a previously validated file.
fn read_code_slice(
    content: &str,
    file_key: &str,
    start_line: usize,
    end_line: Option<usize>,
    output_budget: CodeSliceBudget,
) -> ServiceResult<CodeSliceDraft> {
    if start_line == 0 {
        return Err(ServiceError::InvalidInput(
            "start-line must be one or greater".to_string(),
        ));
    }
    let requested_end_line = end_line.unwrap_or(start_line);
    if requested_end_line < start_line {
        return Err(ServiceError::InvalidInput(
            "end-line must be greater than or equal to start-line".to_string(),
        ));
    }
    let mut line_count = 0usize;
    let mut offset = 0usize;
    let mut selected_start = None;
    let mut selected_end = None;
    for line in content.split_inclusive('\n') {
        line_count = line_count.saturating_add(1);
        let line_start = offset;
        let line_end_with_terminator = offset.checked_add(line.len()).ok_or_else(|| {
            ServiceError::InvalidInput("slice source byte offset overflowed".to_string())
        })?;
        let line_end = if line.ends_with("\r\n") {
            line_end_with_terminator - 2
        } else if line.ends_with('\n') {
            line_end_with_terminator - 1
        } else {
            line_end_with_terminator
        };
        if line_count == start_line {
            selected_start = Some(line_start);
        }
        if line_count >= start_line && line_count <= requested_end_line {
            selected_end = Some(line_end);
        }
        offset = line_end_with_terminator;
    }
    if start_line > line_count {
        return Err(ServiceError::InvalidInput(format!(
            "start-line {start_line} exceeds file line count {line_count}"
        )));
    }
    let end_index = requested_end_line.min(line_count);
    let selected_start = selected_start
        .ok_or_else(|| ServiceError::InvalidInput("slice start byte was not found".to_string()))?;
    let selected_end = selected_end
        .ok_or_else(|| ServiceError::InvalidInput("slice end byte was not found".to_string()))?;
    let content_bytes = selected_end.checked_sub(selected_start).ok_or_else(|| {
        ServiceError::InvalidInput("slice content byte range was invalid".to_string())
    })?;
    if content_bytes > output_budget.output_bytes() as usize {
        return Err(ServiceError::InvalidInput(format!(
            "verbatim slice content exceeds the requested {}-byte output ceiling; narrow the line or symbol range or raise output-bytes",
            output_budget.output_bytes()
        )));
    }
    let content = content[selected_start..selected_end].to_string();
    Ok(CodeSliceDraft {
        slice: CodeSlice {
            path: file_key.to_string(),
            classification: None,
            start_line,
            end_line: end_index,
            line_count,
            estimated_tokens: estimate_tokens(&content),
            content,
        },
        output_budget,
    })
}

/// Parse a user-facing symbol kind selector.
///
/// # Errors
///
/// Returns an error when the value is not one of the supported persisted kinds.
pub fn parse_symbol_kind(kind: &str) -> ServiceResult<SymbolKind> {
    let normalized = kind.trim().to_ascii_lowercase();
    let parsed = SymbolKind::from_db(&normalized);
    if parsed == SymbolKind::Unknown && normalized != "unknown" {
        return Err(ServiceError::InvalidInput(format!(
            "unsupported symbol kind {kind:?}"
        )));
    }
    Ok(parsed)
}

/// Describe symbol candidates for ambiguity errors.
fn describe_symbol_candidates(symbols: &[CodeSymbol]) -> String {
    symbols
        .iter()
        .map(|symbol| {
            format!(
                "{} parent={} kind={} lines={}-{}",
                symbol.name,
                symbol.parent.as_deref().unwrap_or(""),
                symbol.kind,
                symbol.line_start,
                symbol.line_end
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Count source lines in loaded content.
fn line_count_from_content(content: &str) -> usize {
    content.lines().count()
}

/// Return distinct symbol names for caller lookup.
fn symbol_names(symbols: &[CodeSymbol]) -> Vec<String> {
    let mut names = symbols
        .iter()
        .map(|symbol| symbol.name.clone())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

/// Return symbol kinds that can provide file-level metadata.
fn metadata_symbol_kinds() -> [SymbolKind; 3] {
    [
        SymbolKind::Package,
        SymbolKind::Workspace,
        SymbolKind::Module,
    ]
}

/// Return symbol kinds grouped in the `types` summary section.
fn type_symbol_kinds() -> [SymbolKind; 7] {
    [
        SymbolKind::Struct,
        SymbolKind::Enum,
        SymbolKind::Trait,
        SymbolKind::Interface,
        SymbolKind::Type,
        SymbolKind::Package,
        SymbolKind::Workspace,
    ]
}

/// Combine displayed symbol rows for caller lookup without changing section order.
fn summarized_symbol_set(
    functions: &[CodeSymbol],
    methods: &[CodeSymbol],
    classes: &[CodeSymbol],
    types: &[CodeSymbol],
) -> Vec<CodeSymbol> {
    functions
        .iter()
        .chain(methods)
        .chain(classes)
        .chain(types)
        .cloned()
        .collect()
}

/// Return exact call target names that can safely resolve to displayed symbols.
fn caller_target_names(symbols: &[CodeSymbol], import_aliases: &ImportAliasMap) -> Vec<String> {
    let mut targets = HashSet::new();
    for symbol in symbols {
        targets.insert(symbol.name.clone());
        for alias in symbol_target_aliases(symbol) {
            targets.insert(alias);
        }
    }
    for alias in import_aliases.values().flatten() {
        targets.insert(alias.target_name.clone());
    }
    let mut values = targets.into_iter().collect::<Vec<_>>();
    values.sort();
    values
}

/// Build reverse call lookup for displayed symbols across the indexed graph.
fn called_by_map(
    symbols: &[CodeSymbol],
    relations: &[SymbolRelation],
    name_counts: &HashMap<String, usize>,
    alias_counts: &HashMap<String, usize>,
    import_aliases: &ImportAliasMap,
) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for symbol in symbols {
        let symbol_key = symbol_summary_key(symbol);
        for relation in relations.iter().filter(|relation| {
            relation_matches_symbol(relation, symbol, name_counts, alias_counts, import_aliases)
        }) {
            let caller = caller_reference(relation);
            let callers = map.entry(symbol_key.clone()).or_default();
            if !callers.iter().any(|existing| existing == &caller) {
                callers.push(caller);
            }
        }
    }
    for callers in map.values_mut() {
        callers.sort();
        callers.truncate(CALLERS_PER_SYMBOL_LIMIT);
    }
    map
}

/// Return whether a relation can be deterministically attached to a symbol.
fn relation_matches_symbol(
    relation: &SymbolRelation,
    symbol: &CodeSymbol,
    name_counts: &HashMap<String, usize>,
    alias_counts: &HashMap<String, usize>,
    import_aliases: &ImportAliasMap,
) -> bool {
    if relation.kind != RelationKind::Calls {
        return false;
    }
    let target = relation.target_name.trim();
    if target == symbol.name
        && (relation.path == symbol.path
            || name_counts.get(&symbol.name).copied().unwrap_or(0) <= 1)
    {
        return true;
    }
    if symbol_target_aliases(symbol)
        .iter()
        .any(|alias| alias == target && alias_counts.get(alias).copied().unwrap_or(0) <= 1)
    {
        return true;
    }
    import_aliases
        .get(&symbol_summary_key(symbol))
        .is_some_and(|aliases| {
            aliases.iter().any(|alias| {
                alias.caller_path == relation.path && alias.target_name == relation.target_name
            })
        })
}

/// Count target aliases across displayed symbols.
fn symbol_alias_counts(symbols: &[CodeSymbol]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for alias in symbols.iter().flat_map(symbol_target_aliases) {
        *counts.entry(alias).or_insert(0) += 1;
    }
    counts
}

/// Return exact qualified target strings that identify a symbol by file path.
fn symbol_target_aliases(symbol: &CodeSymbol) -> Vec<String> {
    let mut aliases = HashSet::new();
    let modules = module_aliases_for_path(&symbol.path);
    for module in &modules {
        aliases.insert(format!("{module}::{}", symbol.name));
        aliases.insert(format!("{module}.{}", symbol.name));
        aliases.insert(format!("crate::{module}::{}", symbol.name));
        aliases.insert(format!("crate.{module}.{}", symbol.name));
    }
    if modules.is_empty() {
        aliases.insert(format!("crate::{}", symbol.name));
        aliases.insert(format!("self::{}", symbol.name));
    }
    let mut values = aliases.into_iter().collect::<Vec<_>>();
    values.sort();
    values
}

/// Build a stable identity key for a summarized symbol row.
fn symbol_summary_key(symbol: &CodeSymbol) -> String {
    format!("{}\0{}\0{}", symbol.path, symbol.name, symbol.line_start)
}

/// Return a compact caller reference.
fn caller_reference(relation: &SymbolRelation) -> String {
    format!("{}::{}", relation.path, relation.source_name)
}

/// Summarize already-selected symbols.
fn summarize_symbols(
    symbols: &[CodeSymbol],
    called_by: &HashMap<String, Vec<String>>,
) -> Vec<FileSymbolSummary> {
    let mut rows = symbols
        .iter()
        .map(|symbol| FileSymbolSummary {
            name: symbol.name.clone(),
            kind: symbol.kind.to_string(),
            line: symbol.line_start,
            end_line: symbol.line_end,
            signature: symbol.signature.clone(),
            exported: symbol.exported,
            documentation: symbol.documentation.clone().unwrap_or_default(),
            parent: symbol.parent.clone().unwrap_or_default(),
            called_by: called_by
                .get(&symbol_summary_key(symbol))
                .cloned()
                .unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        left.line
            .cmp(&right.line)
            .then_with(|| left.name.cmp(&right.name))
    });
    rows
}

/// Return a best-effort package or module name from indexed symbols.
fn package_name(symbols: &[CodeSymbol]) -> String {
    symbols
        .iter()
        .find(|symbol| matches!(symbol.kind, SymbolKind::Package | SymbolKind::Workspace))
        .or_else(|| {
            symbols.iter().find(|symbol| {
                symbol.kind == SymbolKind::Module
                    && matches!(
                        symbol.detail.as_deref(),
                        Some(
                            "package_declaration"
                                | "package_clause"
                                | "package_header"
                                | "namespace_declaration"
                                | "file_scoped_namespace_declaration"
                                | "module_declaration"
                        )
                    )
            })
        })
        .map(|symbol| symbol.name.clone())
        .unwrap_or_default()
}

/// Return file-level documentation from the best indexed symbol source.
fn file_docstring(symbols: &[CodeSymbol]) -> String {
    symbols
        .iter()
        .find(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::Package | SymbolKind::Workspace | SymbolKind::Module
            ) && symbol
                .documentation
                .as_deref()
                .is_some_and(|value| !value.is_empty())
        })
        .and_then(|symbol| symbol.documentation.clone())
        .unwrap_or_default()
}

/// Extract file-level documentation from source text.
fn file_level_docstring(content: &str) -> Option<String> {
    leading_string_docstring(content).or_else(|| leading_doc_comments(content))
}

/// Extract a Python-style file docstring at the beginning of a file.
fn leading_string_docstring(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    for quote in ["\"\"\"", "'''"] {
        if let Some(rest) = trimmed.strip_prefix(quote)
            && let Some(end) = rest.find(quote)
        {
            return compact_doc_text(&rest[..end]);
        }
    }
    None
}

/// Extract leading file-level doc comments.
fn leading_doc_comments(content: &str) -> Option<String> {
    let mut lines = Vec::new();
    let mut in_block = false;
    let mut module_style: Option<bool> = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() && lines.is_empty() {
            continue;
        }
        if in_block {
            if let Some(end) = trimmed.find("*/") {
                lines.push(trimmed[..end].trim_start_matches('*').trim().to_string());
                break;
            }
            lines.push(trimmed.trim_start_matches('*').trim().to_string());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("//!") {
            if module_style.is_some_and(|module| !module) {
                break;
            }
            module_style = Some(true);
            lines.push(value.trim().to_string());
        } else if let Some(value) = trimmed.strip_prefix("///") {
            if module_style.is_some_and(|module| module) {
                break;
            }
            module_style = Some(false);
            lines.push(value.trim().to_string());
        } else if let Some(value) = trimmed.strip_prefix("/*!") {
            if module_style.is_some_and(|module| !module) {
                break;
            }
            module_style = Some(true);
            in_block = true;
            if let Some(end) = value.find("*/") {
                lines.push(value[..end].trim_start_matches('*').trim().to_string());
                break;
            }
            lines.push(value.trim_start_matches('*').trim().to_string());
        } else if let Some(value) = trimmed.strip_prefix("/**") {
            if module_style.is_some_and(|module| module) {
                break;
            }
            module_style = Some(false);
            in_block = true;
            if let Some(end) = value.find("*/") {
                lines.push(value[..end].trim_start_matches('*').trim().to_string());
                break;
            }
            lines.push(value.trim_start_matches('*').trim().to_string());
        } else {
            break;
        }
    }
    compact_doc_text(&lines.join(" "))
}

/// Normalize documentation text to one compact line.
fn compact_doc_text(raw: &str) -> Option<String> {
    let text = raw
        .lines()
        .map(|line| line.trim().trim_start_matches('*').trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() { None } else { Some(text) }
}

/// Return sorted exported symbol names.
#[cfg(test)]
fn exported_symbol_names(symbols: &[CodeSymbol]) -> Vec<String> {
    let mut names = symbols
        .iter()
        .filter(|symbol| symbol.exported)
        .map(|symbol| symbol.name.clone())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectatlas_core::graph::{
        CoverageRecord, GraphIdentityField, GraphIdentityRejectionReason, GraphIdentityText,
        GraphLimitKind, RepositoryNodePath, SourceSpan,
    };
    use projectatlas_core::symbols::{ParserKind, SymbolGraph};
    use projectatlas_core::telemetry::{
        AgentEfficiencyBaseline, AgentEfficiencyEvidenceState, UsageDetailAvailability,
    };
    use projectatlas_core::{Node, Purpose, PurposeSource, PurposeStatus, normalized_parent};
    use std::error::Error;
    use std::io;

    #[cfg(unix)]
    #[test]
    fn file_summary_reads_non_utf8_native_root_without_display_reconstruction()
    -> Result<(), Box<dyn Error>> {
        use std::os::unix::ffi::OsStringExt;

        let temp = tempfile::tempdir()?;
        let root = temp
            .path()
            .join(std::ffi::OsString::from_vec(vec![b's', b'r', b'c', 0x80]));
        fs::create_dir(&root)?;
        fs::write(root.join("entry.rs"), "pub fn native_root() {}\n")?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(&root)?;
        let node = test_node("entry.rs", "native-root-hash");
        store.replace_scan(std::slice::from_ref(&node))?;
        index_test_file_texts(&mut store, &root, std::slice::from_ref(&node))?;

        let report = build_file_summary(&store, Path::new("entry.rs"), 10)?;
        require_eq(
            &report.source_status,
            &SOURCE_STATUS_LIVE.to_string(),
            "non-UTF-8 root source status",
        )?;
        require_eq(&report.line_count, &1, "non-UTF-8 root source line count")?;
        Ok(())
    }

    #[test]
    fn token_report_service_selects_typed_reports_and_requires_a_project_binding()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        let atlas_dir = root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let store = AtlasStore::open_for_project(&atlas_dir.join("projectatlas.db"), &root)?;

        let overview = load_token_report(
            &store,
            TokenReportRequest::Overview {
                caller_label: None,
                benchmark_results: None,
            },
        )?;
        match overview {
            TokenReport::Overview(overview) => {
                require_eq(&overview.calls, &0, "empty overview calls")?;
                require_eq(
                    &overview.detail_availability,
                    &UsageDetailAvailability::Retained,
                    "empty overview detail availability",
                )?;
                require_eq(
                    &overview.agent_efficiency.state,
                    &AgentEfficiencyEvidenceState::Unavailable,
                    "empty overview agent-efficiency state",
                )?;
            }
            TokenReport::Trends(_) => {
                return Err(io::Error::other("overview request returned token trends").into());
            }
        }

        let trends = load_token_report(
            &store,
            TokenReportRequest::Trends {
                caller_label: None,
                window: TokenTrendWindow::Month,
            },
        )?;
        match trends {
            TokenReport::Trends(report) => {
                require_eq(&report.window, &TokenTrendWindow::Month, "trend window")?;
                require_eq(
                    &report.detail_availability,
                    &UsageDetailAvailability::Retained,
                    "empty trend detail availability",
                )?;
            }
            TokenReport::Overview(_) => {
                return Err(io::Error::other("trend request returned token overview").into());
            }
        }

        let unbound = AtlasStore::in_memory()?;
        if !matches!(
            load_token_report(
                &unbound,
                TokenReportRequest::Overview {
                    caller_label: None,
                    benchmark_results: None,
                }
            ),
            Err(ServiceError::SelectedProjectUnavailable)
        ) {
            return Err(io::Error::other("unbound token report did not fail closed").into());
        }
        Ok(())
    }

    #[test]
    fn token_report_service_bounds_and_classifies_benchmark_evidence() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("benchmark-service");
        let atlas_dir = root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let store = AtlasStore::open_for_project(&atlas_dir.join("projectatlas.db"), &root)?;
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/benchmarks/v0.4-agent-navigation-results.json");
        let published = root.join("published.json");
        fs::copy(&source, &published)?;

        let report = load_token_report(
            &store,
            TokenReportRequest::Overview {
                caller_label: None,
                benchmark_results: Some(Path::new("published.json")),
            },
        )?;
        let TokenReport::Overview(report) = report else {
            return Err(io::Error::other("benchmark request returned token trends").into());
        };
        require_eq(
            &report.agent_efficiency.state,
            &AgentEfficiencyEvidenceState::Partial,
            "published benchmark state",
        )?;
        let comparison =
            load_agent_efficiency_comparison(&store, Some(Path::new("published.json")))?;
        require_eq(
            &comparison.state,
            &AgentEfficiencyEvidenceState::Partial,
            "standalone benchmark enrichment state",
        )?;
        let frozen = report
            .agent_efficiency
            .baselines
            .iter()
            .find(|row| row.baseline == AgentEfficiencyBaseline::FrozenProjectAtlasV0326)
            .ok_or_else(|| io::Error::other("frozen baseline row missing"))?;
        require_eq(
            &frozen.baseline_failed_trials,
            &3,
            "published frozen failed trials",
        )?;

        fs::write(root.join("malformed.json"), b"{")?;
        let malformed = load_token_report(
            &store,
            TokenReportRequest::Overview {
                caller_label: None,
                benchmark_results: Some(Path::new("malformed.json")),
            },
        )?;
        let TokenReport::Overview(malformed) = malformed else {
            return Err(io::Error::other("malformed request returned token trends").into());
        };
        require_eq(
            &malformed.agent_efficiency.state,
            &AgentEfficiencyEvidenceState::Failed,
            "malformed benchmark state",
        )?;

        let stale = String::from_utf8(fs::read(&source)?)?.replacen(
            "\"schema_version\": 1",
            "\"schema_version\": 2",
            1,
        );
        fs::write(root.join("stale.json"), stale)?;
        let stale = load_token_report(
            &store,
            TokenReportRequest::Overview {
                caller_label: None,
                benchmark_results: Some(Path::new("stale.json")),
            },
        )?;
        let TokenReport::Overview(stale) = stale else {
            return Err(io::Error::other("stale request returned token trends").into());
        };
        require_eq(
            &stale.agent_efficiency.state,
            &AgentEfficiencyEvidenceState::Incompatible,
            "stale benchmark state",
        )?;

        let missing = load_token_report(
            &store,
            TokenReportRequest::Overview {
                caller_label: None,
                benchmark_results: Some(Path::new("missing.json")),
            },
        )?;
        let TokenReport::Overview(missing) = missing else {
            return Err(io::Error::other("missing request returned token trends").into());
        };
        require_eq(
            &missing.agent_efficiency.state,
            &AgentEfficiencyEvidenceState::Failed,
            "missing benchmark state",
        )?;

        fs::write(
            root.join("oversized.json"),
            vec![b' '; super::agent_efficiency::BENCHMARK_MAX_BYTES + 1],
        )?;
        let oversized = load_token_report(
            &store,
            TokenReportRequest::Overview {
                caller_label: None,
                benchmark_results: Some(Path::new("oversized.json")),
            },
        )?;
        let TokenReport::Overview(oversized) = oversized else {
            return Err(io::Error::other("oversized request returned token trends").into());
        };
        require_eq(
            &oversized.agent_efficiency.state,
            &AgentEfficiencyEvidenceState::Failed,
            "oversized benchmark state",
        )?;

        for escaped in [published.as_path(), Path::new("../outside.json")] {
            if !matches!(
                load_token_report(
                    &store,
                    TokenReportRequest::Overview {
                        caller_label: None,
                        benchmark_results: Some(escaped),
                    },
                ),
                Err(ServiceError::InvalidInput(_))
            ) {
                return Err(io::Error::other(
                    "escaping benchmark path did not fail at the service boundary",
                )
                .into());
            }
        }
        Ok(())
    }

    #[test]
    fn summary_digest_and_opt_in_coverage_page_share_current_typed_rows()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("coverage-service");
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("src/lib.rs"), "pub fn run() {}\n")?;
        let db_path = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&db_path, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("service fixture identity is missing"))?;
        let generation = IndexGeneration::new(1);
        let mut publication = store.begin_index_publication("coverage-service")?;
        publication.begin_scan_replacement()?;
        publication.upsert_scan_node_batch(&[test_node("src/lib.rs", "coverage-hash")])?;
        publication.finish_scan_replacement()?;
        publication.replace_symbol_graph(&SymbolGraph {
            path: "src/lib.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::new(),
            relations: Vec::new(),
        })?;
        let mut coverage = vec![
            CoverageRecord::new(
                CoverageScope::Path {
                    path: RepositoryNodePath::new(Path::new("src/lib.rs"))?,
                },
                None,
                CoverageState::Partial,
                3,
                1,
                generation,
                Some(GraphIdentityText::new("one fallback fact omitted")?),
                Some(GraphLimitKind::Rows),
            )?,
            CoverageRecord::new(
                CoverageScope::Project,
                Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                CoverageState::Failed,
                0,
                1,
                generation,
                Some(GraphIdentityText::new("parser failed")?),
                None,
            )?,
        ];
        for index in 0..=COVERAGE_DIGEST_ROW_LIMIT {
            let sibling_path = format!("src/lib.rs.{index:02}");
            coverage.push(CoverageRecord::new(
                CoverageScope::Path {
                    path: RepositoryNodePath::new(Path::new(&sibling_path))?,
                },
                None,
                CoverageState::Complete,
                1,
                0,
                generation,
                None,
                None,
            )?);
        }
        publication.replace_repository_graph(project, &[], &[], &[], &coverage)?;
        publication.replace_graph_identity_rejections(
            project,
            &[GraphIdentityRejection {
                path: RepositoryNodePath::new(Path::new("src/lib.rs"))?,
                span: SourceSpan::new(1, 0, 1, 2)?,
                parser: ParserKind::TreeSitter,
                field: GraphIdentityField::Symbol,
                reason: GraphIdentityRejectionReason::Empty,
                fact_index: 0,
            }],
        )?;
        publication.complete()?;
        drop(store);

        let store = AtlasStore::open_read_only_for_project(&db_path, &root)?;
        let summary = build_file_summary(&store, Path::new("src/lib.rs"), 10)?;
        require_eq(
            &summary.coverage.available,
            &true,
            "summary coverage availability",
        )?;
        require_eq(
            &summary.coverage.states.partial,
            &1,
            "summary partial coverage count",
        )?;
        require_eq(
            &summary.coverage.states.complete,
            &0,
            "summary excluded lexical sibling coverage",
        )?;
        require_eq(
            &summary.coverage.states.failed,
            &0,
            "summary excluded project-wide coverage",
        )?;
        require_eq(
            &summary.coverage.truncated,
            &false,
            "summary exact-file coverage truncation",
        )?;
        require_eq(
            &summary.coverage.trust,
            &CoverageTrustState::Partial,
            "summary exact-file coverage trust",
        )?;
        require_eq(
            &summary.coverage.provider,
            &Some(ParserKind::TreeSitter),
            "summary fact provider",
        )?;
        require_eq(
            &summary.coverage.next_call.capability,
            &NavigationNextCapability::Health,
            "summary coverage next call",
        )?;

        let report = load_coverage_discovery(
            &store,
            RepositoryCoverageQuery {
                start_index: 0,
                limit: 10,
                path_prefix: None,
                parser: None,
                provider: None,
                relation: Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                state: Some(CoverageState::Failed),
                reason: Some("parser failed".to_string()),
            },
        )?;
        require_eq(&report.returned, &1, "filtered service coverage row")?;
        require_eq(
            &report.total,
            &CoverageTotalState::Exact(1),
            "bounded exact coverage total",
        )?;
        require_eq(
            &report.rows[0].next_call.capability,
            &NavigationNextCapability::Health,
            "project coverage next call",
        )?;
        let truncated = load_coverage_discovery(
            &store,
            RepositoryCoverageQuery {
                start_index: 0,
                limit: 1,
                path_prefix: None,
                parser: None,
                provider: None,
                relation: None,
                state: None,
                reason: None,
            },
        )?;
        require_eq(
            &truncated.total,
            &CoverageTotalState::AtLeast(2),
            "truncated coverage lower bound",
        )?;
        require_eq(&truncated.continuation, &Some(1), "coverage continuation")?;
        require(
            truncated
                .rows
                .iter()
                .any(|row| row.path == "src/lib.rs" && row.identity_rejections.len() == 1),
            "structured coverage omitted typed identity rejection details",
        )?;
        let exhausted = load_coverage_discovery(
            &store,
            RepositoryCoverageQuery {
                start_index: 100,
                limit: 1,
                path_prefix: None,
                parser: None,
                provider: None,
                relation: None,
                state: None,
                reason: None,
            },
        )?;
        require_eq(
            &exhausted.total,
            &CoverageTotalState::Unknown,
            "exhausted nonzero continuation total",
        )?;
        Ok(())
    }

    #[test]
    fn metadata_helpers_are_stable() -> Result<(), Box<dyn Error>> {
        let mut package = test_symbol("Cargo.toml", SymbolKind::Package, "projectatlas");
        package.documentation = Some("ProjectAtlas package manifest.".to_string());
        let mut alpha = test_symbol("src/lib.rs", SymbolKind::Function, "alpha");
        alpha.exported = true;
        alpha.documentation = Some("Alpha entry point.".to_string());
        let mut beta = test_symbol("src/lib.rs", SymbolKind::Function, "beta");
        beta.exported = true;
        let private = test_symbol("src/lib.rs", SymbolKind::Function, "private");
        let symbols = vec![beta, package, private, alpha];

        require_eq(
            &package_name(&symbols),
            &"projectatlas".to_string(),
            "package name",
        )?;
        require_eq(
            &file_docstring(&symbols),
            &"ProjectAtlas package manifest.".to_string(),
            "file docstring",
        )?;
        require_eq(
            &exported_symbol_names(&symbols),
            &vec!["alpha".to_string(), "beta".to_string()],
            "exported symbols",
        )?;
        require_eq(
            &file_level_docstring("//! Module level docs.\nfn main() {}"),
            &Some("Module level docs.".to_string()),
            "rust module docs",
        )?;
        require_eq(
            &file_level_docstring("\"\"\"Python module docs.\"\"\"\nclass Atlas: pass"),
            &Some("Python module docs.".to_string()),
            "python module docs",
        )?;
        Ok(())
    }

    #[test]
    fn file_summary_marks_fallback_symbol_graph_as_fallback() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir(root.join("src"))?;
        fs::write(
            root.join("src").join("component.vue"),
            "<script setup></script>",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[test_node("src/component.vue", "hash-vue")])?;
        store.set_purpose(
            "src/component.vue",
            "Provide Vue component behavior",
            PurposeSource::Agent,
        )?;
        store.set_node_summary("src/component.vue", "vue component with bindings selected.")?;
        let mut fallback_symbol = test_symbol("src/component.vue", SymbolKind::Value, "selected");
        fallback_symbol.parser = ParserKind::Fallback;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/component.vue".to_string(),
            language: Some("vue".to_string()),
            parser: ParserKind::Fallback,
            symbols: vec![fallback_symbol],
            relations: Vec::new(),
        })?;

        let report = build_file_summary(&store, Path::new("src/component.vue"), 10)?;
        require_eq(
            &report.parser_kind,
            &"fallback-symbol-graph".to_string(),
            "fallback parser kind",
        )?;
        require_eq(
            &report.summary_status,
            &"fallback".to_string(),
            "fallback summary status",
        )
    }

    #[test]
    fn file_summary_marks_empty_fallback_graph_as_fallback() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir(root.join("scripts"))?;
        fs::write(root.join("scripts").join("config.ps1"), "# comment only\n")?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[test_node("scripts/config.ps1", "hash-ps1")])?;
        store.set_node_summary(
            "scripts/config.ps1",
            "powershell source file with no declarations found.",
        )?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "scripts/config.ps1".to_string(),
            language: Some("powershell".to_string()),
            parser: ParserKind::Fallback,
            symbols: Vec::new(),
            relations: Vec::new(),
        })?;

        let report = build_file_summary(&store, Path::new("scripts/config.ps1"), 10)?;
        require_eq(
            &report.parser_kind,
            &"fallback-symbol-graph".to_string(),
            "empty fallback parser kind",
        )?;
        require_eq(
            &report.summary_status,
            &"fallback".to_string(),
            "empty fallback summary status",
        )
    }

    #[test]
    fn file_summary_uses_metadata_parser_for_empty_nonfallback_graphs() -> Result<(), Box<dyn Error>>
    {
        for (path, language, parser, expected) in [
            (
                "src/empty.rs",
                "rust",
                ParserKind::TreeSitter,
                "tree-sitter-symbol-graph",
            ),
            (
                "src/component.vue",
                "vue",
                ParserKind::Structural,
                "structural-symbol-graph",
            ),
            (
                "Cargo.toml",
                "cargo-manifest",
                ParserKind::Manifest,
                "manifest-symbol-graph",
            ),
        ] {
            let temp = tempfile::tempdir()?;
            let root = temp.path();
            if let Some(parent) = Path::new(path).parent() {
                fs::create_dir_all(root.join(parent))?;
            }
            fs::write(root.join(path), "\n")?;
            let mut store = AtlasStore::in_memory()?;
            store.set_project_root(root)?;
            store.replace_scan(&[test_node(path, "hash-empty")])?;
            store.set_node_summary(path, "source file with no declarations found.")?;
            store.replace_symbol_graph(&SymbolGraph {
                path: path.to_string(),
                language: Some(language.to_string()),
                parser,
                symbols: Vec::new(),
                relations: Vec::new(),
            })?;

            let report = build_file_summary(&store, Path::new(path), 10)?;
            require_eq(
                &report.parser_kind,
                &expected.to_string(),
                "empty nonfallback parser kind",
            )?;
        }
        Ok(())
    }

    #[test]
    fn file_summary_marks_structural_symbol_graph_as_ok() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir(root.join("src"))?;
        fs::write(
            root.join("src").join("component.vue"),
            "<script setup>const selected = ref(false)</script>",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[test_node("src/component.vue", "hash-vue")])?;
        store.set_node_summary("src/component.vue", "vue component with bindings selected.")?;
        let mut structural_symbol = test_symbol("src/component.vue", SymbolKind::Value, "selected");
        structural_symbol.parser = ParserKind::Structural;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/component.vue".to_string(),
            language: Some("vue".to_string()),
            parser: ParserKind::Structural,
            symbols: vec![structural_symbol],
            relations: Vec::new(),
        })?;

        let report = build_file_summary(&store, Path::new("src/component.vue"), 10)?;
        require_eq(
            &report.parser_kind,
            &"structural-symbol-graph".to_string(),
            "structural parser kind",
        )?;
        require_eq(
            &report.summary_status,
            &"ok".to_string(),
            "structural summary status",
        )
    }

    #[test]
    fn file_summary_reports_mixed_vue_symbol_graph_with_structural_metadata()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir(root.join("src"))?;
        fs::write(
            root.join("src").join("component.vue"),
            "<script lang=\"ts\">export function submitOrder() {}</script>\n<script setup>const selected = ref(false)</script>",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[test_node("src/component.vue", "hash-vue")])?;
        store.set_node_summary(
            "src/component.vue",
            "vue source defining values selected and function submitOrder.",
        )?;
        let mut structural_symbol = test_symbol("src/component.vue", SymbolKind::Value, "selected");
        structural_symbol.parser = ParserKind::Structural;
        structural_symbol.detail = Some("vue-composition-binding".to_string());
        let mut fallback_symbol =
            test_symbol("src/component.vue", SymbolKind::Function, "submitOrder");
        fallback_symbol.parser = ParserKind::Fallback;
        fallback_symbol.detail = Some("fallback-js-function".to_string());
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/component.vue".to_string(),
            language: Some("vue".to_string()),
            parser: ParserKind::Structural,
            symbols: vec![structural_symbol, fallback_symbol],
            relations: Vec::new(),
        })?;

        let report = build_file_summary(&store, Path::new("src/component.vue"), 10)?;
        require_eq(
            &report.parser_kind,
            &"mixed-symbol-graph".to_string(),
            "mixed parser kind",
        )?;
        require_eq(
            &report.summary_status,
            &"ok".to_string(),
            "mixed summary status",
        )
    }

    #[test]
    fn module_aliases_include_package_entries_and_compound_extensions() -> Result<(), Box<dyn Error>>
    {
        require_eq(
            &module_aliases_for_path("src/packages/foo/index.ts"),
            &vec![
                "foo".to_string(),
                "packages.foo".to_string(),
                "packages::foo".to_string(),
            ],
            "typescript package entry aliases",
        )?;
        require_eq(
            &module_aliases_for_path("src/types/api.d.ts"),
            &vec![
                "api".to_string(),
                "types.api".to_string(),
                "types::api".to_string(),
            ],
            "typescript definition aliases",
        )?;
        require_eq(
            &module_aliases_for_path("src/package/__init__.py"),
            &vec!["package".to_string()],
            "python package entry aliases",
        )?;
        require_eq(
            &module_aliases_for_path("src/lib.rs"),
            &Vec::<String>::new(),
            "rust root lib aliases",
        )
    }

    #[test]
    fn file_summary_includes_cross_file_called_by() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir(root.join("src"))?;
        fs::write(
            root.join("src").join("lib.rs"),
            "/// Shared helper.\npub fn helper() {}\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/lib.rs", "hash-lib"),
            test_node("src/main.rs", "hash-main"),
        ])?;
        store.set_purpose(
            "src/lib.rs",
            "Provide shared library behavior",
            PurposeSource::Agent,
        )?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/lib.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![{
                let mut symbol = test_symbol("src/lib.rs", SymbolKind::Function, "helper");
                symbol.exported = true;
                symbol.line_start = 2;
                symbol.line_end = 2;
                symbol
            }],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/main.rs", SymbolKind::Function, "main")],
            relations: vec![SymbolRelation {
                path: "src/main.rs".to_string(),
                source_name: "main".to_string(),
                target_name: "crate::helper".to_string(),
                kind: RelationKind::Calls,
                line: 1,
                context: "helper();".to_string(),
                parser: ParserKind::TreeSitter,
            }],
        })?;

        let report = build_file_summary(&store, Path::new("src/lib.rs"), 10)?;
        require_eq(
            &report.file_purpose_status,
            &PurposeStatus::Approved.to_string(),
            "purpose status",
        )?;
        require_eq(
            &report.file_purpose_agent_reviewed,
            &true,
            "purpose agent reviewed",
        )?;
        let helper = report
            .functions
            .iter()
            .find(|symbol| symbol.name == "helper")
            .ok_or_else(|| io::Error::other("helper summary missing"))?;
        require_eq(
            &helper.called_by,
            &vec!["src/main.rs::main".to_string()],
            "cross-file called-by",
        )?;
        Ok(())
    }

    #[test]
    fn file_summary_rejects_ambiguous_called_by_matches() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir(root.join("src"))?;
        fs::write(root.join("src").join("a.rs"), "pub fn helper() {}\n")?;
        fs::write(root.join("src").join("b.rs"), "pub fn helper() {}\n")?;
        fs::write(
            root.join("src").join("main.rs"),
            "mod a;\nmod b;\nfn main() { b::helper(); }\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/a.rs", "hash-a"),
            test_node("src/b.rs", "hash-b"),
            test_node("src/main.rs", "hash-main"),
        ])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/a.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/a.rs", SymbolKind::Function, "helper")],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/b.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/b.rs", SymbolKind::Function, "helper")],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/main.rs", SymbolKind::Function, "main")],
            relations: vec![SymbolRelation {
                path: "src/main.rs".to_string(),
                source_name: "main".to_string(),
                target_name: "b::helper".to_string(),
                kind: RelationKind::Calls,
                line: 3,
                context: "b::helper();".to_string(),
                parser: ParserKind::TreeSitter,
            }],
        })?;

        let a_report = build_file_summary(&store, Path::new("src/a.rs"), 10)?;
        let a_helper = a_report
            .functions
            .iter()
            .find(|symbol| symbol.name == "helper")
            .ok_or_else(|| io::Error::other("a::helper summary missing"))?;
        require_eq(&a_helper.called_by, &Vec::<String>::new(), "a called-by")?;

        let b_report = build_file_summary(&store, Path::new("src/b.rs"), 10)?;
        let b_helper = b_report
            .functions
            .iter()
            .find(|symbol| symbol.name == "helper")
            .ok_or_else(|| io::Error::other("b::helper summary missing"))?;
        require_eq(
            &b_helper.called_by,
            &vec!["src/main.rs::main".to_string()],
            "b called-by",
        )?;
        Ok(())
    }

    #[test]
    fn file_summary_rejects_ambiguous_module_alias_called_by_matches() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src").join("foo"))?;
        fs::create_dir_all(root.join("src").join("bar"))?;
        fs::write(root.join("src/foo/service.rs"), "pub fn run() {}\n")?;
        fs::write(root.join("src/bar/service.rs"), "pub fn run() {}\n")?;
        fs::write(root.join("src/main.rs"), "fn main() { service::run(); }\n")?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/foo/service.rs", "hash-foo"),
            test_node("src/bar/service.rs", "hash-bar"),
            test_node("src/main.rs", "hash-main"),
        ])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/foo/service.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol(
                "src/foo/service.rs",
                SymbolKind::Function,
                "run",
            )],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/bar/service.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol(
                "src/bar/service.rs",
                SymbolKind::Function,
                "run",
            )],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/main.rs", SymbolKind::Function, "main")],
            relations: vec![SymbolRelation {
                path: "src/main.rs".to_string(),
                source_name: "main".to_string(),
                target_name: "service::run".to_string(),
                kind: RelationKind::Calls,
                line: 1,
                context: "service::run();".to_string(),
                parser: ParserKind::TreeSitter,
            }],
        })?;

        for path in ["src/foo/service.rs", "src/bar/service.rs"] {
            let report = build_file_summary(&store, Path::new(path), 10)?;
            let run = report
                .functions
                .iter()
                .find(|symbol| symbol.name == "run")
                .ok_or_else(|| io::Error::other("run summary missing"))?;
            require_eq(
                &run.called_by,
                &Vec::<String>::new(),
                "ambiguous module alias called-by",
            )?;
        }
        Ok(())
    }

    #[test]
    fn file_summary_resolves_rust_import_alias_called_by() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src/foo"))?;
        fs::write(root.join("src/foo/service.rs"), "pub fn run() {}\n")?;
        fs::write(
            root.join("src/main.rs"),
            "use crate::foo::service as foo_service;\nfn main() { foo_service::run(); }\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/foo/service.rs", "hash-service"),
            test_node("src/main.rs", "hash-main"),
        ])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/foo/service.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol(
                "src/foo/service.rs",
                SymbolKind::Function,
                "run",
            )],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/main.rs", SymbolKind::Function, "main")],
            relations: vec![
                SymbolRelation {
                    path: "src/main.rs".to_string(),
                    source_name: "<module>".to_string(),
                    target_name: "use crate::foo::service as foo_service;".to_string(),
                    kind: RelationKind::Imports,
                    line: 1,
                    context: "use crate::foo::service as foo_service;".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/main.rs".to_string(),
                    source_name: "main".to_string(),
                    target_name: "foo_service::run".to_string(),
                    kind: RelationKind::Calls,
                    line: 2,
                    context: "foo_service::run();".to_string(),
                    parser: ParserKind::TreeSitter,
                },
            ],
        })?;

        assert_single_called_by(
            &build_file_summary(&store, Path::new("src/foo/service.rs"), 10)?,
            "run",
            "src/main.rs::main",
        )
    }

    #[test]
    fn file_summary_resolves_typescript_named_import_alias_called_by() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("src/service.ts"), "export function run() {}\n")?;
        fs::write(
            root.join("src/main.ts"),
            "import { run as serviceRun } from \"./service\";\nserviceRun();\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/service.ts", "hash-service"),
            test_node("src/main.ts", "hash-main"),
        ])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/service.ts".to_string(),
            language: Some("typescript".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/service.ts", SymbolKind::Function, "run")],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.ts".to_string(),
            language: Some("typescript".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/main.ts", SymbolKind::Function, "main")],
            relations: vec![
                SymbolRelation {
                    path: "src/main.ts".to_string(),
                    source_name: "<module>".to_string(),
                    target_name: "import { run as serviceRun } from \"./service\";".to_string(),
                    kind: RelationKind::Imports,
                    line: 1,
                    context: "import { run as serviceRun } from \"./service\";".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/main.ts".to_string(),
                    source_name: "main".to_string(),
                    target_name: "serviceRun".to_string(),
                    kind: RelationKind::Calls,
                    line: 2,
                    context: "serviceRun();".to_string(),
                    parser: ParserKind::TreeSitter,
                },
            ],
        })?;

        assert_single_called_by(
            &build_file_summary(&store, Path::new("src/service.ts"), 10)?,
            "run",
            "src/main.ts::main",
        )
    }

    #[test]
    fn file_summary_resolves_typescript_explicit_index_import_called_by()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src/api"))?;
        fs::write(root.join("src/api/index.ts"), "export function run() {}\n")?;
        fs::write(
            root.join("src/main.ts"),
            "import { run as apiRun } from \"./api/index\";\napiRun();\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/api/index.ts", "hash-api"),
            test_node("src/main.ts", "hash-main"),
        ])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/api/index.ts".to_string(),
            language: Some("typescript".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/api/index.ts", SymbolKind::Function, "run")],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.ts".to_string(),
            language: Some("typescript".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/main.ts", SymbolKind::Function, "main")],
            relations: vec![
                SymbolRelation {
                    path: "src/main.ts".to_string(),
                    source_name: "<module>".to_string(),
                    target_name: "import { run as apiRun } from \"./api/index\";".to_string(),
                    kind: RelationKind::Imports,
                    line: 1,
                    context: "import { run as apiRun } from \"./api/index\";".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/main.ts".to_string(),
                    source_name: "main".to_string(),
                    target_name: "apiRun".to_string(),
                    kind: RelationKind::Calls,
                    line: 2,
                    context: "apiRun();".to_string(),
                    parser: ParserKind::TreeSitter,
                },
            ],
        })?;

        assert_single_called_by(
            &build_file_summary(&store, Path::new("src/api/index.ts"), 10)?,
            "run",
            "src/main.ts::main",
        )
    }

    #[test]
    fn file_summary_rejects_unrelated_typescript_alias_collision() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("src/service.ts"), "export function run() {}\n")?;
        fs::write(root.join("src/format.ts"), "export function format() {}\n")?;
        fs::write(
            root.join("src/main.ts"),
            "import { run as call } from \"./service\";\nimport { format as call } from \"./format\";\ncall();\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/service.ts", "hash-service"),
            test_node("src/format.ts", "hash-format"),
            test_node("src/main.ts", "hash-main"),
        ])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/service.ts".to_string(),
            language: Some("typescript".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/service.ts", SymbolKind::Function, "run")],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/format.ts".to_string(),
            language: Some("typescript".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/format.ts", SymbolKind::Function, "format")],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.ts".to_string(),
            language: Some("typescript".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/main.ts", SymbolKind::Function, "main")],
            relations: vec![
                SymbolRelation {
                    path: "src/main.ts".to_string(),
                    source_name: "<module>".to_string(),
                    target_name: "import { run as call } from \"./service\";".to_string(),
                    kind: RelationKind::Imports,
                    line: 1,
                    context: "import { run as call } from \"./service\";".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/main.ts".to_string(),
                    source_name: "<module>".to_string(),
                    target_name: "import { format as call } from \"./format\";".to_string(),
                    kind: RelationKind::Imports,
                    line: 2,
                    context: "import { format as call } from \"./format\";".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/main.ts".to_string(),
                    source_name: "main".to_string(),
                    target_name: "call".to_string(),
                    kind: RelationKind::Calls,
                    line: 3,
                    context: "call();".to_string(),
                    parser: ParserKind::TreeSitter,
                },
            ],
        })?;

        let report = build_file_summary(&store, Path::new("src/service.ts"), 10)?;
        let run = report
            .functions
            .iter()
            .find(|symbol| symbol.name == "run")
            .ok_or_else(|| io::Error::other("run summary missing"))?;
        require_eq(
            &run.called_by,
            &Vec::<String>::new(),
            "unrelated alias collision called-by",
        )
    }

    #[test]
    fn file_summary_resolves_python_import_alias_called_by() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src/package"))?;
        fs::write(root.join("src/package/module.py"), "def run():\n    pass\n")?;
        fs::write(
            root.join("src/main.py"),
            "import package.module as service\nservice.run()\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/package/module.py", "hash-module"),
            test_node("src/main.py", "hash-main"),
        ])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/package/module.py".to_string(),
            language: Some("python".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol(
                "src/package/module.py",
                SymbolKind::Function,
                "run",
            )],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.py".to_string(),
            language: Some("python".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/main.py", SymbolKind::Function, "main")],
            relations: vec![
                SymbolRelation {
                    path: "src/main.py".to_string(),
                    source_name: "<module>".to_string(),
                    target_name: "import package.module as service".to_string(),
                    kind: RelationKind::Imports,
                    line: 1,
                    context: "import package.module as service".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/main.py".to_string(),
                    source_name: "main".to_string(),
                    target_name: "service.run".to_string(),
                    kind: RelationKind::Calls,
                    line: 2,
                    context: "service.run()".to_string(),
                    parser: ParserKind::TreeSitter,
                },
            ],
        })?;

        assert_single_called_by(
            &build_file_summary(&store, Path::new("src/package/module.py"), 10)?,
            "run",
            "src/main.py::main",
        )
    }

    #[test]
    fn file_summary_resolves_python_no_alias_import_when_name_is_ambiguous()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src/package"))?;
        fs::write(root.join("src/package/module.py"), "def run():\n    pass\n")?;
        fs::write(
            root.join("src/main.py"),
            "from package.module import run\nrun()\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/package/module.py", "hash-module"),
            test_node("src/main.py", "hash-main"),
        ])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/package/module.py".to_string(),
            language: Some("python".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol(
                "src/package/module.py",
                SymbolKind::Function,
                "run",
            )],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.py".to_string(),
            language: Some("python".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![
                test_symbol("src/main.py", SymbolKind::Function, "main"),
                test_symbol("src/main.py", SymbolKind::Import, "run"),
            ],
            relations: vec![
                SymbolRelation {
                    path: "src/main.py".to_string(),
                    source_name: "<module>".to_string(),
                    target_name: "from package.module import run".to_string(),
                    kind: RelationKind::Imports,
                    line: 1,
                    context: "from package.module import run".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/main.py".to_string(),
                    source_name: "main".to_string(),
                    target_name: "run".to_string(),
                    kind: RelationKind::Calls,
                    line: 2,
                    context: "run()".to_string(),
                    parser: ParserKind::TreeSitter,
                },
            ],
        })?;

        assert_single_called_by(
            &build_file_summary(&store, Path::new("src/package/module.py"), 10)?,
            "run",
            "src/main.py::main",
        )
    }

    #[test]
    fn file_summary_rejects_ambiguous_import_alias_called_by() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src/foo"))?;
        fs::create_dir_all(root.join("src/bar"))?;
        fs::write(root.join("src/foo/service.py"), "def run():\n    pass\n")?;
        fs::write(root.join("src/bar/service.py"), "def run():\n    pass\n")?;
        fs::write(
            root.join("src/main.py"),
            "from foo.service import run as call_service\nfrom bar.service import run as call_service\ncall_service()\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/foo/service.py", "hash-foo"),
            test_node("src/bar/service.py", "hash-bar"),
            test_node("src/main.py", "hash-main"),
        ])?;
        for path in ["src/foo/service.py", "src/bar/service.py"] {
            store.replace_symbol_graph(&SymbolGraph {
                path: path.to_string(),
                language: Some("python".to_string()),
                parser: ParserKind::TreeSitter,
                symbols: vec![test_symbol(path, SymbolKind::Function, "run")],
                relations: Vec::new(),
            })?;
        }
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.py".to_string(),
            language: Some("python".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol("src/main.py", SymbolKind::Function, "main")],
            relations: vec![
                SymbolRelation {
                    path: "src/main.py".to_string(),
                    source_name: "<module>".to_string(),
                    target_name: "from foo.service import run as call_service".to_string(),
                    kind: RelationKind::Imports,
                    line: 1,
                    context: "from foo.service import run as call_service".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/main.py".to_string(),
                    source_name: "<module>".to_string(),
                    target_name: "from bar.service import run as call_service".to_string(),
                    kind: RelationKind::Imports,
                    line: 2,
                    context: "from bar.service import run as call_service".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/main.py".to_string(),
                    source_name: "main".to_string(),
                    target_name: "call_service".to_string(),
                    kind: RelationKind::Calls,
                    line: 3,
                    context: "call_service()".to_string(),
                    parser: ParserKind::TreeSitter,
                },
            ],
        })?;

        for path in ["src/foo/service.py", "src/bar/service.py"] {
            let report = build_file_summary(&store, Path::new(path), 10)?;
            let run = report
                .functions
                .iter()
                .find(|symbol| symbol.name == "run")
                .ok_or_else(|| io::Error::other("run summary missing"))?;
            require_eq(
                &run.called_by,
                &Vec::<String>::new(),
                "ambiguous import alias called-by",
            )?;
        }
        Ok(())
    }

    #[test]
    fn file_summary_marks_indexed_metadata_fallback() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[test_node("src/missing.rs", "hash-missing")])?;

        let report = build_file_summary(&store, Path::new("src/missing.rs"), 10)?;
        require_eq(
            &report.source_status,
            &SOURCE_STATUS_INDEXED.to_string(),
            "source status",
        )?;
        if report.source_error.is_empty() {
            return Err(io::Error::other("source fallback error was empty").into());
        }
        Ok(())
    }

    #[test]
    fn search_uses_globset_and_stops_after_requested_page() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(root.join("docs"))?;
        fs::write(root.join("src").join("a.rs"), "needle one\n")?;
        fs::write(root.join("src").join("b.rs"), "needle two\n")?;
        fs::write(root.join("docs").join("readme.md"), "needle docs\n")?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[
            test_node("src/a.rs", "hash-a"),
            test_node("src/b.rs", "hash-b"),
            test_node("docs/readme.md", "hash-docs"),
        ])?;
        index_test_file_texts(
            &mut store,
            root,
            &[
                test_node("src/a.rs", "hash-a"),
                test_node("src/b.rs", "hash-b"),
                test_node("docs/readme.md", "hash-docs"),
            ],
        )?;

        let report =
            search_indexed_files(&store, "needle", false, false, false, Some("*.rs"), 0, 0, 1)?;
        require_eq(&report.returned, &1, "returned rows")?;
        require_eq(&report.searched_files, &1, "bounded searched files")?;
        require_eq(&report.truncated, &true, "truncated flag")?;
        require_eq(&report.observed_total, &report.total, "observed total")?;
        require_eq(
            &report.total_is_complete,
            &false,
            "truncated search completeness",
        )?;

        let report = search_indexed_files(
            &store,
            "needle",
            false,
            false,
            false,
            Some("src\\*.rs"),
            0,
            0,
            10,
        )?;
        require_eq(&report.returned, &2, "windows glob returned rows")?;
        require_eq(&report.total_is_complete, &true, "complete search total")?;
        if report
            .results
            .iter()
            .any(|row| row.path == "docs/readme.md")
        {
            return Err(io::Error::other("globset filter included docs/readme.md").into());
        }
        Ok(())
    }

    #[test]
    fn classified_summary_search_and_ranking_filter_before_result_limits()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(root.join("docs"))?;
        fs::write(root.join("src/a.rs"), "needle source\n")?;
        fs::write(root.join("docs/guide.md"), "needle documentation\n")?;
        fs::write(root.join("settings.toml"), "needle = 'configuration'\n")?;
        let nodes = [
            test_node_with_language("src/a.rs", "hash-source", ".rs", "rust"),
            test_node_with_language("docs/guide.md", "hash-documentation", ".md", "markdown"),
            test_node_with_language("settings.toml", "hash-configuration", ".toml", "toml"),
        ];
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&nodes)?;
        index_test_file_texts(&mut store, root, &nodes)?;
        let project = store
            .project_instance_id()?
            .ok_or("classified service fixture project identity is missing")?;
        let mut publication = store.begin_index_publication("classified-service-test")?;
        publication.upsert_file_content_classification_batch(&[
            projectatlas_db::FileContentClassification {
                path: "src/a.rs".to_string(),
                classification: ContentClassification::Source,
            },
            projectatlas_db::FileContentClassification {
                path: "docs/guide.md".to_string(),
                classification: ContentClassification::Documentation,
            },
            projectatlas_db::FileContentClassification {
                path: "settings.toml".to_string(),
                classification: ContentClassification::ConfigurationData,
            },
        ])?;
        publication.replace_repository_graph(project, &[], &[], &[], &[])?;
        publication.complete()?;
        store.set_purpose(
            "docs/guide.md",
            "Misleading source-code wording must not override classification",
            PurposeSource::Agent,
        )?;

        let summary = build_file_summary_with_selection(
            &store,
            Path::new("docs/guide.md"),
            10,
            ContentSelection::Documentation,
        )?;
        require_eq(
            &summary.classification,
            &ContentClassification::Documentation,
            "summary classification",
        )?;
        require(
            summary.file_purpose.contains("source-code wording")
                && summary.classification == ContentClassification::Documentation,
            "purpose mutation overrode the registry-owned file classification",
        )?;
        let legacy_summary = build_file_summary(&store, Path::new("docs/guide.md"), 10)?;
        let explicit_legacy_summary = build_file_summary_with_selection(
            &store,
            Path::new("docs/guide.md"),
            10,
            ContentSelection::UnspecifiedLegacy,
        )?;
        require_eq(
            &serde_json::to_value(&legacy_summary)?,
            &serde_json::to_value(&explicit_legacy_summary)?,
            "legacy summary wrapper compatibility",
        )?;
        let slice = read_indexed_code_slice_from_source_with_selection(
            &store,
            Path::new("docs/guide.md"),
            1,
            Some(1),
            "# Guide\n",
            ContentSelection::Documentation,
        )?;
        require_eq(
            &slice.classification,
            &Some(ContentClassification::Documentation),
            "indexed slice classification",
        )?;
        if !matches!(
            build_file_summary_with_selection(
                &store,
                Path::new("docs/guide.md"),
                10,
                ContentSelection::Source,
            ),
            Err(ServiceError::InvalidInput(message)) if message.contains("outside the selected content")
        ) {
            return Err(io::Error::other(
                "summary accepted a file outside the explicit content selection",
            )
            .into());
        }

        let legacy = search_indexed_files(&store, "needle", false, false, false, None, 0, 0, 10)?;
        let explicit_legacy = search_indexed_files_with_control(
            &store,
            &SearchQuery {
                pattern: "needle",
                regex: false,
                fuzzy: false,
                case_sensitive: false,
                file_pattern: None,
                context_lines: 0,
                start_index: 0,
                limit: 10,
                content_selection: ContentSelection::UnspecifiedLegacy,
                retrieval_mode: SearchRetrievalMode::Lexical,
            },
            None,
        )?;
        require_eq(
            &serde_json::to_value(&legacy)?,
            &serde_json::to_value(&explicit_legacy)?,
            "legacy search wrapper compatibility",
        )?;
        require_eq(&legacy.returned, &3, "legacy mixed search rows")?;
        require(
            legacy.results.iter().map(|row| row.classification).eq([
                ContentClassification::Documentation,
                ContentClassification::ConfigurationData,
                ContentClassification::Source,
            ]),
            "legacy search omitted or reordered mixed classifications",
        )?;

        for regex in [false, true] {
            let documentation = search_indexed_files_with_control(
                &store,
                &SearchQuery {
                    pattern: "needle",
                    regex,
                    fuzzy: false,
                    case_sensitive: false,
                    file_pattern: None,
                    context_lines: 0,
                    start_index: 0,
                    limit: 1,
                    content_selection: ContentSelection::Documentation,
                    retrieval_mode: SearchRetrievalMode::Lexical,
                },
                None,
            )?;
            require(
                documentation.returned == 1
                    && documentation.results[0].path == "docs/guide.md"
                    && documentation.results[0].classification
                        == ContentClassification::Documentation,
                "documentation selection was applied after the search result limit",
            )?;
        }

        let both = search_indexed_files_with_control(
            &store,
            &SearchQuery {
                pattern: "needle",
                regex: true,
                fuzzy: false,
                case_sensitive: false,
                file_pattern: None,
                context_lines: 0,
                start_index: 0,
                limit: 10,
                content_selection: ContentSelection::Both,
                retrieval_mode: SearchRetrievalMode::Lexical,
            },
            None,
        )?;
        require(
            both.returned == 2
                && both.results.iter().all(|row| {
                    matches!(
                        row.classification,
                        ContentClassification::Source | ContentClassification::Documentation
                    )
                }),
            "both selection did not exclude configuration data",
        )?;

        let ranked = load_classified_ranked_file_nodes_with_reasons(
            &store,
            "",
            None,
            None,
            1,
            false,
            ContentSelection::Documentation,
        )?;
        require(
            ranked.len() == 1
                && ranked[0].node.node.path == "docs/guide.md"
                && ranked[0].classification == ContentClassification::Documentation,
            "documentation selection was applied after the ranked-file limit",
        )?;
        let documentation_next =
            build_next_report_with_selection(&store, "", Some(1), ContentSelection::Documentation)?;
        require(
            !documentation_next.suggestions.is_empty()
                && documentation_next
                    .suggestions
                    .iter()
                    .all(|suggestion| suggestion.contains("--content-selection documentation")),
            "classified next suggestions lost the explicit content selection",
        )?;
        let legacy_next = build_next_report(&store, "", Some(3))?;
        let explicit_legacy_next = build_next_report_with_selection(
            &store,
            "",
            Some(3),
            ContentSelection::UnspecifiedLegacy,
        )?;
        require_eq(
            &serde_json::to_value(&legacy_next)?,
            &serde_json::to_value(&explicit_legacy_next)?,
            "legacy next wrapper compatibility",
        )?;
        Ok(())
    }

    #[test]
    fn search_fts_candidates_preserve_fallback_results_and_unsafe_shapes_fall_back()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(root.join("docs"))?;
        fs::write(
            root.join("src/a.rs"),
            "Needle alpha\nneedle-beta\nnëedle unicode\n",
        )?;
        fs::write(root.join("src/b.rs"), "prefixneedlesuffix gamma\n")?;
        fs::write(root.join("docs/readme.md"), "needle docs\n")?;
        let nodes = [
            test_node("src/a.rs", "hash-a"),
            test_node("src/b.rs", "hash-b"),
            test_node("docs/readme.md", "hash-docs"),
        ];
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&nodes)?;
        index_test_file_texts(&mut store, root, &nodes)?;

        let lexical = search_indexed_files_with_control(
            &store,
            &SearchQuery {
                pattern: "needle",
                regex: false,
                fuzzy: false,
                case_sensitive: false,
                file_pattern: Some("src/*.rs"),
                context_lines: 0,
                start_index: 0,
                limit: 20,
                content_selection: ContentSelection::UnspecifiedLegacy,
                retrieval_mode: SearchRetrievalMode::Lexical,
            },
            None,
        )?;
        require_eq(
            &lexical.strategy,
            &"fts5-bm25-candidates-exact-verified".to_string(),
            "safe literal strategy",
        )?;
        require_eq(&lexical.candidate_files, &2, "safe literal candidates")?;

        let fallback = search_indexed_files_with_control(
            &store,
            &SearchQuery {
                pattern: "needle",
                regex: true,
                fuzzy: false,
                case_sensitive: false,
                file_pattern: Some("src/*.rs"),
                context_lines: 0,
                start_index: 0,
                limit: 20,
                content_selection: ContentSelection::UnspecifiedLegacy,
                retrieval_mode: SearchRetrievalMode::Lexical,
            },
            None,
        )?;
        let lexical_rows = lexical
            .results
            .iter()
            .map(|row| (&row.path, row.line, &row.text))
            .collect::<Vec<_>>();
        let fallback_rows = fallback
            .results
            .iter()
            .map(|row| (&row.path, row.line, &row.text))
            .collect::<Vec<_>>();
        require_eq(
            &lexical_rows,
            &fallback_rows,
            "FTS and fallback exact results",
        )?;
        require_eq(
            &fallback.strategy,
            &"persisted-text-fallback".to_string(),
            "regex fallback strategy",
        )?;

        for (pattern, regex, fuzzy, expected_rows) in [
            (
                "ne",
                false,
                false,
                vec!["src/a.rs:1", "src/a.rs:2", "src/b.rs:1"],
            ),
            ("needle-", false, false, vec!["src/a.rs:2"]),
            ("nëedle", false, false, vec!["src/a.rs:3"]),
            (
                "needle",
                false,
                true,
                vec!["src/a.rs:1", "src/a.rs:2", "src/b.rs:1"],
            ),
        ] {
            let report = search_indexed_files_with_control(
                &store,
                &SearchQuery {
                    pattern,
                    regex,
                    fuzzy,
                    case_sensitive: false,
                    file_pattern: Some("src/*.rs"),
                    context_lines: 0,
                    start_index: 0,
                    limit: 20,
                    content_selection: ContentSelection::UnspecifiedLegacy,
                    retrieval_mode: SearchRetrievalMode::Lexical,
                },
                None,
            )?;
            require_eq(
                &report.strategy,
                &"persisted-text-fallback".to_string(),
                "unsafe shape fallback strategy",
            )?;
            require_eq(&report.candidate_files, &0, "unsafe shape candidates")?;
            let rows = report
                .results
                .iter()
                .map(|row| format!("{}:{}", row.path, row.line))
                .collect::<Vec<_>>();
            require_eq(
                &rows,
                &expected_rows
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
                "unsafe fallback exact rows",
            )?;
        }

        for (pattern, expected_rows) in [("Needle", 1), ("needle", 2)] {
            let exact = search_indexed_files_with_control(
                &store,
                &SearchQuery {
                    pattern,
                    regex: false,
                    fuzzy: false,
                    case_sensitive: true,
                    file_pattern: Some("src/*.rs"),
                    context_lines: 0,
                    start_index: 0,
                    limit: 20,
                    content_selection: ContentSelection::UnspecifiedLegacy,
                    retrieval_mode: SearchRetrievalMode::Lexical,
                },
                None,
            )?;
            let regex = search_indexed_files_with_control(
                &store,
                &SearchQuery {
                    pattern,
                    regex: true,
                    fuzzy: false,
                    case_sensitive: true,
                    file_pattern: Some("src/*.rs"),
                    context_lines: 0,
                    start_index: 0,
                    limit: 20,
                    content_selection: ContentSelection::UnspecifiedLegacy,
                    retrieval_mode: SearchRetrievalMode::Lexical,
                },
                None,
            )?;
            let exact_rows = exact
                .results
                .iter()
                .map(|row| (&row.path, row.line, &row.text))
                .collect::<Vec<_>>();
            let regex_rows = regex
                .results
                .iter()
                .map(|row| (&row.path, row.line, &row.text))
                .collect::<Vec<_>>();
            require_eq(&exact_rows, &regex_rows, "case-sensitive equivalence")?;
            require_eq(&exact.returned, &expected_rows, "case-sensitive exact rows")?;
        }
        Ok(())
    }

    #[test]
    fn search_fts_candidate_overflow_uses_complete_persisted_text_fallback()
    -> Result<(), Box<dyn Error>> {
        const MATCHING_FILES: usize = MAX_FILE_TEXT_FTS_CANDIDATES + 1;
        const CONTENT: &str = "needle\n";
        let mut store = AtlasStore::in_memory()?;
        let paths = (0..MATCHING_FILES)
            .map(|index| format!("overflow/{index:04}.rs"))
            .collect::<Vec<_>>();
        let nodes = paths
            .iter()
            .map(|path| test_node(path, "hash"))
            .collect::<Vec<_>>();
        let texts = paths
            .iter()
            .map(|path| IndexedFileText {
                path: path.clone(),
                content_hash: Some("hash".to_string()),
                byte_count: CONTENT.len(),
                line_count: 1,
                content: CONTENT.to_string(),
            })
            .collect::<Vec<_>>();
        store.replace_scan(&nodes)?;
        store.replace_file_texts_for_paths(&paths, &texts)?;

        let request = SearchQuery {
            pattern: "needle",
            regex: false,
            fuzzy: false,
            case_sensitive: false,
            file_pattern: Some("overflow/*.rs"),
            context_lines: 0,
            start_index: MATCHING_FILES - 1,
            limit: 1,
            content_selection: ContentSelection::UnspecifiedLegacy,
            retrieval_mode: SearchRetrievalMode::Lexical,
        };
        let overflow = search_indexed_files_with_control(&store, &request, None)?;
        require_eq(
            &overflow.strategy,
            &"persisted-text-fallback".to_string(),
            "overflow fallback strategy",
        )?;
        require_eq(
            &overflow.candidate_files,
            &MAX_FILE_TEXT_FTS_CANDIDATES,
            "overflow retained candidates",
        )?;
        require_eq(
            &overflow.searched_files,
            &MATCHING_FILES,
            "overflow fallback searched files",
        )?;
        require_eq(
            &overflow.searched_bytes,
            &(MATCHING_FILES * CONTENT.len()),
            "overflow fallback searched bytes",
        )?;
        require_eq(
            &overflow.results[0].path,
            &format!("overflow/{:04}.rs", MATCHING_FILES - 1),
            "overflow fallback exact path order",
        )?;

        let authoritative = search_indexed_files_with_control(
            &store,
            &SearchQuery {
                regex: true,
                ..request
            },
            None,
        )?;
        let overflow_rows = overflow
            .results
            .iter()
            .map(|row| (&row.path, row.line, &row.text))
            .collect::<Vec<_>>();
        let authoritative_rows = authoritative
            .results
            .iter()
            .map(|row| (&row.path, row.line, &row.text))
            .collect::<Vec<_>>();
        require_eq(
            &overflow_rows,
            &authoritative_rows,
            "overflow and authoritative fallback rows",
        )?;
        Ok(())
    }

    #[test]
    fn search_reports_resource_bounds_cancellation_and_optional_capability_state()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("src/a.rs"), "needle one\n")?;
        fs::write(root.join("src/b.rs"), "needle two\n")?;
        let nodes = [
            test_node("src/a.rs", "hash-a"),
            test_node("src/b.rs", "hash-b"),
        ];
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&nodes)?;
        index_test_file_texts(&mut store, root, &nodes)?;
        let query = SearchQuery {
            pattern: "needle",
            regex: true,
            fuzzy: false,
            case_sensitive: false,
            file_pattern: Some("src/*.rs"),
            context_lines: 0,
            start_index: 0,
            limit: 20,
            content_selection: ContentSelection::UnspecifiedLegacy,
            retrieval_mode: SearchRetrievalMode::Lexical,
        };

        let file_bounded = search_indexed_files_with_bounds(
            &store,
            &query,
            None,
            SearchBounds {
                selected_files: 1,
                selected_bytes: usize::MAX,
                elapsed: Duration::from_secs(1),
                retained_bytes: usize::MAX,
            },
        )?;
        require_eq(
            &file_bounded.searched_files,
            &1,
            "file bound searched files",
        )?;
        require_eq(&file_bounded.truncated, &true, "file bound truncation")?;
        require_eq(
            &file_bounded.truncation_reason,
            &Some("selected-file-limit".to_string()),
            "file bound reason",
        )?;

        let byte_bounded = search_indexed_files_with_bounds(
            &store,
            &query,
            None,
            SearchBounds {
                selected_files: usize::MAX,
                selected_bytes: 1,
                elapsed: Duration::from_secs(1),
                retained_bytes: usize::MAX,
            },
        )?;
        require_eq(
            &byte_bounded.searched_files,
            &0,
            "byte bound searched files",
        )?;
        require_eq(
            &byte_bounded.truncation_reason,
            &Some("selected-byte-limit".to_string()),
            "byte bound reason",
        )?;

        let output_bounded = search_indexed_files_with_bounds(
            &store,
            &query,
            None,
            SearchBounds {
                selected_files: usize::MAX,
                selected_bytes: usize::MAX,
                elapsed: Duration::from_secs(1),
                retained_bytes: 1,
            },
        )?;
        require_eq(&output_bounded.returned, &0, "output bound returned rows")?;
        require_eq(
            &output_bounded.truncation_reason,
            &Some("retained-byte-limit".to_string()),
            "output bound reason",
        )?;

        let cancellation = IndexCancellation::new();
        cancellation.cancel();
        let control = IndexWorkControl::new(cancellation, None);
        let cancelled = search_indexed_files_with_control(&store, &query, Some(&control));
        if !matches!(cancelled, Err(ServiceError::Db(DbError::IndexWork(_)))) {
            return Err(io::Error::other("search cancellation was not typed").into());
        }
        let expired = IndexWorkControl::new(IndexCancellation::new(), Some(Duration::ZERO));
        let deadline = search_indexed_files_with_control(&store, &query, Some(&expired))?;
        require_eq(&deadline.truncated, &true, "deadline truncation")?;
        require_eq(
            &deadline.truncation_reason,
            &Some("elapsed-time-limit".to_string()),
            "deadline truncation reason",
        )?;
        require_eq(
            &deadline.total_is_complete,
            &false,
            "deadline total completeness",
        )?;

        let line_cancellation = IndexCancellation::new();
        let line_control = IndexWorkControl::new(line_cancellation.clone(), None);
        line_cancellation.cancel();
        let mut line_report = file_bounded;
        let line_match = append_line_matches(
            &mut line_report,
            "src/a.rs",
            ContentClassification::Source,
            &["needle"],
            &LineMatcher::Literal {
                needle: "needle".to_string(),
                case_sensitive: true,
            },
            0,
            1,
            usize::MAX,
            &line_control,
        );
        if !matches!(
            line_match,
            Err(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::TextIndex
            })
        ) {
            return Err(io::Error::other("in-memory line matching ignored cancellation").into());
        }

        let maximum_pattern = "a".repeat(SEARCH_MAX_PATTERN_BYTES);
        search_indexed_files_with_control(
            &store,
            &SearchQuery {
                pattern: &maximum_pattern,
                regex: false,
                limit: 0,
                ..query
            },
            None,
        )?;
        let oversized_pattern = "a".repeat(SEARCH_MAX_PATTERN_BYTES + 1);
        if !matches!(
            search_indexed_files_with_control(
                &store,
                &SearchQuery {
                    pattern: &oversized_pattern,
                    regex: false,
                    limit: 0,
                    ..query
                },
                None,
            ),
            Err(ServiceError::InvalidInput(_))
        ) {
            return Err(io::Error::other("oversized search pattern was accepted").into());
        }
        let maximum_file_pattern = "a".repeat(SEARCH_MAX_FILE_PATTERN_BYTES);
        search_indexed_files_with_control(
            &store,
            &SearchQuery {
                pattern: "needle",
                regex: false,
                file_pattern: Some(&maximum_file_pattern),
                limit: 0,
                ..query
            },
            None,
        )?;
        let oversized_file_pattern = "a".repeat(SEARCH_MAX_FILE_PATTERN_BYTES + 1);
        if !matches!(
            search_indexed_files_with_control(
                &store,
                &SearchQuery {
                    pattern: "needle",
                    regex: false,
                    file_pattern: Some(&oversized_file_pattern),
                    limit: 0,
                    ..query
                },
                None,
            ),
            Err(ServiceError::InvalidInput(_))
        ) {
            return Err(io::Error::other("oversized search file pattern was accepted").into());
        }

        let unavailable = search_indexed_files_with_control(
            &store,
            &SearchQuery {
                retrieval_mode: SearchRetrievalMode::Semantic,
                ..query
            },
            None,
        );
        if !matches!(
            unavailable,
            Err(ServiceError::SearchCapabilityUnavailable {
                requested_mode: SearchRetrievalMode::Semantic,
                state: SEARCH_SEMANTIC_UNAVAILABLE_STATE,
                guidance: SEARCH_SEMANTIC_RECOVERY,
            })
        ) {
            return Err(io::Error::other("semantic unavailable state was not typed").into());
        }
        Ok(())
    }

    #[test]
    fn file_glob_filter_matches_repository_paths() -> Result<(), Box<dyn Error>> {
        let nodes = vec![
            test_indexed_node("src/a.rs", "hash-a"),
            test_indexed_node("src/nested/b.rs", "hash-b"),
            test_indexed_node("docs/readme.md", "hash-docs"),
        ];

        let filtered = filter_files_by_glob(nodes.clone(), Some("*.rs"))?;
        require_eq(&filtered.len(), &2, "rs glob count")?;
        let matcher = FilePathMatcher::new(Some("*.rs"))?;
        require_eq(&matcher.filters(), &true, "compiled glob filters")?;
        require_eq(&matcher.is_match("src/a.rs"), &true, "compiled nested rs")?;
        require_eq(&matcher.is_match("a.rs"), &true, "compiled basename rs")?;
        require_eq(
            &matcher.is_match("docs/readme.md"),
            &false,
            "compiled markdown miss",
        )?;

        let nested = filter_files_by_glob(nodes, Some("src\\nested\\*.rs"))?;
        require_eq(&nested.len(), &1, "windows glob count")?;
        require_eq(
            &nested[0].node.path,
            &"src/nested/b.rs".to_string(),
            "windows glob path",
        )?;
        Ok(())
    }

    #[test]
    fn ranked_file_nodes_uses_shared_glob_policy() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_node("src/a.rs", "hash-a"),
            test_node("src/nested/b.rs", "hash-b"),
            test_node("docs/readme.md", "hash-docs"),
        ])?;
        for path in ["src/a.rs", "src/nested/b.rs", "docs/readme.md"] {
            store.set_purpose(path, "needle orientation target", PurposeSource::Agent)?;
            store.set_node_summary(path, "needle indexed summary")?;
        }

        let selected = load_ranked_file_nodes(&store, "needle", None, Some("*.rs"), 10, false)?;
        require_eq(&selected.len(), &2, "ranked rs glob count")?;
        if selected
            .iter()
            .any(|node| node.node.path == "docs/readme.md")
        {
            return Err(io::Error::other("ranked glob included docs/readme.md").into());
        }

        let nested =
            load_ranked_file_nodes(&store, "needle", None, Some("src/nested/*.rs"), 10, false)?;
        require_eq(&nested.len(), &1, "ranked nested glob count")?;
        require_eq(
            &nested[0].node.path,
            &"src/nested/b.rs".to_string(),
            "ranked nested glob path",
        )?;
        Ok(())
    }

    #[test]
    fn ranked_file_nodes_can_include_indexed_text_hits() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(root.join("docs"))?;
        fs::write(
            root.join("src").join("owner.rs"),
            "const ROUTE = \"hiddenNeedle\";\n",
        )?;
        fs::write(root.join("docs").join("owner.md"), "hiddenNeedle docs\n")?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        let nodes = [
            test_node("src/owner.rs", "hash-src-owner"),
            test_node("docs/owner.md", "hash-doc-owner"),
        ];
        store.replace_scan(&nodes)?;
        index_test_file_texts(&mut store, root, &nodes)?;

        let default_ranked =
            load_ranked_file_nodes(&store, "hiddenNeedle", Some("src"), Some("*.rs"), 10, false)?;
        require_eq(
            &default_ranked.len(),
            &0,
            "default ranking ignores content-only hits",
        )?;

        let content_ranked =
            load_ranked_file_nodes(&store, "hiddenNeedle", Some("src"), Some("*.rs"), 10, true)?;
        require_eq(&content_ranked.len(), &1, "content-aware ranked count")?;
        require_eq(
            &content_ranked[0].node.path,
            &"src/owner.rs".to_string(),
            "content-aware ranked path",
        )?;
        Ok(())
    }

    #[test]
    fn ranked_file_reasons_match_indexed_ranking_signals() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(root.join("tests"))?;
        fs::write(
            root.join("src").join("installer.rs"),
            "pub fn install_runtime() { let _marker = \"hiddenNeedle\"; }\n",
        )?;
        fs::write(
            root.join("tests").join("installer.rs"),
            "#[test]\nfn installer_pair() {}\n",
        )?;
        fs::write(root.join("src").join("noise.rs"), "pub fn unrelated() {}\n")?;

        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        let nodes = [
            test_node("src/installer.rs", "hash-installer"),
            test_node("tests/installer.rs", "hash-installer-test"),
            test_node("src/noise.rs", "hash-noise"),
        ];
        store.replace_scan(&nodes)?;
        store.set_purpose(
            "src/installer.rs",
            "Installer runtime release target",
            PurposeSource::Agent,
        )?;
        store.set_node_summary("src/installer.rs", "Release installer summary")?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/installer.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![test_symbol(
                "src/installer.rs",
                SymbolKind::Function,
                "install_runtime",
            )],
            relations: Vec::new(),
        })?;
        index_test_file_texts(&mut store, root, &nodes)?;

        let ranked = load_ranked_file_nodes_with_reasons(
            &store,
            "installer runtime release hiddenNeedle install_runtime",
            None,
            Some("*.rs"),
            2,
            true,
        )?;
        require_eq(&ranked.len(), &2, "ranked source/test pair count")?;
        require_eq(
            &ranked[0].node.node.path,
            &"src/installer.rs".to_string(),
            "strong indexed signal ranks first",
        )?;
        require_reason(&ranked[0].reasons, "path matched install")?;
        require_reason(&ranked[0].reasons, "purpose matched install")?;
        require_reason(&ranked[0].reasons, "summary matched install")?;
        require_reason(&ranked[0].reasons, "symbol install_runtime matched install")?;
        require_reason(&ranked[0].reasons, "indexed text matched hiddenneedle")?;
        require_reason(&ranked[0].reasons, "paired test file tests/installer.rs")?;
        require_eq(
            &ranked[0]
                .reason_codes
                .contains(&RankedReasonCode::ReviewedPurpose),
            &true,
            "reviewed purpose reason code",
        )?;
        require_eq(
            &ranked[0].connection_counts,
            &Vec::new(),
            "deterministic no-graph fallback counts",
        )?;
        require_eq(
            &ranked[0].next_call.capability,
            &NavigationNextCapability::Summary,
            "no-graph fallback next call",
        )?;
        if !ranked.iter().any(|node| {
            node.node.node.path == "tests/installer.rs"
                && node
                    .reasons
                    .iter()
                    .any(|reason| reason == "paired source file src/installer.rs")
        }) {
            return Err(io::Error::other("paired test result/reason was missing").into());
        }
        Ok(())
    }

    #[test]
    fn ranked_evidence_keeps_reviewed_purpose_ahead_of_bounded_graph_popularity()
    -> Result<(), Box<dyn Error>> {
        let store = AtlasStore::in_memory()?;
        let mut popular = test_indexed_node("src/popular.rs", "popular-hash");
        popular.purpose = Purpose {
            path: popular.node.path.clone(),
            purpose: Some("generated auth suggestion".to_string()),
            source: PurposeSource::Generated,
            status: PurposeStatus::Suggested,
        };
        popular.summary = None;
        let counts = [
            RankedConnectionKind::Package,
            RankedConnectionKind::Import,
            RankedConnectionKind::Call,
            RankedConnectionKind::Reference,
            RankedConnectionKind::Test,
            RankedConnectionKind::Route,
            RankedConnectionKind::Config,
        ]
        .into_iter()
        .map(|kind| projectatlas_core::RankedConnectionCount {
            kind,
            count: RANKED_CONNECTION_FAMILY_LIMIT as usize,
            truncated: true,
        })
        .collect::<Vec<_>>();
        let popular_connections = RepositoryNavigationConnections {
            path: popular.node.path.clone(),
            counts,
            connections: vec![projectatlas_core::RankedConnection {
                kind: RankedConnectionKind::Call,
                direction: projectatlas_core::RankedConnectionDirection::Inbound,
                target: RankedConnectionTarget::Local {
                    path: "src/auth.rs".to_string(),
                    symbol: Some("authenticate".to_string()),
                },
            }],
            truncated: true,
        };
        let popular_evidence = ranked_node_evidence(
            &store,
            &popular,
            &["auth".to_string()],
            "",
            &HashSet::new(),
            &popular_connections,
        )?;
        require_eq(
            &popular_evidence.reviewed_purpose,
            &false,
            "generated purpose authority",
        )?;
        if popular_evidence.context_score > 32 {
            return Err(io::Error::other("graph popularity was not saturated").into());
        }

        let mut reviewed = test_indexed_node("src/responsibility.rs", "reviewed-hash");
        reviewed.purpose.purpose = Some("Own auth responsibility".to_string());
        reviewed.summary = None;
        let reviewed_evidence = ranked_node_evidence(
            &store,
            &reviewed,
            &["auth".to_string()],
            "",
            &HashSet::new(),
            &RepositoryNavigationConnections {
                path: reviewed.node.path.clone(),
                counts: Vec::new(),
                connections: Vec::new(),
                truncated: false,
            },
        )?;
        require_eq(
            &reviewed_evidence.reviewed_purpose,
            &true,
            "reviewed purpose tier",
        )?;
        require_eq(
            &ranked_evidence_order(&reviewed_evidence, &popular_evidence),
            &std::cmp::Ordering::Less,
            "reviewed purpose dominance",
        )?;
        Ok(())
    }

    #[test]
    fn ranked_service_preserves_dominant_tiers_across_more_than_one_hundred_weaker_matches()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let mut nodes = vec![
            test_node("needle", "hash-exact"),
            test_node("deep/needle", "hash-name"),
            test_node("reviewed.rs", "hash-reviewed"),
        ];
        nodes
            .extend((0..130).map(|index| test_node(&format!("weak/needle-{index:03}.rs"), "hash")));
        store.replace_scan(&nodes)?;
        store.set_purpose(
            "reviewed.rs",
            "Own needle responsibility",
            PurposeSource::Agent,
        )?;
        for index in 0..130 {
            let path = format!("weak/needle-{index:03}.rs");
            store.set_suggested_purpose(&path, "Generated needle suggestion")?;
            store.set_node_summary(&path, "Observed needle summary")?;
        }

        let ranked = load_ranked_file_nodes_with_reasons(&store, "needle", None, None, 3, false)?;
        require_eq(
            &ranked
                .iter()
                .map(|node| node.node.node.path.as_str())
                .collect::<Vec<_>>(),
            &vec!["needle", "deep/needle", "reviewed.rs"],
            "service exact path basename and reviewed-purpose order",
        )?;
        require_eq(
            &ranked[0]
                .reason_codes
                .contains(&RankedReasonCode::ExactPath),
            &true,
            "exact path reason code",
        )?;
        require_eq(
            &ranked[1]
                .reason_codes
                .contains(&RankedReasonCode::ExactName),
            &true,
            "exact basename reason code",
        )?;
        require_eq(
            &ranked[2]
                .reason_codes
                .contains(&RankedReasonCode::ReviewedPurpose),
            &true,
            "reviewed purpose reason code after adversarial admission",
        )?;
        Ok(())
    }

    #[test]
    fn fuzzy_search_matches_approximate_line_terms() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src"))?;
        fs::write(
            root.join("src").join("main.rs"),
            "fn build_project_atlas() {}\nfn unrelated() {}\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        let nodes = [test_node("src/main.rs", "hash-main")];
        store.replace_scan(&nodes)?;
        index_test_file_texts(&mut store, root, &nodes)?;

        let report =
            search_indexed_files(&store, "bpa", false, true, false, Some("*.rs"), 0, 0, 10)?;
        require_eq(&report.mode, &"fuzzy".to_string(), "search mode")?;
        require_eq(&report.returned, &1, "fuzzy returned rows")?;
        require_eq(
            &report.results[0].text,
            &"fn build_project_atlas() {}".to_string(),
            "fuzzy match text",
        )?;

        let invalid = search_indexed_files(&store, "bpa", true, true, false, None, 0, 0, 10);
        if invalid.is_ok() {
            return Err(io::Error::other("regex+fuzzy search was accepted").into());
        }
        Ok(())
    }

    #[test]
    fn symbol_slice_reports_ambiguity_and_accepts_parent_selector() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src"))?;
        fs::write(
            root.join("src").join("lib.rs"),
            "struct A;\nimpl A {\n    fn run(&self) {\n        a();\n    }\n}\nstruct B;\nimpl B {\n    fn run(&self) {\n        b();\n    }\n}\n",
        )?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[test_node("src/lib.rs", "hash-lib")])?;
        let mut a_run = test_symbol("src/lib.rs", SymbolKind::Method, "run");
        a_run.parent = Some("A".to_string());
        a_run.signature = "fn run(&self) for A".to_string();
        a_run.line_start = 3;
        a_run.line_end = 5;
        let mut b_run = test_symbol("src/lib.rs", SymbolKind::Method, "run");
        b_run.parent = Some("B".to_string());
        b_run.signature = "fn run(&self) for B".to_string();
        b_run.line_start = 9;
        b_run.line_end = 11;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/lib.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![a_run, b_run],
            relations: Vec::new(),
        })?;

        let ambiguous = read_symbol_slice(
            &store,
            Path::new("src/lib.rs"),
            &SymbolSliceSelector {
                name: "run",
                ..SymbolSliceSelector::default()
            },
        );
        if !matches!(ambiguous, Err(ServiceError::InvalidInput(message)) if message.contains("ambiguous") && message.contains("parent=A") && message.contains("parent=B"))
        {
            return Err(
                io::Error::other("ambiguous symbol slice did not report candidates").into(),
            );
        }

        let slice = read_symbol_slice(
            &store,
            Path::new("src/lib.rs"),
            &SymbolSliceSelector {
                name: "run",
                parent: Some("B"),
                ..SymbolSliceSelector::default()
            },
        )?;
        if !slice.content.contains("b();") || slice.content.contains("a();") {
            return Err(io::Error::other("parent selector returned wrong symbol slice").into());
        }
        let signature_slice = read_symbol_slice(
            &store,
            Path::new("src/lib.rs"),
            &SymbolSliceSelector {
                name: "run",
                signature: Some("fn run(&self) for A"),
                ..SymbolSliceSelector::default()
            },
        )?;
        if !signature_slice.content.contains("a();") || signature_slice.content.contains("b();") {
            return Err(io::Error::other("signature selector returned wrong symbol slice").into());
        }
        Ok(())
    }

    #[test]
    fn code_slice_budget_preserves_verbatim_utf8_and_rejects_oversized_output()
    -> Result<(), Box<dyn Error>> {
        let source = "fn café() {\r\n    println!(\"λ\");\r\n}\r\n";
        let budget = CodeSliceBudget::new(512)?;
        let slice = read_code_slice(source, "src/lib.rs", 1, Some(3), budget)?;
        require_eq(
            &slice.slice().content,
            &source
                .strip_suffix("\r\n")
                .ok_or_else(|| io::Error::other("CRLF fixture terminator missing"))?
                .to_string(),
            "verbatim UTF-8 slice",
        )?;
        let encoded = slice.fit_output::<_, ServiceError, _>(|slice| {
            serde_json::to_vec(slice).map_err(ServiceError::from)
        })?;
        if encoded.len() > budget.output_bytes() as usize {
            return Err(io::Error::other(
                "accepted slice exceeded its exact encoded-output ceiling",
            )
            .into());
        }
        let payload: serde_json::Value = serde_json::from_slice(&encoded)?;
        if payload.get("output_budget").is_some() {
            return Err(io::Error::other(
                "additive slice budget changed the compatibility payload",
            )
            .into());
        }

        let content_error =
            read_code_slice(source, "src/lib.rs", 1, Some(3), CodeSliceBudget::new(8)?);
        if !matches!(
            content_error,
            Err(ServiceError::InvalidInput(message))
                if message.contains("verbatim slice content exceeds")
        ) {
            return Err(io::Error::other(
                "oversized verbatim slice content was allocated or truncated",
            )
            .into());
        }

        let envelope_budget = CodeSliceBudget::new(64)?;
        let envelope = read_code_slice("λ", "src/lib.rs", 1, Some(1), envelope_budget)?;
        let envelope_error = envelope.fit_output::<_, ServiceError, _>(|slice| {
            serde_json::to_vec(slice).map_err(ServiceError::from)
        });
        if !matches!(
            envelope_error,
            Err(ServiceError::InvalidInput(message))
                if message.contains("slice output exceeds")
        ) {
            return Err(io::Error::other("oversized encoded slice envelope was accepted").into());
        }
        if CodeSliceBudget::new(0).is_ok()
            || CodeSliceBudget::new(GraphLimits::MAX_OUTPUT_BYTES + 1).is_ok()
        {
            return Err(io::Error::other("invalid slice output ceilings were accepted").into());
        }
        Ok(())
    }

    #[test]
    fn line_slice_reads_current_disk_content_after_index_validation() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src"))?;
        let file = root.join("src").join("lib.rs");
        fs::write(&file, "pub fn old_name() {}\n")?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(root)?;
        store.replace_scan(&[test_node("src/lib.rs", "old-hash")])?;

        fs::write(&file, "pub fn current_name() {}\n")?;
        let slice = read_indexed_code_slice(&store, Path::new("src/lib.rs"), 1, Some(1))?;

        require_eq(
            &slice.content,
            &"pub fn current_name() {}".to_string(),
            "slice content",
        )?;
        Ok(())
    }

    /// Build a representative file node.
    fn test_node(path: &str, hash: &str) -> Node {
        test_node_with_language(path, hash, ".rs", "rust")
    }

    /// Build a representative file node with an explicit registry language.
    fn test_node_with_language(path: &str, hash: &str, extension: &str, language: &str) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::File,
            parent_path: normalized_parent(path),
            extension: Some(extension.to_string()),
            language: Some(language.to_string()),
            size_bytes: Some(12),
            mtime_ns: Some(10),
            content_hash: Some(hash.to_string()),
        }
    }

    /// Build a representative indexed file node.
    fn test_indexed_node(path: &str, hash: &str) -> IndexedNode {
        IndexedNode {
            node: test_node(path, hash),
            purpose: Purpose {
                path: path.to_string(),
                purpose: Some(format!("Purpose for {path}")),
                source: PurposeSource::Agent,
                status: PurposeStatus::Approved,
            },
            summary: Some(format!("Summary for {path}")),
        }
    }

    /// Persist fixture text rows for search service tests.
    fn index_test_file_texts(
        store: &mut AtlasStore,
        root: &Path,
        nodes: &[Node],
    ) -> Result<(), Box<dyn Error>> {
        let mut paths = Vec::new();
        let mut texts = Vec::new();
        for node in nodes {
            paths.push(node.path.clone());
            let native = root.join(repo_path_to_native(&node.path));
            let content = fs::read_to_string(native)?;
            texts.push(IndexedFileText {
                path: node.path.clone(),
                content_hash: node.content_hash.clone(),
                byte_count: content.len(),
                line_count: content.lines().count(),
                content,
            });
        }
        store.replace_file_texts_for_paths(&paths, &texts)?;
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
            source_selector: None,
            parent: None,
            parser: ParserKind::TreeSitter,
            detail: None,
        }
    }

    fn assert_single_called_by(
        report: &FileSummaryReport,
        symbol_name: &str,
        caller: &str,
    ) -> Result<(), Box<dyn Error>> {
        let symbol = report
            .functions
            .iter()
            .find(|symbol| symbol.name == symbol_name)
            .ok_or_else(|| io::Error::other(format!("{symbol_name} summary missing")))?;
        require_eq(
            &symbol.called_by,
            &vec![caller.to_string()],
            "import alias called-by",
        )
    }

    /// Return one ordinary test error instead of panicking inside fallible tests.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }

    /// Require two test values to be equal without panicking.
    fn require_eq<T>(actual: &T, expected: &T, label: &str) -> Result<(), Box<dyn Error>>
    where
        T: std::fmt::Debug + PartialEq,
    {
        if actual == expected {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "{label} mismatch: expected {expected:?}, got {actual:?}"
            ))
            .into())
        }
    }

    /// Require a ranked reason to contain a stable phrase.
    fn require_reason(reasons: &[String], expected: &str) -> Result<(), Box<dyn Error>> {
        if reasons.iter().any(|reason| reason.contains(expected)) {
            Ok(())
        } else {
            Err(io::Error::other(format!("reason {expected:?} missing from {reasons:?}")).into())
        }
    }
}
