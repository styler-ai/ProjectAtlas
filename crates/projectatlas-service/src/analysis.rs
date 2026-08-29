//! Closed, bounded architecture, impact, and static-trace projections.

mod impact;

#[cfg(test)]
mod analysis_test_observer {
    use std::cell::RefCell;

    /// Named production phase reached by one synchronous analysis request.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(super) enum AnalysisPhaseEvent {
        /// Exact byte ledger passed to the architecture community projection.
        CompositionBudget {
            /// Remaining bytes handed to architecture findings.
            symbol_byte_budget: u64,
            /// Existing architecture findings' append charge.
            existing_finding_append_bytes: u64,
            /// Remaining bytes handed to community findings.
            community_budget: u64,
        },
        /// Induced relationship closure is about to traverse its first bounded frontier.
        Traversal,
        /// Admitted symbol hydration is about to enter bounded storage work.
        SymbolHydration,
        /// Topology and finding composition is about to retain aggregate output state.
        Composition,
        /// Optional dead-code candidate discovery has entered complete-scope work.
        DeadCodeDiscovery,
        /// Adapter-specific output fitting has begun under the retained request control.
        OutputRendering,
    }

    /// Thread-local callback used only while one unit test owns observation.
    type Observer = Box<dyn FnMut(AnalysisPhaseEvent)>;

    thread_local! {
        /// Observer scoped to the synchronous analysis test thread.
        static OBSERVER: RefCell<Option<Observer>> = RefCell::new(None);
    }

    /// Restores a prior nested observer on every return or unwind path.
    struct ObserverGuard {
        /// Observer replaced for the current scope.
        previous: Option<Observer>,
    }

    impl Drop for ObserverGuard {
        fn drop(&mut self) {
            OBSERVER.with(|slot| {
                drop(slot.replace(self.previous.take()));
            });
        }
    }

    /// Run one operation while observing named production analysis phases.
    pub(super) fn observe_analysis_phase<T>(
        observer: impl FnMut(AnalysisPhaseEvent) + 'static,
        operation: impl FnOnce() -> T,
    ) -> T {
        let previous = OBSERVER.with(|slot| slot.replace(Some(Box::new(observer))));
        let _guard = ObserverGuard { previous };
        operation()
    }

    /// Notify the current thread-local observer when one is installed.
    pub(super) fn notify(event: AnalysisPhaseEvent) {
        OBSERVER.with(|slot| {
            if let Some(observer) = slot.borrow_mut().as_mut() {
                observer(event);
            }
        });
    }
}

use super::relations::{
    ExternalRelationIdentity, external_relation_identities, load_detailed_relations,
};
use super::{
    CoverageTrustState, DetailedRelationBudget, DetailedRelationNode, DetailedRelationQuery,
    DetailedRelationReport, DetailedRelationWork, RelationAnchor, RelationDirection,
    RelationNextCall, RelationPurpose, RelationResolutionFilter, RelationTotalState, ServiceError,
    ServiceResult, coverage_trust, selected_project_binding,
};
use impact::{LoadedVcs, digest_vcs_paths, impact_findings, load_vcs_paths};
use projectatlas_core::graph::{
    Completeness, ConfidenceClass, EntitySelector, ExtendedRelationKind, GraphEntity,
    GraphIdentityText, GraphLimitKind, GraphLimits, GraphRelationKind, ProjectInstanceId,
    RelationResolution,
};
use projectatlas_core::language::ContentSelection;
use projectatlas_core::symbols::{CodeSymbol, RelationKind};
use projectatlas_core::{
    CanonicalProjectRoot, IndexCancellation, IndexWorkControl, IndexWorkStage,
};
use projectatlas_db::{
    AtlasStore, DbError, MAX_REPOSITORY_GRAPH_FRONTIER, MAX_SYMBOL_BATCH_DECODED_BYTES,
    MAX_SYMBOL_BATCH_PATHS, MAX_SYMBOL_BATCH_ROWS, RepositoryGraphAdjacencyContinuation,
    RepositoryGraphDirection, RepositoryGraphReadBudget, SymbolBatchReadBudget,
    SymbolBatchReadLimit,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{self, Write};
use std::time::{Duration, Instant};

/// Current closed relation-analysis cursor schema.
const ANALYSIS_CURSOR_VERSION: u16 = 1;
/// Domain separator for the native project-root continuation binding.
const ANALYSIS_ROOT_DOMAIN: &str = "projectatlas:analysis-root:v1";
/// Maximum decoded cursor bytes accepted from an adapter.
const ANALYSIS_CURSOR_MAX_BYTES: usize = 256 * 1024;
/// Maximum nodes retained by one analysis projection.
const MAX_ANALYSIS_NODES: u32 = 512;
/// Maximum edges retained by one analysis projection.
const MAX_ANALYSIS_EDGES: u32 = 2_048;
/// Version of the deterministic architecture-community projection.
const COMMUNITY_ALGORITHM_VERSION: u16 = 1;
/// Version of the stable community ordering and identifier contract.
const COMMUNITY_ORDERING_VERSION: u16 = 1;
/// Maximum asynchronous label-propagation rounds per request.
const COMMUNITY_MAX_ITERATIONS: u32 = 24;
/// Self-vote keeps isolated and weakly connected nodes deterministic.
const COMMUNITY_SELF_WEIGHT: u32 = 1;
/// Conservative duplicate serialized-byte charge for community working state.
const COMMUNITY_WORKING_SET_MULTIPLIER: u64 = 8;
/// Conservative per-node/edge container overhead for community working state.
const COMMUNITY_WORKING_SET_ENTRY_BYTES: u64 = 256;
/// Conservative fixed map, vector, and propagation-state overhead.
const COMMUNITY_WORKING_SET_FIXED_BYTES: u64 = 32 * 1024;
/// Fixed JSON array framing already retained by the analysis composition.
const JSON_ARRAY_FRAMING_BYTES: u64 = 2;
/// Fixed JSON array separator inserted before an appended finding.
const JSON_ARRAY_SEPARATOR_BYTES: u64 = 1;

/// Closed analysis projection selected on the existing relation route.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationAnalysisMode {
    /// Components, communities, cycles, purpose, complexity, and bottleneck candidates.
    Architecture,
    /// Version-control-aware affected nodes and conservative dead-code candidates.
    Impact,
    /// One node-simple static relationship path to an exact target label.
    Trace,
}

/// Version-control scope used by the impact projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GitImpactSelection {
    /// Uncommitted tracked and untracked working-tree changes.
    WorkingTree,
    /// Changes staged in the index.
    Index,
    /// Files changed between two exact revision expressions.
    RevisionRange {
        /// Older revision expression.
        base: String,
        /// Newer revision expression.
        head: String,
    },
}

/// Complete request for one bounded closed analysis projection.
#[derive(Clone, Debug)]
pub struct RelationAnalysisQuery {
    /// Existing normalized relation traversal, filters, budgets, and cursor.
    pub relations: DetailedRelationQuery,
    /// Closed projection to compute over the bounded traversal.
    pub mode: RelationAnalysisMode,
    /// Exact file or symbol selector required by static trace mode.
    pub trace_target: Option<RelationAnchor>,
    /// Optional VCS scope; impact defaults to the working tree.
    pub vcs: Option<GitImpactSelection>,
    /// Include weak communities that exclude containment edges.
    pub include_communities: bool,
    /// Include iterative strongly-connected-component findings.
    pub include_cycles: bool,
    /// Include conservative non-exported dead-code candidates.
    pub include_dead_code: bool,
}

/// Confidence disposition of one analysis finding.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisStatus {
    /// The bounded evidence establishes the stated structural fact.
    Confirmed,
    /// The evidence is useful for review but is not a semantic proof.
    Candidate,
    /// Complete bounded evidence establishes a negative result.
    Absent,
    /// A declared coverage or traversal boundary prevents a safe conclusion.
    Inconclusive,
}

/// Closed finding families emitted by relation analysis.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisFindingKind {
    /// Weak component over every admitted local relation.
    Component,
    /// Weak community with containment edges excluded.
    Community,
    /// Strongly connected dependency cycle.
    DependencyCycle,
    /// Owners share one approved purpose.
    PurposeAlignment,
    /// Connected owners retain conflicting approved purposes.
    PurposeDrift,
    /// Declaration span and graph-degree structural candidate.
    StructuralComplexity,
    /// High-degree or cross-owner dependency junction.
    Bottleneck,
    /// Node affected by the selected VCS path set.
    Impact,
    /// Conservative non-exported declaration with no trusted inbound relation.
    DeadCode,
    /// Node-simple static relationship path.
    StaticTrace,
    /// Ambiguous or unresolved static relation that prevents a closed conclusion.
    ResolutionGap,
}

/// One deterministic bounded analysis finding.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnalysisFinding {
    /// Closed finding family.
    pub kind: AnalysisFindingKind,
    /// Evidence disposition.
    pub status: AnalysisStatus,
    /// Concise interpretation that does not overclaim semantic proof.
    pub summary: String,
    /// Typed reusable nodes with purpose, coverage, and next-call routing.
    pub nodes: Vec<AnalysisNode>,
    /// Optional exact structural metric, such as degree or declaration span.
    pub metric: Option<u64>,
    /// Exact relation evidence when this finding is caused by a resolution gap.
    pub evidence: Option<AnalysisRelationEvidence>,
    /// Deterministic community metadata when this is a community finding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub community: Option<CommunityAnalysis>,
}

/// Convergence state of one bounded label-propagation run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommunityConvergence {
    /// Every node retained its selected label during a complete round.
    Converged,
    /// The fixed iteration ceiling was reached before convergence.
    IterationLimit,
    /// The input coverage or resource envelope did not permit a partition.
    Inconclusive,
}

/// Coverage state attached to one community projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommunityCoverage {
    /// All admitted nodes and resolved local edges were inspected.
    Complete,
    /// A traversal or relation boundary prevented a complete partition.
    Partial,
}

/// One fixed relation-family weight in the v1 community contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CommunityRelationWeight {
    /// Relation family receiving this vote weight.
    pub relation: GraphRelationKind,
    /// Positive vote weight used by label propagation.
    pub weight: u32,
}

/// Versioned limits and input selectors used by one community run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CommunityParameters {
    /// Label-propagation algorithm version.
    pub algorithm_version: u16,
    /// Stable node, label, and identifier ordering version.
    pub ordering_version: u16,
    /// Maximum label-propagation rounds.
    pub max_iterations: u32,
    /// Maximum nodes admitted to one projection.
    pub node_limit: u32,
    /// Maximum edges admitted to one projection.
    pub edge_limit: u32,
    /// Maximum encoded output bytes inherited from the bounded analysis route.
    pub output_bytes: u32,
    /// Optional detailed relation-family filter.
    pub relation: Option<GraphRelationKind>,
}

/// One resolved relation retained as community evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CommunityEdgeEvidence {
    /// Canonical source entity identity.
    pub source: String,
    /// Canonical target entity identity.
    pub target: String,
    /// Resolved local relation family.
    pub relation: GraphRelationKind,
    /// Vote weight applied to this edge.
    pub weight: u32,
}

/// Complete non-persistent metadata for one deterministic community.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CommunityAnalysis {
    /// Stable ID derived from the versioned parameters and sorted members.
    pub id: String,
    /// Exact members with their existing source, purpose, and coverage evidence.
    pub members: Vec<AnalysisNode>,
    /// Resolved local edges induced by these members.
    pub evidence: Vec<CommunityEdgeEvidence>,
    /// Fixed relation weights admitted by this run.
    pub weights: Vec<CommunityRelationWeight>,
    /// Versioned algorithm and resource parameters.
    pub parameters: CommunityParameters,
    /// Number of completed propagation rounds.
    pub iteration: u32,
    /// Typed convergence disposition.
    pub convergence: CommunityConvergence,
    /// Typed graph coverage disposition.
    pub coverage: CommunityCoverage,
    /// Whether a fixed node, edge, or output bound prevented a complete result.
    pub truncated: bool,
}

/// One reusable analysis node retaining its authoritative navigation evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnalysisNode {
    /// Exact entity, current approved purpose, and path coverage.
    pub node: DetailedRelationNode,
    /// Existing public call that can consume this selector directly.
    pub next_call: Option<RelationNextCall>,
}

/// Exact ambiguous or unresolved relation retained inside its fitted finding.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnalysisRelationEvidence {
    /// Full normalized relation, including resolution reference and confidence.
    pub relation: projectatlas_core::graph::LogicalRelation,
    /// Exact detailed-relation request that can inspect this gap directly.
    pub next_call: Option<RelationAnalysisNextCall>,
}

/// Exact existing relation call that can inspect one resolution gap directly.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RelationAnalysisNextCall {
    /// Exact local source anchor.
    pub anchor: RelationAnchor,
    /// Direction that retains the source-side unresolved or ambiguous relation.
    pub direction: RelationDirection,
    /// Exact legacy or extended relation family.
    pub relation: GraphRelationKind,
    /// Exact resolution class retained by the finding.
    pub resolution: RelationResolutionFilter,
    /// Minimum confidence that retains the exact relation trust class.
    pub minimum_confidence: ConfidenceClass,
    /// Explicit classified-content restriction retained across the next call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_selection: Option<ContentSelection>,
}

/// Typed VCS availability retained in impact responses.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum VcsImpact {
    /// Architecture and trace modes did not request VCS evidence.
    NotRequested,
    /// Git returned a bounded normalized path set.
    Available {
        /// Exact selector used by the request.
        selection: GitImpactSelection,
        /// Exact number of normalized repository-relative changed paths.
        changed_path_count: u64,
    },
    /// Git is unavailable, the root is not a worktree, or the command failed.
    Unavailable {
        /// Exact selector that could not be evaluated.
        selection: GitImpactSelection,
        /// Bounded actionable reason.
        reason: String,
    },
}

/// Exact work performed in addition to the existing detailed traversal.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct RelationAnalysisWork {
    /// Existing detailed traversal and hydration work.
    pub relations: DetailedRelationWork,
    /// Adjacency rows inspected while closing edges among admitted nodes.
    pub closure_inspected_edges: u32,
    /// Raw `SQLite` bytes decoded by induced-edge closure reads.
    pub closure_decoded_bytes: u64,
    /// Git stdout bytes retained while normalizing impact evidence.
    pub vcs_retained_bytes: u64,
    /// Number of admitted entities analyzed.
    pub analyzed_nodes: u32,
    /// Number of unique admitted local edges analyzed.
    pub analyzed_edges: u32,
    /// Symbols retained by bounded per-file exact-identity hydration.
    pub hydrated_symbols: u32,
    /// Decoded symbol and range-index bytes inspected during finding computation.
    pub hydrated_symbol_bytes: u64,
    /// Whether symbol path, row, or byte limits omitted candidates.
    pub symbol_hydration_truncated: bool,
    /// Exact serialized-equivalent analysis-owned bytes retained at return.
    pub retained_composition_bytes: u64,
    /// Peak aggregate relation, closure, VCS, symbol, topology,
    /// community-working-set, and finding bytes.
    pub peak_intermediate_bytes: u64,
    /// Whether composition limits omitted supported analysis projections.
    pub composition_truncated: bool,
    /// Exact bytes emitted by the selected adapter envelope.
    pub rendered_output_bytes: u64,
}

/// Bounded analysis report returned through the existing CLI and MCP relation route.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RelationAnalysisReport {
    /// Closed projection selected by the caller.
    pub mode: RelationAnalysisMode,
    /// Existing exact relation anchor with authoritative purpose and coverage.
    pub anchor: DetailedRelationNode,
    /// Complete graph generation used by every relation row.
    pub generation: projectatlas_core::IndexGeneration,
    /// Accepted purpose revision used by every projected node.
    pub authored_purpose_revision: u64,
    /// Existing generation- and query-bound traversal continuation.
    pub continuation: Option<String>,
    /// Findings retained in this fitted response.
    pub returned: u32,
    /// Exact or lower-bound finding cardinality before output-prefix fitting.
    pub total: RelationTotalState,
    /// Whether traversal, closure, or output fitting omitted supported evidence.
    pub truncated: bool,
    /// Stable unique hard limits reached by traversal, closure, or rendering.
    pub reached_limits: Vec<GraphLimitKind>,
    /// Typed VCS evidence for impact mode.
    pub vcs: VcsImpact,
    /// Exact bounded work retained by this analysis.
    pub work: RelationAnalysisWork,
    /// Deterministically ordered structural findings.
    pub findings: Vec<AnalysisFinding>,
}

/// Unrendered analysis report that can fit an adapter's exact final envelope.
pub struct RelationAnalysisDraft {
    /// Fully hydrated maximum candidate report.
    report: RelationAnalysisReport,
    /// Exact encoded-output ceiling requested by the caller.
    output_bytes: u32,
    /// Result-defining traversal budget bound into continuations.
    budget: DetailedRelationBudget,
    /// Normalized query identity bound into continuations.
    cursor_binding: AnalysisCursorBinding,
    /// Repository and authored-purpose generation bound into continuations.
    cursor_snapshot: AnalysisCursorSnapshot,
    /// Relation cursor replayed when output fitting omits finding rows.
    replay_relation_cursor: Option<String>,
    /// Findings emitted before this draft began.
    finding_offset: u32,
    /// Optional normalized VCS evidence identity.
    vcs_digest: Option<[u8; 32]>,
    /// Exact external identities reached by the bounded detailed traversal.
    external_relation_identities: BTreeSet<ExternalRelationIdentity>,
    /// Shared request cancellation and deadline retained through rendering.
    control: IndexWorkControl,
}

impl RelationAnalysisDraft {
    /// Fully hydrated report before adapter-specific output fitting.
    #[must_use]
    pub const fn candidate_report(&self) -> &RelationAnalysisReport {
        &self.report
    }

    /// Borrow the exact request control retained through output rendering.
    pub(super) const fn control(&self) -> &IndexWorkControl {
        &self.control
    }

    /// Move the call-scoped rendezvous identities out before output fitting.
    pub(super) fn take_external_relation_identities(
        &mut self,
    ) -> BTreeSet<ExternalRelationIdentity> {
        std::mem::take(&mut self.external_relation_identities)
    }

    /// Fit one complete adapter envelope by retaining the largest finding prefix.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when rendering fails or even the empty finding
    /// envelope exceeds the declared output or aggregate intermediate ceiling.
    pub fn fit_output<F, E, O>(self, mut encode: F) -> Result<(RelationAnalysisReport, O), E>
    where
        F: FnMut(&RelationAnalysisReport, &IndexWorkControl) -> Result<O, E>,
        E: From<ServiceError>,
        O: AsRef<[u8]>,
    {
        check_control(Some(&self.control)).map_err(E::from)?;
        #[cfg(test)]
        analysis_test_observer::notify(analysis_test_observer::AnalysisPhaseEvent::OutputRendering);
        let original_report_bytes =
            serialized_bytes_controlled(&self.report, Some(&self.control)).map_err(E::from)?;
        let construction_peak = self.report.work.peak_intermediate_bytes;
        let mut low = 0;
        let mut high = self.report.findings.len();
        let mut best = None;
        let mut output_limited = false;
        let mut intermediate_limited = false;
        let mut empty_output_oversized = false;
        let mut empty_intermediate_oversized = false;
        while low <= high {
            check_control(Some(&self.control)).map_err(E::from)?;
            let middle = low + (high - low) / 2;
            let fit_limits = [
                output_limited.then_some(GraphLimitKind::OutputBytes),
                intermediate_limited.then_some(GraphLimitKind::IntermediateBytes),
            ];
            let mut candidate =
                analysis_prefix(&self.report, middle, fit_limits.into_iter().flatten());
            if middle < self.report.findings.len() {
                let middle = u32::try_from(middle).map_err(|_overflow| {
                    E::from(ServiceError::InvalidInput(
                        "analysis finding offset overflowed".to_string(),
                    ))
                })?;
                let finding_offset = self.finding_offset.checked_add(middle).ok_or_else(|| {
                    E::from(ServiceError::InvalidInput(
                        "analysis finding offset overflowed".to_string(),
                    ))
                })?;
                candidate.continuation = Some(
                    encode_analysis_cursor(
                        self.replay_relation_cursor.as_deref(),
                        finding_offset,
                        &self.cursor_binding,
                        self.cursor_snapshot,
                        self.vcs_digest,
                        self.budget,
                    )
                    .map_err(E::from)?,
                );
            }
            check_control(Some(&self.control)).map_err(E::from)?;
            let mut encoded = encode(&candidate, &self.control)?;
            check_control(Some(&self.control)).map_err(E::from)?;
            let mut stable = false;
            for _ in 0..8 {
                check_control(Some(&self.control)).map_err(E::from)?;
                let rendered = u64::try_from(encoded.as_ref().len()).map_err(|source| {
                    E::from(ServiceError::InvalidInput(format!(
                        "analysis rendered byte count overflowed: {source}"
                    )))
                })?;
                let candidate_report_bytes =
                    serialized_bytes_controlled(&candidate, Some(&self.control))
                        .map_err(E::from)?;
                let fitting_peak = original_report_bytes
                    .checked_add(candidate_report_bytes)
                    .and_then(|bytes| bytes.checked_add(rendered))
                    .ok_or_else(|| {
                        E::from(ServiceError::InvalidInput(
                            "analysis output fitting byte count overflowed".to_string(),
                        ))
                    })?;
                let peak = construction_peak.max(fitting_peak);
                if candidate.work.rendered_output_bytes == rendered
                    && candidate.work.peak_intermediate_bytes == peak
                {
                    stable = true;
                    break;
                }
                candidate.work.rendered_output_bytes = rendered;
                candidate.work.peak_intermediate_bytes = peak;
                drop(encoded);
                encoded = encode(&candidate, &self.control)?;
                check_control(Some(&self.control)).map_err(E::from)?;
            }
            if !stable {
                return Err(E::from(ServiceError::InvalidInput(
                    "analysis output accounting did not stabilize".to_string(),
                )));
            }
            let output_fits = encoded.as_ref().len() <= self.output_bytes as usize;
            let intermediate_fits =
                candidate.work.peak_intermediate_bytes <= self.budget.intermediate_bytes();
            if output_fits && intermediate_fits {
                best = Some((candidate, encoded));
                low = middle.saturating_add(1);
            } else {
                if middle == 0 {
                    empty_output_oversized = !output_fits;
                    empty_intermediate_oversized = !intermediate_fits;
                    break;
                }
                let newly_output_limited = !output_fits && !output_limited;
                let newly_intermediate_limited = !intermediate_fits && !intermediate_limited;
                output_limited |= !output_fits;
                intermediate_limited |= !intermediate_fits;
                if newly_output_limited || newly_intermediate_limited {
                    best = None;
                    low = 0;
                }
                high = middle - 1;
            }
        }
        check_control(Some(&self.control)).map_err(E::from)?;
        best.ok_or_else(|| {
            let message = if empty_intermediate_oversized {
                "empty analysis envelope exceeds the aggregate intermediate-byte budget"
            } else if empty_output_oversized {
                "graph output byte limit is too small for the empty analysis envelope"
            } else if intermediate_limited {
                "analysis output fitting exceeds the aggregate intermediate-byte budget"
            } else {
                "graph output byte limit is too small for the empty analysis envelope"
            };
            E::from(ServiceError::InvalidInput(message.to_string()))
        })
    }
}

#[derive(Clone, Serialize)]
/// One local normalized edge admitted to topology algorithms.
struct LocalEdge {
    /// Canonical source entity identity.
    source: String,
    /// Canonical target entity identity.
    target: String,
    /// Closed relation family.
    kind: GraphRelationKind,
    /// Whether the owning relation evidence is complete.
    complete: bool,
}

#[derive(Default)]
/// Analysis-owned work accumulated beyond the detailed relation traversal.
struct SupplementalWork {
    /// Persisted symbols retained for declaration-aware findings.
    hydrated_symbols: u32,
    /// Decoded rows plus serialized-equivalent range-index bytes retained.
    hydrated_symbol_bytes: u64,
    /// Peak symbol hydration bytes including request-path auxiliaries.
    hydrated_symbol_peak_bytes: u64,
    /// Whether a declared hydration bound omitted candidates.
    symbol_hydration_truncated: bool,
    /// Stable limits reached by supplemental work.
    reached_limits: Vec<GraphLimitKind>,
    /// Analysis-owned bytes retained in the composed report.
    retained_composition_bytes: u64,
    /// Whether composition fitting omitted supported evidence.
    composition_truncated: bool,
    /// Conservative transient bytes charged while constructing community state.
    community_working_set_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Normalized result-defining request identity encoded in every cursor.
struct AnalysisCursorBinding {
    /// Digest of the selected canonical project root.
    root_digest: [u8; 32],
    /// Exact normalized traversal anchor.
    anchor: RelationAnchor,
    /// Traversal direction.
    direction: RelationDirection,
    /// Optional relation-family filter.
    relation: Option<GraphRelationKind>,
    /// Minimum admitted trust class.
    minimum_confidence: ConfidenceClass,
    /// Resolution-class filter.
    resolution: RelationResolutionFilter,
    /// Classified-content restriction that changes admitted topology.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content_selection: Option<ContentSelection>,
    /// Closed feature selections that affect results.
    options: AnalysisCursorOptions,
    /// Exact traversal resource envelope.
    budget: DetailedRelationBudget,
    /// Detailed relation algorithm contract version.
    algorithm_version: u16,
    /// Detailed relation ordering contract version.
    ordering_version: u16,
    /// Closed analysis projection.
    mode: RelationAnalysisMode,
    /// Optional exact trace target.
    trace_target: Option<RelationAnchor>,
    /// Optional VCS selector.
    vcs: Option<GitImpactSelection>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// Typed inclusion state for cursor-bound optional analysis behavior.
enum AnalysisFeatureSelection {
    /// Feature output is excluded.
    Excluded,
    /// Feature output is included.
    Included,
}

impl From<bool> for AnalysisFeatureSelection {
    fn from(included: bool) -> Self {
        if included {
            Self::Included
        } else {
            Self::Excluded
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Closed feature selections grouped into one cursor option contract.
struct AnalysisCursorOptions {
    /// Retain exact relation occurrence spans.
    relation_occurrences: AnalysisFeatureSelection,
    /// Emit weak community candidates.
    communities: AnalysisFeatureSelection,
    /// Emit dependency-cycle candidates.
    cycles: AnalysisFeatureSelection,
    /// Emit conservative dead-code candidates.
    dead_code: AnalysisFeatureSelection,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Repository state that must remain stable while a cursor is replayed.
struct AnalysisCursorSnapshot {
    /// Stable selected project instance.
    project: ProjectInstanceId,
    /// Complete repository-graph generation.
    generation: projectatlas_core::IndexGeneration,
    /// Accepted authored-purpose revision.
    authored_purpose_revision: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Bounded continuation for relation traversal and finding-prefix replay.
struct AnalysisCursor {
    /// Cursor schema version.
    version: u16,
    /// Result-defining normalized request identity.
    binding: AnalysisCursorBinding,
    /// Repository state captured by the first page.
    snapshot: AnalysisCursorSnapshot,
    /// Optional underlying relation traversal continuation.
    relation_cursor: Option<String>,
    /// Findings emitted before the next fitted page.
    finding_offset: u32,
    /// Optional normalized VCS evidence digest.
    vcs_digest: Option<[u8; 32]>,
}

/// Load one closed analysis view over the existing bounded relation service.
///
/// # Errors
///
/// Returns the existing detailed-relation errors plus bounded VCS, closure, and
/// output validation failures.
pub fn load_relation_analysis(
    store: &AtlasStore,
    query: &RelationAnalysisQuery,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<RelationAnalysisDraft> {
    load_relation_analysis_with_closure_deadline(store, query, control, None, false)
}

/// Load analysis while retaining its bounded external traversal identities.
pub(super) fn load_relation_analysis_for_federation(
    store: &AtlasStore,
    query: &RelationAnalysisQuery,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<RelationAnalysisDraft> {
    load_relation_analysis_with_closure_deadline(store, query, control, None, true)
}

/// Load analysis with an optional earlier closure-stage deadline ceiling.
fn load_relation_analysis_with_closure_deadline(
    store: &AtlasStore,
    query: &RelationAnalysisQuery,
    control: Option<&IndexWorkControl>,
    closure_deadline_ceiling: Option<Instant>,
    retain_external_relation_identities: bool,
) -> ServiceResult<RelationAnalysisDraft> {
    validate_analysis_query(query)?;
    let started = Instant::now();
    let deadline = started
        .checked_add(Duration::from_millis(query.relations.budget.deadline_ms()))
        .unwrap_or(started);
    let deadline = control
        .and_then(IndexWorkControl::deadline)
        .map_or(deadline, |caller_deadline| caller_deadline.min(deadline));
    let analysis_control = control.map_or_else(
        || IndexWorkControl::with_deadline(IndexCancellation::new(), deadline),
        |caller| {
            caller.with_timeout_ceiling(deadline.saturating_duration_since(caller.started_at()))
        },
    );
    let control = Some(&analysis_control);
    check_control(control)?;
    let selected_binding = selected_project_binding(store)?;
    let cursor_binding = analysis_cursor_binding(query, &selected_binding.project_root_identity)?;
    let decoded_cursor = query
        .relations
        .cursor
        .as_deref()
        .map(|cursor| decode_analysis_cursor(cursor, &cursor_binding))
        .transpose()?;
    let mut relation_query = query.relations.clone();
    relation_query.budget = bounded_analysis_budget(query.relations.budget)?;
    relation_query.cursor = decoded_cursor
        .as_ref()
        .and_then(|cursor| cursor.relation_cursor.clone());
    let replay_relation_cursor = relation_query.cursor.clone();
    let finding_offset = decoded_cursor
        .as_ref()
        .map_or(0, |cursor| cursor.finding_offset);
    let relations = load_detailed_relations(store, &relation_query, control)?;
    let external_relation_identities = if retain_external_relation_identities {
        external_relation_identities(&relations)
    } else {
        BTreeSet::new()
    };
    let external_relation_identity_bytes =
        serialized_bytes_controlled(&external_relation_identities, control)?;
    let cursor_snapshot = AnalysisCursorSnapshot {
        project: relations.anchor.entity.key().project(),
        generation: relations.generation,
        authored_purpose_revision: relations.authored_purpose_revision,
    };
    if decoded_cursor
        .as_ref()
        .is_some_and(|cursor| cursor.snapshot != cursor_snapshot)
    {
        return Err(ServiceError::RelationCursorStale {
            field: "analysis snapshot",
        });
    }
    check_control(control)?;
    let mut nodes = collect_nodes(&relations, control)?;
    let mut edges = collect_report_edges(&relations, control)?;
    let mut closure_query = query.clone();
    closure_query.relations = relation_query;
    let closure = close_induced_edges(
        store,
        &closure_query,
        &relations.work,
        closure_deadline_ceiling.map_or(deadline, |ceiling| ceiling.min(deadline)),
        &nodes,
        &mut edges,
        control,
    )?;
    check_control(control)?;
    let evidence_complete =
        relation_evidence_complete(&relations, &nodes, &edges, query, &closure, false);
    let community_evidence_complete =
        relation_evidence_complete(&relations, &nodes, &edges, query, &closure, true);
    let dead_code_scope_complete = dead_code_scope_complete(&relations, query);
    check_control(control)?;
    let vcs_load = if query.mode == RelationAnalysisMode::Impact {
        let selection = query.vcs.clone().unwrap_or(GitImpactSelection::WorkingTree);
        load_vcs_paths(
            selected_binding.project_root_identity.as_path(),
            selection,
            query.relations.budget.intermediate_bytes().saturating_sub(
                relations
                    .work
                    .intermediate_bytes
                    .saturating_add(closure.decoded_bytes)
                    .saturating_add(external_relation_identity_bytes),
            ),
            deadline,
            control,
        )
    } else {
        LoadedVcs {
            report: VcsImpact::NotRequested,
            changed_paths: Vec::new(),
            retained_bytes: 0,
        }
    };
    let vcs = vcs_load.report;
    let vcs_digest = (query.mode == RelationAnalysisMode::Impact)
        .then(|| digest_vcs_paths(&vcs_load.changed_paths, control))
        .transpose()?;
    if decoded_cursor
        .as_ref()
        .is_some_and(|cursor| cursor.vcs_digest != vcs_digest)
    {
        return Err(ServiceError::RelationCursorStale {
            field: "VCS evidence",
        });
    }
    let mut supplemental_work = SupplementalWork::default();
    let analysis_allowance = query.relations.budget.intermediate_bytes().saturating_sub(
        relations
            .work
            .intermediate_bytes
            .saturating_add(closure.decoded_bytes)
            .saturating_add(external_relation_identity_bytes),
    );
    let projection_allowance = analysis_allowance.saturating_sub(vcs_load.retained_bytes);
    #[cfg(test)]
    analysis_test_observer::notify(analysis_test_observer::AnalysisPhaseEvent::Composition);
    let mut topology_bytes =
        serialized_bytes_controlled(&(nodes.values().collect::<Vec<_>>(), &edges), control)?;
    let mut gaps = resolution_gap_findings(&relations, control)?;
    gaps.extend(closure.resolution_gaps.iter().cloned());
    check_control(control)?;
    gaps.sort_by(|left, right| resolution_gap_identity(left).cmp(resolution_gap_identity(right)));
    gaps.dedup_by(|left, right| resolution_gap_identity(left) == resolution_gap_identity(right));
    check_control(control)?;
    let gap_bytes = serialized_bytes_controlled(&gaps, control)?;
    let projection_safe = topology_bytes.saturating_add(gap_bytes) <= projection_allowance;
    let mut findings = if projection_safe {
        gaps
    } else {
        supplemental_work.composition_truncated = true;
        push_limit(
            &mut supplemental_work.reached_limits,
            GraphLimitKind::IntermediateBytes,
        );
        edges.clear();
        nodes.retain(|_, node| node.entity.key() == relations.anchor.entity.key());
        topology_bytes =
            serialized_bytes_controlled(&(nodes.values().collect::<Vec<_>>(), &edges), control)?;
        vec![AnalysisFinding {
            kind: AnalysisFindingKind::Component,
            status: AnalysisStatus::Inconclusive,
            summary: "analysis composition crossed the shared intermediate-byte budget".to_string(),
            nodes: vec![analysis_node(&relations.anchor)],
            metric: None,
            evidence: None,
            community: None,
        }]
    };
    let initial_finding_bytes = serialized_bytes_controlled(&findings, control)?;
    let initial_finding_count = findings.len();
    let symbol_byte_budget = projection_allowance
        .saturating_sub(topology_bytes)
        .saturating_sub(initial_finding_bytes);
    if projection_safe {
        findings.extend(match query.mode {
            RelationAnalysisMode::Architecture => architecture_findings(
                store,
                &nodes,
                &edges,
                evidence_complete,
                community_evidence_complete,
                query,
                symbol_byte_budget,
                initial_finding_count,
                &mut supplemental_work,
                control,
            )?,
            RelationAnalysisMode::Impact => impact_findings(
                store,
                &nodes,
                &edges,
                evidence_complete,
                dead_code_scope_complete,
                &vcs,
                &vcs_load.changed_paths,
                query,
                symbol_byte_budget,
                &mut supplemental_work,
                control,
            )?,
            RelationAnalysisMode::Trace => {
                trace_findings(&relations, query.trace_target.as_ref(), evidence_complete)?
            }
        });
    }
    let generated_finding_count = u32::try_from(findings.len()).map_err(|_overflow| {
        ServiceError::InvalidInput("analysis finding count overflowed".to_string())
    })?;
    let mut finding_bytes = serialized_bytes_controlled(&findings, control)?;
    let generated_composition_bytes = vcs_load
        .retained_bytes
        .saturating_add(topology_bytes)
        .saturating_add(finding_bytes);
    supplemental_work.retained_composition_bytes = vcs_load
        .retained_bytes
        .saturating_add(topology_bytes)
        .saturating_add(finding_bytes);
    while supplemental_work.retained_composition_bytes > analysis_allowance && findings.len() > 1 {
        check_control(control)?;
        findings.pop();
        supplemental_work.composition_truncated = true;
        push_limit(
            &mut supplemental_work.reached_limits,
            GraphLimitKind::IntermediateBytes,
        );
        finding_bytes = serialized_bytes_controlled(&findings, control)?;
        supplemental_work.retained_composition_bytes = vcs_load
            .retained_bytes
            .saturating_add(topology_bytes)
            .saturating_add(finding_bytes);
    }
    if supplemental_work.retained_composition_bytes > analysis_allowance {
        findings.clear();
        supplemental_work.composition_truncated = true;
        push_limit(
            &mut supplemental_work.reached_limits,
            GraphLimitKind::IntermediateBytes,
        );
        finding_bytes = serialized_bytes_controlled(&findings, control)?;
        supplemental_work.retained_composition_bytes = vcs_load
            .retained_bytes
            .saturating_add(topology_bytes)
            .saturating_add(finding_bytes);
    }
    let hydration_peak = relations
        .work
        .intermediate_bytes
        .saturating_add(closure.decoded_bytes)
        .saturating_add(vcs_load.retained_bytes)
        .saturating_add(topology_bytes)
        .saturating_add(initial_finding_bytes)
        .saturating_add(external_relation_identity_bytes)
        .saturating_add(supplemental_work.hydrated_symbol_peak_bytes);
    let generation_peak = relations
        .work
        .intermediate_bytes
        .saturating_add(closure.decoded_bytes)
        .saturating_add(external_relation_identity_bytes)
        .saturating_add(generated_composition_bytes)
        .saturating_add(supplemental_work.community_working_set_bytes);
    let final_peak = relations
        .work
        .intermediate_bytes
        .saturating_add(closure.decoded_bytes)
        .saturating_add(supplemental_work.retained_composition_bytes)
        .saturating_add(external_relation_identity_bytes);
    let peak_intermediate_bytes = hydration_peak.max(generation_peak).max(final_peak);
    let community_projection_truncated = findings.iter().any(|finding| {
        finding
            .community
            .as_ref()
            .is_some_and(|community| community.truncated)
    });
    let full_finding_count = u32::try_from(findings.len()).map_err(|_overflow| {
        ServiceError::InvalidInput("analysis finding count overflowed".to_string())
    })?;
    if finding_offset > full_finding_count {
        return Err(ServiceError::RelationCursorInvalid {
            reason: "analysis finding offset exceeds the recomputed page",
        });
    }
    findings.drain(..finding_offset as usize);
    let mut reached_limits = relations.reached_limits.clone();
    if !closure.complete {
        push_limit(
            &mut reached_limits,
            if closure.deadline_reached {
                GraphLimitKind::Deadline
            } else {
                GraphLimitKind::Edges
            },
        );
    }
    for limit in &supplemental_work.reached_limits {
        push_limit(&mut reached_limits, *limit);
    }
    let work = RelationAnalysisWork {
        relations: relations.work.clone(),
        closure_inspected_edges: closure.inspected_edges,
        closure_decoded_bytes: closure.decoded_bytes,
        vcs_retained_bytes: vcs_load.retained_bytes,
        analyzed_nodes: u32::try_from(nodes.len()).unwrap_or(u32::MAX),
        analyzed_edges: u32::try_from(edges.len()).unwrap_or(u32::MAX),
        hydrated_symbols: supplemental_work.hydrated_symbols,
        hydrated_symbol_bytes: supplemental_work.hydrated_symbol_bytes,
        symbol_hydration_truncated: supplemental_work.symbol_hydration_truncated,
        retained_composition_bytes: supplemental_work.retained_composition_bytes,
        peak_intermediate_bytes,
        composition_truncated: supplemental_work.composition_truncated,
        rendered_output_bytes: 0,
    };
    let analysis_truncated = (!evidence_complete && relations.truncated)
        || !closure.complete
        || supplemental_work.symbol_hydration_truncated
        || supplemental_work.composition_truncated
        || community_projection_truncated;
    let returned = u32::try_from(findings.len()).unwrap_or(u32::MAX);
    let total = if analysis_truncated {
        RelationTotalState::AtLeast(u64::from(generated_finding_count))
    } else {
        RelationTotalState::Exact(u64::from(full_finding_count))
    };
    let continuation = if evidence_complete {
        None
    } else {
        relations
            .continuation
            .as_deref()
            .map(|cursor| {
                encode_analysis_cursor(
                    Some(cursor),
                    0,
                    &cursor_binding,
                    cursor_snapshot,
                    vcs_digest,
                    query.relations.budget,
                )
            })
            .transpose()?
    };
    let report = RelationAnalysisReport {
        mode: query.mode,
        anchor: relations.anchor,
        generation: relations.generation,
        authored_purpose_revision: relations.authored_purpose_revision,
        continuation,
        returned,
        total,
        truncated: analysis_truncated,
        reached_limits,
        vcs,
        work,
        findings,
    };
    nodes.clear();
    check_control(control)?;
    Ok(RelationAnalysisDraft {
        report,
        output_bytes: query.relations.budget.output_bytes(),
        budget: query.relations.budget,
        cursor_binding,
        cursor_snapshot,
        replay_relation_cursor,
        finding_offset,
        vcs_digest,
        external_relation_identities,
        control: analysis_control,
    })
}

/// Validate closed mode, selector, option, and budget combinations.
fn validate_analysis_query(query: &RelationAnalysisQuery) -> ServiceResult<()> {
    if query.mode == RelationAnalysisMode::Trace {
        let Some(target) = query.trace_target.as_ref() else {
            return Err(ServiceError::InvalidInput(
                "analysis trace requires an exact file or symbol target".to_string(),
            ));
        };
        if matches!(
            target,
            RelationAnchor::Symbol {
                symbol_kind: None,
                ..
            }
        ) || matches!(
            target,
            RelationAnchor::Symbol {
                signature: None,
                ..
            }
        ) {
            return Err(ServiceError::InvalidInput(
                "analysis trace symbol targets require exact kind and signature".to_string(),
            ));
        }
    } else if query.trace_target.is_some() {
        return Err(ServiceError::InvalidInput(
            "trace_target is valid only for static trace analysis".to_string(),
        ));
    }
    if query.mode != RelationAnalysisMode::Impact && query.vcs.is_some() {
        return Err(ServiceError::InvalidInput(
            "VCS selection is valid only for impact analysis".to_string(),
        ));
    }
    if query.mode != RelationAnalysisMode::Architecture
        && (query.include_communities || query.include_cycles)
    {
        return Err(ServiceError::InvalidInput(
            "community and cycle controls are valid only for architecture analysis".to_string(),
        ));
    }
    if query.mode != RelationAnalysisMode::Impact && query.include_dead_code {
        return Err(ServiceError::InvalidInput(
            "dead-code controls are valid only for impact analysis".to_string(),
        ));
    }
    Ok(())
}

/// Clamp the existing traversal budget to analysis product ceilings.
fn bounded_analysis_budget(
    budget: DetailedRelationBudget,
) -> ServiceResult<DetailedRelationBudget> {
    let limits = GraphLimits::new(
        budget.page_rows().min(MAX_ANALYSIS_NODES),
        budget.occurrences_per_relation(),
        budget.depth(),
        budget.output_bytes(),
    )
    .map_err(|error| ServiceError::InvalidInput(error.to_string()))?;
    DetailedRelationBudget::from_graph_limits(limits).with_aggregate_limits(
        Some(budget.edges().min(MAX_ANALYSIS_EDGES)),
        Some(budget.nodes().min(MAX_ANALYSIS_NODES)),
        Some(budget.visited().min(MAX_ANALYSIS_NODES)),
        Some(budget.occurrences_total()),
        Some(budget.intermediate_bytes()),
        Some(budget.deadline_ms()),
    )
}

/// Normalize every result-defining request field for cursor identity.
fn analysis_cursor_binding(
    query: &RelationAnalysisQuery,
    project_root: &CanonicalProjectRoot,
) -> ServiceResult<AnalysisCursorBinding> {
    Ok(AnalysisCursorBinding {
        root_digest: super::canonical_root_digest(ANALYSIS_ROOT_DOMAIN, project_root)?,
        anchor: query.relations.anchor.clone(),
        direction: query.relations.direction,
        relation: query.relations.relation,
        minimum_confidence: query.relations.minimum_confidence,
        resolution: query.relations.resolution,
        content_selection: query
            .relations
            .content_selection
            .explicit_value()
            .map(|_| query.relations.content_selection),
        options: AnalysisCursorOptions {
            relation_occurrences: query.relations.include_occurrences.into(),
            communities: query.include_communities.into(),
            cycles: query.include_cycles.into(),
            dead_code: query.include_dead_code.into(),
        },
        budget: query.relations.budget,
        algorithm_version: ANALYSIS_CURSOR_VERSION,
        ordering_version: 1,
        mode: query.mode,
        trace_target: query.trace_target.clone(),
        vcs: (query.mode == RelationAnalysisMode::Impact)
            .then(|| query.vcs.clone().unwrap_or(GitImpactSelection::WorkingTree)),
    })
}

/// Decode and validate one analysis continuation against its request.
fn decode_analysis_cursor(
    encoded: &str,
    expected: &AnalysisCursorBinding,
) -> ServiceResult<AnalysisCursor> {
    if encoded.is_empty() || encoded.len() > ANALYSIS_CURSOR_MAX_BYTES {
        return Err(ServiceError::RelationCursorInvalid {
            reason: "analysis cursor length is empty or above the product ceiling",
        });
    }
    let cursor: AnalysisCursor = serde_json::from_str(encoded).map_err(|_malformed| {
        ServiceError::RelationCursorInvalid {
            reason: "analysis cursor JSON is malformed or contains unknown fields",
        }
    })?;
    if cursor.version != ANALYSIS_CURSOR_VERSION {
        return Err(ServiceError::RelationCursorStale {
            field: "analysis algorithm version",
        });
    }
    if cursor.binding != *expected {
        return Err(ServiceError::RelationCursorMismatched {
            field: "analysis query",
        });
    }
    Ok(cursor)
}

/// Encode one bounded analysis continuation.
fn encode_analysis_cursor(
    relation_cursor: Option<&str>,
    finding_offset: u32,
    binding: &AnalysisCursorBinding,
    snapshot: AnalysisCursorSnapshot,
    vcs_digest: Option<[u8; 32]>,
    budget: DetailedRelationBudget,
) -> ServiceResult<String> {
    let encoded = serde_json::to_string(&AnalysisCursor {
        version: ANALYSIS_CURSOR_VERSION,
        binding: binding.clone(),
        snapshot,
        relation_cursor: relation_cursor.map(str::to_string),
        finding_offset,
        vcs_digest,
    })?;
    if encoded.len() > ANALYSIS_CURSOR_MAX_BYTES
        || encoded.len() > budget.intermediate_bytes() as usize
    {
        return Err(ServiceError::RelationCursorInvalid {
            reason: "encoded analysis cursor exceeds the intermediate-state ceiling",
        });
    }
    Ok(encoded)
}

/// Collect unique non-external nodes from a detailed relation page.
fn collect_nodes(
    report: &DetailedRelationReport,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<BTreeMap<String, DetailedRelationNode>> {
    let mut nodes = BTreeMap::new();
    insert_node(&mut nodes, &report.anchor);
    for row in &report.rows {
        check_control(control)?;
        insert_node(&mut nodes, &row.source);
        if let Some(target) = &row.target {
            insert_node(&mut nodes, target);
        }
        for node in &row.path {
            insert_node(&mut nodes, node);
        }
    }
    Ok(nodes)
}

/// Project unresolved and ambiguous relation rows into typed findings.
fn resolution_gap_findings(
    report: &DetailedRelationReport,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<Vec<AnalysisFinding>> {
    let mut findings = Vec::new();
    for row in &report.rows {
        check_control(control)?;
        if matches!(
            row.relation.resolution(),
            RelationResolution::Ambiguous { .. } | RelationResolution::Unresolved { .. }
        ) {
            findings.push(resolution_gap_finding(&row.relation, &row.source));
        }
    }
    Ok(findings)
}

/// Build one exact resolution-gap finding from a detailed relation row.
fn resolution_gap_finding(
    relation: &projectatlas_core::graph::LogicalRelation,
    source: &DetailedRelationNode,
) -> AnalysisFinding {
    AnalysisFinding {
        kind: AnalysisFindingKind::ResolutionGap,
        status: AnalysisStatus::Inconclusive,
        summary: "ambiguous or unresolved relation blocks a closed structural conclusion"
            .to_string(),
        nodes: vec![analysis_node(source)],
        metric: None,
        evidence: Some(AnalysisRelationEvidence {
            relation: relation.clone(),
            next_call: relation_gap_next_call(relation, source),
        }),
        community: None,
    }
}

/// Return the canonical relation identity used to sort and deduplicate gaps.
fn resolution_gap_identity(finding: &AnalysisFinding) -> &str {
    finding
        .evidence
        .as_ref()
        .map_or("", |evidence| evidence.relation.key().canonical_identity())
}

/// Build an exact existing relation request for one source-side gap.
fn relation_gap_next_call(
    relation: &projectatlas_core::graph::LogicalRelation,
    source: &DetailedRelationNode,
) -> Option<RelationAnalysisNextCall> {
    let anchor = relation_anchor_for_entity(&source.entity)?;
    let resolution = match relation.resolution() {
        RelationResolution::Ambiguous { .. } => RelationResolutionFilter::Ambiguous,
        RelationResolution::Unresolved { .. } => RelationResolutionFilter::Unresolved,
        RelationResolution::Resolved { .. } => RelationResolutionFilter::Resolved,
        RelationResolution::External { .. } => RelationResolutionFilter::External,
    };
    Some(RelationAnalysisNextCall {
        anchor,
        direction: RelationDirection::Outbound,
        relation: relation.kind(),
        resolution,
        minimum_confidence: relation.confidence(),
        content_selection: source.content_selection,
    })
}

/// Convert a locally addressable graph entity into a relation anchor.
fn relation_anchor_for_entity(entity: &GraphEntity) -> Option<RelationAnchor> {
    match entity.selector() {
        EntitySelector::File { path } => Some(RelationAnchor::File { file: path.clone() }),
        EntitySelector::Symbol { symbol } => Some(RelationAnchor::Symbol {
            file: symbol.file.clone(),
            name: symbol.name.as_str().to_string(),
            symbol_kind: Some(symbol.kind),
            parent: symbol
                .parent
                .as_ref()
                .map(|parent| parent.as_str().to_string()),
            signature: Some(symbol.signature.as_str().to_string()),
        }),
        EntitySelector::Project
        | EntitySelector::Folder { .. }
        | EntitySelector::Package { .. }
        | EntitySelector::External { .. } => None,
    }
}

/// Insert one local node once by canonical identity.
fn insert_node(nodes: &mut BTreeMap<String, DetailedRelationNode>, node: &DetailedRelationNode) {
    if !matches!(node.entity.selector(), EntitySelector::External { .. }) {
        nodes
            .entry(node.entity.key().canonical_identity().to_string())
            .or_insert_with(|| node.clone());
    }
}

/// Collect local edges from one detailed relation page.
fn collect_report_edges(
    report: &DetailedRelationReport,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<Vec<LocalEdge>> {
    let mut edges = Vec::new();
    for row in &report.rows {
        check_control(control)?;
        if let Some(edge) = local_edge(&row.relation, &row.source.entity, row.target.as_ref()) {
            edges.push(edge);
        }
    }
    Ok(edges)
}

/// Convert a resolved local relation row into an algorithm edge.
fn local_edge(
    relation: &projectatlas_core::graph::LogicalRelation,
    source: &GraphEntity,
    target: Option<&DetailedRelationNode>,
) -> Option<LocalEdge> {
    let target = target?;
    if matches!(target.entity.selector(), EntitySelector::External { .. }) {
        return None;
    }
    Some(LocalEdge {
        source: source.key().canonical_identity().to_string(),
        target: target.entity.key().canonical_identity().to_string(),
        kind: relation.kind(),
        complete: relation.completeness() == Completeness::Complete,
    })
}

/// Completeness state for admitted community relations during closure.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CommunityClosureScope {
    /// Every admitted community relation stayed within the induced nodes.
    #[default]
    Closed,
    /// At least one admitted community relation escaped the induced nodes.
    Open,
}

/// Work and evidence retained while closing edges among admitted nodes.
#[derive(Default)]
struct ClosureWork {
    /// Whether every admitted frontier completed under all bounds.
    complete: bool,
    /// Whether the closure-stage deadline stopped otherwise valid work.
    deadline_reached: bool,
    /// Whether the induced node scope was closed in the requested direction.
    induced_scope_closed: bool,
    /// Scope status for admitted community relations.
    community_scope: CommunityClosureScope,
    /// Database adjacency rows inspected by closure.
    inspected_edges: u32,
    /// Database-decoded bytes retained during closure.
    decoded_bytes: u64,
    /// Resolution gaps discovered after the first relation page.
    resolution_gaps: Vec<AnalysisFinding>,
}

/// Close bounded local edges among the nodes admitted by detailed traversal.
fn close_induced_edges(
    store: &AtlasStore,
    query: &RelationAnalysisQuery,
    relation_work: &DetailedRelationWork,
    deadline: Instant,
    nodes: &BTreeMap<String, DetailedRelationNode>,
    edges: &mut Vec<LocalEdge>,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<ClosureWork> {
    let mut work = ClosureWork {
        complete: true,
        induced_scope_closed: query.relations.direction == RelationDirection::Outbound,
        community_scope: if query.relations.direction == RelationDirection::Outbound {
            CommunityClosureScope::Closed
        } else {
            CommunityClosureScope::Open
        },
        ..ClosureWork::default()
    };
    let mut keys = Vec::with_capacity(nodes.len());
    let mut known = BTreeSet::new();
    for (key, node) in nodes {
        check_control(control)?;
        keys.push(node.entity.key().clone());
        known.insert(key.clone());
    }
    #[cfg(test)]
    analysis_test_observer::notify(analysis_test_observer::AnalysisPhaseEvent::Traversal);
    check_control(control)?;
    for chunk in keys.chunks(MAX_REPOSITORY_GRAPH_FRONTIER) {
        let mut continuation: Option<RepositoryGraphAdjacencyContinuation> = None;
        loop {
            if Instant::now() >= deadline {
                work.complete = false;
                work.deadline_reached = true;
                break;
            }
            check_control(control)?;
            let remaining = query.relations.budget.edges().saturating_sub(
                relation_work
                    .inspected_edges
                    .saturating_add(work.inspected_edges),
            );
            if remaining == 0 {
                work.complete = false;
                break;
            }
            let page_limit = remaining.min(query.relations.budget.page_rows()).max(1);
            let decoded_remaining = query
                .relations
                .budget
                .intermediate_bytes()
                .saturating_sub(
                    relation_work
                        .intermediate_bytes
                        .saturating_add(work.decoded_bytes),
                )
                .min(RepositoryGraphReadBudget::MAX_DECODED_BYTES);
            if decoded_remaining == 0 {
                work.complete = false;
                break;
            }
            let endpoints = page_limit.saturating_add(1).saturating_mul(2);
            let budget = RepositoryGraphReadBudget::new(
                u32::try_from(chunk.len()).map_err(|_overflow| {
                    ServiceError::InvalidInput("analysis frontier overflowed".to_string())
                })?,
                page_limit,
                decoded_remaining,
                endpoints,
                endpoints,
            )
            .map_err(|error| ServiceError::InvalidInput(error.to_string()))?;
            let read = store.repository_graph_adjacency_page_filtered_bounded_with_documents(
                chunk,
                RepositoryGraphDirection::Outbound,
                query.relations.relation,
                super::relations::include_document_relations(&query.relations),
                continuation.as_ref(),
                page_limit,
                budget,
                control,
            )?;
            work.inspected_edges = work
                .inspected_edges
                .checked_add(u32::try_from(read.page.rows.len()).map_err(|_overflow| {
                    ServiceError::InvalidInput("analysis edge work overflowed".to_string())
                })?)
                .ok_or_else(|| {
                    ServiceError::InvalidInput("analysis edge work overflowed".to_string())
                })?;
            work.decoded_bytes = work.decoded_bytes.saturating_add(read.work.decoded_bytes);
            for row in read.page.rows {
                check_control(control)?;
                if !analysis_relation_matches(&row.detail.relation, &query.relations) {
                    continue;
                }
                let community_relation =
                    community_relation_weight(row.detail.relation.kind()).is_some();
                let source_key = row.detail.source.key().canonical_identity().to_string();
                if matches!(
                    row.detail.relation.resolution(),
                    RelationResolution::Ambiguous { .. } | RelationResolution::Unresolved { .. }
                ) {
                    work.induced_scope_closed = false;
                    if community_relation {
                        work.community_scope = CommunityClosureScope::Open;
                    }
                    if let Some(source) = nodes.get(&source_key) {
                        work.resolution_gaps
                            .push(resolution_gap_finding(&row.detail.relation, source));
                    }
                    continue;
                }
                let Some(target) = row.detail.target else {
                    work.induced_scope_closed = false;
                    if community_relation {
                        work.community_scope = CommunityClosureScope::Open;
                    }
                    continue;
                };
                let target_key = target.key().canonical_identity().to_string();
                if known.contains(&source_key) && known.contains(&target_key) {
                    edges.push(LocalEdge {
                        source: source_key,
                        target: target_key,
                        kind: row.detail.relation.kind(),
                        complete: row.detail.relation.completeness() == Completeness::Complete,
                    });
                } else {
                    work.induced_scope_closed = false;
                    if community_relation {
                        work.community_scope = CommunityClosureScope::Open;
                    }
                }
            }
            if read.page.truncated {
                continuation = read.page.continuation;
                if continuation.is_none() {
                    return Err(ServiceError::InvalidInput(
                        "truncated analysis closure omitted its continuation".to_string(),
                    ));
                }
            } else {
                break;
            }
        }
        if !work.complete {
            break;
        }
    }
    edges.sort_by(|left, right| {
        (&left.source, &left.target, format!("{:?}", left.kind)).cmp(&(
            &right.source,
            &right.target,
            format!("{:?}", right.kind),
        ))
    });
    edges.dedup_by(|left, right| {
        left.source == right.source && left.target == right.target && left.kind == right.kind
    });
    work.resolution_gaps
        .sort_by(|left, right| resolution_gap_identity(left).cmp(resolution_gap_identity(right)));
    work.resolution_gaps
        .dedup_by(|left, right| resolution_gap_identity(left) == resolution_gap_identity(right));
    Ok(work)
}

/// Determine whether topology findings have complete admitted evidence.
fn relation_evidence_complete(
    report: &DetailedRelationReport,
    nodes: &BTreeMap<String, DetailedRelationNode>,
    edges: &[LocalEdge],
    query: &RelationAnalysisQuery,
    closure: &ClosureWork,
    community_only: bool,
) -> bool {
    closure.complete
        && if community_only {
            closure.community_scope == CommunityClosureScope::Closed
        } else {
            closure.induced_scope_closed
        }
        && report.reached_limits.is_empty()
        && query.relations.direction == RelationDirection::Outbound
        && edges
            .iter()
            .filter(|edge| !community_only || community_relation_weight(edge.kind).is_some())
            .all(|edge| edge.complete)
        && nodes.values().all(|node| match node.entity.selector() {
            EntitySelector::Project => true,
            EntitySelector::Folder { .. }
            | EntitySelector::File { .. }
            | EntitySelector::Package { .. }
            | EntitySelector::Symbol { .. } => {
                !node.coverage.is_empty()
                    && node.coverage.iter().all(|coverage| {
                        coverage_trust(coverage.state()) == CoverageTrustState::Trusted
                    })
            }
            EntitySelector::External { .. } => false,
        })
}

/// Determine whether the exact inbound scope can support dead-code candidates.
fn dead_code_scope_complete(
    report: &DetailedRelationReport,
    query: &RelationAnalysisQuery,
) -> bool {
    let exact_symbol_anchor = matches!(
        &query.relations.anchor,
        RelationAnchor::Symbol {
            symbol_kind: Some(_),
            signature: Some(_),
            ..
        }
    );
    let exact_total = matches!(
        report.total,
        RelationTotalState::Exact(total) if total == u64::from(report.returned)
    );
    query.relations.direction == RelationDirection::Inbound
        && query.relations.relation.is_none()
        && query.relations.resolution == RelationResolutionFilter::Resolved
        && query.relations.minimum_confidence == ConfidenceClass::Low
        && exact_symbol_anchor
        && exact_total
        && !report.truncated
        && report.continuation.is_none()
        && report.reached_limits.is_empty()
        && !report.anchor.coverage.is_empty()
        && report
            .anchor
            .coverage
            .iter()
            .all(|coverage| coverage_trust(coverage.state()) == CoverageTrustState::Trusted)
        && report.rows.iter().all(|row| {
            matches!(
                row.relation.resolution(),
                RelationResolution::Resolved { .. }
            ) && row.relation.completeness() == Completeness::Complete
        })
}

/// Apply detailed relation family, trust, and resolution filters.
fn analysis_relation_matches(
    relation: &projectatlas_core::graph::LogicalRelation,
    query: &DetailedRelationQuery,
) -> bool {
    super::relations::relation_matches(relation, query)
}

/// Compute bounded architecture findings over admitted topology and symbols.
fn architecture_findings(
    store: &AtlasStore,
    nodes: &BTreeMap<String, DetailedRelationNode>,
    edges: &[LocalEdge],
    complete: bool,
    community_complete: bool,
    query: &RelationAnalysisQuery,
    symbol_byte_budget: u64,
    preceding_finding_count: usize,
    supplemental_work: &mut SupplementalWork,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<Vec<AnalysisFinding>> {
    let mut findings = structural_findings(
        store,
        nodes,
        edges,
        complete,
        symbol_byte_budget,
        supplemental_work,
        control,
    )?;
    if !complete {
        findings.push(AnalysisFinding {
            kind: AnalysisFindingKind::Component,
            status: AnalysisStatus::Inconclusive,
            summary: "architecture candidates are incomplete because traversal or local coverage is partial"
                .to_string(),
            nodes: analysis_nodes_for(nodes, &nodes.keys().cloned().collect::<Vec<_>>()),
            metric: Some(nodes.len() as u64),
            evidence: None,
            community: None,
        });
    }
    let components = weak_components(nodes, edges, false);
    for component in &components {
        findings.push(AnalysisFinding {
            kind: AnalysisFindingKind::Component,
            status: AnalysisStatus::Candidate,
            summary: "component candidate from a weakly connected admitted relation set"
                .to_string(),
            nodes: analysis_nodes_for(nodes, component),
            metric: Some(component.len() as u64),
            evidence: None,
            community: None,
        });
        findings.push(purpose_finding(nodes, component, complete));
    }
    if query.include_communities {
        let mut existing_finding_bytes = serialized_bytes_controlled(&findings, control)?;
        let mut existing_finding_append_bytes = serialized_findings_append_bytes(
            existing_finding_bytes,
            findings.len(),
            preceding_finding_count,
        );
        if existing_finding_append_bytes > symbol_byte_budget {
            supplemental_work.composition_truncated = true;
            push_limit(
                &mut supplemental_work.reached_limits,
                GraphLimitKind::IntermediateBytes,
            );
            while existing_finding_append_bytes > symbol_byte_budget && findings.len() > 1 {
                check_control(control)?;
                findings.pop();
                existing_finding_bytes = serialized_bytes_controlled(&findings, control)?;
                existing_finding_append_bytes = serialized_findings_append_bytes(
                    existing_finding_bytes,
                    findings.len(),
                    preceding_finding_count,
                );
            }
            if existing_finding_append_bytes > symbol_byte_budget {
                findings.clear();
                existing_finding_bytes = serialized_bytes_controlled(&findings, control)?;
                existing_finding_append_bytes = serialized_findings_append_bytes(
                    existing_finding_bytes,
                    findings.len(),
                    preceding_finding_count,
                );
            }
        }
        let community_budget = symbol_byte_budget.saturating_sub(existing_finding_append_bytes);
        #[cfg(test)]
        analysis_test_observer::notify(
            analysis_test_observer::AnalysisPhaseEvent::CompositionBudget {
                symbol_byte_budget,
                existing_finding_append_bytes,
                community_budget,
            },
        );
        let existing_community_finding_count =
            preceding_finding_count.saturating_add(findings.len());
        let (community_findings, community_working_set_bytes, community_limits) =
            community_findings_with_budget(
                nodes,
                edges,
                community_complete,
                query,
                community_budget,
                existing_community_finding_count,
                control,
            )?;
        supplemental_work.community_working_set_bytes = supplemental_work
            .community_working_set_bytes
            .max(community_working_set_bytes);
        let community_truncated = !community_limits.is_empty();
        for limit in community_limits {
            push_limit(&mut supplemental_work.reached_limits, limit);
        }
        if community_truncated
            || community_findings.iter().any(|finding| {
                finding
                    .community
                    .as_ref()
                    .is_some_and(|community| community.truncated)
            })
        {
            supplemental_work.composition_truncated = true;
        }
        findings.extend(community_findings);
    }
    if query.include_cycles {
        let dependency_edges = edges
            .iter()
            .filter(|edge| dependency_relation(edge.kind))
            .cloned()
            .collect::<Vec<_>>();
        let cycles = strongly_connected_components(nodes, &dependency_edges)
            .into_iter()
            .filter(|component| {
                component.len() > 1
                    || dependency_edges.iter().any(|edge| {
                        component.first() == Some(&edge.source) && edge.source == edge.target
                    })
            })
            .collect::<Vec<_>>();
        if cycles.is_empty() {
            findings.push(AnalysisFinding {
                kind: AnalysisFindingKind::DependencyCycle,
                status: if complete {
                    AnalysisStatus::Absent
                } else {
                    AnalysisStatus::Inconclusive
                },
                summary: if complete {
                    "no dependency cycle exists in the complete admitted bounded scope"
                } else {
                    "no cycle was observed, but traversal or coverage is incomplete"
                }
                .to_string(),
                nodes: Vec::new(),
                metric: Some(0),
                evidence: None,
                community: None,
            });
        } else {
            for cycle in cycles {
                findings.push(AnalysisFinding {
                    kind: AnalysisFindingKind::DependencyCycle,
                    status: AnalysisStatus::Candidate,
                    summary: "iterative dependency-family SCC found a static cycle candidate"
                        .to_string(),
                    nodes: analysis_nodes_for(nodes, &cycle),
                    metric: Some(cycle.len() as u64),
                    evidence: None,
                    community: None,
                });
            }
        }
    }
    Ok(findings)
}

/// Return whether a relation participates in dependency SCC and impact flow.
fn dependency_relation(kind: GraphRelationKind) -> bool {
    matches!(
        kind,
        GraphRelationKind::Legacy(
            RelationKind::Imports | RelationKind::Calls | RelationKind::DependsOn
        ) | GraphRelationKind::Extended(
            ExtendedRelationKind::Tests
                | ExtendedRelationKind::RoutesTo
                | ExtendedRelationKind::Configures
        )
    )
}

/// Classify purpose alignment for one connected component.
fn purpose_finding(
    nodes: &BTreeMap<String, DetailedRelationNode>,
    component: &[String],
    complete: bool,
) -> AnalysisFinding {
    let mut purposes = BTreeSet::new();
    let mut unavailable = false;
    for key in component {
        match nodes.get(key).map(|node| &node.purpose) {
            Some(RelationPurpose::Approved { purpose, .. }) => {
                purposes.insert(purpose.clone());
            }
            Some(RelationPurpose::Unavailable { .. } | RelationPurpose::NotApplicable) | None => {
                unavailable = true;
            }
        }
    }
    let (kind, status, summary) = if purposes.len() > 1 {
        (
            AnalysisFindingKind::PurposeDrift,
            AnalysisStatus::Candidate,
            "connected owners retain multiple approved purpose responsibilities",
        )
    } else if purposes.len() == 1 && !unavailable && complete {
        (
            AnalysisFindingKind::PurposeAlignment,
            AnalysisStatus::Confirmed,
            "connected owners share one approved purpose responsibility",
        )
    } else {
        (
            AnalysisFindingKind::PurposeAlignment,
            AnalysisStatus::Inconclusive,
            if complete {
                "purpose alignment is unavailable for at least one admitted owner"
            } else {
                "purpose alignment is inconclusive under partial traversal or coverage"
            },
        )
    };
    AnalysisFinding {
        kind,
        status,
        summary: summary.to_string(),
        nodes: analysis_nodes_for(nodes, component),
        metric: Some(purposes.len() as u64),
        evidence: None,
        community: None,
    }
}

/// Compute declaration-span and graph-degree structural candidates.
fn structural_findings(
    store: &AtlasStore,
    nodes: &BTreeMap<String, DetailedRelationNode>,
    edges: &[LocalEdge],
    topology_complete: bool,
    symbol_byte_budget: u64,
    supplemental_work: &mut SupplementalWork,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<Vec<AnalysisFinding>> {
    let degrees = degrees(nodes, edges);
    let symbols_by_file = load_admitted_symbols(store, nodes, symbol_byte_budget, control)?;
    supplemental_work.hydrated_symbols = supplemental_work
        .hydrated_symbols
        .saturating_add(symbols_by_file.rows_retained);
    supplemental_work.hydrated_symbol_bytes = supplemental_work
        .hydrated_symbol_bytes
        .saturating_add(symbols_by_file.retained_bytes);
    supplemental_work.hydrated_symbol_peak_bytes = supplemental_work
        .hydrated_symbol_peak_bytes
        .max(symbols_by_file.peak_bytes);
    supplemental_work.symbol_hydration_truncated |= !symbols_by_file.complete;
    supplemental_work
        .reached_limits
        .extend(symbols_by_file.reached_limits.iter().copied());
    let mut candidates = Vec::new();
    for (key, node) in nodes {
        check_control(control)?;
        let Some((path, name, kind, parent, signature)) = symbol_identity(&node.entity) else {
            continue;
        };
        if let Some(symbol) = symbols_by_file.rows_for_path(path).and_then(|symbols| {
            symbols.iter().find(|candidate| {
                candidate.name == name
                    && candidate.kind == kind
                    && candidate.parent.as_deref() == parent
                    && candidate.signature == signature
            })
        }) {
            let span = symbol
                .line_end
                .saturating_sub(symbol.line_start)
                .saturating_add(1) as u64;
            let degree = degrees.get(key).copied().unwrap_or_default() as u64;
            candidates.push((span, degree, key.clone()));
        }
    }
    let symbols_complete = symbols_by_file.complete;
    drop(symbols_by_file);
    let mut findings = Vec::new();
    if let Some((span, _degree, key)) = candidates.iter().max_by(std::cmp::Ord::cmp) {
        findings.push(AnalysisFinding {
            kind: AnalysisFindingKind::StructuralComplexity,
            status: if symbols_complete && topology_complete {
                AnalysisStatus::Candidate
            } else {
                AnalysisStatus::Inconclusive
            },
            summary: if symbols_complete && topology_complete {
                "largest language-valid declaration span in the admitted scope; not cyclomatic complexity"
            } else {
                "structural candidate observed, but bounded symbol hydration omitted admitted declarations"
            }
            .to_string(),
            nodes: analysis_nodes_for(nodes, std::slice::from_ref(key)),
            metric: Some(*span),
            evidence: None,
            community: None,
        });
    }
    if let Some((_span, degree, key)) = candidates
        .iter()
        .max_by(|left, right| (left.1, left.0, &left.2).cmp(&(right.1, right.0, &right.2)))
    {
        findings.push(AnalysisFinding {
            kind: AnalysisFindingKind::Bottleneck,
            status: if symbols_complete && topology_complete {
                AnalysisStatus::Candidate
            } else {
                AnalysisStatus::Inconclusive
            },
            summary: "highest admitted static fan-in plus fan-out junction".to_string(),
            nodes: analysis_nodes_for(nodes, std::slice::from_ref(key)),
            metric: Some(*degree),
            evidence: None,
            community: None,
        });
    }
    Ok(findings)
}

/// Bounded persisted symbols plus a compact exact-path range index.
struct AdmittedSymbols {
    /// Deterministically sorted symbols returned by the database.
    rows: Vec<CodeSymbol>,
    /// Sorted exact-path ranges into `rows`.
    ranges: Vec<SymbolPathRange>,
    /// Whether every admitted path, row, and byte fit the envelope.
    complete: bool,
    /// Number of persisted symbols retained.
    rows_retained: u32,
    /// Database-decoded rows plus serialized-equivalent range-index bytes.
    retained_bytes: u64,
    /// Retained bytes plus serialized-equivalent request-path auxiliaries.
    peak_bytes: u64,
    /// Stable public limits that omitted symbol candidates.
    reached_limits: Vec<GraphLimitKind>,
}

#[derive(Serialize)]
/// One sorted exact-path slice into the retained symbol vector.
struct SymbolPathRange {
    /// Exact repository-relative owning path.
    path: String,
    /// Inclusive symbol-vector offset.
    start: usize,
    /// Exclusive symbol-vector offset.
    end: usize,
}

impl AdmittedSymbols {
    /// Return the retained persisted symbols for one exact path.
    fn rows_for_path(&self, path: &str) -> Option<&[CodeSymbol]> {
        let index = self
            .ranges
            .binary_search_by(|range| range.path.as_str().cmp(path))
            .ok()?;
        let range = &self.ranges[index];
        self.rows.get(range.start..range.end)
    }
}

/// Load one bounded indexed symbol batch and build its compact path ranges.
fn load_admitted_symbols(
    store: &AtlasStore,
    nodes: &BTreeMap<String, DetailedRelationNode>,
    byte_budget: u64,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<AdmittedSymbols> {
    let path_limit = usize::try_from(MAX_SYMBOL_BATCH_PATHS).map_err(|source| {
        ServiceError::InvalidInput(format!("symbol path ceiling overflowed: {source}"))
    })?;
    let mut paths = Vec::new();
    let mut path_truncated = false;
    for node in nodes.values() {
        check_control(control)?;
        let EntitySelector::Symbol { symbol } = node.entity.selector() else {
            continue;
        };
        let path = symbol.file.as_str();
        let Err(index) = paths.binary_search_by(|candidate: &String| candidate.as_str().cmp(path))
        else {
            continue;
        };
        paths.insert(index, path.to_string());
        if paths.len() > path_limit {
            paths.pop();
            path_truncated = true;
        }
    }
    let paths = paths.into_boxed_slice().into_vec();
    let byte_limit = byte_budget.min(MAX_SYMBOL_BATCH_DECODED_BYTES);
    let mut reached_limits = Vec::new();
    if path_truncated {
        push_limit(&mut reached_limits, GraphLimitKind::Rows);
    }
    if paths.is_empty() {
        return Ok(AdmittedSymbols {
            rows: Vec::new(),
            ranges: Vec::new(),
            complete: !path_truncated,
            rows_retained: 0,
            retained_bytes: 0,
            peak_bytes: symbol_path_request_bytes(&paths, control)?,
            reached_limits,
        });
    }
    #[cfg(test)]
    analysis_test_observer::notify(analysis_test_observer::AnalysisPhaseEvent::SymbolHydration);
    check_control(control)?;
    let grouping_reserve = symbol_hydration_reserve_bytes(&paths, control)?;
    let decoded_byte_limit = byte_limit.saturating_sub(grouping_reserve);
    if decoded_byte_limit == 0 {
        push_limit(&mut reached_limits, GraphLimitKind::IntermediateBytes);
        return Ok(AdmittedSymbols {
            rows: Vec::new(),
            ranges: Vec::new(),
            complete: false,
            rows_retained: 0,
            retained_bytes: 0,
            peak_bytes: symbol_path_request_bytes(&paths, control)?,
            reached_limits,
        });
    }
    let read = store.load_symbols_for_paths_bounded(
        &paths,
        SymbolBatchReadBudget::new(
            MAX_SYMBOL_BATCH_PATHS,
            MAX_SYMBOL_BATCH_ROWS,
            decoded_byte_limit.min(MAX_SYMBOL_BATCH_DECODED_BYTES),
        )?,
        control,
    )?;
    match read.reached_limit {
        Some(SymbolBatchReadLimit::Paths | SymbolBatchReadLimit::Rows) => {
            push_limit(&mut reached_limits, GraphLimitKind::Rows);
        }
        Some(SymbolBatchReadLimit::DecodedBytes) => {
            push_limit(&mut reached_limits, GraphLimitKind::IntermediateBytes);
        }
        None => {}
    }
    let ranges = symbol_path_ranges(&read.rows, control)?;
    let retained_bytes = read
        .work
        .decoded_bytes
        .saturating_add(symbol_range_index_bytes(&ranges, control)?);
    let peak_bytes = retained_bytes.saturating_add(symbol_path_request_bytes(&paths, control)?);
    Ok(AdmittedSymbols {
        rows: read.rows,
        ranges,
        complete: !path_truncated && !read.truncated,
        rows_retained: read.work.returned_rows,
        retained_bytes,
        peak_bytes,
        reached_limits,
    })
}

/// Build sorted non-overlapping exact-path ranges over sorted symbol rows.
fn symbol_path_ranges(
    rows: &[CodeSymbol],
    control: Option<&IndexWorkControl>,
) -> ServiceResult<Vec<SymbolPathRange>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < rows.len() {
        check_control(control)?;
        let mut end = start.saturating_add(1);
        while end < rows.len() && rows[end].path == rows[start].path {
            check_control(control)?;
            end = end.saturating_add(1);
        }
        ranges.push(SymbolPathRange {
            path: rows[start].path.clone(),
            start,
            end,
        });
        start = end;
    }
    Ok(ranges.into_boxed_slice().into_vec())
}

/// Count serialized request-path and worst-case path-range auxiliaries.
fn symbol_hydration_reserve_bytes(
    paths: &[String],
    control: Option<&IndexWorkControl>,
) -> ServiceResult<u64> {
    let mut ranges = Vec::with_capacity(paths.len());
    for path in paths {
        check_control(control)?;
        ranges.push(SymbolPathRange {
            path: path.clone(),
            start: 0,
            end: 0,
        });
    }
    Ok(symbol_path_request_bytes(paths, control)?
        .saturating_add(serialized_bytes_controlled(&ranges, control)?))
}

/// Count serialized-equivalent request-path bytes.
fn symbol_path_request_bytes(
    paths: &[String],
    control: Option<&IndexWorkControl>,
) -> ServiceResult<u64> {
    serialized_bytes_controlled(paths, control)
}

/// Count serialized-equivalent retained path-range bytes.
fn symbol_range_index_bytes(
    ranges: &[SymbolPathRange],
    control: Option<&IndexWorkControl>,
) -> ServiceResult<u64> {
    serialized_bytes_controlled(ranges, control)
}

/// Allocation-free serialized-equivalent byte counter.
struct SerializedByteCounter<'a> {
    /// Bytes that the serializer would write.
    bytes: u64,
    /// Shared request control checked between serializer writes.
    control: Option<&'a IndexWorkControl>,
    /// Whether serialization stopped on cancellation or deadline.
    interrupted: bool,
}

impl Write for SerializedByteCounter<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self
            .control
            .is_some_and(|control| control.check(IndexWorkStage::RepositoryTraversal).is_err())
        {
            self.interrupted = true;
            // `Write::write_all` retries `Interrupted` indefinitely. Use a
            // non-retriable transport error and translate it back to the
            // control's typed terminal failure after serialization returns.
            return Err(io::Error::other("analysis serialization interrupted"));
        }
        self.bytes = self
            .bytes
            .checked_add(u64::try_from(buffer.len()).unwrap_or(u64::MAX))
            .ok_or_else(|| io::Error::other("analysis serialized byte count overflowed"))?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Measure one bounded value while retaining the request deadline and cancellation.
fn serialized_bytes_controlled<T: Serialize + ?Sized>(
    value: &T,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<u64> {
    let mut counter = SerializedByteCounter {
        bytes: 0,
        control,
        interrupted: false,
    };
    let result = serde_json::to_writer(&mut counter, value);
    if counter.interrupted {
        check_control(control)?;
    }
    result?;
    check_control(control)?;
    Ok(counter.bytes)
}

/// Charge one finding as an append to an already retained JSON findings array.
///
/// The enclosing array's `[]` framing is charged by the composition owner. An
/// append therefore adds only the finding bytes and, after the first finding,
/// the fixed comma separator.
const fn serialized_finding_append_bytes(
    finding_bytes: u64,
    preceding_finding_count: usize,
) -> u64 {
    finding_bytes.saturating_add(if preceding_finding_count == 0 {
        0
    } else {
        JSON_ARRAY_SEPARATOR_BYTES
    })
}

/// Charge a serialized findings vector appended to an existing JSON array.
const fn serialized_findings_append_bytes(
    findings_bytes: u64,
    finding_count: usize,
    preceding_finding_count: usize,
) -> u64 {
    if finding_count == 0 {
        return 0;
    }
    findings_bytes.saturating_sub(if preceding_finding_count == 0 {
        JSON_ARRAY_FRAMING_BYTES
    } else {
        JSON_ARRAY_SEPARATOR_BYTES
    })
}

/// Find one node-simple static relation path to an exact target.
fn trace_findings(
    report: &DetailedRelationReport,
    target: Option<&RelationAnchor>,
    evidence_complete: bool,
) -> ServiceResult<Vec<AnalysisFinding>> {
    let target = target.ok_or_else(|| {
        ServiceError::InvalidInput("analysis trace requires an exact target".to_string())
    })?;
    let mut matching = BTreeSet::new();
    if entity_matches_anchor(&report.anchor.entity, target) {
        matching.insert(report.anchor.entity.key().canonical_identity().to_string());
    }
    for row in &report.rows {
        for node in &row.path {
            if entity_matches_anchor(&node.entity, target) {
                matching.insert(node.entity.key().canonical_identity().to_string());
            }
        }
    }
    if matching.len() > 1 {
        return Err(ServiceError::InvalidInput(
            "analysis trace target is ambiguous in the admitted graph scope".to_string(),
        ));
    }
    let target_key = matching.iter().next();
    if target_key.is_some_and(|key| key == report.anchor.entity.key().canonical_identity()) {
        return Ok(vec![AnalysisFinding {
            kind: AnalysisFindingKind::StaticTrace,
            status: AnalysisStatus::Confirmed,
            summary: "static trace target is the selected anchor".to_string(),
            nodes: vec![analysis_node(&report.anchor)],
            metric: Some(0),
            evidence: None,
            community: None,
        }]);
    }
    if let Some(row) = target_key.and_then(|target_key| {
        report.rows.iter().find(|row| {
            row.path
                .last()
                .is_some_and(|node| node.entity.key().canonical_identity() == target_key.as_str())
        })
    }) {
        return Ok(vec![AnalysisFinding {
            kind: AnalysisFindingKind::StaticTrace,
            status: AnalysisStatus::Confirmed,
            summary: "node-simple static relation path; not a runtime execution trace".to_string(),
            nodes: row.path.iter().map(analysis_node).collect(),
            metric: Some(u64::from(row.depth)),
            evidence: None,
            community: None,
        }]);
    }
    Ok(vec![AnalysisFinding {
        kind: AnalysisFindingKind::StaticTrace,
        status: if evidence_complete {
            AnalysisStatus::Absent
        } else {
            AnalysisStatus::Inconclusive
        },
        summary: if evidence_complete {
            "target is not reachable in the complete admitted static scope"
        } else {
            "target was not observed before the bounded traversal or coverage stopped"
        }
        .to_string(),
        nodes: Vec::new(),
        metric: None,
        evidence: None,
        community: None,
    }])
}

/// Return the fixed relation-family weights admitted by community v1.
fn community_relation_weights() -> Vec<CommunityRelationWeight> {
    GraphRelationKind::ALL
        .into_iter()
        .filter_map(|relation| {
            community_relation_weight(relation)
                .map(|weight| CommunityRelationWeight { relation, weight })
        })
        .collect()
}

/// Return the fixed vote weight for an admitted relation family.
fn community_relation_weight(kind: GraphRelationKind) -> Option<u32> {
    match kind {
        GraphRelationKind::Legacy(RelationKind::Calls) => Some(8),
        GraphRelationKind::Legacy(RelationKind::Imports) => Some(5),
        GraphRelationKind::Legacy(RelationKind::DependsOn) => Some(3),
        GraphRelationKind::Extended(
            ExtendedRelationKind::Tests | ExtendedRelationKind::RoutesTo,
        ) => Some(4),
        GraphRelationKind::Extended(
            ExtendedRelationKind::References
            | ExtendedRelationKind::Configures
            | ExtendedRelationKind::Reads
            | ExtendedRelationKind::Writes,
        ) => Some(2),
        GraphRelationKind::Extended(
            ExtendedRelationKind::Documents | ExtendedRelationKind::Deploys,
        ) => Some(1),
        GraphRelationKind::Legacy(RelationKind::Contains) => None,
    }
}

/// Return the weight for one complete relation admitted by the community projection.
fn admitted_community_edge_weight(edge: &LocalEdge) -> Option<u32> {
    if !edge.complete {
        return None;
    }
    community_relation_weight(edge.kind)
}

/// Compute deterministic bounded weighted label-propagation communities.
#[cfg(test)]
fn community_findings(
    nodes: &BTreeMap<String, DetailedRelationNode>,
    edges: &[LocalEdge],
    complete: bool,
    query: &RelationAnalysisQuery,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<Vec<AnalysisFinding>> {
    Ok(community_findings_with_budget(
        nodes,
        edges,
        complete,
        query,
        query.relations.budget.intermediate_bytes(),
        0,
        control,
    )?
    .0)
}

/// Compute communities with an explicit remaining intermediate-byte allowance.
fn community_findings_with_budget(
    nodes: &BTreeMap<String, DetailedRelationNode>,
    edges: &[LocalEdge],
    complete: bool,
    query: &RelationAnalysisQuery,
    intermediate_bytes: u64,
    preceding_finding_count: usize,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<(Vec<AnalysisFinding>, u64, Vec<GraphLimitKind>)> {
    check_control(control)?;
    let weights = community_relation_weights();
    let parameters = CommunityParameters {
        algorithm_version: COMMUNITY_ALGORITHM_VERSION,
        ordering_version: COMMUNITY_ORDERING_VERSION,
        max_iterations: COMMUNITY_MAX_ITERATIONS,
        node_limit: query.relations.budget.nodes().min(MAX_ANALYSIS_NODES),
        edge_limit: query.relations.budget.edges().min(MAX_ANALYSIS_EDGES),
        output_bytes: query.relations.budget.output_bytes(),
        relation: query.relations.relation,
    };
    let complete = complete
        && edges
            .iter()
            .filter(|edge| community_relation_weight(edge.kind).is_some())
            .all(|edge| edge.complete);
    let marker_coverage = if complete {
        CommunityCoverage::Complete
    } else {
        CommunityCoverage::Partial
    };
    let truncation_marker = community_truncation_finding(&weights, parameters, 0, marker_coverage);
    let truncation_marker_bytes = serialized_bytes_controlled(&truncation_marker, control)?;
    let truncation_marker_append_bytes =
        serialized_finding_append_bytes(truncation_marker_bytes, preceding_finding_count);
    let working_set_bytes = community_working_set_upper_bound(nodes, edges, control)?;
    if working_set_bytes.saturating_add(truncation_marker_append_bytes) > intermediate_bytes {
        let findings = if truncation_marker_append_bytes <= intermediate_bytes {
            vec![truncation_marker]
        } else {
            Vec::new()
        };
        return Ok((findings, 0, vec![GraphLimitKind::IntermediateBytes]));
    }
    let construction_bytes = intermediate_bytes.saturating_sub(working_set_bytes);
    let (keys, admitted_edges, resource_limits) =
        admitted_community_scope(nodes, edges, parameters);
    let resource_truncated = !resource_limits.is_empty();
    if !complete || resource_truncated {
        let mut evidence = admitted_edges;
        if resource_truncated {
            evidence.truncate(parameters.edge_limit as usize);
        }
        let metadata_fits = serialized_finding_append_bytes(
            community_finding_upper_bound(nodes, &keys, &evidence, &weights, parameters, control)?,
            preceding_finding_count,
        ) <= construction_bytes;
        let metadata_keys = if metadata_fits { keys.as_slice() } else { &[] };
        let metadata_evidence = if metadata_fits {
            evidence.as_slice()
        } else {
            &[]
        };
        let metadata_truncated = resource_truncated || !metadata_fits;
        let mut truncation_limits = resource_limits;
        if !metadata_fits {
            push_limit(&mut truncation_limits, GraphLimitKind::IntermediateBytes);
        }
        let metadata = community_metadata(
            metadata_keys,
            metadata_evidence,
            weights,
            parameters,
            0,
            CommunityConvergence::Inconclusive,
            if complete {
                CommunityCoverage::Complete
            } else {
                CommunityCoverage::Partial
            },
            metadata_truncated,
            nodes,
        );
        return Ok((
            vec![AnalysisFinding {
                kind: AnalysisFindingKind::Community,
                status: AnalysisStatus::Inconclusive,
                summary: if resource_truncated {
                    "community projection crossed its fixed node or edge resource ceiling"
                } else {
                    "community projection is inconclusive because relation coverage is partial"
                }
                .to_string(),
                nodes: analysis_nodes_for(nodes, metadata_keys),
                metric: Some(metadata_keys.len() as u64),
                evidence: None,
                community: Some(metadata),
            }],
            working_set_bytes,
            truncation_limits,
        ));
    }

    let (labels, iteration, convergence) =
        propagate_community_labels(&keys, &admitted_edges, parameters.max_iterations, control)?;
    if convergence != CommunityConvergence::Converged {
        let metadata_fits = community_finding_upper_bound(
            nodes,
            &keys,
            &admitted_edges,
            &weights,
            parameters,
            control,
        )?;
        let metadata_fits = serialized_finding_append_bytes(metadata_fits, preceding_finding_count)
            <= construction_bytes;
        let metadata_keys = if metadata_fits { keys.as_slice() } else { &[] };
        let metadata_evidence = if metadata_fits {
            admitted_edges.as_slice()
        } else {
            &[]
        };
        let metadata_truncated = !metadata_fits;
        let mut truncation_limits = Vec::new();
        if !metadata_fits {
            truncation_limits.push(GraphLimitKind::IntermediateBytes);
        }
        let metadata = community_metadata(
            metadata_keys,
            metadata_evidence,
            weights,
            parameters,
            iteration,
            convergence,
            CommunityCoverage::Complete,
            metadata_truncated,
            nodes,
        );
        return Ok((
            vec![AnalysisFinding {
                kind: AnalysisFindingKind::Community,
                status: AnalysisStatus::Inconclusive,
                summary: "community label propagation reached its fixed iteration ceiling"
                    .to_string(),
                nodes: analysis_nodes_for(nodes, metadata_keys),
                metric: Some(metadata_keys.len() as u64),
                evidence: None,
                community: Some(metadata),
            }],
            working_set_bytes,
            truncation_limits,
        ));
    }

    let (findings, truncation_limits) = community_candidate_findings(
        nodes,
        &keys,
        &admitted_edges,
        &labels,
        &weights,
        parameters,
        iteration,
        convergence,
        control,
        preceding_finding_count,
        construction_bytes,
    )?;
    Ok((findings, working_set_bytes, truncation_limits))
}

/// Admit the stable node and edge prefix under the community resource ceiling.
fn admitted_community_scope(
    nodes: &BTreeMap<String, DetailedRelationNode>,
    edges: &[LocalEdge],
    parameters: CommunityParameters,
) -> (Vec<String>, Vec<CommunityEdgeEvidence>, Vec<GraphLimitKind>) {
    let all_keys = nodes.keys().cloned().collect::<Vec<_>>();
    let keys = all_keys
        .iter()
        .take(parameters.node_limit as usize)
        .cloned()
        .collect::<Vec<_>>();
    let admitted_nodes = keys.iter().collect::<BTreeSet<_>>();
    let mut admitted_edges = Vec::new();
    let mut edge_count = 0_usize;
    for edge in edges {
        let Some(weight) = admitted_community_edge_weight(edge) else {
            continue;
        };
        if !admitted_nodes.contains(&edge.source) || !admitted_nodes.contains(&edge.target) {
            continue;
        }
        edge_count = edge_count.saturating_add(1);
        if edge_count <= parameters.edge_limit as usize {
            admitted_edges.push(CommunityEdgeEvidence {
                source: edge.source.clone(),
                target: edge.target.clone(),
                relation: edge.kind,
                weight,
            });
        }
    }
    admitted_edges.sort_by(|left, right| {
        (&left.source, &left.target, left.relation.as_str()).cmp(&(
            &right.source,
            &right.target,
            right.relation.as_str(),
        ))
    });
    admitted_edges.dedup_by(|left, right| {
        left.source == right.source
            && left.target == right.target
            && left.relation == right.relation
    });
    let mut reached_limits = Vec::new();
    if all_keys.len() > parameters.node_limit as usize {
        reached_limits.push(GraphLimitKind::Nodes);
    }
    if edge_count > parameters.edge_limit as usize {
        reached_limits.push(GraphLimitKind::Edges);
    }
    (keys, admitted_edges, reached_limits)
}

/// Propagate labels in stable node order with a fixed sequential work budget.
fn propagate_community_labels(
    keys: &[String],
    edges: &[CommunityEdgeEvidence],
    max_iterations: u32,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<(BTreeMap<String, String>, u32, CommunityConvergence)> {
    check_control(control)?;
    let mut adjacency = keys
        .iter()
        .map(|key| (key.clone(), Vec::<(String, u32)>::new()))
        .collect::<BTreeMap<_, _>>();
    for edge in edges {
        check_control(control)?;
        adjacency
            .entry(edge.source.clone())
            .or_default()
            .push((edge.target.clone(), edge.weight));
        adjacency
            .entry(edge.target.clone())
            .or_default()
            .push((edge.source.clone(), edge.weight));
    }
    for neighbors in adjacency.values_mut() {
        neighbors.sort();
    }

    // ponytail: one bounded sequential pass is deterministic and sufficient at
    // the 512-node ceiling; replace only after a representative profile proves
    // that parallel label updates repay their extra synchronization.
    let mut labels = keys
        .iter()
        .map(|key| (key.clone(), key.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut converged = keys.is_empty();
    let mut iteration = 0;
    while !converged && iteration < max_iterations {
        check_control(control)?;
        iteration += 1;
        let mut changed = false;
        for key in keys {
            check_control(control)?;
            let mut scores = BTreeMap::<String, u32>::new();
            scores.insert(key.clone(), COMMUNITY_SELF_WEIGHT);
            if let Some(neighbors) = adjacency.get(key) {
                for (neighbor, weight) in neighbors {
                    check_control(control)?;
                    if let Some(label) = labels.get(neighbor) {
                        let score = scores.entry(label.clone()).or_default();
                        *score = score.saturating_add(*weight);
                    }
                }
            }
            let selected = select_community_label(key, scores);
            if labels.get(key) != Some(&selected) {
                labels.insert(key.clone(), selected);
                changed = true;
            }
        }
        converged = !changed;
    }

    Ok((
        labels,
        iteration,
        if converged {
            CommunityConvergence::Converged
        } else {
            CommunityConvergence::IterationLimit
        },
    ))
}

/// Select the highest-scoring label, breaking equal scores by stable key order.
fn select_community_label(key: &str, scores: BTreeMap<String, u32>) -> String {
    let mut selected = key.to_string();
    let mut selected_score = 0;
    for (label, score) in scores {
        if score > selected_score || (score == selected_score && label < selected) {
            selected = label;
            selected_score = score;
        }
    }
    selected
}

/// Convert stable labels into candidate findings with induced evidence.
fn community_candidate_findings(
    nodes: &BTreeMap<String, DetailedRelationNode>,
    keys: &[String],
    admitted_edges: &[CommunityEdgeEvidence],
    labels: &BTreeMap<String, String>,
    weights: &[CommunityRelationWeight],
    parameters: CommunityParameters,
    iteration: u32,
    convergence: CommunityConvergence,
    control: Option<&IndexWorkControl>,
    preceding_finding_count: usize,
    intermediate_bytes: u64,
) -> ServiceResult<(Vec<AnalysisFinding>, Vec<GraphLimitKind>)> {
    let mut groups = BTreeMap::<String, Vec<String>>::new();
    for key in keys {
        check_control(control)?;
        let label = labels.get(key).cloned().unwrap_or_else(|| key.clone());
        groups.entry(label).or_default().push(key.clone());
    }
    let mut findings = Vec::new();
    let mut retained_append_bytes = 0_u64;
    let mut truncated = false;
    for members in groups.values() {
        check_control(control)?;
        let candidate_bound = community_candidate_upper_bound(
            nodes,
            members,
            admitted_edges,
            weights,
            parameters,
            control,
        )?;
        let candidate_append_bytes = serialized_finding_append_bytes(
            candidate_bound,
            preceding_finding_count.saturating_add(findings.len()),
        );
        if retained_append_bytes.saturating_add(candidate_append_bytes) > intermediate_bytes {
            truncated = true;
            break;
        }
        let evidence = admitted_edges
            .iter()
            .filter(|edge| {
                members.binary_search(&edge.source).is_ok()
                    && members.binary_search(&edge.target).is_ok()
            })
            .cloned()
            .collect::<Vec<_>>();
        let metadata = community_metadata(
            members,
            &evidence,
            weights.to_vec(),
            parameters,
            iteration,
            convergence,
            CommunityCoverage::Complete,
            false,
            nodes,
        );
        let finding = AnalysisFinding {
            kind: AnalysisFindingKind::Community,
            status: AnalysisStatus::Candidate,
            summary: if members.len() == 1 {
                "singleton community from the complete admitted relation scope"
            } else {
                "deterministic weighted relationship community candidate"
            }
            .to_string(),
            nodes: analysis_nodes_for(nodes, members),
            metric: Some(members.len() as u64),
            evidence: None,
            community: Some(metadata),
        };
        let finding_bytes = serialized_bytes_controlled(&finding, control)?;
        let finding_append_bytes = serialized_finding_append_bytes(
            finding_bytes,
            preceding_finding_count.saturating_add(findings.len()),
        );
        if retained_append_bytes.saturating_add(finding_append_bytes) > intermediate_bytes {
            truncated = true;
            break;
        }
        retained_append_bytes = retained_append_bytes.saturating_add(finding_append_bytes);
        findings.push(finding);
    }
    if truncated {
        check_control(control)?;
        let marker = community_truncation_finding(
            weights,
            parameters,
            iteration,
            CommunityCoverage::Complete,
        );
        let marker_bytes = serialized_bytes_controlled(&marker, control)?;
        let marker_append_bytes = serialized_finding_append_bytes(
            marker_bytes,
            preceding_finding_count.saturating_add(findings.len()),
        );
        if retained_append_bytes.saturating_add(marker_append_bytes) > intermediate_bytes {
            findings.clear();
        } else {
            findings.push(marker);
        }
    }
    Ok((
        findings,
        truncated
            .then_some(vec![GraphLimitKind::IntermediateBytes])
            .unwrap_or_default(),
    ))
}

/// Charge every graph-sized community temporary before allocating it.
///
/// The edge charge mirrors the complete, weighted relation admission used by
/// `admitted_community_scope`; excluded relation families never own community
/// clones and therefore do not consume this working-set budget.
fn community_working_set_upper_bound(
    nodes: &BTreeMap<String, DetailedRelationNode>,
    edges: &[LocalEdge],
    control: Option<&IndexWorkControl>,
) -> ServiceResult<u64> {
    let mut endpoint_bytes = 0_u64;
    for key in nodes.keys() {
        check_control(control)?;
        endpoint_bytes = endpoint_bytes
            .saturating_add(u64::try_from(key.len()).unwrap_or(u64::MAX))
            .saturating_add(std::mem::size_of::<String>() as u64);
    }
    let mut admitted_edge_count = 0_u64;
    for edge in edges {
        check_control(control)?;
        if admitted_community_edge_weight(edge).is_none() {
            continue;
        }
        admitted_edge_count = admitted_edge_count.saturating_add(1);
        endpoint_bytes = endpoint_bytes
            .saturating_add(u64::try_from(edge.source.len()).unwrap_or(u64::MAX))
            .saturating_add(u64::try_from(edge.target.len()).unwrap_or(u64::MAX))
            .saturating_add(std::mem::size_of::<String>() as u64 * 2)
            .saturating_add(std::mem::size_of::<CommunityEdgeEvidence>() as u64);
    }
    let entry_count = u64::try_from(nodes.len())
        .unwrap_or(u64::MAX)
        .saturating_add(admitted_edge_count);
    Ok(endpoint_bytes
        .saturating_mul(COMMUNITY_WORKING_SET_MULTIPLIER)
        .saturating_add(entry_count.saturating_mul(COMMUNITY_WORKING_SET_ENTRY_BYTES))
        .saturating_add(COMMUNITY_WORKING_SET_FIXED_BYTES))
}

/// Build the typed marker returned when community construction cannot fit.
fn community_truncation_finding(
    weights: &[CommunityRelationWeight],
    parameters: CommunityParameters,
    iteration: u32,
    coverage: CommunityCoverage,
) -> AnalysisFinding {
    AnalysisFinding {
        kind: AnalysisFindingKind::Community,
        status: AnalysisStatus::Inconclusive,
        summary: "community projection crossed the intermediate-byte budget before all communities were constructed"
            .to_string(),
        nodes: Vec::new(),
        metric: None,
        evidence: None,
        community: Some(CommunityAnalysis {
            id: community_id(&[], weights, parameters),
            members: Vec::new(),
            evidence: Vec::new(),
            weights: weights.to_vec(),
            parameters,
            iteration,
            convergence: CommunityConvergence::Inconclusive,
            coverage,
            truncated: true,
        }),
    }
}

/// Conservatively charge one candidate before allocating its owned vectors.
fn community_finding_upper_bound(
    nodes: &BTreeMap<String, DetailedRelationNode>,
    members: &[String],
    evidence: &[CommunityEdgeEvidence],
    weights: &[CommunityRelationWeight],
    parameters: CommunityParameters,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<u64> {
    let member_bytes = community_member_bytes(nodes, members, control)?;
    let evidence_bytes = serialized_bytes_controlled(evidence, control)?;
    let weights_bytes = serialized_bytes_controlled(weights, control)?;
    let parameter_bytes = serialized_bytes_controlled(&parameters, control)?;
    Ok(member_bytes
        .saturating_mul(2)
        .saturating_add(evidence_bytes)
        .saturating_add(weights_bytes)
        .saturating_add(parameter_bytes)
        .saturating_add((members.len() as u64).saturating_mul(2))
        .saturating_add(2_048))
}

/// Conservatively charge admitted evidence without allocating a group copy.
fn community_candidate_upper_bound(
    nodes: &BTreeMap<String, DetailedRelationNode>,
    members: &[String],
    admitted_edges: &[CommunityEdgeEvidence],
    weights: &[CommunityRelationWeight],
    parameters: CommunityParameters,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<u64> {
    let member_bytes = community_member_bytes(nodes, members, control)?;
    let mut evidence_bytes = 0_u64;
    for edge in admitted_edges {
        check_control(control)?;
        if members.binary_search(&edge.source).is_ok()
            && members.binary_search(&edge.target).is_ok()
        {
            evidence_bytes = evidence_bytes
                .saturating_add(serialized_bytes_controlled(edge, control)?)
                .saturating_add(1);
        }
    }
    let weights_bytes = serialized_bytes_controlled(weights, control)?;
    let parameter_bytes = serialized_bytes_controlled(&parameters, control)?;
    Ok(member_bytes
        .saturating_mul(2)
        .saturating_add(evidence_bytes)
        .saturating_add(weights_bytes)
        .saturating_add(parameter_bytes)
        .saturating_add((members.len() as u64).saturating_mul(2))
        .saturating_add(2_048))
}

/// Measure projected members one at a time before retaining a finding.
fn community_member_bytes(
    nodes: &BTreeMap<String, DetailedRelationNode>,
    members: &[String],
    control: Option<&IndexWorkControl>,
) -> ServiceResult<u64> {
    let mut bytes = 0_u64;
    for key in members {
        check_control(control)?;
        if let Some(node) = nodes.get(key) {
            bytes =
                bytes.saturating_add(serialized_bytes_controlled(&analysis_node(node), control)?);
        }
    }
    Ok(bytes)
}

/// Build stable metadata for one community or typed inconclusive result.
fn community_metadata(
    members: &[String],
    evidence: &[CommunityEdgeEvidence],
    weights: Vec<CommunityRelationWeight>,
    parameters: CommunityParameters,
    iteration: u32,
    convergence: CommunityConvergence,
    coverage: CommunityCoverage,
    truncated: bool,
    nodes: &BTreeMap<String, DetailedRelationNode>,
) -> CommunityAnalysis {
    let mut members = members.to_vec();
    members.sort();
    let id = community_id(&members, &weights, parameters);
    let mut evidence = evidence.to_vec();
    evidence.sort_by(|left, right| {
        (&left.source, &left.target, left.relation.as_str()).cmp(&(
            &right.source,
            &right.target,
            right.relation.as_str(),
        ))
    });
    CommunityAnalysis {
        id,
        members: analysis_nodes_for(nodes, &members),
        evidence,
        weights,
        parameters,
        iteration,
        convergence,
        coverage,
        truncated,
    }
}

/// Hash one stable community identifier from its complete versioned inputs.
fn community_id(
    members: &[String],
    weights: &[CommunityRelationWeight],
    parameters: CommunityParameters,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"projectatlas:architecture-community");
    hasher.update(&[0]);
    hasher.update(&parameters.algorithm_version.to_le_bytes());
    hasher.update(&parameters.ordering_version.to_le_bytes());
    hasher.update(&parameters.max_iterations.to_le_bytes());
    hasher.update(&parameters.node_limit.to_le_bytes());
    hasher.update(&parameters.edge_limit.to_le_bytes());
    hasher.update(&parameters.output_bytes.to_le_bytes());
    hasher.update(&COMMUNITY_SELF_WEIGHT.to_le_bytes());
    hasher.update(
        parameters
            .relation
            .map_or("none", GraphRelationKind::as_str)
            .as_bytes(),
    );
    hasher.update(&[0]);
    for entry in weights {
        hasher.update(entry.relation.as_str().as_bytes());
        hasher.update(&[0]);
        hasher.update(&entry.weight.to_le_bytes());
    }
    for member in members {
        hasher.update(member.as_bytes());
        hasher.update(&[0]);
    }
    format!(
        "community-v{}-{}",
        parameters.algorithm_version,
        hasher.finalize().to_hex()
    )
}

/// Compute deterministic weak components with optional containment exclusion.
fn weak_components(
    nodes: &BTreeMap<String, DetailedRelationNode>,
    edges: &[LocalEdge],
    exclude_contains: bool,
) -> Vec<Vec<String>> {
    let mut adjacency = nodes
        .keys()
        .map(|key| (key.clone(), Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for edge in edges {
        if exclude_contains && edge.kind == GraphRelationKind::Legacy(RelationKind::Contains) {
            continue;
        }
        adjacency
            .entry(edge.source.clone())
            .or_default()
            .push(edge.target.clone());
        adjacency
            .entry(edge.target.clone())
            .or_default()
            .push(edge.source.clone());
    }
    let mut visited = BTreeSet::new();
    let mut components = Vec::new();
    for start in nodes.keys() {
        if !visited.insert(start.clone()) {
            continue;
        }
        let mut queue = VecDeque::from([start.clone()]);
        let mut component = Vec::new();
        while let Some(node) = queue.pop_front() {
            component.push(node.clone());
            if let Some(neighbors) = adjacency.get(&node) {
                for neighbor in neighbors {
                    if visited.insert(neighbor.clone()) {
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }
        component.sort();
        components.push(component);
    }
    components
}

/// Compute iterative deterministic SCCs over dependency relations.
fn strongly_connected_components(
    nodes: &BTreeMap<String, DetailedRelationNode>,
    edges: &[LocalEdge],
) -> Vec<Vec<String>> {
    let keys = nodes.keys().cloned().collect::<Vec<_>>();
    let indices = keys
        .iter()
        .enumerate()
        .map(|(index, key)| (key.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let mut forward = vec![Vec::new(); keys.len()];
    let mut reverse = vec![Vec::new(); keys.len()];
    for edge in edges {
        let (Some(&source), Some(&target)) = (indices.get(&edge.source), indices.get(&edge.target))
        else {
            continue;
        };
        forward[source].push(target);
        reverse[target].push(source);
    }
    let mut seen = vec![false; keys.len()];
    let mut order = Vec::new();
    for start in 0..keys.len() {
        if seen[start] {
            continue;
        }
        let mut stack = vec![(start, false)];
        while let Some((node, expanded)) = stack.pop() {
            if expanded {
                order.push(node);
                continue;
            }
            if seen[node] {
                continue;
            }
            seen[node] = true;
            stack.push((node, true));
            for &next in forward[node].iter().rev() {
                if !seen[next] {
                    stack.push((next, false));
                }
            }
        }
    }
    seen.fill(false);
    let mut components = Vec::new();
    for &start in order.iter().rev() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![start];
        let mut component = Vec::new();
        while let Some(node) = stack.pop() {
            component.push(keys[node].clone());
            for &next in &reverse[node] {
                if !seen[next] {
                    seen[next] = true;
                    stack.push(next);
                }
            }
        }
        component.sort();
        components.push(component);
    }
    components
}

/// Count admitted local fan-in plus fan-out for every node.
fn degrees(
    nodes: &BTreeMap<String, DetailedRelationNode>,
    edges: &[LocalEdge],
) -> BTreeMap<String, usize> {
    let mut values = nodes
        .keys()
        .map(|key| (key.clone(), 0))
        .collect::<BTreeMap<_, _>>();
    for edge in edges {
        *values.entry(edge.source.clone()).or_default() += 1;
        *values.entry(edge.target.clone()).or_default() += 1;
    }
    values
}

/// Count trusted inbound usage relations for every node.
///
/// Structural containment establishes declaration ownership, not runtime or
/// compile-time use, so it cannot by itself suppress a dead-code candidate.
fn usage_indegrees(
    nodes: &BTreeMap<String, DetailedRelationNode>,
    edges: &[LocalEdge],
    control: Option<&IndexWorkControl>,
) -> ServiceResult<BTreeMap<String, usize>> {
    let mut values = BTreeMap::new();
    for key in nodes.keys() {
        check_control(control)?;
        values.insert(key.clone(), 0);
    }
    for edge in edges
        .iter()
        .filter(|edge| edge.kind != GraphRelationKind::Legacy(RelationKind::Contains))
    {
        check_control(control)?;
        *values.entry(edge.target.clone()).or_default() += 1;
    }
    Ok(values)
}

/// Project selected canonical identities into reusable analysis nodes.
fn analysis_nodes_for(
    nodes: &BTreeMap<String, DetailedRelationNode>,
    keys: &[String],
) -> Vec<AnalysisNode> {
    keys.iter()
        .filter_map(|key| nodes.get(key))
        .map(analysis_node)
        .collect()
}

/// Preserve one detailed node and its exact reusable next call.
fn analysis_node(node: &DetailedRelationNode) -> AnalysisNode {
    let next_call = super::relations::next_call_for_entity(
        &node.entity,
        node.content_selection
            .unwrap_or(ContentSelection::UnspecifiedLegacy),
        node.classification,
    );
    AnalysisNode {
        node: node.clone(),
        next_call,
    }
}

/// Compare a normalized graph entity with an exact public relation anchor.
fn entity_matches_anchor(entity: &GraphEntity, target: &RelationAnchor) -> bool {
    match (entity.selector(), target) {
        (EntitySelector::File { path }, RelationAnchor::File { file }) => path == file,
        (
            EntitySelector::Symbol { symbol },
            RelationAnchor::Symbol {
                file,
                name,
                symbol_kind,
                parent,
                signature,
            },
        ) => {
            symbol.file == *file
                && symbol.name.as_str() == name
                && symbol_kind.is_none_or(|kind| symbol.kind == kind)
                && symbol.parent.as_ref().map(GraphIdentityText::as_str) == parent.as_deref()
                && signature
                    .as_deref()
                    .is_none_or(|signature| symbol.signature.as_str() == signature)
        }
        _ => false,
    }
}

/// Return the owning repository path for a file or symbol entity.
fn entity_path(entity: &GraphEntity) -> Option<&str> {
    match entity.selector() {
        EntitySelector::Folder { path } => Some(path.as_str()),
        EntitySelector::File { path } => Some(path.as_str()),
        EntitySelector::Package { package } => Some(package.manifest.as_str()),
        EntitySelector::Symbol { symbol } => Some(symbol.file.as_str()),
        EntitySelector::Project | EntitySelector::External { .. } => None,
    }
}

/// Borrow the exact declaration identity from one symbol entity.
fn symbol_identity(
    entity: &GraphEntity,
) -> Option<(
    &str,
    &str,
    projectatlas_core::symbols::SymbolKind,
    Option<&str>,
    &str,
)> {
    let EntitySelector::Symbol { symbol } = entity.selector() else {
        return None;
    };
    Some((
        symbol.file.as_str(),
        symbol.name.as_str(),
        symbol.kind,
        symbol.parent.as_ref().map(GraphIdentityText::as_str),
        symbol.signature.as_str(),
    ))
}

/// Clone one fitted finding prefix and update its returned work.
fn analysis_prefix(
    report: &RelationAnalysisReport,
    rows: usize,
    reached_limits: impl IntoIterator<Item = GraphLimitKind>,
) -> RelationAnalysisReport {
    let mut candidate = report.clone();
    if rows < candidate.findings.len() {
        candidate.findings.truncate(rows);
        candidate.truncated = true;
        for limit in reached_limits {
            push_limit(&mut candidate.reached_limits, limit);
        }
    }
    candidate.returned = u32::try_from(candidate.findings.len()).unwrap_or(u32::MAX);
    candidate.work.rendered_output_bytes = 0;
    candidate
}

/// Insert one stable limit once while preserving encounter order.
fn push_limit(limits: &mut Vec<GraphLimitKind>, limit: GraphLimitKind) {
    if !limits.contains(&limit) {
        limits.push(limit);
    }
}

/// Check shared cancellation and deadline state at traversal work boundaries.
fn check_control(control: Option<&IndexWorkControl>) -> ServiceResult<()> {
    if let Some(control) = control {
        control
            .check(IndexWorkStage::RepositoryTraversal)
            .map_err(DbError::from)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "analysis/tests.rs"]
mod tests;
