//! Bounded detailed relation navigation over normalized repository graph storage.

use super::{ServiceError, ServiceResult, selected_project_binding};
use projectatlas_core::graph::{
    ConfidenceClass, CoverageRecord, CoverageScope, EntitySelector, GraphEntity, GraphEntityKey,
    GraphLimitKind, GraphLimits, GraphRelationKind, LogicalRelation, RelationOccurrence,
    RelationResolution, RepositoryFilePath, RepositoryNodePath, SymbolSelector,
};
use projectatlas_core::symbols::SymbolKind;
use projectatlas_core::{IndexWorkControl, Purpose, PurposeSource, PurposeStatus};
use projectatlas_db::{
    AtlasStore, MAX_REPOSITORY_GRAPH_FRONTIER, RepositoryGraphAdjacencyContinuation,
    RepositoryGraphDirection, RepositoryGraphRelationRow,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Stable maximum rows materialized by the generated adjacency statement.
const ADJACENCY_WORK_ROWS: usize = GraphLimits::MAX_ROWS as usize + 1;

/// Direction of one detailed relation request from its selected anchor.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
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
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
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
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
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
    /// Result, occurrence, depth, and output ceilings.
    pub limits: GraphLimits,
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
    },
    /// Inspect a file or package owner through the existing summary route.
    Summary {
        /// Exact file selector.
        file: RepositoryFilePath,
    },
    /// Inspect one declaration through the existing exact slice route.
    SymbolSlice {
        /// Stable declaration selector.
        symbol: SymbolSelector,
    },
}

/// One endpoint with authoritative purpose state projected at response time.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DetailedRelationNode {
    /// Typed generation-bound graph entity.
    pub entity: GraphEntity,
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
    /// Exact relation source with purpose projection.
    pub source: DetailedRelationNode,
    /// Retained resolved or external target with purpose projection.
    pub target: Option<DetailedRelationNode>,
    /// Purpose disposition for the target, including unresolved identities.
    pub target_purpose: RelationPurpose,
    /// Stable node-simple path from the anchor through this step.
    pub path: Vec<GraphEntityKey>,
    /// Exact supporting source occurrences when requested.
    pub occurrences: Vec<RelationOccurrence>,
    /// Whether the per-relation occurrence ceiling omitted additional rows.
    pub occurrences_truncated: bool,
    /// Existing call that accepts the exact local target selector.
    pub next_call: Option<RelationNextCall>,
}

/// Bounded detailed relation traversal returned to CLI and MCP adapters.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DetailedRelationReport {
    /// Exact selected local anchor.
    pub anchor: DetailedRelationNode,
    /// Direction followed from the anchor.
    pub direction: RelationDirection,
    /// Number of retained relation steps.
    pub returned: usize,
    /// Number of adjacency rows inspected within the edge budget.
    pub inspected_edges: usize,
    /// Number of cyclic or lower-ranked duplicate-node paths pruned.
    pub pruned_paths: usize,
    /// Whether any declared result boundary stopped the traversal.
    pub truncated: bool,
    /// Stable unique hard limits reached while constructing the response.
    pub reached_limits: Vec<GraphLimitKind>,
    /// Ranked node-simple relation steps.
    pub rows: Vec<DetailedRelationRow>,
}

/// Internal relation row before owner-purpose and occurrence projection.
struct TraversalRow {
    /// One-based distance from the selected anchor.
    depth: u32,
    /// Fully hydrated normalized relation row.
    detail: RepositoryGraphRelationRow,
    /// Node-simple entity-key path reaching this row.
    path: Vec<GraphEntityKey>,
}

/// Load one detailed relation traversal through a stable store snapshot.
///
/// # Errors
///
/// Returns an error when the exact anchor is absent or ambiguous, graph rows
/// are invalid, cancellation fires, or a bounded database read fails.
pub fn load_detailed_relations(
    store: &AtlasStore,
    query: &DetailedRelationQuery,
    control: Option<&IndexWorkControl>,
) -> ServiceResult<DetailedRelationReport> {
    let binding = selected_project_binding(store)?;
    let anchor = resolve_anchor(store, binding.project_instance_id, &query.anchor)?;
    let anchor_digest = anchor.key().digest_bytes().map_err(invalid_graph_input)?;
    let mut visited = HashSet::from([anchor_digest]);
    let mut paths = HashMap::from([(anchor_digest, vec![anchor.key().clone()])]);
    let mut frontier = vec![anchor.key().clone()];
    let mut retained = Vec::new();
    let mut inspected_edges = 0usize;
    let mut pruned_paths = 0usize;
    let mut reached_limits = Vec::new();
    let row_limit = query.limits.rows() as usize;

    for depth in 1..=query.limits.depth() {
        if frontier.is_empty() || inspected_edges >= row_limit || retained.len() >= row_limit {
            break;
        }
        let mut depth_rows = read_frontier(
            store,
            &frontier,
            query,
            control,
            row_limit.saturating_sub(inspected_edges),
            &mut inspected_edges,
            &mut reached_limits,
        )?;
        depth_rows.retain(|row| relation_matches(&row.detail.relation, query));
        depth_rows.sort_by(|left, right| relation_rank_order(&left.detail, &right.detail));

        let mut next_frontier = Vec::new();
        for row in depth_rows {
            if retained.len() >= row_limit {
                push_limit(&mut reached_limits, GraphLimitKind::Rows);
                break;
            }
            let frontier_key = &frontier[row.frontier_index as usize];
            let frontier_digest = frontier_key.digest_bytes().map_err(invalid_graph_input)?;
            let mut path = paths.get(&frontier_digest).cloned().ok_or_else(|| {
                ServiceError::InvalidInput(
                    "graph traversal lost the selected frontier path".to_string(),
                )
            })?;
            let next = traversable_entity(&row.detail, query.direction);
            if let Some(next) = next {
                let digest = next.key().digest_bytes().map_err(invalid_graph_input)?;
                if visited.contains(&digest) {
                    pruned_paths = pruned_paths.saturating_add(1);
                    continue;
                }
                visited.insert(digest);
                path.push(next.key().clone());
                paths.insert(digest, path.clone());
                if next_frontier.len() < MAX_REPOSITORY_GRAPH_FRONTIER {
                    next_frontier.push(next.key().clone());
                } else {
                    push_limit(&mut reached_limits, GraphLimitKind::Rows);
                }
            }
            retained.push(TraversalRow {
                depth,
                detail: row.detail,
                path,
            });
        }
        frontier = next_frontier;
        if depth == query.limits.depth() && !frontier.is_empty() {
            push_limit(&mut reached_limits, GraphLimitKind::Depth);
        }
    }

    let purposes = load_purposes(store, &anchor, &retained, control)?;
    let coverage = load_coverage(
        store,
        binding.project_instance_id,
        &anchor,
        &retained,
        control,
    )?;
    let anchor_node = detailed_node(anchor, &purposes, &coverage);
    let occurrence_pages = load_occurrence_pages(store, &retained, query, control)?;
    if occurrence_pages.iter().any(|(_, truncated)| *truncated) {
        push_limit(&mut reached_limits, GraphLimitKind::Occurrences);
    }
    let rows = retained
        .into_iter()
        .zip(occurrence_pages)
        .map(|(row, occurrences)| detailed_row(row, query, &purposes, &coverage, occurrences))
        .collect::<Vec<_>>();
    let truncated = !reached_limits.is_empty();
    let mut report = DetailedRelationReport {
        anchor: anchor_node,
        direction: query.direction,
        returned: rows.len(),
        inspected_edges,
        pruned_paths,
        truncated,
        reached_limits,
        rows,
    };
    enforce_output_limit(&mut report, query.limits.output_bytes())?;
    Ok(report)
}

/// One adjacency row retaining the selecting frontier position.
struct FrontierRow {
    /// Position of the selecting key in the current frontier.
    frontier_index: u32,
    /// Fully hydrated normalized relation row.
    detail: RepositoryGraphRelationRow,
}

/// Read every bounded `SQLite` adjacency page needed for one service frontier.
fn read_frontier(
    store: &AtlasStore,
    frontier: &[GraphEntityKey],
    query: &DetailedRelationQuery,
    control: Option<&IndexWorkControl>,
    remaining_edges: usize,
    inspected_edges: &mut usize,
    reached_limits: &mut Vec<GraphLimitKind>,
) -> ServiceResult<Vec<FrontierRow>> {
    let per_page = (ADJACENCY_WORK_ROWS / frontier.len())
        .saturating_sub(1)
        .min(remaining_edges)
        .max(1);
    let page_limit = u32::try_from(per_page).map_err(|_overflow| {
        ServiceError::InvalidInput("graph adjacency page limit overflowed".to_string())
    })?;
    let mut continuation: Option<RepositoryGraphAdjacencyContinuation> = None;
    let mut rows = Vec::new();
    loop {
        let page = store.repository_graph_adjacency_page_filtered(
            frontier,
            query.direction.into(),
            query.relation,
            continuation.as_ref(),
            page_limit,
            control,
        )?;
        *inspected_edges = inspected_edges.saturating_add(page.rows.len());
        rows.extend(page.rows.into_iter().map(|row| FrontierRow {
            frontier_index: row.frontier_index,
            detail: row.detail,
        }));
        if *inspected_edges >= query.limits.rows() as usize {
            if page.truncated {
                push_limit(reached_limits, GraphLimitKind::Rows);
            }
            break;
        }
        if !page.truncated {
            break;
        }
        continuation = page.continuation;
        if continuation.is_none() {
            return Err(ServiceError::InvalidInput(
                "truncated graph adjacency page omitted its continuation".to_string(),
            ));
        }
    }
    Ok(rows)
}

/// Resolve one exact file or symbol anchor without falling back to discovery.
fn resolve_anchor(
    store: &AtlasStore,
    project: projectatlas_core::graph::ProjectInstanceId,
    anchor: &RelationAnchor,
) -> ServiceResult<GraphEntity> {
    match anchor {
        RelationAnchor::File { file } => {
            let selector = EntitySelector::File { path: file.clone() };
            let key = GraphEntityKey::new(project, &selector);
            store.repository_graph_entity(&key)?.ok_or_else(|| {
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
            let page =
                store.repository_graph_entities_by_path(project, &path, GraphLimits::MAX_ROWS)?;
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
fn relation_matches(relation: &LogicalRelation, query: &DetailedRelationQuery) -> bool {
    query.relation.is_none_or(|kind| relation.kind() == kind)
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
    anchor: &GraphEntity,
    rows: &[TraversalRow],
    control: Option<&IndexWorkControl>,
) -> ServiceResult<BTreeMap<String, Purpose>> {
    let mut paths = BTreeSet::new();
    if let Some(path) = purpose_owner(anchor) {
        paths.insert(path);
    }
    for row in rows {
        if let Some(path) = purpose_owner(&row.detail.source) {
            paths.insert(path);
        }
        if let Some(target) = &row.detail.target
            && let Some(path) = purpose_owner(target)
        {
            paths.insert(path);
        }
    }
    let selected = paths.into_iter().collect::<Vec<_>>();
    Ok(store
        .load_nodes_by_paths_controlled(&selected, control)?
        .into_iter()
        .map(|node| (node.node.path.clone(), node.purpose))
        .collect())
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
    let Some(path) = purpose_owner(entity) else {
        return RelationPurpose::NotApplicable;
    };
    let Some(purpose) = purposes.get(&path) else {
        return RelationPurpose::Unavailable { path: Some(path) };
    };
    if purpose.status == PurposeStatus::Approved
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
    RelationPurpose::Unavailable { path: Some(path) }
}

/// Compose one hydrated response node from graph, purpose, and coverage state.
fn detailed_node(
    entity: GraphEntity,
    purposes: &BTreeMap<String, Purpose>,
    coverage: &BTreeMap<String, Vec<CoverageRecord>>,
) -> DetailedRelationNode {
    let purpose = purpose_projection(&entity, purposes);
    let coverage = purpose_owner(&entity)
        .and_then(|path| coverage.get(&path).cloned())
        .unwrap_or_default();
    DetailedRelationNode {
        entity,
        purpose,
        coverage,
    }
}

/// Compose one public traversal row from its internal retained state.
fn detailed_row(
    row: TraversalRow,
    query: &DetailedRelationQuery,
    purposes: &BTreeMap<String, Purpose>,
    coverage: &BTreeMap<String, Vec<CoverageRecord>>,
    occurrence_page: (Vec<RelationOccurrence>, bool),
) -> DetailedRelationRow {
    let (occurrences, occurrences_truncated) = occurrence_page;
    let next_call = traversable_entity(&row.detail, query.direction).and_then(next_call_for_entity);
    let target_purpose = row
        .detail
        .target
        .as_ref()
        .map_or(RelationPurpose::Unavailable { path: None }, |target| {
            purpose_projection(target, purposes)
        });
    DetailedRelationRow {
        depth: row.depth,
        direction: query.direction,
        relation: row.detail.relation,
        source: detailed_node(row.detail.source, purposes, coverage),
        target: row
            .detail
            .target
            .map(|target| detailed_node(target, purposes, coverage)),
        target_purpose,
        path: row.path,
        occurrences,
        occurrences_truncated,
        next_call,
    }
}

/// Batch-load authoritative path coverage for every unique local owner.
fn load_coverage(
    store: &AtlasStore,
    project: projectatlas_core::graph::ProjectInstanceId,
    anchor: &GraphEntity,
    rows: &[TraversalRow],
    control: Option<&IndexWorkControl>,
) -> ServiceResult<BTreeMap<String, Vec<CoverageRecord>>> {
    let mut paths = BTreeSet::new();
    for entity in std::iter::once(anchor).chain(
        rows.iter()
            .flat_map(|row| std::iter::once(&row.detail.source).chain(row.detail.target.iter())),
    ) {
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
        let page = store.repository_graph_path_coverage(project, &normalized, control)?;
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
    control: Option<&IndexWorkControl>,
) -> ServiceResult<Vec<(Vec<RelationOccurrence>, bool)>> {
    if !query.include_occurrences {
        return Ok((0..rows.len()).map(|_| (Vec::new(), false)).collect());
    }
    let per_relation = query.limits.occurrences() as usize + 1;
    let batch_size = (ADJACENCY_WORK_ROWS / per_relation).clamp(1, MAX_REPOSITORY_GRAPH_FRONTIER);
    let mut pages = Vec::with_capacity(rows.len());
    for chunk in rows.chunks(batch_size) {
        let relations = chunk
            .iter()
            .map(|row| row.detail.relation.clone())
            .collect::<Vec<_>>();
        pages.extend(
            store
                .repository_graph_occurrence_pages(&relations, query.limits.occurrences(), control)?
                .into_iter()
                .map(|page| (page.rows, page.truncated)),
        );
    }
    Ok(pages)
}

/// Map one resolved local entity to an existing exact navigation call.
fn next_call_for_entity(entity: &GraphEntity) -> Option<RelationNextCall> {
    match entity.selector() {
        EntitySelector::Project | EntitySelector::External { .. } => None,
        EntitySelector::Folder { path } => Some(RelationNextCall::Files {
            folder: path.clone(),
        }),
        EntitySelector::File { path } => Some(RelationNextCall::Summary { file: path.clone() }),
        EntitySelector::Package { package } => Some(RelationNextCall::Summary {
            file: package.manifest.clone(),
        }),
        EntitySelector::Symbol { symbol } => Some(RelationNextCall::SymbolSlice {
            symbol: symbol.clone(),
        }),
    }
}

/// Trim complete rows until the serialized report fits its hard byte ceiling.
fn enforce_output_limit(
    report: &mut DetailedRelationReport,
    output_bytes: u32,
) -> ServiceResult<()> {
    let maximum = output_bytes as usize;
    while serde_json::to_vec(report)?.len() > maximum {
        if report.rows.pop().is_none() {
            return Err(ServiceError::InvalidInput(
                "graph output byte limit is too small for the empty response envelope".to_string(),
            ));
        }
        push_limit(&mut report.reached_limits, GraphLimitKind::OutputBytes);
        report.returned = report.rows.len();
        report.truncated = true;
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
        Completeness, CoverageState, GraphIdentityText, RelationResolution, SourceSpan,
    };
    use projectatlas_core::symbols::RelationKind;
    use projectatlas_core::{IndexGeneration, Node, NodeKind};
    use std::error::Error;
    use std::fs;
    use std::io;
    use std::path::Path;

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
            test_node("src/a.rs", "hash-a"),
            test_node("src/b.rs", "hash-b"),
        ])?;
        publication.finish_scan_replacement()?;
        publication.replace_repository_graph(
            project,
            &[source, target],
            &[forward, backward, unresolved],
            &[occurrence],
            &coverage,
        )?;
        publication.complete()?;
        store.set_purpose("src/a.rs", "Own source calls", PurposeSource::Agent)?;
        store.set_purpose("src/b.rs", "Own target behavior", PurposeSource::Agent)?;
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
                limits: GraphLimits::new(10, 10, 3, 64 * 1024)?,
            },
            None,
        )?;

        require(report.returned == 1, "resolved row count changed")?;
        require(report.inspected_edges == 3, "inspected edge count changed")?;
        require(report.pruned_paths == 1, "cycle pruning count changed")?;
        require(!report.truncated, "complete traversal reported truncation")?;
        require(report.rows[0].depth == 1, "resolved row depth changed")?;
        require(report.rows[0].path.len() == 2, "node-simple path changed")?;
        require(
            report.rows[0].occurrences.len() == 1,
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
                    purpose: RelationPurpose::Approved { ref purpose, .. },
                    ..
                }) if purpose == "Own target behavior"
            ),
            "target purpose projection changed",
        )?;
        require(
            matches!(
                report.rows[0].next_call,
                Some(RelationNextCall::Summary { ref file }) if file.as_str() == "src/b.rs"
            ),
            "resolved target next call changed",
        )?;
        let exact_output_limit = u32::try_from(serde_json::to_vec(&report)?.len() - 1)?;
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
                limits: GraphLimits::new(10, 10, 3, exact_output_limit)?,
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
                limits: GraphLimits::new(10, 10, 1, 64 * 1024)?,
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
        Ok(())
    }

    /// Return one ordinary test error instead of panicking inside fallible tests.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            return Ok(());
        }
        Err(io::Error::other(message).into())
    }

    fn test_node(path: &str, hash: &str) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::File,
            parent_path: Some("src".to_string()),
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(16),
            mtime_ns: Some(1),
            content_hash: Some(hash.to_string()),
        }
    }
}
