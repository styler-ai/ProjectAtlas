//! Bounded detailed relation navigation over normalized repository graph storage.

use super::{ServiceError, ServiceResult, selected_project_binding};
use projectatlas_core::graph::{
    ConfidenceClass, CoverageRecord, CoverageScope, DocumentTargetUnresolvedReason, EntitySelector,
    ExtendedRelationKind, GraphEntity, GraphEntityKey, GraphLimitKind, GraphLimits,
    GraphRelationKind, LogicalRelation, RelationOccurrence, RelationResolution, RepositoryFilePath,
    RepositoryNodePath, SymbolSelector,
};
use projectatlas_core::language::{ContentClassification, ContentSelection};
use projectatlas_core::symbols::SymbolKind;
use projectatlas_core::{
    IndexCancellation, IndexGeneration, IndexWorkControl, IndexWorkFailure, IndexWorkStage,
    Purpose, PurposeSource, PurposeStatus,
};
use projectatlas_db::{
    AtlasStore, DbError, MAX_FILE_CONTENT_CLASSIFICATION_PATHS, MAX_REPOSITORY_GRAPH_FRONTIER,
    RepositoryGraphAdjacencyContinuation, RepositoryGraphDirection, RepositoryGraphReadBudget,
    RepositoryGraphReadWork, RepositoryGraphRelationRow,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{self, Write};
use std::time::{Duration, Instant};

/// Stable maximum rows materialized by the generated adjacency statement.
const ADJACENCY_WORK_ROWS: usize = GraphLimits::MAX_ROWS as usize + 1;

/// Maximum encoded relation cursor accepted at the service boundary.
const DETAILED_RELATION_CURSOR_MAX_BYTES: usize = 4 * 1_024 * 1_024;

/// Version of the concrete bounded-frontier cursor contract.
const DETAILED_RELATION_CURSOR_VERSION: u16 = 1;

/// Domain separator for selected-root cursor identity.
const DETAILED_RELATION_ROOT_DOMAIN: &str = "projectatlas:detailed-relation-root:v1";

/// Adapter-facing inverse label for an inbound canonical document relation.
const DOCUMENTED_BY_INBOUND_VIEW: &str = "documented_by";

/// Direction of one detailed relation request from its selected anchor.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationDirection {
    /// Follow relations whose source is the active frontier.
    Outbound,
    /// Follow relations whose retained target is the active frontier.
    Inbound,
}

impl From<RelationDirection> for RepositoryGraphDirection {
    fn from(value: RelationDirection) -> Self {
        match value {
            RelationDirection::Outbound => Self::Outbound,
            RelationDirection::Inbound => Self::Inbound,
        }
    }
}

/// Exact local starting point for relation navigation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RelationAnchor {
    /// Select one exact indexed file entity.
    File {
        /// Exact normalized repository file.
        file: RepositoryFilePath,
    },
    /// Select one exact declaration, with optional fields used to disambiguate it.
    Symbol {
        /// Owning source file.
        file: RepositoryFilePath,
        /// Exact declaration name.
        name: String,
        /// Optional exact declaration kind.
        symbol_kind: Option<SymbolKind>,
        /// Optional exact parent declaration or namespace.
        parent: Option<String>,
        /// Optional exact normalized signature.
        signature: Option<String>,
    },
}

/// Closed filter for persisted resolution state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationResolutionFilter {
    /// Retain every valid resolution state.
    Any,
    /// Retain exact local targets only.
    Resolved,
    /// Retain ambiguous references only.
    Ambiguous,
    /// Retain unresolved references only.
    Unresolved,
    /// Retain external targets only.
    External,
}

/// Parse one user-facing detailed-relation direction.
///
/// # Errors
///
/// Returns an error for values other than `outbound` or `inbound`.
pub fn parse_relation_direction(value: &str) -> ServiceResult<RelationDirection> {
    match value.trim().to_ascii_lowercase().as_str() {
        "outbound" => Ok(RelationDirection::Outbound),
        "inbound" => Ok(RelationDirection::Inbound),
        _ => Err(ServiceError::InvalidInput(format!(
            "unsupported relation direction {value:?}"
        ))),
    }
}

/// Parse one user-facing detailed-relation confidence floor.
///
/// # Errors
///
/// Returns an error when the confidence class is unknown.
pub fn parse_relation_confidence(value: &str) -> ServiceResult<ConfidenceClass> {
    match value.trim().to_ascii_lowercase().as_str() {
        "exact" => Ok(ConfidenceClass::Exact),
        "high" => Ok(ConfidenceClass::High),
        "medium" => Ok(ConfidenceClass::Medium),
        "low" => Ok(ConfidenceClass::Low),
        _ => Err(ServiceError::InvalidInput(format!(
            "unsupported relation confidence {value:?}"
        ))),
    }
}

/// Parse one user-facing detailed-relation resolution filter.
///
/// # Errors
///
/// Returns an error when the resolution state is unknown.
pub fn parse_relation_resolution(value: &str) -> ServiceResult<RelationResolutionFilter> {
    match value.trim().to_ascii_lowercase().as_str() {
        "any" => Ok(RelationResolutionFilter::Any),
        "resolved" => Ok(RelationResolutionFilter::Resolved),
        "ambiguous" => Ok(RelationResolutionFilter::Ambiguous),
        "unresolved" => Ok(RelationResolutionFilter::Unresolved),
        "external" => Ok(RelationResolutionFilter::External),
        _ => Err(ServiceError::InvalidInput(format!(
            "unsupported relation resolution {value:?}"
        ))),
    }
}

/// Complete typed request for one bounded detailed relation traversal.
#[derive(Clone, Debug)]
pub struct DetailedRelationQuery {
    /// Exact local starting point.
    pub anchor: RelationAnchor,
    /// Direction followed from every active frontier.
    pub direction: RelationDirection,
    /// Optional exact legacy or extended family.
    pub relation: Option<GraphRelationKind>,
    /// Lowest accepted confidence class.
    pub minimum_confidence: ConfidenceClass,
    /// Accepted resolution state.
    pub resolution: RelationResolutionFilter,
    /// Whether exact source occurrences should be retained.
    pub include_occurrences: bool,
    /// Aggregate service and output ceilings.
    pub budget: DetailedRelationBudget,
    /// Opaque generation- and purpose-bound continuation from a prior page.
    pub cursor: Option<String>,
    /// Classified-content restriction for anchors and ordinary traversal frontiers.
    pub content_selection: ContentSelection,
}

/// Aggregate budgets for one detailed relation page and its resumable state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DetailedRelationBudget {
    /// Maximum rows retained in one response page.
    page_rows: u32,
    /// Maximum breadth-first traversal depth.
    depth: u32,
    /// Maximum adjacency rows inspected in one page request.
    edges: u32,
    /// Maximum traversal nodes retained across continuation pages.
    nodes: u32,
    /// Maximum unique visited identities retained across continuation pages.
    visited: u32,
    /// Maximum exact occurrences retained for one relation.
    occurrences_per_relation: u32,
    /// Maximum exact occurrences retained across the page.
    occurrences_total: u32,
    /// Maximum aggregate decoded database, cursor, and peak composition bytes.
    intermediate_bytes: u64,
    /// Maximum service-owned traversal duration in milliseconds.
    deadline_ms: u64,
    /// Maximum bytes emitted by the selected adapter.
    output_bytes: u32,
}

impl DetailedRelationBudget {
    /// Absolute edge-work ceiling for one page.
    pub const MAX_EDGES: u32 = 100_000;
    /// Absolute node and visited-state ceiling for one traversal.
    pub const MAX_NODES: u32 = GraphLimits::MAX_ROWS;
    /// Absolute aggregate occurrence ceiling for one page.
    pub const MAX_OCCURRENCES_TOTAL: u32 = 100_000;
    /// Absolute decoded/cursor/intermediate state ceiling.
    pub const MAX_INTERMEDIATE_BYTES: u64 = 32 * 1_024 * 1_024;
    /// Absolute service-owned elapsed-time ceiling.
    pub const MAX_DEADLINE_MS: u64 = 60_000;

    /// Derive compatibility-preserving aggregate defaults from the legacy four limits.
    #[must_use]
    pub fn from_graph_limits(limits: GraphLimits) -> Self {
        let nodes = Self::MAX_NODES;
        let occurrences_total = limits
            .rows()
            .saturating_mul(limits.occurrences())
            .min(Self::MAX_OCCURRENCES_TOTAL);
        let intermediate_bytes = u64::from(limits.output_bytes())
            .saturating_mul(4)
            .clamp(64 * 1_024, Self::MAX_INTERMEDIATE_BYTES);
        Self {
            page_rows: limits.rows(),
            depth: limits.depth(),
            edges: limits.rows(),
            nodes,
            visited: nodes,
            occurrences_per_relation: limits.occurrences(),
            occurrences_total,
            intermediate_bytes,
            deadline_ms: 10_000,
            output_bytes: limits.output_bytes(),
        }
    }

    /// Apply additive aggregate-work overrides to compatibility defaults.
    ///
    /// # Errors
    ///
    /// Returns an error when any resulting ceiling is zero, inconsistent, or
    /// above its product maximum.
    pub fn with_aggregate_limits(
        mut self,
        edges: Option<u32>,
        nodes: Option<u32>,
        visited: Option<u32>,
        occurrences_total: Option<u32>,
        intermediate_bytes: Option<u64>,
        deadline_ms: Option<u64>,
    ) -> ServiceResult<Self> {
        self.edges = edges.unwrap_or(self.edges);
        self.nodes = nodes.unwrap_or(self.nodes);
        self.visited = visited.unwrap_or(self.visited);
        self.occurrences_total = occurrences_total.unwrap_or(self.occurrences_total);
        self.intermediate_bytes = intermediate_bytes.unwrap_or(self.intermediate_bytes);
        self.deadline_ms = deadline_ms.unwrap_or(self.deadline_ms);
        self.validate()
    }

    /// Validate every result-defining budget before traversal or cursor use.
    fn validate(self) -> ServiceResult<Self> {
        let valid = self.page_rows > 0
            && self.page_rows <= GraphLimits::MAX_ROWS
            && self.depth > 0
            && self.depth <= GraphLimits::MAX_DEPTH
            && self.edges > 0
            && self.edges <= Self::MAX_EDGES
            && self.nodes > 0
            && self.nodes <= Self::MAX_NODES
            && self.visited > 0
            && self.visited <= Self::MAX_NODES
            && self.occurrences_per_relation > 0
            && self.occurrences_per_relation <= GraphLimits::MAX_OCCURRENCES
            && self.occurrences_total > 0
            && self.occurrences_total <= Self::MAX_OCCURRENCES_TOTAL
            && self.intermediate_bytes >= 64 * 1_024
            && self.intermediate_bytes <= Self::MAX_INTERMEDIATE_BYTES
            && self.deadline_ms > 0
            && self.deadline_ms <= Self::MAX_DEADLINE_MS
            && self.output_bytes > 0
            && self.output_bytes <= GraphLimits::MAX_OUTPUT_BYTES;
        if !valid {
            return Err(ServiceError::InvalidInput(
                "detailed relation budget is zero, internally inconsistent, or above a product ceiling"
                    .to_string(),
            ));
        }
        Ok(self)
    }

    /// Maximum rows returned by this page.
    #[must_use]
    pub const fn page_rows(self) -> u32 {
        self.page_rows
    }

    /// Maximum traversal depth.
    #[must_use]
    pub const fn depth(self) -> u32 {
        self.depth
    }

    /// Maximum adjacency rows inspected by this page.
    #[must_use]
    pub const fn edges(self) -> u32 {
        self.edges
    }

    /// Maximum unique traversal nodes.
    #[must_use]
    pub const fn nodes(self) -> u32 {
        self.nodes
    }

    /// Maximum unique visited nodes.
    #[must_use]
    pub const fn visited(self) -> u32 {
        self.visited
    }

    /// Maximum retained occurrences for one relation.
    #[must_use]
    pub const fn occurrences_per_relation(self) -> u32 {
        self.occurrences_per_relation
    }

    /// Maximum occurrences retained across this page.
    #[must_use]
    pub const fn occurrences_total(self) -> u32 {
        self.occurrences_total
    }

    /// Maximum cursor and intermediate decoded bytes.
    #[must_use]
    pub const fn intermediate_bytes(self) -> u64 {
        self.intermediate_bytes
    }

    /// Service-owned elapsed-time budget.
    #[must_use]
    pub const fn deadline_ms(self) -> u64 {
        self.deadline_ms
    }

    /// Maximum rendered adapter output bytes.
    #[must_use]
    pub const fn output_bytes(self) -> u32 {
        self.output_bytes
    }
}

/// Purpose state projected from the authoritative indexed owner at query time.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RelationPurpose {
    /// Accepted agent-authored purpose owned by this exact local path.
    Approved {
        /// Owning file or folder path.
        path: String,
        /// Accepted responsibility text.
        purpose: String,
        /// Persisted authorship source.
        source: PurposeSource,
        /// Persisted approval state.
        status: PurposeStatus,
    },
    /// A local owner exists, but no authoritative accepted purpose is available.
    Unavailable {
        /// Owning file or folder path when the unavailable node is local.
        path: Option<String>,
    },
    /// External or unresolved identity has no local purpose owner.
    NotApplicable,
}

/// Exact existing call that can consume one resolved local target directly.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "capability", rename_all = "snake_case")]
pub enum RelationNextCall {
    /// Continue folder navigation through the existing file route.
    Files {
        /// Exact folder selector.
        folder: projectatlas_core::graph::RepositoryNodePath,
        /// Explicit classified-content restriction retained across navigation.
        #[serde(skip_serializing_if = "Option::is_none")]
        content_selection: Option<ContentSelection>,
    },
    /// Inspect a file or package owner through the existing summary route.
    Summary {
        /// Exact file selector.
        file: RepositoryFilePath,
        /// Explicit classified-content restriction retained across navigation.
        #[serde(skip_serializing_if = "Option::is_none")]
        content_selection: Option<ContentSelection>,
    },
    /// Inspect one declaration through the existing exact slice route.
    SymbolSlice {
        /// Stable declaration selector.
        symbol: SymbolSelector,
        /// Explicit classified-content restriction retained across navigation.
        #[serde(skip_serializing_if = "Option::is_none")]
        content_selection: Option<ContentSelection>,
    },
}

/// One endpoint with authoritative purpose state projected at response time.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DetailedRelationNode {
    /// Typed generation-bound graph entity.
    pub entity: GraphEntity,
    /// Persisted role of the local file endpoint, when the entity has one.
    pub classification: Option<ContentClassification>,
    /// Explicit selection captured for exact follow-up calls, or legacy omission.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_selection: Option<ContentSelection>,
    /// Accepted owner purpose, unavailable local purpose, or non-local state.
    pub purpose: RelationPurpose,
    /// Authoritative graph coverage for this node's local owning path.
    pub coverage: Vec<CoverageRecord>,
}

/// One ranked node-simple step in a detailed relation response.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DetailedRelationRow {
    /// One-based traversal depth from the selected anchor.
    pub depth: u32,
    /// Direction relative to the selected frontier.
    pub direction: RelationDirection,
    /// Fully reconstructed normalized relation.
    pub relation: LogicalRelation,
    /// Closed reason retained only for an unresolved canonical document relation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_unresolved_reason: Option<DocumentTargetUnresolvedReason>,
    /// Read-only inverse label for an inbound document relation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inbound_view: Option<&'static str>,
    /// Exact relation source with purpose projection.
    pub source: DetailedRelationNode,
    /// Retained resolved or external target with purpose projection.
    pub target: Option<DetailedRelationNode>,
    /// Purpose disposition for the target, including unresolved identities.
    pub target_purpose: RelationPurpose,
    /// Stable node-simple path from the anchor through this step.
    pub path: Vec<DetailedRelationNode>,
    /// Exact supporting source occurrences when requested.
    pub occurrences: Vec<RelationOccurrence>,
    /// Whether the per-relation occurrence ceiling omitted additional rows.
    pub occurrences_truncated: bool,
    /// Existing call that accepts the exact local target selector.
    pub next_call: Option<RelationNextCall>,
}

/// Typed cardinality knowledge for one bounded relation page.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "state", content = "value", rename_all = "snake_case")]
pub enum RelationTotalState {
    /// The traversal is exhausted and this total is complete.
    Exact(u64),
    /// At least this many rows have been emitted or proved pending.
    AtLeast(u64),
    /// No useful cardinality lower bound is available yet.
    Unknown,
}

/// Exact aggregate work retained for one detailed relation page.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct DetailedRelationWork {
    /// Rows returned by this page.
    pub returned_rows: u32,
    /// Adjacency rows inspected by this page.
    pub inspected_edges: u32,
    /// Unique traversal nodes retained in the cursor state.
    pub active_nodes: u32,
    /// Unique visited nodes retained in the cursor state.
    pub visited_nodes: u32,
    /// Exact occurrences retained by this page.
    pub retained_occurrences: u32,
    /// Compact keys supplied to bounded database batches.
    pub database_requested_rows: u32,
    /// Fully reconstructed rows returned by bounded database batches.
    pub database_returned_rows: u32,
    /// Exact raw `SQLite` payload bytes decoded by bounded database batches.
    pub database_decoded_bytes: u64,
    /// Aggregate batch-local unique graph endpoints reconstructed by database reads.
    pub hydrated_entities: u32,
    /// Aggregate batch-local unique purpose-owner paths hydrated by database reads.
    pub hydrated_purpose_paths: u32,
    /// Unique local file paths hydrated through classification batch reads.
    pub hydrated_classification_paths: u32,
    /// Exact serialized bytes retained by one composed anchor and row set.
    pub retained_composition_bytes: u64,
    /// Aggregate decoded database, encoded cursor, and peak composition bytes.
    pub intermediate_bytes: u64,
    /// Exact adapter bytes, filled by the selected output renderer.
    pub rendered_output_bytes: u64,
}

/// Exact cumulative database work owned by one detailed relation request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RelationDatabaseWork {
    /// Compact keys supplied to completed database batches.
    requested_rows: u32,
    /// Fully reconstructed rows returned by completed database batches.
    returned_rows: u32,
    /// Raw `SQLite` payload bytes decoded by completed database batches.
    decoded_bytes: u64,
    /// Aggregate batch-local unique graph endpoints reconstructed by completed reads.
    hydrated_entities: u32,
    /// Aggregate batch-local unique purpose-owner paths hydrated by completed reads.
    hydrated_paths: u32,
}

impl RelationDatabaseWork {
    /// Add one successful all-or-error database batch to the aggregate ledger.
    fn record(&mut self, work: RepositoryGraphReadWork) -> ServiceResult<()> {
        self.requested_rows = self
            .requested_rows
            .checked_add(work.requested_rows)
            .ok_or_else(relation_work_overflow)?;
        self.returned_rows = self
            .returned_rows
            .checked_add(work.returned_rows)
            .ok_or_else(relation_work_overflow)?;
        self.decoded_bytes = self
            .decoded_bytes
            .checked_add(work.decoded_bytes)
            .ok_or_else(relation_work_overflow)?;
        self.hydrated_entities = self
            .hydrated_entities
            .checked_add(work.hydrated_entities)
            .ok_or_else(relation_work_overflow)?;
        self.hydrated_paths = self
            .hydrated_paths
            .checked_add(work.hydrated_paths)
            .ok_or_else(relation_work_overflow)?;
        Ok(())
    }
}

/// Bounded detailed relation traversal returned to CLI and MCP adapters.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DetailedRelationReport {
    /// Exact selected local anchor.
    pub anchor: DetailedRelationNode,
    /// Complete graph generation captured by this page.
    pub generation: IndexGeneration,
    /// Accepted authored-purpose revision captured by this page.
    pub authored_purpose_revision: u64,
    /// Direction followed from the anchor.
    pub direction: RelationDirection,
    /// Explicit classified-content restriction, or `None` for legacy behavior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_selection: Option<ContentSelection>,
    /// Number of retained relation steps.
    pub returned: u32,
    /// Number of cyclic or lower-ranked duplicate-node paths pruned.
    pub pruned_paths: u64,
    /// Whether any declared result boundary stopped the traversal.
    pub truncated: bool,
    /// Generation-, purpose-, query-, order-, and budget-bound continuation.
    pub continuation: Option<String>,
    /// Exact, lower-bound, or unknown traversal cardinality.
    pub total: RelationTotalState,
    /// Stable unique hard limits reached while constructing the response.
    pub reached_limits: Vec<GraphLimitKind>,
    /// Aggregate page and retained-state work.
    pub work: DetailedRelationWork,
    /// Ranked node-simple relation steps.
    pub rows: Vec<DetailedRelationRow>,
}

/// Exact typed external identity eligible for call-scoped rendezvous.
pub(super) type ExternalRelationIdentity = (String, String, String);

/// Cursor algorithm responsible for traversal-state interpretation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DetailedRelationAlgorithm {
    /// Bounded breadth-first Rust frontier over indexed `SQLite` adjacency.
    BoundedFrontierV1,
}

/// Stable service-owned order applied within every bounded adjacency batch.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DetailedRelationOrdering {
    /// Breadth first, then confidence, resolution, canonical identity, and digest.
    BreadthFirstRankedBatchV1,
}

/// Normalized result-defining query copied into one cursor binding.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DetailedRelationCursorQuery {
    /// Exact normalized file or symbol anchor.
    anchor: RelationAnchor,
    /// Direction followed from the anchor.
    direction: RelationDirection,
    /// Optional exact relation-family filter.
    relation: Option<GraphRelationKind>,
    /// Lowest retained confidence class.
    minimum_confidence: ConfidenceClass,
    /// Retained relation resolution states.
    resolution: RelationResolutionFilter,
    /// Whether exact source occurrences are included.
    include_occurrences: bool,
    /// Classified-content restriction that changes anchor and frontier admission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content_selection: Option<ContentSelection>,
}

impl From<&DetailedRelationQuery> for DetailedRelationCursorQuery {
    fn from(query: &DetailedRelationQuery) -> Self {
        Self {
            anchor: query.anchor.clone(),
            direction: query.direction,
            relation: query.relation,
            minimum_confidence: query.minimum_confidence,
            resolution: query.resolution,
            include_occurrences: query.include_occurrences,
            content_selection: query
                .content_selection
                .explicit_value()
                .map(|_| query.content_selection),
        }
    }
}

/// State whose equality determines whether a cursor may resume this request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DetailedRelationCursorBinding {
    /// Durable selected-project identity.
    project: projectatlas_core::graph::ProjectInstanceId,
    /// Domain-separated normalized project-root digest.
    root_digest: [u8; 32],
    /// Complete graph generation captured by the page.
    generation: IndexGeneration,
    /// Accepted authored-purpose revision captured by the page.
    authored_purpose_revision: u64,
    /// State-machine algorithm and version owner.
    capability: DetailedRelationAlgorithm,
    /// Normalized result-defining request.
    query: DetailedRelationCursorQuery,
    /// Deterministic traversal and ranking order.
    ordering: DetailedRelationOrdering,
    /// Result-defining aggregate budget.
    budget: DetailedRelationBudget,
}

/// Compact node-simple traversal node whose parent reconstructs its exact path.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct TraversalNodeState {
    /// Stable entity-key digest.
    digest: [u8; 32],
    /// Earlier node index that proves the node-simple path.
    parent: Option<u32>,
}

/// One ranked relation already proved by a bounded adjacency batch.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PendingRelationState {
    /// Stable logical relation-key digest.
    relation_digest: [u8; 32],
    /// One-based breadth-first depth for the relation step.
    depth: u32,
    /// Terminal traversal-node index for path reconstruction.
    path_terminal: u32,
}

/// Resumable bounded-frontier state; no hydrated row or authored text is duplicated.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RelationTraversalState {
    /// Current one-based breadth-first depth.
    depth: u32,
    /// Compact unique entity identities in parent-before-child order.
    nodes: Vec<TraversalNodeState>,
    /// Current-depth node indices awaiting adjacency expansion.
    frontier: Vec<u32>,
    /// First current-frontier index not fully expanded.
    frontier_index: u32,
    /// Next-depth node indices discovered from the current frontier.
    next_frontier: Vec<u32>,
    /// Opaque database keyset inside the active frontier chunk.
    adjacency: Option<RepositoryGraphAdjacencyContinuation>,
    /// One fixed ranked adjacency batch already proved by storage.
    pending: Vec<PendingRelationState>,
    /// First pending relation not yet emitted.
    pending_index: u32,
    /// Rows emitted by all completed continuation pages.
    emitted_rows: u64,
    /// Cyclic or lower-ranked duplicate-node paths pruned so far.
    pruned_paths: u64,
}

/// Versioned opaque cursor serialized as bounded compact JSON.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DetailedRelationCursor {
    /// Outer cursor wire version.
    version: u16,
    /// Snapshot, query, ordering, and budget identity.
    binding: DetailedRelationCursorBinding,
    /// Compact resumable traversal state.
    state: RelationTraversalState,
}

/// Fully hydrated candidate page whose row prefix can be fit to an exact adapter envelope.
pub struct DetailedRelationPageDraft {
    /// Fully hydrated maximum candidate report.
    report: DetailedRelationReport,
    /// Rows emitted before this request began.
    old_emitted: u64,
    /// Cursor binding reused by exact output-prefix fitting.
    binding: DetailedRelationCursorBinding,
    /// Traversal checkpoint before the fixed candidate batch begins.
    prefix_state: Option<RelationTraversalState>,
    /// Exact result-defining request budget.
    budget: DetailedRelationBudget,
    /// Absolute service-owned deadline retained through final adapter rendering.
    deadline: Instant,
}

impl DetailedRelationPageDraft {
    /// Number of already-hydrated rows available to the adapter.
    #[must_use]
    pub fn candidate_rows(&self) -> usize {
        self.report.rows.len()
    }

    /// Exact requested encoded-output ceiling.
    #[must_use]
    pub const fn maximum_output_bytes(&self) -> usize {
        self.budget.output_bytes() as usize
    }

    /// Build a complete report for one prefix without losing the omitted pending rows.
    ///
    /// # Errors
    ///
    /// Returns an error when the prefix is invalid or its continuation cannot
    /// fit the declared intermediate-state ceiling.
    pub fn report_for_prefix(&self, selected_rows: usize) -> ServiceResult<DetailedRelationReport> {
        if selected_rows > self.report.rows.len() {
            return Err(ServiceError::InvalidInput(
                "detailed relation output prefix exceeds the candidate page".to_string(),
            ));
        }
        let full_rows = self.report.rows.len();
        let mut report = self.report.clone();
        if selected_rows < full_rows {
            let mut state =
                self.prefix_state
                    .clone()
                    .ok_or(ServiceError::RelationCursorInvalid {
                        reason: "output prefix has no matching traversal checkpoint",
                    })?;
            state.pending_index = state
                .pending_index
                .checked_add(u32::try_from(selected_rows).map_err(|_overflow| {
                    ServiceError::InvalidInput(
                        "detailed relation output prefix index overflowed".to_string(),
                    )
                })?)
                .ok_or_else(|| {
                    ServiceError::InvalidInput(
                        "detailed relation output prefix index overflowed".to_string(),
                    )
                })?;
            state.emitted_rows = self.old_emitted.saturating_add(selected_rows as u64);
            report.continuation = Some(encode_relation_cursor(&self.binding, &state, self.budget)?);
            report.pruned_paths = state.pruned_paths;
            push_limit(&mut report.reached_limits, GraphLimitKind::OutputBytes);
            let pending = state
                .pending
                .len()
                .saturating_sub(state.pending_index as usize) as u64;
            report.total = RelationTotalState::AtLeast(state.emitted_rows.saturating_add(pending));
            report.work.active_nodes = u32::try_from(state.nodes.len()).unwrap_or(u32::MAX);
            report.work.visited_nodes = report.work.active_nodes;
        }
        report.rows.truncate(selected_rows);
        report.returned = u32::try_from(selected_rows).map_err(|_overflow| {
            ServiceError::InvalidInput("output row count overflowed".to_string())
        })?;
        report.work.returned_rows = report.returned;
        report.work.retained_occurrences = report
            .rows
            .iter()
            .map(|row| row.occurrences.len() as u32)
            .sum();
        report.work.retained_composition_bytes =
            relation_composition_bytes(&report.anchor, &report.rows)?;
        let prefix_cursor_bytes = report
            .continuation
            .as_ref()
            .map_or(0, |cursor| cursor.len() as u64);
        let prefix_intermediate_bytes = relation_intermediate_bytes(
            report.work.database_decoded_bytes,
            prefix_cursor_bytes,
            0,
            report.work.retained_composition_bytes,
        )?;
        report.work.intermediate_bytes = report
            .work
            .intermediate_bytes
            .max(prefix_intermediate_bytes);
        if report.work.intermediate_bytes > self.budget.intermediate_bytes() {
            return Err(ServiceError::InvalidInput(
                "detailed relation output prefix exceeds the aggregate intermediate-byte budget"
                    .to_string(),
            ));
        }
        report.work.rendered_output_bytes = 0;
        report.truncated = report.continuation.is_some() || !report.reached_limits.is_empty();
        Ok(report)
    }

    /// Fit the largest row prefix to the exact encoded adapter envelope.
    ///
    /// The encoder must return the complete text that the adapter will emit,
    /// including any top-level wrapper or selected-project audit prefix.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when traversal cancellation is observed, a
    /// continuation cannot be encoded, rendering fails, or even the empty
    /// response envelope exceeds the requested output-byte ceiling.
    pub fn fit_output<F, E>(
        &self,
        control: Option<&IndexWorkControl>,
        encode: F,
    ) -> Result<(DetailedRelationReport, String), E>
    where
        F: FnMut(&DetailedRelationReport) -> Result<String, E>,
        E: From<ServiceError>,
    {
        check_relation_deadline(self.deadline).map_err(E::from)?;
        fit_detailed_relation_output(self, control, encode)
    }

    /// Preserve the direct service API by fitting its compact bare JSON envelope.
    fn fit_compact(
        &self,
        control: Option<&IndexWorkControl>,
    ) -> ServiceResult<DetailedRelationReport> {
        self.fit_output(control, |report| {
            serde_json::to_string(report).map_err(ServiceError::from)
        })
        .map(|(report, _encoded)| report)
    }
}

/// Internal relation row before owner-purpose and occurrence projection.
struct TraversalRow {
    /// One-based distance from the selected anchor.
    depth: u32,
    /// Fully hydrated normalized relation row.
    detail: RepositoryGraphRelationRow,
    /// Node-simple entity-key path reaching this row.
    path: Vec<GraphEntity>,
}

/// Count serialized-equivalent bytes without allocating another encoded buffer.
#[derive(Default)]
struct SerializedByteCounter {
    /// Exact bytes accepted by the serializer.
    bytes: u64,
}

/// Return the exact admitted file path that owns an entity classification.
fn classification_path(entity: &GraphEntity) -> Option<String> {
    match entity.selector() {
        EntitySelector::File { path } => Some(path.as_str().to_string()),
        EntitySelector::Package { package } => Some(package.manifest.as_str().to_string()),
        EntitySelector::Symbol { symbol } => Some(symbol.file.as_str().to_string()),
        EntitySelector::Project
        | EntitySelector::Folder { .. }
        | EntitySelector::External { .. } => None,
    }
}

/// Load exact entity classifications through bounded set-oriented DB calls.
fn load_entity_classifications<'entity>(
    store: &AtlasStore,
    entities: impl IntoIterator<Item = &'entity GraphEntity>,
) -> ServiceResult<BTreeMap<String, ContentClassification>> {
    let paths = entities
        .into_iter()
        .filter_map(classification_path)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut classifications = BTreeMap::new();
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

/// Return whether one local file-bearing entity belongs to the selection.
fn entity_matches_selection(
    entity: &GraphEntity,
    classifications: &BTreeMap<String, ContentClassification>,
    selection: ContentSelection,
) -> bool {
    selection == ContentSelection::UnspecifiedLegacy
        || classification_path(entity)
            .and_then(|path| classifications.get(&path).copied())
            .is_some_and(|classification| selection.includes(classification))
}

/// Return whether an explicitly requested document edge may expose one cross-class endpoint.
fn explicit_document_endpoint(query: &DetailedRelationQuery, relation: &LogicalRelation) -> bool {
    query.relation == Some(GraphRelationKind::Extended(ExtendedRelationKind::Documents))
        && relation.kind() == GraphRelationKind::Extended(ExtendedRelationKind::Documents)
}

/// Project the inbound wire label without storing an inverse graph relation.
fn inbound_relation_view(
    direction: RelationDirection,
    relation: &LogicalRelation,
) -> Option<&'static str> {
    (direction == RelationDirection::Inbound
        && relation.kind() == GraphRelationKind::Extended(ExtendedRelationKind::Documents))
    .then_some(DOCUMENTED_BY_INBOUND_VIEW)
}

impl Write for SerializedByteCounter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let bytes = u64::try_from(buffer.len())
            .map_err(|_overflow| io::Error::other("serialized byte count overflowed"))?;
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| io::Error::other("serialized byte count overflowed"))?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Load one detailed relation traversal through a stable store snapshot.
///
/// # Errors
///
/// Returns an error when the exact anchor is absent or ambiguous, graph rows
/// are invalid, cancellation fires, or a bounded database read fails.
pub fn load_detailed_relation_page(
    store: &AtlasStore,
    query: &DetailedRelationQuery,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<DetailedRelationPageDraft> {
    check_relation_control(control)?;
    let budget = query.budget.validate()?;
    let started = Instant::now();
    let deadline = started
        .checked_add(Duration::from_millis(budget.deadline_ms()))
        .unwrap_or(started);
    let request_control = relation_request_control(control, deadline);
    let control = Some(&request_control);
    let binding = selected_project_binding(store)?;
    check_relation_control(control)?;
    let generation = store.repository_graph_generation()?.ok_or_else(|| {
        ServiceError::InvalidInput(
            "repository graph has no complete generation for relation navigation".to_string(),
        )
    })?;
    let anchor_path = match &query.anchor {
        RelationAnchor::File { file } | RelationAnchor::Symbol { file, .. } => file.as_str(),
    };
    let anchor_classification =
        super::selected_file_classification(store, anchor_path, query.content_selection)?;
    let mut database_work = RelationDatabaseWork::default();
    let anchor = resolve_anchor(
        store,
        binding.project_instance_id,
        generation,
        &query.anchor,
        budget,
        &mut database_work,
        control,
    )?;
    let anchor_classifications = BTreeMap::from([(anchor_path.to_string(), anchor_classification)]);
    let mut hydrated_classification_paths = anchor_classifications
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    check_relation_control(control)?;
    let anchor_digest = anchor.key().digest_bytes().map_err(invalid_graph_input)?;
    let authored_purpose_revision = store.authored_purpose_revision()?;
    check_relation_control(control)?;
    let cursor_binding = DetailedRelationCursorBinding {
        project: binding.project_instance_id,
        root_digest: detailed_relation_root_digest(&binding.project_root),
        generation,
        authored_purpose_revision,
        capability: DetailedRelationAlgorithm::BoundedFrontierV1,
        query: DetailedRelationCursorQuery::from(query),
        ordering: DetailedRelationOrdering::BreadthFirstRankedBatchV1,
        budget,
    };
    let mut state = if let Some(encoded) = &query.cursor {
        decode_relation_cursor(encoded, &cursor_binding)?
    } else {
        RelationTraversalState {
            depth: 1,
            nodes: vec![TraversalNodeState {
                digest: anchor_digest,
                parent: None,
            }],
            frontier: vec![0],
            frontier_index: 0,
            next_frontier: Vec::new(),
            adjacency: None,
            pending: Vec::new(),
            pending_index: 0,
            emitted_rows: 0,
            pruned_paths: 0,
        }
    };
    validate_traversal_state(&state, budget, anchor_digest)?;
    let mut entities = hydrate_traversal_entities(
        store,
        binding.project_instance_id,
        generation,
        &state,
        budget,
        encoded_relation_state_bytes(&cursor_binding, &state, budget)?,
        &mut database_work,
        control,
    )?;
    let mut visited = state
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            u32::try_from(index)
                .map(|index| (node.digest, index))
                .map_err(|_overflow| ServiceError::RelationCursorInvalid {
                    reason: "node index exceeds the cursor representation",
                })
        })
        .collect::<ServiceResult<HashMap<_, _>>>()?;
    let mut selected = Vec::new();
    let mut prefix_state = None;
    let mut inspected_edges = 0_u32;
    let mut reached_limits = Vec::new();
    let mut terminal_limit = false;
    let mut exhausted = false;

    while selected.len() < budget.page_rows() as usize {
        if relation_deadline_elapsed(deadline) {
            push_limit(&mut reached_limits, GraphLimitKind::Deadline);
            break;
        }
        check_relation_control(control)?;

        if (state.pending_index as usize) < state.pending.len() {
            prefix_state.get_or_insert_with(|| state.clone());
            selected.push(state.pending[state.pending_index as usize].clone());
            state.pending_index = state.pending_index.saturating_add(1);
            continue;
        }
        if !selected.is_empty() {
            break;
        }
        state.pending.clear();
        state.pending_index = 0;

        if state.frontier_index as usize >= state.frontier.len() {
            if state.next_frontier.is_empty() {
                exhausted = true;
                break;
            }
            if state.depth >= budget.depth() {
                push_limit(&mut reached_limits, GraphLimitKind::Depth);
                terminal_limit = true;
                break;
            }
            state.frontier = std::mem::take(&mut state.next_frontier);
            state.frontier_index = 0;
            state.adjacency = None;
            state.depth = state.depth.saturating_add(1);
        }

        if inspected_edges >= budget.edges() {
            push_limit(&mut reached_limits, GraphLimitKind::Edges);
            break;
        }
        let chunk_start = state.frontier_index as usize;
        let chunk_end = chunk_start
            .saturating_add(MAX_REPOSITORY_GRAPH_FRONTIER)
            .min(state.frontier.len());
        let chunk_nodes = state.frontier[chunk_start..chunk_end].to_vec();
        let frontier = chunk_nodes
            .iter()
            .map(|index| {
                entities
                    .get(*index as usize)
                    .map(|entity| entity.key().clone())
                    .ok_or(ServiceError::RelationCursorInvalid {
                        reason: "frontier node index is absent",
                    })
            })
            .collect::<ServiceResult<Vec<_>>>()?;
        let mut depth_rows = Vec::new();
        loop {
            if relation_deadline_elapsed(deadline) {
                push_limit(&mut reached_limits, GraphLimitKind::Deadline);
                break;
            }
            check_relation_control(control)?;
            let remaining_edges = budget.edges().saturating_sub(inspected_edges);
            if remaining_edges == 0 {
                break;
            }
            let per_page = (ADJACENCY_WORK_ROWS / frontier.len())
                .saturating_sub(1)
                .min(remaining_edges as usize)
                .max(1);
            let page_limit = u32::try_from(per_page).map_err(|_overflow| {
                ServiceError::InvalidInput("graph adjacency page limit overflowed".to_string())
            })?;
            let state_bytes = encoded_relation_state_bytes(&cursor_binding, &state, budget)?;
            let endpoint_limit = page_limit.saturating_add(1).saturating_mul(2);
            let database_budget = relation_database_budget(
                budget,
                database_work,
                state_bytes,
                frontier.len(),
                page_limit,
                endpoint_limit,
                endpoint_limit,
            )?;
            let bounded_page = store
                .repository_graph_adjacency_page_filtered_bounded_with_documents(
                    &frontier,
                    query.direction.into(),
                    query.relation,
                    include_document_relations(query),
                    state.adjacency.as_ref(),
                    page_limit,
                    database_budget,
                    control,
                )?;
            database_work.record(bounded_page.work)?;
            let page = bounded_page.page;
            inspected_edges = inspected_edges
                .checked_add(u32::try_from(page.rows.len()).map_err(|_overflow| {
                    ServiceError::InvalidInput("inspected edge count overflowed".to_string())
                })?)
                .ok_or_else(|| {
                    ServiceError::InvalidInput("inspected edge count overflowed".to_string())
                })?;
            depth_rows.extend(page.rows.into_iter().map(|row| FrontierRow {
                frontier_index: row.frontier_index,
                detail: row.detail,
            }));
            if page.truncated {
                state.adjacency = Some(page.continuation.ok_or_else(|| {
                    ServiceError::InvalidInput(
                        "truncated graph adjacency page omitted its continuation".to_string(),
                    )
                })?);
                if inspected_edges >= budget.edges() {
                    break;
                }
            } else {
                state.adjacency = None;
                state.frontier_index = u32::try_from(chunk_end).map_err(|_overflow| {
                    ServiceError::InvalidInput("frontier index overflowed".to_string())
                })?;
                break;
            }
        }
        depth_rows.retain(|row| relation_matches(&row.detail.relation, query));
        depth_rows.sort_by(|left, right| relation_rank_order(&left.detail, &right.detail));
        let endpoint_classifications = load_entity_classifications(
            store,
            depth_rows
                .iter()
                .filter_map(|row| traversable_entity(&row.detail, query.direction)),
        )?;
        hydrated_classification_paths.extend(endpoint_classifications.keys().cloned());
        let prior_state = state.clone();
        let prior_entity_count = entities.len();
        for row in depth_rows {
            check_relation_control(control)?;
            let local_frontier = row.frontier_index as usize;
            let Some(&parent_index) = chunk_nodes.get(local_frontier) else {
                return Err(ServiceError::InvalidInput(
                    "graph adjacency row selected an invalid frontier index".to_string(),
                ));
            };
            let mut path_terminal = parent_index;
            if let Some(next) = traversable_entity(&row.detail, query.direction) {
                let endpoint_selected = entity_matches_selection(
                    next,
                    &endpoint_classifications,
                    query.content_selection,
                );
                let cross_class_document = explicit_document_endpoint(query, &row.detail.relation);
                if !endpoint_selected && !cross_class_document {
                    continue;
                }
                let digest = next.key().digest_bytes().map_err(invalid_graph_input)?;
                if visited.contains_key(&digest) {
                    state.pruned_paths = state.pruned_paths.saturating_add(1);
                    continue;
                }
                if state.nodes.len() >= budget.nodes() as usize {
                    push_limit(&mut reached_limits, GraphLimitKind::Nodes);
                    terminal_limit = true;
                    break;
                }
                if visited.len() >= budget.visited() as usize {
                    push_limit(&mut reached_limits, GraphLimitKind::Visited);
                    terminal_limit = true;
                    break;
                }
                path_terminal = u32::try_from(state.nodes.len()).map_err(|_overflow| {
                    ServiceError::InvalidInput("traversal node index overflowed".to_string())
                })?;
                state.nodes.push(TraversalNodeState {
                    digest,
                    parent: Some(parent_index),
                });
                if endpoint_selected {
                    state.next_frontier.push(path_terminal);
                }
                visited.insert(digest, path_terminal);
                entities.push(next.clone());
            }
            state.pending.push(PendingRelationState {
                relation_digest: row
                    .detail
                    .relation
                    .key()
                    .digest_bytes()
                    .map_err(invalid_graph_input)?,
                depth: state.depth,
                path_terminal,
            });
        }
        if terminal_limit {
            break;
        }
        let state_bytes = encoded_relation_state_bytes(&cursor_binding, &state, budget)?;
        if state_bytes.saturating_add(database_work.decoded_bytes) > budget.intermediate_bytes() {
            state = prior_state;
            entities.truncate(prior_entity_count);
            visited = state
                .nodes
                .iter()
                .enumerate()
                .map(|(index, node)| (node.digest, index as u32))
                .collect();
            push_limit(&mut reached_limits, GraphLimitKind::IntermediateBytes);
            terminal_limit = true;
            break;
        }
    }

    if inspected_edges >= budget.edges()
        && traversal_has_work(&state, budget.depth())
        && !terminal_limit
        && !exhausted
    {
        push_limit(&mut reached_limits, GraphLimitKind::Edges);
    }
    if selected.len() >= budget.page_rows() as usize && traversal_has_work(&state, budget.depth()) {
        push_limit(&mut reached_limits, GraphLimitKind::Rows);
    }
    let relation_digests = selected
        .iter()
        .map(|pending| pending.relation_digest)
        .collect::<Vec<_>>();
    let mut projected_state = state.clone();
    projected_state.emitted_rows = projected_state
        .emitted_rows
        .saturating_add(selected.len() as u64);
    let retained_cursor_bytes =
        encoded_relation_state_bytes(&cursor_binding, &projected_state, budget)?;
    let mut relation_details = Vec::with_capacity(relation_digests.len());
    for chunk in relation_digests.chunks(MAX_REPOSITORY_GRAPH_FRONTIER) {
        let chunk_rows = u32::try_from(chunk.len()).map_err(|_overflow| {
            ServiceError::InvalidInput("relation hydration batch size overflowed".to_string())
        })?;
        let endpoint_limit = chunk_rows.saturating_mul(2).max(1);
        let database_budget = relation_database_budget(
            budget,
            database_work,
            retained_cursor_bytes,
            chunk.len(),
            chunk_rows,
            endpoint_limit,
            endpoint_limit,
        )?;
        let batch = store.repository_graph_relation_rows_by_digest(
            binding.project_instance_id,
            generation,
            chunk,
            database_budget,
            control,
        )?;
        database_work.record(batch.work)?;
        relation_details.extend(batch.rows);
    }
    let retained = selected
        .iter()
        .cloned()
        .zip(relation_details)
        .map(|(pending, detail)| {
            let path = traversal_path(&state.nodes, &entities, pending.path_terminal)?;
            Ok(TraversalRow {
                depth: pending.depth,
                detail,
                path,
            })
        })
        .collect::<ServiceResult<Vec<_>>>()?;

    let mut classification_entities = vec![&anchor];
    for row in &retained {
        classification_entities.push(&row.detail.source);
        classification_entities.extend(row.detail.target.iter());
        classification_entities.extend(&row.path);
    }
    let classifications = load_entity_classifications(store, classification_entities)?;
    hydrated_classification_paths.extend(classifications.keys().cloned());

    let purposes = load_purposes(
        store,
        binding.project_instance_id,
        generation,
        &anchor,
        &retained,
        budget,
        &mut database_work,
        retained_cursor_bytes,
        control,
    )?;
    let coverage = load_coverage(
        store,
        binding.project_instance_id,
        generation,
        &anchor,
        &retained,
        budget,
        &mut database_work,
        retained_cursor_bytes,
        control,
    )?;
    let anchor_node = detailed_node(
        anchor,
        query.content_selection,
        &classifications,
        &purposes,
        &coverage,
    );
    let (occurrence_pages, retained_occurrences) = load_occurrence_pages(
        store,
        &retained,
        query,
        budget,
        &mut database_work,
        retained_cursor_bytes,
        control,
        &mut reached_limits,
    )?;
    let working_composition_bytes = relation_working_composition_bytes(
        &entities,
        &retained,
        query.direction,
        &purposes,
        &coverage,
        &classifications,
        &occurrence_pages,
    )?;
    let precomposition_bytes = relation_intermediate_bytes(
        database_work.decoded_bytes,
        retained_cursor_bytes,
        working_composition_bytes,
        0,
    )?;
    if precomposition_bytes > budget.intermediate_bytes() {
        return Err(ServiceError::InvalidInput(
            "detailed relation aggregate intermediate-byte budget was exhausted before composition"
                .to_string(),
        ));
    }
    let mut rows = Vec::with_capacity(retained.len());
    for (row, occurrences) in retained.into_iter().zip(occurrence_pages) {
        check_relation_control(control)?;
        rows.push(detailed_row(
            row,
            query,
            &classifications,
            &purposes,
            &coverage,
            occurrences,
        ));
    }
    let retained_composition_bytes = relation_composition_bytes(&anchor_node, &rows)?;

    let old_emitted = state.emitted_rows;
    state.emitted_rows = state.emitted_rows.saturating_add(rows.len() as u64);
    let traversal_remaining = traversal_has_work(&state, budget.depth());
    let has_more = traversal_remaining && !terminal_limit;
    let continuation = has_more
        .then(|| encode_relation_cursor(&cursor_binding, &state, budget))
        .transpose()?;
    let cursor_bytes = continuation
        .as_ref()
        .map_or(serialized_relation_state_bytes(&state)?, |cursor| {
            cursor.len() as u64
        });
    let intermediate_bytes = relation_intermediate_bytes(
        database_work.decoded_bytes,
        cursor_bytes,
        working_composition_bytes,
        retained_composition_bytes,
    )?;
    if intermediate_bytes > budget.intermediate_bytes() {
        return Err(ServiceError::InvalidInput(
            "detailed relation aggregate intermediate-byte budget was exhausted before composition"
                .to_string(),
        ));
    }
    let total = if !traversal_remaining && !terminal_limit {
        RelationTotalState::Exact(state.emitted_rows)
    } else {
        let pending = state
            .pending
            .len()
            .saturating_sub(state.pending_index as usize) as u64;
        let proved = state.emitted_rows.saturating_add(pending);
        if proved > 0 {
            RelationTotalState::AtLeast(proved)
        } else {
            RelationTotalState::Unknown
        }
    };
    let returned = u32::try_from(rows.len()).map_err(|_overflow| {
        ServiceError::InvalidInput("returned relation row count overflowed".to_string())
    })?;
    let report = DetailedRelationReport {
        anchor: anchor_node,
        generation,
        authored_purpose_revision,
        direction: query.direction,
        content_selection: query
            .content_selection
            .explicit_value()
            .map(|_| query.content_selection),
        returned,
        pruned_paths: state.pruned_paths,
        truncated: continuation.is_some() || terminal_limit || !reached_limits.is_empty(),
        continuation,
        total,
        reached_limits,
        work: DetailedRelationWork {
            returned_rows: returned,
            inspected_edges,
            active_nodes: u32::try_from(state.nodes.len()).unwrap_or(u32::MAX),
            visited_nodes: u32::try_from(visited.len()).unwrap_or(u32::MAX),
            retained_occurrences,
            database_requested_rows: database_work.requested_rows,
            database_returned_rows: database_work.returned_rows,
            database_decoded_bytes: database_work.decoded_bytes,
            hydrated_entities: database_work.hydrated_entities,
            hydrated_purpose_paths: database_work.hydrated_paths,
            hydrated_classification_paths: u32::try_from(hydrated_classification_paths.len())
                .unwrap_or(u32::MAX),
            retained_composition_bytes,
            intermediate_bytes,
            rendered_output_bytes: 0,
        },
        rows,
    };
    Ok(DetailedRelationPageDraft {
        report,
        old_emitted,
        binding: cursor_binding,
        prefix_state,
        budget,
        deadline,
    })
}

/// Load and compact-fit one detailed relation report for direct library callers.
///
/// Adapters that add wrappers or audit prefixes should use
/// [`load_detailed_relation_page`] and fit the exact final envelope.
///
/// # Errors
///
/// Returns the same traversal, cursor, database, cancellation, and output
/// errors as the draft path.
pub fn load_detailed_relations(
    store: &AtlasStore,
    query: &DetailedRelationQuery,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<DetailedRelationReport> {
    load_detailed_relation_page(store, query, control)?.fit_compact(control)
}

/// One adjacency row retaining the selecting frontier position.
struct FrontierRow {
    /// Position of the selecting key in the current frontier.
    frontier_index: u32,
    /// Fully hydrated normalized relation row.
    detail: RepositoryGraphRelationRow,
}

/// Hash one normalized selected root without exposing machine-local paths in cursors.
fn detailed_relation_root_digest(root: &str) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DETAILED_RELATION_ROOT_DOMAIN.as_bytes());
    hasher.update(&[0]);
    hasher.update(root.as_bytes());
    *hasher.finalize().as_bytes()
}

/// Decode and classify one bounded cursor against the current request snapshot.
fn decode_relation_cursor(
    encoded: &str,
    expected: &DetailedRelationCursorBinding,
) -> ServiceResult<RelationTraversalState> {
    if encoded.is_empty() || encoded.len() > DETAILED_RELATION_CURSOR_MAX_BYTES {
        return Err(ServiceError::RelationCursorInvalid {
            reason: "cursor length is empty or above the product ceiling",
        });
    }
    let cursor: DetailedRelationCursor =
        serde_json::from_str(encoded).map_err(|_source| ServiceError::RelationCursorInvalid {
            reason: "cursor JSON is malformed or contains unknown fields",
        })?;
    if cursor.version != DETAILED_RELATION_CURSOR_VERSION
        || cursor.binding.capability != expected.capability
    {
        return Err(ServiceError::RelationCursorStale {
            field: "algorithm version",
        });
    }
    for (changed, field) in [
        (
            cursor.binding.project != expected.project,
            "project identity",
        ),
        (
            cursor.binding.root_digest != expected.root_digest,
            "project root",
        ),
        (
            cursor.binding.generation != expected.generation,
            "graph generation",
        ),
        (
            cursor.binding.authored_purpose_revision != expected.authored_purpose_revision,
            "authored-purpose revision",
        ),
    ] {
        if changed {
            return Err(ServiceError::RelationCursorStale { field });
        }
    }
    if cursor.binding.query != expected.query {
        return Err(ServiceError::RelationCursorMismatched { field: "query" });
    }
    if cursor.binding.ordering != expected.ordering {
        return Err(ServiceError::RelationCursorMismatched { field: "ordering" });
    }
    if cursor.binding.budget != expected.budget {
        return Err(ServiceError::RelationCursorMismatched { field: "budget" });
    }
    Ok(cursor.state)
}

/// Encode one service-owned traversal state after enforcing both cursor ceilings.
fn encode_relation_cursor(
    binding: &DetailedRelationCursorBinding,
    state: &RelationTraversalState,
    budget: DetailedRelationBudget,
) -> ServiceResult<String> {
    let encoded = serde_json::to_string(&DetailedRelationCursor {
        version: DETAILED_RELATION_CURSOR_VERSION,
        binding: binding.clone(),
        state: state.clone(),
    })?;
    if encoded.len() > DETAILED_RELATION_CURSOR_MAX_BYTES
        || encoded.len() > budget.intermediate_bytes() as usize
    {
        return Err(ServiceError::RelationCursorInvalid {
            reason: "encoded cursor exceeds the intermediate-state ceiling",
        });
    }
    Ok(encoded)
}

/// Validate every compact cursor index and node-simple invariant before database use.
fn validate_traversal_state(
    state: &RelationTraversalState,
    budget: DetailedRelationBudget,
    anchor: [u8; 32],
) -> ServiceResult<()> {
    let serialized = serde_json::to_vec(state)?;
    let invalid_shape = state.depth == 0
        || state.depth > budget.depth()
        || state.nodes.is_empty()
        || state.nodes.len() > budget.nodes() as usize
        || state.nodes.len() > budget.visited() as usize
        || state.nodes[0].digest != anchor
        || state.nodes[0].parent.is_some()
        || state.frontier.is_empty()
        || state.frontier_index as usize > state.frontier.len()
        || state.pending_index as usize > state.pending.len()
        || serialized.len() > budget.intermediate_bytes() as usize;
    if invalid_shape {
        return Err(ServiceError::RelationCursorInvalid {
            reason: "cursor state exceeds its budget or has an invalid root/index shape",
        });
    }
    let mut unique = HashSet::with_capacity(state.nodes.len());
    for (index, node) in state.nodes.iter().enumerate() {
        if !unique.insert(node.digest)
            || (index > 0 && node.parent.is_none())
            || node.parent.is_some_and(|parent| parent as usize >= index)
        {
            return Err(ServiceError::RelationCursorInvalid {
                reason: "cursor nodes are duplicate, cyclic, or not parent ordered",
            });
        }
    }
    if state
        .frontier
        .iter()
        .chain(&state.next_frontier)
        .any(|index| *index as usize >= state.nodes.len())
        || state.pending.iter().any(|pending| {
            pending.path_terminal as usize >= state.nodes.len()
                || pending.depth == 0
                || pending.depth > budget.depth()
        })
        || (state.adjacency.is_some() && state.frontier_index as usize >= state.frontier.len())
    {
        return Err(ServiceError::RelationCursorInvalid {
            reason: "cursor frontier or pending row references an absent node",
        });
    }
    if state
        .pending
        .iter()
        .map(|pending| pending.relation_digest)
        .collect::<HashSet<_>>()
        .len()
        != state.pending.len()
    {
        return Err(ServiceError::RelationCursorInvalid {
            reason: "cursor pending relations are not unique",
        });
    }
    Ok(())
}

/// Rehydrate every compact node through one set-oriented, all-or-error DB boundary.
fn hydrate_traversal_entities(
    store: &AtlasStore,
    project: projectatlas_core::graph::ProjectInstanceId,
    generation: IndexGeneration,
    state: &RelationTraversalState,
    budget: DetailedRelationBudget,
    retained_state_bytes: u64,
    database_work: &mut RelationDatabaseWork,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<Vec<GraphEntity>> {
    let digests = state
        .nodes
        .iter()
        .map(|node| node.digest)
        .collect::<Vec<_>>();
    let mut entities = Vec::with_capacity(digests.len());
    for chunk in digests.chunks(MAX_REPOSITORY_GRAPH_FRONTIER) {
        let chunk_rows = u32::try_from(chunk.len()).map_err(|_overflow| {
            ServiceError::InvalidInput("entity hydration batch size overflowed".to_string())
        })?;
        let database_budget = relation_database_budget(
            budget,
            *database_work,
            retained_state_bytes,
            chunk.len(),
            chunk_rows,
            chunk_rows,
            chunk_rows,
        )?;
        let batch = store.repository_graph_entities_by_digest(
            project,
            generation,
            chunk,
            database_budget,
            control,
        )?;
        database_work.record(batch.work)?;
        entities.extend(batch.rows);
    }
    if entities.len() != state.nodes.len()
        || entities
            .iter()
            .any(|entity| entity.key().project() != project || entity.generation() != generation)
    {
        return Err(ServiceError::RelationCursorStale {
            field: "traversal entities",
        });
    }
    Ok(entities)
}

/// Reconstruct one node-simple path from parent-before-child cursor state.
fn traversal_path(
    nodes: &[TraversalNodeState],
    entities: &[GraphEntity],
    terminal: u32,
) -> ServiceResult<Vec<GraphEntity>> {
    let mut indices = Vec::new();
    let mut cursor = Some(terminal);
    while let Some(index) = cursor {
        let node = nodes
            .get(index as usize)
            .ok_or(ServiceError::RelationCursorInvalid {
                reason: "path terminal references an absent node",
            })?;
        indices.push(index);
        cursor = node.parent;
    }
    indices.reverse();
    indices
        .into_iter()
        .map(|index| {
            entities
                .get(index as usize)
                .cloned()
                .ok_or(ServiceError::RelationCursorInvalid {
                    reason: "hydrated path entity is absent",
                })
        })
        .collect()
}

/// Whether the bounded traversal state can make progress on a later page.
fn traversal_has_work(state: &RelationTraversalState, maximum_depth: u32) -> bool {
    (state.pending_index as usize) < state.pending.len()
        || (state.frontier_index as usize) < state.frontier.len()
        || (!state.next_frontier.is_empty() && state.depth < maximum_depth)
}

/// Resolve one exact file or symbol anchor without falling back to discovery.
fn resolve_anchor(
    store: &AtlasStore,
    project: projectatlas_core::graph::ProjectInstanceId,
    generation: IndexGeneration,
    anchor: &RelationAnchor,
    budget: DetailedRelationBudget,
    database_work: &mut RelationDatabaseWork,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<GraphEntity> {
    match anchor {
        RelationAnchor::File { file } => {
            let selector = EntitySelector::File { path: file.clone() };
            let key = GraphEntityKey::new(project, &selector);
            let database_budget = relation_database_budget(budget, *database_work, 0, 1, 1, 1, 1)?;
            let batch = store.repository_graph_entity_bounded(
                &key,
                generation,
                database_budget,
                control,
            )?;
            database_work.record(batch.work)?;
            batch.rows.into_iter().next().ok_or_else(|| {
                ServiceError::InvalidInput(format!(
                    "graph file anchor is not available: {}",
                    file.as_str()
                ))
            })
        }
        RelationAnchor::Symbol {
            file,
            name,
            symbol_kind,
            parent,
            signature,
        } => {
            let path = projectatlas_core::graph::RepositoryNodePath::new(std::path::Path::new(
                file.as_str(),
            ))
            .map_err(invalid_graph_input)?;
            let entity_limit = GraphLimits::MAX_ROWS;
            let database_budget = relation_database_budget(
                budget,
                *database_work,
                0,
                1,
                entity_limit,
                entity_limit.saturating_add(1),
                1,
            )?;
            let batch = store.repository_graph_entities_by_path_bounded(
                project,
                generation,
                &path,
                entity_limit,
                database_budget,
                control,
            )?;
            database_work.record(batch.work)?;
            let page = batch.page;
            if page.truncated {
                return Err(ServiceError::InvalidInput(format!(
                    "graph symbol anchor search exceeded the row ceiling for {}",
                    file.as_str()
                )));
            }
            let matches = page
                .rows
                .into_iter()
                .filter(|entity| {
                    let EntitySelector::Symbol { symbol } = entity.selector() else {
                        return false;
                    };
                    symbol.file == *file
                        && symbol.name.as_str() == name
                        && symbol_kind.is_none_or(|kind| symbol.kind == kind)
                        && parent.as_deref().is_none_or(|value| {
                            symbol
                                .parent
                                .as_ref()
                                .is_some_and(|item| item.as_str() == value)
                        })
                        && signature
                            .as_deref()
                            .is_none_or(|value| symbol.signature.as_str() == value)
                })
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [entity] => Ok(entity.clone()),
                [] => Err(ServiceError::InvalidInput(format!(
                    "graph symbol anchor is not available: {}::{name}",
                    file.as_str()
                ))),
                _ => Err(ServiceError::InvalidInput(format!(
                    "graph symbol anchor is ambiguous: {}::{name}; add kind, parent, or signature",
                    file.as_str()
                ))),
            }
        }
    }
}

/// Test one normalized relation against the service-owned trust filters.
pub(super) fn relation_matches(relation: &LogicalRelation, query: &DetailedRelationQuery) -> bool {
    query.relation.is_none_or(|kind| relation.kind() == kind)
        && !(query.relation.is_none()
            && query.content_selection == ContentSelection::UnspecifiedLegacy
            && relation.kind() == GraphRelationKind::Extended(ExtendedRelationKind::Documents))
        && confidence_rank(relation.confidence()) >= confidence_rank(query.minimum_confidence)
        && match query.resolution {
            RelationResolutionFilter::Any => true,
            RelationResolutionFilter::Resolved => {
                matches!(relation.resolution(), RelationResolution::Resolved { .. })
            }
            RelationResolutionFilter::Ambiguous => {
                matches!(relation.resolution(), RelationResolution::Ambiguous { .. })
            }
            RelationResolutionFilter::Unresolved => {
                matches!(relation.resolution(), RelationResolution::Unresolved { .. })
            }
            RelationResolutionFilter::External => {
                matches!(relation.resolution(), RelationResolution::External { .. })
            }
        }
}

/// Return whether an all-family traversal explicitly opts into document edges.
pub(super) fn include_document_relations(query: &DetailedRelationQuery) -> bool {
    query.content_selection != ContentSelection::UnspecifiedLegacy
}

/// Retain exact external identities reached by one bounded traversal page.
pub(super) fn external_relation_identities(
    report: &DetailedRelationReport,
) -> BTreeSet<ExternalRelationIdentity> {
    report
        .rows
        .iter()
        .filter_map(|row| {
            let RelationResolution::External { external, .. } = row.relation.resolution() else {
                return None;
            };
            Some((
                row.relation.kind().as_str().to_string(),
                external.system.as_str().to_string(),
                external.identity.as_str().to_string(),
            ))
        })
        .collect()
}

/// Convert confidence classes to their stable descending rank.
const fn confidence_rank(value: ConfidenceClass) -> u8 {
    match value {
        ConfidenceClass::Exact => 4,
        ConfidenceClass::High => 3,
        ConfidenceClass::Medium => 2,
        ConfidenceClass::Low => 1,
    }
}

/// Convert resolution states to their stable descending rank.
fn resolution_rank(value: &RelationResolution) -> u8 {
    match value {
        RelationResolution::Resolved { .. } => 4,
        RelationResolution::External { .. } => 3,
        RelationResolution::Ambiguous { .. } => 2,
        RelationResolution::Unresolved { .. } => 1,
    }
}

/// Compare two retained relations using the deterministic traversal rank.
fn relation_rank_order(
    left: &RepositoryGraphRelationRow,
    right: &RepositoryGraphRelationRow,
) -> std::cmp::Ordering {
    confidence_rank(right.relation.confidence())
        .cmp(&confidence_rank(left.relation.confidence()))
        .then_with(|| {
            resolution_rank(right.relation.resolution())
                .cmp(&resolution_rank(left.relation.resolution()))
        })
        .then_with(|| {
            left.relation
                .key()
                .canonical_identity()
                .cmp(right.relation.key().canonical_identity())
        })
        .then_with(|| {
            left.relation
                .key()
                .digest()
                .cmp(right.relation.key().digest())
        })
}

/// Select the next local endpoint for one traversal direction.
fn traversable_entity(
    detail: &RepositoryGraphRelationRow,
    direction: RelationDirection,
) -> Option<&GraphEntity> {
    match direction {
        RelationDirection::Outbound
            if matches!(
                detail.relation.resolution(),
                RelationResolution::Resolved { .. }
            ) =>
        {
            detail.target.as_ref()
        }
        RelationDirection::Inbound => Some(&detail.source),
        RelationDirection::Outbound => None,
    }
}

/// Batch-load unique authoritative owner purposes for the complete response.
fn load_purposes(
    store: &AtlasStore,
    project: projectatlas_core::graph::ProjectInstanceId,
    generation: IndexGeneration,
    anchor: &GraphEntity,
    rows: &[TraversalRow],
    budget: DetailedRelationBudget,
    database_work: &mut RelationDatabaseWork,
    retained_state_bytes: u64,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<BTreeMap<String, Purpose>> {
    if !store.has_agent_approved_purpose()? {
        return Ok(BTreeMap::new());
    }
    let mut paths = BTreeSet::new();
    paths.extend(purpose_candidates(anchor));
    for row in rows {
        check_relation_control(control)?;
        paths.extend(purpose_candidates(&row.detail.source));
        if let Some(target) = &row.detail.target {
            paths.extend(purpose_candidates(target));
        }
        for entity in &row.path {
            paths.extend(purpose_candidates(entity));
        }
    }
    let selected = paths.into_iter().collect::<Vec<_>>();
    let mut purposes = BTreeMap::new();
    for chunk in selected.chunks(MAX_REPOSITORY_GRAPH_FRONTIER) {
        let chunk_rows = u32::try_from(chunk.len()).map_err(|_overflow| {
            ServiceError::InvalidInput("purpose hydration batch size overflowed".to_string())
        })?;
        let database_budget = relation_database_budget(
            budget,
            *database_work,
            retained_state_bytes,
            chunk.len(),
            chunk_rows,
            1,
            chunk_rows,
        )?;
        let batch = store.load_purpose_owner_nodes_by_paths_controlled(
            project,
            generation,
            chunk,
            database_budget,
            control,
        )?;
        database_work.record(batch.work)?;
        purposes.extend(
            batch
                .rows
                .into_iter()
                .map(|node| (node.node.path.clone(), node.purpose)),
        );
    }
    Ok(purposes)
}

/// Return exact and nearest-ancestor paths that may own an accepted purpose.
fn purpose_candidates(entity: &GraphEntity) -> Vec<String> {
    let Some(exact) = purpose_owner(entity) else {
        return Vec::new();
    };
    let mut candidates = vec![exact.clone()];
    let mut cursor = exact.as_str();
    while let Some((parent, _name)) = cursor.rsplit_once('/') {
        candidates.push(parent.to_string());
        cursor = parent;
    }
    if exact != "." {
        candidates.push(".".to_string());
    }
    candidates
}

/// Return the file or folder path that authoritatively owns an entity purpose.
fn purpose_owner(entity: &GraphEntity) -> Option<String> {
    match entity.selector() {
        EntitySelector::Project => Some(".".to_string()),
        EntitySelector::Folder { path } => Some(path.as_str().to_string()),
        EntitySelector::File { path } => Some(path.as_str().to_string()),
        EntitySelector::Package { package } => Some(package.manifest.as_str().to_string()),
        EntitySelector::Symbol { symbol } => Some(symbol.file.as_str().to_string()),
        EntitySelector::External { .. } => None,
    }
}

/// Project accepted authored purpose state without trusting suggestions.
fn purpose_projection(
    entity: &GraphEntity,
    purposes: &BTreeMap<String, Purpose>,
) -> RelationPurpose {
    let Some(exact_path) = purpose_owner(entity) else {
        return RelationPurpose::NotApplicable;
    };
    for path in purpose_candidates(entity) {
        if let Some(purpose) = purposes.get(&path)
            && purpose.status == PurposeStatus::Approved
            && purpose.source == PurposeSource::Agent
            && let Some(text) = &purpose.purpose
        {
            return RelationPurpose::Approved {
                path,
                purpose: text.clone(),
                source: purpose.source,
                status: purpose.status,
            };
        }
    }
    RelationPurpose::Unavailable {
        path: Some(exact_path),
    }
}

/// Compose one hydrated response node from graph, purpose, and coverage state.
fn detailed_node(
    entity: GraphEntity,
    content_selection: ContentSelection,
    classifications: &BTreeMap<String, ContentClassification>,
    purposes: &BTreeMap<String, Purpose>,
    coverage: &BTreeMap<String, Vec<CoverageRecord>>,
) -> DetailedRelationNode {
    let classification =
        classification_path(&entity).and_then(|path| classifications.get(&path).copied());
    let purpose = purpose_projection(&entity, purposes);
    let coverage = purpose_owner(&entity)
        .and_then(|path| coverage.get(&path).cloned())
        .unwrap_or_default();
    DetailedRelationNode {
        entity,
        classification,
        content_selection: next_call_content_selection(content_selection, classification),
        purpose,
        coverage,
    }
}

/// Compose one public traversal row from its internal retained state.
fn detailed_row(
    row: TraversalRow,
    query: &DetailedRelationQuery,
    classifications: &BTreeMap<String, ContentClassification>,
    purposes: &BTreeMap<String, Purpose>,
    coverage: &BTreeMap<String, Vec<CoverageRecord>>,
    occurrence_page: (Vec<RelationOccurrence>, bool),
) -> DetailedRelationRow {
    let (occurrences, occurrences_truncated) = occurrence_page;
    let document_unresolved_reason = row.detail.document_unresolved_reason;
    let inbound_view = inbound_relation_view(query.direction, &row.detail.relation);
    let next_call = traversable_entity(&row.detail, query.direction).and_then(|entity| {
        let classification =
            classification_path(entity).and_then(|path| classifications.get(&path).copied());
        next_call_for_entity(entity, query.content_selection, classification)
    });
    let target_purpose = row
        .detail
        .target
        .as_ref()
        .map_or(RelationPurpose::Unavailable { path: None }, |target| {
            purpose_projection(target, purposes)
        });
    let path = row
        .path
        .into_iter()
        .map(|entity| {
            detailed_node(
                entity,
                query.content_selection,
                classifications,
                purposes,
                coverage,
            )
        })
        .collect();
    DetailedRelationRow {
        depth: row.depth,
        direction: query.direction,
        relation: row.detail.relation,
        document_unresolved_reason,
        inbound_view,
        source: detailed_node(
            row.detail.source,
            query.content_selection,
            classifications,
            purposes,
            coverage,
        ),
        target: row.detail.target.map(|target| {
            detailed_node(
                target,
                query.content_selection,
                classifications,
                purposes,
                coverage,
            )
        }),
        target_purpose,
        path,
        occurrences,
        occurrences_truncated,
        next_call,
    }
}

/// Batch-load authoritative path coverage for every unique local owner.
fn load_coverage(
    store: &AtlasStore,
    project: projectatlas_core::graph::ProjectInstanceId,
    generation: IndexGeneration,
    anchor: &GraphEntity,
    rows: &[TraversalRow],
    budget: DetailedRelationBudget,
    database_work: &mut RelationDatabaseWork,
    retained_state_bytes: u64,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<BTreeMap<String, Vec<CoverageRecord>>> {
    let mut paths = BTreeSet::new();
    for entity in std::iter::once(anchor).chain(rows.iter().flat_map(|row| {
        std::iter::once(&row.detail.source)
            .chain(row.detail.target.iter())
            .chain(row.path.iter())
    })) {
        if let Some(path) = purpose_owner(entity)
            && path != "."
        {
            paths.insert(path);
        }
    }

    let mut coverage = BTreeMap::<String, Vec<CoverageRecord>>::new();
    let selected = paths
        .into_iter()
        .map(|path| {
            RepositoryNodePath::new(std::path::Path::new(&path))
                .map(|normalized| (path, normalized))
                .map_err(invalid_graph_input)
        })
        .collect::<ServiceResult<Vec<_>>>()?;
    for chunk in selected.chunks(MAX_REPOSITORY_GRAPH_FRONTIER) {
        let normalized = chunk
            .iter()
            .map(|(_, path)| path.clone())
            .collect::<Vec<_>>();
        let database_budget = relation_database_budget(
            budget,
            *database_work,
            retained_state_bytes,
            normalized.len(),
            GraphLimits::MAX_ROWS,
            1,
            u32::try_from(normalized.len()).map_err(|_overflow| {
                ServiceError::InvalidInput("coverage path batch size overflowed".to_string())
            })?,
        )?;
        let batch = store.repository_graph_path_coverage_bounded(
            project,
            generation,
            &normalized,
            database_budget,
            control,
        )?;
        database_work.record(batch.work)?;
        let page = batch.page;
        if page.truncated {
            return Err(ServiceError::InvalidInput(
                "graph coverage hydration exceeded the bounded work ceiling".to_string(),
            ));
        }
        for record in page.rows {
            let CoverageScope::Path { path } = record.scope() else {
                return Err(ServiceError::InvalidInput(
                    "path coverage hydration returned a project-scoped row".to_string(),
                ));
            };
            coverage
                .entry(path.as_str().to_string())
                .or_default()
                .push(record);
        }
    }
    Ok(coverage)
}

/// Batch-load optional exact occurrence pages for every retained relation.
fn load_occurrence_pages(
    store: &AtlasStore,
    rows: &[TraversalRow],
    query: &DetailedRelationQuery,
    budget: DetailedRelationBudget,
    database_work: &mut RelationDatabaseWork,
    retained_state_bytes: u64,
    control: Option<&IndexWorkControl>,
    reached_limits: &mut Vec<GraphLimitKind>,
) -> ServiceResult<(Vec<(Vec<RelationOccurrence>, bool)>, u32)> {
    if !query.include_occurrences {
        return Ok(((0..rows.len()).map(|_| (Vec::new(), false)).collect(), 0));
    }
    let per_relation = budget.occurrences_per_relation();
    let mut remaining = budget.occurrences_total();
    let mut retained = 0_u32;
    let mut pages = Vec::with_capacity(rows.len());
    let mut start = 0_usize;
    while start < rows.len() {
        check_relation_control(control)?;
        if remaining == 0 {
            push_limit(reached_limits, GraphLimitKind::Occurrences);
            pages.extend((start..rows.len()).map(|_| (Vec::new(), true)));
            break;
        }
        let limit = per_relation.min(remaining);
        let per_relation_work = limit as usize + 1;
        let aggregate_batch = (remaining / limit).max(1) as usize;
        let batch_size = (ADJACENCY_WORK_ROWS / per_relation_work)
            .clamp(1, MAX_REPOSITORY_GRAPH_FRONTIER)
            .min(aggregate_batch)
            .min(rows.len() - start);
        let chunk = &rows[start..start + batch_size];
        let relations = chunk
            .iter()
            .map(|row| row.detail.relation.clone())
            .collect::<Vec<_>>();
        let batch_rows = u32::try_from(batch_size).map_err(|_overflow| {
            ServiceError::InvalidInput("occurrence batch size overflowed".to_string())
        })?;
        let returned_rows = batch_rows.saturating_mul(limit).max(1);
        let hydrated_paths = batch_rows.saturating_mul(limit.saturating_add(1)).max(1);
        let database_budget = relation_database_budget(
            budget,
            *database_work,
            retained_state_bytes,
            relations.len(),
            returned_rows,
            1,
            hydrated_paths,
        )?;
        let batch = store.repository_graph_occurrence_pages_bounded(
            &relations,
            limit,
            database_budget,
            control,
        )?;
        database_work.record(batch.work)?;
        for page in batch.pages {
            let count = u32::try_from(page.rows.len()).map_err(|_overflow| {
                ServiceError::InvalidInput("occurrence count overflowed".to_string())
            })?;
            remaining = remaining.saturating_sub(count);
            retained = retained.saturating_add(count);
            if page.truncated {
                push_limit(reached_limits, GraphLimitKind::Occurrences);
            }
            pages.push((page.rows, page.truncated));
        }
        start += batch_size;
    }
    Ok((pages, retained))
}

/// Map one resolved local entity to an existing exact navigation call.
pub(super) fn next_call_for_entity(
    entity: &GraphEntity,
    content_selection: ContentSelection,
    classification: Option<ContentClassification>,
) -> Option<RelationNextCall> {
    let content_selection = next_call_content_selection(content_selection, classification);
    match entity.selector() {
        EntitySelector::Project | EntitySelector::External { .. } => None,
        EntitySelector::Folder { path } => Some(RelationNextCall::Files {
            folder: path.clone(),
            content_selection,
        }),
        EntitySelector::File { path } => Some(RelationNextCall::Summary {
            file: path.clone(),
            content_selection,
        }),
        EntitySelector::Package { package } => Some(RelationNextCall::Summary {
            file: package.manifest.clone(),
            content_selection,
        }),
        EntitySelector::Symbol { symbol } => Some(RelationNextCall::SymbolSlice {
            symbol: symbol.clone(),
            content_selection,
        }),
    }
}

/// Retain the requested selection when possible and narrow cross-class document targets safely.
fn next_call_content_selection(
    requested: ContentSelection,
    classification: Option<ContentClassification>,
) -> Option<ContentSelection> {
    if requested == ContentSelection::UnspecifiedLegacy {
        return None;
    }
    let Some(classification) = classification else {
        return Some(requested);
    };
    if requested.includes(classification) {
        return Some(requested);
    }
    match classification {
        ContentClassification::Source => Some(ContentSelection::Source),
        ContentClassification::Documentation => Some(ContentSelection::Documentation),
        ContentClassification::ConfigurationData
        | ContentClassification::OtherText
        | ContentClassification::Opaque => None,
    }
}

/// Render one prefix until its self-reported byte field reaches a fixed point.
fn render_relation_prefix<F, E>(
    draft: &DetailedRelationPageDraft,
    selected_rows: usize,
    encode: &mut F,
) -> Result<(DetailedRelationReport, String), E>
where
    F: FnMut(&DetailedRelationReport) -> Result<String, E>,
    E: From<ServiceError>,
{
    check_relation_deadline(draft.deadline).map_err(E::from)?;
    let mut report = draft.report_for_prefix(selected_rows).map_err(E::from)?;
    for _attempt in 0..8 {
        check_relation_deadline(draft.deadline).map_err(E::from)?;
        let encoded = encode(&report)?;
        check_relation_deadline(draft.deadline).map_err(E::from)?;
        let rendered = encoded.len() as u64;
        if report.work.rendered_output_bytes == rendered {
            return Ok((report, encoded));
        }
        report.work.rendered_output_bytes = rendered;
    }
    Err(E::from(ServiceError::InvalidInput(
        "detailed relation output byte metadata did not converge".to_string(),
    )))
}

/// Fit the largest monotonic prefix of one fixed pending batch.
fn fit_detailed_relation_output<F, E>(
    draft: &DetailedRelationPageDraft,
    control: Option<&IndexWorkControl>,
    mut encode: F,
) -> Result<(DetailedRelationReport, String), E>
where
    F: FnMut(&DetailedRelationReport) -> Result<String, E>,
    E: From<ServiceError>,
{
    let maximum = draft.maximum_output_bytes();
    let full_rows = draft.candidate_rows();
    check_relation_control(control).map_err(E::from)?;
    check_relation_deadline(draft.deadline).map_err(E::from)?;
    let full = render_relation_prefix(draft, full_rows, &mut encode)?;
    if full.1.len() <= maximum {
        return Ok(full);
    }
    drop(full);
    let mut low = 0_usize;
    let mut high = full_rows.saturating_sub(1);
    let mut best_rows = None;
    while low <= high {
        check_relation_control(control).map_err(E::from)?;
        check_relation_deadline(draft.deadline).map_err(E::from)?;
        let middle = low + (high - low) / 2;
        let candidate = render_relation_prefix(draft, middle, &mut encode)?;
        if candidate.1.len() <= maximum {
            best_rows = Some(middle);
            low = middle.saturating_add(1);
        } else if middle == 0 {
            break;
        } else {
            high = middle - 1;
        }
    }
    let selected_rows = best_rows.ok_or_else(|| {
        E::from(ServiceError::InvalidInput(
            "graph output byte limit is too small for the empty response envelope".to_string(),
        ))
    })?;
    render_relation_prefix(draft, selected_rows, &mut encode)
}

/// Derive one all-or-error database batch envelope from the remaining request budget.
fn relation_database_budget(
    budget: DetailedRelationBudget,
    work: RelationDatabaseWork,
    retained_state_bytes: u64,
    requested_rows: usize,
    returned_rows: u32,
    hydrated_entities: u32,
    hydrated_paths: u32,
) -> ServiceResult<RepositoryGraphReadBudget> {
    let requested_rows = u32::try_from(requested_rows).map_err(|_overflow| {
        ServiceError::InvalidInput("database request row count overflowed".to_string())
    })?;
    let charged = work
        .decoded_bytes
        .checked_add(retained_state_bytes)
        .ok_or_else(relation_work_overflow)?;
    let decoded_bytes = budget
        .intermediate_bytes()
        .checked_sub(charged)
        .filter(|remaining| *remaining > 0)
        .ok_or_else(|| {
            ServiceError::InvalidInput(
                "detailed relation intermediate-byte budget is exhausted".to_string(),
            )
        })?
        .min(RepositoryGraphReadBudget::MAX_DECODED_BYTES);
    RepositoryGraphReadBudget::new(
        requested_rows,
        returned_rows,
        decoded_bytes,
        hydrated_entities,
        hydrated_paths,
    )
    .map_err(invalid_graph_input)
}

/// Measure deterministic serialized-equivalent bytes without retaining an encoding.
pub(super) fn serialized_equivalent_bytes<T>(value: &T) -> ServiceResult<u64>
where
    T: Serialize + ?Sized,
{
    let mut counter = SerializedByteCounter::default();
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.bytes)
}

/// Measure working composition state retained while public rows are assembled.
fn relation_working_composition_bytes(
    entities: &[GraphEntity],
    rows: &[TraversalRow],
    direction: RelationDirection,
    purposes: &BTreeMap<String, Purpose>,
    coverage: &BTreeMap<String, Vec<CoverageRecord>>,
    classifications: &BTreeMap<String, ContentClassification>,
    occurrence_pages: &[(Vec<RelationOccurrence>, bool)],
) -> ServiceResult<u64> {
    let parts = [
        serialized_equivalent_bytes(entities)?,
        serialized_equivalent_bytes(purposes)?,
        serialized_equivalent_bytes(coverage)?,
        serialized_equivalent_bytes(classifications)?,
        serialized_equivalent_bytes(occurrence_pages)?,
    ];
    let mut bytes = 0_u64;
    for part in parts {
        bytes = bytes.checked_add(part).ok_or_else(relation_work_overflow)?;
    }
    for row in rows {
        let row_bytes = serialized_equivalent_bytes(&(
            row.depth,
            &row.detail.relation,
            inbound_relation_view(direction, &row.detail.relation),
            &row.detail.source,
            &row.detail.target,
            &row.path,
        ))?;
        bytes = bytes
            .checked_add(row_bytes)
            .ok_or_else(relation_work_overflow)?;
    }
    Ok(bytes)
}

/// Measure one fully composed anchor and row set retained by a report.
fn relation_composition_bytes(
    anchor: &DetailedRelationNode,
    rows: &[DetailedRelationRow],
) -> ServiceResult<u64> {
    serialized_equivalent_bytes(&(anchor, rows))
}

/// Charge the larger of construction overlap or two-copy output fitting.
fn relation_intermediate_bytes(
    database_decoded_bytes: u64,
    cursor_bytes: u64,
    working_composition_bytes: u64,
    retained_composition_bytes: u64,
) -> ServiceResult<u64> {
    let construction_peak = working_composition_bytes
        .checked_add(retained_composition_bytes)
        .ok_or_else(relation_work_overflow)?;
    let fitting_peak = retained_composition_bytes
        .checked_mul(2)
        .ok_or_else(relation_work_overflow)?;
    database_decoded_bytes
        .checked_add(cursor_bytes)
        .and_then(|value| value.checked_add(construction_peak.max(fitting_peak)))
        .ok_or_else(relation_work_overflow)
}

/// Exact compact JSON bytes retained by the traversal state alone.
fn serialized_relation_state_bytes(state: &RelationTraversalState) -> ServiceResult<u64> {
    u64::try_from(serde_json::to_vec(state)?.len()).map_err(|_overflow| relation_work_overflow())
}

/// Exact opaque cursor bytes retained when the traversal remains resumable.
fn encoded_relation_state_bytes(
    binding: &DetailedRelationCursorBinding,
    state: &RelationTraversalState,
    budget: DetailedRelationBudget,
) -> ServiceResult<u64> {
    u64::try_from(encode_relation_cursor(binding, state, budget)?.len())
        .map_err(|_overflow| relation_work_overflow())
}

/// Stable classification for impossible aggregate work-counter overflow.
fn relation_work_overflow() -> ServiceError {
    ServiceError::InvalidInput("detailed relation work accounting overflowed".to_string())
}

/// Bind every database call to the earlier caller or service deadline.
pub(super) fn relation_request_control(
    caller: Option<&IndexWorkControl>,
    service_deadline: Instant,
) -> IndexWorkControl {
    let cancellation =
        caller.map_or_else(IndexCancellation::new, |value| value.cancellation().clone());
    let deadline = caller
        .and_then(IndexWorkControl::deadline)
        .map_or(service_deadline, |value| value.min(service_deadline));
    IndexWorkControl::with_deadline(cancellation, deadline)
}

/// Whether the service-owned wall-clock page budget has elapsed.
fn relation_deadline_elapsed(deadline: Instant) -> bool {
    Instant::now() >= deadline
}

/// Fail a non-resumable hydration or render boundary after its service deadline.
fn check_relation_deadline(deadline: Instant) -> ServiceResult<()> {
    if relation_deadline_elapsed(deadline) {
        return Err(DbError::from(IndexWorkFailure::DeadlineExceeded {
            stage: IndexWorkStage::RepositoryTraversal,
        })
        .into());
    }
    Ok(())
}

/// Observe cancellation and the caller deadline during service-owned traversal work.
fn check_relation_control(control: Option<&IndexWorkControl>) -> ServiceResult<()> {
    if let Some(control) = control {
        control
            .check(IndexWorkStage::RepositoryTraversal)
            .map_err(DbError::from)?;
    }
    Ok(())
}

/// Convert a graph-domain validation error into the service input boundary.
fn invalid_graph_input(error: impl std::fmt::Display) -> ServiceError {
    ServiceError::InvalidInput(error.to_string())
}

/// Record one reached limit once while retaining deterministic order.
fn push_limit(reached_limits: &mut Vec<GraphLimitKind>, limit: GraphLimitKind) {
    if !reached_limits.contains(&limit) {
        reached_limits.push(limit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectatlas_core::graph::{
        Completeness, CoverageState, ExtendedRelationKind, GraphIdentityText, RelationResolution,
        SourceSpan,
    };
    use projectatlas_core::symbols::RelationKind;
    use projectatlas_core::{IndexGeneration, Node, NodeKind};
    use std::error::Error;
    use std::fs;
    use std::io;
    use std::path::Path;
    use std::thread;

    #[test]
    fn maximum_depth_does_not_advertise_an_unusable_continuation() {
        let state = RelationTraversalState {
            depth: 1,
            nodes: vec![
                TraversalNodeState {
                    digest: [1; 32],
                    parent: None,
                },
                TraversalNodeState {
                    digest: [2; 32],
                    parent: Some(0),
                },
            ],
            frontier: vec![0],
            frontier_index: 1,
            next_frontier: vec![1],
            adjacency: None,
            pending: Vec::new(),
            pending_index: 0,
            emitted_rows: 1,
            pruned_paths: 0,
        };

        assert!(!traversal_has_work(&state, 1));
        assert!(traversal_has_work(&state, 2));
    }

    #[test]
    fn detailed_relations_are_node_simple_ranked_and_purpose_aware() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("relation-service");
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("src/a.rs"), "pub fn a() {}\n")?;
        fs::write(root.join("src/b.rs"), "pub fn b() {}\n")?;
        let database = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("relation service fixture project identity is missing")?;
        let generation = IndexGeneration::new(1);
        let source = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/a.rs"))?,
            },
            generation,
        )?;
        let target = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/b.rs"))?,
            },
            generation,
        )?;
        let forward = LogicalRelation::new(
            &source,
            GraphRelationKind::Legacy(RelationKind::Calls),
            RelationResolution::resolved(&target)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation,
        )?;
        let backward = LogicalRelation::new(
            &target,
            GraphRelationKind::Legacy(RelationKind::Calls),
            RelationResolution::resolved(&source)?,
            ConfidenceClass::High,
            Completeness::Complete,
            generation,
        )?;
        let unresolved = LogicalRelation::new(
            &source,
            GraphRelationKind::Legacy(RelationKind::Calls),
            RelationResolution::Unresolved {
                reference: GraphIdentityText::new("missing::target")?,
            },
            ConfidenceClass::Medium,
            Completeness::Complete,
            generation,
        )?;
        let occurrence = RelationOccurrence::new(
            &forward,
            RepositoryFilePath::new(Path::new("src/a.rs"))?,
            SourceSpan::new(1, 0, 1, 10)?,
            generation,
        )?;
        let second_occurrence = RelationOccurrence::new(
            &forward,
            RepositoryFilePath::new(Path::new("src/a.rs"))?,
            SourceSpan::new(1, 11, 1, 20)?,
            generation,
        )?;
        let coverage = ["src/a.rs", "src/b.rs"]
            .into_iter()
            .map(|path| {
                CoverageRecord::new(
                    CoverageScope::Path {
                        path: RepositoryNodePath::new(Path::new(path))?,
                    },
                    None,
                    CoverageState::Complete,
                    1,
                    0,
                    generation,
                    None,
                    None,
                )
                .map_err(Into::into)
            })
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
        let mut publication = store.begin_index_publication("relation-service")?;
        publication.begin_scan_replacement()?;
        publication.upsert_scan_node_batch(&[
            test_folder_node("src"),
            test_node("src/a.rs", "hash-a"),
            test_node("src/b.rs", "hash-b"),
        ])?;
        publication.finish_scan_replacement()?;
        publication.replace_repository_graph(
            project,
            &[source, target],
            &[forward, backward, unresolved],
            &[occurrence, second_occurrence],
            &coverage,
        )?;
        publication.complete()?;
        store.set_purpose("src/a.rs", "Own source calls", PurposeSource::Agent)?;
        store.set_purpose("src", "Own source folder", PurposeSource::Agent)?;
        drop(store);

        let store = AtlasStore::open_read_only_for_project(&database, &root)?;
        let report = load_detailed_relations(
            &store,
            &DetailedRelationQuery {
                anchor: RelationAnchor::File {
                    file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                },
                direction: RelationDirection::Outbound,
                relation: Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                minimum_confidence: ConfidenceClass::Low,
                resolution: RelationResolutionFilter::Resolved,
                include_occurrences: true,
                budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                    10,
                    10,
                    3,
                    64 * 1024,
                )?),
                cursor: None,
                content_selection: ContentSelection::UnspecifiedLegacy,
            },
            None,
        )?;

        require(report.returned == 1, "resolved row count changed")?;
        if report.work.inspected_edges != 2 {
            return Err(io::Error::other(format!(
                "inspected edge count changed: expected 2, got {}",
                report.work.inspected_edges
            ))
            .into());
        }
        require(
            report.work.database_requested_rows > 0
                && report.work.database_returned_rows > 0
                && report.work.database_decoded_bytes > 0
                && report.work.hydrated_entities >= 2
                && report.work.hydrated_purpose_paths >= 2
                && report.work.retained_composition_bytes > 0
                && report.work.intermediate_bytes <= 64 * 1024,
            "bounded database work was not aggregated into the service envelope",
        )?;
        require(
            report.work.intermediate_bytes
                >= report
                    .work
                    .database_decoded_bytes
                    .saturating_add(
                        report
                            .continuation
                            .as_ref()
                            .map_or(0, |value| value.len() as u64),
                    )
                    .saturating_add(report.work.retained_composition_bytes.saturating_mul(2)),
            "aggregate intermediate work omitted database, cursor, or composition bytes",
        )?;
        require(report.pruned_paths == 0, "first-page pruning count changed")?;
        require(
            report.truncated,
            "resumable traversal lost truncation state",
        )?;
        let first_cursor = report
            .continuation
            .clone()
            .ok_or("resumable traversal omitted its continuation")?;
        require(report.rows[0].depth == 1, "resolved row depth changed")?;
        require(report.rows[0].path.len() == 2, "node-simple path changed")?;
        require(
            report.rows[0].occurrences.len() == 2,
            "exact occurrence was not retained",
        )?;
        require(report.anchor.coverage.len() == 1, "anchor coverage missing")?;
        require(
            report.rows[0].source.coverage.len() == 1,
            "source coverage missing",
        )?;
        require(
            report.rows[0]
                .target
                .as_ref()
                .map(|node| node.coverage.len())
                == Some(1),
            "target coverage missing",
        )?;
        require(
            matches!(
                report.anchor.purpose,
                RelationPurpose::Approved { ref purpose, .. } if purpose == "Own source calls"
            ),
            "anchor purpose projection changed",
        )?;
        require(
            matches!(
                report.rows[0].target,
                Some(DetailedRelationNode {
                    purpose: RelationPurpose::Approved {
                        ref path,
                        ref purpose,
                        ..
                    },
                    ..
                }) if path == "src" && purpose == "Own source folder"
            ),
            "target did not inherit the nearest accepted folder purpose",
        )?;
        require(
            matches!(
                report.rows[0].path.as_slice(),
                [
                    DetailedRelationNode {
                        purpose: RelationPurpose::Approved { path: source, .. },
                        ..
                    },
                    DetailedRelationNode {
                        purpose: RelationPurpose::Approved { path: target, .. },
                        ..
                    }
                ] if source == "src/a.rs" && target == "src"
            ),
            "node-simple path omitted authoritative purpose projection",
        )?;
        require(
            matches!(
                report.rows[0].next_call,
                Some(RelationNextCall::Summary { ref file, .. }) if file.as_str() == "src/b.rs"
            ),
            "resolved target next call changed",
        )?;
        let terminal_report = load_detailed_relations(
            &store,
            &DetailedRelationQuery {
                anchor: RelationAnchor::File {
                    file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                },
                direction: RelationDirection::Outbound,
                relation: Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                minimum_confidence: ConfidenceClass::Low,
                resolution: RelationResolutionFilter::Resolved,
                include_occurrences: true,
                budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                    10,
                    10,
                    3,
                    64 * 1024,
                )?),
                cursor: Some(first_cursor.clone()),
                content_selection: ContentSelection::UnspecifiedLegacy,
            },
            None,
        )?;
        require(
            terminal_report.returned == 0
                && terminal_report.pruned_paths == 1
                && terminal_report.continuation.is_none()
                && terminal_report.total == RelationTotalState::Exact(1),
            "cursor continuation did not finish the cycle-safe traversal exactly",
        )?;

        let repeated_report = load_detailed_relations(
            &store,
            &DetailedRelationQuery {
                anchor: RelationAnchor::File {
                    file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                },
                direction: RelationDirection::Outbound,
                relation: Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                minimum_confidence: ConfidenceClass::Low,
                resolution: RelationResolutionFilter::Resolved,
                include_occurrences: true,
                budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                    10,
                    10,
                    3,
                    64 * 1024,
                )?),
                cursor: None,
                content_selection: ContentSelection::UnspecifiedLegacy,
            },
            None,
        )?;
        require(
            repeated_report.rows == report.rows
                && repeated_report.continuation == report.continuation,
            "repeated detailed relation page changed rows or cursor bytes",
        )?;

        let mut mismatched_query = DetailedRelationQuery {
            anchor: RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
            },
            direction: RelationDirection::Inbound,
            relation: Some(GraphRelationKind::Legacy(RelationKind::Calls)),
            minimum_confidence: ConfidenceClass::Low,
            resolution: RelationResolutionFilter::Resolved,
            include_occurrences: true,
            budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                10,
                10,
                3,
                64 * 1024,
            )?),
            cursor: Some(first_cursor.clone()),
            content_selection: ContentSelection::UnspecifiedLegacy,
        };
        require(
            matches!(
                load_detailed_relations(&store, &mismatched_query, None),
                Err(ServiceError::RelationCursorMismatched { field: "query" })
            ),
            "query-bound cursor accepted a different direction",
        )?;
        let mut invalid_cursor: serde_json::Value = serde_json::from_str(&first_cursor)?;
        invalid_cursor["version"] = serde_json::json!(DETAILED_RELATION_CURSOR_VERSION + 1);
        mismatched_query.direction = RelationDirection::Outbound;
        mismatched_query.cursor = Some(serde_json::to_string(&invalid_cursor)?);
        require(
            matches!(
                load_detailed_relations(&store, &mismatched_query, None),
                Err(ServiceError::RelationCursorStale {
                    field: "algorithm version"
                })
            ),
            "unknown cursor version did not fail closed",
        )?;
        mismatched_query.cursor = Some("{".to_string());
        require(
            matches!(
                load_detailed_relations(&store, &mismatched_query, None),
                Err(ServiceError::RelationCursorInvalid { .. })
            ),
            "malformed cursor did not fail closed",
        )?;
        mismatched_query.cursor = Some("x".repeat(DETAILED_RELATION_CURSOR_MAX_BYTES + 1));
        require(
            matches!(
                load_detailed_relations(&store, &mismatched_query, None),
                Err(ServiceError::RelationCursorInvalid { .. })
            ),
            "oversized cursor did not fail closed before decoding",
        )?;
        mismatched_query.cursor = Some(first_cursor.clone());
        mismatched_query.budget =
            mismatched_query
                .budget
                .with_aggregate_limits(Some(9), None, None, None, None, None)?;
        require(
            matches!(
                load_detailed_relations(&store, &mismatched_query, None),
                Err(ServiceError::RelationCursorMismatched { field: "budget" })
            ),
            "cursor accepted a different result-defining budget",
        )?;

        let cancellation = projectatlas_core::IndexCancellation::new();
        cancellation.cancel();
        let control = IndexWorkControl::new(cancellation, None);
        mismatched_query.cursor = None;
        let cancelled = load_detailed_relations(&store, &mismatched_query, Some(&control));
        require(
            cancelled
                .err()
                .is_some_and(|error| error.to_string().contains("cancel")),
            "relation traversal did not propagate cancellation",
        )?;
        require(
            relation_deadline_elapsed(
                Instant::now()
                    .checked_sub(Duration::from_millis(2))
                    .ok_or("deadline test clock underflowed")?,
            ),
            "service-owned relation deadline was not classified deterministically",
        )?;
        let mut expired_draft = load_detailed_relation_page(
            &store,
            &DetailedRelationQuery {
                anchor: RelationAnchor::File {
                    file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                },
                direction: RelationDirection::Outbound,
                relation: Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                minimum_confidence: ConfidenceClass::Low,
                resolution: RelationResolutionFilter::Resolved,
                include_occurrences: false,
                budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                    10,
                    10,
                    1,
                    64 * 1024,
                )?),
                cursor: None,
                content_selection: ContentSelection::UnspecifiedLegacy,
            },
            None,
        )?;
        expired_draft.deadline = Instant::now()
            .checked_sub(Duration::from_millis(1))
            .ok_or("render deadline test clock underflowed")?;
        require(
            matches!(
                expired_draft.fit_compact(None),
                Err(ServiceError::Db(DbError::IndexWork(
                    IndexWorkFailure::DeadlineExceeded {
                        stage: IndexWorkStage::RepositoryTraversal
                    }
                )))
            ),
            "adapter rendering ignored the service-owned relation deadline",
        )?;
        let exact_output_limit = 4 * 1024;
        let limited_report = load_detailed_relations(
            &store,
            &DetailedRelationQuery {
                anchor: RelationAnchor::File {
                    file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                },
                direction: RelationDirection::Outbound,
                relation: Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                minimum_confidence: ConfidenceClass::Low,
                resolution: RelationResolutionFilter::Resolved,
                include_occurrences: true,
                budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                    10,
                    10,
                    3,
                    exact_output_limit,
                )?),
                cursor: None,
                content_selection: ContentSelection::UnspecifiedLegacy,
            },
            None,
        )?;
        require(
            limited_report.truncated,
            "output truncation was not reported",
        )?;
        require(
            limited_report
                .reached_limits
                .contains(&GraphLimitKind::OutputBytes),
            "output byte limit was not reported",
        )?;
        require(
            serde_json::to_vec(&limited_report)?.len() <= exact_output_limit as usize,
            "serialized report exceeded its hard output limit",
        )?;

        let unresolved_report = load_detailed_relations(
            &store,
            &DetailedRelationQuery {
                anchor: RelationAnchor::File {
                    file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                },
                direction: RelationDirection::Outbound,
                relation: Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                minimum_confidence: ConfidenceClass::Low,
                resolution: RelationResolutionFilter::Unresolved,
                include_occurrences: false,
                budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                    10,
                    10,
                    1,
                    64 * 1024,
                )?),
                cursor: None,
                content_selection: ContentSelection::UnspecifiedLegacy,
            },
            None,
        )?;
        require(
            unresolved_report.returned == 1,
            "unresolved relation was not retained",
        )?;
        require(
            unresolved_report.rows[0].target.is_none(),
            "unresolved relation fabricated a target",
        )?;
        require(
            unresolved_report.rows[0].target_purpose == RelationPurpose::Unavailable { path: None },
            "unresolved purpose state changed",
        )?;

        let row_limited = load_detailed_relations(
            &store,
            &DetailedRelationQuery {
                anchor: RelationAnchor::File {
                    file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                },
                direction: RelationDirection::Outbound,
                relation: Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                minimum_confidence: ConfidenceClass::Low,
                resolution: RelationResolutionFilter::Any,
                include_occurrences: false,
                budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                    1,
                    10,
                    1,
                    64 * 1024,
                )?)
                .with_aggregate_limits(Some(10), None, None, None, None, None)?,
                cursor: None,
                content_selection: ContentSelection::UnspecifiedLegacy,
            },
            None,
        )?;
        require(
            row_limited.returned == 1
                && row_limited.continuation.is_some()
                && row_limited.reached_limits.contains(&GraphLimitKind::Rows)
                && !row_limited.reached_limits.contains(&GraphLimitKind::Edges),
            "row budget did not remain independent from edge work",
        )?;

        let edge_limited = load_detailed_relations(
            &store,
            &DetailedRelationQuery {
                anchor: RelationAnchor::File {
                    file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                },
                direction: RelationDirection::Outbound,
                relation: Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                minimum_confidence: ConfidenceClass::Low,
                resolution: RelationResolutionFilter::Any,
                include_occurrences: false,
                budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                    10,
                    10,
                    1,
                    64 * 1024,
                )?)
                .with_aggregate_limits(Some(1), None, None, None, None, None)?,
                cursor: None,
                content_selection: ContentSelection::UnspecifiedLegacy,
            },
            None,
        )?;
        require(
            edge_limited.work.inspected_edges == 1
                && edge_limited.continuation.is_some()
                && edge_limited.reached_limits.contains(&GraphLimitKind::Edges)
                && !edge_limited.reached_limits.contains(&GraphLimitKind::Rows),
            "edge budget exhaustion was not reported independently",
        )?;

        let node_limited = load_detailed_relations(
            &store,
            &DetailedRelationQuery {
                anchor: RelationAnchor::File {
                    file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                },
                direction: RelationDirection::Outbound,
                relation: Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                minimum_confidence: ConfidenceClass::Low,
                resolution: RelationResolutionFilter::Resolved,
                include_occurrences: false,
                budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                    10,
                    10,
                    1,
                    64 * 1024,
                )?)
                .with_aggregate_limits(
                    None,
                    Some(1),
                    Some(11),
                    None,
                    None,
                    None,
                )?,
                cursor: None,
                content_selection: ContentSelection::UnspecifiedLegacy,
            },
            None,
        )?;
        require(
            node_limited.returned == 0
                && node_limited.continuation.is_none()
                && node_limited.total == RelationTotalState::Unknown
                && node_limited.reached_limits.contains(&GraphLimitKind::Nodes),
            "terminal node-state budget did not fail bounded",
        )?;

        let visited_limited = load_detailed_relations(
            &store,
            &DetailedRelationQuery {
                anchor: RelationAnchor::File {
                    file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                },
                direction: RelationDirection::Outbound,
                relation: Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                minimum_confidence: ConfidenceClass::Low,
                resolution: RelationResolutionFilter::Resolved,
                include_occurrences: false,
                budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                    10,
                    10,
                    1,
                    64 * 1024,
                )?)
                .with_aggregate_limits(
                    None,
                    Some(11),
                    Some(1),
                    None,
                    None,
                    None,
                )?,
                cursor: None,
                content_selection: ContentSelection::UnspecifiedLegacy,
            },
            None,
        )?;
        require(
            visited_limited.returned == 0
                && visited_limited.continuation.is_none()
                && visited_limited.total == RelationTotalState::Unknown
                && visited_limited
                    .reached_limits
                    .contains(&GraphLimitKind::Visited),
            "terminal visited-state budget did not fail bounded",
        )?;

        let occurrence_limited = load_detailed_relations(
            &store,
            &DetailedRelationQuery {
                anchor: RelationAnchor::File {
                    file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                },
                direction: RelationDirection::Outbound,
                relation: Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                minimum_confidence: ConfidenceClass::Low,
                resolution: RelationResolutionFilter::Resolved,
                include_occurrences: true,
                budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                    10,
                    10,
                    3,
                    64 * 1024,
                )?)
                .with_aggregate_limits(None, None, None, Some(1), None, None)?,
                cursor: None,
                content_selection: ContentSelection::UnspecifiedLegacy,
            },
            None,
        )?;
        require(
            occurrence_limited.work.retained_occurrences == 1
                && occurrence_limited.rows[0].occurrences.len() == 1
                && occurrence_limited
                    .reached_limits
                    .contains(&GraphLimitKind::Occurrences),
            "aggregate occurrence budget did not truncate exact evidence",
        )?;

        drop(store);
        let writable = AtlasStore::open_for_project(&database, &root)?;
        writable.set_purpose("src/a.rs", "Own source calls", PurposeSource::Agent)?;
        drop(writable);
        let store = AtlasStore::open_read_only_for_project(&database, &root)?;
        let unchanged_query = DetailedRelationQuery {
            anchor: RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
            },
            direction: RelationDirection::Outbound,
            relation: Some(GraphRelationKind::Legacy(RelationKind::Calls)),
            minimum_confidence: ConfidenceClass::Low,
            resolution: RelationResolutionFilter::Resolved,
            include_occurrences: true,
            budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                10,
                10,
                3,
                64 * 1024,
            )?),
            cursor: Some(first_cursor.clone()),
            content_selection: ContentSelection::UnspecifiedLegacy,
        };
        require(
            load_detailed_relations(&store, &unchanged_query, None).is_ok(),
            "an accepted-purpose no-op made the relation cursor stale",
        )?;
        drop(store);
        let writable = AtlasStore::open_for_project(&database, &root)?;
        writable.set_purpose("src/a.rs", "Own updated source calls", PurposeSource::Agent)?;
        drop(writable);
        let store = AtlasStore::open_read_only_for_project(&database, &root)?;
        let stale_query = DetailedRelationQuery {
            anchor: RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
            },
            direction: RelationDirection::Outbound,
            relation: Some(GraphRelationKind::Legacy(RelationKind::Calls)),
            minimum_confidence: ConfidenceClass::Low,
            resolution: RelationResolutionFilter::Resolved,
            include_occurrences: true,
            budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                10,
                10,
                3,
                64 * 1024,
            )?),
            cursor: Some(first_cursor),
            content_selection: ContentSelection::UnspecifiedLegacy,
        };
        require(
            matches!(
                load_detailed_relations(&store, &stale_query, None),
                Err(ServiceError::RelationCursorStale {
                    field: "authored-purpose revision"
                })
            ),
            "purpose-bound cursor survived an authored-purpose revision",
        )?;

        let mut current_query = stale_query;
        current_query.cursor = None;
        let current_cursor = load_detailed_relations(&store, &current_query, None)?
            .continuation
            .ok_or("current-generation traversal omitted its continuation")?;
        drop(store);

        let mut writable = AtlasStore::open_for_project(&database, &root)?;
        let generation = IndexGeneration::new(2);
        let source = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/a.rs"))?,
            },
            generation,
        )?;
        let target = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/b.rs"))?,
            },
            generation,
        )?;
        let forward = LogicalRelation::new(
            &source,
            GraphRelationKind::Legacy(RelationKind::Calls),
            RelationResolution::resolved(&target)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation,
        )?;
        let backward = LogicalRelation::new(
            &target,
            GraphRelationKind::Legacy(RelationKind::Calls),
            RelationResolution::resolved(&source)?,
            ConfidenceClass::High,
            Completeness::Complete,
            generation,
        )?;
        let unresolved = LogicalRelation::new(
            &source,
            GraphRelationKind::Legacy(RelationKind::Calls),
            RelationResolution::Unresolved {
                reference: GraphIdentityText::new("missing::target")?,
            },
            ConfidenceClass::Medium,
            Completeness::Complete,
            generation,
        )?;
        let mut publication = writable.begin_index_publication("relation-service-generation")?;
        publication.replace_repository_graph(
            project,
            &[source, target],
            &[forward, backward, unresolved],
            &[],
            &[],
        )?;
        publication.complete()?;
        drop(writable);

        let store = AtlasStore::open_read_only_for_project(&database, &root)?;
        current_query.cursor = Some(current_cursor);
        require(
            matches!(
                load_detailed_relations(&store, &current_query, None),
                Err(ServiceError::RelationCursorStale {
                    field: "graph generation"
                })
            ),
            "generation-bound cursor survived graph publication",
        )?;
        Ok(())
    }

    #[test]
    fn detailed_relation_pages_preserve_extended_inbound_symbol_and_parallel_behavior()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("relation-pagination");
        fs::create_dir_all(root.join("src"))?;
        for name in ["a.rs", "b.rs", "c.rs", "d.rs"] {
            fs::write(root.join("src").join(name), format!("// {name}\n"))?;
        }
        let database = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("relation pagination fixture identity is missing")?;
        let generation = IndexGeneration::new(1);
        let a = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/a.rs"))?,
            },
            generation,
        )?;
        let b = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/b.rs"))?,
            },
            generation,
        )?;
        let c = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/c.rs"))?,
            },
            generation,
        )?;
        let d = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/d.rs"))?,
            },
            generation,
        )?;
        let entry = GraphEntity::new(
            project,
            EntitySelector::Symbol {
                symbol: SymbolSelector {
                    file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                    name: GraphIdentityText::new("entry")?,
                    kind: SymbolKind::Function,
                    parent: Some(GraphIdentityText::new("Root")?),
                    signature: GraphIdentityText::new("entry()")?,
                },
            },
            generation,
        )?;
        let entry_overload = GraphEntity::new(
            project,
            EntitySelector::Symbol {
                symbol: SymbolSelector {
                    file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                    name: GraphIdentityText::new("entry")?,
                    kind: SymbolKind::Function,
                    parent: Some(GraphIdentityText::new("Root")?),
                    signature: GraphIdentityText::new("entry(u8)")?,
                },
            },
            generation,
        )?;
        let references = GraphRelationKind::Extended(ExtendedRelationKind::References);
        let a_b = LogicalRelation::new(
            &a,
            references,
            RelationResolution::resolved(&b)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation,
        )?;
        let a_c = LogicalRelation::new(
            &a,
            references,
            RelationResolution::resolved(&c)?,
            ConfidenceClass::High,
            Completeness::Complete,
            generation,
        )?;
        let b_d = LogicalRelation::new(
            &b,
            references,
            RelationResolution::resolved(&d)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation,
        )?;
        let c_d = LogicalRelation::new(
            &c,
            references,
            RelationResolution::resolved(&d)?,
            ConfidenceClass::Medium,
            Completeness::Complete,
            generation,
        )?;
        let d_a = LogicalRelation::new(
            &d,
            references,
            RelationResolution::resolved(&a)?,
            ConfidenceClass::Low,
            Completeness::Complete,
            generation,
        )?;
        let entry_b = LogicalRelation::new(
            &entry,
            references,
            RelationResolution::resolved(&b)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation,
        )?;
        let mut publication = store.begin_index_publication("relation-pagination")?;
        publication.begin_scan_replacement()?;
        publication.upsert_scan_node_batch(&[
            test_folder_node("src"),
            test_node("src/a.rs", "hash-a"),
            test_node("src/b.rs", "hash-b"),
            test_node("src/c.rs", "hash-c"),
            test_node("src/d.rs", "hash-d"),
        ])?;
        publication.finish_scan_replacement()?;
        publication.replace_repository_graph(
            project,
            &[a, b, c, d, entry, entry_overload],
            &[a_b, a_c, b_d, c_d, d_a, entry_b],
            &[],
            &[],
        )?;
        publication.complete()?;
        store.set_purpose("src", "Own graph components", PurposeSource::Agent)?;
        drop(store);

        let store = AtlasStore::open_read_only_for_project(&database, &root)?;
        let file_anchor = RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
        };
        let complete_budget =
            DetailedRelationBudget::from_graph_limits(GraphLimits::new(10, 5, 3, 256 * 1024)?)
                .with_aggregate_limits(Some(100), Some(100), Some(100), None, None, None)?;
        let complete_query = DetailedRelationQuery {
            anchor: file_anchor.clone(),
            direction: RelationDirection::Outbound,
            relation: Some(references),
            minimum_confidence: ConfidenceClass::Low,
            resolution: RelationResolutionFilter::Resolved,
            include_occurrences: false,
            budget: complete_budget,
            cursor: None,
            content_selection: ContentSelection::UnspecifiedLegacy,
        };
        let (complete_rows, complete_total, complete_pruned_paths) =
            collect_relation_pages(&store, complete_query.clone(), 10)?;
        require(
            complete_rows.len() == 3
                && complete_pruned_paths == 2
                && complete_total == RelationTotalState::Exact(3),
            "extended diamond/cycle traversal did not finish node-simple and exact",
        )?;

        let page_budget =
            DetailedRelationBudget::from_graph_limits(GraphLimits::new(1, 5, 3, 256 * 1024)?)
                .with_aggregate_limits(Some(100), Some(100), Some(100), None, None, None)?;
        let (paged_rows, terminal_total, _paged_pruned_paths) = collect_relation_pages(
            &store,
            DetailedRelationQuery {
                anchor: file_anchor,
                direction: RelationDirection::Outbound,
                relation: Some(references),
                minimum_confidence: ConfidenceClass::Low,
                resolution: RelationResolutionFilter::Resolved,
                include_occurrences: false,
                budget: page_budget,
                cursor: None,
                content_selection: ContentSelection::UnspecifiedLegacy,
            },
            10,
        )?;
        require(
            paged_rows == complete_rows && terminal_total == RelationTotalState::Exact(3),
            "multi-page traversal changed extended relation ranking, paths, or total",
        )?;

        let inbound = load_detailed_relations(
            &store,
            &DetailedRelationQuery {
                anchor: RelationAnchor::File {
                    file: RepositoryFilePath::new(Path::new("src/d.rs"))?,
                },
                direction: RelationDirection::Inbound,
                relation: Some(references),
                minimum_confidence: ConfidenceClass::Low,
                resolution: RelationResolutionFilter::Resolved,
                include_occurrences: false,
                budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                    10,
                    5,
                    1,
                    256 * 1024,
                )?)
                .with_aggregate_limits(
                    Some(100),
                    None,
                    None,
                    None,
                    None,
                    None,
                )?,
                cursor: None,
                content_selection: ContentSelection::UnspecifiedLegacy,
            },
            None,
        )?;
        require(
            inbound.returned == 2
                && inbound.rows.iter().all(|row| row.inbound_view.is_none())
                && inbound.rows[0].relation.confidence() == ConfidenceClass::Exact
                && inbound.rows[1].relation.confidence() == ConfidenceClass::Medium,
            "inbound extended relations were not ranked across the bounded batch",
        )?;

        let ambiguous_symbol = DetailedRelationQuery {
            anchor: RelationAnchor::Symbol {
                file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                name: "entry".to_string(),
                symbol_kind: None,
                parent: None,
                signature: None,
            },
            direction: RelationDirection::Outbound,
            relation: Some(references),
            minimum_confidence: ConfidenceClass::Low,
            resolution: RelationResolutionFilter::Resolved,
            include_occurrences: false,
            budget: complete_budget,
            cursor: None,
            content_selection: ContentSelection::UnspecifiedLegacy,
        };
        require(
            load_detailed_relations(&store, &ambiguous_symbol, None)
                .err()
                .is_some_and(|error| error.to_string().contains("ambiguous")),
            "ambiguous symbol anchor did not require an exact selector",
        )?;
        let exact_symbol = load_detailed_relations(
            &store,
            &DetailedRelationQuery {
                anchor: RelationAnchor::Symbol {
                    file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                    name: "entry".to_string(),
                    symbol_kind: Some(SymbolKind::Function),
                    parent: Some("Root".to_string()),
                    signature: Some("entry()".to_string()),
                },
                ..ambiguous_symbol
            },
            None,
        )?;
        require(
            exact_symbol.returned == 1
                && exact_symbol.rows[0]
                    .next_call
                    .as_ref()
                    .is_some_and(|next| matches!(next, RelationNextCall::Summary { file, .. } if file.as_str() == "src/b.rs")),
            "exact symbol selector did not retain its reusable target call",
        )?;
        drop(store);

        let parallel_query = complete_query;
        let mut readers = Vec::new();
        for _reader in 0..2 {
            let database = database.clone();
            let root = root.clone();
            let query = parallel_query.clone();
            readers.push(thread::spawn(
                move || -> Result<Vec<DetailedRelationRow>, String> {
                    let store = AtlasStore::open_read_only_for_project(&database, &root)
                        .map_err(|error| error.to_string())?;
                    collect_relation_pages(&store, query, 10)
                        .map(|(rows, _total, _pruned_paths)| rows)
                        .map_err(|error| error.to_string())
                },
            ));
        }
        for reader in readers {
            let rows = reader
                .join()
                .map_err(|_panic| io::Error::other("parallel relation reader panicked"))?
                .map_err(io::Error::other)?;
            require(
                rows == complete_rows,
                "parallel relation snapshot changed deterministic rows",
            )?;
        }
        Ok(())
    }

    #[test]
    fn classified_relations_preserve_legacy_defaults_and_stop_cross_class_frontiers()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("classified-relations");
        fs::create_dir_all(root.join("docs"))?;
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("docs/guide.md"), "# Guide\n")?;
        fs::write(root.join("docs/other.md"), "# Other\n")?;
        fs::write(root.join("src/lib.rs"), "pub fn library() {}\n")?;
        let database = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or("classified relation fixture project identity is missing")?;
        let generation = IndexGeneration::new(1);
        let guide = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("docs/guide.md"))?,
            },
            generation,
        )?;
        let other_document = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("docs/other.md"))?,
            },
            generation,
        )?;
        let source = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/lib.rs"))?,
            },
            generation,
        )?;
        let documents = GraphRelationKind::Extended(ExtendedRelationKind::Documents);
        let references = GraphRelationKind::Extended(ExtendedRelationKind::References);
        let guide_documents_source = LogicalRelation::new(
            &guide,
            documents,
            RelationResolution::resolved(&source)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation,
        )?;
        let source_documents_other = LogicalRelation::new(
            &source,
            documents,
            RelationResolution::resolved(&other_document)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation,
        )?;
        let guide_references_other = LogicalRelation::new(
            &guide,
            references,
            RelationResolution::resolved(&other_document)?,
            ConfidenceClass::Low,
            Completeness::Complete,
            generation,
        )?;
        let first_document_occurrence = RelationOccurrence::new(
            &guide_documents_source,
            RepositoryFilePath::new(Path::new("docs/guide.md"))?,
            SourceSpan::new(2, 0, 2, 12)?,
            generation,
        )?;
        let second_document_occurrence = RelationOccurrence::new(
            &guide_documents_source,
            RepositoryFilePath::new(Path::new("docs/guide.md"))?,
            SourceSpan::new(4, 0, 4, 12)?,
            generation,
        )?;
        let document_coverage = [
            CoverageRecord::new(
                CoverageScope::Path {
                    path: RepositoryNodePath::new(Path::new("docs/guide.md"))?,
                },
                Some(documents),
                CoverageState::Complete,
                1,
                0,
                generation,
                None,
                None,
            )?,
            CoverageRecord::new(
                CoverageScope::Path {
                    path: RepositoryNodePath::new(Path::new("docs/other.md"))?,
                },
                Some(documents),
                CoverageState::NoCandidates,
                0,
                0,
                generation,
                None,
                None,
            )?,
            CoverageRecord::new(
                CoverageScope::Path {
                    path: RepositoryNodePath::new(Path::new("src/lib.rs"))?,
                },
                Some(documents),
                CoverageState::Complete,
                1,
                0,
                generation,
                None,
                None,
            )?,
        ];
        let mut publication = store.begin_index_publication("classified-relations")?;
        publication.begin_scan_replacement()?;
        publication.upsert_scan_node_batch(&[
            test_folder_node("docs"),
            test_folder_node("src"),
            classified_test_node("docs/guide.md", "hash-guide", ".md", "markdown"),
            classified_test_node("docs/other.md", "hash-other", ".md", "markdown"),
            classified_test_node("src/lib.rs", "hash-source", ".rs", "rust"),
        ])?;
        publication.upsert_file_content_classification_batch(&[
            projectatlas_db::FileContentClassification {
                path: "docs/guide.md".to_string(),
                classification: ContentClassification::Documentation,
            },
            projectatlas_db::FileContentClassification {
                path: "docs/other.md".to_string(),
                classification: ContentClassification::Documentation,
            },
            projectatlas_db::FileContentClassification {
                path: "src/lib.rs".to_string(),
                classification: ContentClassification::Source,
            },
        ])?;
        publication.finish_scan_replacement()?;
        publication.replace_repository_graph(
            project,
            &[guide, other_document, source],
            &[
                guide_documents_source,
                source_documents_other,
                guide_references_other,
            ],
            &[first_document_occurrence, second_document_occurrence],
            &document_coverage,
        )?;
        publication.complete()?;
        drop(store);

        let store = AtlasStore::open_read_only_for_project(&database, &root)?;
        let anchor = RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("docs/guide.md"))?,
        };
        let legacy = load_detailed_relations(
            &store,
            &DetailedRelationQuery {
                anchor: anchor.clone(),
                direction: RelationDirection::Outbound,
                relation: None,
                minimum_confidence: ConfidenceClass::Low,
                resolution: RelationResolutionFilter::Resolved,
                include_occurrences: false,
                budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                    1,
                    5,
                    3,
                    256 * 1024,
                )?)
                .with_aggregate_limits(
                    Some(1),
                    Some(10),
                    Some(10),
                    None,
                    None,
                    None,
                )?,
                cursor: None,
                content_selection: ContentSelection::UnspecifiedLegacy,
            },
            None,
        )?;
        require(
            legacy.returned == 1 && legacy.rows[0].relation.kind() == references,
            "legacy all-family query let a document edge consume its pre-limit candidate page",
        )?;
        let legacy_json = serde_json::to_value(&legacy)?;
        require(
            legacy_json.get("content_selection").is_none()
                && legacy_json["anchor"].get("content_selection").is_none()
                && legacy_json["rows"][0].get("inbound_view").is_none()
                && legacy_json["rows"][0]["source"]
                    .get("content_selection")
                    .is_none()
                && legacy_json["rows"][0]["next_call"]
                    .get("content_selection")
                    .is_none(),
            "legacy relation output serialized a new selection field",
        )?;

        let explicit_documents = load_detailed_relations(
            &store,
            &DetailedRelationQuery {
                anchor: anchor.clone(),
                direction: RelationDirection::Outbound,
                relation: Some(documents),
                minimum_confidence: ConfidenceClass::Low,
                resolution: RelationResolutionFilter::Resolved,
                include_occurrences: true,
                budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                    10,
                    5,
                    3,
                    256 * 1024,
                )?)
                .with_aggregate_limits(
                    Some(10),
                    Some(10),
                    Some(10),
                    None,
                    None,
                    None,
                )?,
                cursor: None,
                content_selection: ContentSelection::Documentation,
            },
            None,
        )?;
        require(
            explicit_documents.returned == 1
                && explicit_documents.anchor.coverage.iter().any(|coverage| {
                    coverage.relation() == Some(documents)
                        && coverage.state() == CoverageState::Complete
                })
                && explicit_documents.rows[0].inbound_view.is_none()
                && explicit_documents.anchor.classification
                    == Some(ContentClassification::Documentation)
                && explicit_documents.rows[0]
                    .target
                    .as_ref()
                    .is_some_and(|target| {
                        target.classification == Some(ContentClassification::Source)
                            && target.content_selection == Some(ContentSelection::Source)
                    })
                && matches!(
                    explicit_documents.rows[0].next_call,
                    Some(RelationNextCall::Summary {
                        content_selection: Some(ContentSelection::Source),
                        ..
                    })
                ),
            "explicit document relation did not retain its classified cross-class endpoint",
        )?;

        let empty_documents = load_detailed_relations(
            &store,
            &DetailedRelationQuery {
                anchor: RelationAnchor::File {
                    file: RepositoryFilePath::new(Path::new("docs/other.md"))?,
                },
                direction: RelationDirection::Outbound,
                relation: Some(documents),
                minimum_confidence: ConfidenceClass::Low,
                resolution: RelationResolutionFilter::Any,
                include_occurrences: true,
                budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                    10,
                    5,
                    3,
                    256 * 1024,
                )?),
                cursor: None,
                content_selection: ContentSelection::Documentation,
            },
            None,
        )?;
        require(
            empty_documents.returned == 0
                && empty_documents.total == RelationTotalState::Exact(0)
                && empty_documents.anchor.coverage.iter().any(|coverage| {
                    coverage.relation() == Some(documents)
                        && coverage.state() == CoverageState::NoCandidates
                        && coverage.total() == 0
                }),
            "empty document traversal omitted explicit no-candidate coverage",
        )?;
        require(
            explicit_documents.rows.iter().all(|row| row.depth == 1),
            "cross-class document endpoint expanded as an unrelated traversal frontier",
        )?;
        require(
            explicit_documents.total == RelationTotalState::Exact(1)
                && explicit_documents.continuation.is_none()
                && !explicit_documents.truncated,
            "classified cross-class traversal did not report exact terminal completeness",
        )?;
        require(
            explicit_documents.work.hydrated_classification_paths == 2,
            "relation projection did not batch the two unique classified endpoint paths",
        )?;
        let explicit_documents_json = serde_json::to_value(&explicit_documents)?;
        require(
            explicit_documents_json["rows"][0]
                .get("inbound_view")
                .is_none(),
            "outbound document relation serialized an inverse view",
        )?;

        let inbound_documents = load_detailed_relations(
            &store,
            &DetailedRelationQuery {
                anchor: RelationAnchor::File {
                    file: RepositoryFilePath::new(Path::new("src/lib.rs"))?,
                },
                direction: RelationDirection::Inbound,
                relation: Some(documents),
                minimum_confidence: ConfidenceClass::Low,
                resolution: RelationResolutionFilter::Resolved,
                include_occurrences: true,
                budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                    10,
                    5,
                    3,
                    256 * 1024,
                )?)
                .with_aggregate_limits(
                    Some(10),
                    Some(10),
                    Some(10),
                    None,
                    None,
                    None,
                )?,
                cursor: None,
                content_selection: ContentSelection::Source,
            },
            None,
        )?;
        require(
            inbound_documents.returned == 1
                && inbound_documents.rows[0].relation.kind() == documents
                && inbound_documents.rows[0].inbound_view == Some("documented_by")
                && inbound_documents.rows[0].source.classification
                    == Some(ContentClassification::Documentation)
                && matches!(
                    inbound_documents.rows[0].next_call,
                    Some(RelationNextCall::Summary {
                        content_selection: Some(ContentSelection::Documentation),
                        ..
                    })
                ),
            "inbound document relation did not expose its read-only documented_by view",
        )?;
        let outbound_row = &explicit_documents.rows[0];
        let inbound_row = &inbound_documents.rows[0];
        require(
            outbound_row.relation.key() == inbound_row.relation.key()
                && outbound_row.relation.resolution() == inbound_row.relation.resolution()
                && outbound_row.relation.confidence() == inbound_row.relation.confidence()
                && outbound_row.relation.completeness() == inbound_row.relation.completeness()
                && outbound_row.relation.generation() == inbound_row.relation.generation()
                && outbound_row.occurrences == inbound_row.occurrences
                && outbound_row.occurrences.len() == 2
                && !outbound_row.occurrences_truncated
                && !inbound_row.occurrences_truncated,
            "outbound documents and inbound documented_by views disagreed on canonical evidence",
        )?;
        let inbound_documents_json = serde_json::to_value(&inbound_documents)?;
        let inbound_documents_encoded = serde_json::to_string(&inbound_documents)?;
        require(
            inbound_documents_json["rows"][0]["inbound_view"] == "documented_by"
                && inbound_documents_encoded.len() as u64
                    == inbound_documents.work.rendered_output_bytes,
            "inbound document relation serialized the wrong inverse view",
        )?;

        let cursor_query = DetailedRelationQuery {
            anchor,
            direction: RelationDirection::Outbound,
            relation: Some(references),
            minimum_confidence: ConfidenceClass::Low,
            resolution: RelationResolutionFilter::Resolved,
            include_occurrences: false,
            budget: DetailedRelationBudget::from_graph_limits(GraphLimits::new(
                1,
                5,
                3,
                256 * 1024,
            )?)
            .with_aggregate_limits(Some(10), Some(10), Some(10), None, None, None)?,
            cursor: None,
            content_selection: ContentSelection::Documentation,
        };
        let cursor = load_detailed_relations(&store, &cursor_query, None)?
            .continuation
            .ok_or("classified relation page omitted a continuation")?;
        let mismatched = DetailedRelationQuery {
            cursor: Some(cursor),
            content_selection: ContentSelection::Both,
            ..cursor_query
        };
        require(
            matches!(
                load_detailed_relations(&store, &mismatched, None),
                Err(ServiceError::RelationCursorMismatched { field: "query" })
            ),
            "relation cursor accepted a different content selection",
        )?;
        Ok(())
    }

    #[test]
    fn confidence_and_output_limits_fail_bounded() -> Result<(), Box<dyn Error>> {
        let limits = GraphLimits::new(1, 1, 1, 1)?;
        require(limits.rows() == 1, "graph row limit changed")?;
        let relation = RelationResolution::Unresolved {
            reference: GraphIdentityText::new("missing::target")?,
        };
        require(resolution_rank(&relation) == 1, "unresolved rank changed")?;
        require(
            confidence_rank(ConfidenceClass::Exact) > confidence_rank(ConfidenceClass::Low),
            "confidence rank changed",
        )?;
        let base = DetailedRelationBudget::from_graph_limits(GraphLimits::new(5, 2, 3, 64 * 1024)?);
        let aggregate = base.with_aggregate_limits(
            Some(17),
            Some(13),
            Some(11),
            Some(7),
            Some(128 * 1024),
            Some(2_000),
        )?;
        require(
            aggregate.edges() == 17
                && aggregate.nodes() == 13
                && aggregate.visited() == 11
                && aggregate.occurrences_total() == 7
                && aggregate.intermediate_bytes() == 128 * 1024
                && aggregate.deadline_ms() == 2_000,
            "aggregate relation budget overrides changed",
        )?;
        require(
            base.with_aggregate_limits(None, None, Some(0), None, None, None)
                .is_err(),
            "zero visited-state budget was accepted",
        )?;
        require(
            base.with_aggregate_limits(
                Some(DetailedRelationBudget::MAX_EDGES + 1),
                None,
                None,
                None,
                None,
                None,
            )
            .is_err(),
            "oversized edge budget was accepted",
        )?;
        Ok(())
    }

    /// Return one ordinary test error instead of panicking inside fallible tests.
    /// Collect one relation traversal through its public continuation contract.
    fn collect_relation_pages(
        store: &AtlasStore,
        mut query: DetailedRelationQuery,
        maximum_pages: usize,
    ) -> ServiceResult<(Vec<DetailedRelationRow>, RelationTotalState, u64)> {
        let mut rows = Vec::new();
        for _page in 0..maximum_pages {
            let report = load_detailed_relations(store, &query, None)?;
            rows.extend(report.rows);
            let Some(cursor) = report.continuation else {
                return Ok((rows, report.total, report.pruned_paths));
            };
            query.cursor = Some(cursor);
        }
        Err(ServiceError::InvalidInput(
            "relation traversal did not terminate within the test page ceiling".to_string(),
        ))
    }

    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            return Ok(());
        }
        Err(io::Error::other(message).into())
    }

    fn test_node(path: &str, hash: &str) -> Node {
        classified_test_node(path, hash, ".rs", "rust")
    }

    fn classified_test_node(path: &str, hash: &str, extension: &str, language: &str) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::File,
            parent_path: Some(
                path.rsplit_once('/')
                    .map_or(".", |(parent, _file)| parent)
                    .to_string(),
            ),
            extension: Some(extension.to_string()),
            language: Some(language.to_string()),
            size_bytes: Some(16),
            mtime_ns: Some(1),
            content_hash: Some(hash.to_string()),
        }
    }

    fn test_folder_node(path: &str) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::Folder,
            parent_path: Some(".".to_string()),
            extension: None,
            language: None,
            size_bytes: None,
            mtime_ns: Some(1),
            content_hash: None,
        }
    }
}
