//! Normalized repository-graph persistence and bounded prepared queries.

use super::{
    AtlasStore, DbError, DbResult, IndexPublicationGuard, IndexPublicationState, count_to_usize,
    with_sqlite_read_progress,
};
use crate::project_identity::{
    load_graph_generation, load_project_identity, require_bound_project_identity,
    set_graph_generation, verify_project_identity,
};
use projectatlas_core::graph::{
    CanonicalResolutionKey, Completeness, ConfidenceClass, CoverageRecord, CoverageScope,
    CoverageState, EntityResolutionKey, EntitySelector, ExtendedRelationKind, ExternalSelector,
    GraphContractError, GraphEntity, GraphEntityKey, GraphIdentityText, GraphLimitKind,
    GraphLimits, GraphRelationKind, LogicalRelation, PackageSelector, ProjectInstanceId,
    RelationDependencyKey, RelationOccurrence, RelationResolution, RepositoryFilePath,
    RepositoryNodePath, ResolutionKeyDomain, SourceSpan, SymbolSelector,
};
use projectatlas_core::symbols::{ParserKind, RelationKind, SymbolKind};
use projectatlas_core::{
    IndexGeneration, IndexWorkControl, IndexWorkStage, NodeKind, RankedConnection,
    RankedConnectionCount, RankedConnectionDirection, RankedConnectionKind, RankedConnectionTarget,
};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::num::{NonZeroU32, NonZeroU64};
use std::path::Path;

/// One bounded page of typed normalized graph rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryGraphPage<T> {
    /// Fully validated rows in deterministic storage order.
    pub rows: Vec<T>,
    /// Whether at least one additional validated row exists.
    pub truncated: bool,
}

/// Validated resource envelope for one bounded repository-graph database read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryGraphReadBudget {
    /// Maximum input keys, frontier selectors, or purpose-owner paths.
    requested_rows: NonZeroU32,
    /// Maximum fully reconstructed rows returned to the caller.
    returned_rows: NonZeroU32,
    /// Maximum raw `SQLite` payload bytes decoded by the complete batch.
    decoded_bytes: NonZeroU64,
    /// Maximum unique entities reconstructed by the complete batch.
    hydrated_entities: NonZeroU32,
    /// Maximum unique purpose-owning repository paths retained from entities.
    hydrated_paths: NonZeroU32,
}

impl RepositoryGraphReadBudget {
    /// Absolute compact-key or purpose-path request ceiling for one batch.
    pub const MAX_REQUESTED_ROWS: u32 = GraphLimits::MAX_ROWS;
    /// Absolute reconstructed-row ceiling for one batch.
    pub const MAX_RETURNED_ROWS: u32 = GraphLimits::MAX_ROWS;
    /// Absolute decoded payload ceiling for one hydration batch.
    pub const MAX_DECODED_BYTES: u64 = 32 * 1_024 * 1_024;
    /// Absolute unique-entity ceiling including one adjacency sentinel.
    pub const MAX_HYDRATED_ENTITIES: u32 = 2 * (GraphLimits::MAX_ROWS + 1);
    /// Absolute unique purpose-owner path ceiling for one hydration batch.
    pub const MAX_HYDRATED_PATHS: u32 = 2 * (GraphLimits::MAX_ROWS + 1);

    /// Construct one bounded repository-graph read envelope.
    ///
    /// # Errors
    ///
    /// Returns an error when a limit is zero or above its absolute batch
    /// ceiling.
    pub fn new(
        requested_rows: u32,
        returned_rows: u32,
        decoded_bytes: u64,
        hydrated_entities: u32,
        hydrated_paths: u32,
    ) -> Result<Self, GraphContractError> {
        if requested_rows == 0 || requested_rows > Self::MAX_REQUESTED_ROWS {
            return Err(GraphContractError::InvalidLimits {
                reason: "graph read requested-row budget is zero or above the batch ceiling",
            });
        }
        if returned_rows == 0 || returned_rows > Self::MAX_RETURNED_ROWS {
            return Err(GraphContractError::InvalidLimits {
                reason: "graph read returned-row budget is zero or above the batch ceiling",
            });
        }
        if decoded_bytes == 0 || decoded_bytes > Self::MAX_DECODED_BYTES {
            return Err(GraphContractError::InvalidLimits {
                reason: "graph read decoded-byte budget is zero or above the batch ceiling",
            });
        }
        if hydrated_entities == 0 || hydrated_entities > Self::MAX_HYDRATED_ENTITIES {
            return Err(GraphContractError::InvalidLimits {
                reason: "graph read entity budget is zero or above the batch ceiling",
            });
        }
        if hydrated_paths == 0 || hydrated_paths > Self::MAX_HYDRATED_PATHS {
            return Err(GraphContractError::InvalidLimits {
                reason: "graph read path budget is zero or above the batch ceiling",
            });
        }
        Ok(Self {
            requested_rows: NonZeroU32::new(requested_rows).ok_or(
                GraphContractError::InvalidLimits {
                    reason: "graph read requested-row budget must be nonzero",
                },
            )?,
            returned_rows: NonZeroU32::new(returned_rows).ok_or(
                GraphContractError::InvalidLimits {
                    reason: "graph read returned-row budget must be nonzero",
                },
            )?,
            decoded_bytes: NonZeroU64::new(decoded_bytes).ok_or(
                GraphContractError::InvalidLimits {
                    reason: "graph read decoded-byte budget must be nonzero",
                },
            )?,
            hydrated_entities: NonZeroU32::new(hydrated_entities).ok_or(
                GraphContractError::InvalidLimits {
                    reason: "graph read entity budget must be nonzero",
                },
            )?,
            hydrated_paths: NonZeroU32::new(hydrated_paths).ok_or(
                GraphContractError::InvalidLimits {
                    reason: "graph read path budget must be nonzero",
                },
            )?,
        })
    }

    /// Maximum input keys, frontier selectors, or purpose-owner paths.
    #[must_use]
    pub const fn requested_rows(self) -> u32 {
        self.requested_rows.get()
    }

    /// Maximum fully reconstructed rows.
    #[must_use]
    pub const fn returned_rows(self) -> u32 {
        self.returned_rows.get()
    }

    /// Maximum decoded raw payload bytes.
    #[must_use]
    pub const fn decoded_bytes(self) -> u64 {
        self.decoded_bytes.get()
    }

    /// Maximum unique hydrated entities.
    #[must_use]
    pub const fn hydrated_entities(self) -> u32 {
        self.hydrated_entities.get()
    }

    /// Maximum unique purpose-owner paths.
    #[must_use]
    pub const fn hydrated_paths(self) -> u32 {
        self.hydrated_paths.get()
    }
}

/// Exact work observed while hydrating one stable-key graph batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryGraphReadWork {
    /// Input keys, frontier selectors, or purpose-owner paths supplied.
    pub requested_rows: u32,
    /// Fully reconstructed rows returned to the caller.
    pub returned_rows: u32,
    /// Raw `SQLite` BLOB, TEXT, and fixed scalar bytes decoded.
    pub decoded_bytes: u64,
    /// Unique entities reconstructed from normalized rows.
    pub hydrated_entities: u32,
    /// Unique purpose-owning repository paths retained from those entities.
    pub hydrated_paths: u32,
}

/// Fully reconstructed graph rows plus their exact bounded read work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryGraphReadBatch<T> {
    /// Fully validated rows in caller key order.
    pub rows: Vec<T>,
    /// Exact resource use for the complete successful batch.
    pub work: RepositoryGraphReadWork,
}

/// One stable paged graph result plus exact database work for the page attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryGraphReadPage<T> {
    /// Stable bounded page, including its truncation sentinel result.
    pub page: RepositoryGraphPage<T>,
    /// Exact work for all decoded rows, including a removed sentinel.
    pub work: RepositoryGraphReadWork,
}

/// Ordered per-owner graph pages plus exact aggregate database work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryGraphReadPages<T> {
    /// Pages in caller owner order.
    pub pages: Vec<RepositoryGraphPage<T>>,
    /// Exact aggregate work across every page and truncation sentinel.
    pub work: RepositoryGraphReadWork,
}

/// Bounded filters for opt-in project-wide coverage discovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryCoverageQuery {
    /// Zero-based result offset after filters are applied.
    pub start_index: u32,
    /// Maximum rows returned before the overflow sentinel.
    pub limit: u32,
    /// Optional normalized repository path prefix.
    pub path_prefix: Option<String>,
    /// Optional source parser pass.
    pub parser: Option<ParserKind>,
    /// Optional derived-fact provider pass.
    pub provider: Option<ParserKind>,
    /// Optional relation family.
    pub relation: Option<GraphRelationKind>,
    /// Optional coverage lifecycle state.
    pub state: Option<CoverageState>,
    /// Optional exact persisted reason.
    pub reason: Option<String>,
}

/// One discovered coverage row with parse and fact-provider provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryCoverageRow {
    /// Validated normalized graph coverage record.
    pub coverage: CoverageRecord,
    /// Source parser pass for path-scoped coverage.
    pub parser: Option<ParserKind>,
    /// Fact provider pass for path-scoped coverage.
    pub provider: Option<ParserKind>,
}

/// One folder or file whose current graph context should enrich navigation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryNavigationNode {
    /// Exact repository-relative path.
    pub path: String,
    /// Folder or file ownership semantics used by the set query.
    pub kind: NodeKind,
}

/// Bounded current graph evidence for one folder or file navigation row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryNavigationConnections {
    /// Exact repository-relative owner path.
    pub path: String,
    /// Sparse stable-order family counts.
    pub counts: Vec<RankedConnectionCount>,
    /// Bounded stable-order connection sample.
    pub connections: Vec<RankedConnection>,
    /// Whether the bounded sample omitted any validated relation through family or global overflow.
    pub truncated: bool,
}

/// Maximum owners admitted to one generated set-oriented navigation statement.
const NAVIGATION_CONNECTION_OWNER_CHUNK: usize = 8;

/// Stable family order and normalized persisted selectors for navigation context.
const NAVIGATION_CONNECTION_FAMILIES: &[(RankedConnectionKind, &str, &str)] = &[
    (RankedConnectionKind::Package, "legacy", "depends-on"),
    (RankedConnectionKind::Import, "legacy", "imports"),
    (RankedConnectionKind::Call, "legacy", "calls"),
    (RankedConnectionKind::Reference, "extended", "references"),
    (RankedConnectionKind::Test, "extended", "tests"),
    (RankedConnectionKind::Route, "extended", "routes-to"),
    (RankedConnectionKind::Config, "extended", "configures"),
];

/// Conservative persisted footprint owned by exact affected source paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryAffectedSourceFootprint {
    /// Existing persisted rows, including a conservative resolution-witness allowance.
    pub rows: u64,
    /// UTF-8, BLOB, and fixed-width scalar bytes represented by those rows.
    pub retained_bytes: u64,
    /// Whether `rows` reached the caller's `LIMIT + 1` overflow sentinel.
    pub truncated: bool,
}

/// One persisted export candidate paired with the canonical key that selected it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryResolutionCandidate {
    /// Exact canonical key exported by the entity.
    key: CanonicalResolutionKey,
    /// Typed entity that currently exports the key.
    entity: GraphEntity,
}

impl RepositoryResolutionCandidate {
    /// Borrow the canonical key that selected this candidate.
    #[must_use]
    pub const fn key(&self) -> &CanonicalResolutionKey {
        &self.key
    }

    /// Borrow the typed export candidate.
    #[must_use]
    pub const fn entity(&self) -> &GraphEntity {
        &self.entity
    }
}

/// Closed relation lookup shapes owned by normalized graph storage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryGraphRelationQuery {
    /// Relations whose source is one exact stable entity.
    Outbound {
        /// Exact project-qualified source key.
        source: GraphEntityKey,
    },
    /// Relations whose resolved or external target is one exact stable entity.
    Inbound {
        /// Exact project-qualified target key.
        target: GraphEntityKey,
    },
    /// Relations in one typed legacy or extended family.
    Family {
        /// Exact relation family.
        relation: GraphRelationKind,
    },
}

/// Direction of one batched normalized-graph adjacency read.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum RepositoryGraphDirection {
    /// Relations whose source is in the selected frontier.
    Outbound,
    /// Relations whose retained target is in the selected frontier.
    Inbound,
}

/// One normalized relation with the endpoint entities already hydrated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryGraphRelationRow {
    /// Fully reconstructed normalized relation.
    pub relation: LogicalRelation,
    /// Exact source entity named by the relation.
    pub source: GraphEntity,
    /// Retained resolved or external target, when the relation has one.
    pub target: Option<GraphEntity>,
}

/// Opaque keyset used only to continue one bounded adjacency request.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct RepositoryGraphAdjacencyContinuation {
    /// Project whose read snapshot produced this keyset.
    project: ProjectInstanceId,
    /// Complete graph generation whose read snapshot produced this keyset.
    generation: IndexGeneration,
    /// Direction whose stable order produced this keyset.
    direction: RepositoryGraphDirection,
    /// Optional exact relation family whose stable order produced this keyset.
    relation: Option<GraphRelationKind>,
    /// Ordered frontier identity whose result order produced this keyset.
    frontier: Vec<[u8; 32]>,
    /// Zero-based frontier position of the last returned relation.
    frontier_index: u32,
    /// Persisted relation family scope of the last returned relation.
    relation_scope: String,
    /// Persisted relation family value of the last returned relation.
    relation_kind: String,
    /// Canonical identity of the last returned relation.
    canonical_identity: String,
    /// Stable compact key of the last returned relation.
    relation_key: [u8; 32],
}

/// One logical relation paired with the frontier entity that selected it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryGraphAdjacencyRow {
    /// Zero-based position of the selecting entity in the request frontier.
    pub frontier_index: u32,
    /// Exact project-qualified entity that selected this relation.
    pub frontier: GraphEntityKey,
    /// Direction relative to the selecting frontier entity.
    pub direction: RepositoryGraphDirection,
    /// Normalized relation and its already-hydrated endpoints.
    pub detail: RepositoryGraphRelationRow,
}

/// One bounded direction-specific adjacency page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryGraphAdjacencyPage {
    /// Fully validated rows in deterministic frontier and relation order.
    pub rows: Vec<RepositoryGraphAdjacencyRow>,
    /// Whether at least one additional validated relation exists.
    pub truncated: bool,
    /// Opaque continuation for the same frontier and direction when truncated.
    pub continuation: Option<RepositoryGraphAdjacencyContinuation>,
}

/// One bounded adjacency page plus exact database work for the page attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryGraphAdjacencyReadPage {
    /// Stable direction-specific page and continuation state.
    pub page: RepositoryGraphAdjacencyPage,
    /// Exact raw-row and endpoint hydration work for the complete page.
    pub work: RepositoryGraphReadWork,
}

/// Maximum unique entities admitted to one batched adjacency statement.
pub const MAX_REPOSITORY_GRAPH_FRONTIER: usize = 256;

/// Maximum relation rows admitted across all per-frontier query branches.
const MAX_REPOSITORY_GRAPH_ADJACENCY_WORK_ROWS: usize = GraphLimits::MAX_ROWS as usize + 1;

/// Maximum stable entity keys hydrated through one prepared `VALUES` join.
const GRAPH_ENTITY_HYDRATION_CHUNK: usize = 128;

/// Raw normalized entity row collected before domain reconstruction.
struct EntityRow {
    /// Compact stable entity key.
    key: Vec<u8>,
    /// Owning project identity.
    project: Vec<u8>,
    /// Canonical collision witness.
    canonical: String,
    /// Normalized selector variant.
    kind: String,
    /// Folder, file, or symbol repository path.
    repository_path: Option<String>,
    /// Package ecosystem.
    package_manager: Option<String>,
    /// Manifest package name.
    package_name: Option<String>,
    /// Owning package manifest.
    manifest_path: Option<String>,
    /// Declaration name.
    symbol_name: Option<String>,
    /// Declaration kind.
    symbol_kind: Option<String>,
    /// Optional containing declaration.
    symbol_parent: Option<String>,
    /// Stable declaration signature.
    symbol_signature: Option<String>,
    /// External namespace.
    external_system: Option<String>,
    /// Identity inside the external namespace.
    external_identity: Option<String>,
}

/// Raw normalized relation row collected before domain reconstruction.
struct RelationRow {
    /// Compact stable relation key.
    key: Vec<u8>,
    /// Owning project identity.
    project: Vec<u8>,
    /// Canonical collision witness.
    canonical: String,
    /// Stable source entity key.
    source: Vec<u8>,
    /// Legacy or extended family scope.
    relation_scope: String,
    /// Family spelling within the scope.
    relation_kind: String,
    /// Resolution lifecycle state.
    resolution_status: String,
    /// Optional resolved or external target key.
    target: Option<Vec<u8>>,
    /// Optional unresolved reference text.
    reference: Option<String>,
    /// Optional ambiguous candidate count.
    candidate_count: Option<i64>,
    /// Coarse trust class.
    confidence: String,
    /// Producer completeness.
    completeness: String,
}

/// One raw relation paired with its selecting frontier position.
struct AdjacencyRelationRow {
    /// Zero-based position inside the request frontier.
    frontier_index: u32,
    /// Raw normalized relation selected by the indexed adjacency branch.
    relation: RelationRow,
}

/// Raw normalized relation occurrence row.
struct OccurrenceRow {
    /// Stable logical relation key.
    relation: Vec<u8>,
    /// Exact repository-local source file.
    file_path: String,
    /// First one-based source line.
    start_line: i64,
    /// First zero-based source column.
    start_column: i64,
    /// Last one-based source line.
    end_line: i64,
    /// Exclusive zero-based end column.
    end_column: i64,
}

/// Raw normalized graph coverage row.
struct CoverageRow {
    /// Owning project identity.
    project: Vec<u8>,
    /// Project or path scope discriminator.
    scope_kind: String,
    /// Optional repository path scope.
    scope_path: Option<String>,
    /// Optional legacy or extended relation scope.
    relation_scope: Option<String>,
    /// Optional relation family spelling.
    relation_kind: Option<String>,
    /// Coverage lifecycle state.
    state: String,
    /// Persisted total items in scope.
    total: i64,
    /// Successfully covered items.
    covered: i64,
    /// Omitted or untrusted items.
    omitted: i64,
    /// Optional actionable explanation.
    reason: Option<String>,
    /// Optional reached product limit.
    reached_limit: Option<String>,
    /// Optional source parser pass joined from file metadata.
    parser: Option<String>,
    /// Optional derived-fact provider pass joined from file metadata.
    provider: Option<String>,
}

/// Mutable accounting retained only for one bounded hydration call.
pub(crate) struct RepositoryGraphReadMeter {
    /// Validated caller envelope.
    budget: RepositoryGraphReadBudget,
    /// Input selectors admitted before any query runs.
    requested_rows: u32,
    /// Raw payload bytes decoded so far.
    decoded_bytes: u64,
    /// Unique entities reconstructed so far.
    hydrated_entities: u32,
    /// Unique purpose-owning paths retained from hydrated entities.
    hydrated_paths: HashSet<String>,
}

impl RepositoryGraphReadMeter {
    /// Admit one request before any `SQLite` work begins.
    pub(crate) fn new(budget: RepositoryGraphReadBudget, requested_rows: usize) -> DbResult<Self> {
        let requested_rows =
            u32::try_from(requested_rows).map_err(|_source| GraphContractError::InvalidLimits {
                reason: "graph read requested-row count overflowed",
            })?;
        if requested_rows > budget.requested_rows() {
            return Err(GraphContractError::InvalidLimits {
                reason: "graph read requested rows exceed the batch budget",
            }
            .into());
        }
        Ok(Self {
            budget,
            requested_rows,
            decoded_bytes: 0,
            hydrated_entities: 0,
            hydrated_paths: HashSet::new(),
        })
    }

    /// Charge one decoded raw row before it leaves the row iterator.
    pub(crate) fn record_decoded_bytes(&mut self, bytes: u64) -> DbResult<()> {
        let decoded_bytes =
            self.decoded_bytes
                .checked_add(bytes)
                .ok_or(GraphContractError::InvalidLimits {
                    reason: "graph read decoded-byte accounting overflowed",
                })?;
        if decoded_bytes > self.budget.decoded_bytes() {
            return Err(GraphContractError::InvalidLimits {
                reason: "graph read decoded bytes exceed the batch budget",
            }
            .into());
        }
        self.decoded_bytes = decoded_bytes;
        Ok(())
    }

    /// Charge one unique reconstructed entity and its exact purpose owner path.
    fn record_entity(&mut self, entity: &GraphEntity) -> DbResult<()> {
        let hydrated_entities =
            self.hydrated_entities
                .checked_add(1)
                .ok_or(GraphContractError::InvalidLimits {
                    reason: "graph read entity accounting overflowed",
                })?;
        if hydrated_entities > self.budget.hydrated_entities() {
            return Err(GraphContractError::InvalidLimits {
                reason: "graph read hydrated entities exceed the batch budget",
            }
            .into());
        }
        if let Some(path) = graph_entity_purpose_owner(entity) {
            self.record_hydrated_path(path)?;
        }
        self.hydrated_entities = hydrated_entities;
        Ok(())
    }

    /// Charge one unique repository path hydrated from authoritative node state.
    pub(crate) fn record_hydrated_path(&mut self, path: &str) -> DbResult<()> {
        if self.hydrated_paths.contains(path) {
            return Ok(());
        }
        let hydrated_paths = u32::try_from(self.hydrated_paths.len()).map_err(|_source| {
            GraphContractError::InvalidLimits {
                reason: "graph read path accounting overflowed",
            }
        })?;
        if hydrated_paths >= self.budget.hydrated_paths() {
            return Err(GraphContractError::InvalidLimits {
                reason: "graph read hydrated paths exceed the batch budget",
            }
            .into());
        }
        self.hydrated_paths.insert(path.to_string());
        Ok(())
    }

    /// Finish exact work only after every requested row was reconstructed.
    pub(crate) fn finish(self, returned_rows: usize) -> DbResult<RepositoryGraphReadWork> {
        let returned_rows =
            u32::try_from(returned_rows).map_err(|_source| GraphContractError::InvalidLimits {
                reason: "graph read returned-row count overflowed",
            })?;
        if returned_rows > self.budget.returned_rows() {
            return Err(GraphContractError::InvalidLimits {
                reason: "graph read returned rows exceed the batch budget",
            }
            .into());
        }
        let hydrated_paths = u32::try_from(self.hydrated_paths.len()).map_err(|_source| {
            GraphContractError::InvalidLimits {
                reason: "graph read path accounting overflowed",
            }
        })?;
        Ok(RepositoryGraphReadWork {
            requested_rows: self.requested_rows,
            returned_rows,
            decoded_bytes: self.decoded_bytes,
            hydrated_entities: self.hydrated_entities,
            hydrated_paths,
        })
    }
}

/// One bounded flattened relation row used only for navigation enrichment.
struct NavigationConnectionRow {
    /// Zero-based owner position inside the current statement chunk.
    owner_index: usize,
    /// Closed connection family selected by the query branch.
    kind: RankedConnectionKind,
    /// Direction relative to the owner.
    direction: RankedConnectionDirection,
    /// Stable relation key used for deterministic ordering and deduplication.
    relation_key: Vec<u8>,
    /// Persisted resolution lifecycle state.
    resolution_status: String,
    /// Persisted unresolved or ambiguous identity.
    reference: Option<String>,
    /// Related normalized entity kind, absent only for unresolved output.
    entity_kind: Option<String>,
    /// Related repository path.
    repository_path: Option<String>,
    /// Related package ecosystem.
    package_manager: Option<String>,
    /// Related package name.
    package_name: Option<String>,
    /// Related package manifest.
    manifest_path: Option<String>,
    /// Related declaration name.
    symbol_name: Option<String>,
    /// Related external namespace.
    external_system: Option<String>,
    /// Related external identity.
    external_identity: Option<String>,
}

/// Return one empty navigation page while retaining the requested owner path.
fn empty_navigation_connections(path: &str) -> RepositoryNavigationConnections {
    RepositoryNavigationConnections {
        path: path.to_string(),
        counts: Vec::new(),
        connections: Vec::new(),
        truncated: false,
    }
}

/// Load one bounded chunk with a single compound set-oriented statement.
fn collect_navigation_connection_rows(
    connection: &Connection,
    owners: &[RepositoryNavigationNode],
    family_limit_plus_one: i64,
) -> DbResult<Vec<NavigationConnectionRow>> {
    let mut branches = Vec::with_capacity(owners.len() * NAVIGATION_CONNECTION_FAMILIES.len() * 2);
    let mut values = Vec::new();
    for (owner_index, owner) in owners.iter().enumerate() {
        for &(kind, scope, relation) in NAVIGATION_CONNECTION_FAMILIES {
            branches.push(navigation_connection_branch(
                owner_index,
                owner,
                kind,
                scope,
                relation,
                RankedConnectionDirection::Outbound,
                family_limit_plus_one,
                &mut values,
            ));
            if owner.kind != NodeKind::Folder || owner.path != "." {
                branches.push(navigation_connection_branch(
                    owner_index,
                    owner,
                    kind,
                    scope,
                    relation,
                    RankedConnectionDirection::Inbound,
                    family_limit_plus_one,
                    &mut values,
                ));
            }
        }
    }
    let sql = branches.join(" UNION ALL ");
    let mut statement = connection.prepare(&sql)?;
    let mut rows = statement.query(params_from_iter(values))?;
    let mut collected = Vec::new();
    while let Some(row) = rows.next()? {
        collected.push(navigation_connection_row(row)?);
    }
    Ok(collected)
}

/// Build one indexed outbound or inbound query branch for one owner and family.
fn navigation_connection_branch(
    owner_index: usize,
    owner: &RepositoryNavigationNode,
    kind: RankedConnectionKind,
    scope: &'static str,
    relation: &'static str,
    direction: RankedConnectionDirection,
    family_limit_plus_one: i64,
    values: &mut Vec<Value>,
) -> String {
    values.push(Value::Integer(owner_index as i64));
    values.push(Value::Text(
        navigation_connection_kind_name(kind).to_string(),
    ));
    if owner.kind == NodeKind::Folder
        && owner.path == "."
        && direction == RankedConnectionDirection::Outbound
    {
        values.push(Value::Text(scope.to_string()));
        values.push(Value::Text(relation.to_string()));
        values.push(Value::Integer(family_limit_plus_one));
        return "SELECT * FROM (
                    SELECT ? AS owner_index, ? AS expected_kind, 'outbound' AS direction,
                           r.relation_key, r.relation_scope, r.relation_kind,
                           r.resolution_status, r.reference_text,
                           related.entity_kind, related.repository_path,
                           related.package_manager, related.package_name, related.manifest_path,
                           related.symbol_name, related.external_system, related.external_identity
                      FROM graph_relations r INDEXED BY idx_graph_relations_kind_order
                      LEFT JOIN graph_entities related
                        ON related.entity_key = r.target_entity_key
                     WHERE r.relation_scope = ? AND r.relation_kind = ?
                     ORDER BY r.canonical_identity, r.relation_key
                     LIMIT ?
                )"
        .to_string();
    }
    let (relation_key, related_key, index, direction_name) = match direction {
        RankedConnectionDirection::Outbound => (
            "source_entity_key",
            "target_entity_key",
            "idx_graph_relations_source_kind",
            "outbound",
        ),
        RankedConnectionDirection::Inbound => (
            "target_entity_key",
            "source_entity_key",
            "idx_graph_relations_target_kind",
            "inbound",
        ),
    };
    let owned = navigation_owned_entity_sql(owner, values);
    let exclude_internal = if direction == RankedConnectionDirection::Inbound {
        let owned_sources = navigation_owned_entity_sql(owner, values);
        format!(" AND r.source_entity_key NOT IN ({owned_sources})")
    } else {
        String::new()
    };
    values.push(Value::Text(scope.to_string()));
    values.push(Value::Text(relation.to_string()));
    values.push(Value::Integer(family_limit_plus_one));
    format!(
        "SELECT * FROM (
             SELECT ? AS owner_index, ? AS expected_kind, '{direction_name}' AS direction,
                    r.relation_key, r.relation_scope, r.relation_kind,
                    r.resolution_status, r.reference_text,
                    related.entity_kind, related.repository_path,
                    related.package_manager, related.package_name, related.manifest_path,
                    related.symbol_name, related.external_system, related.external_identity
               FROM graph_relations r INDEXED BY {index}
               LEFT JOIN graph_entities related ON related.entity_key = r.{related_key}
              WHERE r.{relation_key} IN ({owned})
                {exclude_internal}
                AND r.relation_scope = ? AND r.relation_kind = ?
              ORDER BY r.canonical_identity, r.relation_key
              LIMIT ?
         )"
    )
}

/// Build the indexed entity-key ownership set for one navigation owner.
fn navigation_owned_entity_sql(
    owner: &RepositoryNavigationNode,
    values: &mut Vec<Value>,
) -> String {
    match owner.kind {
        NodeKind::File => {
            values.push(Value::Text(owner.path.clone()));
            values.push(Value::Text(owner.path.clone()));
            "SELECT entity_key
               FROM graph_entities INDEXED BY idx_graph_entities_path
              WHERE repository_path = ?
             UNION
             SELECT entity_key
               FROM graph_entities INDEXED BY idx_graph_entities_manifest_path
              WHERE manifest_path = ?"
                .to_string()
        }
        NodeKind::Folder if owner.path == "." => "SELECT entity_key
               FROM graph_entities INDEXED BY idx_graph_entities_path
              WHERE repository_path IS NOT NULL
             UNION
             SELECT entity_key
               FROM graph_entities INDEXED BY idx_graph_entities_manifest_path
              WHERE manifest_path IS NOT NULL"
            .to_string(),
        NodeKind::Folder => {
            let lower = format!("{}/", owner.path);
            let upper = format!("{}0", owner.path);
            values.push(Value::Text(owner.path.clone()));
            values.push(Value::Text(lower.clone()));
            values.push(Value::Text(upper.clone()));
            values.push(Value::Text(owner.path.clone()));
            values.push(Value::Text(lower));
            values.push(Value::Text(upper));
            "SELECT entity_key
               FROM graph_entities INDEXED BY idx_graph_entities_path
              WHERE repository_path = ?
             UNION
             SELECT entity_key
               FROM graph_entities INDEXED BY idx_graph_entities_path
              WHERE repository_path >= ? AND repository_path < ?
             UNION
             SELECT entity_key
               FROM graph_entities INDEXED BY idx_graph_entities_manifest_path
              WHERE manifest_path = ?
             UNION
             SELECT entity_key
               FROM graph_entities INDEXED BY idx_graph_entities_manifest_path
              WHERE manifest_path >= ? AND manifest_path < ?"
                .to_string()
        }
    }
}

/// Decode one flattened navigation row without accepting partial corruption.
fn navigation_connection_row(row: &Row<'_>) -> DbResult<NavigationConnectionRow> {
    let owner_index = count_to_usize("navigation_connection.owner_index", row.get(0)?)?;
    let expected_kind: String = row.get(1)?;
    let direction = match row.get::<_, String>(2)?.as_str() {
        "outbound" => RankedConnectionDirection::Outbound,
        "inbound" => RankedConnectionDirection::Inbound,
        _ => {
            return Err(DbError::GraphRowShape {
                table: "graph_relations",
                reason: "navigation relation direction is invalid",
            });
        }
    };
    let relation_key: Vec<u8> = row.get(3)?;
    if relation_key.len() != 32 {
        return Err(DbError::InvalidBlobLength {
            field: "graph_relations.relation_key",
            expected: 32,
            found: relation_key.len(),
        });
    }
    let relation_scope: String = row.get(4)?;
    let relation_kind: String = row.get(5)?;
    let kind = navigation_connection_kind(&relation_scope, &relation_kind)?;
    if navigation_connection_kind_name(kind) != expected_kind {
        return Err(DbError::GraphRowShape {
            table: "graph_relations",
            reason: "navigation relation family does not match its query branch",
        });
    }
    let resolution_status: String = row.get(6)?;
    if !matches!(
        resolution_status.as_str(),
        "resolved" | "ambiguous" | "unresolved" | "external"
    ) {
        return Err(DbError::InvalidEnum {
            field: "graph_relations.resolution_status",
            value: resolution_status,
        });
    }
    Ok(NavigationConnectionRow {
        owner_index,
        kind,
        direction,
        relation_key,
        resolution_status,
        reference: row.get(7)?,
        entity_kind: row.get(8)?,
        repository_path: row.get(9)?,
        package_manager: row.get(10)?,
        package_name: row.get(11)?,
        manifest_path: row.get(12)?,
        symbol_name: row.get(13)?,
        external_system: row.get(14)?,
        external_identity: row.get(15)?,
    })
}

/// Compose deterministic per-owner pages from fully decoded rows.
fn navigation_connection_pages(
    owners: &[RepositoryNavigationNode],
    mut rows: Vec<NavigationConnectionRow>,
    family_limit: usize,
    sample_limit: usize,
) -> DbResult<Vec<RepositoryNavigationConnections>> {
    rows.sort_by(|left, right| {
        left.owner_index
            .cmp(&right.owner_index)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| {
                navigation_direction_order(left.direction)
                    .cmp(&navigation_direction_order(right.direction))
            })
            .then_with(|| left.relation_key.cmp(&right.relation_key))
    });
    let mut grouped =
        BTreeMap::<(usize, RankedConnectionKind), Vec<NavigationConnectionRow>>::new();
    for row in rows {
        if row.owner_index >= owners.len() {
            return Err(DbError::GraphRowShape {
                table: "graph_relations",
                reason: "navigation relation owner index is outside its request chunk",
            });
        }
        grouped
            .entry((row.owner_index, row.kind))
            .or_default()
            .push(row);
    }

    owners
        .iter()
        .enumerate()
        .map(|(owner_index, owner)| {
            let mut page = empty_navigation_connections(&owner.path);
            for &(kind, _, _) in NAVIGATION_CONNECTION_FAMILIES {
                let Some(rows) = grouped.remove(&(owner_index, kind)) else {
                    continue;
                };
                let mut seen = HashSet::new();
                let unique = rows
                    .into_iter()
                    .filter(|row| seen.insert(row.relation_key.clone()))
                    .collect::<Vec<_>>();
                let truncated = unique.len() > family_limit;
                let count = unique.len().min(family_limit);
                page.counts.push(RankedConnectionCount {
                    kind,
                    count,
                    truncated,
                });
                for (family_index, row) in unique.into_iter().enumerate() {
                    let target = navigation_connection_target(&row)?;
                    if family_index < family_limit && page.connections.len() < sample_limit {
                        page.connections.push(RankedConnection {
                            kind,
                            direction: row.direction,
                            target,
                        });
                    } else if family_index < family_limit {
                        page.truncated = true;
                    }
                }
                page.truncated |= truncated;
            }
            Ok(page)
        })
        .collect()
}

/// Convert a persisted endpoint or unresolved reference into its compact target.
fn navigation_connection_target(row: &NavigationConnectionRow) -> DbResult<RankedConnectionTarget> {
    if row.direction == RankedConnectionDirection::Outbound
        && matches!(row.resolution_status.as_str(), "ambiguous" | "unresolved")
    {
        let reference = row.reference.clone().ok_or(DbError::GraphRowShape {
            table: "graph_relations",
            reason: "unresolved navigation relation is missing its reference",
        })?;
        if row.entity_kind.is_some() {
            return Err(DbError::GraphRowShape {
                table: "graph_relations",
                reason: "unresolved navigation relation retained a target entity",
            });
        }
        return Ok(RankedConnectionTarget::Unresolved { reference });
    }
    if !matches!(row.resolution_status.as_str(), "resolved" | "external") {
        return Err(DbError::GraphRowShape {
            table: "graph_relations",
            reason: "inbound navigation relation has no resolved target",
        });
    }
    match row.entity_kind.as_deref() {
        Some("project") => Ok(RankedConnectionTarget::Local {
            path: ".".to_string(),
            symbol: None,
        }),
        Some("folder" | "file") => Ok(RankedConnectionTarget::Local {
            path: required_navigation_target(
                row.repository_path.as_deref(),
                "local navigation target is missing its repository path",
            )?,
            symbol: None,
        }),
        Some("symbol") => Ok(RankedConnectionTarget::Local {
            path: required_navigation_target(
                row.repository_path.as_deref(),
                "symbol navigation target is missing its repository path",
            )?,
            symbol: Some(required_navigation_target(
                row.symbol_name.as_deref(),
                "symbol navigation target is missing its declaration name",
            )?),
        }),
        Some("package") => Ok(RankedConnectionTarget::Package {
            manager: required_navigation_target(
                row.package_manager.as_deref(),
                "package navigation target is missing its manager",
            )?,
            name: required_navigation_target(
                row.package_name.as_deref(),
                "package navigation target is missing its name",
            )?,
            manifest: required_navigation_target(
                row.manifest_path.as_deref(),
                "package navigation target is missing its manifest",
            )?,
        }),
        Some("external") => Ok(RankedConnectionTarget::External {
            system: required_navigation_target(
                row.external_system.as_deref(),
                "external navigation target is missing its system",
            )?,
            identity: required_navigation_target(
                row.external_identity.as_deref(),
                "external navigation target is missing its identity",
            )?,
        }),
        Some(_) => Err(DbError::GraphRowShape {
            table: "graph_entities",
            reason: "navigation endpoint has an unsupported entity kind",
        }),
        None => Err(DbError::GraphRowShape {
            table: "graph_relations",
            reason: "resolved navigation relation target is missing",
        }),
    }
}

/// Clone one required nonempty target field.
fn required_navigation_target(value: Option<&str>, reason: &'static str) -> DbResult<String> {
    value
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or(DbError::GraphRowShape {
            table: "graph_entities",
            reason,
        })
}

/// Map persisted relation family fields to the navigation family inventory.
fn navigation_connection_kind(scope: &str, relation: &str) -> DbResult<RankedConnectionKind> {
    NAVIGATION_CONNECTION_FAMILIES
        .iter()
        .find_map(|&(kind, expected_scope, expected_relation)| {
            (scope == expected_scope && relation == expected_relation).then_some(kind)
        })
        .ok_or(DbError::GraphRowShape {
            table: "graph_relations",
            reason: "relation family is not available to navigation enrichment",
        })
}

/// Return the stable compact name for one navigation family.
const fn navigation_connection_kind_name(kind: RankedConnectionKind) -> &'static str {
    match kind {
        RankedConnectionKind::Package => "package",
        RankedConnectionKind::Import => "import",
        RankedConnectionKind::Call => "call",
        RankedConnectionKind::Reference => "reference",
        RankedConnectionKind::Test => "test",
        RankedConnectionKind::Route => "route",
        RankedConnectionKind::Config => "config",
    }
}

/// Return stable outbound-before-inbound sample order.
const fn navigation_direction_order(direction: RankedConnectionDirection) -> u8 {
    match direction {
        RankedConnectionDirection::Outbound => 0,
        RankedConnectionDirection::Inbound => 1,
    }
}

impl AtlasStore {
    /// Load bounded current graph context for folder and file navigation rows.
    ///
    /// Owners are processed through a fixed-size set-oriented statement per
    /// chunk. Each family uses separate indexed outbound and inbound branches,
    /// exact file plus manifest ownership, or bounded folder-prefix ownership.
    /// No partial result is returned if any statement or row fails.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid paths or limits, unavailable publication
    /// state, `SQLite` failures, or any corrupt relation or endpoint row.
    pub fn repository_navigation_connections(
        &self,
        owners: &[RepositoryNavigationNode],
        family_limit: u32,
        sample_limit: usize,
    ) -> DbResult<Vec<RepositoryNavigationConnections>> {
        let family_limit_plus_one = validated_limit_plus_one(
            family_limit,
            GraphLimits::MAX_ROWS,
            "navigation connection rows must be nonzero and within the product ceiling",
        )?;
        if sample_limit == 0 || sample_limit > GraphLimits::MAX_ROWS as usize {
            return Err(GraphContractError::InvalidLimits {
                reason: "navigation connection sample must be nonzero and within the product ceiling",
            }
            .into());
        }
        for owner in owners {
            match owner.kind {
                NodeKind::Folder => {
                    RepositoryNodePath::new(Path::new(&owner.path))?;
                }
                NodeKind::File => {
                    RepositoryFilePath::new(Path::new(&owner.path))?;
                }
            }
        }
        if owners.is_empty() {
            return Ok(Vec::new());
        }
        if self.repository_graph_generation()?.is_none() {
            return Ok(owners
                .iter()
                .map(|owner| empty_navigation_connections(&owner.path))
                .collect());
        }
        let project = load_project_identity(&self.connection)?
            .ok_or(DbError::ProjectInstanceIdentityMissing)?;
        require_bound_project_identity(&self.connection, project)?;

        let mut result = Vec::with_capacity(owners.len());
        for chunk in owners.chunks(NAVIGATION_CONNECTION_OWNER_CHUNK) {
            let rows =
                collect_navigation_connection_rows(&self.connection, chunk, family_limit_plus_one)?;
            result.extend(navigation_connection_pages(
                chunk,
                rows,
                family_limit as usize,
                sample_limit,
            )?);
        }
        Ok(result)
    }

    /// Load one typed graph entity by its compact stable key.
    ///
    /// # Errors
    ///
    /// Returns an error when publication state, project identity, row shape,
    /// canonical identity, or persisted key material is invalid.
    pub fn repository_graph_entity(&self, key: &GraphEntityKey) -> DbResult<Option<GraphEntity>> {
        let Some(generation) = self.repository_graph_generation()? else {
            return Ok(None);
        };
        if !verify_project_identity(&self.connection, key.project())? {
            return Ok(None);
        }
        Ok(self
            .repository_graph_entity_bounded(
                key,
                generation,
                maximum_repository_graph_read_budget()?,
                None,
            )?
            .rows
            .into_iter()
            .next())
    }

    /// Load one optional exact entity under a stable graph read envelope.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale project or generation, cancellation,
    /// `SQLite` failure, corrupt entity state, or any decoded-byte, entity, or
    /// path hydration overrun. A missing exact key returns an empty successful
    /// batch with exact zero returned work.
    pub fn repository_graph_entity_bounded(
        &self,
        key: &GraphEntityKey,
        generation: IndexGeneration,
        budget: RepositoryGraphReadBudget,
        control: Option<&IndexWorkControl>,
    ) -> DbResult<RepositoryGraphReadBatch<GraphEntity>> {
        self.require_repository_graph_snapshot(key.project(), generation)?;
        let digest = key.digest_bytes()?;
        let mut meter = RepositoryGraphReadMeter::new(budget, 1)?;
        let mut entities = load_graph_entities_by_digest_metered(
            self,
            &[digest],
            key.project(),
            generation,
            control,
            Some(&mut meter),
        )?;
        let rows = entities.remove(&digest).into_iter().collect::<Vec<_>>();
        let work = meter.finish(rows.len())?;
        Ok(RepositoryGraphReadBatch { rows, work })
    }

    /// Hydrate an ordered unique set of graph entities from compact stable keys.
    ///
    /// # Errors
    ///
    /// Returns an error for duplicate or oversized input, a stale project or
    /// generation, cancellation, a missing entity, `SQLite` failure, or any
    /// invalid persisted key or canonical identity. No partial set is returned.
    pub fn repository_graph_entities_by_digest(
        &self,
        project: ProjectInstanceId,
        generation: IndexGeneration,
        digests: &[[u8; 32]],
        budget: RepositoryGraphReadBudget,
        control: Option<&IndexWorkControl>,
    ) -> DbResult<RepositoryGraphReadBatch<GraphEntity>> {
        validate_graph_hydration_request(digests)?;
        self.require_repository_graph_snapshot(project, generation)?;
        let mut meter = RepositoryGraphReadMeter::new(budget, digests.len())?;
        let mut entities = load_graph_entities_by_digest_metered(
            self,
            digests,
            project,
            generation,
            control,
            Some(&mut meter),
        )?;
        let mut ordered = Vec::with_capacity(digests.len());
        for digest in digests {
            ordered.push(entities.remove(digest).ok_or(DbError::GraphRowShape {
                table: "graph_entities",
                reason: "requested graph entity is missing",
            })?);
        }
        let work = meter.finish(ordered.len())?;
        Ok(RepositoryGraphReadBatch {
            rows: ordered,
            work,
        })
    }

    /// Load a bounded page of entities that use one exact repository path.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid limits, unavailable publication state,
    /// project mismatch, `SQLite` failure, or any corrupt row in the page.
    pub fn repository_graph_entities_by_path(
        &self,
        project: ProjectInstanceId,
        path: &RepositoryNodePath,
        limit: u32,
    ) -> DbResult<RepositoryGraphPage<GraphEntity>> {
        validated_limit_plus_one(
            limit,
            GraphLimits::MAX_ROWS,
            "graph rows must be nonzero and within the product ceiling",
        )?;
        let Some(generation) = self.repository_graph_generation()? else {
            return Ok(empty_page());
        };
        if !verify_project_identity(&self.connection, project)? {
            return Ok(empty_page());
        }
        Ok(self
            .repository_graph_entities_by_path_bounded(
                project,
                generation,
                path,
                limit,
                maximum_repository_graph_read_budget()?,
                None,
            )?
            .page)
    }

    /// Load one exact-path entity page under a stable graph read envelope.
    ///
    /// # Errors
    ///
    /// Returns the same fail-closed errors as
    /// [`Self::repository_graph_entities_by_path`], rejects a stale project or
    /// generation and any returned-row, decoded-byte, entity, or path hydration
    /// overrun, and meters the raw truncation sentinel.
    pub fn repository_graph_entities_by_path_bounded(
        &self,
        project: ProjectInstanceId,
        generation: IndexGeneration,
        path: &RepositoryNodePath,
        limit: u32,
        budget: RepositoryGraphReadBudget,
        control: Option<&IndexWorkControl>,
    ) -> DbResult<RepositoryGraphReadPage<GraphEntity>> {
        let limit_plus_one = validated_limit_plus_one(
            limit,
            GraphLimits::MAX_ROWS,
            "graph rows must be nonzero and within the product ceiling",
        )?;
        self.require_repository_graph_snapshot(project, generation)?;
        let mut meter = RepositoryGraphReadMeter::new(budget, 1)?;
        let raw = with_sqlite_read_progress(
            &self.connection,
            control,
            IndexWorkStage::RepositoryTraversal,
            || {
                let mut statement = self.connection.prepare_cached(
                    "SELECT entity_key, project_instance_id, canonical_identity, entity_kind,
                            repository_path, package_manager, package_name, manifest_path,
                            symbol_name, symbol_kind, symbol_parent, symbol_signature,
                            external_system, external_identity
                       FROM graph_entities
                      WHERE project_instance_id = ?1 AND repository_path = ?2
                      ORDER BY entity_kind, canonical_identity, entity_key
                      LIMIT ?3",
                )?;
                collect_entity_rows_metered(
                    statement.query(params![
                        &project.as_bytes()[..],
                        path.as_str(),
                        limit_plus_one
                    ])?,
                    &mut meter,
                )
            },
        )?;
        let page = page_from_raw(raw, limit, |row| {
            let entity = entity_from_row(row, project, generation)?;
            meter.record_entity(&entity)?;
            Ok(entity)
        })?;
        let work = meter.finish(page.rows.len())?;
        Ok(RepositoryGraphReadPage { page, work })
    }

    /// Load bounded graph entities that export one exact canonical resolution key.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid limits, a project mismatch, a conflicting
    /// canonical witness, corrupt graph rows, or `SQLite` failure.
    pub fn repository_resolution_candidates(
        &self,
        key: &CanonicalResolutionKey,
        limit: u32,
    ) -> DbResult<RepositoryGraphPage<GraphEntity>> {
        let limit_plus_one = validated_limit_plus_one(
            limit,
            GraphLimits::MAX_ROWS,
            "resolution candidates must be nonzero and within the product ceiling",
        )?;
        let Some(generation) = self.repository_graph_generation()? else {
            return Ok(empty_page());
        };
        if !verify_project_identity(&self.connection, key.project())? {
            return Ok(empty_page());
        }
        if !validate_persisted_resolution_key(&self.connection, key)? {
            return Ok(empty_page());
        }
        let raw = {
            let mut statement = self.connection.prepare_cached(
                "SELECT entity.entity_key, entity.project_instance_id,
                        entity.canonical_identity, entity.entity_kind,
                        entity.repository_path, entity.package_manager,
                        entity.package_name, entity.manifest_path,
                        entity.symbol_name, entity.symbol_kind,
                        entity.symbol_parent, entity.symbol_signature,
                        entity.external_system, entity.external_identity
                   FROM graph_entity_exports AS export
                        INDEXED BY idx_graph_entity_exports_key
                   JOIN graph_entities AS entity
                     ON entity.project_instance_id = export.project_instance_id
                    AND entity.entity_key = export.entity_key
                  WHERE export.project_instance_id = ?1
                    AND export.resolution_domain = ?2
                    AND export.key_digest = ?3
                  ORDER BY export.entity_key
                  LIMIT ?4",
            )?;
            collect_entity_rows(statement.query(params![
                &key.project().as_bytes()[..],
                key.domain().as_str(),
                &key.digest_bytes()[..],
                limit_plus_one,
            ])?)?
        };
        page_from_raw(raw, limit, |row| {
            entity_from_row(row, key.project(), generation)
        })
    }

    /// Load bounded export candidates for a canonical-key batch in one set-oriented pass.
    ///
    /// Results retain the selecting key so callers can resolve several relation
    /// occurrences without issuing one query per dependency key. Duplicate input
    /// keys and duplicate candidate bindings are returned once in stable order.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid limits, mixed projects, a conflicting
    /// canonical witness, corrupt graph rows, or `SQLite` failure. A terminal row
    /// conversion failure rejects the complete operation rather than returning a
    /// partial candidate set.
    pub fn repository_resolution_candidates_for_keys(
        &self,
        project: ProjectInstanceId,
        keys: &[CanonicalResolutionKey],
        limit: u32,
    ) -> DbResult<RepositoryGraphPage<RepositoryResolutionCandidate>> {
        validated_limit_plus_one(
            limit,
            GraphLimits::MAX_ROWS,
            "resolution candidates must be nonzero and within the product ceiling",
        )?;
        let Some(generation) = self.repository_graph_generation()? else {
            return Ok(empty_page());
        };
        if !verify_project_identity(&self.connection, project)? {
            return Ok(empty_page());
        }
        let keys = normalized_resolution_keys(project, keys)?;
        validate_persisted_resolution_keys(&self.connection, &keys)?;
        let mut candidates = Vec::new();
        for chunk in keys.chunks(RESOLUTION_KEYS_PER_QUERY) {
            if chunk.is_empty() {
                continue;
            }
            let values_clause = resolution_values_clause(chunk.len(), 4);
            let sql = format!(
                "WITH requested(project_instance_id, resolution_domain, key_digest, canonical_identity)
                      AS (VALUES {values_clause})
                 SELECT entity.entity_key, entity.project_instance_id,
                        entity.canonical_identity, entity.entity_kind,
                        entity.repository_path, entity.package_manager,
                        entity.package_name, entity.manifest_path,
                        entity.symbol_name, entity.symbol_kind,
                        entity.symbol_parent, entity.symbol_signature,
                        entity.external_system, entity.external_identity,
                        stored.project_instance_id, stored.resolution_domain,
                        stored.key_digest, stored.canonical_identity
                   FROM requested
                   JOIN graph_resolution_keys AS stored
                     ON stored.project_instance_id = requested.project_instance_id
                    AND stored.resolution_domain = requested.resolution_domain
                    AND stored.key_digest = requested.key_digest
                    AND stored.canonical_identity = requested.canonical_identity
                   JOIN graph_entity_exports AS export
                        INDEXED BY idx_graph_entity_exports_key
                     ON export.project_instance_id = stored.project_instance_id
                    AND export.resolution_domain = stored.resolution_domain
                    AND export.key_digest = stored.key_digest
                   JOIN graph_entities AS entity
                     ON entity.project_instance_id = export.project_instance_id
                    AND entity.entity_key = export.entity_key
                  ORDER BY stored.resolution_domain, stored.key_digest, export.entity_key
                  LIMIT ?"
            );
            let mut values = resolution_key_values(chunk, true);
            values.push(Value::Integer(i64::from(limit) + 1));
            let mut statement = self.connection.prepare(&sql)?;
            let mut rows = statement.query(params_from_iter(values.iter()))?;
            while let Some(row) = rows.next()? {
                let entity = entity_from_row(entity_row(row)?, project, generation)?;
                let key_project = project_from_blob(
                    "graph_resolution_keys.project_instance_id",
                    row.get::<_, Vec<u8>>(14)?,
                )?;
                require_project(project, key_project)?;
                let domain_text = row.get::<_, String>(15)?;
                let domain = ResolutionKeyDomain::try_from(domain_text.as_str())?;
                let digest = fixed_bytes::<32>(
                    "graph_resolution_keys.key_digest",
                    row.get::<_, Vec<u8>>(16)?,
                )?;
                let key = CanonicalResolutionKey::from_persisted(
                    key_project,
                    domain,
                    digest,
                    row.get(17)?,
                )?;
                candidates.push(RepositoryResolutionCandidate { key, entity });
            }
            if candidates.len() > limit as usize {
                break;
            }
        }
        candidates.sort_by(|left, right| {
            left.key
                .cmp(&right.key)
                .then_with(|| left.entity.key().digest().cmp(right.entity.key().digest()))
                .then_with(|| {
                    left.entity
                        .key()
                        .canonical_identity()
                        .cmp(right.entity.key().canonical_identity())
                })
        });
        candidates.dedup_by(|left, right| {
            left.key == right.key && left.entity.key() == right.entity.key()
        });
        let truncated = candidates.len() > limit as usize;
        candidates.truncate(limit as usize);
        Ok(RepositoryGraphPage {
            rows: candidates,
            truncated,
        })
    }

    /// Load bounded canonical export keys previously owned by exact source paths.
    ///
    /// The result is deduplicated and sorted by canonical key identity so callers
    /// can union it deterministically with newly staged exports.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid paths or limits, project mismatch, corrupt
    /// persisted keys, canonical witness collisions, or `SQLite` failure.
    pub fn repository_export_keys_for_paths(
        &self,
        project: ProjectInstanceId,
        paths: &[String],
        limit: u32,
    ) -> DbResult<RepositoryGraphPage<CanonicalResolutionKey>> {
        validated_limit_plus_one(
            limit,
            GraphLimits::MAX_ROWS,
            "resolution export keys must be nonzero and within the product ceiling",
        )?;
        if self.repository_graph_generation()?.is_none()
            || !verify_project_identity(&self.connection, project)?
        {
            return Ok(empty_page());
        }
        let paths = normalized_file_paths(paths)?;
        let keys = resolution_keys_for_owner_paths(
            &self.connection,
            project,
            &paths,
            ResolutionOwner::EntityExports,
            Some(limit),
        )?;
        Ok(page_from_ordered_set(keys, limit))
    }

    /// Find the bounded distinct source paths that depend on canonical keys.
    ///
    /// `truncated` is set from aggregate `LIMIT + 1` handling. Callers must
    /// escalate to a full refresh before opening a publication transaction when
    /// it is true.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid limits, mixed projects, canonical witness
    /// collisions, invalid persisted paths, or `SQLite` failure. No mutation is
    /// performed.
    pub fn repository_affected_source_paths(
        &self,
        project: ProjectInstanceId,
        keys: &[CanonicalResolutionKey],
        limit: u32,
    ) -> DbResult<RepositoryGraphPage<RepositoryFilePath>> {
        validated_limit_plus_one(
            limit,
            GraphLimits::MAX_ROWS,
            "affected source paths must be nonzero and within the product ceiling",
        )?;
        if self.repository_graph_generation()?.is_none()
            || !verify_project_identity(&self.connection, project)?
        {
            return Ok(empty_page());
        }
        let keys = normalized_resolution_keys(project, keys)?;
        validate_persisted_resolution_keys(&self.connection, &keys)?;
        let paths = affected_source_paths(&self.connection, &keys, limit)?;
        Ok(page_from_ordered_set(paths, limit))
    }

    /// Account the persisted closure owned by exact affected source paths.
    ///
    /// Resolution witnesses are conservatively counted once per retained export
    /// or dependency binding. Callers must require a full refresh when
    /// `truncated` is true; in that case `rows` is the `limit + 1` lower bound
    /// and `retained_bytes` covers only that bounded prefix.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid paths or limits, corrupt persisted rows,
    /// unavailable owned indexes, or any `SQLite` preparation, iteration, or
    /// conversion failure. An unavailable graph returns an empty footprint;
    /// a differently bound graph returns the typed project-mismatch error.
    pub fn repository_affected_source_footprint(
        &self,
        project: ProjectInstanceId,
        paths: &[String],
        limit: u32,
    ) -> DbResult<RepositoryAffectedSourceFootprint> {
        let limit_plus_one = validated_limit_plus_one(
            limit,
            GraphLimits::MAX_ROWS,
            "affected source footprint rows must be nonzero and within the product ceiling",
        )?;
        if self.repository_graph_generation()?.is_none()
            || !verify_project_identity(&self.connection, project)?
        {
            return Ok(empty_affected_source_footprint());
        }
        let paths = normalized_file_paths(paths)?;
        affected_source_footprint(&self.connection, project, &paths, limit_plus_one)
    }

    /// Load a bounded page of logical relations through one indexed query shape.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid limits, mismatched project identities,
    /// `SQLite` failure, or any corrupt entity/relation row in the complete page.
    pub fn repository_graph_relations(
        &self,
        query: RepositoryGraphRelationQuery,
        limit: u32,
    ) -> DbResult<RepositoryGraphPage<LogicalRelation>> {
        let page = self.repository_graph_relation_rows(query, limit, None)?;
        Ok(RepositoryGraphPage {
            rows: page.rows.into_iter().map(|row| row.relation).collect(),
            truncated: page.truncated,
        })
    }

    /// Load a bounded relation page with unique endpoint hydration.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid limits, mismatched project identities,
    /// cancellation, `SQLite` failure, or any corrupt entity/relation row in
    /// the complete page.
    pub fn repository_graph_relation_rows(
        &self,
        query: RepositoryGraphRelationQuery,
        limit: u32,
        control: Option<&IndexWorkControl>,
    ) -> DbResult<RepositoryGraphPage<RepositoryGraphRelationRow>> {
        let limit_plus_one = validated_limit_plus_one(
            limit,
            GraphLimits::MAX_ROWS,
            "graph rows must be nonzero and within the product ceiling",
        )?;
        let Some(generation) = self.repository_graph_generation()? else {
            return Ok(empty_page());
        };
        let (project, raw) = match query {
            RepositoryGraphRelationQuery::Outbound { source } => {
                let project = source.project();
                if !verify_project_identity(&self.connection, project)? {
                    return Ok(empty_page());
                }
                let raw = with_sqlite_read_progress(
                    &self.connection,
                    control,
                    IndexWorkStage::RepositoryTraversal,
                    || {
                        self.collect_relation_rows_by_key(
                            "source_entity_key",
                            &source.digest_bytes()?,
                            limit_plus_one,
                        )
                    },
                )?;
                (project, raw)
            }
            RepositoryGraphRelationQuery::Inbound { target } => {
                let project = target.project();
                if !verify_project_identity(&self.connection, project)? {
                    return Ok(empty_page());
                }
                let raw = with_sqlite_read_progress(
                    &self.connection,
                    control,
                    IndexWorkStage::RepositoryTraversal,
                    || {
                        self.collect_relation_rows_by_key(
                            "target_entity_key",
                            &target.digest_bytes()?,
                            limit_plus_one,
                        )
                    },
                )?;
                (project, raw)
            }
            RepositoryGraphRelationQuery::Family { relation } => {
                let project = load_project_identity(&self.connection)?
                    .ok_or(DbError::GraphPublicationUnavailable)?;
                let (scope, kind) = relation_parts(relation);
                let raw = with_sqlite_read_progress(
                    &self.connection,
                    control,
                    IndexWorkStage::RepositoryTraversal,
                    || {
                        let mut statement = self.connection.prepare_cached(
                            "SELECT relation_key, project_instance_id, canonical_identity,
                                source_entity_key, relation_scope, relation_kind,
                                resolution_status, target_entity_key, reference_text,
                                candidate_count, confidence, completeness
                           FROM graph_relations
                          WHERE project_instance_id = ?1
                            AND relation_scope = ?2 AND relation_kind = ?3
                          ORDER BY canonical_identity, relation_key
                          LIMIT ?4",
                        )?;
                        collect_relation_rows(statement.query(params![
                            &project.as_bytes()[..],
                            scope,
                            kind,
                            limit_plus_one
                        ])?)
                    },
                )?;
                (project, raw)
            }
        };
        let entities = load_relation_entities(self, &raw, project, generation, control)?;
        page_from_raw(raw, limit, |row| {
            relation_detail_from_row(&entities, row, project, generation)
        })
    }

    /// Hydrate an ordered unique set of normalized relations from compact stable keys.
    ///
    /// # Errors
    ///
    /// Returns an error for duplicate or oversized input, a stale project or
    /// generation, cancellation, a missing relation or endpoint, `SQLite`
    /// failure, or any invalid persisted key or canonical identity. No partial
    /// set is returned.
    pub fn repository_graph_relation_rows_by_digest(
        &self,
        project: ProjectInstanceId,
        generation: IndexGeneration,
        digests: &[[u8; 32]],
        budget: RepositoryGraphReadBudget,
        control: Option<&IndexWorkControl>,
    ) -> DbResult<RepositoryGraphReadBatch<RepositoryGraphRelationRow>> {
        validate_graph_hydration_request(digests)?;
        self.require_repository_graph_snapshot(project, generation)?;
        let mut meter = RepositoryGraphReadMeter::new(budget, digests.len())?;
        if digests.is_empty() {
            return Ok(RepositoryGraphReadBatch {
                rows: Vec::new(),
                work: meter.finish(0)?,
            });
        }
        let sql = graph_relation_hydration_sql(digests.len());
        let mut bindings = digests
            .iter()
            .map(|digest| Value::Blob(digest.to_vec()))
            .collect::<Vec<_>>();
        bindings.push(Value::Blob(project.as_bytes().to_vec()));
        let raw = with_sqlite_read_progress(
            &self.connection,
            control,
            IndexWorkStage::RepositoryTraversal,
            || {
                let mut statement = self.connection.prepare(&sql)?;
                collect_relation_rows_metered(
                    statement.query(params_from_iter(bindings.iter()))?,
                    &mut meter,
                )
            },
        )?;
        let mut relations = HashMap::with_capacity(raw.len());
        for row in raw {
            let digest = fixed_bytes::<32>("graph_relations.relation_key", row.key.clone())?;
            if relations.insert(digest, row).is_some() {
                return Err(DbError::GraphRowShape {
                    table: "graph_relations",
                    reason: "batched relation hydration returned a duplicate key",
                });
            }
        }
        let mut ordered = Vec::with_capacity(digests.len());
        for digest in digests {
            ordered.push(relations.remove(digest).ok_or(DbError::GraphRowShape {
                table: "graph_relations",
                reason: "requested graph relation is missing",
            })?);
        }
        let references = ordered.iter().collect::<Vec<_>>();
        let entities = load_relation_entity_references_metered(
            self,
            &references,
            project,
            generation,
            control,
            Some(&mut meter),
        )?;
        let rows = ordered
            .into_iter()
            .map(|row| relation_detail_from_row(&entities, row, project, generation))
            .collect::<DbResult<Vec<_>>>()?;
        let work = meter.finish(rows.len())?;
        Ok(RepositoryGraphReadBatch { rows, work })
    }

    /// Load one bounded direction-specific adjacency page for a unique frontier.
    ///
    /// The complete frontier is bound through one statement whose indexed
    /// per-frontier branches each cap their candidate rows before the stable
    /// compound order. Endpoint entities are hydrated in bounded set-oriented
    /// batches. The opaque continuation is valid only with the same project,
    /// generation, frontier, and direction inside the same request snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid or mixed-project frontiers, invalid limits
    /// or continuation state, cancellation, `SQLite` failures, or any corrupt
    /// relation or endpoint row. No partial page is returned.
    pub fn repository_graph_adjacency_page(
        &self,
        frontier: &[GraphEntityKey],
        direction: RepositoryGraphDirection,
        continuation: Option<&RepositoryGraphAdjacencyContinuation>,
        limit: u32,
        control: Option<&IndexWorkControl>,
    ) -> DbResult<RepositoryGraphAdjacencyPage> {
        Ok(self
            .repository_graph_adjacency_page_bounded(
                frontier,
                direction,
                continuation,
                limit,
                maximum_repository_graph_read_budget()?,
                control,
            )?
            .page)
    }

    /// Load one bounded adjacency page and report exact database work.
    ///
    /// # Errors
    ///
    /// Returns the same fail-closed errors as
    /// [`Self::repository_graph_adjacency_page`] and rejects any page whose
    /// returned, decoded, endpoint, or path hydration crosses `budget`.
    pub fn repository_graph_adjacency_page_bounded(
        &self,
        frontier: &[GraphEntityKey],
        direction: RepositoryGraphDirection,
        continuation: Option<&RepositoryGraphAdjacencyContinuation>,
        limit: u32,
        budget: RepositoryGraphReadBudget,
        control: Option<&IndexWorkControl>,
    ) -> DbResult<RepositoryGraphAdjacencyReadPage> {
        self.repository_graph_adjacency_page_filtered_bounded(
            frontier,
            direction,
            None,
            continuation,
            limit,
            budget,
            control,
        )
    }

    /// Load one bounded direction-specific adjacency page for an optional exact family.
    ///
    /// # Errors
    ///
    /// Returns the same fail-closed errors as
    /// [`Self::repository_graph_adjacency_page`] and binds the optional family
    /// to continuation state.
    pub fn repository_graph_adjacency_page_filtered(
        &self,
        frontier: &[GraphEntityKey],
        direction: RepositoryGraphDirection,
        relation: Option<GraphRelationKind>,
        continuation: Option<&RepositoryGraphAdjacencyContinuation>,
        limit: u32,
        control: Option<&IndexWorkControl>,
    ) -> DbResult<RepositoryGraphAdjacencyPage> {
        Ok(self
            .repository_graph_adjacency_page_filtered_bounded(
                frontier,
                direction,
                relation,
                continuation,
                limit,
                maximum_repository_graph_read_budget()?,
                control,
            )?
            .page)
    }

    /// Load one family-filtered adjacency page and report exact database work.
    ///
    /// # Errors
    ///
    /// Returns the same fail-closed errors as
    /// [`Self::repository_graph_adjacency_page_filtered`] and rejects any page
    /// whose complete query work crosses `budget`.
    pub fn repository_graph_adjacency_page_filtered_bounded(
        &self,
        frontier: &[GraphEntityKey],
        direction: RepositoryGraphDirection,
        relation: Option<GraphRelationKind>,
        continuation: Option<&RepositoryGraphAdjacencyContinuation>,
        limit: u32,
        budget: RepositoryGraphReadBudget,
        control: Option<&IndexWorkControl>,
    ) -> DbResult<RepositoryGraphAdjacencyReadPage> {
        let limit_plus_one = validated_limit_plus_one(
            limit,
            GraphLimits::MAX_ROWS,
            "graph adjacency rows must be nonzero and within the product ceiling",
        )?;
        if limit > budget.returned_rows() {
            return Err(GraphContractError::InvalidLimits {
                reason: "graph adjacency page limit exceeds the return budget",
            }
            .into());
        }
        if frontier.len() > MAX_REPOSITORY_GRAPH_FRONTIER {
            return Err(GraphContractError::InvalidLimits {
                reason: "graph adjacency frontier exceeds the product ceiling",
            }
            .into());
        }
        if frontier.is_empty() {
            if continuation.is_some() {
                return Err(GraphContractError::InvalidLimits {
                    reason: "graph adjacency continuation requires a nonempty frontier",
                }
                .into());
            }
            let meter = RepositoryGraphReadMeter::new(budget, 0)?;
            return Ok(RepositoryGraphAdjacencyReadPage {
                page: empty_adjacency_page(),
                work: meter.finish(0)?,
            });
        }
        let mut meter = RepositoryGraphReadMeter::new(budget, frontier.len())?;
        let limit_plus_one_usize = usize::try_from(limit_plus_one).map_err(|_source| {
            GraphContractError::InvalidLimits {
                reason: "graph adjacency page limit overflowed",
            }
        })?;
        let project = frontier[0].project();
        let mut unique = BTreeSet::new();
        let mut frontier_digests = Vec::with_capacity(frontier.len());
        for key in frontier {
            require_project(project, key.project())?;
            let digest = key.digest_bytes()?;
            if !unique.insert(digest) {
                return Err(GraphContractError::InvalidLimits {
                    reason: "graph adjacency frontier must contain unique entities",
                }
                .into());
            }
            frontier_digests.push(digest);
        }

        let Some(generation) = self.repository_graph_generation()? else {
            if continuation.is_some() {
                return Err(GraphContractError::InvalidLimits {
                    reason: "graph adjacency continuation has no active generation",
                }
                .into());
            }
            return Ok(RepositoryGraphAdjacencyReadPage {
                page: empty_adjacency_page(),
                work: meter.finish(0)?,
            });
        };
        if !verify_project_identity(&self.connection, project)? {
            if continuation.is_some() {
                return Err(GraphContractError::InvalidLimits {
                    reason: "graph adjacency continuation does not match the bound project",
                }
                .into());
            }
            return Ok(RepositoryGraphAdjacencyReadPage {
                page: empty_adjacency_page(),
                work: meter.finish(0)?,
            });
        }

        if let Some(continuation) = continuation
            && (continuation.project != project
                || continuation.generation != generation
                || continuation.direction != direction
                || continuation.relation != relation
                || continuation.frontier != frontier_digests
                || continuation.frontier_index as usize >= frontier.len())
        {
            return Err(GraphContractError::InvalidLimits {
                reason: "graph adjacency continuation does not match the request",
            }
            .into());
        }

        let continuation_index = continuation.map_or(0, |value| value.frontier_index as usize);
        let active_frontier = frontier.len() - continuation_index;
        let work_rows = active_frontier.checked_mul(limit_plus_one_usize).ok_or(
            GraphContractError::InvalidLimits {
                reason: "graph adjacency intermediate row ceiling overflowed",
            },
        )?;
        if work_rows > MAX_REPOSITORY_GRAPH_ADJACENCY_WORK_ROWS {
            return Err(GraphContractError::InvalidLimits {
                reason: "graph adjacency frontier and page exceed the intermediate row ceiling",
            }
            .into());
        }

        let bindings_per_frontier = if relation.is_some() { 4 } else { 2 };
        let mut bindings = Vec::with_capacity(
            active_frontier * bindings_per_frontier + continuation.map_or(2, |_| 6),
        );
        bindings.push(Value::Blob(project.as_bytes().to_vec()));
        for (index, digest) in frontier_digests.iter().enumerate().skip(continuation_index) {
            bindings.push(Value::Blob(digest.to_vec()));
            if let Some(relation) = relation {
                let (scope, kind) = relation_parts(relation);
                bindings.push(Value::Text(scope.to_string()));
                bindings.push(Value::Text(kind.to_string()));
            }
            if index == continuation_index
                && let Some(continuation) = continuation
            {
                bindings.push(Value::Text(continuation.relation_scope.clone()));
                bindings.push(Value::Text(continuation.relation_kind.clone()));
                bindings.push(Value::Text(continuation.canonical_identity.clone()));
                bindings.push(Value::Blob(continuation.relation_key.to_vec()));
            }
            bindings.push(Value::Integer(limit_plus_one));
        }
        bindings.push(Value::Integer(limit_plus_one));

        let sql = adjacency_relation_sql(
            frontier.len(),
            direction,
            continuation.map(|value| value.frontier_index as usize),
            relation.is_some(),
        );
        let raw = with_sqlite_read_progress(
            &self.connection,
            control,
            IndexWorkStage::RepositoryTraversal,
            || {
                let mut statement = self.connection.prepare(&sql)?;
                collect_adjacency_relation_rows_metered(
                    statement.query(params_from_iter(bindings.iter()))?,
                    &mut meter,
                )
            },
        )?;
        let truncated = raw.len() > limit as usize;
        let next = if truncated {
            let last = raw.get(limit as usize - 1).ok_or(DbError::GraphRowShape {
                table: "graph_relations",
                reason: "adjacency truncation row is missing",
            })?;
            Some(RepositoryGraphAdjacencyContinuation {
                project,
                generation,
                direction,
                relation,
                frontier: frontier_digests,
                frontier_index: last.frontier_index,
                relation_scope: last.relation.relation_scope.clone(),
                relation_kind: last.relation.relation_kind.clone(),
                canonical_identity: last.relation.canonical.clone(),
                relation_key: fixed_bytes::<32>(
                    "graph_relations.relation_key",
                    last.relation.key.clone(),
                )?,
            })
        } else {
            None
        };
        let relation_rows = raw.iter().map(|row| &row.relation).collect::<Vec<_>>();
        let entities = load_relation_entity_references_metered(
            self,
            &relation_rows,
            project,
            generation,
            control,
            Some(&mut meter),
        )?;
        let mut rows = Vec::with_capacity(raw.len());
        for row in raw {
            if let Some(control) = control {
                control.check(IndexWorkStage::RepositoryTraversal)?;
            }
            let frontier_key = frontier
                .get(row.frontier_index as usize)
                .ok_or(DbError::GraphRowShape {
                    table: "graph_relations",
                    reason: "adjacency row has an invalid frontier position",
                })?
                .clone();
            rows.push(RepositoryGraphAdjacencyRow {
                frontier_index: row.frontier_index,
                frontier: frontier_key,
                direction,
                detail: relation_detail_from_row(&entities, row.relation, project, generation)?,
            });
        }
        if truncated {
            rows.pop();
        }
        let work = meter.finish(rows.len())?;
        Ok(RepositoryGraphAdjacencyReadPage {
            page: RepositoryGraphAdjacencyPage {
                rows,
                truncated,
                continuation: next,
            },
            work,
        })
    }

    /// Load bounded exact source occurrences for one logical relation.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid limits, project mismatch, unavailable
    /// publication state, `SQLite` failure, or any invalid span or key.
    pub fn repository_graph_occurrences(
        &self,
        relation: &LogicalRelation,
        limit: u32,
    ) -> DbResult<RepositoryGraphPage<RelationOccurrence>> {
        let limit_plus_one = validated_limit_plus_one(
            limit,
            GraphLimits::MAX_OCCURRENCES,
            "graph occurrences must be nonzero and within the product ceiling",
        )?;
        let Some(generation) = self.repository_graph_generation()? else {
            return Ok(empty_page());
        };
        if relation.generation() != generation {
            return Err(
                projectatlas_core::graph::GraphContractError::GenerationMismatch {
                    context: "relation occurrence query",
                }
                .into(),
            );
        }
        if !verify_project_identity(&self.connection, relation.key().project())? {
            return Ok(empty_page());
        }
        let raw = {
            let mut statement = self.connection.prepare_cached(
                "SELECT relation_key, file_path, start_line, start_column,
                        end_line, end_column
                   FROM graph_relation_occurrences
                  WHERE relation_key = ?1
                  ORDER BY file_path, start_line, start_column, end_line, end_column
                  LIMIT ?2",
            )?;
            let mut rows =
                statement.query(params![&relation.key().digest_bytes()?[..], limit_plus_one])?;
            let mut collected = Vec::new();
            while let Some(row) = rows.next()? {
                collected.push(occurrence_row(row)?);
            }
            collected
        };
        page_from_raw(raw, limit, |row| {
            occurrence_from_row(row, relation, generation)
        })
    }

    /// Load per-relation occurrence pages through one bounded set-oriented statement.
    ///
    /// Result pages retain input order. Every generated branch uses the
    /// relation-leading unique index and admits only `limit + 1` rows before
    /// the final stable merge.
    ///
    /// # Errors
    ///
    /// Returns an error for mixed projects or generations, oversized input or
    /// intermediate work, cancellation, unavailable publication state,
    /// `SQLite` failure, or any invalid span or key.
    pub fn repository_graph_occurrence_pages(
        &self,
        relations: &[LogicalRelation],
        limit: u32,
        control: Option<&IndexWorkControl>,
    ) -> DbResult<Vec<RepositoryGraphPage<RelationOccurrence>>> {
        Ok(self
            .repository_graph_occurrence_pages_bounded(
                relations,
                limit,
                maximum_repository_graph_read_budget()?,
                control,
            )?
            .pages)
    }

    /// Load ordered per-relation occurrence pages under one exact read envelope.
    ///
    /// # Errors
    ///
    /// Returns the same fail-closed errors as
    /// [`Self::repository_graph_occurrence_pages`] and rejects any aggregate
    /// returned-row, decoded-byte, or occurrence-path hydration overrun. Raw
    /// `limit + 1` sentinels are included in decoded and path work.
    pub fn repository_graph_occurrence_pages_bounded(
        &self,
        relations: &[LogicalRelation],
        limit: u32,
        budget: RepositoryGraphReadBudget,
        control: Option<&IndexWorkControl>,
    ) -> DbResult<RepositoryGraphReadPages<RelationOccurrence>> {
        let limit_plus_one = validated_limit_plus_one(
            limit,
            GraphLimits::MAX_OCCURRENCES,
            "graph occurrences must be nonzero and within the product ceiling",
        )?;
        if relations.len() > MAX_REPOSITORY_GRAPH_FRONTIER {
            return Err(GraphContractError::InvalidLimits {
                reason: "graph occurrence batch exceeds the product ceiling",
            }
            .into());
        }
        let mut meter = RepositoryGraphReadMeter::new(budget, relations.len())?;
        if relations.is_empty() {
            return Ok(RepositoryGraphReadPages {
                pages: Vec::new(),
                work: meter.finish(0)?,
            });
        }
        let work_rows = relations.len().checked_mul(limit_plus_one as usize).ok_or(
            GraphContractError::InvalidLimits {
                reason: "graph occurrence batch work overflowed",
            },
        )?;
        if work_rows > MAX_REPOSITORY_GRAPH_ADJACENCY_WORK_ROWS {
            return Err(GraphContractError::InvalidLimits {
                reason: "graph occurrence batch exceeds the intermediate row ceiling",
            }
            .into());
        }
        let Some(generation) = self.repository_graph_generation()? else {
            return Ok(RepositoryGraphReadPages {
                pages: (0..relations.len()).map(|_| empty_page()).collect(),
                work: meter.finish(0)?,
            });
        };
        let project = relations[0].key().project();
        if !verify_project_identity(&self.connection, project)? {
            return Ok(RepositoryGraphReadPages {
                pages: (0..relations.len()).map(|_| empty_page()).collect(),
                work: meter.finish(0)?,
            });
        }
        let mut bindings = Vec::with_capacity(relations.len() * 2);
        for relation in relations {
            require_project(project, relation.key().project())?;
            if relation.generation() != generation {
                return Err(GraphContractError::GenerationMismatch {
                    context: "relation occurrence batch query",
                }
                .into());
            }
            bindings.push(Value::Blob(relation.key().digest_bytes()?.to_vec()));
            bindings.push(Value::Integer(limit_plus_one));
        }
        let sql = occurrence_pages_sql(relations.len());
        let raw = with_sqlite_read_progress(
            &self.connection,
            control,
            IndexWorkStage::RepositoryTraversal,
            || {
                let mut statement = self.connection.prepare(&sql)?;
                let mut queried = statement.query(params_from_iter(bindings.iter()))?;
                let mut grouped = (0..relations.len())
                    .map(|_| Vec::new())
                    .collect::<Vec<Vec<OccurrenceRow>>>();
                while let Some(row) = queried.next()? {
                    let index = row.get::<_, usize>(0)?;
                    let group = grouped.get_mut(index).ok_or(DbError::GraphRowShape {
                        table: "graph_relation_occurrences",
                        reason: "occurrence batch returned an invalid relation position",
                    })?;
                    let occurrence = occurrence_row_at(row, 1)?;
                    meter.record_decoded_bytes(
                        occurrence_row_decoded_bytes(&occurrence)?
                            .checked_add(8)
                            .ok_or(GraphContractError::InvalidLimits {
                                reason: "graph occurrence batch decoded row size overflowed",
                            })?,
                    )?;
                    group.push(occurrence);
                }
                Ok(grouped)
            },
        )?;
        let pages = raw
            .into_iter()
            .zip(relations)
            .map(|(rows, relation)| {
                page_from_raw(rows, limit, |row| {
                    let occurrence = occurrence_from_row(row, relation, generation)?;
                    meter.record_hydrated_path(occurrence.file().as_str())?;
                    Ok(occurrence)
                })
            })
            .collect::<DbResult<Vec<_>>>()?;
        let returned_rows = pages.iter().try_fold(0_usize, |count, page| {
            count
                .checked_add(page.rows.len())
                .ok_or(GraphContractError::InvalidLimits {
                    reason: "graph occurrence returned-row accounting overflowed",
                })
        })?;
        let work = meter.finish(returned_rows)?;
        Ok(RepositoryGraphReadPages { pages, work })
    }

    /// Load bounded coverage rows for one exact project or path scope.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid limits, unavailable publication state,
    /// project mismatch, `SQLite` failure, or any inconsistent coverage row.
    pub fn repository_graph_coverage(
        &self,
        project: ProjectInstanceId,
        scope: &CoverageScope,
        limit: u32,
    ) -> DbResult<RepositoryGraphPage<CoverageRecord>> {
        let limit_plus_one = validated_limit_plus_one(
            limit,
            GraphLimits::MAX_ROWS,
            "graph rows must be nonzero and within the product ceiling",
        )?;
        let Some(generation) = self.repository_graph_generation()? else {
            return Ok(empty_page());
        };
        if !verify_project_identity(&self.connection, project)? {
            return Ok(empty_page());
        }
        let (scope_kind, scope_path) = coverage_scope_parts(scope);
        let raw = {
            let mut statement = self.connection.prepare_cached(
                "SELECT project_instance_id, scope_kind, scope_path, relation_scope,
                        relation_kind, state, total, covered, omitted, reason, reached_limit,
                        NULL, NULL
                   FROM graph_coverage
                  WHERE project_instance_id = ?1
                    AND scope_kind = ?2 AND scope_path IS ?3
                  ORDER BY relation_scope, relation_kind, state, id
                  LIMIT ?4",
            )?;
            let mut rows = statement.query(params![
                &project.as_bytes()[..],
                scope_kind,
                scope_path,
                limit_plus_one
            ])?;
            let mut collected = Vec::new();
            while let Some(row) = rows.next()? {
                collected.push(coverage_row(row)?);
            }
            collected
        };
        page_from_raw(raw, limit, |row| {
            coverage_from_row(row, project, generation)
        })
    }

    /// Load current coverage for a bounded unique set of exact repository paths.
    ///
    /// The complete path set is bound to one prepared statement so service
    /// traversal never performs one coverage query per returned node.
    ///
    /// # Errors
    ///
    /// Returns an error for duplicate or oversized path sets, cancellation,
    /// unavailable publication state, project mismatch, `SQLite` failure, or
    /// invalid persisted coverage.
    pub fn repository_graph_path_coverage(
        &self,
        project: ProjectInstanceId,
        paths: &[RepositoryNodePath],
        control: Option<&IndexWorkControl>,
    ) -> DbResult<RepositoryGraphPage<CoverageRecord>> {
        validate_path_coverage_request(paths)?;
        if paths.is_empty() {
            return Ok(empty_page());
        }
        let Some(generation) = self.repository_graph_generation()? else {
            return Ok(empty_page());
        };
        if !verify_project_identity(&self.connection, project)? {
            return Ok(empty_page());
        }
        Ok(self
            .repository_graph_path_coverage_bounded(
                project,
                generation,
                paths,
                maximum_repository_graph_read_budget()?,
                control,
            )?
            .page)
    }

    /// Load current exact-path coverage under one stable graph read envelope.
    ///
    /// # Errors
    ///
    /// Returns the same fail-closed errors as
    /// [`Self::repository_graph_path_coverage`], rejects a stale project or
    /// generation and any returned-row, decoded-byte, or coverage-path
    /// hydration overrun, and never returns a partial batch. The raw truncation
    /// sentinel is included in decoded and path work.
    pub fn repository_graph_path_coverage_bounded(
        &self,
        project: ProjectInstanceId,
        generation: IndexGeneration,
        paths: &[RepositoryNodePath],
        budget: RepositoryGraphReadBudget,
        control: Option<&IndexWorkControl>,
    ) -> DbResult<RepositoryGraphReadPage<CoverageRecord>> {
        validate_path_coverage_request(paths)?;
        self.require_repository_graph_snapshot(project, generation)?;
        let mut meter = RepositoryGraphReadMeter::new(budget, paths.len())?;
        if paths.is_empty() {
            return Ok(RepositoryGraphReadPage {
                page: empty_page(),
                work: meter.finish(0)?,
            });
        }

        let sql = path_coverage_sql(paths.len());
        let mut bindings = Vec::with_capacity(paths.len() + 3);
        bindings.push(Value::Blob(project.as_bytes().to_vec()));
        bindings.push(Value::Text("path".to_string()));
        bindings.extend(
            paths
                .iter()
                .map(|path| Value::Text(path.as_str().to_string())),
        );
        bindings.push(Value::Integer(i64::from(GraphLimits::MAX_ROWS) + 1));
        let raw = with_sqlite_read_progress(
            &self.connection,
            control,
            IndexWorkStage::RepositoryTraversal,
            || {
                let mut statement = self.connection.prepare(&sql)?;
                let mut rows = statement.query(params_from_iter(bindings.iter()))?;
                let mut collected = Vec::new();
                while let Some(row) = rows.next()? {
                    let coverage = coverage_row(row)?;
                    meter.record_decoded_bytes(coverage_row_decoded_bytes(&coverage)?)?;
                    collected.push(coverage);
                }
                Ok(collected)
            },
        )?;
        let page = page_from_raw(raw, GraphLimits::MAX_ROWS, |row| {
            let coverage = coverage_from_row(row, project, generation)?;
            if let CoverageScope::Path { path } = coverage.scope() {
                meter.record_hydrated_path(path.as_str())?;
            }
            Ok(coverage)
        })?;
        let work = meter.finish(page.rows.len())?;
        Ok(RepositoryGraphReadPage { page, work })
    }

    /// Discover one bounded page of current coverage with optional typed filters.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid bounds, unavailable publication state,
    /// project mismatch, `SQLite` failure, or invalid persisted coverage or
    /// parser provenance.
    pub fn repository_coverage_page(
        &self,
        project: ProjectInstanceId,
        query: &RepositoryCoverageQuery,
    ) -> DbResult<RepositoryGraphPage<RepositoryCoverageRow>> {
        let limit_plus_one = validated_limit_plus_one(
            query.limit,
            GraphLimits::MAX_ROWS,
            "coverage rows must be nonzero and within the product ceiling",
        )?;
        if query.start_index >= GraphLimits::MAX_ROWS {
            return Err(GraphContractError::InvalidLimits {
                reason: "coverage start index is at or above the product ceiling",
            }
            .into());
        }
        let Some(generation) = self.repository_graph_generation()? else {
            return Ok(empty_page());
        };
        if !verify_project_identity(&self.connection, project)? {
            return Ok(empty_page());
        }

        let provenance_driven = query.parser.is_some() || query.provider.is_some();
        let path_prefix = query.path_prefix.as_deref().filter(|prefix| *prefix != ".");
        let mut sql = String::from(
            "SELECT coverage.project_instance_id, coverage.scope_kind,
                    coverage.scope_path, coverage.relation_scope,
                    coverage.relation_kind, coverage.state, coverage.total,
                    coverage.covered, coverage.omitted, coverage.reason,
                    coverage.reached_limit, metadata.source_parser,
                    metadata.fact_parser
               FROM ",
        );
        if provenance_driven {
            sql.push_str(
                "source_parse_metadata AS metadata
                 CROSS JOIN graph_coverage AS coverage
                   ON coverage.scope_kind = 'path'
                  AND coverage.scope_path = metadata.path",
            );
        } else {
            sql.push_str(
                "graph_coverage AS coverage
                 LEFT JOIN source_parse_metadata AS metadata
                   ON metadata.path = coverage.scope_path",
            );
        }
        sql.push_str(" WHERE coverage.project_instance_id = ?");
        let mut values = vec![Value::Blob(project.as_bytes().to_vec())];

        if let Some(prefix) = path_prefix {
            sql.push_str(
                " AND coverage.scope_kind = 'path'
                  AND coverage.scope_path >= ? AND coverage.scope_path < ?
                  AND (coverage.scope_path = ? OR coverage.scope_path >= ?)",
            );
            values.push(Value::Text(prefix.to_string()));
            values.push(Value::Text(format!("{prefix}0")));
            values.push(Value::Text(prefix.to_string()));
            values.push(Value::Text(format!("{prefix}/")));
        }
        if let Some(parser) = query.parser {
            sql.push_str(" AND metadata.source_parser = ?");
            values.push(Value::Text(parser.to_string()));
        }
        if let Some(provider) = query.provider {
            sql.push_str(" AND metadata.fact_parser = ?");
            values.push(Value::Text(provider.to_string()));
        }
        if let Some(relation) = query.relation {
            let (scope, kind) = relation_parts(relation);
            sql.push_str(" AND coverage.relation_scope = ? AND coverage.relation_kind = ?");
            values.push(Value::Text(scope.to_string()));
            values.push(Value::Text(kind.to_string()));
        }
        if let Some(state) = query.state {
            sql.push_str(" AND coverage.state = ?");
            values.push(Value::Text(coverage_state_name(state).to_string()));
        }
        if let Some(reason) = query.reason.as_deref() {
            sql.push_str(" AND coverage.reason = ?");
            values.push(Value::Text(reason.to_string()));
        }

        if provenance_driven {
            sql.push_str(" ORDER BY metadata.path, coverage.id");
        } else if path_prefix.is_some() {
            sql.push_str(
                " ORDER BY coverage.scope_path, coverage.relation_scope,
                          coverage.relation_kind, coverage.state, coverage.id",
            );
        } else if query.relation.is_some() {
            sql.push_str(
                " ORDER BY coverage.relation_scope, coverage.relation_kind,
                          coverage.state, coverage.id",
            );
        } else if query.state.is_some() {
            sql.push_str(" ORDER BY coverage.state, coverage.scope_path, coverage.id");
        } else if query.reason.is_some() {
            sql.push_str(" ORDER BY coverage.reason, coverage.scope_path, coverage.id");
        } else {
            sql.push_str(
                " ORDER BY coverage.scope_kind, coverage.scope_path,
                          coverage.relation_scope, coverage.relation_kind,
                          coverage.state, coverage.id",
            );
        }
        sql.push_str(" LIMIT ? OFFSET ?");
        values.push(Value::Integer(limit_plus_one));
        values.push(Value::Integer(i64::from(query.start_index)));

        let raw = {
            let mut statement = self.connection.prepare_cached(&sql)?;
            let mut rows = statement.query(params_from_iter(values.iter()))?;
            let mut collected = Vec::new();
            while let Some(row) = rows.next()? {
                collected.push(coverage_row(row)?);
            }
            collected
        };
        page_from_raw(raw, query.limit, |row| {
            coverage_discovery_from_row(row, project, generation)
        })
    }

    /// Return the complete generation used to reconstruct normalized graph rows.
    ///
    /// # Errors
    ///
    /// Returns an error when publication metadata is incomplete or disagrees
    /// with the project identity's active graph generation.
    pub fn repository_graph_generation(&self) -> DbResult<Option<IndexGeneration>> {
        let Some(publication) = self.index_publication()? else {
            return Ok(None);
        };
        if publication.state != IndexPublicationState::Complete
            || publication.generation == IndexGeneration::ZERO
        {
            return Err(DbError::GraphPublicationUnavailable);
        }
        let Some(graph_generation) = load_graph_generation(&self.connection)? else {
            return Ok(None);
        };
        if graph_generation == IndexGeneration::ZERO || graph_generation != publication.generation {
            return Err(DbError::GraphRowShape {
                table: "project_identity",
                reason: "typed graph generation does not match complete publication",
            });
        }
        Ok(Some(graph_generation))
    }

    /// Require one exact published graph snapshot for cursor-owned hydration.
    pub(crate) fn require_repository_graph_snapshot(
        &self,
        project: ProjectInstanceId,
        generation: IndexGeneration,
    ) -> DbResult<()> {
        require_bound_project_identity(&self.connection, project)?;
        let current = self
            .repository_graph_generation()?
            .ok_or(DbError::GraphPublicationUnavailable)?;
        if current != generation {
            return Err(GraphContractError::InvalidLimits {
                reason: "graph hydration generation does not match the current publication",
            }
            .into());
        }
        Ok(())
    }

    /// Collect one indexed relation page by source or target key.
    fn collect_relation_rows_by_key(
        &self,
        key_column: &'static str,
        key: &[u8; 32],
        limit_plus_one: i64,
    ) -> DbResult<Vec<RelationRow>> {
        let sql = match key_column {
            "source_entity_key" => {
                "SELECT relation_key, project_instance_id, canonical_identity,
                        source_entity_key, relation_scope, relation_kind,
                        resolution_status, target_entity_key, reference_text,
                        candidate_count, confidence, completeness
                   FROM graph_relations
                  WHERE source_entity_key = ?1
                  ORDER BY relation_scope, relation_kind, canonical_identity, relation_key
                  LIMIT ?2"
            }
            "target_entity_key" => {
                "SELECT relation_key, project_instance_id, canonical_identity,
                        source_entity_key, relation_scope, relation_kind,
                        resolution_status, target_entity_key, reference_text,
                        candidate_count, confidence, completeness
                   FROM graph_relations
                  WHERE target_entity_key = ?1
                  ORDER BY relation_scope, relation_kind, canonical_identity, relation_key
                  LIMIT ?2"
            }
            _ => {
                return Err(DbError::GraphRowShape {
                    table: "graph_relations",
                    reason: "unsupported internal relation lookup",
                });
            }
        };
        let mut statement = self.connection.prepare_cached(sql)?;
        collect_relation_rows(statement.query(params![&key[..], limit_plus_one])?)
    }
}

impl IndexPublicationGuard<'_> {
    /// Replace the complete normalized repository graph inside this publication.
    ///
    /// # Errors
    ///
    /// Returns an error when records do not belong to the pending generation or
    /// selected project, a stable-key collision is detected, or `SQLite` fails.
    pub fn replace_repository_graph(
        &mut self,
        project: ProjectInstanceId,
        entities: &[GraphEntity],
        relations: &[LogicalRelation],
        occurrences: &[RelationOccurrence],
        coverage: &[CoverageRecord],
    ) -> DbResult<()> {
        self.replace_repository_graph_with_resolution_keys(
            project,
            entities,
            relations,
            occurrences,
            coverage,
            &[],
            &[],
        )
    }

    /// Replace the complete graph and canonical resolution-key projection atomically.
    ///
    /// # Errors
    ///
    /// Returns an error when graph or key records do not belong to the pending
    /// generation and selected project, a stable-key collision is detected, an
    /// owner has no exact source path, or `SQLite` fails.
    #[allow(clippy::too_many_arguments)]
    pub fn replace_repository_graph_with_resolution_keys(
        &mut self,
        project: ProjectInstanceId,
        entities: &[GraphEntity],
        relations: &[LogicalRelation],
        occurrences: &[RelationOccurrence],
        coverage: &[CoverageRecord],
        entity_exports: &[EntityResolutionKey],
        relation_dependencies: &[RelationDependencyKey],
    ) -> DbResult<()> {
        let generation = self.pending_graph_generation()?;
        validate_graph_batch(
            project,
            generation,
            entities,
            relations,
            occurrences,
            coverage,
        )?;
        validate_resolution_key_batch(project, entity_exports, relation_dependencies)?;
        let savepoint = self.store.connection.savepoint()?;
        require_bound_project_identity(&savepoint, project)?;
        savepoint.execute("DELETE FROM graph_resolution_keys", [])?;
        savepoint.execute("DELETE FROM graph_coverage", [])?;
        savepoint.execute("DELETE FROM graph_relations", [])?;
        savepoint.execute("DELETE FROM graph_entities", [])?;
        insert_graph_batch(
            &savepoint,
            project,
            entities,
            relations,
            occurrences,
            coverage,
        )?;
        insert_resolution_key_batch(&savepoint, project, entity_exports, relation_dependencies)?;
        set_graph_generation(&savepoint, generation)?;
        savepoint.commit()?;
        Ok(())
    }

    /// Replace the normalized graph closure owned by affected repository paths.
    ///
    /// Unchanged rows stay physically untouched and are reconstructed at the next
    /// complete publication generation. The caller supplies the complete new
    /// closure for the affected paths.
    ///
    /// # Errors
    ///
    /// Returns an error when records do not belong to the pending generation or
    /// selected project, a stable-key collision is detected, or `SQLite` fails.
    pub fn replace_repository_graph_for_paths(
        &mut self,
        project: ProjectInstanceId,
        affected_paths: &[String],
        entities: &[GraphEntity],
        relations: &[LogicalRelation],
        occurrences: &[RelationOccurrence],
        coverage: &[CoverageRecord],
    ) -> DbResult<()> {
        self.replace_repository_graph_for_paths_with_resolution_keys(
            project,
            affected_paths,
            entities,
            relations,
            occurrences,
            coverage,
            &[],
            &[],
        )
    }

    /// Replace one affected graph closure and its canonical resolution keys atomically.
    ///
    /// # Errors
    ///
    /// Returns an error when graph or key records do not belong to the pending
    /// generation and selected project, a stable-key collision is detected, an
    /// owner has no exact source path, or `SQLite` fails.
    #[allow(clippy::too_many_arguments)]
    pub fn replace_repository_graph_for_paths_with_resolution_keys(
        &mut self,
        project: ProjectInstanceId,
        affected_paths: &[String],
        entities: &[GraphEntity],
        relations: &[LogicalRelation],
        occurrences: &[RelationOccurrence],
        coverage: &[CoverageRecord],
        entity_exports: &[EntityResolutionKey],
        relation_dependencies: &[RelationDependencyKey],
    ) -> DbResult<()> {
        let generation = self.pending_graph_generation()?;
        validate_graph_batch(
            project,
            generation,
            entities,
            relations,
            occurrences,
            coverage,
        )?;
        validate_resolution_key_batch(project, entity_exports, relation_dependencies)?;
        let affected_paths = affected_paths
            .iter()
            .map(|path| RepositoryNodePath::new(Path::new(path)))
            .collect::<Result<Vec<_>, _>>()?;
        let savepoint = self.store.connection.savepoint()?;
        require_bound_project_identity(&savepoint, project)?;
        if affected_paths.iter().any(|path| path.as_str() == ".") {
            savepoint.execute("DELETE FROM graph_resolution_keys", [])?;
            savepoint.execute("DELETE FROM graph_coverage", [])?;
            savepoint.execute("DELETE FROM graph_relations", [])?;
            savepoint.execute("DELETE FROM graph_entities", [])?;
            insert_graph_batch(
                &savepoint,
                project,
                entities,
                relations,
                occurrences,
                coverage,
            )?;
            insert_resolution_key_batch(
                &savepoint,
                project,
                entity_exports,
                relation_dependencies,
            )?;
            set_graph_generation(&savepoint, generation)?;
            savepoint.commit()?;
            return Ok(());
        }
        let touched_keys = resolution_keys_for_owner_paths(
            &savepoint,
            project,
            &affected_paths,
            ResolutionOwner::Both,
            None,
        )?;
        let mut orphan_candidates = affected_external_candidates(&savepoint, &affected_paths)?;
        invalidate_repository_graph_paths(&savepoint, &affected_paths, &mut orphan_candidates)?;
        insert_graph_batch(
            &savepoint,
            project,
            entities,
            relations,
            occurrences,
            coverage,
        )?;
        insert_resolution_key_batch(&savepoint, project, entity_exports, relation_dependencies)?;
        for entity in entities {
            if matches!(entity.selector(), EntitySelector::External { .. }) {
                orphan_candidates.insert(entity.key().digest_bytes()?);
            }
        }
        remove_orphan_external_candidates(&savepoint, &orphan_candidates)?;
        remove_touched_orphan_resolution_keys(&savepoint, &touched_keys)?;
        set_graph_generation(&savepoint, generation)?;
        savepoint.commit()?;
        Ok(())
    }

    /// Return the generation that will become complete if this guard commits.
    fn pending_graph_generation(&self) -> DbResult<IndexGeneration> {
        self.previous_generation
            .checked_next()
            .ok_or(DbError::PublicationGenerationOverflow)
    }
}

/// Conservative key count that stays below `SQLite`'s legacy bind ceiling.
const RESOLUTION_KEYS_PER_QUERY: usize = 200;
/// Conservative path count that stays below `SQLite`'s legacy bind ceiling.
const RESOLUTION_PATHS_PER_QUERY: usize = 400;

/// Closed owner projections that retain canonical resolution keys.
#[derive(Clone, Copy)]
enum ResolutionOwner {
    /// Entity-export bindings only.
    EntityExports,
    /// Both binding families, used for touched-key garbage collection.
    Both,
}

impl ResolutionOwner {
    /// Return the owner tables and their path-first access indexes.
    fn tables(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::EntityExports => &[("graph_entity_exports", "idx_graph_entity_exports_owner")],
            Self::Both => &[
                ("graph_entity_exports", "idx_graph_entity_exports_owner"),
                (
                    "graph_relation_dependencies",
                    "idx_graph_relation_dependencies_owner",
                ),
            ],
        }
    }
}

/// Validate and deduplicate exact source paths before querying owner bindings.
fn normalized_file_paths(paths: &[String]) -> DbResult<Vec<RepositoryNodePath>> {
    let mut normalized = BTreeSet::new();
    for path in paths {
        let file = RepositoryFilePath::new(Path::new(path))?;
        normalized.insert(RepositoryNodePath::new(Path::new(file.as_str()))?);
    }
    Ok(normalized.into_iter().collect())
}

/// Validate project ownership and deduplicate canonical keys deterministically.
fn normalized_resolution_keys(
    project: ProjectInstanceId,
    keys: &[CanonicalResolutionKey],
) -> DbResult<Vec<CanonicalResolutionKey>> {
    let mut normalized = BTreeSet::new();
    for key in keys {
        if key.project() != project {
            return Err(DbError::GraphProjectIdentityMismatch {
                expected: project.to_string(),
                found: key.project().to_string(),
            });
        }
        normalized.insert(key.clone());
    }
    Ok(normalized.into_iter().collect())
}

/// Convert a sorted unique set into one `LIMIT + 1` page.
fn page_from_ordered_set<T: Ord>(rows: BTreeSet<T>, limit: u32) -> RepositoryGraphPage<T> {
    let truncated = rows.len() > limit as usize;
    RepositoryGraphPage {
        rows: rows.into_iter().take(limit as usize).collect(),
        truncated,
    }
}

/// Load and validate canonical keys retained by path-owned graph projections.
fn resolution_keys_for_owner_paths(
    connection: &Connection,
    project: ProjectInstanceId,
    paths: &[RepositoryNodePath],
    owners: ResolutionOwner,
    limit: Option<u32>,
) -> DbResult<BTreeSet<CanonicalResolutionKey>> {
    let mut keys = BTreeSet::new();
    for (table, index) in owners.tables() {
        for chunk in paths.chunks(RESOLUTION_PATHS_PER_QUERY) {
            if chunk.is_empty() {
                continue;
            }
            let placeholders = vec!["?"; chunk.len()].join(",");
            let limit_clause = limit.map_or("", |_| " LIMIT ?");
            let sql = format!(
                "SELECT DISTINCT resolution.project_instance_id,
                        resolution.resolution_domain, resolution.key_digest,
                        resolution.canonical_identity
                   FROM {table} AS owner INDEXED BY {index}
                   JOIN graph_resolution_keys AS resolution
                     ON resolution.project_instance_id = owner.project_instance_id
                    AND resolution.resolution_domain = owner.resolution_domain
                    AND resolution.key_digest = owner.key_digest
                  WHERE owner.project_instance_id = ?
                    AND owner.owner_path IN ({placeholders})
                  ORDER BY resolution.resolution_domain, resolution.key_digest{limit_clause}"
            );
            let mut values = Vec::with_capacity(chunk.len() + 2);
            values.push(Value::Blob(project.as_bytes().to_vec()));
            values.extend(
                chunk
                    .iter()
                    .map(|path| Value::Text(path.as_str().to_string())),
            );
            if let Some(limit) = limit {
                values.push(Value::Integer(i64::from(limit) + 1));
            }
            let mut statement = connection.prepare(&sql)?;
            let mut rows = statement.query(params_from_iter(values.iter()))?;
            while let Some(row) = rows.next()? {
                keys.insert(resolution_key_from_row(row, project)?);
            }
            if limit.is_some_and(|limit| keys.len() > limit as usize) {
                return Ok(keys);
            }
        }
    }
    Ok(keys)
}

/// Reconstruct one canonical key row without accepting malformed persisted data.
fn resolution_key_from_row(
    row: &Row<'_>,
    expected_project: ProjectInstanceId,
) -> DbResult<CanonicalResolutionKey> {
    let project = project_from_blob(
        "graph_resolution_keys.project_instance_id",
        row.get::<_, Vec<u8>>(0)?,
    )?;
    require_project(expected_project, project)?;
    let domain_text = row.get::<_, String>(1)?;
    let domain = ResolutionKeyDomain::try_from(domain_text.as_str())?;
    let digest = fixed_bytes::<32>(
        "graph_resolution_keys.key_digest",
        row.get::<_, Vec<u8>>(2)?,
    )?;
    Ok(CanonicalResolutionKey::from_persisted(
        project,
        domain,
        digest,
        row.get(3)?,
    )?)
}

/// Validate one requested key against any retained canonical witness.
fn validate_persisted_resolution_key(
    connection: &Connection,
    key: &CanonicalResolutionKey,
) -> DbResult<bool> {
    let stored = connection
        .query_row(
            "SELECT canonical_identity
               FROM graph_resolution_keys
              WHERE project_instance_id = ?1
                AND resolution_domain = ?2
                AND key_digest = ?3",
            params![
                &key.project().as_bytes()[..],
                key.domain().as_str(),
                &key.digest_bytes()[..],
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(stored) = stored else {
        return Ok(false);
    };
    if stored != key.canonical_identity() {
        return Err(DbError::ResolutionKeyCollision {
            domain: key.domain().as_str(),
            digest: key.digest_bytes(),
        });
    }
    CanonicalResolutionKey::from_persisted(
        key.project(),
        key.domain(),
        key.digest_bytes(),
        stored,
    )?;
    Ok(true)
}

/// Validate all retained witnesses for a key batch without per-key queries.
fn validate_persisted_resolution_keys(
    connection: &Connection,
    keys: &[CanonicalResolutionKey],
) -> DbResult<()> {
    for chunk in keys.chunks(RESOLUTION_KEYS_PER_QUERY) {
        if chunk.is_empty() {
            continue;
        }
        let values_clause = resolution_values_clause(chunk.len(), 4);
        let sql = format!(
            "WITH requested(project_instance_id, resolution_domain, key_digest, canonical_identity)
                  AS (VALUES {values_clause})
             SELECT requested.resolution_domain, requested.key_digest,
                    requested.canonical_identity, stored.canonical_identity
               FROM requested
               JOIN graph_resolution_keys AS stored
                 ON stored.project_instance_id = requested.project_instance_id
                AND stored.resolution_domain = requested.resolution_domain
                AND stored.key_digest = requested.key_digest
              ORDER BY requested.resolution_domain, requested.key_digest"
        );
        let values = resolution_key_values(chunk, true);
        let mut statement = connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values.iter()))?;
        while let Some(row) = rows.next()? {
            let domain_text = row.get::<_, String>(0)?;
            let domain = ResolutionKeyDomain::try_from(domain_text.as_str())?;
            let digest = fixed_bytes::<32>(
                "graph_resolution_keys.key_digest",
                row.get::<_, Vec<u8>>(1)?,
            )?;
            let requested = row.get::<_, String>(2)?;
            let stored = row.get::<_, String>(3)?;
            if requested != stored {
                return Err(DbError::ResolutionKeyCollision {
                    domain: domain.as_str(),
                    digest,
                });
            }
        }
    }
    Ok(())
}

/// Return distinct ordered dependency-owning paths for a bounded key set.
fn affected_source_paths(
    connection: &Connection,
    keys: &[CanonicalResolutionKey],
    limit: u32,
) -> DbResult<BTreeSet<RepositoryFilePath>> {
    let mut paths = BTreeSet::new();
    for chunk in keys.chunks(RESOLUTION_KEYS_PER_QUERY) {
        if chunk.is_empty() {
            continue;
        }
        let values_clause = resolution_values_clause(chunk.len(), 4);
        let sql = format!(
            "WITH requested(project_instance_id, resolution_domain, key_digest, canonical_identity)
                  AS (VALUES {values_clause})
             SELECT DISTINCT dependency.owner_path
               FROM requested
               JOIN graph_resolution_keys AS stored
                 ON stored.project_instance_id = requested.project_instance_id
                AND stored.resolution_domain = requested.resolution_domain
                AND stored.key_digest = requested.key_digest
                AND stored.canonical_identity = requested.canonical_identity
               JOIN graph_relation_dependencies AS dependency
                    INDEXED BY idx_graph_relation_dependencies_key
                 ON dependency.project_instance_id = stored.project_instance_id
                AND dependency.resolution_domain = stored.resolution_domain
                AND dependency.key_digest = stored.key_digest
              ORDER BY dependency.owner_path
              LIMIT ?"
        );
        let mut values = resolution_key_values(chunk, true);
        values.push(Value::Integer(i64::from(limit) + 1));
        let mut statement = connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values.iter()))?;
        while let Some(row) = rows.next()? {
            paths.insert(RepositoryFilePath::new(Path::new(
                &row.get::<_, String>(0)?,
            ))?);
        }
        if paths.len() > limit as usize {
            return Ok(paths);
        }
    }
    Ok(paths)
}

/// Account one exact-path closure through bounded index-owned branches.
fn affected_source_footprint(
    connection: &Connection,
    project: ProjectInstanceId,
    paths: &[RepositoryNodePath],
    limit_plus_one: i64,
) -> DbResult<RepositoryAffectedSourceFootprint> {
    let maximum_rows = u64::try_from(limit_plus_one).map_err(|source| DbError::InvalidCount {
        field: "affected_source_footprint.limit_plus_one",
        value: limit_plus_one,
        source,
    })?;
    let mut footprint = empty_affected_source_footprint();
    for chunk in paths.chunks(RESOLUTION_PATHS_PER_QUERY) {
        if chunk.is_empty() {
            continue;
        }
        let remaining = maximum_rows.saturating_sub(footprint.rows);
        if remaining == 0 {
            footprint.truncated = true;
            break;
        }
        let sql = affected_source_footprint_sql(chunk.len());
        let mut values = Vec::with_capacity(chunk.len() + 2);
        values.push(Value::Blob(project.as_bytes().to_vec()));
        values.extend(
            chunk
                .iter()
                .map(|path| Value::Text(path.as_str().to_string())),
        );
        values.push(Value::Integer(i64::try_from(remaining).map_err(
            |source| DbError::InvalidCount {
                field: "affected_source_footprint.remaining_rows",
                value: i64::MAX,
                source,
            },
        )?));
        let mut statement = connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values.iter()))?;
        while let Some(row) = rows.next()? {
            let bytes = nonnegative_u64(
                "affected_source_footprint.retained_bytes",
                row.get::<_, i64>(0)?,
            )?;
            footprint.rows = footprint.rows.saturating_add(1);
            footprint.retained_bytes = footprint.retained_bytes.saturating_add(bytes);
        }
        if footprint.rows >= maximum_rows {
            footprint.truncated = true;
            break;
        }
    }
    Ok(footprint)
}

/// Build one bounded union of exact-path footprint branches.
fn affected_source_footprint_sql(path_count: usize) -> String {
    let requested = vec!["(?)"; path_count].join(",");
    format!(
        "WITH selected_project(project_instance_id) AS (VALUES (?)),
              requested(path) AS (VALUES {requested})
         SELECT retained_bytes
           FROM (
             SELECT length(CAST(metadata.path AS BLOB))
                    + coalesce(length(CAST(metadata.language AS BLOB)), 0)
                    + length(CAST(metadata.source_parser AS BLOB))
                    + length(CAST(metadata.fact_parser AS BLOB)) + 16
                    + length(CAST(metadata.updated_at AS BLOB)) AS retained_bytes
               FROM requested
               JOIN source_parse_metadata AS metadata
                    INDEXED BY sqlite_autoindex_source_parse_metadata_1
                 ON metadata.path = requested.path
             UNION ALL
             SELECT 32 + length(CAST(symbol.path AS BLOB))
                    + coalesce(length(CAST(symbol.language AS BLOB)), 0)
                    + length(CAST(symbol.name AS BLOB))
                    + length(CAST(symbol.kind AS BLOB))
                    + length(CAST(symbol.signature AS BLOB))
                    + coalesce(length(CAST(symbol.documentation AS BLOB)), 0)
                    + coalesce(length(CAST(symbol.parent AS BLOB)), 0)
                    + length(CAST(symbol.parser AS BLOB))
                    + coalesce(length(CAST(symbol.detail AS BLOB)), 0)
                    + length(CAST(symbol.created_at AS BLOB))
                    + length(CAST(symbol.updated_at AS BLOB))
               FROM requested
               JOIN symbols AS symbol INDEXED BY idx_symbols_path
                 ON symbol.path = requested.path
             UNION ALL
             SELECT 16 + length(CAST(symbol_relation.path AS BLOB))
                    + length(CAST(symbol_relation.source_name AS BLOB))
                    + length(CAST(symbol_relation.target_name AS BLOB))
                    + length(CAST(symbol_relation.kind AS BLOB))
                    + length(CAST(symbol_relation.context AS BLOB))
                    + length(CAST(symbol_relation.parser AS BLOB))
                    + length(CAST(symbol_relation.created_at AS BLOB))
               FROM requested
               JOIN symbol_relations AS symbol_relation INDEXED BY idx_symbol_relations_path
                 ON symbol_relation.path = requested.path
             UNION ALL
             SELECT 48 + length(CAST(entity.canonical_identity AS BLOB))
                    + length(CAST(entity.entity_kind AS BLOB))
                    + coalesce(length(CAST(entity.repository_path AS BLOB)), 0)
                    + coalesce(length(CAST(entity.package_manager AS BLOB)), 0)
                    + coalesce(length(CAST(entity.package_name AS BLOB)), 0)
                    + coalesce(length(CAST(entity.manifest_path AS BLOB)), 0)
                    + coalesce(length(CAST(entity.symbol_name AS BLOB)), 0)
                    + coalesce(length(CAST(entity.symbol_kind AS BLOB)), 0)
                    + coalesce(length(CAST(entity.symbol_parent AS BLOB)), 0)
                    + coalesce(length(CAST(entity.symbol_signature AS BLOB)), 0)
                    + coalesce(length(CAST(entity.external_system AS BLOB)), 0)
                    + coalesce(length(CAST(entity.external_identity AS BLOB)), 0)
               FROM requested
               JOIN graph_entities AS entity INDEXED BY idx_graph_entities_path
                 ON entity.repository_path = requested.path
               JOIN selected_project
                 ON selected_project.project_instance_id = entity.project_instance_id
             UNION ALL
             SELECT 48 + length(CAST(entity.canonical_identity AS BLOB))
                    + length(CAST(entity.entity_kind AS BLOB))
                    + coalesce(length(CAST(entity.repository_path AS BLOB)), 0)
                    + coalesce(length(CAST(entity.package_manager AS BLOB)), 0)
                    + coalesce(length(CAST(entity.package_name AS BLOB)), 0)
                    + coalesce(length(CAST(entity.manifest_path AS BLOB)), 0)
                    + coalesce(length(CAST(entity.symbol_name AS BLOB)), 0)
                    + coalesce(length(CAST(entity.symbol_kind AS BLOB)), 0)
                    + coalesce(length(CAST(entity.symbol_parent AS BLOB)), 0)
                    + coalesce(length(CAST(entity.symbol_signature AS BLOB)), 0)
                    + coalesce(length(CAST(entity.external_system AS BLOB)), 0)
                    + coalesce(length(CAST(entity.external_identity AS BLOB)), 0)
               FROM requested
               JOIN graph_entities AS entity INDEXED BY idx_graph_entities_manifest_path
                 ON entity.manifest_path = requested.path
               JOIN selected_project
                 ON selected_project.project_instance_id = entity.project_instance_id
             UNION ALL
             SELECT 80 + length(CAST(relation.canonical_identity AS BLOB))
                    + length(CAST(relation.relation_scope AS BLOB))
                    + length(CAST(relation.relation_kind AS BLOB))
                    + length(CAST(relation.resolution_status AS BLOB))
                    + coalesce(length(relation.target_entity_key), 0)
                    + coalesce(length(CAST(relation.reference_text AS BLOB)), 0)
                    + CASE WHEN relation.candidate_count IS NULL THEN 0 ELSE 8 END
                    + length(CAST(relation.confidence AS BLOB))
                    + length(CAST(relation.completeness AS BLOB))
               FROM requested
               JOIN graph_entities AS entity INDEXED BY idx_graph_entities_path
                 ON entity.repository_path = requested.path
               JOIN selected_project
                 ON selected_project.project_instance_id = entity.project_instance_id
               JOIN graph_relations AS relation INDEXED BY idx_graph_relations_source_kind
                 ON relation.source_entity_key = entity.entity_key
                AND relation.project_instance_id = selected_project.project_instance_id
             UNION ALL
             SELECT 80 + length(CAST(relation.canonical_identity AS BLOB))
                    + length(CAST(relation.relation_scope AS BLOB))
                    + length(CAST(relation.relation_kind AS BLOB))
                    + length(CAST(relation.resolution_status AS BLOB))
                    + coalesce(length(relation.target_entity_key), 0)
                    + coalesce(length(CAST(relation.reference_text AS BLOB)), 0)
                    + CASE WHEN relation.candidate_count IS NULL THEN 0 ELSE 8 END
                    + length(CAST(relation.confidence AS BLOB))
                    + length(CAST(relation.completeness AS BLOB))
               FROM requested
               JOIN graph_entities AS entity INDEXED BY idx_graph_entities_manifest_path
                 ON entity.manifest_path = requested.path
               JOIN selected_project
                 ON selected_project.project_instance_id = entity.project_instance_id
               JOIN graph_relations AS relation INDEXED BY idx_graph_relations_source_kind
                 ON relation.source_entity_key = entity.entity_key
                AND relation.project_instance_id = selected_project.project_instance_id
             UNION ALL
             SELECT 72 + length(CAST(occurrence.file_path AS BLOB))
               FROM requested
               JOIN graph_relation_occurrences AS occurrence
                    INDEXED BY idx_graph_occurrences_file_span
                 ON occurrence.file_path = requested.path
             UNION ALL
             SELECT 48 + length(CAST(coverage.scope_kind AS BLOB))
                    + coalesce(length(CAST(coverage.scope_path AS BLOB)), 0)
                    + coalesce(length(CAST(coverage.relation_scope AS BLOB)), 0)
                    + coalesce(length(CAST(coverage.relation_kind AS BLOB)), 0)
                    + length(CAST(coverage.state AS BLOB))
                    + coalesce(length(CAST(coverage.reason AS BLOB)), 0)
                    + coalesce(length(CAST(coverage.reached_limit AS BLOB)), 0)
               FROM requested
               JOIN graph_coverage AS coverage INDEXED BY idx_graph_coverage_path
                 ON coverage.scope_path = requested.path
               JOIN selected_project
                 ON selected_project.project_instance_id = coverage.project_instance_id
             UNION ALL
             SELECT 80 + length(CAST(export.owner_path AS BLOB))
                    + length(CAST(export.resolution_domain AS BLOB))
               FROM requested
               JOIN graph_entity_exports AS export INDEXED BY idx_graph_entity_exports_owner
                 ON export.owner_path = requested.path
               JOIN selected_project
                 ON selected_project.project_instance_id = export.project_instance_id
             UNION ALL
             SELECT length(witness.project_instance_id)
                    + length(CAST(witness.resolution_domain AS BLOB))
                    + length(witness.key_digest)
                    + length(CAST(witness.canonical_identity AS BLOB))
               FROM requested
               JOIN graph_entity_exports AS export INDEXED BY idx_graph_entity_exports_owner
                 ON export.owner_path = requested.path
               JOIN selected_project
                 ON selected_project.project_instance_id = export.project_instance_id
               LEFT JOIN graph_resolution_keys AS witness
                 ON witness.project_instance_id = export.project_instance_id
                AND witness.resolution_domain = export.resolution_domain
                AND witness.key_digest = export.key_digest
             UNION ALL
             SELECT 80 + length(CAST(dependency.owner_path AS BLOB))
                    + length(CAST(dependency.resolution_domain AS BLOB))
               FROM requested
               JOIN graph_relation_dependencies AS dependency
                    INDEXED BY idx_graph_relation_dependencies_owner
                 ON dependency.owner_path = requested.path
               JOIN selected_project
                 ON selected_project.project_instance_id = dependency.project_instance_id
             UNION ALL
             SELECT length(witness.project_instance_id)
                    + length(CAST(witness.resolution_domain AS BLOB))
                    + length(witness.key_digest)
                    + length(CAST(witness.canonical_identity AS BLOB))
               FROM requested
               JOIN graph_relation_dependencies AS dependency
                    INDEXED BY idx_graph_relation_dependencies_owner
                 ON dependency.owner_path = requested.path
               JOIN selected_project
                 ON selected_project.project_instance_id = dependency.project_instance_id
               LEFT JOIN graph_resolution_keys AS witness
                 ON witness.project_instance_id = dependency.project_instance_id
                AND witness.resolution_domain = dependency.resolution_domain
                AND witness.key_digest = dependency.key_digest
           )
          LIMIT ?"
    )
}

/// Build an anonymous `VALUES` clause with fixed columns per row.
fn resolution_values_clause(rows: usize, columns: usize) -> String {
    let row = format!("({})", vec!["?"; columns].join(","));
    vec![row; rows].join(",")
}

/// Bind one canonical-key batch in the same order as its `VALUES` rows.
fn resolution_key_values(keys: &[CanonicalResolutionKey], witness: bool) -> Vec<Value> {
    let columns = if witness { 4 } else { 3 };
    let mut values = Vec::with_capacity(keys.len() * columns);
    for key in keys {
        values.push(Value::Blob(key.project().as_bytes().to_vec()));
        values.push(Value::Text(key.domain().as_str().to_string()));
        values.push(Value::Blob(key.digest_bytes().to_vec()));
        if witness {
            values.push(Value::Text(key.canonical_identity().to_string()));
        }
    }
    values
}

/// Validate graph-owner bindings before acquiring the savepoint.
fn validate_resolution_key_batch(
    project: ProjectInstanceId,
    entity_exports: &[EntityResolutionKey],
    relation_dependencies: &[RelationDependencyKey],
) -> DbResult<()> {
    let foreign = entity_exports.iter().find_map(|binding| {
        (binding.entity().project() != project || binding.key().project() != project)
            .then(|| binding.key().project())
    });
    let foreign = foreign.or_else(|| {
        relation_dependencies.iter().find_map(|binding| {
            (binding.relation().project() != project || binding.key().project() != project)
                .then(|| binding.key().project())
        })
    });
    if let Some(found) = foreign {
        return Err(DbError::GraphProjectIdentityMismatch {
            expected: project.to_string(),
            found: found.to_string(),
        });
    }
    Ok(())
}

/// Insert a validated resolution-key projection through prepared owner statements.
fn insert_resolution_key_batch(
    connection: &Connection,
    project: ProjectInstanceId,
    entity_exports: &[EntityResolutionKey],
    relation_dependencies: &[RelationDependencyKey],
) -> DbResult<()> {
    let mut insert_key = connection.prepare_cached(
        "INSERT INTO graph_resolution_keys(
             project_instance_id, resolution_domain, key_digest, canonical_identity
         ) VALUES(?1, ?2, ?3, ?4)
         ON CONFLICT(project_instance_id, resolution_domain, key_digest)
         DO UPDATE SET canonical_identity = excluded.canonical_identity
         WHERE graph_resolution_keys.canonical_identity = excluded.canonical_identity",
    )?;
    let mut insert_export = connection.prepare_cached(
        "INSERT INTO graph_entity_exports(
             project_instance_id, entity_key, owner_path, resolution_domain, key_digest
         )
         SELECT entity.project_instance_id, entity.entity_key,
                CASE entity.entity_kind
                    WHEN 'file' THEN entity.repository_path
                    WHEN 'symbol' THEN entity.repository_path
                    WHEN 'package' THEN entity.manifest_path
                END,
                ?3, ?4
           FROM graph_entities AS entity
          WHERE entity.project_instance_id = ?1 AND entity.entity_key = ?2
            AND entity.entity_kind IN ('file', 'symbol', 'package')
         ON CONFLICT(project_instance_id, entity_key, resolution_domain, key_digest)
         DO UPDATE SET owner_path = excluded.owner_path",
    )?;
    let mut insert_dependency = connection.prepare_cached(
        "INSERT INTO graph_relation_dependencies(
             project_instance_id, relation_key, owner_path, resolution_domain, key_digest
         )
         SELECT relation.project_instance_id, relation.relation_key,
                CASE source.entity_kind
                    WHEN 'file' THEN source.repository_path
                    WHEN 'symbol' THEN source.repository_path
                    WHEN 'package' THEN source.manifest_path
                END,
                ?3, ?4
           FROM graph_relations AS relation
           JOIN graph_entities AS source
             ON source.project_instance_id = relation.project_instance_id
            AND source.entity_key = relation.source_entity_key
          WHERE relation.project_instance_id = ?1 AND relation.relation_key = ?2
            AND source.entity_kind IN ('file', 'symbol', 'package')
         ON CONFLICT(project_instance_id, relation_key, resolution_domain, key_digest)
         DO UPDATE SET owner_path = excluded.owner_path",
    )?;

    for binding in entity_exports {
        insert_resolution_key(&mut insert_key, binding.key())?;
        let key = binding.key();
        let inserted = insert_export.execute(params![
            &project.as_bytes()[..],
            &binding.entity().digest_bytes()?[..],
            key.domain().as_str(),
            &key.digest_bytes()[..],
        ])?;
        if inserted != 1 {
            return Err(DbError::GraphRowShape {
                table: "graph_entity_exports",
                reason: "export keys require a local file, symbol, or package owner",
            });
        }
    }
    for binding in relation_dependencies {
        insert_resolution_key(&mut insert_key, binding.key())?;
        let key = binding.key();
        let inserted = insert_dependency.execute(params![
            &project.as_bytes()[..],
            &binding.relation().digest_bytes()?[..],
            key.domain().as_str(),
            &key.digest_bytes()[..],
        ])?;
        if inserted != 1 {
            return Err(DbError::GraphRowShape {
                table: "graph_relation_dependencies",
                reason: "dependency keys require a local file, symbol, or package source",
            });
        }
    }
    Ok(())
}

/// Insert one key witness, rejecting equal digests with different material.
fn insert_resolution_key(
    statement: &mut rusqlite::CachedStatement<'_>,
    key: &CanonicalResolutionKey,
) -> DbResult<()> {
    let inserted = statement.execute(params![
        &key.project().as_bytes()[..],
        key.domain().as_str(),
        &key.digest_bytes()[..],
        key.canonical_identity(),
    ])?;
    if inserted == 0 {
        return Err(DbError::ResolutionKeyCollision {
            domain: key.domain().as_str(),
            digest: key.digest_bytes(),
        });
    }
    Ok(())
}

/// Delete only touched witness rows left without either owner family.
fn remove_touched_orphan_resolution_keys(
    connection: &Connection,
    keys: &BTreeSet<CanonicalResolutionKey>,
) -> DbResult<()> {
    let keys = keys.iter().cloned().collect::<Vec<_>>();
    for chunk in keys.chunks(RESOLUTION_KEYS_PER_QUERY) {
        if chunk.is_empty() {
            continue;
        }
        let values_clause = resolution_values_clause(chunk.len(), 3);
        let sql = format!(
            "WITH touched(project_instance_id, resolution_domain, key_digest)
                  AS (VALUES {values_clause})
             DELETE FROM graph_resolution_keys
              WHERE rowid IN (
                    SELECT stored.rowid
                      FROM graph_resolution_keys AS stored
                      JOIN touched
                        ON touched.project_instance_id = stored.project_instance_id
                       AND touched.resolution_domain = stored.resolution_domain
                       AND touched.key_digest = stored.key_digest
                     WHERE NOT EXISTS (
                               SELECT 1 FROM graph_entity_exports AS export
                                WHERE export.project_instance_id = stored.project_instance_id
                                  AND export.resolution_domain = stored.resolution_domain
                                  AND export.key_digest = stored.key_digest
                           )
                       AND NOT EXISTS (
                               SELECT 1 FROM graph_relation_dependencies AS dependency
                                WHERE dependency.project_instance_id = stored.project_instance_id
                                  AND dependency.resolution_domain = stored.resolution_domain
                                  AND dependency.key_digest = stored.key_digest
                           )
              )"
        );
        let values = resolution_key_values(chunk, false);
        connection.execute(&sql, params_from_iter(values.iter()))?;
    }
    Ok(())
}

/// Collect external entities whose relation to an affected local entity may vanish.
fn affected_external_candidates(
    connection: &Connection,
    affected_paths: &[RepositoryNodePath],
) -> DbResult<HashSet<[u8; 32]>> {
    let mut by_repository_path = connection.prepare_cached(
        "SELECT entity_key FROM graph_entities
          WHERE repository_path = ?1
             OR (repository_path >= ?2 AND repository_path < ?3)",
    )?;
    let mut by_manifest_path = connection.prepare_cached(
        "SELECT entity_key FROM graph_entities
          WHERE manifest_path = ?1 OR (manifest_path >= ?2 AND manifest_path < ?3)",
    )?;
    let mut local_keys = HashSet::new();
    for path in affected_paths {
        let path = path.as_str();
        let (descendant_start, descendant_end) = repository_descendant_bounds(path);
        for statement in [&mut by_repository_path, &mut by_manifest_path] {
            let mut rows = statement.query(params![path, descendant_start, descendant_end])?;
            while let Some(row) = rows.next()? {
                local_keys.insert(fixed_bytes::<32>(
                    "graph_entities.entity_key",
                    row.get::<_, Vec<u8>>(0)?,
                )?);
            }
        }
    }

    let mut outgoing = connection.prepare_cached(
        "SELECT relation.target_entity_key
           FROM graph_relations AS relation INDEXED BY idx_graph_relations_source_kind
           JOIN graph_entities AS external
             ON external.entity_key = relation.target_entity_key
          WHERE relation.source_entity_key = ?1 AND external.entity_kind = 'external'",
    )?;
    let mut incoming = connection.prepare_cached(
        "SELECT relation.source_entity_key
           FROM graph_relations AS relation INDEXED BY idx_graph_relations_target_kind
           JOIN graph_entities AS external
             ON external.entity_key = relation.source_entity_key
          WHERE relation.target_entity_key = ?1 AND external.entity_kind = 'external'",
    )?;
    let mut candidates = HashSet::new();
    for local_key in local_keys {
        for statement in [&mut outgoing, &mut incoming] {
            let mut rows = statement.query([&local_key[..]])?;
            while let Some(row) = rows.next()? {
                candidates.insert(fixed_bytes::<32>(
                    "graph_entities.entity_key",
                    row.get::<_, Vec<u8>>(0)?,
                )?);
            }
        }
    }
    Ok(candidates)
}

/// Delete one affected local closure through statements prepared once per batch.
fn invalidate_repository_graph_paths(
    connection: &Connection,
    affected_paths: &[RepositoryNodePath],
    orphan_candidates: &mut HashSet<[u8; 32]>,
) -> DbResult<()> {
    let mut affected_relations = HashSet::new();
    let mut relation_occurrences = connection.prepare_cached(
        "SELECT relation_key FROM graph_relation_occurrences
          WHERE file_path = ?1 OR (file_path >= ?2 AND file_path < ?3)",
    )?;
    let mut occurrences = connection.prepare_cached(
        "DELETE FROM graph_relation_occurrences
          WHERE file_path = ?1 OR (file_path >= ?2 AND file_path < ?3)",
    )?;
    let mut coverage = connection.prepare_cached(
        "DELETE FROM graph_coverage
          INDEXED BY idx_graph_coverage_path
          WHERE scope_kind = 'path'
            AND (scope_path = ?1 OR (scope_path >= ?2 AND scope_path < ?3))",
    )?;
    let mut entities_by_path = connection.prepare_cached(
        "DELETE FROM graph_entities
          WHERE repository_path = ?1
             OR (repository_path >= ?2 AND repository_path < ?3)",
    )?;
    let mut entities_by_manifest = connection.prepare_cached(
        "DELETE FROM graph_entities
          WHERE manifest_path = ?1 OR (manifest_path >= ?2 AND manifest_path < ?3)",
    )?;
    for path in affected_paths {
        let path = path.as_str();
        let (descendant_start, descendant_end) = repository_descendant_bounds(path);
        let mut rows =
            relation_occurrences.query(params![path, descendant_start, descendant_end])?;
        while let Some(row) = rows.next()? {
            affected_relations.insert(fixed_bytes::<32>(
                "graph_relation_occurrences.relation_key",
                row.get::<_, Vec<u8>>(0)?,
            )?);
        }
        occurrences.execute(params![path, descendant_start, descendant_end])?;
        coverage.execute(params![path, descendant_start, descendant_end])?;
        entities_by_path.execute(params![path, descendant_start, descendant_end])?;
        entities_by_manifest.execute(params![path, descendant_start, descendant_end])?;
    }
    collect_external_relation_endpoints(connection, &affected_relations, orphan_candidates)?;
    let mut relation = connection.prepare_cached(
        "DELETE FROM graph_relations
          WHERE relation_key = ?1
            AND NOT EXISTS (
                SELECT 1 FROM graph_relation_occurrences
                 WHERE relation_key = ?1
            )",
    )?;
    for relation_key in affected_relations {
        relation.execute([&relation_key[..]])?;
    }
    Ok(())
}

/// Retain external endpoints whose occurrence-backed relation may be removed.
fn collect_external_relation_endpoints(
    connection: &Connection,
    relation_keys: &HashSet<[u8; 32]>,
    candidates: &mut HashSet<[u8; 32]>,
) -> DbResult<()> {
    let mut endpoints = connection.prepare_cached(
        "SELECT source_entity_key, target_entity_key
           FROM graph_relations
          WHERE relation_key = ?1",
    )?;
    let mut is_external = connection.prepare_cached(
        "SELECT EXISTS(
            SELECT 1 FROM graph_entities
             WHERE entity_key = ?1 AND entity_kind = 'external'
        )",
    )?;
    for relation_key in relation_keys {
        let endpoints = endpoints
            .query_row([&relation_key[..]], |row| {
                Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Option<Vec<u8>>>(1)?))
            })
            .optional()?;
        let Some((source, target)) = endpoints else {
            continue;
        };
        for endpoint in [Some(source), target].into_iter().flatten() {
            let endpoint = fixed_bytes::<32>("graph_relations endpoint", endpoint)?;
            if is_external.query_row([&endpoint[..]], |row| row.get::<_, bool>(0))? {
                candidates.insert(endpoint);
            }
        }
    }
    Ok(())
}

/// Remove only candidate external entities that no surviving relation references.
fn remove_orphan_external_candidates(
    connection: &Connection,
    candidates: &HashSet<[u8; 32]>,
) -> DbResult<()> {
    let mut statement = connection.prepare_cached(
        "DELETE FROM graph_entities
          WHERE entity_key = ?1 AND entity_kind = 'external'
            AND NOT EXISTS (
                SELECT 1 FROM graph_relations INDEXED BY idx_graph_relations_source_kind
                 WHERE source_entity_key = ?1
            )
            AND NOT EXISTS (
                SELECT 1 FROM graph_relations INDEXED BY idx_graph_relations_target_kind
                 WHERE target_entity_key = ?1
            )",
    )?;
    for candidate in candidates {
        statement.execute([&candidate[..]])?;
    }
    Ok(())
}

/// Return case-preserving indexed bounds for every slash-delimited descendant.
fn repository_descendant_bounds(path: &str) -> (String, String) {
    (format!("{path}/"), format!("{path}0"))
}

/// Validate ownership and generation before any graph mutation occurs.
fn validate_graph_batch(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    entities: &[GraphEntity],
    relations: &[LogicalRelation],
    occurrences: &[RelationOccurrence],
    coverage: &[CoverageRecord],
) -> DbResult<()> {
    if entities
        .iter()
        .any(|entity| entity.key().project() != project)
        || relations
            .iter()
            .any(|relation| relation.key().project() != project)
        || occurrences
            .iter()
            .any(|occurrence| occurrence.relation().project() != project)
    {
        return Err(DbError::GraphProjectIdentityMismatch {
            expected: project.to_string(),
            found: "record from another project".to_string(),
        });
    }
    if entities
        .iter()
        .any(|entity| entity.generation() != generation)
        || relations
            .iter()
            .any(|relation| relation.generation() != generation)
        || occurrences
            .iter()
            .any(|occurrence| occurrence.generation() != generation)
        || coverage
            .iter()
            .any(|record| record.generation() != generation)
    {
        return Err(
            projectatlas_core::graph::GraphContractError::GenerationMismatch {
                context: "repository graph publication batch",
            }
            .into(),
        );
    }
    Ok(())
}

/// Insert one validated graph batch through cached normalized statements.
fn insert_graph_batch(
    connection: &Connection,
    project: ProjectInstanceId,
    entities: &[GraphEntity],
    relations: &[LogicalRelation],
    occurrences: &[RelationOccurrence],
    coverage: &[CoverageRecord],
) -> DbResult<()> {
    insert_entities(connection, project, entities)?;
    insert_relations(connection, project, relations)?;
    insert_occurrences(connection, occurrences)?;
    insert_coverage(connection, project, coverage)
}

/// Insert typed entities while refusing compact-key collisions.
fn insert_entities(
    connection: &Connection,
    project: ProjectInstanceId,
    entities: &[GraphEntity],
) -> DbResult<()> {
    let mut insert = connection.prepare_cached(
        "INSERT INTO graph_entities(
            entity_key, project_instance_id, canonical_identity, entity_kind,
            repository_path, package_manager, package_name, manifest_path,
            symbol_name, symbol_kind, symbol_parent, symbol_signature,
            external_system, external_identity
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT(entity_key) DO NOTHING",
    )?;
    let mut existing = connection.prepare_cached(
        "SELECT project_instance_id, canonical_identity
           FROM graph_entities WHERE entity_key = ?1",
    )?;
    for entity in entities {
        let columns = entity_columns(entity.selector());
        let key = entity.key().digest_bytes()?;
        insert.execute(params![
            &key[..],
            &project.as_bytes()[..],
            entity.key().canonical_identity(),
            columns.kind,
            columns.repository_path,
            columns.package_manager,
            columns.package_name,
            columns.manifest_path,
            columns.symbol_name,
            columns.symbol_kind,
            columns.symbol_parent,
            columns.symbol_signature,
            columns.external_system,
            columns.external_identity,
        ])?;
        let (stored_project, stored_canonical): (Vec<u8>, String) =
            existing.query_row([&key[..]], |row| Ok((row.get(0)?, row.get(1)?)))?;
        if fixed_bytes::<16>("graph_entities.project_instance_id", stored_project)?
            != project.as_bytes()
            || stored_canonical != entity.key().canonical_identity()
        {
            return Err(
                projectatlas_core::graph::GraphContractError::StableKeyCollision {
                    digest: entity.key().digest().to_string(),
                }
                .into(),
            );
        }
    }
    Ok(())
}

/// Insert typed logical relations while allowing trust metadata to refresh.
fn insert_relations(
    connection: &Connection,
    project: ProjectInstanceId,
    relations: &[LogicalRelation],
) -> DbResult<()> {
    let mut statement = connection.prepare_cached(
        "INSERT INTO graph_relations(
            relation_key, project_instance_id, canonical_identity, source_entity_key,
            relation_scope, relation_kind, resolution_status, target_entity_key,
            reference_text, candidate_count, confidence, completeness
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(relation_key) DO UPDATE SET
            confidence = excluded.confidence,
            completeness = excluded.completeness
         WHERE graph_relations.project_instance_id = excluded.project_instance_id
           AND graph_relations.canonical_identity = excluded.canonical_identity
           AND graph_relations.source_entity_key = excluded.source_entity_key
           AND graph_relations.relation_scope = excluded.relation_scope
           AND graph_relations.relation_kind = excluded.relation_kind
           AND graph_relations.resolution_status = excluded.resolution_status
           AND graph_relations.target_entity_key IS excluded.target_entity_key
           AND graph_relations.reference_text IS excluded.reference_text
           AND graph_relations.candidate_count IS excluded.candidate_count",
    )?;
    for relation in relations {
        let (scope, kind) = relation_parts(relation.kind());
        let resolution = resolution_columns(relation.resolution())?;
        let key = relation.key().digest_bytes()?;
        let source = relation.source().digest_bytes()?;
        let changed = statement.execute(params![
            &key[..],
            &project.as_bytes()[..],
            relation.key().canonical_identity(),
            &source[..],
            scope,
            kind,
            resolution.status,
            resolution.target.as_ref().map(|target| &target[..]),
            resolution.reference,
            resolution.candidate_count,
            confidence_name(relation.confidence()),
            completeness_name(relation.completeness()),
        ])?;
        if changed == 0 {
            return Err(
                projectatlas_core::graph::GraphContractError::StableKeyCollision {
                    digest: relation.key().digest().to_string(),
                }
                .into(),
            );
        }
    }
    Ok(())
}

/// Insert every exact source occurrence without duplicating logical evidence.
fn insert_occurrences(connection: &Connection, occurrences: &[RelationOccurrence]) -> DbResult<()> {
    let mut statement = connection.prepare_cached(
        "INSERT INTO graph_relation_occurrences(
            relation_key, file_path, start_line, start_column, end_line, end_column
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(relation_key, file_path, start_line, start_column, end_line, end_column)
         DO NOTHING",
    )?;
    for occurrence in occurrences {
        let key = occurrence.relation().digest_bytes()?;
        let span = occurrence.span();
        statement.execute(params![
            &key[..],
            occurrence.file().as_str(),
            i64::from(span.start_line()),
            i64::from(span.start_column()),
            i64::from(span.end_line()),
            i64::from(span.end_column()),
        ])?;
    }
    Ok(())
}

/// Replace coverage rows by their normalized identity.
fn insert_coverage(
    connection: &Connection,
    project: ProjectInstanceId,
    coverage: &[CoverageRecord],
) -> DbResult<()> {
    let mut remove = connection.prepare_cached(
        "DELETE FROM graph_coverage
          WHERE project_instance_id = ?1 AND scope_kind = ?2 AND scope_path IS ?3
            AND relation_scope IS ?4 AND relation_kind IS ?5",
    )?;
    let mut insert = connection.prepare_cached(
        "INSERT INTO graph_coverage(
            project_instance_id, scope_kind, scope_path, relation_scope, relation_kind,
            state, total, covered, omitted, reason, reached_limit
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?;
    for record in coverage {
        let (scope_kind, scope_path) = coverage_scope_parts(record.scope());
        let (relation_scope, relation_kind) = record
            .relation()
            .map(relation_parts)
            .map_or((None, None), |(scope, kind)| (Some(scope), Some(kind)));
        let values = params![
            &project.as_bytes()[..],
            scope_kind,
            scope_path,
            relation_scope,
            relation_kind,
        ];
        remove.execute(values)?;
        insert.execute(params![
            &project.as_bytes()[..],
            scope_kind,
            scope_path,
            relation_scope,
            relation_kind,
            coverage_state_name(record.state()),
            sqlite_count("graph_coverage.total", record.total())?,
            sqlite_count("graph_coverage.covered", record.covered())?,
            sqlite_count("graph_coverage.omitted", record.omitted())?,
            record.reason().map(GraphIdentityText::as_str),
            record.reached_limit().map(limit_kind_name),
        ])?;
    }
    Ok(())
}

/// Borrowed normalized selector columns for one entity insert.
struct EntityColumns<'selector> {
    /// Normalized selector variant.
    kind: &'static str,
    /// Folder, file, or symbol repository path.
    repository_path: Option<&'selector str>,
    /// Package ecosystem.
    package_manager: Option<&'selector str>,
    /// Package name.
    package_name: Option<&'selector str>,
    /// Package manifest path.
    manifest_path: Option<&'selector str>,
    /// Declaration name.
    symbol_name: Option<&'selector str>,
    /// Declaration kind.
    symbol_kind: Option<&'static str>,
    /// Optional containing declaration.
    symbol_parent: Option<&'selector str>,
    /// Stable declaration signature.
    symbol_signature: Option<&'selector str>,
    /// External namespace.
    external_system: Option<&'selector str>,
    /// External identity.
    external_identity: Option<&'selector str>,
}

/// Map one typed selector to its normalized database columns.
fn entity_columns(selector: &EntitySelector) -> EntityColumns<'_> {
    match selector {
        EntitySelector::Project => EntityColumns {
            kind: "project",
            repository_path: None,
            package_manager: None,
            package_name: None,
            manifest_path: None,
            symbol_name: None,
            symbol_kind: None,
            symbol_parent: None,
            symbol_signature: None,
            external_system: None,
            external_identity: None,
        },
        EntitySelector::Folder { path } => EntityColumns {
            kind: "folder",
            repository_path: Some(path.as_str()),
            package_manager: None,
            package_name: None,
            manifest_path: None,
            symbol_name: None,
            symbol_kind: None,
            symbol_parent: None,
            symbol_signature: None,
            external_system: None,
            external_identity: None,
        },
        EntitySelector::File { path } => EntityColumns {
            kind: "file",
            repository_path: Some(path.as_str()),
            package_manager: None,
            package_name: None,
            manifest_path: None,
            symbol_name: None,
            symbol_kind: None,
            symbol_parent: None,
            symbol_signature: None,
            external_system: None,
            external_identity: None,
        },
        EntitySelector::Package { package } => EntityColumns {
            kind: "package",
            repository_path: None,
            package_manager: Some(package.manager.as_str()),
            package_name: Some(package.name.as_str()),
            manifest_path: Some(package.manifest.as_str()),
            symbol_name: None,
            symbol_kind: None,
            symbol_parent: None,
            symbol_signature: None,
            external_system: None,
            external_identity: None,
        },
        EntitySelector::Symbol { symbol } => EntityColumns {
            kind: "symbol",
            repository_path: Some(symbol.file.as_str()),
            package_manager: None,
            package_name: None,
            manifest_path: None,
            symbol_name: Some(symbol.name.as_str()),
            symbol_kind: Some(symbol_kind_name(symbol.kind)),
            symbol_parent: symbol.parent.as_ref().map(GraphIdentityText::as_str),
            symbol_signature: Some(symbol.signature.as_str()),
            external_system: None,
            external_identity: None,
        },
        EntitySelector::External { external } => EntityColumns {
            kind: "external",
            repository_path: None,
            package_manager: None,
            package_name: None,
            manifest_path: None,
            symbol_name: None,
            symbol_kind: None,
            symbol_parent: None,
            symbol_signature: None,
            external_system: Some(external.system.as_str()),
            external_identity: Some(external.identity.as_str()),
        },
    }
}

/// Borrowed normalized resolution columns for one relation insert.
struct ResolutionColumns<'resolution> {
    /// Normalized resolution state.
    status: &'static str,
    /// Optional resolved or external target.
    target: Option<[u8; 32]>,
    /// Optional unresolved reference text.
    reference: Option<&'resolution str>,
    /// Optional ambiguous candidate count.
    candidate_count: Option<i64>,
}

/// Map typed resolution state to normalized database columns.
fn resolution_columns(resolution: &RelationResolution) -> DbResult<ResolutionColumns<'_>> {
    match resolution {
        RelationResolution::Resolved { target, .. } => Ok(ResolutionColumns {
            status: "resolved",
            target: Some(target.digest_bytes()?),
            reference: None,
            candidate_count: None,
        }),
        RelationResolution::Ambiguous {
            reference,
            candidates,
        } => Ok(ResolutionColumns {
            status: "ambiguous",
            target: None,
            reference: Some(reference.as_str()),
            candidate_count: Some(i64::from(candidates.get())),
        }),
        RelationResolution::Unresolved { reference } => Ok(ResolutionColumns {
            status: "unresolved",
            target: None,
            reference: Some(reference.as_str()),
            candidate_count: None,
        }),
        RelationResolution::External { target, .. } => Ok(ResolutionColumns {
            status: "external",
            target: Some(target.digest_bytes()?),
            reference: None,
            candidate_count: None,
        }),
    }
}

/// Reconstruct one typed entity and validate persisted key witnesses.
fn entity_from_row(
    row: EntityRow,
    expected_project: ProjectInstanceId,
    generation: IndexGeneration,
) -> DbResult<GraphEntity> {
    let project = project_from_blob("graph_entities.project_instance_id", row.project.clone())?;
    require_project(expected_project, project)?;
    validate_entity_row_shape(&row)?;
    let selector = match row.kind.as_str() {
        "project" => EntitySelector::Project,
        "folder" => EntitySelector::Folder {
            path: RepositoryNodePath::new(Path::new(required_text(
                "graph_entities",
                "folder path is missing",
                row.repository_path.as_deref(),
            )?))?,
        },
        "file" => EntitySelector::File {
            path: RepositoryFilePath::new(Path::new(required_text(
                "graph_entities",
                "file path is missing",
                row.repository_path.as_deref(),
            )?))?,
        },
        "package" => EntitySelector::Package {
            package: PackageSelector {
                manager: GraphIdentityText::new(required_text(
                    "graph_entities",
                    "package manager is missing",
                    row.package_manager.as_deref(),
                )?)?,
                name: GraphIdentityText::new(required_text(
                    "graph_entities",
                    "package name is missing",
                    row.package_name.as_deref(),
                )?)?,
                manifest: RepositoryFilePath::new(Path::new(required_text(
                    "graph_entities",
                    "package manifest is missing",
                    row.manifest_path.as_deref(),
                )?))?,
            },
        },
        "symbol" => EntitySelector::Symbol {
            symbol: SymbolSelector {
                file: RepositoryFilePath::new(Path::new(required_text(
                    "graph_entities",
                    "symbol file is missing",
                    row.repository_path.as_deref(),
                )?))?,
                name: GraphIdentityText::new(required_text(
                    "graph_entities",
                    "symbol name is missing",
                    row.symbol_name.as_deref(),
                )?)?,
                kind: parse_symbol_kind(required_text(
                    "graph_entities",
                    "symbol kind is missing",
                    row.symbol_kind.as_deref(),
                )?)?,
                parent: row.symbol_parent.map(GraphIdentityText::new).transpose()?,
                signature: GraphIdentityText::new(required_text(
                    "graph_entities",
                    "symbol signature is missing",
                    row.symbol_signature.as_deref(),
                )?)?,
            },
        },
        "external" => EntitySelector::External {
            external: ExternalSelector {
                system: GraphIdentityText::new(required_text(
                    "graph_entities",
                    "external system is missing",
                    row.external_system.as_deref(),
                )?)?,
                identity: GraphIdentityText::new(required_text(
                    "graph_entities",
                    "external identity is missing",
                    row.external_identity.as_deref(),
                )?)?,
            },
        },
        value => {
            return Err(DbError::InvalidEnum {
                field: "graph_entities.entity_kind",
                value: value.to_string(),
            });
        }
    };
    let entity = GraphEntity::new(project, selector, generation)?;
    validate_entity_key(&entity, row.key, &row.canonical)?;
    Ok(entity)
}

/// Validate selector-column shape independently of physical schema checks.
fn validate_entity_row_shape(row: &EntityRow) -> DbResult<()> {
    let repository = row.repository_path.is_some();
    let package = (
        row.package_manager.is_some(),
        row.package_name.is_some(),
        row.manifest_path.is_some(),
    );
    let symbol = (
        row.symbol_name.is_some(),
        row.symbol_kind.is_some(),
        row.symbol_parent.is_some(),
        row.symbol_signature.is_some(),
    );
    let external = (
        row.external_system.is_some(),
        row.external_identity.is_some(),
    );
    let valid = match row.kind.as_str() {
        "project" => {
            !repository
                && package == (false, false, false)
                && symbol == (false, false, false, false)
                && external == (false, false)
        }
        "folder" | "file" => {
            repository
                && package == (false, false, false)
                && symbol == (false, false, false, false)
                && external == (false, false)
        }
        "package" => {
            !repository
                && package == (true, true, true)
                && symbol == (false, false, false, false)
                && external == (false, false)
        }
        "symbol" => {
            repository
                && package == (false, false, false)
                && symbol.0
                && symbol.1
                && symbol.3
                && external == (false, false)
        }
        "external" => {
            !repository
                && package == (false, false, false)
                && symbol == (false, false, false, false)
                && external == (true, true)
        }
        _ => true,
    };
    if !valid {
        return Err(DbError::GraphRowShape {
            table: "graph_entities",
            reason: "selector columns contradict entity kind",
        });
    }
    Ok(())
}

/// Build one direction-owned batched adjacency statement.
fn adjacency_relation_sql(
    frontier_count: usize,
    direction: RepositoryGraphDirection,
    continuation_index: Option<usize>,
    relation_filter: bool,
) -> String {
    let (key_column, index_name) = match direction {
        RepositoryGraphDirection::Outbound => {
            ("source_entity_key", "idx_graph_relations_source_kind")
        }
        RepositoryGraphDirection::Inbound => {
            ("target_entity_key", "idx_graph_relations_target_kind")
        }
    };
    let request = "request(project_instance_id) AS (VALUES (?))";
    let relation_filter = if relation_filter {
        "AND relation.relation_scope = ? AND relation.relation_kind = ?"
    } else {
        ""
    };
    let branches = (continuation_index.unwrap_or(0)..frontier_count)
        .map(|frontier_index| {
            let continuation = if continuation_index == Some(frontier_index) {
                "AND (relation.relation_scope, relation.relation_kind,
                      relation.canonical_identity, relation.relation_key) >
                     (?, ?, ?, ?)"
                    .to_string()
            } else {
                String::new()
            };
            format!(
                "SELECT * FROM (
                     SELECT {frontier_index} AS frontier_index,
                            relation.relation_key, relation.project_instance_id,
                            relation.canonical_identity, relation.source_entity_key,
                            relation.relation_scope, relation.relation_kind,
                            relation.resolution_status, relation.target_entity_key,
                            relation.reference_text, relation.candidate_count,
                            relation.confidence, relation.completeness
                       FROM graph_relations AS relation INDEXED BY {index_name}
                       CROSS JOIN request
                      WHERE relation.{key_column} = ?
                        AND relation.project_instance_id = request.project_instance_id
                        {relation_filter}
                        {continuation}
                      ORDER BY relation.relation_scope, relation.relation_kind,
                               relation.canonical_identity, relation.relation_key
                      LIMIT ?
                 )"
            )
        })
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    format!(
        "WITH {request}
         {branches}
         ORDER BY frontier_index, relation_scope, relation_kind,
                  canonical_identity, relation_key
         LIMIT ?"
    )
}

/// Build one anonymous fixed-column `VALUES` clause.
fn graph_values_clause(rows: usize, columns: usize) -> String {
    let row = format!("({})", vec!["?"; columns].join(", "));
    vec![row; rows].join(", ")
}

/// Build one indexed stable-key entity hydration statement.
fn graph_entity_hydration_sql(entity_count: usize) -> String {
    format!(
        "WITH requested(entity_key) AS (VALUES {})
         SELECT entity.entity_key, entity.project_instance_id,
                entity.canonical_identity, entity.entity_kind,
                entity.repository_path, entity.package_manager,
                entity.package_name, entity.manifest_path,
                entity.symbol_name, entity.symbol_kind,
                entity.symbol_parent, entity.symbol_signature,
                entity.external_system, entity.external_identity
           FROM requested
           JOIN graph_entities AS entity
             ON entity.entity_key = requested.entity_key
          WHERE entity.project_instance_id = ?",
        graph_values_clause(entity_count, 1),
    )
}

/// Build one project-scoped stable-key relation hydration statement.
fn graph_relation_hydration_sql(relation_count: usize) -> String {
    format!(
        "WITH requested(relation_key) AS (VALUES {})
         SELECT relation.relation_key, relation.project_instance_id,
                relation.canonical_identity, relation.source_entity_key,
                relation.relation_scope, relation.relation_kind,
                relation.resolution_status, relation.target_entity_key,
                relation.reference_text, relation.candidate_count,
                relation.confidence, relation.completeness
           FROM requested
           JOIN graph_relations AS relation INDEXED BY idx_graph_relations_project_key
             ON relation.relation_key = requested.relation_key
            AND relation.project_instance_id = ?",
        graph_values_clause(relation_count, 1),
    )
}

/// Build direction-independent per-relation occurrence branches.
fn occurrence_pages_sql(relation_count: usize) -> String {
    let branches = (0..relation_count)
        .map(|index| {
            format!(
                "SELECT {index} AS relation_index, relation_key, file_path,
                        start_line, start_column, end_line, end_column
                   FROM (
                        SELECT relation_key, file_path, start_line, start_column,
                               end_line, end_column
                          FROM graph_relation_occurrences
                         WHERE relation_key = ?
                         ORDER BY file_path, start_line, start_column,
                                  end_line, end_column
                         LIMIT ?
                   ) AS occurrence_page_{index}"
            )
        })
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    format!(
        "{branches}
         ORDER BY relation_index, file_path, start_line, start_column,
                  end_line, end_column"
    )
}

/// Build the set-oriented exact-path coverage hydration statement.
fn path_coverage_sql(path_count: usize) -> String {
    let path_bindings = vec!["?"; path_count].join(", ");
    format!(
        "SELECT project_instance_id, scope_kind, scope_path, relation_scope,
                relation_kind, state, total, covered, omitted, reason, reached_limit,
                NULL, NULL
           FROM graph_coverage
          WHERE project_instance_id = ?
            AND scope_kind = ?
            AND scope_path IN ({path_bindings})
          ORDER BY scope_path, relation_scope, relation_kind, state, id
          LIMIT ?"
    )
}

/// Load every unique relation endpoint through bounded set-oriented joins.
fn load_relation_entities(
    store: &AtlasStore,
    rows: &[RelationRow],
    project: ProjectInstanceId,
    generation: IndexGeneration,
    control: Option<&IndexWorkControl>,
) -> DbResult<HashMap<[u8; 32], GraphEntity>> {
    let references = rows.iter().collect::<Vec<_>>();
    load_relation_entity_references(store, &references, project, generation, control)
}

/// Load relation endpoint references shared by ordinary and adjacency pages.
fn load_relation_entity_references(
    store: &AtlasStore,
    rows: &[&RelationRow],
    project: ProjectInstanceId,
    generation: IndexGeneration,
    control: Option<&IndexWorkControl>,
) -> DbResult<HashMap<[u8; 32], GraphEntity>> {
    load_relation_entity_references_metered(store, rows, project, generation, control, None)
}

/// Load relation endpoint references with optional exact batch accounting.
fn load_relation_entity_references_metered(
    store: &AtlasStore,
    rows: &[&RelationRow],
    project: ProjectInstanceId,
    generation: IndexGeneration,
    control: Option<&IndexWorkControl>,
    meter: Option<&mut RepositoryGraphReadMeter>,
) -> DbResult<HashMap<[u8; 32], GraphEntity>> {
    let mut digests = BTreeSet::new();
    for row in rows {
        let row_project =
            project_from_blob("graph_relations.project_instance_id", row.project.clone())?;
        require_project(project, row_project)?;
        digests.insert(fixed_bytes::<32>(
            "graph_relations.source_entity_key",
            row.source.clone(),
        )?);
        if let Some(target) = &row.target {
            digests.insert(fixed_bytes::<32>(
                "graph_relations.target_entity_key",
                target.clone(),
            )?);
        }
    }
    load_graph_entities_by_digest_metered(
        store,
        &digests.into_iter().collect::<Vec<_>>(),
        project,
        generation,
        control,
        meter,
    )
}

/// Hydrate one unique stable-key set with optional exact batch accounting.
fn load_graph_entities_by_digest_metered(
    store: &AtlasStore,
    digests: &[[u8; 32]],
    project: ProjectInstanceId,
    generation: IndexGeneration,
    control: Option<&IndexWorkControl>,
    mut meter: Option<&mut RepositoryGraphReadMeter>,
) -> DbResult<HashMap<[u8; 32], GraphEntity>> {
    let mut entities = HashMap::with_capacity(digests.len());
    for chunk in digests.chunks(GRAPH_ENTITY_HYDRATION_CHUNK) {
        if let Some(control) = control {
            control.check(IndexWorkStage::RepositoryTraversal)?;
        }
        let sql = graph_entity_hydration_sql(chunk.len());
        let mut bindings = chunk
            .iter()
            .map(|digest| Value::Blob(digest.to_vec()))
            .collect::<Vec<_>>();
        bindings.push(Value::Blob(project.as_bytes().to_vec()));
        let raw = with_sqlite_read_progress(
            &store.connection,
            control,
            IndexWorkStage::RepositoryTraversal,
            || {
                let mut statement = store.connection.prepare(&sql)?;
                let rows = statement.query(params_from_iter(bindings.iter()))?;
                if let Some(meter) = meter.as_deref_mut() {
                    collect_entity_rows_metered(rows, meter)
                } else {
                    collect_entity_rows(rows)
                }
            },
        )?;
        for row in raw {
            let entity = entity_from_row(row, project, generation)?;
            let digest = entity.key().digest_bytes()?;
            if entities.contains_key(&digest) {
                return Err(DbError::GraphRowShape {
                    table: "graph_entities",
                    reason: "batched entity hydration returned a duplicate key",
                });
            }
            if let Some(meter) = meter.as_deref_mut() {
                meter.record_entity(&entity)?;
            }
            entities.insert(digest, entity);
        }
    }
    Ok(entities)
}

/// Construct the compatibility envelope used by legacy bounded read wrappers.
fn maximum_repository_graph_read_budget() -> DbResult<RepositoryGraphReadBudget> {
    Ok(RepositoryGraphReadBudget::new(
        RepositoryGraphReadBudget::MAX_REQUESTED_ROWS,
        RepositoryGraphReadBudget::MAX_RETURNED_ROWS,
        RepositoryGraphReadBudget::MAX_DECODED_BYTES,
        RepositoryGraphReadBudget::MAX_HYDRATED_ENTITIES,
        RepositoryGraphReadBudget::MAX_HYDRATED_PATHS,
    )?)
}

/// Validate one bounded unique exact-path coverage request.
fn validate_path_coverage_request(paths: &[RepositoryNodePath]) -> DbResult<()> {
    if paths.len() > MAX_REPOSITORY_GRAPH_FRONTIER {
        return Err(GraphContractError::InvalidLimits {
            reason: "graph coverage path set exceeds the product ceiling",
        }
        .into());
    }
    if paths
        .iter()
        .map(RepositoryNodePath::as_str)
        .collect::<BTreeSet<_>>()
        .len()
        != paths.len()
    {
        return Err(GraphContractError::InvalidLimits {
            reason: "graph coverage paths must be unique",
        }
        .into());
    }
    Ok(())
}

/// Validate the bounded unique stable-key set shared by cursor hydration calls.
fn validate_graph_hydration_request(digests: &[[u8; 32]]) -> DbResult<()> {
    if digests.len() > MAX_REPOSITORY_GRAPH_FRONTIER {
        return Err(GraphContractError::InvalidLimits {
            reason: "graph hydration key set exceeds the product ceiling",
        }
        .into());
    }
    if digests.iter().copied().collect::<HashSet<_>>().len() != digests.len() {
        return Err(GraphContractError::InvalidLimits {
            reason: "graph hydration keys must be unique",
        }
        .into());
    }
    Ok(())
}

/// Return the exact repository path that can own authored purpose for an entity.
fn graph_entity_purpose_owner(entity: &GraphEntity) -> Option<&str> {
    match entity.selector() {
        EntitySelector::Project => Some("."),
        EntitySelector::Folder { path } => Some(path.as_str()),
        EntitySelector::File { path } => Some(path.as_str()),
        EntitySelector::Package { package } => Some(package.manifest.as_str()),
        EntitySelector::Symbol { symbol } => Some(symbol.file.as_str()),
        EntitySelector::External { .. } => None,
    }
}

/// Reconstruct one relation and retain its already-hydrated endpoint entities.
fn relation_detail_from_row(
    entities: &HashMap<[u8; 32], GraphEntity>,
    row: RelationRow,
    expected_project: ProjectInstanceId,
    generation: IndexGeneration,
) -> DbResult<RepositoryGraphRelationRow> {
    let source_key = fixed_bytes::<32>("graph_relations.source_entity_key", row.source.clone())?;
    let target_key = row
        .target
        .as_ref()
        .map(|target| fixed_bytes::<32>("graph_relations.target_entity_key", target.clone()))
        .transpose()?;
    let relation = relation_from_row(entities, row, expected_project, generation)?;
    let source = entities
        .get(&source_key)
        .cloned()
        .ok_or(DbError::GraphRowShape {
            table: "graph_relations",
            reason: "source entity is missing",
        })?;
    let target = target_key
        .map(|key| {
            entities.get(&key).cloned().ok_or(DbError::GraphRowShape {
                table: "graph_relations",
                reason: "retained target entity is missing",
            })
        })
        .transpose()?;
    Ok(RepositoryGraphRelationRow {
        relation,
        source,
        target,
    })
}

/// Reconstruct one typed logical relation through existing domain constructors.
fn relation_from_row(
    entities: &HashMap<[u8; 32], GraphEntity>,
    row: RelationRow,
    expected_project: ProjectInstanceId,
    generation: IndexGeneration,
) -> DbResult<LogicalRelation> {
    let project = project_from_blob("graph_relations.project_instance_id", row.project.clone())?;
    require_project(expected_project, project)?;
    let source_key = fixed_bytes::<32>("graph_relations.source_entity_key", row.source.clone())?;
    let source = entities
        .get(&source_key)
        .cloned()
        .ok_or(DbError::GraphRowShape {
            table: "graph_relations",
            reason: "source entity is missing",
        })?;
    let kind = parse_relation_kind(&row.relation_scope, &row.relation_kind)?;
    let resolution = match row.resolution_status.as_str() {
        "resolved" => {
            require_relation_resolution_shape(&row, true, false, false)?;
            let target_key = fixed_bytes::<32>(
                "graph_relations.target_entity_key",
                row.target.clone().ok_or(DbError::GraphRowShape {
                    table: "graph_relations",
                    reason: "resolved target is missing",
                })?,
            )?;
            let target = entities
                .get(&target_key)
                .cloned()
                .ok_or(DbError::GraphRowShape {
                    table: "graph_relations",
                    reason: "resolved target entity is missing",
                })?;
            RelationResolution::resolved(&target)?
        }
        "external" => {
            require_relation_resolution_shape(&row, true, false, false)?;
            let target_key = fixed_bytes::<32>(
                "graph_relations.target_entity_key",
                row.target.clone().ok_or(DbError::GraphRowShape {
                    table: "graph_relations",
                    reason: "external target is missing",
                })?,
            )?;
            let target = entities
                .get(&target_key)
                .cloned()
                .ok_or(DbError::GraphRowShape {
                    table: "graph_relations",
                    reason: "external target entity is missing",
                })?;
            RelationResolution::external(&target)?
        }
        "ambiguous" => {
            require_relation_resolution_shape(&row, false, true, true)?;
            let candidates = positive_u32(
                "graph_relations.candidate_count",
                row.candidate_count.ok_or(DbError::GraphRowShape {
                    table: "graph_relations",
                    reason: "ambiguous candidate count is missing",
                })?,
            )?;
            RelationResolution::Ambiguous {
                reference: GraphIdentityText::new(row.reference.clone().ok_or(
                    DbError::GraphRowShape {
                        table: "graph_relations",
                        reason: "ambiguous reference is missing",
                    },
                )?)?,
                candidates,
            }
        }
        "unresolved" => {
            require_relation_resolution_shape(&row, false, true, false)?;
            RelationResolution::Unresolved {
                reference: GraphIdentityText::new(row.reference.clone().ok_or(
                    DbError::GraphRowShape {
                        table: "graph_relations",
                        reason: "unresolved reference is missing",
                    },
                )?)?,
            }
        }
        value => {
            return Err(DbError::InvalidEnum {
                field: "graph_relations.resolution_status",
                value: value.to_string(),
            });
        }
    };
    let relation = LogicalRelation::new(
        &source,
        kind,
        resolution,
        parse_confidence(&row.confidence)?,
        parse_completeness(&row.completeness)?,
        generation,
    )?;
    validate_relation_key(&relation, row.key, &row.canonical)?;
    Ok(relation)
}

/// Reject contradictory normalized resolution columns.
fn require_relation_resolution_shape(
    row: &RelationRow,
    target_required: bool,
    reference_required: bool,
    candidates_required: bool,
) -> DbResult<()> {
    let valid = row.target.is_some() == target_required
        && row.reference.is_some() == reference_required
        && row.candidate_count.is_some() == candidates_required;
    if !valid {
        return Err(DbError::GraphRowShape {
            table: "graph_relations",
            reason: "resolution columns contradict status",
        });
    }
    Ok(())
}

/// Reconstruct one exact relation occurrence.
fn occurrence_from_row(
    row: OccurrenceRow,
    relation: &LogicalRelation,
    generation: IndexGeneration,
) -> DbResult<RelationOccurrence> {
    let stored_key = fixed_bytes::<32>("graph_relation_occurrences.relation_key", row.relation)?;
    if stored_key != relation.key().digest_bytes()? {
        return Err(DbError::GraphRowShape {
            table: "graph_relation_occurrences",
            reason: "occurrence relation key does not match query",
        });
    }
    RelationOccurrence::new(
        relation,
        RepositoryFilePath::new(Path::new(&row.file_path))?,
        SourceSpan::new(
            positive_u32_value("graph_relation_occurrences.start_line", row.start_line)?,
            nonnegative_u32("graph_relation_occurrences.start_column", row.start_column)?,
            positive_u32_value("graph_relation_occurrences.end_line", row.end_line)?,
            nonnegative_u32("graph_relation_occurrences.end_column", row.end_column)?,
        )?,
        generation,
    )
    .map_err(Into::into)
}

/// Reconstruct one graph coverage record and verify project ownership.
fn coverage_from_row(
    row: CoverageRow,
    expected_project: ProjectInstanceId,
    generation: IndexGeneration,
) -> DbResult<CoverageRecord> {
    let project = project_from_blob("graph_coverage.project_instance_id", row.project)?;
    require_project(expected_project, project)?;
    let scope = match (row.scope_kind.as_str(), row.scope_path) {
        ("project", None) => CoverageScope::Project,
        ("path", Some(path)) => CoverageScope::Path {
            path: RepositoryNodePath::new(Path::new(&path))?,
        },
        ("project" | "path", _) => {
            return Err(DbError::GraphRowShape {
                table: "graph_coverage",
                reason: "scope columns contradict scope kind",
            });
        }
        (value, _) => {
            return Err(DbError::InvalidEnum {
                field: "graph_coverage.scope_kind",
                value: value.to_string(),
            });
        }
    };
    let relation = match (row.relation_scope, row.relation_kind) {
        (None, None) => None,
        (Some(scope), Some(kind)) => Some(parse_relation_kind(&scope, &kind)?),
        _ => {
            return Err(DbError::GraphRowShape {
                table: "graph_coverage",
                reason: "relation scope and kind must both be present or absent",
            });
        }
    };
    let persisted_total = nonnegative_u64("graph_coverage.total", row.total)?;
    let record = CoverageRecord::new(
        scope,
        relation,
        parse_coverage_state(&row.state)?,
        nonnegative_u64("graph_coverage.covered", row.covered)?,
        nonnegative_u64("graph_coverage.omitted", row.omitted)?,
        generation,
        row.reason.map(GraphIdentityText::new).transpose()?,
        row.reached_limit
            .as_deref()
            .map(parse_limit_kind)
            .transpose()?,
    )?;
    if record.total() != persisted_total {
        return Err(DbError::GraphRowShape {
            table: "graph_coverage",
            reason: "total does not equal covered plus omitted",
        });
    }
    Ok(record)
}

/// Reconstruct discovered coverage together with strict parser provenance.
fn coverage_discovery_from_row(
    row: CoverageRow,
    expected_project: ProjectInstanceId,
    generation: IndexGeneration,
) -> DbResult<RepositoryCoverageRow> {
    let parser = row
        .parser
        .as_deref()
        .map(|value| parse_parser_kind("source_parse_metadata.source_parser", value))
        .transpose()?;
    let provider = row
        .provider
        .as_deref()
        .map(|value| parse_parser_kind("source_parse_metadata.fact_parser", value))
        .transpose()?;
    let coverage = coverage_from_row(row, expected_project, generation)?;
    Ok(RepositoryCoverageRow {
        coverage,
        parser,
        provider,
    })
}

/// Fail with both project identities when normalized ownership differs.
fn require_project(expected: ProjectInstanceId, found: ProjectInstanceId) -> DbResult<()> {
    if expected != found {
        return Err(DbError::GraphProjectIdentityMismatch {
            expected: expected.to_string(),
            found: found.to_string(),
        });
    }
    Ok(())
}

/// Validate one stored entity key and canonical collision witness.
fn validate_entity_key(
    entity: &GraphEntity,
    stored_key: Vec<u8>,
    stored_canonical: &str,
) -> DbResult<()> {
    let stored_key = fixed_bytes::<32>("graph_entities.entity_key", stored_key)?;
    if stored_key != entity.key().digest_bytes()? {
        return Err(projectatlas_core::graph::GraphContractError::InvalidStableKeyDigest.into());
    }
    if stored_canonical != entity.key().canonical_identity() {
        return Err(
            projectatlas_core::graph::GraphContractError::StableKeyCollision {
                digest: entity.key().digest().to_string(),
            }
            .into(),
        );
    }
    Ok(())
}

/// Validate one stored relation key and canonical collision witness.
fn validate_relation_key(
    relation: &LogicalRelation,
    stored_key: Vec<u8>,
    stored_canonical: &str,
) -> DbResult<()> {
    let stored_key = fixed_bytes::<32>("graph_relations.relation_key", stored_key)?;
    if stored_key != relation.key().digest_bytes()? {
        return Err(projectatlas_core::graph::GraphContractError::InvalidStableKeyDigest.into());
    }
    if stored_canonical != relation.key().canonical_identity() {
        return Err(
            projectatlas_core::graph::GraphContractError::StableKeyCollision {
                digest: relation.key().digest().to_string(),
            }
            .into(),
        );
    }
    Ok(())
}

/// Collect every raw entity row, including the truncation sentinel row.
fn collect_entity_rows(mut rows: rusqlite::Rows<'_>) -> DbResult<Vec<EntityRow>> {
    let mut collected = Vec::new();
    while let Some(row) = rows.next()? {
        collected.push(entity_row(row)?);
    }
    Ok(collected)
}

/// Collect raw entity rows while enforcing decoded payload bytes.
fn collect_entity_rows_metered(
    mut rows: rusqlite::Rows<'_>,
    meter: &mut RepositoryGraphReadMeter,
) -> DbResult<Vec<EntityRow>> {
    let mut collected = Vec::new();
    while let Some(row) = rows.next()? {
        let raw = entity_row(row)?;
        meter.record_decoded_bytes(entity_row_decoded_bytes(&raw)?)?;
        collected.push(raw);
    }
    Ok(collected)
}

/// Read one raw entity row without interpreting enum or selector values.
fn entity_row(row: &Row<'_>) -> rusqlite::Result<EntityRow> {
    Ok(EntityRow {
        key: row.get(0)?,
        project: row.get(1)?,
        canonical: row.get(2)?,
        kind: row.get(3)?,
        repository_path: row.get(4)?,
        package_manager: row.get(5)?,
        package_name: row.get(6)?,
        manifest_path: row.get(7)?,
        symbol_name: row.get(8)?,
        symbol_kind: row.get(9)?,
        symbol_parent: row.get(10)?,
        symbol_signature: row.get(11)?,
        external_system: row.get(12)?,
        external_identity: row.get(13)?,
    })
}

/// Count exact dynamic payload bytes decoded for one normalized entity row.
fn entity_row_decoded_bytes(row: &EntityRow) -> DbResult<u64> {
    decoded_payload_bytes(
        [
            row.key.len(),
            row.project.len(),
            row.canonical.len(),
            row.kind.len(),
            row.repository_path.as_ref().map_or(0, String::len),
            row.package_manager.as_ref().map_or(0, String::len),
            row.package_name.as_ref().map_or(0, String::len),
            row.manifest_path.as_ref().map_or(0, String::len),
            row.symbol_name.as_ref().map_or(0, String::len),
            row.symbol_kind.as_ref().map_or(0, String::len),
            row.symbol_parent.as_ref().map_or(0, String::len),
            row.symbol_signature.as_ref().map_or(0, String::len),
            row.external_system.as_ref().map_or(0, String::len),
            row.external_identity.as_ref().map_or(0, String::len),
        ],
        0,
    )
}

/// Collect every raw relation row, including the truncation sentinel row.
fn collect_relation_rows(mut rows: rusqlite::Rows<'_>) -> DbResult<Vec<RelationRow>> {
    let mut collected = Vec::new();
    while let Some(row) = rows.next()? {
        collected.push(relation_row(row)?);
    }
    Ok(collected)
}

/// Collect raw relation rows while enforcing decoded payload bytes.
fn collect_relation_rows_metered(
    mut rows: rusqlite::Rows<'_>,
    meter: &mut RepositoryGraphReadMeter,
) -> DbResult<Vec<RelationRow>> {
    let mut collected = Vec::new();
    while let Some(row) = rows.next()? {
        let raw = relation_row(row)?;
        meter.record_decoded_bytes(relation_row_decoded_bytes(&raw)?)?;
        collected.push(raw);
    }
    Ok(collected)
}

/// Collect raw adjacency rows and meter the truncation sentinel before return.
fn collect_adjacency_relation_rows_metered(
    mut rows: rusqlite::Rows<'_>,
    meter: &mut RepositoryGraphReadMeter,
) -> DbResult<Vec<AdjacencyRelationRow>> {
    let mut collected = Vec::new();
    while let Some(row) = rows.next()? {
        let raw = AdjacencyRelationRow {
            frontier_index: row.get(0)?,
            relation: relation_row_at(row, 1)?,
        };
        meter.record_decoded_bytes(
            relation_row_decoded_bytes(&raw.relation)?
                .checked_add(8)
                .ok_or(GraphContractError::InvalidLimits {
                    reason: "graph adjacency decoded row size overflowed",
                })?,
        )?;
        collected.push(raw);
    }
    Ok(collected)
}

/// Read one raw relation row without interpreting enum or resolution values.
fn relation_row(row: &Row<'_>) -> rusqlite::Result<RelationRow> {
    relation_row_at(row, 0)
}

/// Read one raw relation row beginning at the selected column offset.
fn relation_row_at(row: &Row<'_>, offset: usize) -> rusqlite::Result<RelationRow> {
    Ok(RelationRow {
        key: row.get(offset)?,
        project: row.get(offset + 1)?,
        canonical: row.get(offset + 2)?,
        source: row.get(offset + 3)?,
        relation_scope: row.get(offset + 4)?,
        relation_kind: row.get(offset + 5)?,
        resolution_status: row.get(offset + 6)?,
        target: row.get(offset + 7)?,
        reference: row.get(offset + 8)?,
        candidate_count: row.get(offset + 9)?,
        confidence: row.get(offset + 10)?,
        completeness: row.get(offset + 11)?,
    })
}

/// Count exact dynamic and fixed payload bytes decoded for one relation row.
fn relation_row_decoded_bytes(row: &RelationRow) -> DbResult<u64> {
    decoded_payload_bytes(
        [
            row.key.len(),
            row.project.len(),
            row.canonical.len(),
            row.source.len(),
            row.relation_scope.len(),
            row.relation_kind.len(),
            row.resolution_status.len(),
            row.target.as_ref().map_or(0, Vec::len),
            row.reference.as_ref().map_or(0, String::len),
            row.confidence.len(),
            row.completeness.len(),
        ],
        8,
    )
}

/// Sum decoded variable-width values plus fixed scalar widths without overflow.
fn decoded_payload_bytes(
    lengths: impl IntoIterator<Item = usize>,
    fixed_bytes: u64,
) -> DbResult<u64> {
    let mut decoded = fixed_bytes;
    for length in lengths {
        let length =
            u64::try_from(length).map_err(|_source| GraphContractError::InvalidLimits {
                reason: "graph read decoded field length overflowed",
            })?;
        decoded = decoded
            .checked_add(length)
            .ok_or(GraphContractError::InvalidLimits {
                reason: "graph read decoded row size overflowed",
            })?;
    }
    Ok(decoded)
}

/// Read one raw relation occurrence row.
fn occurrence_row(row: &Row<'_>) -> rusqlite::Result<OccurrenceRow> {
    occurrence_row_at(row, 0)
}

/// Read one raw relation occurrence row at a stable column offset.
fn occurrence_row_at(row: &Row<'_>, offset: usize) -> rusqlite::Result<OccurrenceRow> {
    Ok(OccurrenceRow {
        relation: row.get(offset)?,
        file_path: row.get(offset + 1)?,
        start_line: row.get(offset + 2)?,
        start_column: row.get(offset + 3)?,
        end_line: row.get(offset + 4)?,
        end_column: row.get(offset + 5)?,
    })
}

/// Count exact occurrence payload bytes, including four fixed span scalars.
fn occurrence_row_decoded_bytes(row: &OccurrenceRow) -> DbResult<u64> {
    decoded_payload_bytes([row.relation.len(), row.file_path.len()], 32)
}

/// Read one raw graph coverage row.
fn coverage_row(row: &Row<'_>) -> rusqlite::Result<CoverageRow> {
    Ok(CoverageRow {
        project: row.get(0)?,
        scope_kind: row.get(1)?,
        scope_path: row.get(2)?,
        relation_scope: row.get(3)?,
        relation_kind: row.get(4)?,
        state: row.get(5)?,
        total: row.get(6)?,
        covered: row.get(7)?,
        omitted: row.get(8)?,
        reason: row.get(9)?,
        reached_limit: row.get(10)?,
        parser: row.get(11)?,
        provider: row.get(12)?,
    })
}

/// Count exact coverage payload bytes, including three fixed count scalars.
fn coverage_row_decoded_bytes(row: &CoverageRow) -> DbResult<u64> {
    decoded_payload_bytes(
        [
            row.project.len(),
            row.scope_kind.len(),
            row.scope_path.as_ref().map_or(0, String::len),
            row.relation_scope.as_ref().map_or(0, String::len),
            row.relation_kind.as_ref().map_or(0, String::len),
            row.state.len(),
            row.reason.as_ref().map_or(0, String::len),
            row.reached_limit.as_ref().map_or(0, String::len),
            row.parser.as_ref().map_or(0, String::len),
            row.provider.as_ref().map_or(0, String::len),
        ],
        24,
    )
}

/// Convert a fully collected raw page and validate the sentinel before truncating.
fn page_from_raw<Raw, Domain>(
    raw: Vec<Raw>,
    limit: u32,
    mut convert: impl FnMut(Raw) -> DbResult<Domain>,
) -> DbResult<RepositoryGraphPage<Domain>> {
    let mut rows = raw
        .into_iter()
        .map(&mut convert)
        .collect::<DbResult<Vec<_>>>()?;
    let truncated = rows.len() > limit as usize;
    if truncated {
        rows.pop();
    }
    Ok(RepositoryGraphPage { rows, truncated })
}

/// Return an empty graph page when no project graph has been initialized.
fn empty_page<T>() -> RepositoryGraphPage<T> {
    RepositoryGraphPage {
        rows: Vec::new(),
        truncated: false,
    }
}

/// Return an empty adjacency page when no selected graph/frontier is available.
fn empty_adjacency_page() -> RepositoryGraphAdjacencyPage {
    RepositoryGraphAdjacencyPage {
        rows: Vec::new(),
        truncated: false,
        continuation: None,
    }
}

/// Return an empty footprint when no complete selected graph is available.
const fn empty_affected_source_footprint() -> RepositoryAffectedSourceFootprint {
    RepositoryAffectedSourceFootprint {
        rows: 0,
        retained_bytes: 0,
        truncated: false,
    }
}

/// Validate and convert a requested page size into `LIMIT + 1`.
fn validated_limit_plus_one(limit: u32, ceiling: u32, reason: &'static str) -> DbResult<i64> {
    if limit == 0 || limit > ceiling {
        return Err(projectatlas_core::graph::GraphContractError::InvalidLimits { reason }.into());
    }
    Ok(i64::from(limit) + 1)
}

/// Convert a fixed-width normalized BLOB without truncation.
fn fixed_bytes<const WIDTH: usize>(field: &'static str, bytes: Vec<u8>) -> DbResult<[u8; WIDTH]> {
    let found = bytes.len();
    bytes
        .try_into()
        .map_err(|_bytes| DbError::InvalidBlobLength {
            field,
            expected: WIDTH,
            found,
        })
}

/// Reconstruct a project identity from its normalized binary column.
fn project_from_blob(field: &'static str, bytes: Vec<u8>) -> DbResult<ProjectInstanceId> {
    ProjectInstanceId::from_bytes(fixed_bytes::<16>(field, bytes)?).map_err(Into::into)
}

/// Return a required text column or a stable row-shape failure.
fn required_text<'value>(
    table: &'static str,
    reason: &'static str,
    value: Option<&'value str>,
) -> DbResult<&'value str> {
    value.ok_or(DbError::GraphRowShape { table, reason })
}

/// Convert a nonnegative `SQLite` count to `u64`.
fn nonnegative_u64(field: &'static str, value: i64) -> DbResult<u64> {
    u64::try_from(value).map_err(|source| DbError::InvalidCount {
        field,
        value,
        source,
    })
}

/// Convert a positive `SQLite` count to `NonZeroU32`.
fn positive_u32(field: &'static str, value: i64) -> DbResult<NonZeroU32> {
    let value = positive_u32_value(field, value)?;
    NonZeroU32::new(value).ok_or(DbError::GraphRowShape {
        table: "graph_relations",
        reason: "candidate count must be positive",
    })
}

/// Convert a positive `SQLite` integer to `u32`.
fn positive_u32_value(field: &'static str, value: i64) -> DbResult<u32> {
    let converted = u32::try_from(value).map_err(|source| DbError::InvalidCount {
        field,
        value,
        source,
    })?;
    if converted == 0 {
        return Err(DbError::GraphRowShape {
            table: "repository_graph",
            reason: "positive integer column contains zero",
        });
    }
    Ok(converted)
}

/// Convert a nonnegative `SQLite` integer to `u32`.
fn nonnegative_u32(field: &'static str, value: i64) -> DbResult<u32> {
    u32::try_from(value).map_err(|source| DbError::InvalidCount {
        field,
        value,
        source,
    })
}

/// Convert a domain count to one lossless `SQLite` integer.
fn sqlite_count(field: &'static str, value: u64) -> DbResult<i64> {
    i64::try_from(value).map_err(|_source| DbError::GraphCountOverflow { field, value })
}

/// Split one typed relation family into its normalized scope and spelling.
const fn relation_parts(relation: GraphRelationKind) -> (&'static str, &'static str) {
    match relation {
        GraphRelationKind::Legacy(RelationKind::Contains) => ("legacy", "contains"),
        GraphRelationKind::Legacy(RelationKind::Imports) => ("legacy", "imports"),
        GraphRelationKind::Legacy(RelationKind::Calls) => ("legacy", "calls"),
        GraphRelationKind::Legacy(RelationKind::DependsOn) => ("legacy", "depends-on"),
        GraphRelationKind::Extended(ExtendedRelationKind::References) => ("extended", "references"),
        GraphRelationKind::Extended(ExtendedRelationKind::Tests) => ("extended", "tests"),
        GraphRelationKind::Extended(ExtendedRelationKind::RoutesTo) => ("extended", "routes-to"),
        GraphRelationKind::Extended(ExtendedRelationKind::Configures) => ("extended", "configures"),
        GraphRelationKind::Extended(ExtendedRelationKind::Reads) => ("extended", "reads"),
        GraphRelationKind::Extended(ExtendedRelationKind::Writes) => ("extended", "writes"),
    }
}

/// Parse one normalized relation family without accepting unknown values.
fn parse_relation_kind(scope: &str, kind: &str) -> DbResult<GraphRelationKind> {
    match (scope, kind) {
        ("legacy", "contains") => Ok(GraphRelationKind::Legacy(RelationKind::Contains)),
        ("legacy", "imports") => Ok(GraphRelationKind::Legacy(RelationKind::Imports)),
        ("legacy", "calls") => Ok(GraphRelationKind::Legacy(RelationKind::Calls)),
        ("legacy", "depends-on") => Ok(GraphRelationKind::Legacy(RelationKind::DependsOn)),
        ("extended", "references") => Ok(GraphRelationKind::Extended(
            ExtendedRelationKind::References,
        )),
        ("extended", "tests") => Ok(GraphRelationKind::Extended(ExtendedRelationKind::Tests)),
        ("extended", "routes-to") => {
            Ok(GraphRelationKind::Extended(ExtendedRelationKind::RoutesTo))
        }
        ("extended", "configures") => Ok(GraphRelationKind::Extended(
            ExtendedRelationKind::Configures,
        )),
        ("extended", "reads") => Ok(GraphRelationKind::Extended(ExtendedRelationKind::Reads)),
        ("extended", "writes") => Ok(GraphRelationKind::Extended(ExtendedRelationKind::Writes)),
        _ => Err(DbError::InvalidEnum {
            field: "graph_relations.relation_kind",
            value: format!("{scope}:{kind}"),
        }),
    }
}

/// Return the normalized symbol-kind spelling.
const fn symbol_kind_name(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "function",
        SymbolKind::Method => "method",
        SymbolKind::Class => "class",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::Trait => "trait",
        SymbolKind::Interface => "interface",
        SymbolKind::Module => "module",
        SymbolKind::Type => "type",
        SymbolKind::Value => "value",
        SymbolKind::Import => "import",
        SymbolKind::Package => "package",
        SymbolKind::Workspace => "workspace",
        SymbolKind::Dependency => "dependency",
        SymbolKind::Unknown => "unknown",
    }
}

/// Parse one normalized symbol-kind spelling.
fn parse_symbol_kind(value: &str) -> DbResult<SymbolKind> {
    match value {
        "function" => Ok(SymbolKind::Function),
        "method" => Ok(SymbolKind::Method),
        "class" => Ok(SymbolKind::Class),
        "struct" => Ok(SymbolKind::Struct),
        "enum" => Ok(SymbolKind::Enum),
        "trait" => Ok(SymbolKind::Trait),
        "interface" => Ok(SymbolKind::Interface),
        "module" => Ok(SymbolKind::Module),
        "type" => Ok(SymbolKind::Type),
        "value" => Ok(SymbolKind::Value),
        "import" => Ok(SymbolKind::Import),
        "package" => Ok(SymbolKind::Package),
        "workspace" => Ok(SymbolKind::Workspace),
        "dependency" => Ok(SymbolKind::Dependency),
        "unknown" => Ok(SymbolKind::Unknown),
        _ => Err(DbError::InvalidEnum {
            field: "graph_entities.symbol_kind",
            value: value.to_string(),
        }),
    }
}

/// Return the normalized confidence spelling.
const fn confidence_name(confidence: ConfidenceClass) -> &'static str {
    match confidence {
        ConfidenceClass::Exact => "exact",
        ConfidenceClass::High => "high",
        ConfidenceClass::Medium => "medium",
        ConfidenceClass::Low => "low",
    }
}

/// Parse one normalized confidence spelling.
fn parse_confidence(value: &str) -> DbResult<ConfidenceClass> {
    match value {
        "exact" => Ok(ConfidenceClass::Exact),
        "high" => Ok(ConfidenceClass::High),
        "medium" => Ok(ConfidenceClass::Medium),
        "low" => Ok(ConfidenceClass::Low),
        _ => Err(DbError::InvalidEnum {
            field: "graph_relations.confidence",
            value: value.to_string(),
        }),
    }
}

/// Return the normalized completeness spelling.
const fn completeness_name(completeness: Completeness) -> &'static str {
    match completeness {
        Completeness::Complete => "complete",
        Completeness::Partial => "partial",
    }
}

/// Parse one normalized completeness spelling.
fn parse_completeness(value: &str) -> DbResult<Completeness> {
    match value {
        "complete" => Ok(Completeness::Complete),
        "partial" => Ok(Completeness::Partial),
        _ => Err(DbError::InvalidEnum {
            field: "graph_relations.completeness",
            value: value.to_string(),
        }),
    }
}

/// Return normalized coverage scope columns.
fn coverage_scope_parts(scope: &CoverageScope) -> (&'static str, Option<&str>) {
    match scope {
        CoverageScope::Project => ("project", None),
        CoverageScope::Path { path } => ("path", Some(path.as_str())),
    }
}

/// Return the normalized coverage lifecycle spelling.
const fn coverage_state_name(state: CoverageState) -> &'static str {
    match state {
        CoverageState::Complete => "complete",
        CoverageState::Partial => "partial",
        CoverageState::Failed => "failed",
        CoverageState::Ignored => "ignored",
        CoverageState::Oversized => "oversized",
        CoverageState::Quarantined => "quarantined",
        CoverageState::Stale => "stale",
    }
}

/// Parse one normalized coverage lifecycle spelling.
pub(crate) fn parse_coverage_state(value: &str) -> DbResult<CoverageState> {
    match value {
        "complete" => Ok(CoverageState::Complete),
        "partial" => Ok(CoverageState::Partial),
        "failed" => Ok(CoverageState::Failed),
        "ignored" => Ok(CoverageState::Ignored),
        "oversized" => Ok(CoverageState::Oversized),
        "quarantined" => Ok(CoverageState::Quarantined),
        "stale" => Ok(CoverageState::Stale),
        _ => Err(DbError::InvalidEnum {
            field: "graph_coverage.state",
            value: value.to_string(),
        }),
    }
}

/// Parse one normalized parser provenance spelling without fallback coercion.
fn parse_parser_kind(field: &'static str, value: &str) -> DbResult<ParserKind> {
    match value {
        "tree-sitter" => Ok(ParserKind::TreeSitter),
        "manifest" => Ok(ParserKind::Manifest),
        "structural" => Ok(ParserKind::Structural),
        "fallback" => Ok(ParserKind::Fallback),
        _ => Err(DbError::InvalidEnum {
            field,
            value: value.to_string(),
        }),
    }
}

/// Return the normalized reached-limit spelling.
const fn limit_kind_name(limit: GraphLimitKind) -> &'static str {
    match limit {
        GraphLimitKind::Rows => "rows",
        GraphLimitKind::Nodes => "nodes",
        GraphLimitKind::Edges => "edges",
        GraphLimitKind::Occurrences => "occurrences",
        GraphLimitKind::Visited => "visited",
        GraphLimitKind::IntermediateBytes => "intermediate_bytes",
        GraphLimitKind::Deadline => "deadline",
        GraphLimitKind::Depth => "depth",
        GraphLimitKind::OutputBytes => "output_bytes",
    }
}

/// Parse one normalized reached-limit spelling.
pub(crate) fn parse_limit_kind(value: &str) -> DbResult<GraphLimitKind> {
    match value {
        "rows" => Ok(GraphLimitKind::Rows),
        "nodes" => Ok(GraphLimitKind::Nodes),
        "edges" => Ok(GraphLimitKind::Edges),
        "occurrences" => Ok(GraphLimitKind::Occurrences),
        "visited" => Ok(GraphLimitKind::Visited),
        "intermediate_bytes" => Ok(GraphLimitKind::IntermediateBytes),
        "deadline" => Ok(GraphLimitKind::Deadline),
        "depth" => Ok(GraphLimitKind::Depth),
        "output_bytes" => Ok(GraphLimitKind::OutputBytes),
        _ => Err(DbError::InvalidEnum {
            field: "graph_coverage.reached_limit",
            value: value.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IndexedFileText;
    use projectatlas_core::symbols::{CodeSymbol, ParserKind, SymbolGraph, SymbolRelation};
    use projectatlas_core::{Node, NodeKind};
    use std::error::Error;
    use std::fmt::Debug;
    use std::fs;
    use std::io;
    use std::time::{Duration, Instant};

    /// Traverse one relation family with the same bounded adjacency primitive used by the service.
    fn collect_bounded_outbound_calls(
        store: &AtlasStore,
        anchor: &GraphEntityKey,
    ) -> Result<(Vec<[u8; 32]>, usize, usize), Box<dyn Error>> {
        let mut visited = BTreeMap::new();
        visited.insert(anchor.digest_bytes()?, anchor.clone());
        let mut frontier = vec![anchor.clone()];
        let mut inspected_edges = 0_usize;
        let mut peak_frontier = frontier.len();
        let calls = GraphRelationKind::Legacy(RelationKind::Calls);

        while !frontier.is_empty() {
            let mut next = BTreeMap::new();
            for chunk in frontier.chunks(MAX_REPOSITORY_GRAPH_FRONTIER) {
                let work_per_row = MAX_REPOSITORY_GRAPH_ADJACENCY_WORK_ROWS / chunk.len();
                let page_limit = work_per_row
                    .saturating_sub(1)
                    .min(GraphLimits::MAX_ROWS as usize)
                    .max(1) as u32;
                let mut continuation = None;
                loop {
                    let page = store.repository_graph_adjacency_page_filtered(
                        chunk,
                        RepositoryGraphDirection::Outbound,
                        Some(calls),
                        continuation.as_ref(),
                        page_limit,
                        None,
                    )?;
                    inspected_edges = inspected_edges
                        .checked_add(page.rows.len())
                        .ok_or_else(|| io::Error::other("measured edge count overflowed"))?;
                    for row in page.rows {
                        if let Some(target) = row.detail.target {
                            let digest = target.key().digest_bytes()?;
                            if !visited.contains_key(&digest) {
                                next.entry(digest).or_insert_with(|| target.key().clone());
                            }
                        }
                    }
                    if !page.truncated {
                        break;
                    }
                    continuation = Some(page.continuation.ok_or_else(|| {
                        io::Error::other("truncated adjacency page omitted its continuation")
                    })?);
                }
            }
            frontier = next.into_values().collect();
            peak_frontier = peak_frontier.max(frontier.len());
            for key in &frontier {
                visited.insert(key.digest_bytes()?, key.clone());
            }
        }

        Ok((
            visited.into_keys().collect(),
            inspected_edges,
            peak_frontier,
        ))
    }

    /// Coherent typed graph fixture used by storage, corruption, and publication tests.
    struct GraphFixture {
        /// Owning project identity.
        project: ProjectInstanceId,
        /// Every entity selector variant.
        entities: Vec<GraphEntity>,
        /// Every relation resolution state.
        relations: Vec<LogicalRelation>,
        /// Two occurrences for one logical relation.
        occurrences: Vec<RelationOccurrence>,
        /// Every coverage lifecycle state.
        coverage: Vec<CoverageRecord>,
    }

    /// Canonical keys used by export, ambiguity, and unresolved dependency tests.
    struct ResolutionFixture {
        /// Published typed graph.
        graph: GraphFixture,
        /// Key exported by the fixture symbol and consumed by a resolved relation.
        resolved: CanonicalResolutionKey,
        /// Key consumed by an ambiguous relation.
        ambiguous: CanonicalResolutionKey,
        /// Key consumed by an unresolved relation.
        unresolved: CanonicalResolutionKey,
    }

    /// Build one complete typed graph for the selected generation.
    fn graph_fixture(
        project: ProjectInstanceId,
        generation: IndexGeneration,
    ) -> Result<GraphFixture, Box<dyn Error>> {
        let project_entity = GraphEntity::new(project, EntitySelector::Project, generation)?;
        let folder = GraphEntity::new(
            project,
            EntitySelector::Folder {
                path: RepositoryNodePath::new(Path::new("src"))?,
            },
            generation,
        )?;
        let file = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/Äuth.rs"))?,
            },
            generation,
        )?;
        let package = GraphEntity::new(
            project,
            EntitySelector::Package {
                package: PackageSelector {
                    manager: GraphIdentityText::new("cargo")?,
                    name: GraphIdentityText::new("ProjectAtlas")?,
                    manifest: RepositoryFilePath::new(Path::new("Cargo.toml"))?,
                },
            },
            generation,
        )?;
        let symbol = GraphEntity::new(
            project,
            EntitySelector::Symbol {
                symbol: SymbolSelector {
                    file: RepositoryFilePath::new(Path::new("src/Äuth.rs"))?,
                    name: GraphIdentityText::new("verifyToken")?,
                    kind: SymbolKind::Function,
                    parent: Some(GraphIdentityText::new("Auth")?),
                    signature: GraphIdentityText::new("verifyToken(&str)")?,
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

        let resolved = LogicalRelation::new(
            &file,
            GraphRelationKind::Legacy(RelationKind::Calls),
            RelationResolution::resolved(&symbol)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation,
        )?;
        let ambiguous = LogicalRelation::new(
            &file,
            GraphRelationKind::Extended(ExtendedRelationKind::References),
            RelationResolution::Ambiguous {
                reference: GraphIdentityText::new("Session")?,
                candidates: NonZeroU32::new(2)
                    .ok_or_else(|| io::Error::other("fixture candidate count is zero"))?,
            },
            ConfidenceClass::Medium,
            Completeness::Partial,
            generation,
        )?;
        let unresolved = LogicalRelation::new(
            &file,
            GraphRelationKind::Extended(ExtendedRelationKind::Configures),
            RelationResolution::Unresolved {
                reference: GraphIdentityText::new("AUTH_KEY")?,
            },
            ConfidenceClass::Low,
            Completeness::Partial,
            generation,
        )?;
        let external_relation = LogicalRelation::new(
            &file,
            GraphRelationKind::Legacy(RelationKind::DependsOn),
            RelationResolution::external(&external)?,
            ConfidenceClass::High,
            Completeness::Complete,
            generation,
        )?;
        let occurrences = vec![
            RelationOccurrence::new(
                &resolved,
                RepositoryFilePath::new(Path::new("src/Äuth.rs"))?,
                SourceSpan::new(10, 4, 10, 18)?,
                generation,
            )?,
            RelationOccurrence::new(
                &resolved,
                RepositoryFilePath::new(Path::new("src/Äuth.rs"))?,
                SourceSpan::new(22, 2, 22, 16)?,
                generation,
            )?,
        ];
        let coverage = vec![
            CoverageRecord::new(
                CoverageScope::Project,
                None,
                CoverageState::Complete,
                4,
                0,
                generation,
                None,
                None,
            )?,
            CoverageRecord::new(
                CoverageScope::Path {
                    path: RepositoryNodePath::new(Path::new("src"))?,
                },
                None,
                CoverageState::Partial,
                3,
                1,
                generation,
                Some(GraphIdentityText::new("one parser region omitted")?),
                Some(GraphLimitKind::Rows),
            )?,
            incomplete_coverage(
                GraphRelationKind::Legacy(RelationKind::Calls),
                CoverageState::Failed,
                "parser failed",
                generation,
            )?,
            incomplete_coverage(
                GraphRelationKind::Legacy(RelationKind::Imports),
                CoverageState::Ignored,
                "ignored by policy",
                generation,
            )?,
            incomplete_coverage(
                GraphRelationKind::Legacy(RelationKind::Contains),
                CoverageState::Oversized,
                "file exceeded limit",
                generation,
            )?,
            incomplete_coverage(
                GraphRelationKind::Legacy(RelationKind::DependsOn),
                CoverageState::Quarantined,
                "provider quarantined",
                generation,
            )?,
            incomplete_coverage(
                GraphRelationKind::Extended(ExtendedRelationKind::References),
                CoverageState::Stale,
                "source changed",
                generation,
            )?,
        ];
        Ok(GraphFixture {
            project,
            entities: vec![project_entity, folder, file, package, symbol, external],
            relations: vec![resolved, ambiguous, unresolved, external_relation],
            occurrences,
            coverage,
        })
    }

    /// Construct one project-qualified declaration resolution key.
    fn declaration_key(
        project: ProjectInstanceId,
        identity: &str,
        relation: GraphRelationKind,
    ) -> Result<CanonicalResolutionKey, Box<dyn Error>> {
        Ok(CanonicalResolutionKey::new(
            project,
            ResolutionKeyDomain::Declaration,
            &GraphIdentityText::new("tree-sitter")?,
            &GraphIdentityText::new("rust")?,
            None,
            Some(&GraphIdentityText::new("crate")?),
            Some(relation),
            &GraphIdentityText::new(identity)?,
        ))
    }

    /// Publish one graph with duplicate-safe export and all dependency states.
    fn publish_resolution_fixture(
        store: &mut AtlasStore,
        fingerprint: &str,
    ) -> Result<ResolutionFixture, Box<dyn Error>> {
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("bound fixture identity is missing"))?;
        let graph = graph_fixture(project, IndexGeneration::new(1))?;
        let resolved = declaration_key(
            project,
            "verifyToken",
            GraphRelationKind::Legacy(RelationKind::Calls),
        )?;
        let ambiguous = declaration_key(
            project,
            "Session",
            GraphRelationKind::Extended(ExtendedRelationKind::References),
        )?;
        let unresolved = declaration_key(
            project,
            "AUTH_KEY",
            GraphRelationKind::Extended(ExtendedRelationKind::Configures),
        )?;
        let export = EntityResolutionKey::new(graph.entities[4].key().clone(), resolved.clone())?;
        let exports = vec![export.clone(), export];
        let dependencies = vec![
            RelationDependencyKey::new(graph.relations[0].key().clone(), resolved.clone())?,
            RelationDependencyKey::new(graph.relations[1].key().clone(), ambiguous.clone())?,
            RelationDependencyKey::new(graph.relations[2].key().clone(), unresolved.clone())?,
        ];
        let mut publication = store.begin_index_publication(fingerprint)?;
        publication.begin_scan_replacement()?;
        publication.upsert_scan_node_batch(&[
            graph_node(".", NodeKind::Folder, None),
            graph_node("src", NodeKind::Folder, Some(".")),
            graph_node("src/Äuth.rs", NodeKind::File, Some("src")),
            graph_node("Cargo.toml", NodeKind::File, Some(".")),
        ])?;
        publication.finish_scan_replacement()?;
        publication.replace_repository_graph_with_resolution_keys(
            graph.project,
            &graph.entities,
            &graph.relations,
            &graph.occurrences,
            &graph.coverage,
            &exports,
            &dependencies,
        )?;
        publication.complete()?;
        Ok(ResolutionFixture {
            graph,
            resolved,
            ambiguous,
            unresolved,
        })
    }

    /// Replace one source-owned parser graph with a requested symbol degree.
    fn replace_fixture_symbol_rows(
        store: &mut AtlasStore,
        path: &str,
        symbol_count: usize,
    ) -> Result<(), Box<dyn Error>> {
        let symbols = (0..symbol_count)
            .map(|index| CodeSymbol {
                path: path.to_string(),
                language: Some("rust".to_string()),
                name: format!("symbol_{index}"),
                kind: SymbolKind::Function,
                signature: format!("fn symbol_{index}()"),
                exported: index == 0,
                documentation: None,
                line_start: index + 1,
                line_end: index + 1,
                parent: None,
                parser: ParserKind::TreeSitter,
                detail: Some("function_item".to_string()),
            })
            .collect();
        store.replace_symbol_graph(&SymbolGraph {
            path: path.to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols,
            relations: vec![SymbolRelation {
                path: path.to_string(),
                source_name: "symbol_0".to_string(),
                target_name: "dependency".to_string(),
                kind: RelationKind::Calls,
                line: 1,
                context: "dependency()".to_string(),
                parser: ParserKind::TreeSitter,
            }],
        })?;
        Ok(())
    }

    /// Construct one non-complete project-wide coverage row.
    fn incomplete_coverage(
        relation: GraphRelationKind,
        state: CoverageState,
        reason: &str,
        generation: IndexGeneration,
    ) -> Result<CoverageRecord, Box<dyn Error>> {
        Ok(CoverageRecord::new(
            CoverageScope::Project,
            Some(relation),
            state,
            0,
            1,
            generation,
            Some(GraphIdentityText::new(reason)?),
            None,
        )?)
    }

    /// Construct one local-source node for graph publication fixtures.
    fn graph_node(path: &str, kind: NodeKind, parent_path: Option<&str>) -> Node {
        Node {
            path: path.to_string(),
            kind,
            parent_path: parent_path.map(str::to_string),
            extension: None,
            language: None,
            size_bytes: None,
            mtime_ns: None,
            content_hash: None,
        }
    }

    /// Persisted all-family fixture for folder/file navigation enrichment.
    struct NavigationFixture {
        /// Selected project identity.
        project: ProjectInstanceId,
        /// Exact source path used by file-level assertions.
        api_path: String,
        /// Exact manifest path used by package-ownership assertions.
        manifest_path: String,
    }

    /// Publish every navigation family plus skewed inbound call context.
    fn publish_navigation_fixture(
        store: &mut AtlasStore,
        fingerprint: &str,
    ) -> Result<NavigationFixture, Box<dyn Error>> {
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("navigation fixture identity is missing"))?;
        let generation = IndexGeneration::new(1);
        let api_path = "src/auth/api.rs".to_string();
        let manifest_path = "Cargo.toml".to_string();
        let mut nodes = vec![
            graph_node(".", NodeKind::Folder, None),
            graph_node("src", NodeKind::Folder, Some(".")),
            graph_node("src/auth", NodeKind::Folder, Some("src")),
            graph_node(&api_path, NodeKind::File, Some("src/auth")),
            graph_node("src/auth/caller.rs", NodeKind::File, Some("src/auth")),
            graph_node("src/other.rs", NodeKind::File, Some("src")),
            graph_node("src/authz.rs", NodeKind::File, Some("src")),
            graph_node("tests", NodeKind::Folder, Some(".")),
            graph_node("tests/api_test.rs", NodeKind::File, Some("tests")),
            graph_node("clients", NodeKind::Folder, Some(".")),
            graph_node(&manifest_path, NodeKind::File, Some(".")),
        ];
        let api = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new(&api_path))?,
            },
            generation,
        )?;
        let internal_caller = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/auth/caller.rs"))?,
            },
            generation,
        )?;
        let other = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/other.rs"))?,
            },
            generation,
        )?;
        let sibling = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/authz.rs"))?,
            },
            generation,
        )?;
        let test = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("tests/api_test.rs"))?,
            },
            generation,
        )?;
        let package = GraphEntity::new(
            project,
            EntitySelector::Package {
                package: PackageSelector {
                    manager: GraphIdentityText::new("cargo")?,
                    name: GraphIdentityText::new("projectatlas-navigation")?,
                    manifest: RepositoryFilePath::new(Path::new(&manifest_path))?,
                },
            },
            generation,
        )?;
        let mut entities = vec![
            api.clone(),
            internal_caller.clone(),
            other.clone(),
            sibling.clone(),
            test.clone(),
            package.clone(),
        ];
        let mut relations = vec![
            LogicalRelation::new(
                &api,
                GraphRelationKind::Legacy(RelationKind::DependsOn),
                RelationResolution::resolved(&package)?,
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?,
            LogicalRelation::new(
                &api,
                GraphRelationKind::Legacy(RelationKind::Imports),
                RelationResolution::resolved(&other)?,
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?,
            LogicalRelation::new(
                &api,
                GraphRelationKind::Legacy(RelationKind::Calls),
                RelationResolution::resolved(&other)?,
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?,
            LogicalRelation::new(
                &api,
                GraphRelationKind::Extended(ExtendedRelationKind::References),
                RelationResolution::Unresolved {
                    reference: GraphIdentityText::new("SessionStore")?,
                },
                ConfidenceClass::High,
                Completeness::Partial,
                generation,
            )?,
            LogicalRelation::new(
                &test,
                GraphRelationKind::Extended(ExtendedRelationKind::Tests),
                RelationResolution::resolved(&api)?,
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?,
            LogicalRelation::new(
                &api,
                GraphRelationKind::Extended(ExtendedRelationKind::RoutesTo),
                RelationResolution::resolved(&other)?,
                ConfidenceClass::High,
                Completeness::Complete,
                generation,
            )?,
            LogicalRelation::new(
                &api,
                GraphRelationKind::Extended(ExtendedRelationKind::Configures),
                RelationResolution::Unresolved {
                    reference: GraphIdentityText::new("AUTH_MODE")?,
                },
                ConfidenceClass::High,
                Completeness::Partial,
                generation,
            )?,
            LogicalRelation::new(
                &internal_caller,
                GraphRelationKind::Legacy(RelationKind::Calls),
                RelationResolution::resolved(&api)?,
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?,
            LogicalRelation::new(
                &sibling,
                GraphRelationKind::Legacy(RelationKind::Imports),
                RelationResolution::resolved(&other)?,
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?,
        ];
        for index in 0..4 {
            let path = format!("clients/caller-{index}.rs");
            nodes.push(graph_node(&path, NodeKind::File, Some("clients")));
            let caller = GraphEntity::new(
                project,
                EntitySelector::File {
                    path: RepositoryFilePath::new(Path::new(&path))?,
                },
                generation,
            )?;
            relations.push(LogicalRelation::new(
                &caller,
                GraphRelationKind::Legacy(RelationKind::Calls),
                RelationResolution::resolved(&api)?,
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?);
            entities.push(caller);
        }

        let mut publication = store.begin_index_publication(fingerprint)?;
        publication.begin_scan_replacement()?;
        publication.upsert_scan_node_batch(&nodes)?;
        publication.finish_scan_replacement()?;
        publication.replace_repository_graph(project, &entities, &relations, &[], &[])?;
        publication.complete()?;
        Ok(NavigationFixture {
            project,
            api_path,
            manifest_path,
        })
    }

    /// Return a test failure without relying on panic-only assertions.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message.to_string()).into())
        }
    }

    /// Compare values while preserving useful failure context in fallible tests.
    fn require_eq<T: Debug + PartialEq>(
        actual: &T,
        expected: &T,
        context: &str,
    ) -> Result<(), Box<dyn Error>> {
        require(
            actual == expected,
            &format!("{context}: expected {expected:?}, found {actual:?}"),
        )
    }

    /// Require a database operation to fail and return its typed error.
    fn require_db_error<T>(result: DbResult<T>, message: &str) -> Result<DbError, Box<dyn Error>> {
        let Err(error) = result else {
            return Err(io::Error::other(message.to_string()).into());
        };
        Ok(error)
    }

    /// Prove cursor hydration seeks through stable-key indexes.
    fn assert_cursor_hydration_indexes(store: &AtlasStore) -> Result<(), Box<dyn Error>> {
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("bound fixture identity is missing"))?;
        let cases = [
            (
                "entity cursor hydration",
                graph_entity_hydration_sql(2),
                vec![
                    Value::Blob(vec![0; 32]),
                    Value::Blob(vec![1; 32]),
                    Value::Blob(project.as_bytes().to_vec()),
                ],
                "project_instance_id=? AND entity_key=?",
                "SCAN entity",
                false,
            ),
            (
                "relation cursor hydration",
                graph_relation_hydration_sql(2),
                vec![
                    Value::Blob(vec![0; 32]),
                    Value::Blob(vec![1; 32]),
                    Value::Blob(project.as_bytes().to_vec()),
                ],
                "idx_graph_relations_project_key",
                "SCAN relation",
                false,
            ),
            (
                "batched occurrence hydration",
                occurrence_pages_sql(2),
                vec![
                    Value::Blob(vec![0; 32]),
                    Value::Integer(2),
                    Value::Blob(vec![1; 32]),
                    Value::Integer(2),
                ],
                "sqlite_autoindex_graph_relation_occurrences_1",
                "SCAN graph_relation_occurrences",
                true,
            ),
            (
                "batched path coverage hydration",
                path_coverage_sql(2),
                vec![
                    Value::Blob(project.as_bytes().to_vec()),
                    Value::Text("path".to_string()),
                    Value::Text("Cargo.toml".to_string()),
                    Value::Text("src/Äuth.rs".to_string()),
                    Value::Integer(i64::from(GraphLimits::MAX_ROWS) + 1),
                ],
                "idx_graph_coverage_scope_order",
                "SCAN graph_coverage",
                false,
            ),
        ];
        for (context, sql, bindings, required_plan, forbidden_scan, allow_bounded_sort) in cases {
            let mut statement = store
                .connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
            let details = statement
                .query_map(params_from_iter(bindings.iter()), |row| {
                    row.get::<_, String>(3)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            require(
                details.iter().any(|detail| detail.contains(required_plan))
                    && details.iter().all(|detail| {
                        !detail.contains(forbidden_scan)
                            && (allow_bounded_sort || !detail.contains("USE TEMP B-TREE"))
                    }),
                &format!(
                    "{context} did not use {required_plan} without a scan or sort: {details:?}"
                ),
            )?;
        }
        Ok(())
    }

    /// Prove each normal graph query shape enters through its owning index.
    fn assert_query_indexes(store: &AtlasStore) -> Result<(), Box<dyn Error>> {
        let cases: &[(&str, &str, &[&str])] = &[
            (
                "entity path lookup",
                "EXPLAIN QUERY PLAN
                 SELECT entity_key FROM graph_entities
                  WHERE project_instance_id = zeroblob(16)
                    AND repository_path = 'src/Äuth.rs'
                  ORDER BY entity_kind, canonical_identity, entity_key
                  LIMIT 11",
                &["idx_graph_entities_path"],
            ),
            (
                "outbound relation lookup",
                "EXPLAIN QUERY PLAN
                 SELECT relation_key FROM graph_relations
                  WHERE source_entity_key = zeroblob(32)
                  ORDER BY relation_scope, relation_kind, canonical_identity, relation_key
                  LIMIT 11",
                &["idx_graph_relations_source_kind"],
            ),
            (
                "inbound relation lookup",
                "EXPLAIN QUERY PLAN
                 SELECT relation_key FROM graph_relations
                  WHERE target_entity_key = zeroblob(32)
                  ORDER BY relation_scope, relation_kind, canonical_identity, relation_key
                  LIMIT 11",
                &["idx_graph_relations_target_kind"],
            ),
            (
                "relation family lookup",
                "EXPLAIN QUERY PLAN
                 SELECT relation_key FROM graph_relations
                  WHERE project_instance_id = zeroblob(16)
                    AND relation_scope = 'legacy'
                    AND relation_kind = 'calls'
                  ORDER BY canonical_identity, relation_key
                  LIMIT 11",
                &["idx_graph_relations_kind_order"],
            ),
            (
                "relation occurrence lookup",
                "EXPLAIN QUERY PLAN
                 SELECT file_path FROM graph_relation_occurrences
                  WHERE relation_key = zeroblob(32)
                  ORDER BY file_path, start_line, start_column, end_line, end_column
                  LIMIT 11",
                &["sqlite_autoindex_graph_relation_occurrences_1"],
            ),
            (
                "occurrence path invalidation",
                "EXPLAIN QUERY PLAN
                 SELECT relation_key FROM graph_relation_occurrences
                  WHERE file_path = 'src'
                     OR (file_path >= 'src/' AND file_path < 'src0')",
                &["idx_graph_occurrences_file_span"],
            ),
            (
                "coverage path invalidation",
                "EXPLAIN QUERY PLAN
                 SELECT id FROM graph_coverage
                        INDEXED BY idx_graph_coverage_path
                  WHERE scope_kind = 'path'
                    AND (scope_path = 'src'
                     OR (scope_path >= 'src/' AND scope_path < 'src0'))",
                &["idx_graph_coverage_path"],
            ),
            (
                "entity repository-path invalidation",
                "EXPLAIN QUERY PLAN
                 SELECT entity_key FROM graph_entities
                  WHERE repository_path = 'src'
                     OR (repository_path >= 'src/' AND repository_path < 'src0')",
                &["idx_graph_entities_path"],
            ),
            (
                "entity manifest-path invalidation",
                "EXPLAIN QUERY PLAN
                 SELECT entity_key FROM graph_entities
                  WHERE manifest_path = 'src'
                     OR (manifest_path >= 'src/' AND manifest_path < 'src0')",
                &["idx_graph_entities_manifest_path"],
            ),
            (
                "outbound external cleanup candidate",
                "EXPLAIN QUERY PLAN
                 SELECT relation.target_entity_key
                   FROM graph_relations AS relation
                        INDEXED BY idx_graph_relations_source_kind
                   JOIN graph_entities AS external
                     ON external.entity_key = relation.target_entity_key
                  WHERE relation.source_entity_key = zeroblob(32)
                    AND external.entity_kind = 'external'",
                &["idx_graph_relations_source_kind"],
            ),
            (
                "inbound external cleanup candidate",
                "EXPLAIN QUERY PLAN
                 SELECT relation.source_entity_key
                   FROM graph_relations AS relation
                        INDEXED BY idx_graph_relations_target_kind
                   JOIN graph_entities AS external
                     ON external.entity_key = relation.source_entity_key
                  WHERE relation.target_entity_key = zeroblob(32)
                    AND external.entity_kind = 'external'",
                &["idx_graph_relations_target_kind"],
            ),
            (
                "candidate-bounded external cleanup",
                "EXPLAIN QUERY PLAN
                 DELETE FROM graph_entities
                  WHERE entity_key = zeroblob(32) AND entity_kind = 'external'
                    AND NOT EXISTS (
                        SELECT 1 FROM graph_relations
                               INDEXED BY idx_graph_relations_source_kind
                         WHERE source_entity_key = zeroblob(32)
                    )
                    AND NOT EXISTS (
                        SELECT 1 FROM graph_relations
                               INDEXED BY idx_graph_relations_target_kind
                         WHERE target_entity_key = zeroblob(32)
                    )",
                &[
                    "sqlite_autoindex_graph_entities_1",
                    "idx_graph_relations_source_kind",
                    "idx_graph_relations_target_kind",
                ],
            ),
            (
                "coverage scope lookup",
                "EXPLAIN QUERY PLAN
                 SELECT id FROM graph_coverage
                  WHERE project_instance_id = zeroblob(16)
                    AND scope_kind = 'path'
                    AND scope_path IS 'src'
                  ORDER BY relation_scope, relation_kind, state, id
                  LIMIT 11",
                &["idx_graph_coverage_scope_order"],
            ),
            (
                "resolution witness lookup",
                "EXPLAIN QUERY PLAN
                 SELECT canonical_identity FROM graph_resolution_keys
                  WHERE project_instance_id = zeroblob(16)
                    AND resolution_domain = 'declaration'
                    AND key_digest = zeroblob(32)",
                &["sqlite_autoindex_graph_resolution_keys_1"],
            ),
            (
                "resolution export lookup",
                "EXPLAIN QUERY PLAN
                 SELECT entity_key FROM graph_entity_exports
                  WHERE project_instance_id = zeroblob(16)
                    AND resolution_domain = 'declaration'
                    AND key_digest = zeroblob(32)
                  ORDER BY entity_key",
                &["idx_graph_entity_exports_key"],
            ),
            (
                "resolution export owner lookup",
                "EXPLAIN QUERY PLAN
                 SELECT resolution_domain, key_digest, entity_key
                   FROM graph_entity_exports
                  WHERE project_instance_id = zeroblob(16)
                    AND owner_path = 'src/Äuth.rs'
                  ORDER BY resolution_domain, key_digest, entity_key",
                &["idx_graph_entity_exports_owner"],
            ),
            (
                "resolution dependency lookup",
                "EXPLAIN QUERY PLAN
                 SELECT owner_path, relation_key FROM graph_relation_dependencies
                  WHERE project_instance_id = zeroblob(16)
                    AND resolution_domain = 'declaration'
                    AND key_digest = zeroblob(32)
                  ORDER BY owner_path, relation_key",
                &["idx_graph_relation_dependencies_key"],
            ),
            (
                "resolution dependency owner lookup",
                "EXPLAIN QUERY PLAN
                 SELECT resolution_domain, key_digest, relation_key
                   FROM graph_relation_dependencies
                  WHERE project_instance_id = zeroblob(16)
                    AND owner_path = 'src/Äuth.rs'
                  ORDER BY resolution_domain, key_digest, relation_key",
                &["idx_graph_relation_dependencies_owner"],
            ),
            (
                "relation composite integrity lookup",
                "EXPLAIN QUERY PLAN
                 SELECT relation_key FROM graph_relations
                  WHERE project_instance_id = zeroblob(16)
                    AND relation_key = zeroblob(32)",
                &["idx_graph_relations_project_key"],
            ),
        ];

        for (context, sql, required_indexes) in cases {
            let mut statement = store.connection.prepare(sql)?;
            let details = statement
                .query_map([], |row| row.get::<_, String>(3))?
                .collect::<Result<Vec<_>, _>>()?;
            require(
                required_indexes
                    .iter()
                    .all(|index| details.iter().any(|detail| detail.contains(index))),
                &format!("{context} did not use {required_indexes:?}; query plan was {details:?}"),
            )?;
            require(
                details.iter().all(|detail| {
                    !detail.contains("SCAN graph_") && !detail.contains("USE TEMP B-TREE")
                }),
                &format!("{context} was not bounded by index order: {details:?}"),
            )?;
        }
        Ok(())
    }

    /// Require one coverage-discovery query shape to seek through its owning indexes.
    fn assert_coverage_discovery_plan(
        connection: &Connection,
        sql: &str,
        values: &[Value],
        required_indexes: &[&str],
        allow_bounded_partial_sort: bool,
        context: &str,
    ) -> Result<(), Box<dyn Error>> {
        let mut statement = connection.prepare(sql)?;
        let details = statement
            .query_map(params_from_iter(values.iter()), |row| {
                row.get::<_, String>(3)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        require(
            required_indexes
                .iter()
                .all(|index| details.iter().any(|detail| detail.contains(index))),
            &format!("{context} did not use {required_indexes:?}; query plan was {details:?}"),
        )?;
        require(
            details.iter().all(|detail| {
                let uses_temporary_sort = detail.contains("USE TEMP B-TREE");
                let bounded_partial_sort = detail.contains("USE TEMP B-TREE FOR LAST");
                !detail.contains("SCAN coverage")
                    && (!uses_temporary_sort
                        || (allow_bounded_partial_sort && bounded_partial_sort))
            }),
            &format!("{context} used an unbounded scan or sort: {details:?}"),
        )?;
        Ok(())
    }

    /// Publish one complete fixture and its lexical source text.
    fn publish_fixture(
        store: &mut AtlasStore,
        fingerprint: &str,
    ) -> Result<GraphFixture, Box<dyn Error>> {
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("bound fixture identity is missing"))?;
        let mut fixture = graph_fixture(project, IndexGeneration::new(1))?;
        fixture.coverage.push(CoverageRecord::new(
            CoverageScope::Path {
                path: RepositoryNodePath::new(Path::new("src/Äuth.rs"))?,
            },
            None,
            CoverageState::Complete,
            1,
            0,
            IndexGeneration::new(1),
            None,
            None,
        )?);
        let mut occurrences = fixture.occurrences.clone();
        occurrences.push(fixture.occurrences[0].clone());
        let mut publication = store.begin_index_publication(fingerprint)?;
        publication.begin_scan_replacement()?;
        publication.upsert_scan_node_batch(&[
            graph_node(".", NodeKind::Folder, None),
            graph_node("src", NodeKind::Folder, Some(".")),
            graph_node("src/Äuth.rs", NodeKind::File, Some("src")),
            graph_node("Cargo.toml", NodeKind::File, Some(".")),
        ])?;
        publication.finish_scan_replacement()?;
        publication.replace_symbol_graph(&SymbolGraph {
            path: "src/Äuth.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::new(),
            relations: vec![SymbolRelation {
                path: "src/Äuth.rs".to_string(),
                source_name: "verifyToken".to_string(),
                target_name: "legacyTarget".to_string(),
                kind: RelationKind::Calls,
                line: 10,
                context: "legacyTarget()".to_string(),
                parser: ParserKind::TreeSitter,
            }],
        })?;
        publication.replace_file_texts_for_paths(
            &["src/Äuth.rs".to_string()],
            &[IndexedFileText {
                path: "src/Äuth.rs".to_string(),
                content_hash: Some("hash-old".to_string()),
                byte_count: 16,
                line_count: 1,
                content: "fn verifyToken()".to_string(),
            }],
        )?;
        publication.replace_repository_graph(
            fixture.project,
            &fixture.entities,
            &fixture.relations,
            &occurrences,
            &fixture.coverage,
        )?;
        publication.complete()?;
        Ok(fixture)
    }

    #[test]
    fn resolution_keys_round_trip_reopen_and_preserve_all_dependency_states()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_root = temp.path().join("resolution-round-trip");
        let atlas_dir = project_root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let db_path = atlas_dir.join("projectatlas.db");
        let mut writer = AtlasStore::open_for_project(&db_path, &project_root)?;
        let fixture = publish_resolution_fixture(&mut writer, "resolution-round-trip")?;
        assert_query_indexes(&writer)?;

        let counts = writer.connection.query_row(
            "SELECT
                 (SELECT COUNT(*) FROM graph_resolution_keys),
                 (SELECT COUNT(*) FROM graph_entity_exports),
                 (SELECT COUNT(*) FROM graph_relation_dependencies)",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        require_eq(&counts, &(3, 1, 3), "deduplicated resolution-key rows")?;
        let states = writer
            .connection
            .prepare(
                "SELECT relation.resolution_status, COUNT(*)
                   FROM graph_relation_dependencies AS dependency
                   JOIN graph_relations AS relation
                     ON relation.project_instance_id = dependency.project_instance_id
                    AND relation.relation_key = dependency.relation_key
                  GROUP BY relation.resolution_status
                  ORDER BY relation.resolution_status",
            )?
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        require_eq(
            &states,
            &vec![
                ("ambiguous".to_string(), 1),
                ("resolved".to_string(), 1),
                ("unresolved".to_string(), 1),
            ],
            "dependency resolution states",
        )?;
        drop(writer);

        let reader = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
        let exports = reader.repository_export_keys_for_paths(
            fixture.graph.project,
            &["src/Äuth.rs".to_string()],
            10,
        )?;
        require_eq(&exports.truncated, &false, "export key truncation")?;
        require_eq(
            &exports.rows,
            &vec![fixture.resolved.clone()],
            "export key round trip",
        )?;
        let candidates = reader.repository_resolution_candidates(&fixture.resolved, 10)?;
        require_eq(&candidates.truncated, &false, "candidate truncation")?;
        require_eq(
            &candidates.rows,
            &vec![fixture.graph.entities[4].clone()],
            "candidate export round trip",
        )?;
        let batch_candidates = reader.repository_resolution_candidates_for_keys(
            fixture.graph.project,
            &[
                fixture.unresolved.clone(),
                fixture.resolved.clone(),
                fixture.ambiguous.clone(),
                fixture.resolved.clone(),
            ],
            10,
        )?;
        require_eq(
            &batch_candidates.truncated,
            &false,
            "batch candidate truncation",
        )?;
        require_eq(
            &batch_candidates.rows.len(),
            &1,
            "batch candidate deduplication",
        )?;
        require_eq(
            batch_candidates.rows[0].key(),
            &fixture.resolved,
            "batch candidate selecting key",
        )?;
        require_eq(
            batch_candidates.rows[0].entity(),
            &fixture.graph.entities[4],
            "batch candidate entity",
        )?;
        let affected = reader.repository_affected_source_paths(
            fixture.graph.project,
            &[
                fixture.resolved.clone(),
                fixture.resolved,
                fixture.ambiguous,
                fixture.unresolved,
            ],
            10,
        )?;
        require_eq(&affected.truncated, &false, "affected path truncation")?;
        require_eq(
            &affected.rows,
            &vec![RepositoryFilePath::new(Path::new("src/Äuth.rs"))?],
            "resolved ambiguous and unresolved dependency owners",
        )?;
        reader.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn high_degree_dependency_closure_is_unique_and_reports_overflow_before_mutation()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_root = temp.path().join("high-degree-resolution");
        fs::create_dir_all(&project_root)?;
        let db_path = project_root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&db_path, &project_root)?;
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("bound identity is missing"))?;
        let generation = IndexGeneration::new(1);
        let dependency_key = declaration_key(
            project,
            "sharedTarget",
            GraphRelationKind::Extended(ExtendedRelationKind::References),
        )?;
        let mut nodes = vec![
            graph_node(".", NodeKind::Folder, None),
            graph_node("src", NodeKind::Folder, Some(".")),
        ];
        let mut entities = Vec::new();
        let mut relations = Vec::new();
        let mut dependencies = Vec::new();
        for index in 0..5 {
            let path = format!("src/caller-{index}.rs");
            nodes.push(graph_node(&path, NodeKind::File, Some("src")));
            let entity = GraphEntity::new(
                project,
                EntitySelector::File {
                    path: RepositoryFilePath::new(Path::new(&path))?,
                },
                generation,
            )?;
            let relation = LogicalRelation::new(
                &entity,
                GraphRelationKind::Extended(ExtendedRelationKind::References),
                RelationResolution::Unresolved {
                    reference: GraphIdentityText::new("sharedTarget")?,
                },
                ConfidenceClass::High,
                Completeness::Complete,
                generation,
            )?;
            dependencies.push(RelationDependencyKey::new(
                relation.key().clone(),
                dependency_key.clone(),
            )?);
            entities.push(entity);
            relations.push(relation);
        }
        dependencies.push(dependencies[0].clone());
        let mut publication = store.begin_index_publication("high-degree-resolution")?;
        publication.begin_scan_replacement()?;
        publication.upsert_scan_node_batch(&nodes)?;
        publication.finish_scan_replacement()?;
        publication.replace_repository_graph_with_resolution_keys(
            project,
            &entities,
            &relations,
            &[],
            &[],
            &[],
            &dependencies,
        )?;
        publication.complete()?;

        let before = store.index_publication()?;
        let bounded = store.repository_affected_source_paths(
            project,
            std::slice::from_ref(&dependency_key),
            2,
        )?;
        require_eq(&bounded.rows.len(), &2, "bounded affected path count")?;
        require_eq(&bounded.truncated, &true, "bounded affected path overflow")?;
        require_eq(
            &store.index_publication()?,
            &before,
            "overflow lookup mutated publication state",
        )?;
        let complete = store.repository_affected_source_paths(project, &[dependency_key], 10)?;
        require_eq(&complete.rows.len(), &5, "complete affected path count")?;
        require_eq(
            &complete.truncated,
            &false,
            "complete affected path overflow",
        )?;
        let unique = complete.rows.iter().collect::<BTreeSet<_>>();
        require_eq(&unique.len(), &5, "affected paths are unique")?;
        Ok(())
    }

    #[test]
    fn affected_source_footprint_accounts_exact_owned_rows_and_uses_path_indexes()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_root = temp.path().join("affected-source-footprint");
        fs::create_dir_all(&project_root)?;
        let db_path = project_root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&db_path, &project_root)?;
        let fixture = publish_resolution_fixture(&mut store, "affected-source-footprint")?;
        replace_fixture_symbol_rows(&mut store, "src/Äuth.rs", 1)?;
        store.connection.execute(
            "INSERT INTO graph_coverage(
                 project_instance_id, scope_kind, scope_path, relation_scope,
                 relation_kind, state, total, covered, omitted, reason, reached_limit
             ) VALUES(?1, 'path', 'src/Äuth.rs', NULL, NULL,
                      'complete', 1, 1, 0, NULL, NULL)",
            [&fixture.graph.project.as_bytes()[..]],
        )?;

        let mut paths = (0..=RESOLUTION_PATHS_PER_QUERY)
            .map(|index| format!("missing/{index:04}.rs"))
            .collect::<Vec<_>>();
        paths.extend(["src/Äuth.rs".to_string(), "src/Äuth.rs".to_string()]);
        let footprint =
            store.repository_affected_source_footprint(fixture.graph.project, &paths, 100)?;
        require_eq(&footprint.rows, &20, "exact affected persisted rows")?;
        require(
            footprint.retained_bytes > footprint.rows,
            "affected footprint omitted decoded bytes",
        )?;
        require_eq(&footprint.truncated, &false, "exact footprint truncation")?;

        let sql = format!("EXPLAIN QUERY PLAN {}", affected_source_footprint_sql(1));
        let values = [
            Value::Blob(fixture.graph.project.as_bytes().to_vec()),
            Value::Text("src/Äuth.rs".to_string()),
            Value::Integer(101),
        ];
        let mut statement = store.connection.prepare(&sql)?;
        let details = statement
            .query_map(params_from_iter(values.iter()), |row| {
                row.get::<_, String>(3)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for index in [
            "sqlite_autoindex_source_parse_metadata_1",
            "idx_symbols_path",
            "idx_symbol_relations_path",
            "idx_graph_entities_path",
            "idx_graph_entities_manifest_path",
            "idx_graph_relations_source_kind",
            "idx_graph_occurrences_file_span",
            "idx_graph_coverage_path",
            "idx_graph_entity_exports_owner",
            "idx_graph_relation_dependencies_owner",
            "sqlite_autoindex_graph_resolution_keys_1",
        ] {
            require(
                details.iter().any(|detail| detail.contains(index)),
                &format!("affected footprint missed {index}; plan was {details:?}"),
            )?;
        }
        require(
            details.iter().all(|detail| !detail.contains("SCAN graph_")),
            &format!("affected footprint scanned graph storage: {details:?}"),
        )?;
        Ok(())
    }

    #[test]
    fn affected_source_footprint_reports_high_degree_overflow_before_mutation()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_root = temp.path().join("affected-source-degree");
        fs::create_dir_all(&project_root)?;
        let db_path = project_root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&db_path, &project_root)?;
        let fixture = publish_resolution_fixture(&mut store, "affected-source-degree")?;
        replace_fixture_symbol_rows(&mut store, "src/Äuth.rs", 25)?;
        let before = store.index_publication()?;

        let bounded = store.repository_affected_source_footprint(
            fixture.graph.project,
            &["src/Äuth.rs".to_string()],
            5,
        )?;
        require_eq(&bounded.rows, &6, "footprint limit plus one sentinel")?;
        require_eq(&bounded.truncated, &true, "high-degree footprint overflow")?;
        require(
            bounded.retained_bytes > 0,
            "bounded footprint lost retained bytes",
        )?;
        require_eq(
            &store.index_publication()?,
            &before,
            "footprint overflow mutated publication",
        )?;
        Ok(())
    }

    #[test]
    fn affected_source_footprint_validates_inputs_identity_and_graph_availability()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let selected_root = temp.path().join("selected");
        let other_root = temp.path().join("other");
        fs::create_dir_all(&selected_root)?;
        fs::create_dir_all(&other_root)?;
        let mut selected =
            AtlasStore::open_for_project(&selected_root.join("projectatlas.db"), &selected_root)?;
        let fixture = publish_resolution_fixture(&mut selected, "footprint-validation")?;
        let other = AtlasStore::open_for_project(&other_root.join("projectatlas.db"), &other_root)?;
        let other_project = other
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("other bound identity is missing"))?;

        let invalid_limit = require_db_error(
            selected.repository_affected_source_footprint(
                fixture.graph.project,
                &["src/Äuth.rs".to_string()],
                0,
            ),
            "zero footprint limit was accepted",
        )?;
        require(
            matches!(invalid_limit, DbError::GraphContract(_)),
            &format!("invalid footprint limit returned {invalid_limit}"),
        )?;
        let invalid_path = require_db_error(
            selected.repository_affected_source_footprint(
                fixture.graph.project,
                &["../outside.rs".to_string()],
                10,
            ),
            "escaping footprint path was accepted",
        )?;
        require(
            matches!(invalid_path, DbError::GraphContract(_)),
            &format!("invalid footprint path returned {invalid_path}"),
        )?;
        let mismatched = require_db_error(
            selected.repository_affected_source_footprint(
                other_project,
                &["src/Äuth.rs".to_string()],
                10,
            ),
            "mismatched project footprint was accepted",
        )?;
        require(
            matches!(mismatched, DbError::GraphProjectIdentityMismatch { .. }),
            &format!("mismatched footprint returned {mismatched}"),
        )?;
        require_eq(
            &other.repository_affected_source_footprint(
                other_project,
                &["src/Äuth.rs".to_string()],
                10,
            )?,
            &empty_affected_source_footprint(),
            "unpublished graph footprint",
        )?;
        Ok(())
    }

    #[test]
    fn affected_source_footprint_rejects_missing_resolution_witnesses() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let project_root = temp.path().join("affected-source-corruption");
        fs::create_dir_all(&project_root)?;
        let db_path = project_root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&db_path, &project_root)?;
        let fixture = publish_resolution_fixture(&mut store, "affected-source-corruption")?;
        store
            .connection
            .execute_batch("PRAGMA foreign_keys = OFF")?;
        store.connection.execute(
            "DELETE FROM graph_resolution_keys
              WHERE project_instance_id = ?1
                AND resolution_domain = ?2
                AND key_digest = ?3",
            params![
                &fixture.graph.project.as_bytes()[..],
                fixture.resolved.domain().as_str(),
                &fixture.resolved.digest_bytes()[..],
            ],
        )?;
        let error = require_db_error(
            store.repository_affected_source_footprint(
                fixture.graph.project,
                &["src/Äuth.rs".to_string()],
                100,
            ),
            "missing resolution witness returned a partial footprint",
        )?;
        require(
            matches!(error, DbError::Sqlite(_)),
            &format!("missing witness returned the wrong error: {error}"),
        )?;
        Ok(())
    }

    #[test]
    fn resolution_key_failures_roll_back_and_owner_foreign_keys_cascade()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_root = temp.path().join("resolution-failures");
        fs::create_dir_all(&project_root)?;
        let db_path = project_root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&db_path, &project_root)?;
        let fixture = publish_resolution_fixture(&mut store, "resolution-failures")?;
        let before_publication = store.index_publication()?;
        let before_counts = store.connection.query_row(
            "SELECT
                 (SELECT COUNT(*) FROM graph_resolution_keys),
                 (SELECT COUNT(*) FROM graph_entity_exports),
                 (SELECT COUNT(*) FROM graph_relation_dependencies)",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;

        let replacement = graph_fixture(fixture.graph.project, IndexGeneration::new(2))?;
        let invalid_export = EntityResolutionKey::new(
            replacement.entities[0].key().clone(),
            fixture.resolved.clone(),
        )?;
        {
            let mut publication = store.begin_index_publication("resolution-failures")?;
            let error = require_db_error(
                publication.replace_repository_graph_with_resolution_keys(
                    replacement.project,
                    &replacement.entities,
                    &replacement.relations,
                    &replacement.occurrences,
                    &replacement.coverage,
                    &[invalid_export],
                    &[],
                ),
                "source-less export owner unexpectedly published",
            )?;
            require(
                matches!(error, DbError::GraphRowShape { .. }),
                &format!("invalid export owner returned the wrong error: {error}"),
            )?;
        }
        require_eq(
            &store.index_publication()?,
            &before_publication,
            "failed key publication generation",
        )?;
        let after_counts = store.connection.query_row(
            "SELECT
                 (SELECT COUNT(*) FROM graph_resolution_keys),
                 (SELECT COUNT(*) FROM graph_entity_exports),
                 (SELECT COUNT(*) FROM graph_relation_dependencies)",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        require_eq(
            &after_counts,
            &before_counts,
            "failed key publication durable rows",
        )?;
        require_eq(
            &store
                .repository_resolution_candidates(&fixture.resolved, 10)?
                .rows,
            &vec![fixture.graph.entities[4].clone()],
            "failed key publication previous candidates",
        )?;

        store.connection.execute(
            "UPDATE graph_resolution_keys
                SET canonical_identity = 'conflicting collision witness'
              WHERE project_instance_id = ?1
                AND resolution_domain = ?2
                AND key_digest = ?3",
            params![
                &fixture.graph.project.as_bytes()[..],
                fixture.resolved.domain().as_str(),
                &fixture.resolved.digest_bytes()[..],
            ],
        )?;
        let collision = require_db_error(
            store.repository_resolution_candidates(&fixture.resolved, 10),
            "conflicting resolution witness was accepted",
        )?;
        require(
            matches!(collision, DbError::ResolutionKeyCollision { .. }),
            &format!("conflicting witness returned the wrong error: {collision}"),
        )?;
        let corrupt_page = require_db_error(
            store.repository_export_keys_for_paths(
                fixture.graph.project,
                &["src/Äuth.rs".to_string()],
                10,
            ),
            "corrupt witness returned a partial key page",
        )?;
        require(
            matches!(corrupt_page, DbError::GraphContract(_)),
            &format!("corrupt witness returned the wrong page error: {corrupt_page}"),
        )?;
        store.connection.execute(
            "UPDATE graph_resolution_keys
                SET canonical_identity = ?1
              WHERE project_instance_id = ?2
                AND resolution_domain = ?3
                AND key_digest = ?4",
            params![
                fixture.resolved.canonical_identity(),
                &fixture.graph.project.as_bytes()[..],
                fixture.resolved.domain().as_str(),
                &fixture.resolved.digest_bytes()[..],
            ],
        )?;

        let relation_digest = fixture.graph.relations[2].key().digest_bytes()?;
        store.connection.execute(
            "DELETE FROM graph_relations
              WHERE project_instance_id = ?1 AND relation_key = ?2",
            params![&fixture.graph.project.as_bytes()[..], &relation_digest[..]],
        )?;
        let remaining_dependencies = store.connection.query_row(
            "SELECT COUNT(*) FROM graph_relation_dependencies",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        require_eq(
            &remaining_dependencies,
            &(before_counts.2 - 1),
            "relation-owner dependency cascade",
        )?;
        let entity_digest = fixture.graph.entities[4].key().digest_bytes()?;
        store.connection.execute(
            "DELETE FROM graph_entities
              WHERE project_instance_id = ?1 AND entity_key = ?2",
            params![&fixture.graph.project.as_bytes()[..], &entity_digest[..]],
        )?;
        let remaining_exports =
            store
                .connection
                .query_row("SELECT COUNT(*) FROM graph_entity_exports", [], |row| {
                    row.get::<_, i64>(0)
                })?;
        require_eq(&remaining_exports, &0, "entity-owner export cascade")?;
        Ok(())
    }

    #[test]
    fn affected_replacement_swaps_resolution_keys_and_collects_only_touched_orphans()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_root = temp.path().join("resolution-replacement");
        fs::create_dir_all(&project_root)?;
        let db_path = project_root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&db_path, &project_root)?;
        let first = publish_resolution_fixture(&mut store, "resolution-replacement")?;
        let replacement = graph_fixture(first.graph.project, IndexGeneration::new(2))?;
        let renamed = declaration_key(
            first.graph.project,
            "verifySession",
            GraphRelationKind::Legacy(RelationKind::Calls),
        )?;
        let exports = vec![EntityResolutionKey::new(
            replacement.entities[4].key().clone(),
            renamed.clone(),
        )?];
        let dependencies = vec![
            RelationDependencyKey::new(replacement.relations[0].key().clone(), renamed.clone())?,
            RelationDependencyKey::new(
                replacement.relations[1].key().clone(),
                first.ambiguous.clone(),
            )?,
            RelationDependencyKey::new(
                replacement.relations[2].key().clone(),
                first.unresolved.clone(),
            )?,
        ];
        let mut publication = store.begin_index_publication("resolution-replacement")?;
        publication.replace_repository_graph_for_paths_with_resolution_keys(
            replacement.project,
            &["src/Äuth.rs".to_string()],
            &replacement.entities,
            &replacement.relations,
            &replacement.occurrences,
            &replacement.coverage,
            &exports,
            &dependencies,
        )?;
        publication.complete()?;

        require_eq(
            &store
                .repository_resolution_candidates(&first.resolved, 10)?
                .rows
                .len(),
            &0,
            "removed export candidates",
        )?;
        require_eq(
            &store
                .repository_affected_source_paths(
                    first.graph.project,
                    std::slice::from_ref(&first.resolved),
                    10,
                )?
                .rows
                .len(),
            &0,
            "removed dependency owners",
        )?;
        require_eq(
            &store.repository_resolution_candidates(&renamed, 10)?.rows,
            &vec![replacement.entities[4].clone()],
            "renamed export candidates",
        )?;
        let exports = store.repository_export_keys_for_paths(
            first.graph.project,
            &["src/Äuth.rs".to_string()],
            10,
        )?;
        require_eq(&exports.rows, &vec![renamed], "replacement export keys")?;
        let registry_rows = store.connection.query_row(
            "SELECT COUNT(*) FROM graph_resolution_keys",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        require_eq(&registry_rows, &3, "touched orphan witness cleanup")?;
        require_eq(
            &store
                .index_publication()?
                .ok_or_else(|| io::Error::other("replacement publication is missing"))?
                .generation,
            &IndexGeneration::new(2),
            "replacement generation",
        )?;
        Ok(())
    }

    #[test]
    fn affected_graph_replacement_preserves_only_the_unaffected_closure()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_root = temp.path().join("affected-closure");
        let atlas_dir = project_root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let db_path = atlas_dir.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&db_path, &project_root)?;

        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("bound affected-closure identity is missing"))?;
        let generation_one = IndexGeneration::new(1);
        let project_entity = GraphEntity::new(project, EntitySelector::Project, generation_one)?;
        let affected_folder = GraphEntity::new(
            project,
            EntitySelector::Folder {
                path: RepositoryNodePath::new(Path::new("src/a"))?,
            },
            generation_one,
        )?;
        let affected_file = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/a/local.rs"))?,
            },
            generation_one,
        )?;
        let case_distinct_file = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/A/keep.rs"))?,
            },
            generation_one,
        )?;
        let package = GraphEntity::new(
            project,
            EntitySelector::Package {
                package: PackageSelector {
                    manager: GraphIdentityText::new("cargo")?,
                    name: GraphIdentityText::new("api")?,
                    manifest: RepositoryFilePath::new(Path::new("packages/api/Cargo.toml"))?,
                },
            },
            generation_one,
        )?;
        let orphan_external = GraphEntity::new(
            project,
            EntitySelector::External {
                external: ExternalSelector {
                    system: GraphIdentityText::new("crates.io")?,
                    identity: GraphIdentityText::new("orphan@1")?,
                },
            },
            generation_one,
        )?;
        let retained_external = GraphEntity::new(
            project,
            EntitySelector::External {
                external: ExternalSelector {
                    system: GraphIdentityText::new("crates.io")?,
                    identity: GraphIdentityText::new("retained@1")?,
                },
            },
            generation_one,
        )?;
        let occurrence_owned_external = GraphEntity::new(
            project,
            EntitySelector::External {
                external: ExternalSelector {
                    system: GraphIdentityText::new("crates.io")?,
                    identity: GraphIdentityText::new("occurrence-owned@1")?,
                },
            },
            generation_one,
        )?;
        let affected_relation = LogicalRelation::new(
            &affected_file,
            GraphRelationKind::Legacy(RelationKind::DependsOn),
            RelationResolution::external(&orphan_external)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation_one,
        )?;
        let package_relation = LogicalRelation::new(
            &package,
            GraphRelationKind::Legacy(RelationKind::DependsOn),
            RelationResolution::external(&orphan_external)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation_one,
        )?;
        let retained_relation = LogicalRelation::new(
            &case_distinct_file,
            GraphRelationKind::Legacy(RelationKind::DependsOn),
            RelationResolution::external(&retained_external)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation_one,
        )?;
        let project_external_relation = LogicalRelation::new(
            &project_entity,
            GraphRelationKind::Legacy(RelationKind::DependsOn),
            RelationResolution::external(&retained_external)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation_one,
        )?;
        let occurrence_backed_project_relation = LogicalRelation::new(
            &project_entity,
            GraphRelationKind::Legacy(RelationKind::DependsOn),
            RelationResolution::external(&occurrence_owned_external)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation_one,
        )?;
        let affected_occurrence = RelationOccurrence::new(
            &affected_relation,
            RepositoryFilePath::new(Path::new("src/a/local.rs"))?,
            SourceSpan::new(3, 0, 3, 12)?,
            generation_one,
        )?;
        let retained_occurrence = RelationOccurrence::new(
            &retained_relation,
            RepositoryFilePath::new(Path::new("src/A/keep.rs"))?,
            SourceSpan::new(5, 0, 5, 14)?,
            generation_one,
        )?;
        let retained_relation_affected_occurrence = RelationOccurrence::new(
            &retained_relation,
            RepositoryFilePath::new(Path::new("src/a/local.rs"))?,
            SourceSpan::new(6, 0, 6, 14)?,
            generation_one,
        )?;
        let project_relation_occurrence = RelationOccurrence::new(
            &occurrence_backed_project_relation,
            RepositoryFilePath::new(Path::new("src/a/local.rs"))?,
            SourceSpan::new(9, 0, 9, 16)?,
            generation_one,
        )?;
        let initial_coverage = vec![
            CoverageRecord::new(
                CoverageScope::Project,
                None,
                CoverageState::Complete,
                4,
                0,
                generation_one,
                None,
                None,
            )?,
            CoverageRecord::new(
                CoverageScope::Path {
                    path: RepositoryNodePath::new(Path::new("src/a"))?,
                },
                None,
                CoverageState::Partial,
                1,
                1,
                generation_one,
                Some(GraphIdentityText::new("affected coverage")?),
                Some(GraphLimitKind::Rows),
            )?,
            CoverageRecord::new(
                CoverageScope::Path {
                    path: RepositoryNodePath::new(Path::new("src/A"))?,
                },
                None,
                CoverageState::Complete,
                1,
                0,
                generation_one,
                None,
                None,
            )?,
        ];
        {
            let mut publication = store.begin_index_publication("affected-closure")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&[
                graph_node(".", NodeKind::Folder, None),
                graph_node("src", NodeKind::Folder, Some(".")),
                graph_node("src/a", NodeKind::Folder, Some("src")),
                graph_node("src/a/local.rs", NodeKind::File, Some("src/a")),
                graph_node("src/a/new.rs", NodeKind::File, Some("src/a")),
                graph_node("src/A", NodeKind::Folder, Some("src")),
                graph_node("src/A/keep.rs", NodeKind::File, Some("src/A")),
                graph_node("packages", NodeKind::Folder, Some(".")),
                graph_node("packages/api", NodeKind::Folder, Some("packages")),
                graph_node(
                    "packages/api/Cargo.toml",
                    NodeKind::File,
                    Some("packages/api"),
                ),
                graph_node("README.md", NodeKind::File, Some(".")),
            ])?;
            publication.finish_scan_replacement()?;
            publication.replace_repository_graph(
                project,
                &[
                    project_entity.clone(),
                    affected_folder,
                    affected_file.clone(),
                    case_distinct_file.clone(),
                    package.clone(),
                    orphan_external.clone(),
                    retained_external.clone(),
                    occurrence_owned_external.clone(),
                ],
                &[
                    affected_relation,
                    package_relation,
                    retained_relation,
                    project_external_relation,
                    occurrence_backed_project_relation,
                ],
                &[
                    affected_occurrence,
                    retained_occurrence,
                    retained_relation_affected_occurrence,
                    project_relation_occurrence,
                ],
                &initial_coverage,
            )?;
            publication.complete()?;
        }

        let generation_two = IndexGeneration::new(2);
        let replacement_folder = GraphEntity::new(
            project,
            EntitySelector::Folder {
                path: RepositoryNodePath::new(Path::new("src/a"))?,
            },
            generation_two,
        )?;
        let replacement_file = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/a/new.rs"))?,
            },
            generation_two,
        )?;
        let retained_external_for_relation = GraphEntity::new(
            project,
            retained_external.selector().clone(),
            generation_two,
        )?;
        let replacement_relation = LogicalRelation::new(
            &replacement_file,
            GraphRelationKind::Legacy(RelationKind::DependsOn),
            RelationResolution::external(&retained_external_for_relation)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation_two,
        )?;
        let replacement_occurrence = RelationOccurrence::new(
            &replacement_relation,
            RepositoryFilePath::new(Path::new("src/a/new.rs"))?,
            SourceSpan::new(7, 0, 7, 10)?,
            generation_two,
        )?;
        let replacement_coverage = CoverageRecord::new(
            CoverageScope::Path {
                path: RepositoryNodePath::new(Path::new("src/a"))?,
            },
            None,
            CoverageState::Complete,
            1,
            0,
            generation_two,
            None,
            None,
        )?;
        {
            let mut publication = store.begin_index_publication("affected-closure")?;
            publication.replace_repository_graph_for_paths(
                project,
                &["src/a".to_string(), "packages/api/Cargo.toml".to_string()],
                &[replacement_folder, replacement_file.clone()],
                &[replacement_relation],
                &[replacement_occurrence],
                &[replacement_coverage],
            )?;
            publication.complete()?;
        }

        drop(store);
        let store = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;

        require_eq(
            &store.repository_graph_entity(affected_file.key())?,
            &None,
            "affected descendant removal",
        )?;
        require_eq(
            &store.repository_graph_entity(package.key())?,
            &None,
            "manifest-owned package removal",
        )?;
        require_eq(
            &store.repository_graph_entity(orphan_external.key())?,
            &None,
            "candidate-bounded orphan external cleanup",
        )?;
        require_eq(
            &store.repository_graph_entity(occurrence_owned_external.key())?,
            &None,
            "final affected occurrence relation and external cleanup",
        )?;
        let preserved_case = store
            .repository_graph_entity(case_distinct_file.key())?
            .ok_or_else(|| io::Error::other("case-distinct sibling was removed"))?;
        require_eq(
            &preserved_case.generation(),
            &generation_two,
            "case-distinct sibling generation injection",
        )?;
        let preserved_external = store
            .repository_graph_entity(retained_external.key())?
            .ok_or_else(|| io::Error::other("referenced external entity was removed"))?;
        require_eq(
            &preserved_external.generation(),
            &generation_two,
            "unaffected external generation injection",
        )?;
        let retained_relations = store.repository_graph_relations(
            RepositoryGraphRelationQuery::Outbound {
                source: case_distinct_file.key().clone(),
            },
            10,
        )?;
        require_eq(
            &retained_relations.rows.len(),
            &1,
            "relation with one unaffected occurrence",
        )?;
        require_eq(
            &store
                .repository_graph_occurrences(&retained_relations.rows[0], 10)?
                .rows
                .len(),
            &1,
            "only the affected occurrence was removed",
        )?;
        let replacement_relations = store.repository_graph_relations(
            RepositoryGraphRelationQuery::Outbound {
                source: replacement_file.key().clone(),
            },
            10,
        )?;
        require_eq(
            &replacement_relations.rows.len(),
            &1,
            "replacement relation count",
        )?;
        require_eq(
            &store
                .repository_graph_occurrences(&replacement_relations.rows[0], 10)?
                .rows
                .len(),
            &1,
            "replacement source occurrence",
        )?;
        let affected_coverage = store.repository_graph_coverage(
            project,
            &CoverageScope::Path {
                path: RepositoryNodePath::new(Path::new("src/a"))?,
            },
            10,
        )?;
        require(
            affected_coverage.rows.len() == 1
                && affected_coverage.rows[0].state() == CoverageState::Complete,
            "affected path coverage was not replaced",
        )?;
        let case_coverage = store.repository_graph_coverage(
            project,
            &CoverageScope::Path {
                path: RepositoryNodePath::new(Path::new("src/A"))?,
            },
            10,
        )?;
        require_eq(
            &case_coverage.rows.len(),
            &1,
            "case-distinct coverage preservation",
        )?;
        require_eq(
            &store
                .repository_graph_coverage(project, &CoverageScope::Project, 10)?
                .rows
                .len(),
            &1,
            "unaffected project coverage preservation",
        )?;
        require_eq(
            &store
                .repository_graph_relations(
                    RepositoryGraphRelationQuery::Outbound {
                        source: project_entity.key().clone(),
                    },
                    10,
                )?
                .rows
                .len(),
            &1,
            "project-to-external relation preservation",
        )?;

        store.finish_index_read_snapshot()?;
        drop(store);
        let mut store = AtlasStore::open_for_project(&db_path, &project_root)?;
        let generation_three = IndexGeneration::new(3);
        let root_project = GraphEntity::new(project, EntitySelector::Project, generation_three)?;
        let readme = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("README.md"))?,
            },
            generation_three,
        )?;
        let root_coverage = CoverageRecord::new(
            CoverageScope::Project,
            None,
            CoverageState::Complete,
            1,
            0,
            generation_three,
            None,
            None,
        )?;
        {
            let mut publication = store.begin_index_publication("affected-closure")?;
            publication.replace_repository_graph_for_paths(
                project,
                &[".".to_string()],
                &[root_project.clone(), readme],
                &[],
                &[],
                &[root_coverage],
            )?;
            publication.complete()?;
        }
        drop(store);
        let store = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
        require_eq(
            &store.repository_graph_entity(case_distinct_file.key())?,
            &None,
            "root replacement stale local removal",
        )?;
        require_eq(
            &store.repository_graph_entity(retained_external.key())?,
            &None,
            "root replacement external removal",
        )?;
        require_eq(
            &store
                .repository_graph_relations(
                    RepositoryGraphRelationQuery::Outbound {
                        source: root_project.key().clone(),
                    },
                    10,
                )?
                .rows
                .len(),
            &0,
            "root replacement project relation removal",
        )?;
        require_eq(
            &store
                .repository_graph_coverage(project, &CoverageScope::Project, 10)?
                .rows
                .len(),
            &1,
            "root replacement project coverage",
        )?;
        require_eq(
            &store
                .repository_graph_coverage(
                    project,
                    &CoverageScope::Path {
                        path: RepositoryNodePath::new(Path::new("src/A"))?,
                    },
                    10,
                )?
                .rows
                .len(),
            &0,
            "root replacement path coverage removal",
        )?;

        store.finish_index_read_snapshot()?;
        drop(store);
        let mut store = AtlasStore::open_for_project(&db_path, &project_root)?;
        {
            let mut projection = store.begin_index_projection_refresh("affected-closure")?;
            projection.replace_file_texts_for_paths(
                &["README.md".to_string()],
                &[IndexedFileText {
                    path: "README.md".to_string(),
                    content_hash: Some("readme-hash".to_string()),
                    byte_count: 7,
                    line_count: 1,
                    content: "# Atlas".to_string(),
                }],
            )?;
            projection.complete()?;
        }
        drop(store);
        let store = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
        let error = require_db_error(
            store.repository_graph_entity(root_project.key()),
            "non-graph publication blessed stale graph rows",
        )?;
        require(
            matches!(
                error,
                DbError::GraphRowShape {
                    table: "project_identity",
                    ..
                }
            ),
            &format!("unexpected stale graph generation error: {error}"),
        )?;
        store.finish_index_read_snapshot()?;
        Ok(())
    }

    /// Assert one reader sees a complete internally consistent graph projection.
    fn require_graph_projection(
        store: &AtlasStore,
        fixture: &GraphFixture,
        generation: IndexGeneration,
        lexical_content: &str,
    ) -> Result<(), Box<dyn Error>> {
        let publication = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("graph publication metadata missing"))?;
        require_eq(
            &publication.state,
            &IndexPublicationState::Complete,
            "graph publication state",
        )?;
        require_eq(
            &publication.generation,
            &generation,
            "graph publication generation",
        )?;
        let source = fixture
            .entities
            .iter()
            .find(|entity| matches!(entity.selector(), EntitySelector::File { .. }))
            .ok_or_else(|| io::Error::other("source file fixture missing"))?;
        let entity = store
            .repository_graph_entity(source.key())?
            .ok_or_else(|| io::Error::other("source graph entity missing"))?;
        require_eq(&entity.generation(), &generation, "graph entity generation")?;
        let relations = store.repository_graph_relations(
            RepositoryGraphRelationQuery::Outbound {
                source: source.key().clone(),
            },
            10,
        )?;
        require_eq(&relations.rows.len(), &4, "graph relation count")?;
        require(
            relations
                .rows
                .iter()
                .all(|relation| relation.generation() == generation),
            "graph relation generation mismatch",
        )?;
        let calls = relations
            .rows
            .iter()
            .find(|relation| relation.kind() == GraphRelationKind::Legacy(RelationKind::Calls))
            .ok_or_else(|| io::Error::other("call relation missing"))?;
        let occurrences = store.repository_graph_occurrences(calls, 10)?;
        require_eq(&occurrences.rows.len(), &2, "graph occurrence count")?;
        require(
            occurrences
                .rows
                .iter()
                .all(|occurrence| occurrence.generation() == generation),
            "graph occurrence generation mismatch",
        )?;
        let occurrence_pages = store.repository_graph_occurrence_pages(&relations.rows, 1, None)?;
        require_eq(
            &occurrence_pages.len(),
            &relations.rows.len(),
            "batched occurrence page count",
        )?;
        for (relation, page) in relations.rows.iter().zip(&occurrence_pages) {
            let is_calls = relation.kind() == GraphRelationKind::Legacy(RelationKind::Calls);
            require_eq(
                &page.rows.len(),
                &usize::from(is_calls),
                "batched occurrence rows",
            )?;
            require_eq(&page.truncated, &is_calls, "batched occurrence truncation")?;
        }
        let coverage =
            store.repository_graph_coverage(fixture.project, &CoverageScope::Project, 10)?;
        require_eq(&coverage.rows.len(), &6, "graph coverage count")?;
        require(
            coverage
                .rows
                .iter()
                .all(|record| record.generation() == generation),
            "graph coverage generation mismatch",
        )?;
        let lexical = store
            .load_file_text("src/Äuth.rs")?
            .ok_or_else(|| io::Error::other("lexical source row missing"))?;
        require_eq(
            &lexical.content.as_str(),
            &lexical_content,
            "lexical source generation",
        )?;
        require_eq(
            &store.symbol_relation_count()?,
            &1,
            "legacy symbol relation compatibility",
        )?;
        Ok(())
    }

    #[test]
    fn typed_graph_round_trips_through_bounded_indexed_queries() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_root = temp.path().join("typed-graph");
        let atlas_dir = project_root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let db_path = atlas_dir.join("projectatlas.db");
        let mut writer = AtlasStore::open_for_project(&db_path, &project_root)?;
        let fixture = publish_fixture(&mut writer, "typed-graph")?;
        drop(writer);
        let store = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;

        for expected in &fixture.entities {
            require_eq(
                &store.repository_graph_entity(expected.key())?,
                &Some(expected.clone()),
                "stable entity lookup",
            )?;
        }

        let source_path = RepositoryNodePath::new(Path::new("src/Äuth.rs"))?;
        let truncated =
            store.repository_graph_entities_by_path(fixture.project, &source_path, 1)?;
        require(
            truncated.truncated && truncated.rows.len() == 1,
            "entity LIMIT + 1",
        )?;
        let path_rows =
            store.repository_graph_entities_by_path(fixture.project, &source_path, 10)?;
        require(
            !path_rows.truncated && path_rows.rows.len() == 2,
            "Unicode/case path lookup",
        )?;
        for (result, context) in [
            (
                store.repository_graph_entities_by_path(fixture.project, &source_path, 0),
                "zero entity page limit",
            ),
            (
                store.repository_graph_entities_by_path(
                    fixture.project,
                    &source_path,
                    GraphLimits::MAX_ROWS + 1,
                ),
                "over-ceiling entity page limit",
            ),
        ] {
            let error = require_db_error(result, context)?;
            require(
                matches!(error, DbError::GraphContract(_)),
                &format!("unexpected {context} error: {error}"),
            )?;
        }

        let source = fixture
            .entities
            .iter()
            .find(|entity| matches!(entity.selector(), EntitySelector::File { .. }))
            .ok_or_else(|| io::Error::other("source file fixture missing"))?;
        let outbound = store.repository_graph_relations(
            RepositoryGraphRelationQuery::Outbound {
                source: source.key().clone(),
            },
            10,
        )?;
        require_eq(&outbound.rows.len(), &4, "all resolution states")?;
        require(
            outbound.rows.iter().any(|relation| {
                matches!(relation.resolution(), RelationResolution::Resolved { .. })
            }) && outbound.rows.iter().any(|relation| {
                matches!(relation.resolution(), RelationResolution::Ambiguous { .. })
            }) && outbound.rows.iter().any(|relation| {
                matches!(relation.resolution(), RelationResolution::Unresolved { .. })
            }) && outbound.rows.iter().any(|relation| {
                matches!(relation.resolution(), RelationResolution::External { .. })
            }),
            "resolution variants did not round-trip",
        )?;
        let outbound_truncated = store.repository_graph_relations(
            RepositoryGraphRelationQuery::Outbound {
                source: source.key().clone(),
            },
            3,
        )?;
        require(
            outbound_truncated.truncated && outbound_truncated.rows.len() == 3,
            "relation LIMIT + 1",
        )?;
        for (limit, context) in [
            (0, "zero relation page limit"),
            (
                GraphLimits::MAX_ROWS + 1,
                "over-ceiling relation page limit",
            ),
        ] {
            let error = require_db_error(
                store.repository_graph_relations(
                    RepositoryGraphRelationQuery::Outbound {
                        source: source.key().clone(),
                    },
                    limit,
                ),
                context,
            )?;
            require(
                matches!(error, DbError::GraphContract(_)),
                &format!("unexpected {context} error: {error}"),
            )?;
        }

        let symbol = fixture
            .entities
            .iter()
            .find(|entity| matches!(entity.selector(), EntitySelector::Symbol { .. }))
            .ok_or_else(|| io::Error::other("symbol fixture missing"))?;
        let inbound = store.repository_graph_relations(
            RepositoryGraphRelationQuery::Inbound {
                target: symbol.key().clone(),
            },
            10,
        )?;
        require_eq(&inbound.rows.len(), &1, "inbound relation lookup")?;
        let calls = store.repository_graph_relations(
            RepositoryGraphRelationQuery::Family {
                relation: GraphRelationKind::Legacy(RelationKind::Calls),
            },
            10,
        )?;
        require_eq(&calls.rows.len(), &1, "relation-family lookup")?;
        let occurrence_page = store.repository_graph_occurrences(&calls.rows[0], 1)?;
        require(
            occurrence_page.truncated && occurrence_page.rows.len() == 1,
            "occurrence LIMIT + 1",
        )?;
        let all_occurrences = store.repository_graph_occurrences(&calls.rows[0], 10)?;
        require_eq(
            &all_occurrences.rows.len(),
            &2,
            "logical relation occurrence retention",
        )?;
        let error = require_db_error(
            store.repository_graph_occurrences(&calls.rows[0], 0),
            "zero occurrence page limit was accepted",
        )?;
        require(
            matches!(error, DbError::GraphContract(_)),
            &format!("unexpected zero occurrence-limit error: {error}"),
        )?;
        let error = require_db_error(
            store.repository_graph_occurrences(&calls.rows[0], GraphLimits::MAX_OCCURRENCES + 1),
            "over-ceiling occurrence page was accepted",
        )?;
        require(
            matches!(error, DbError::GraphContract(_)),
            &format!("unexpected occurrence-limit error: {error}"),
        )?;

        let project_coverage =
            store.repository_graph_coverage(fixture.project, &CoverageScope::Project, 10)?;
        let path_coverage = store.repository_graph_coverage(
            fixture.project,
            &CoverageScope::Path {
                path: RepositoryNodePath::new(Path::new("src"))?,
            },
            10,
        )?;
        require_eq(&project_coverage.rows.len(), &6, "project coverage states")?;
        require_eq(&path_coverage.rows.len(), &1, "path coverage state")?;
        require(
            path_coverage.rows[0].state() == CoverageState::Partial,
            "partial coverage did not round-trip",
        )?;
        for (limit, context) in [
            (0, "zero coverage page limit"),
            (
                GraphLimits::MAX_ROWS + 1,
                "over-ceiling coverage page limit",
            ),
        ] {
            let error = require_db_error(
                store.repository_graph_coverage(fixture.project, &CoverageScope::Project, limit),
                context,
            )?;
            require(
                matches!(error, DbError::GraphContract(_)),
                &format!("unexpected {context} error: {error}"),
            )?;
        }

        let source_next = GraphEntity::new(
            fixture.project,
            source.selector().clone(),
            IndexGeneration::new(2),
        )?;
        let symbol_next = GraphEntity::new(
            fixture.project,
            symbol.selector().clone(),
            IndexGeneration::new(2),
        )?;
        let next_generation_call = LogicalRelation::new(
            &source_next,
            GraphRelationKind::Legacy(RelationKind::Calls),
            RelationResolution::resolved(&symbol_next)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            IndexGeneration::new(2),
        )?;
        let error = require_db_error(
            store.repository_graph_occurrences(&next_generation_call, 10),
            "generation-mismatched occurrence request was accepted",
        )?;
        require(
            matches!(error, DbError::GraphContract(_)),
            &format!("unexpected occurrence generation error: {error}"),
        )?;

        let lexical = store
            .load_file_text("src/Äuth.rs")?
            .ok_or_else(|| io::Error::other("lexical source row missing"))?;
        require_eq(
            &lexical.content,
            &"fn verifyToken()".to_string(),
            "lexical owner",
        )?;
        require_eq(
            &store.symbol_relation_count()?,
            &1,
            "legacy relation projection changed",
        )?;

        store.finish_index_read_snapshot()?;
        drop(store);
        let mut writer = AtlasStore::open_for_project(&db_path, &project_root)?;
        let mut publication = writer.begin_index_publication("typed-graph")?;
        publication.replace_repository_graph_for_paths(
            fixture.project,
            &["src/unrelated.rs".to_string()],
            &[],
            &[],
            &[],
            &[],
        )?;
        publication.complete()?;
        drop(writer);
        let store = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
        let reused = store
            .repository_graph_entity(source.key())?
            .ok_or_else(|| io::Error::other("unchanged entity disappeared"))?;
        require_eq(
            &reused.generation(),
            &IndexGeneration::new(2),
            "unchanged graph row generation injection",
        )?;
        for expected in &fixture.entities {
            let reused = store
                .repository_graph_entity(expected.key())?
                .ok_or_else(|| io::Error::other("unchanged graph entity disappeared"))?;
            require_eq(
                &reused.generation(),
                &IndexGeneration::new(2),
                "incremental graph row reuse",
            )?;
        }
        assert_query_indexes(&store)?;
        store.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn batched_adjacency_uses_direction_owned_indexes_and_stable_keysets()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_root = temp.path().join("batched-adjacency");
        let atlas_dir = project_root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let db_path = atlas_dir.join("projectatlas.db");
        let mut writer = AtlasStore::open_for_project(&db_path, &project_root)?;
        let fixture = publish_fixture(&mut writer, "batched-adjacency")?;
        drop(writer);
        let store = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;

        let source = fixture
            .entities
            .iter()
            .find(|entity| matches!(entity.selector(), EntitySelector::File { .. }))
            .ok_or_else(|| io::Error::other("source file fixture missing"))?;
        let symbol = fixture
            .entities
            .iter()
            .find(|entity| matches!(entity.selector(), EntitySelector::Symbol { .. }))
            .ok_or_else(|| io::Error::other("symbol fixture missing"))?;
        let external = fixture
            .entities
            .iter()
            .find(|entity| matches!(entity.selector(), EntitySelector::External { .. }))
            .ok_or_else(|| io::Error::other("external fixture missing"))?;
        let outbound_frontier = vec![source.key().clone()];

        let first = store.repository_graph_adjacency_page(
            &outbound_frontier,
            RepositoryGraphDirection::Outbound,
            None,
            2,
            None,
        )?;
        require(
            first.truncated && first.rows.len() == 2 && first.continuation.is_some(),
            "outbound adjacency did not retain its LIMIT + 1 keyset",
        )?;
        require(
            first.rows.iter().all(|row| {
                row.frontier_index == 0
                    && row.frontier == source.key().clone()
                    && row.direction == RepositoryGraphDirection::Outbound
            }),
            "outbound adjacency lost its selecting frontier",
        )?;
        let continuation = first
            .continuation
            .clone()
            .ok_or_else(|| io::Error::other("outbound continuation missing"))?;
        let second = store.repository_graph_adjacency_page(
            &outbound_frontier,
            RepositoryGraphDirection::Outbound,
            Some(&continuation),
            10,
            None,
        )?;
        require(
            !second.truncated && second.rows.len() == 2 && second.continuation.is_none(),
            "outbound continuation did not finish the stable relation order",
        )?;
        let ordinary = store.repository_graph_relations(
            RepositoryGraphRelationQuery::Outbound {
                source: source.key().clone(),
            },
            10,
        )?;
        let combined = first
            .rows
            .into_iter()
            .chain(second.rows)
            .map(|row| row.detail.relation)
            .collect::<Vec<_>>();
        require_eq(
            &combined,
            &ordinary.rows,
            "adjacency keyset order versus ordinary relation order",
        )?;
        let calls = GraphRelationKind::Legacy(RelationKind::Calls);
        let filtered = store.repository_graph_adjacency_page_filtered(
            &outbound_frontier,
            RepositoryGraphDirection::Outbound,
            Some(calls),
            None,
            10,
            None,
        )?;
        let expected_calls = ordinary
            .rows
            .iter()
            .filter(|relation| relation.kind() == calls)
            .cloned()
            .collect::<Vec<_>>();
        require_eq(
            &filtered
                .rows
                .iter()
                .map(|row| row.detail.relation.clone())
                .collect::<Vec<_>>(),
            &expected_calls,
            "filtered adjacency versus ordinary family selection",
        )?;
        require(
            !filtered.truncated && filtered.continuation.is_none(),
            "filtered adjacency unexpectedly truncated its complete family",
        )?;

        let inbound_frontier = vec![symbol.key().clone(), external.key().clone()];
        let inbound = store.repository_graph_adjacency_page(
            &inbound_frontier,
            RepositoryGraphDirection::Inbound,
            None,
            10,
            None,
        )?;
        require(
            !inbound.truncated
                && inbound.rows.len() == 2
                && inbound.rows[0].frontier_index == 0
                && inbound.rows[0].frontier == symbol.key().clone()
                && inbound.rows[1].frontier_index == 1
                && inbound.rows[1].frontier == external.key().clone()
                && inbound
                    .rows
                    .iter()
                    .all(|row| row.direction == RepositoryGraphDirection::Inbound),
            "inbound adjacency did not preserve bounded frontier order",
        )?;

        for (direction, expected_index) in [
            (
                RepositoryGraphDirection::Outbound,
                "idx_graph_relations_source_kind",
            ),
            (
                RepositoryGraphDirection::Inbound,
                "idx_graph_relations_target_kind",
            ),
        ] {
            for continuation_index in [None, Some(0)] {
                let sql = format!(
                    "EXPLAIN QUERY PLAN {}",
                    adjacency_relation_sql(2, direction, continuation_index, false)
                );
                let mut statement = store.connection.prepare(&sql)?;
                let mut bindings = vec![Value::Blob(fixture.project.as_bytes().to_vec())];
                bindings.push(Value::Blob(source.key().digest_bytes()?.to_vec()));
                if continuation_index.is_some() {
                    bindings.extend([
                        Value::Text(String::new()),
                        Value::Text(String::new()),
                        Value::Text(String::new()),
                        Value::Blob(vec![0; 32]),
                    ]);
                }
                bindings.extend([
                    Value::Integer(11),
                    Value::Blob(symbol.key().digest_bytes()?.to_vec()),
                    Value::Integer(11),
                    Value::Integer(11),
                ]);
                let details = statement
                    .query_map(params_from_iter(bindings.iter()), |row| {
                        row.get::<_, String>(3)
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                require(
                    details.iter().any(|detail| detail.contains(expected_index))
                        && details
                            .iter()
                            .all(|detail| !detail.contains("SCAN relation")),
                    &format!(
                        "{direction:?} adjacency plan (continuation_index={continuation_index:?}) did not own its index: {details:?}"
                    ),
                )?;
            }

            let (scope, kind) = relation_parts(calls);
            let filtered_sql = format!(
                "EXPLAIN QUERY PLAN {}",
                adjacency_relation_sql(2, direction, None, true)
            );
            let mut filtered_statement = store.connection.prepare(&filtered_sql)?;
            let filtered_bindings = [
                Value::Blob(fixture.project.as_bytes().to_vec()),
                Value::Blob(source.key().digest_bytes()?.to_vec()),
                Value::Text(scope.to_string()),
                Value::Text(kind.to_string()),
                Value::Integer(11),
                Value::Blob(symbol.key().digest_bytes()?.to_vec()),
                Value::Text(scope.to_string()),
                Value::Text(kind.to_string()),
                Value::Integer(11),
                Value::Integer(11),
            ];
            let filtered_details = filtered_statement
                .query_map(params_from_iter(filtered_bindings.iter()), |row| {
                    row.get::<_, String>(3)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            require(
                filtered_details
                    .iter()
                    .any(|detail| detail.contains(expected_index))
                    && filtered_details
                        .iter()
                        .all(|detail| !detail.contains("SCAN relation")),
                &format!(
                    "filtered {direction:?} adjacency plan did not own its index: {filtered_details:?}"
                ),
            )?;
        }

        let duplicate_error = require_db_error(
            store.repository_graph_adjacency_page(
                &[source.key().clone(), source.key().clone()],
                RepositoryGraphDirection::Outbound,
                None,
                1,
                None,
            ),
            "duplicate adjacency frontier was accepted",
        )?;
        require(
            matches!(duplicate_error, DbError::GraphContract(_)),
            &format!("unexpected duplicate-frontier error: {duplicate_error}"),
        )?;
        let oversized = vec![source.key().clone(); MAX_REPOSITORY_GRAPH_FRONTIER + 1];
        let oversized_error = require_db_error(
            store.repository_graph_adjacency_page(
                &oversized,
                RepositoryGraphDirection::Outbound,
                None,
                1,
                None,
            ),
            "oversized adjacency frontier was accepted",
        )?;
        require(
            matches!(oversized_error, DbError::GraphContract(_)),
            &format!("unexpected oversized-frontier error: {oversized_error}"),
        )?;
        let foreign = GraphEntity::new(
            ProjectInstanceId::from_bytes([0x7f; 16])?,
            EntitySelector::Project,
            IndexGeneration::new(1),
        )?;
        let project_error = require_db_error(
            store.repository_graph_adjacency_page(
                &[source.key().clone(), foreign.key().clone()],
                RepositoryGraphDirection::Outbound,
                None,
                1,
                None,
            ),
            "mixed-project adjacency frontier was accepted",
        )?;
        require(
            matches!(project_error, DbError::GraphProjectIdentityMismatch { .. }),
            &format!("unexpected mixed-project error: {project_error}"),
        )?;
        let direction_error = require_db_error(
            store.repository_graph_adjacency_page(
                &outbound_frontier,
                RepositoryGraphDirection::Inbound,
                Some(&continuation),
                1,
                None,
            ),
            "cross-direction adjacency continuation was accepted",
        )?;
        require(
            matches!(direction_error, DbError::GraphContract(_)),
            &format!("unexpected continuation-direction error: {direction_error}"),
        )?;
        let mut filtered_continuation = continuation.clone();
        filtered_continuation.relation = Some(calls);
        let family_error = require_db_error(
            store.repository_graph_adjacency_page_filtered(
                &outbound_frontier,
                RepositoryGraphDirection::Outbound,
                Some(GraphRelationKind::Extended(
                    ExtendedRelationKind::Configures,
                )),
                Some(&filtered_continuation),
                1,
                None,
            ),
            "cross-family adjacency continuation was accepted",
        )?;
        require(
            matches!(family_error, DbError::GraphContract(_)),
            &format!("unexpected continuation-family error: {family_error}"),
        )?;
        let frontier_error = require_db_error(
            store.repository_graph_adjacency_page(
                &[external.key().clone()],
                RepositoryGraphDirection::Outbound,
                Some(&continuation),
                1,
                None,
            ),
            "cross-frontier adjacency continuation was accepted",
        )?;
        require(
            matches!(frontier_error, DbError::GraphContract(_)),
            &format!("unexpected continuation-frontier error: {frontier_error}"),
        )?;

        let inbound_first = store.repository_graph_adjacency_page(
            &inbound_frontier,
            RepositoryGraphDirection::Inbound,
            None,
            1,
            None,
        )?;
        let inbound_continuation = inbound_first
            .continuation
            .ok_or_else(|| io::Error::other("inbound continuation missing"))?;
        let reordered_frontier = vec![external.key().clone(), symbol.key().clone()];
        let reordered_error = require_db_error(
            store.repository_graph_adjacency_page(
                &reordered_frontier,
                RepositoryGraphDirection::Inbound,
                Some(&inbound_continuation),
                1,
                None,
            ),
            "reordered adjacency continuation frontier was accepted",
        )?;
        require(
            matches!(reordered_error, DbError::GraphContract(_)),
            &format!("unexpected reordered-frontier error: {reordered_error}"),
        )?;
        let empty_frontier_error = require_db_error(
            store.repository_graph_adjacency_page(
                &[],
                RepositoryGraphDirection::Inbound,
                Some(&inbound_continuation),
                1,
                None,
            ),
            "adjacency continuation without a frontier was accepted",
        )?;
        require(
            matches!(empty_frontier_error, DbError::GraphContract(_)),
            &format!("unexpected empty-frontier error: {empty_frontier_error}"),
        )?;

        let mut foreign_project_continuation = continuation.clone();
        foreign_project_continuation.project = foreign.key().project();
        let continuation_project_error = require_db_error(
            store.repository_graph_adjacency_page(
                &outbound_frontier,
                RepositoryGraphDirection::Outbound,
                Some(&foreign_project_continuation),
                1,
                None,
            ),
            "cross-project adjacency continuation was accepted",
        )?;
        require(
            matches!(continuation_project_error, DbError::GraphContract(_)),
            &format!("unexpected continuation-project error: {continuation_project_error}"),
        )?;
        let mut stale_generation_continuation = continuation.clone();
        stale_generation_continuation.generation = stale_generation_continuation
            .generation
            .checked_next()
            .ok_or_else(|| io::Error::other("fixture generation overflowed"))?;
        let continuation_generation_error = require_db_error(
            store.repository_graph_adjacency_page(
                &outbound_frontier,
                RepositoryGraphDirection::Outbound,
                Some(&stale_generation_continuation),
                1,
                None,
            ),
            "cross-generation adjacency continuation was accepted",
        )?;
        require(
            matches!(continuation_generation_error, DbError::GraphContract(_)),
            &format!("unexpected continuation-generation error: {continuation_generation_error}"),
        )?;

        let mut maximum_frontier = Vec::with_capacity(MAX_REPOSITORY_GRAPH_FRONTIER);
        for index in 0..MAX_REPOSITORY_GRAPH_FRONTIER {
            let entity = GraphEntity::new(
                fixture.project,
                EntitySelector::External {
                    external: ExternalSelector {
                        system: GraphIdentityText::new("work-envelope")?,
                        identity: GraphIdentityText::new(format!("candidate-{index}"))?,
                    },
                },
                continuation.generation,
            )?;
            maximum_frontier.push(entity.key().clone());
        }
        let bounded_work = store.repository_graph_adjacency_page(
            &maximum_frontier,
            RepositoryGraphDirection::Outbound,
            None,
            38,
            None,
        )?;
        require(
            bounded_work.rows.is_empty() && !bounded_work.truncated,
            "maximum adjacency frontier rejected work below the intermediate ceiling",
        )?;
        let excessive_work_error = require_db_error(
            store.repository_graph_adjacency_page(
                &maximum_frontier,
                RepositoryGraphDirection::Outbound,
                None,
                39,
                None,
            ),
            "adjacency intermediate work above the ceiling was accepted",
        )?;
        require(
            matches!(excessive_work_error, DbError::GraphContract(_)),
            &format!("unexpected intermediate-work error: {excessive_work_error}"),
        )?;

        let successful_cancellation = projectatlas_core::IndexCancellation::new();
        let successful_control = IndexWorkControl::new(successful_cancellation.clone(), None);
        store.repository_graph_adjacency_page(
            &outbound_frontier,
            RepositoryGraphDirection::Outbound,
            None,
            10,
            Some(&successful_control),
        )?;
        successful_cancellation.cancel();
        require_eq(
            &store
                .connection
                .query_row("SELECT 1", [], |row| row.get::<_, i64>(0))?,
            &1,
            "cleared adjacency progress handler",
        )?;

        let cancellation = projectatlas_core::IndexCancellation::new();
        cancellation.cancel();
        let control = IndexWorkControl::new(cancellation, None);
        let cancelled = store.repository_graph_adjacency_page(
            &outbound_frontier,
            RepositoryGraphDirection::Outbound,
            None,
            10,
            Some(&control),
        );
        require(
            matches!(
                cancelled,
                Err(DbError::IndexWork(
                    projectatlas_core::IndexWorkFailure::Cancelled {
                        stage: IndexWorkStage::RepositoryTraversal
                    }
                ))
            ),
            "adjacency cancellation was not typed",
        )?;
        store.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn cursor_hydration_is_ordered_bounded_and_fail_closed() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_root = temp.path().join("cursor-hydration");
        let atlas_dir = project_root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let db_path = atlas_dir.join("projectatlas.db");
        let mut writer = AtlasStore::open_for_project(&db_path, &project_root)?;
        let fixture = publish_fixture(&mut writer, "cursor-hydration")?;
        writer.connection.execute(
            "INSERT INTO graph_relation_occurrences(
                 relation_key, file_path, start_line, start_column, end_line, end_column
             ) VALUES(?1, 'Cargo.toml', 1, 0, 1, 1)",
            params![&fixture.relations[0].key().digest_bytes()?[..]],
        )?;
        writer.connection.execute(
            "INSERT INTO graph_coverage(
                 project_instance_id, scope_kind, scope_path, relation_scope,
                 relation_kind, state, total, covered, omitted, reason, reached_limit
             ) VALUES(?1, 'path', 'Cargo.toml', NULL, NULL,
                      'complete', 1, 1, 0, NULL, NULL)",
            params![&fixture.project.as_bytes()[..]],
        )?;
        drop(writer);
        let store = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
        let generation = store
            .repository_graph_generation()?
            .ok_or_else(|| io::Error::other("graph generation missing"))?;
        let full_budget = RepositoryGraphReadBudget::new(
            MAX_REPOSITORY_GRAPH_FRONTIER as u32,
            MAX_REPOSITORY_GRAPH_FRONTIER as u32,
            RepositoryGraphReadBudget::MAX_DECODED_BYTES,
            RepositoryGraphReadBudget::MAX_HYDRATED_ENTITIES,
            RepositoryGraphReadBudget::MAX_HYDRATED_PATHS,
        )?;

        let expected_entities = fixture
            .entities
            .iter()
            .rev()
            .take(3)
            .cloned()
            .collect::<Vec<_>>();
        let entity_digests = expected_entities
            .iter()
            .map(|entity| entity.key().digest_bytes())
            .collect::<Result<Vec<_>, _>>()?;
        let entity_batch = store.repository_graph_entities_by_digest(
            fixture.project,
            generation,
            &entity_digests,
            full_budget,
            None,
        )?;
        require_eq(
            &entity_batch.rows,
            &expected_entities,
            "ordered entity cursor hydration",
        )?;
        require_eq(
            &entity_batch.work,
            &RepositoryGraphReadWork {
                requested_rows: 3,
                returned_rows: 3,
                decoded_bytes: entity_batch.work.decoded_bytes,
                hydrated_entities: 3,
                hydrated_paths: 2,
            },
            "exact entity cursor work",
        )?;
        require(
            entity_batch.work.decoded_bytes > 0,
            "entity cursor decoded no SQLite payload bytes",
        )?;

        let file_entity = fixture
            .entities
            .iter()
            .find(|entity| matches!(entity.selector(), EntitySelector::File { .. }))
            .ok_or_else(|| io::Error::other("file anchor fixture missing"))?;
        let file_anchor = store.repository_graph_entity_bounded(
            file_entity.key(),
            generation,
            full_budget,
            None,
        )?;
        require_eq(
            &file_anchor.rows,
            &vec![file_entity.clone()],
            "exact file anchor hydration",
        )?;
        require_eq(
            &file_anchor.work,
            &RepositoryGraphReadWork {
                requested_rows: 1,
                returned_rows: 1,
                decoded_bytes: file_anchor.work.decoded_bytes,
                hydrated_entities: 1,
                hydrated_paths: 1,
            },
            "exact file anchor work",
        )?;
        let exact_file_budget =
            RepositoryGraphReadBudget::new(1, 1, file_anchor.work.decoded_bytes, 1, 1)?;
        require_eq(
            &store.repository_graph_entity_bounded(
                file_entity.key(),
                generation,
                exact_file_budget,
                None,
            )?,
            &file_anchor,
            "exact file anchor envelope",
        )?;
        require_eq(
            &store.repository_graph_entity(file_entity.key())?,
            &Some(file_entity.clone()),
            "legacy file anchor wrapper compatibility",
        )?;
        let file_decode_overrun = require_db_error(
            store.repository_graph_entity_bounded(
                file_entity.key(),
                generation,
                RepositoryGraphReadBudget::new(1, 1, file_anchor.work.decoded_bytes - 1, 1, 1)?,
                None,
            ),
            "file anchor decoded-byte overrun was accepted",
        )?;
        require(
            matches!(file_decode_overrun, DbError::GraphContract(_)),
            &format!("unexpected file anchor envelope error: {file_decode_overrun}"),
        )?;
        let missing_file_selector = EntitySelector::File {
            path: RepositoryFilePath::new(Path::new("missing-anchor.rs"))?,
        };
        let missing_file_key = GraphEntityKey::new(fixture.project, &missing_file_selector);
        let missing_file = store.repository_graph_entity_bounded(
            &missing_file_key,
            generation,
            full_budget,
            None,
        )?;
        require(
            missing_file.rows.is_empty()
                && missing_file.work
                    == (RepositoryGraphReadWork {
                        requested_rows: 1,
                        returned_rows: 0,
                        decoded_bytes: 0,
                        hydrated_entities: 0,
                        hydrated_paths: 0,
                    }),
            "missing file anchor did not return exact empty work",
        )?;

        let anchor_path = RepositoryNodePath::new(Path::new("src/Äuth.rs"))?;
        let path_anchors = store.repository_graph_entities_by_path_bounded(
            fixture.project,
            generation,
            &anchor_path,
            1,
            full_budget,
            None,
        )?;
        require(
            path_anchors.page.rows.len() == 1
                && path_anchors.page.truncated
                && matches!(
                    path_anchors.page.rows[0].selector(),
                    EntitySelector::File { .. }
                )
                && path_anchors.work.requested_rows == 1
                && path_anchors.work.returned_rows == 1
                && path_anchors.work.decoded_bytes > 0
                && path_anchors.work.hydrated_entities == 2
                && path_anchors.work.hydrated_paths == 1,
            "path anchor page lost stable order or sentinel work",
        )?;
        let exact_path_anchor_budget =
            RepositoryGraphReadBudget::new(1, 1, path_anchors.work.decoded_bytes, 2, 1)?;
        require_eq(
            &store.repository_graph_entities_by_path_bounded(
                fixture.project,
                generation,
                &anchor_path,
                1,
                exact_path_anchor_budget,
                None,
            )?,
            &path_anchors,
            "exact path anchor envelope",
        )?;
        require_eq(
            &store.repository_graph_entities_by_path(fixture.project, &anchor_path, 1)?,
            &path_anchors.page,
            "legacy path anchor wrapper compatibility",
        )?;
        for (budget, limit, context) in [
            (
                RepositoryGraphReadBudget::new(1, 1, path_anchors.work.decoded_bytes - 1, 2, 1)?,
                1,
                "path anchor decoded-byte overrun was accepted",
            ),
            (
                RepositoryGraphReadBudget::new(1, 1, path_anchors.work.decoded_bytes, 1, 1)?,
                1,
                "path anchor sentinel entity overrun was accepted",
            ),
            (
                exact_path_anchor_budget,
                2,
                "path anchor returned-row overrun was accepted",
            ),
        ] {
            let error = require_db_error(
                store.repository_graph_entities_by_path_bounded(
                    fixture.project,
                    generation,
                    &anchor_path,
                    limit,
                    budget,
                    None,
                ),
                context,
            )?;
            require(
                matches!(error, DbError::GraphContract(_)),
                &format!("unexpected path anchor envelope error: {error}"),
            )?;
        }
        let anchor_cancellation = projectatlas_core::IndexCancellation::new();
        anchor_cancellation.cancel();
        let anchor_control = IndexWorkControl::new(anchor_cancellation, None);
        let cancelled_anchor = store.repository_graph_entities_by_path_bounded(
            fixture.project,
            generation,
            &anchor_path,
            1,
            full_budget,
            Some(&anchor_control),
        );
        require(
            matches!(
                cancelled_anchor,
                Err(DbError::IndexWork(
                    projectatlas_core::IndexWorkFailure::Cancelled {
                        stage: IndexWorkStage::RepositoryTraversal
                    }
                ))
            ),
            "path anchor cancellation was not typed",
        )?;
        let expired_anchor = IndexWorkControl::with_deadline(
            projectatlas_core::IndexCancellation::new(),
            Instant::now(),
        );
        let anchor_deadline = store.repository_graph_entity_bounded(
            file_entity.key(),
            generation,
            full_budget,
            Some(&expired_anchor),
        );
        require(
            matches!(
                anchor_deadline,
                Err(DbError::IndexWork(
                    projectatlas_core::IndexWorkFailure::DeadlineExceeded {
                        stage: IndexWorkStage::RepositoryTraversal
                    }
                ))
            ),
            "file anchor deadline was not typed",
        )?;

        let expected_relations = fixture.relations.iter().rev().cloned().collect::<Vec<_>>();
        let relation_digests = expected_relations
            .iter()
            .map(|relation| relation.key().digest_bytes())
            .collect::<Result<Vec<_>, _>>()?;
        let relation_rows = store.repository_graph_relation_rows_by_digest(
            fixture.project,
            generation,
            &relation_digests,
            full_budget,
            None,
        )?;
        require_eq(
            &relation_rows
                .rows
                .iter()
                .map(|row| row.relation.clone())
                .collect::<Vec<_>>(),
            &expected_relations,
            "ordered relation cursor hydration",
        )?;
        require(
            relation_rows.rows.iter().all(|row| {
                row.source.key().project() == fixture.project
                    && row
                        .target
                        .as_ref()
                        .is_none_or(|target| target.key().project() == fixture.project)
            }),
            "relation cursor hydration returned an unvalidated endpoint",
        )?;
        require_eq(
            &relation_rows.work,
            &RepositoryGraphReadWork {
                requested_rows: 4,
                returned_rows: 4,
                decoded_bytes: relation_rows.work.decoded_bytes,
                hydrated_entities: 3,
                hydrated_paths: 1,
            },
            "exact relation cursor work",
        )?;
        require(
            relation_rows.work.decoded_bytes > entity_batch.work.decoded_bytes,
            "relation cursor did not meter relation and endpoint payload bytes",
        )?;

        let exact_entity_budget =
            RepositoryGraphReadBudget::new(3, 3, entity_batch.work.decoded_bytes, 3, 2)?;
        require_eq(
            &store
                .repository_graph_entities_by_digest(
                    fixture.project,
                    generation,
                    &entity_digests,
                    exact_entity_budget,
                    None,
                )?
                .work,
            &entity_batch.work,
            "exact entity envelope",
        )?;
        let exact_relation_budget =
            RepositoryGraphReadBudget::new(4, 4, relation_rows.work.decoded_bytes, 3, 1)?;
        require_eq(
            &store
                .repository_graph_relation_rows_by_digest(
                    fixture.project,
                    generation,
                    &relation_digests,
                    exact_relation_budget,
                    None,
                )?
                .work,
            &relation_rows.work,
            "exact relation envelope",
        )?;

        for (budget, context) in [
            (
                RepositoryGraphReadBudget::new(3, 3, entity_batch.work.decoded_bytes - 1, 3, 2)?,
                "decoded-byte envelope overrun was accepted",
            ),
            (
                RepositoryGraphReadBudget::new(3, 3, entity_batch.work.decoded_bytes, 3, 1)?,
                "purpose-path envelope overrun was accepted",
            ),
        ] {
            let error = require_db_error(
                store.repository_graph_entities_by_digest(
                    fixture.project,
                    generation,
                    &entity_digests,
                    budget,
                    None,
                ),
                context,
            )?;
            require(
                matches!(error, DbError::GraphContract(_)),
                &format!("unexpected entity envelope error: {error}"),
            )?;
        }
        let endpoint_budget =
            RepositoryGraphReadBudget::new(4, 4, relation_rows.work.decoded_bytes, 2, 1)?;
        let endpoint_error = require_db_error(
            store.repository_graph_relation_rows_by_digest(
                fixture.project,
                generation,
                &relation_digests,
                endpoint_budget,
                None,
            ),
            "endpoint entity envelope overrun was accepted",
        )?;
        require(
            matches!(endpoint_error, DbError::GraphContract(_)),
            &format!("unexpected endpoint envelope error: {endpoint_error}"),
        )?;

        for (budget, context) in [
            (
                RepositoryGraphReadBudget::new(
                    2,
                    3,
                    RepositoryGraphReadBudget::MAX_DECODED_BYTES,
                    3,
                    2,
                )?,
                "requested-row envelope overrun was accepted",
            ),
            (
                RepositoryGraphReadBudget::new(
                    3,
                    2,
                    RepositoryGraphReadBudget::MAX_DECODED_BYTES,
                    3,
                    2,
                )?,
                "returned-row envelope overrun was accepted",
            ),
        ] {
            let error = require_db_error(
                store.repository_graph_entities_by_digest(
                    fixture.project,
                    generation,
                    &entity_digests,
                    budget,
                    None,
                ),
                context,
            )?;
            require(
                matches!(error, DbError::GraphContract(_)),
                &format!("unexpected row-envelope error: {error}"),
            )?;
        }

        for invalid in [
            RepositoryGraphReadBudget::new(0, 1, 1, 1, 1),
            RepositoryGraphReadBudget::new(
                RepositoryGraphReadBudget::MAX_REQUESTED_ROWS + 1,
                1,
                1,
                1,
                1,
            ),
            RepositoryGraphReadBudget::new(
                1,
                RepositoryGraphReadBudget::MAX_RETURNED_ROWS + 1,
                1,
                1,
                1,
            ),
            RepositoryGraphReadBudget::new(
                1,
                1,
                RepositoryGraphReadBudget::MAX_DECODED_BYTES + 1,
                1,
                1,
            ),
            RepositoryGraphReadBudget::new(
                1,
                1,
                1,
                RepositoryGraphReadBudget::MAX_HYDRATED_ENTITIES + 1,
                1,
            ),
            RepositoryGraphReadBudget::new(
                1,
                1,
                1,
                1,
                RepositoryGraphReadBudget::MAX_HYDRATED_PATHS + 1,
            ),
        ] {
            require(
                matches!(invalid, Err(GraphContractError::InvalidLimits { .. })),
                "invalid graph read budget was accepted",
            )?;
        }

        let empty_entities = store.repository_graph_entities_by_digest(
            fixture.project,
            generation,
            &[],
            full_budget,
            None,
        )?;
        let empty_relations = store.repository_graph_relation_rows_by_digest(
            fixture.project,
            generation,
            &[],
            full_budget,
            None,
        )?;
        require(
            empty_entities.rows.is_empty()
                && empty_relations.rows.is_empty()
                && empty_entities.work
                    == (RepositoryGraphReadWork {
                        requested_rows: 0,
                        returned_rows: 0,
                        decoded_bytes: 0,
                        hydrated_entities: 0,
                        hydrated_paths: 0,
                    })
                && empty_relations.work == empty_entities.work,
            "empty cursor hydration was not stable",
        )?;

        let purpose_paths = vec![
            "Cargo.toml".to_string(),
            ".".to_string(),
            "src/Äuth.rs".to_string(),
            "src".to_string(),
        ];
        let purpose_batch = store.load_purpose_owner_nodes_by_paths_controlled(
            fixture.project,
            generation,
            &purpose_paths,
            full_budget,
            None,
        )?;
        require_eq(
            &purpose_batch
                .rows
                .iter()
                .map(|node| node.node.path.clone())
                .collect::<Vec<_>>(),
            &purpose_paths,
            "ordered purpose-owner hydration",
        )?;
        require_eq(
            &purpose_batch.work,
            &RepositoryGraphReadWork {
                requested_rows: 4,
                returned_rows: 4,
                decoded_bytes: purpose_batch.work.decoded_bytes,
                hydrated_entities: 0,
                hydrated_paths: 4,
            },
            "exact purpose-owner work",
        )?;
        require(
            purpose_batch.work.decoded_bytes > 0,
            "purpose-owner hydration decoded no SQLite payload bytes",
        )?;
        let exact_purpose_budget =
            RepositoryGraphReadBudget::new(4, 4, purpose_batch.work.decoded_bytes, 1, 4)?;
        require_eq(
            &store
                .load_purpose_owner_nodes_by_paths_controlled(
                    fixture.project,
                    generation,
                    &purpose_paths,
                    exact_purpose_budget,
                    None,
                )?
                .work,
            &purpose_batch.work,
            "exact purpose-owner envelope",
        )?;
        for (budget, context) in [
            (
                RepositoryGraphReadBudget::new(4, 4, purpose_batch.work.decoded_bytes - 1, 1, 4)?,
                "purpose-owner decoded-byte overrun was accepted",
            ),
            (
                RepositoryGraphReadBudget::new(4, 4, purpose_batch.work.decoded_bytes, 1, 3)?,
                "purpose-owner path overrun was accepted",
            ),
        ] {
            let error = require_db_error(
                store.load_purpose_owner_nodes_by_paths_controlled(
                    fixture.project,
                    generation,
                    &purpose_paths,
                    budget,
                    None,
                ),
                context,
            )?;
            require(
                matches!(error, DbError::GraphContract(_)),
                &format!("unexpected purpose-owner envelope error: {error}"),
            )?;
        }
        let mut missing_purpose_paths = purpose_paths.clone();
        missing_purpose_paths.push("missing/purpose-owner.rs".to_string());
        let missing_purpose = store.load_purpose_owner_nodes_by_paths_controlled(
            fixture.project,
            generation,
            &missing_purpose_paths,
            full_budget,
            None,
        )?;
        require_eq(
            &missing_purpose
                .rows
                .iter()
                .map(|node| node.node.path.clone())
                .collect::<Vec<_>>(),
            &purpose_paths,
            "absent purpose-owner candidate ordering",
        )?;
        require(
            missing_purpose.work.requested_rows == 5
                && missing_purpose.work.returned_rows == 4
                && missing_purpose.work.hydrated_paths == 4,
            "absent purpose-owner candidate work was not exact",
        )?;
        let duplicate_purpose = require_db_error(
            store.load_purpose_owner_nodes_by_paths_controlled(
                fixture.project,
                generation,
                &[purpose_paths[0].clone(), purpose_paths[0].clone()],
                full_budget,
                None,
            ),
            "duplicate purpose-owner paths were accepted",
        )?;
        require(
            matches!(duplicate_purpose, DbError::GraphContract(_)),
            &format!("unexpected duplicate purpose-owner error: {duplicate_purpose}"),
        )?;
        let purpose_cancellation = projectatlas_core::IndexCancellation::new();
        purpose_cancellation.cancel();
        let purpose_control = IndexWorkControl::new(purpose_cancellation, None);
        let cancelled_purpose = store.load_purpose_owner_nodes_by_paths_controlled(
            fixture.project,
            generation,
            &purpose_paths,
            full_budget,
            Some(&purpose_control),
        );
        require(
            matches!(
                cancelled_purpose,
                Err(DbError::IndexWork(
                    projectatlas_core::IndexWorkFailure::Cancelled {
                        stage: IndexWorkStage::RepositoryTraversal
                    }
                ))
            ),
            "purpose-owner cancellation was not typed",
        )?;

        let occurrence_batch = store.repository_graph_occurrence_pages_bounded(
            &fixture.relations,
            1,
            full_budget,
            None,
        )?;
        require(
            occurrence_batch.pages.len() == fixture.relations.len()
                && occurrence_batch.pages[0].rows.len() == 1
                && occurrence_batch.pages[0].truncated
                && occurrence_batch.pages[1..]
                    .iter()
                    .all(|page| page.rows.is_empty() && !page.truncated),
            "batched occurrence pages lost owner order or truncation state",
        )?;
        require_eq(
            &occurrence_batch.work,
            &RepositoryGraphReadWork {
                requested_rows: 4,
                returned_rows: 1,
                decoded_bytes: occurrence_batch.work.decoded_bytes,
                hydrated_entities: 0,
                hydrated_paths: 2,
            },
            "exact occurrence batch work including sentinel path",
        )?;
        let exact_occurrence_budget =
            RepositoryGraphReadBudget::new(4, 1, occurrence_batch.work.decoded_bytes, 1, 2)?;
        require_eq(
            &store.repository_graph_occurrence_pages_bounded(
                &fixture.relations,
                1,
                exact_occurrence_budget,
                None,
            )?,
            &occurrence_batch,
            "exact occurrence envelope",
        )?;
        require_eq(
            &store.repository_graph_occurrence_pages(&fixture.relations, 1, None)?,
            &occurrence_batch.pages,
            "legacy occurrence wrapper compatibility",
        )?;
        for (budget, limit, context) in [
            (
                RepositoryGraphReadBudget::new(
                    4,
                    1,
                    occurrence_batch.work.decoded_bytes - 1,
                    1,
                    2,
                )?,
                1,
                "occurrence decoded-byte overrun was accepted",
            ),
            (
                RepositoryGraphReadBudget::new(4, 1, occurrence_batch.work.decoded_bytes, 1, 1)?,
                1,
                "occurrence sentinel path overrun was accepted",
            ),
            (
                RepositoryGraphReadBudget::new(
                    4,
                    1,
                    RepositoryGraphReadBudget::MAX_DECODED_BYTES,
                    1,
                    RepositoryGraphReadBudget::MAX_HYDRATED_PATHS,
                )?,
                3,
                "occurrence returned-row overrun was accepted",
            ),
        ] {
            let error = require_db_error(
                store.repository_graph_occurrence_pages_bounded(
                    &fixture.relations,
                    limit,
                    budget,
                    None,
                ),
                context,
            )?;
            require(
                matches!(error, DbError::GraphContract(_)),
                &format!("unexpected occurrence envelope error: {error}"),
            )?;
        }
        let occurrence_cancellation = projectatlas_core::IndexCancellation::new();
        occurrence_cancellation.cancel();
        let occurrence_control = IndexWorkControl::new(occurrence_cancellation, None);
        let cancelled_occurrences = store.repository_graph_occurrence_pages_bounded(
            &fixture.relations,
            1,
            full_budget,
            Some(&occurrence_control),
        );
        require(
            matches!(
                cancelled_occurrences,
                Err(DbError::IndexWork(
                    projectatlas_core::IndexWorkFailure::Cancelled {
                        stage: IndexWorkStage::RepositoryTraversal
                    }
                ))
            ),
            "occurrence batch cancellation was not typed",
        )?;
        let expired_occurrences = IndexWorkControl::with_deadline(
            projectatlas_core::IndexCancellation::new(),
            Instant::now(),
        );
        let occurrence_deadline = store.repository_graph_occurrence_pages_bounded(
            &fixture.relations,
            1,
            full_budget,
            Some(&expired_occurrences),
        );
        require(
            matches!(
                occurrence_deadline,
                Err(DbError::IndexWork(
                    projectatlas_core::IndexWorkFailure::DeadlineExceeded {
                        stage: IndexWorkStage::RepositoryTraversal
                    }
                ))
            ),
            "occurrence batch deadline was not typed",
        )?;

        let coverage_paths = vec![
            RepositoryNodePath::new(Path::new("src/Äuth.rs"))?,
            RepositoryNodePath::new(Path::new("Cargo.toml"))?,
        ];
        let coverage_batch = store.repository_graph_path_coverage_bounded(
            fixture.project,
            generation,
            &coverage_paths,
            full_budget,
            None,
        )?;
        require_eq(
            &coverage_batch
                .page
                .rows
                .iter()
                .map(|coverage| match coverage.scope() {
                    CoverageScope::Path { path } => path.as_str().to_string(),
                    CoverageScope::Project => "project".to_string(),
                })
                .collect::<Vec<_>>(),
            &vec!["Cargo.toml".to_string(), "src/Äuth.rs".to_string()],
            "stable path coverage order",
        )?;
        require(
            !coverage_batch.page.truncated
                && coverage_batch.work
                    == (RepositoryGraphReadWork {
                        requested_rows: 2,
                        returned_rows: 2,
                        decoded_bytes: coverage_batch.work.decoded_bytes,
                        hydrated_entities: 0,
                        hydrated_paths: 2,
                    })
                && coverage_batch.work.decoded_bytes > 0,
            "exact coverage batch work was incomplete",
        )?;
        let exact_coverage_budget =
            RepositoryGraphReadBudget::new(2, 2, coverage_batch.work.decoded_bytes, 1, 2)?;
        require_eq(
            &store.repository_graph_path_coverage_bounded(
                fixture.project,
                generation,
                &coverage_paths,
                exact_coverage_budget,
                None,
            )?,
            &coverage_batch,
            "exact coverage envelope",
        )?;
        require_eq(
            &store.repository_graph_path_coverage(fixture.project, &coverage_paths, None)?,
            &coverage_batch.page,
            "legacy coverage wrapper compatibility",
        )?;
        for (budget, context) in [
            (
                RepositoryGraphReadBudget::new(2, 2, coverage_batch.work.decoded_bytes - 1, 1, 2)?,
                "coverage decoded-byte overrun was accepted",
            ),
            (
                RepositoryGraphReadBudget::new(2, 1, coverage_batch.work.decoded_bytes, 1, 2)?,
                "coverage returned-row overrun was accepted",
            ),
            (
                RepositoryGraphReadBudget::new(2, 2, coverage_batch.work.decoded_bytes, 1, 1)?,
                "coverage hydrated-path overrun was accepted",
            ),
        ] {
            let error = require_db_error(
                store.repository_graph_path_coverage_bounded(
                    fixture.project,
                    generation,
                    &coverage_paths,
                    budget,
                    None,
                ),
                context,
            )?;
            require(
                matches!(error, DbError::GraphContract(_)),
                &format!("unexpected coverage envelope error: {error}"),
            )?;
        }
        let coverage_cancellation = projectatlas_core::IndexCancellation::new();
        coverage_cancellation.cancel();
        let coverage_control = IndexWorkControl::new(coverage_cancellation, None);
        let cancelled_coverage = store.repository_graph_path_coverage_bounded(
            fixture.project,
            generation,
            &coverage_paths,
            full_budget,
            Some(&coverage_control),
        );
        require(
            matches!(
                cancelled_coverage,
                Err(DbError::IndexWork(
                    projectatlas_core::IndexWorkFailure::Cancelled {
                        stage: IndexWorkStage::RepositoryTraversal
                    }
                ))
            ),
            "coverage batch cancellation was not typed",
        )?;
        let expired_coverage = IndexWorkControl::with_deadline(
            projectatlas_core::IndexCancellation::new(),
            Instant::now(),
        );
        let coverage_deadline = store.repository_graph_path_coverage_bounded(
            fixture.project,
            generation,
            &coverage_paths,
            full_budget,
            Some(&expired_coverage),
        );
        require(
            matches!(
                coverage_deadline,
                Err(DbError::IndexWork(
                    projectatlas_core::IndexWorkFailure::DeadlineExceeded {
                        stage: IndexWorkStage::RepositoryTraversal
                    }
                ))
            ),
            "coverage batch deadline was not typed",
        )?;

        let missing = [0xff; 32];
        let missing_entity = require_db_error(
            store.repository_graph_entities_by_digest(
                fixture.project,
                generation,
                &[entity_digests[0], missing],
                full_budget,
                None,
            ),
            "missing entity cursor key returned a partial set",
        )?;
        require(
            matches!(
                missing_entity,
                DbError::GraphRowShape {
                    table: "graph_entities",
                    ..
                }
            ),
            &format!("unexpected missing-entity error: {missing_entity}"),
        )?;
        let missing_relation = require_db_error(
            store.repository_graph_relation_rows_by_digest(
                fixture.project,
                generation,
                &[relation_digests[0], missing],
                full_budget,
                None,
            ),
            "missing relation cursor key returned a partial set",
        )?;
        require(
            matches!(
                missing_relation,
                DbError::GraphRowShape {
                    table: "graph_relations",
                    ..
                }
            ),
            &format!("unexpected missing-relation error: {missing_relation}"),
        )?;

        let duplicate = require_db_error(
            store.repository_graph_entities_by_digest(
                fixture.project,
                generation,
                &[entity_digests[0], entity_digests[0]],
                full_budget,
                None,
            ),
            "duplicate entity cursor keys were accepted",
        )?;
        require(
            matches!(duplicate, DbError::GraphContract(_)),
            &format!("unexpected duplicate hydration error: {duplicate}"),
        )?;
        let oversized = vec![[0; 32]; MAX_REPOSITORY_GRAPH_FRONTIER + 1];
        let oversized = require_db_error(
            store.repository_graph_relation_rows_by_digest(
                fixture.project,
                generation,
                &oversized,
                full_budget,
                None,
            ),
            "oversized relation cursor key set was accepted",
        )?;
        require(
            matches!(oversized, DbError::GraphContract(_)),
            &format!("unexpected oversized hydration error: {oversized}"),
        )?;

        let foreign_project = ProjectInstanceId::from_bytes([0x7f; 16])?;
        let foreign = require_db_error(
            store.repository_graph_entities_by_digest(
                foreign_project,
                generation,
                &entity_digests,
                full_budget,
                None,
            ),
            "cross-project entity cursor hydration was accepted",
        )?;
        require(
            matches!(foreign, DbError::GraphProjectIdentityMismatch { .. }),
            &format!("unexpected cross-project hydration error: {foreign}"),
        )?;
        let stale_generation = generation
            .checked_next()
            .ok_or_else(|| io::Error::other("fixture generation overflowed"))?;
        let stale = require_db_error(
            store.repository_graph_relation_rows_by_digest(
                fixture.project,
                stale_generation,
                &relation_digests,
                full_budget,
                None,
            ),
            "stale-generation relation cursor hydration was accepted",
        )?;
        require(
            matches!(stale, DbError::GraphContract(_)),
            &format!("unexpected stale-generation hydration error: {stale}"),
        )?;
        for entities in [true, false] {
            let cancellation = projectatlas_core::IndexCancellation::new();
            cancellation.cancel();
            let control = IndexWorkControl::new(cancellation, None);
            let cancelled = if entities {
                store
                    .repository_graph_entities_by_digest(
                        fixture.project,
                        generation,
                        &entity_digests,
                        full_budget,
                        Some(&control),
                    )
                    .map(|_| ())
            } else {
                store
                    .repository_graph_relation_rows_by_digest(
                        fixture.project,
                        generation,
                        &relation_digests,
                        full_budget,
                        Some(&control),
                    )
                    .map(|_| ())
            };
            require(
                matches!(
                    cancelled,
                    Err(DbError::IndexWork(
                        projectatlas_core::IndexWorkFailure::Cancelled {
                            stage: IndexWorkStage::RepositoryTraversal
                        }
                    ))
                ),
                "cursor hydration cancellation was not typed",
            )?;
        }
        let expired = IndexWorkControl::with_deadline(
            projectatlas_core::IndexCancellation::new(),
            Instant::now(),
        );
        let deadline = store.repository_graph_entities_by_digest(
            fixture.project,
            generation,
            &entity_digests,
            full_budget,
            Some(&expired),
        );
        require(
            matches!(
                deadline,
                Err(DbError::IndexWork(
                    projectatlas_core::IndexWorkFailure::DeadlineExceeded {
                        stage: IndexWorkStage::RepositoryTraversal
                    }
                ))
            ),
            "cursor hydration deadline was not typed",
        )?;

        let source = fixture
            .entities
            .iter()
            .find(|entity| matches!(entity.selector(), EntitySelector::File { .. }))
            .ok_or_else(|| io::Error::other("source file fixture missing"))?;
        let adjacency = store.repository_graph_adjacency_page_bounded(
            &[source.key().clone()],
            RepositoryGraphDirection::Outbound,
            None,
            1,
            full_budget,
            None,
        )?;
        require(
            adjacency.page.rows.len() == 1
                && adjacency.page.truncated
                && adjacency.work.requested_rows == 1
                && adjacency.work.returned_rows == 1
                && adjacency.work.decoded_bytes > 0
                && adjacency.work.hydrated_entities > 0
                && adjacency.work.hydrated_paths > 0,
            "bounded adjacency page omitted exact raw or endpoint work",
        )?;
        let exact_adjacency_budget = RepositoryGraphReadBudget::new(
            1,
            1,
            adjacency.work.decoded_bytes,
            adjacency.work.hydrated_entities,
            adjacency.work.hydrated_paths,
        )?;
        require_eq(
            &store.repository_graph_adjacency_page_bounded(
                &[source.key().clone()],
                RepositoryGraphDirection::Outbound,
                None,
                1,
                exact_adjacency_budget,
                None,
            )?,
            &adjacency,
            "exact adjacency envelope",
        )?;
        let adjacency_overrun = require_db_error(
            store.repository_graph_adjacency_page_bounded(
                &[source.key().clone()],
                RepositoryGraphDirection::Outbound,
                None,
                1,
                RepositoryGraphReadBudget::new(
                    1,
                    1,
                    adjacency.work.decoded_bytes - 1,
                    adjacency.work.hydrated_entities,
                    adjacency.work.hydrated_paths,
                )?,
                None,
            ),
            "adjacency decoded-byte overrun was accepted",
        )?;
        require(
            matches!(adjacency_overrun, DbError::GraphContract(_)),
            &format!("unexpected adjacency envelope error: {adjacency_overrun}"),
        )?;
        let adjacency_return_limit = require_db_error(
            store.repository_graph_adjacency_page_bounded(
                &[source.key().clone()],
                RepositoryGraphDirection::Outbound,
                None,
                2,
                exact_adjacency_budget,
                None,
            ),
            "adjacency page limit exceeded the return budget",
        )?;
        require(
            matches!(adjacency_return_limit, DbError::GraphContract(_)),
            &format!("unexpected adjacency return-budget error: {adjacency_return_limit}"),
        )?;
        let continuation = adjacency
            .page
            .continuation
            .ok_or_else(|| io::Error::other("adjacency continuation missing"))?;
        let encoded =
            serde_json::to_vec(&(RepositoryGraphDirection::Outbound, continuation.clone()))?;
        let decoded: (
            RepositoryGraphDirection,
            RepositoryGraphAdjacencyContinuation,
        ) = serde_json::from_slice(&encoded)?;
        require_eq(
            &decoded,
            &(RepositoryGraphDirection::Outbound, continuation),
            "opaque relation cursor serde round trip",
        )?;

        assert_cursor_hydration_indexes(&store)?;
        store.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn continued_adjacency_seeks_past_high_degree_prefixes() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_root = temp.path().join("continued-adjacency-work");
        let atlas_dir = project_root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let db_path = atlas_dir.join("projectatlas.db");
        let mut writer = AtlasStore::open_for_project(&db_path, &project_root)?;
        let fixture = publish_fixture(&mut writer, "continued-adjacency-work")?;
        let source = fixture
            .entities
            .iter()
            .find(|entity| matches!(entity.selector(), EntitySelector::File { .. }))
            .ok_or_else(|| io::Error::other("source file fixture missing"))?;
        let external = fixture
            .entities
            .iter()
            .find(|entity| matches!(entity.selector(), EntitySelector::External { .. }))
            .ok_or_else(|| io::Error::other("external fixture missing"))?;
        writer.connection.execute(
            "WITH RECURSIVE sequence(value) AS (
                 VALUES(1)
                 UNION ALL
                 SELECT value + 1 FROM sequence WHERE value < 100000
             )
             INSERT INTO graph_relations(
                 relation_key, project_instance_id, canonical_identity,
                 source_entity_key, relation_scope, relation_kind,
                 resolution_status, target_entity_key, reference_text,
                 candidate_count, confidence, completeness
             )
             SELECT CAST(printf('%032d', 1000000 + value) AS BLOB),
                    ?1, printf('perf-%06d', value), ?2,
                    'legacy', 'calls', 'resolved', ?3, NULL, NULL,
                    'exact', 'complete'
               FROM sequence",
            params![
                fixture.project.as_bytes().as_slice(),
                source.key().digest_bytes()?.as_slice(),
                external.key().digest_bytes()?.as_slice(),
            ],
        )?;
        drop(writer);

        let store = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
        let generation = store
            .repository_graph_generation()?
            .ok_or_else(|| io::Error::other("graph generation missing"))?;
        let source_digest = source.key().digest_bytes()?;
        let external_digest = external.key().digest_bytes()?;
        let skipped_frontier = vec![source.key().clone(), external.key().clone()];
        let skipped_continuation = RepositoryGraphAdjacencyContinuation {
            project: fixture.project,
            generation,
            direction: RepositoryGraphDirection::Outbound,
            relation: None,
            frontier: vec![source_digest, external_digest],
            frontier_index: 1,
            relation_scope: String::new(),
            relation_kind: String::new(),
            canonical_identity: String::new(),
            relation_key: [0; 32],
        };
        let skipped_steps = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let skipped_counter = std::sync::Arc::clone(&skipped_steps);
        store.connection.progress_handler(
            1_000,
            Some(move || {
                skipped_counter.fetch_add(1_000, std::sync::atomic::Ordering::Relaxed);
                false
            }),
        );
        let skipped_result = store.repository_graph_adjacency_page(
            &skipped_frontier,
            RepositoryGraphDirection::Outbound,
            Some(&skipped_continuation),
            2,
            None,
        );
        store.connection.progress_handler(0, None::<fn() -> bool>);
        skipped_result?;
        require(
            skipped_steps.load(std::sync::atomic::Ordering::Relaxed) < 100_000,
            "continued adjacency scanned a completed high-degree frontier branch",
        )?;

        let last_key: [u8; 32] = format!("{:032}", 1_100_000)
            .into_bytes()
            .try_into()
            .map_err(|_source| io::Error::other("high-degree cursor key width changed"))?;
        let deep_continuation = RepositoryGraphAdjacencyContinuation {
            project: fixture.project,
            generation,
            direction: RepositoryGraphDirection::Outbound,
            relation: None,
            frontier: vec![source_digest],
            frontier_index: 0,
            relation_scope: "legacy".to_string(),
            relation_kind: "calls".to_string(),
            canonical_identity: "perf-100000".to_string(),
            relation_key: last_key,
        };
        let deep_steps = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let deep_counter = std::sync::Arc::clone(&deep_steps);
        store.connection.progress_handler(
            1_000,
            Some(move || {
                deep_counter.fetch_add(1_000, std::sync::atomic::Ordering::Relaxed);
                false
            }),
        );
        let deep_result = store.repository_graph_adjacency_page(
            &[source.key().clone()],
            RepositoryGraphDirection::Outbound,
            Some(&deep_continuation),
            2,
            None,
        );
        store.connection.progress_handler(0, None::<fn() -> bool>);
        deep_result?;
        require(
            deep_steps.load(std::sync::atomic::Ordering::Relaxed) < 100_000,
            "continued adjacency rescanned a high-degree keyset prefix",
        )?;
        store.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn bounded_rust_frontier_matches_indexed_recursive_cte_on_cycles_and_high_degree()
    -> Result<(), Box<dyn Error>> {
        const HIGH_DEGREE: usize = 4_096;
        const RECURSIVE_CTE: &str = "WITH RECURSIVE walk(entity_key) AS (
                VALUES(?2)
                UNION
                SELECT relation.target_entity_key
                  FROM graph_relations AS relation INDEXED BY idx_graph_relations_source_kind
                  JOIN walk ON relation.source_entity_key = walk.entity_key
                 WHERE relation.project_instance_id = ?1
                   AND relation.relation_scope = 'legacy'
                   AND relation.relation_kind = 'calls'
                   AND relation.target_entity_key IS NOT NULL
            )
            SELECT entity_key FROM walk ORDER BY entity_key";

        let temp = tempfile::tempdir()?;
        let project_root = temp.path().join("frontier-cte-comparison");
        let atlas_dir = project_root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let db_path = atlas_dir.join("projectatlas.db");
        let mut writer = AtlasStore::open_for_project(&db_path, &project_root)?;
        let fixture = publish_fixture(&mut writer, "frontier-cte-comparison")?;
        let source = fixture
            .entities
            .iter()
            .find(|entity| matches!(entity.selector(), EntitySelector::File { .. }))
            .ok_or_else(|| io::Error::other("source file fixture missing"))?;
        let generation = source.generation();
        let mut high_degree_entities = Vec::with_capacity(HIGH_DEGREE);
        let mut cyclic_relations = Vec::with_capacity(HIGH_DEGREE * 2);
        for index in 0..HIGH_DEGREE {
            let target = GraphEntity::new(
                fixture.project,
                EntitySelector::External {
                    external: ExternalSelector {
                        system: GraphIdentityText::new("frontier-measurement")?,
                        identity: GraphIdentityText::new(format!("node-{index:05}"))?,
                    },
                },
                generation,
            )?;
            cyclic_relations.push(LogicalRelation::new(
                source,
                GraphRelationKind::Legacy(RelationKind::Calls),
                RelationResolution::external(&target)?,
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?);
            cyclic_relations.push(LogicalRelation::new(
                &target,
                GraphRelationKind::Legacy(RelationKind::Calls),
                RelationResolution::resolved(source)?,
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?);
            high_degree_entities.push(target);
        }
        let transaction = writer.connection.transaction()?;
        insert_entities(&transaction, fixture.project, &high_degree_entities)?;
        insert_relations(&transaction, fixture.project, &cyclic_relations)?;
        transaction.commit()?;
        drop(writer);

        let store = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
        let project = fixture.project.as_bytes();
        let source_key = source.key().digest_bytes()?;
        let plan_sql = format!("EXPLAIN QUERY PLAN {RECURSIVE_CTE}");
        let cte_plan = store
            .connection
            .prepare(&plan_sql)?
            .query_map(params![&project[..], &source_key[..]], |row| {
                row.get::<_, String>(3)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        require(
            cte_plan
                .iter()
                .any(|detail| detail.contains("idx_graph_relations_source_kind"))
                && cte_plan
                    .iter()
                    .all(|detail| !detail.contains("SCAN relation")),
            &format!("recursive CTE did not retain the source-owned index: {cte_plan:?}"),
        )?;

        let cte_steps = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let cte_counter = std::sync::Arc::clone(&cte_steps);
        store.connection.progress_handler(
            1_000,
            Some(move || {
                cte_counter.fetch_add(1_000, std::sync::atomic::Ordering::Relaxed);
                false
            }),
        );
        let cte_started = Instant::now();
        let cte_result = store
            .connection
            .prepare(RECURSIVE_CTE)?
            .query_map(params![&project[..], &source_key[..]], |row| {
                row.get::<_, Vec<u8>>(0)
            })?
            .map(|row| fixed_bytes::<32>("recursive_cte.entity_key", row?))
            .collect::<DbResult<Vec<_>>>();
        let cte_elapsed = cte_started.elapsed();
        store.connection.progress_handler(0, None::<fn() -> bool>);
        let cte_nodes = cte_result?;

        let rust_steps = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let rust_counter = std::sync::Arc::clone(&rust_steps);
        store.connection.progress_handler(
            1_000,
            Some(move || {
                rust_counter.fetch_add(1_000, std::sync::atomic::Ordering::Relaxed);
                false
            }),
        );
        let rust_started = Instant::now();
        let rust_result = collect_bounded_outbound_calls(&store, source.key());
        let rust_elapsed = rust_started.elapsed();
        store.connection.progress_handler(0, None::<fn() -> bool>);
        let (rust_nodes, inspected_edges, peak_frontier) = rust_result?;
        let (repeated_nodes, repeated_edges, repeated_peak) =
            collect_bounded_outbound_calls(&store, source.key())?;

        require_eq(
            &rust_nodes,
            &cte_nodes,
            "Rust frontier versus recursive CTE topology",
        )?;
        require_eq(
            &repeated_nodes,
            &rust_nodes,
            "deterministic Rust frontier order",
        )?;
        require_eq(
            &repeated_edges,
            &inspected_edges,
            "deterministic inspected edges",
        )?;
        require_eq(
            &repeated_peak,
            &peak_frontier,
            "deterministic peak frontier",
        )?;
        require_eq(
            &rust_nodes.len(),
            &(HIGH_DEGREE + 2),
            "cycle-safe high-degree topology",
        )?;
        require_eq(
            &inspected_edges,
            &(HIGH_DEGREE * 2 + 1),
            "bounded frontier inspected every call edge once",
        )?;
        require(
            cte_steps.load(std::sync::atomic::Ordering::Relaxed) > 0
                && rust_steps.load(std::sync::atomic::Ordering::Relaxed) > 0
                && cte_elapsed > Duration::ZERO
                && rust_elapsed > Duration::ZERO,
            "frontier comparison did not record VM work and elapsed time",
        )?;
        let retained_key_bytes = rust_nodes
            .len()
            .checked_add(peak_frontier)
            .and_then(|keys| keys.checked_mul(std::mem::size_of::<[u8; 32]>()))
            .ok_or_else(|| io::Error::other("retained key measurement overflowed"))?;
        let output_key_bytes = rust_nodes
            .len()
            .checked_mul(std::mem::size_of::<[u8; 32]>())
            .ok_or_else(|| io::Error::other("output key measurement overflowed"))?;
        require(
            retained_key_bytes <= GraphLimits::MAX_OUTPUT_BYTES as usize
                && output_key_bytes <= GraphLimits::MAX_OUTPUT_BYTES as usize,
            "bounded Rust frontier exceeded the shared compact byte ceiling",
        )?;

        store.connection.progress_handler(1, Some(|| true));
        let cancelled_cte = (|| -> Result<Vec<Vec<u8>>, rusqlite::Error> {
            let mut statement = store.connection.prepare(RECURSIVE_CTE)?;
            statement
                .query_map(params![&project[..], &source_key[..]], |row| {
                    row.get::<_, Vec<u8>>(0)
                })?
                .collect()
        })();
        store.connection.progress_handler(0, None::<fn() -> bool>);
        require(
            matches!(
                cancelled_cte,
                Err(rusqlite::Error::SqliteFailure(ref failure, _))
                    if failure.code == rusqlite::ErrorCode::OperationInterrupted
            ),
            "recursive CTE did not stop through the SQLite progress handler",
        )?;
        let cancellation = projectatlas_core::IndexCancellation::new();
        cancellation.cancel();
        let control = IndexWorkControl::new(cancellation, None);
        let cancelled_rust = store.repository_graph_adjacency_page_filtered(
            &[source.key().clone()],
            RepositoryGraphDirection::Outbound,
            Some(GraphRelationKind::Legacy(RelationKind::Calls)),
            None,
            1,
            Some(&control),
        );
        require(
            matches!(
                cancelled_rust,
                Err(DbError::IndexWork(
                    projectatlas_core::IndexWorkFailure::Cancelled {
                        stage: IndexWorkStage::RepositoryTraversal
                    }
                ))
            ),
            "bounded Rust frontier did not retain typed cancellation",
        )?;
        store.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn coverage_discovery_filters_provenance_and_fails_closed_after_reopen()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_root = temp.path().join("coverage-discovery");
        fs::create_dir_all(&project_root)?;
        let db_path = project_root.join("projectatlas.db");
        let mut writer = AtlasStore::open_for_project(&db_path, &project_root)?;
        let fixture = publish_fixture(&mut writer, "coverage-discovery")?;
        let sibling_path = "src/Äuth.rs.backup";
        let sibling_coverage = CoverageRecord::new(
            CoverageScope::Path {
                path: RepositoryNodePath::new(Path::new(sibling_path))?,
            },
            None,
            CoverageState::Complete,
            1,
            0,
            IndexGeneration::new(2),
            None,
            None,
        )?;
        let mut publication = writer.begin_index_publication("coverage-discovery-sibling")?;
        publication.replace_repository_graph_for_paths(
            fixture.project,
            &[sibling_path.to_string()],
            &[],
            &[],
            &[],
            &[sibling_coverage],
        )?;
        publication.complete()?;
        drop(writer);

        let store = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
        let project_value = Value::Blob(fixture.project.as_bytes().to_vec());
        assert_coverage_discovery_plan(
            &store.connection,
            "EXPLAIN QUERY PLAN
             SELECT coverage.id FROM graph_coverage AS coverage
              WHERE coverage.project_instance_id = ?
                AND coverage.scope_kind = 'path'
                AND coverage.scope_path >= ? AND coverage.scope_path < ?
                AND (coverage.scope_path = ? OR coverage.scope_path >= ?)
              ORDER BY coverage.scope_path, coverage.relation_scope, coverage.relation_kind,
                       coverage.state, coverage.id LIMIT 11",
            &[
                project_value.clone(),
                Value::Text("src/Äuth.rs".to_string()),
                Value::Text("src/Äuth.rs0".to_string()),
                Value::Text("src/Äuth.rs".to_string()),
                Value::Text("src/Äuth.rs/".to_string()),
            ],
            &["idx_graph_coverage_scope_order"],
            false,
            "coverage path filter",
        )?;
        assert_coverage_discovery_plan(
            &store.connection,
            "EXPLAIN QUERY PLAN
             SELECT coverage.id FROM graph_coverage AS coverage
              WHERE coverage.project_instance_id = ? AND coverage.state = ?
              ORDER BY coverage.state, coverage.scope_path, coverage.id LIMIT 11",
            &[project_value.clone(), Value::Text("failed".to_string())],
            &["idx_graph_coverage_discovery_state"],
            false,
            "coverage state filter",
        )?;
        assert_coverage_discovery_plan(
            &store.connection,
            "EXPLAIN QUERY PLAN
             SELECT coverage.id FROM graph_coverage AS coverage
              WHERE coverage.project_instance_id = ? AND coverage.reason = ?
              ORDER BY coverage.reason, coverage.scope_path, coverage.id LIMIT 11",
            &[
                project_value.clone(),
                Value::Text("parser failed".to_string()),
            ],
            &["idx_graph_coverage_discovery_reason"],
            false,
            "coverage reason filter",
        )?;
        for (column, index, context) in [
            (
                "source_parser",
                "idx_source_parse_metadata_source_parser_path",
                "coverage parser filter",
            ),
            (
                "fact_parser",
                "idx_source_parse_metadata_fact_parser_path",
                "coverage provider filter",
            ),
        ] {
            assert_coverage_discovery_plan(
                &store.connection,
                &format!(
                    "EXPLAIN QUERY PLAN
                     SELECT coverage.id
                       FROM source_parse_metadata AS metadata
                       CROSS JOIN graph_coverage AS coverage
                         ON coverage.scope_kind = 'path'
                        AND coverage.scope_path = metadata.path
                      WHERE coverage.project_instance_id = ?
                        AND metadata.{column} = ?
                      ORDER BY metadata.path, coverage.id LIMIT 11"
                ),
                &[
                    project_value.clone(),
                    Value::Text("tree-sitter".to_string()),
                ],
                &[index, "idx_graph_coverage_scope_order"],
                true,
                context,
            )?;
        }
        let all = store.repository_coverage_page(
            fixture.project,
            &RepositoryCoverageQuery {
                start_index: 0,
                limit: 2,
                path_prefix: None,
                parser: None,
                provider: None,
                relation: None,
                state: None,
                reason: None,
            },
        )?;
        require(
            all.truncated && all.rows.len() == 2,
            "coverage discovery did not use LIMIT + 1",
        )?;
        let all_states = store.repository_coverage_page(
            fixture.project,
            &RepositoryCoverageQuery {
                start_index: 0,
                limit: 10,
                path_prefix: None,
                parser: None,
                provider: None,
                relation: None,
                state: None,
                reason: None,
            },
        )?;
        for state in [
            CoverageState::Complete,
            CoverageState::Partial,
            CoverageState::Failed,
            CoverageState::Ignored,
            CoverageState::Oversized,
            CoverageState::Quarantined,
            CoverageState::Stale,
        ] {
            require(
                all_states
                    .rows
                    .iter()
                    .any(|row| row.coverage.state() == state),
                &format!("coverage discovery omitted {state:?}"),
            )?;
        }

        let exact_path = store.repository_coverage_page(
            fixture.project,
            &RepositoryCoverageQuery {
                start_index: 0,
                limit: 1,
                path_prefix: Some("src/Äuth.rs".to_string()),
                parser: None,
                provider: None,
                relation: None,
                state: None,
                reason: None,
            },
        )?;
        require(
            !exact_path.truncated && exact_path.rows.len() == 1,
            "exact coverage path admitted a lexical sibling",
        )?;
        require_eq(
            exact_path.rows[0].coverage.scope(),
            &CoverageScope::Path {
                path: RepositoryNodePath::new(Path::new("src/Äuth.rs"))?,
            },
            "exact coverage path scope",
        )?;

        let parsed = store.repository_coverage_page(
            fixture.project,
            &RepositoryCoverageQuery {
                start_index: 0,
                limit: 10,
                path_prefix: Some("src/Äuth.rs".to_string()),
                parser: Some(ParserKind::TreeSitter),
                provider: Some(ParserKind::TreeSitter),
                relation: None,
                state: Some(CoverageState::Complete),
                reason: None,
            },
        )?;
        require_eq(&parsed.rows.len(), &1, "parser/provider filtered coverage")?;
        require_eq(
            &parsed.rows[0].parser,
            &Some(ParserKind::TreeSitter),
            "source parser provenance",
        )?;
        require_eq(
            &parsed.rows[0].provider,
            &Some(ParserKind::TreeSitter),
            "fact provider provenance",
        )?;

        let failed_calls = store.repository_coverage_page(
            fixture.project,
            &RepositoryCoverageQuery {
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
        require_eq(&failed_calls.rows.len(), &1, "combined coverage filters")?;
        require_eq(
            &failed_calls.rows[0].coverage.state(),
            &CoverageState::Failed,
            "failed coverage state",
        )?;

        let absent = store.repository_coverage_page(
            fixture.project,
            &RepositoryCoverageQuery {
                start_index: 0,
                limit: 10,
                path_prefix: None,
                parser: None,
                provider: Some(ParserKind::Manifest),
                relation: None,
                state: None,
                reason: None,
            },
        )?;
        require(
            absent.rows.is_empty(),
            "provider filter returned a false match",
        )?;
        store.finish_index_read_snapshot()?;
        drop(store);

        let writer = AtlasStore::open_for_project(&db_path, &project_root)?;
        writer.connection.execute(
            "UPDATE source_parse_metadata SET source_parser = 'corrupt-parser'
              WHERE path = 'src/Äuth.rs'",
            [],
        )?;
        drop(writer);
        let store = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
        let error = require_db_error(
            store.repository_coverage_page(
                fixture.project,
                &RepositoryCoverageQuery {
                    start_index: 0,
                    limit: 10,
                    path_prefix: Some("src/Äuth.rs".to_string()),
                    parser: None,
                    provider: None,
                    relation: None,
                    state: None,
                    reason: None,
                },
            ),
            "corrupt parser provenance was accepted",
        )?;
        require(
            matches!(error, DbError::InvalidEnum { .. }),
            &format!("unexpected parser corruption error: {error}"),
        )?;
        Ok(())
    }

    #[test]
    fn navigation_connections_cover_families_prefixes_truncation_and_reopen()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("navigation-connections");
        fs::create_dir_all(&root)?;
        let db_path = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&db_path, &root)?;
        let fixture = publish_navigation_fixture(&mut store, "navigation-connections")?;
        let owners = vec![
            RepositoryNavigationNode {
                path: fixture.api_path.clone(),
                kind: NodeKind::File,
            },
            RepositoryNavigationNode {
                path: "src/auth".to_string(),
                kind: NodeKind::Folder,
            },
            RepositoryNavigationNode {
                path: fixture.manifest_path.clone(),
                kind: NodeKind::File,
            },
            RepositoryNavigationNode {
                path: ".".to_string(),
                kind: NodeKind::Folder,
            },
        ];
        let pages = store.repository_navigation_connections(&owners, 2, 20)?;
        require_eq(&pages.len(), &owners.len(), "navigation owner count")?;
        let api = &pages[0];
        let families = api
            .counts
            .iter()
            .map(|count| count.kind)
            .collect::<Vec<_>>();
        require_eq(
            &families,
            &NAVIGATION_CONNECTION_FAMILIES
                .iter()
                .map(|&(kind, _, _)| kind)
                .collect::<Vec<_>>(),
            "all navigation families",
        )?;
        let calls = api
            .counts
            .iter()
            .find(|count| count.kind == RankedConnectionKind::Call)
            .ok_or_else(|| io::Error::other("call navigation count is missing"))?;
        require_eq(&calls.count, &2, "bounded high-degree call count")?;
        require_eq(&calls.truncated, &true, "high-degree call truncation")?;
        require_eq(&api.truncated, &true, "file aggregate truncation")?;
        require(
            api.connections.iter().any(|connection| {
                connection.kind == RankedConnectionKind::Test
                    && connection.direction == RankedConnectionDirection::Inbound
            }),
            "inbound test connection was not projected",
        )?;

        let folder = &pages[1];
        let folder_imports = folder
            .counts
            .iter()
            .find(|count| count.kind == RankedConnectionKind::Import)
            .ok_or_else(|| io::Error::other("folder import count is missing"))?;
        require_eq(
            &folder_imports.count,
            &1,
            "folder prefix import count excluding sibling authz.rs",
        )?;
        let manifest = &pages[2];
        require(
            manifest
                .counts
                .iter()
                .any(|count| count.kind == RankedConnectionKind::Package && count.count == 1),
            "manifest-owned package context is missing",
        )?;
        require(
            pages[3]
                .counts
                .iter()
                .any(|count| count.kind == RankedConnectionKind::Call),
            "root aggregate omitted bounded call context",
        )?;

        let globally_sampled = store.repository_navigation_connections(&owners[..1], 10, 3)?;
        require_eq(
            &globally_sampled[0].connections.len(),
            &3,
            "global connection sample limit",
        )?;
        require_eq(
            &globally_sampled[0].counts.len(),
            &NAVIGATION_CONNECTION_FAMILIES.len(),
            "global sample retained all family counts",
        )?;
        require(
            globally_sampled[0]
                .counts
                .iter()
                .all(|count| !count.truncated),
            "global sample incorrectly reported family overflow",
        )?;
        require_eq(
            &globally_sampled[0].truncated,
            &true,
            "global sample truncation",
        )?;

        drop(store);
        let reader = AtlasStore::open_read_only_for_project(&db_path, &root)?;
        require_eq(
            &reader.project_instance_id()?,
            &Some(fixture.project),
            "reopened navigation identity",
        )?;
        let reopened = reader.repository_navigation_connections(&owners[..1], 2, 20)?;
        require_eq(
            &reopened[0].counts,
            &api.counts,
            "reopened navigation counts",
        )?;
        Ok(())
    }

    #[test]
    fn navigation_connections_use_owned_indexes_and_fail_all_or_error_on_corruption()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("navigation-plan-corruption");
        fs::create_dir_all(&root)?;
        let db_path = root.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&db_path, &root)?;
        let fixture = publish_navigation_fixture(&mut store, "navigation-plan-corruption")?;
        let owner = RepositoryNavigationNode {
            path: fixture.api_path,
            kind: NodeKind::File,
        };
        for (direction, expected_index) in [
            (
                RankedConnectionDirection::Outbound,
                "idx_graph_relations_source_kind",
            ),
            (
                RankedConnectionDirection::Inbound,
                "idx_graph_relations_target_kind",
            ),
        ] {
            let mut values = Vec::new();
            let sql = navigation_connection_branch(
                0,
                &owner,
                RankedConnectionKind::Call,
                "legacy",
                "calls",
                direction,
                3,
                &mut values,
            );
            let mut statement = store
                .connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
            let details = statement
                .query_map(params_from_iter(values.iter()), |row| {
                    row.get::<_, String>(3)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            for expected in [
                expected_index,
                "idx_graph_entities_path",
                "idx_graph_entities_manifest_path",
            ] {
                require(
                    details.iter().any(|detail| detail.contains(expected)),
                    &format!("navigation plan missed {expected}: {details:?}"),
                )?;
            }
            require(
                details.iter().all(|detail| !detail.contains("SCAN graph_")),
                &format!("navigation plan scanned graph storage: {details:?}"),
            )?;
        }
        let folder_owner = RepositoryNavigationNode {
            path: "src/auth".to_string(),
            kind: NodeKind::Folder,
        };
        for (direction, expected_index) in [
            (
                RankedConnectionDirection::Outbound,
                "idx_graph_relations_source_kind",
            ),
            (
                RankedConnectionDirection::Inbound,
                "idx_graph_relations_target_kind",
            ),
        ] {
            let mut values = Vec::new();
            let sql = navigation_connection_branch(
                0,
                &folder_owner,
                RankedConnectionKind::Call,
                "legacy",
                "calls",
                direction,
                3,
                &mut values,
            );
            let details = store
                .connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?
                .query_map(params_from_iter(values.iter()), |row| {
                    row.get::<_, String>(3)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            for expected in [
                expected_index,
                "idx_graph_entities_path",
                "idx_graph_entities_manifest_path",
            ] {
                require(
                    details.iter().any(|detail| detail.contains(expected)),
                    &format!("folder navigation plan missed {expected}: {details:?}"),
                )?;
            }
            require(
                details.iter().all(|detail| !detail.contains("SCAN graph_")),
                &format!("folder navigation plan scanned graph storage: {details:?}"),
            )?;
        }
        let root_owner = RepositoryNavigationNode {
            path: ".".to_string(),
            kind: NodeKind::Folder,
        };
        let mut root_values = Vec::new();
        let root_sql = navigation_connection_branch(
            0,
            &root_owner,
            RankedConnectionKind::Call,
            "legacy",
            "calls",
            RankedConnectionDirection::Outbound,
            2,
            &mut root_values,
        );
        let root_details = store
            .connection
            .prepare(&format!("EXPLAIN QUERY PLAN {root_sql}"))?
            .query_map(params_from_iter(root_values.iter()), |row| {
                row.get::<_, String>(3)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        require(
            root_details
                .iter()
                .any(|detail| detail.contains("idx_graph_relations_kind_order")),
            &format!("root navigation plan missed family index: {root_details:?}"),
        )?;
        require(
            root_details
                .iter()
                .all(|detail| !detail.contains("SCAN graph_")),
            &format!("root navigation plan scanned graph storage: {root_details:?}"),
        )?;

        store
            .connection
            .execute_batch("PRAGMA ignore_check_constraints = ON")?;
        store.connection.execute(
            "UPDATE graph_relations
                SET resolution_status = 'unresolved', reference_text = 'broken-route'
              WHERE relation_scope = 'extended' AND relation_kind = 'routes-to'",
            [],
        )?;
        let error = require_db_error(
            store.repository_navigation_connections(&[owner], 4, 20),
            "corrupt navigation relation returned a partial page",
        )?;
        require(
            matches!(error, DbError::GraphRowShape { .. }),
            &format!("corrupt navigation relation returned {error}"),
        )?;
        Ok(())
    }

    #[test]
    fn graph_queries_fail_closed_on_corrupt_normalized_rows() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_root = temp.path().join("graph-corruption");
        let atlas_dir = project_root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let db_path = atlas_dir.join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&db_path, &project_root)?;
        let fixture = publish_fixture(&mut store, "graph-corruption")?;
        drop(store);
        let store = AtlasStore::open_for_project(&db_path, &project_root)?;
        let source = fixture
            .entities
            .iter()
            .find(|entity| matches!(entity.selector(), EntitySelector::File { .. }))
            .ok_or_else(|| io::Error::other("source file fixture missing"))?;
        let folder = fixture
            .entities
            .iter()
            .find(|entity| matches!(entity.selector(), EntitySelector::Folder { .. }))
            .ok_or_else(|| io::Error::other("folder fixture missing"))?;
        let symbol = fixture
            .entities
            .iter()
            .find(|entity| matches!(entity.selector(), EntitySelector::Symbol { .. }))
            .ok_or_else(|| io::Error::other("symbol fixture missing"))?;
        let ambiguous = fixture
            .relations
            .iter()
            .find(|relation| matches!(relation.resolution(), RelationResolution::Ambiguous { .. }))
            .ok_or_else(|| io::Error::other("ambiguous relation fixture missing"))?;
        let source_digest = source.key().digest_bytes()?;
        let folder_digest = folder.key().digest_bytes()?;
        let symbol_digest = symbol.key().digest_bytes()?;
        let ambiguous_digest = ambiguous.key().digest_bytes()?;
        let source_canonical = store.connection.query_row(
            "SELECT canonical_identity FROM graph_entities WHERE entity_key = ?1",
            [&source_digest[..]],
            |row| row.get::<_, String>(0),
        )?;
        let symbol_canonical = store.connection.query_row(
            "SELECT canonical_identity FROM graph_entities WHERE entity_key = ?1",
            [&symbol_digest[..]],
            |row| row.get::<_, String>(0),
        )?;
        store
            .connection
            .execute_batch("PRAGMA ignore_check_constraints = ON")?;

        store.connection.execute(
            "UPDATE graph_entities SET entity_kind = 'corrupt' WHERE entity_key = ?1",
            [&source_digest[..]],
        )?;
        {
            let reader = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
            let error = require_db_error(
                reader.repository_graph_entity(source.key()),
                "malformed graph enum was accepted",
            )?;
            require(
                matches!(error, DbError::InvalidEnum { .. }),
                &format!("unexpected malformed-enum error: {error}"),
            )?;
            reader.finish_index_read_snapshot()?;
        }
        store.connection.execute(
            "UPDATE graph_entities SET entity_kind = 'file' WHERE entity_key = ?1",
            [&source_digest[..]],
        )?;

        store.connection.execute(
            "UPDATE graph_relations SET candidate_count = 0 WHERE relation_key = ?1",
            [&ambiguous_digest[..]],
        )?;
        {
            let reader = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
            let error = require_db_error(
                reader.repository_graph_relations(
                    RepositoryGraphRelationQuery::Outbound {
                        source: source.key().clone(),
                    },
                    10,
                ),
                "zero ambiguity count was accepted",
            )?;
            require(
                matches!(error, DbError::GraphRowShape { .. }),
                &format!("unexpected candidate-count error: {error}"),
            )?;
            let adjacency_error = require_db_error(
                reader.repository_graph_adjacency_page(
                    &[source.key().clone()],
                    RepositoryGraphDirection::Outbound,
                    None,
                    10,
                    None,
                ),
                "corrupt adjacency relation returned a partial page",
            )?;
            require(
                matches!(adjacency_error, DbError::GraphRowShape { .. }),
                &format!("unexpected adjacency row-shape error: {adjacency_error}"),
            )?;
            reader.finish_index_read_snapshot()?;
        }
        store.connection.execute(
            "UPDATE graph_relations SET candidate_count = 2 WHERE relation_key = ?1",
            [&ambiguous_digest[..]],
        )?;

        store.connection.execute(
            "UPDATE graph_relations SET resolution_status = 'resolved'
              WHERE relation_key = ?1",
            [&ambiguous_digest[..]],
        )?;
        {
            let reader = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
            let error = require_db_error(
                reader.repository_graph_relations(
                    RepositoryGraphRelationQuery::Outbound {
                        source: source.key().clone(),
                    },
                    10,
                ),
                "contradictory resolution columns were accepted",
            )?;
            require(
                matches!(error, DbError::GraphRowShape { .. }),
                &format!("unexpected resolution-shape error: {error}"),
            )?;
            reader.finish_index_read_snapshot()?;
        }
        store.connection.execute(
            "UPDATE graph_relations SET resolution_status = 'ambiguous'
              WHERE relation_key = ?1",
            [&ambiguous_digest[..]],
        )?;

        store.connection.execute(
            "UPDATE graph_coverage SET total = 999
              WHERE scope_kind = 'project' AND relation_scope IS NULL",
            [],
        )?;
        {
            let reader = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
            let error = require_db_error(
                reader.repository_graph_coverage(fixture.project, &CoverageScope::Project, 10),
                "contradictory coverage total was accepted",
            )?;
            require(
                matches!(error, DbError::GraphRowShape { .. }),
                &format!("unexpected coverage-total error: {error}"),
            )?;
            reader.finish_index_read_snapshot()?;
        }
        store.connection.execute(
            "UPDATE graph_coverage SET total = covered + omitted
              WHERE scope_kind = 'project' AND relation_scope IS NULL",
            [],
        )?;

        store.connection.execute(
            "UPDATE project_identity SET active_generation = 99 WHERE singleton = 1",
            [],
        )?;
        {
            let reader = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
            let error = require_db_error(
                reader.repository_graph_entity(source.key()),
                "mismatched typed graph generation was accepted",
            )?;
            require(
                matches!(error, DbError::GraphRowShape { .. }),
                &format!("unexpected typed-generation error: {error}"),
            )?;
            reader.finish_index_read_snapshot()?;
        }
        store.connection.execute(
            "UPDATE project_identity SET active_generation = 1 WHERE singleton = 1",
            [],
        )?;

        store.connection.execute(
            "UPDATE graph_entities SET canonical_identity = 'different-collision-witness'
              WHERE entity_key = ?1",
            [&source_digest[..]],
        )?;
        {
            let reader = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
            let error = require_db_error(
                reader.repository_graph_entity(source.key()),
                "canonical collision witness was accepted",
            )?;
            require(
                matches!(error, DbError::GraphContract(_)),
                &format!("unexpected collision-witness error: {error}"),
            )?;
            reader.finish_index_read_snapshot()?;
        }
        store.connection.execute(
            "UPDATE graph_entities SET canonical_identity = ?1 WHERE entity_key = ?2",
            params![source_canonical, &source_digest[..]],
        )?;

        store.connection.execute(
            "UPDATE graph_entities SET entity_key = zeroblob(32) WHERE entity_key = ?1",
            [&folder_digest[..]],
        )?;
        let folder_path = RepositoryNodePath::new(Path::new("src"))?;
        {
            let reader = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
            let error = require_db_error(
                reader.repository_graph_entities_by_path(fixture.project, &folder_path, 10),
                "invalid stable digest was accepted",
            )?;
            require(
                matches!(error, DbError::GraphContract(_)),
                &format!("unexpected stable-digest error: {error}"),
            )?;
            reader.finish_index_read_snapshot()?;
        }
        store.connection.execute(
            "UPDATE graph_entities SET entity_key = ?1 WHERE entity_key = zeroblob(32)",
            [&folder_digest[..]],
        )?;

        store.connection.execute(
            "UPDATE graph_entities SET entity_key = X'01' WHERE entity_key = ?1",
            [&folder_digest[..]],
        )?;
        {
            let reader = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
            let error = require_db_error(
                reader.repository_graph_entities_by_path(fixture.project, &folder_path, 10),
                "short graph key blob was accepted",
            )?;
            require(
                matches!(
                    error,
                    DbError::InvalidBlobLength {
                        field: "graph_entities.entity_key",
                        expected: 32,
                        found: 1
                    }
                ),
                &format!("unexpected graph-key length error: {error}"),
            )?;
            reader.finish_index_read_snapshot()?;
        }
        store.connection.execute(
            "UPDATE graph_entities SET entity_key = ?1 WHERE entity_key = X'01'",
            [&folder_digest[..]],
        )?;

        store.connection.execute(
            "UPDATE graph_entities SET canonical_identity = X'00' WHERE entity_key = ?1",
            [&symbol_digest[..]],
        )?;
        let source_path = RepositoryNodePath::new(Path::new("src/Äuth.rs"))?;
        {
            let reader = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
            let error = require_db_error(
                reader.repository_graph_entities_by_path(fixture.project, &source_path, 10),
                "later row conversion failure returned a successful partial page",
            )?;
            require(
                matches!(error, DbError::Sqlite(_)),
                &format!("unexpected later-row conversion error: {error}"),
            )?;
            reader.finish_index_read_snapshot()?;
        }
        store.connection.execute(
            "UPDATE graph_entities SET canonical_identity = ?1 WHERE entity_key = ?2",
            params![symbol_canonical, &symbol_digest[..]],
        )?;
        store
            .connection
            .execute_batch("PRAGMA ignore_check_constraints = OFF")?;
        Ok(())
    }

    #[test]
    fn graph_publication_failure_rolls_back_text_graph_and_generation_for_readers()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_root = temp.path().join("graph-publication");
        let atlas_dir = project_root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let db_path = atlas_dir.join("projectatlas.db");
        let mut writer = AtlasStore::open_for_project(&db_path, &project_root)?;
        let fixture_v1 = publish_fixture(&mut writer, "graph-publication")?;
        let old_reader = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
        require_graph_projection(
            &old_reader,
            &fixture_v1,
            IndexGeneration::new(1),
            "fn verifyToken()",
        )?;

        let missing_entity = GraphEntity::new(
            fixture_v1.project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new("src/missing.rs"))?,
            },
            IndexGeneration::new(2),
        )?;
        {
            let mut publication = writer.begin_index_publication("graph-publication")?;
            publication.replace_file_texts_for_paths(
                &["src/Äuth.rs".to_string()],
                &[IndexedFileText {
                    path: "src/Äuth.rs".to_string(),
                    content_hash: Some("hash-new".to_string()),
                    byte_count: "fn verifyTokenUpdated()".len(),
                    line_count: 1,
                    content: "fn verifyTokenUpdated()".to_string(),
                }],
            )?;
            let error = require_db_error(
                publication.replace_repository_graph_for_paths(
                    fixture_v1.project,
                    &["src/Äuth.rs".to_string(), "src/missing.rs".to_string()],
                    &[missing_entity],
                    &[],
                    &[],
                    &[],
                ),
                "missing-node graph publication unexpectedly succeeded",
            )?;
            require(
                matches!(error, DbError::Sqlite(_)),
                &format!("unexpected late graph publication error: {error}"),
            )?;
        }

        require_graph_projection(
            &writer,
            &fixture_v1,
            IndexGeneration::new(1),
            "fn verifyToken()",
        )?;
        let rolled_back_reader = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
        require_graph_projection(
            &rolled_back_reader,
            &fixture_v1,
            IndexGeneration::new(1),
            "fn verifyToken()",
        )?;
        require_graph_projection(
            &old_reader,
            &fixture_v1,
            IndexGeneration::new(1),
            "fn verifyToken()",
        )?;
        rolled_back_reader.finish_index_read_snapshot()?;

        let project = writer
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("bound writer identity is missing"))?;
        let fixture_v2 = graph_fixture(project, IndexGeneration::new(2))?;
        {
            let mut publication = writer.begin_index_publication("graph-publication")?;
            publication.replace_file_texts_for_paths(
                &["src/Äuth.rs".to_string()],
                &[IndexedFileText {
                    path: "src/Äuth.rs".to_string(),
                    content_hash: Some("hash-new".to_string()),
                    byte_count: "fn verifyTokenUpdated()".len(),
                    line_count: 1,
                    content: "fn verifyTokenUpdated()".to_string(),
                }],
            )?;
            publication.replace_repository_graph(
                fixture_v2.project,
                &fixture_v2.entities,
                &fixture_v2.relations,
                &fixture_v2.occurrences,
                &fixture_v2.coverage,
            )?;
            publication.complete()?;
        }

        require_graph_projection(
            &old_reader,
            &fixture_v1,
            IndexGeneration::new(1),
            "fn verifyToken()",
        )?;
        let new_reader = AtlasStore::open_read_only_for_project(&db_path, &project_root)?;
        require_graph_projection(
            &new_reader,
            &fixture_v2,
            IndexGeneration::new(2),
            "fn verifyTokenUpdated()",
        )?;
        new_reader.finish_index_read_snapshot()?;
        old_reader.finish_index_read_snapshot()?;
        Ok(())
    }
}
