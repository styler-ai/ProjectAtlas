//! Purpose: Persist `ProjectAtlas` 3 indexes in `SQLite`.

mod schema;
mod structural_publication;

pub use structural_publication::{
    IncrementalBuiltInPurpose, IncrementalPublication, IncrementalSourceMutation,
    IncrementalStructuralDelta, IncrementalSummaryMutation, StructuralPublicationProgress,
    StructuralPublicationStage, StructuralPublicationTransition, StructuralStaging,
};

use projectatlas_core::budget::DefaultCoreBudgetKind;
use projectatlas_core::graph::{
    Completeness, ConfidenceClass, ContentSpanFingerprint, EvidenceClass, GraphContractError,
    GraphEntityKeyHandle, GraphEntityKeyInput, GraphEntityKind, GraphEvidenceKeyHandle,
    GraphEvidenceOriginInput, GraphKeyArena, GraphLogicalEdgeKeyHandle, GraphRelationKind,
    GraphRelationTargetInput, GraphResolutionKeyHandle, GraphResolverInput, IdentityText,
    IndexEpoch, ProjectInstanceId, PublicationState, RepositoryFilePath, RepositoryPath,
};
use projectatlas_core::health::{
    CATEGORY_DUPLICATE_PURPOSE, CATEGORY_MISSING_PURPOSE, CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED,
    CATEGORY_REPEATED_TEMPORARY_FOLDER, CATEGORY_STALE_PURPOSE, CATEGORY_SUGGESTED_PURPOSE_REVIEW,
    HealthFinding, MESSAGE_MISSING_PURPOSE, MESSAGE_PURPOSE_AGENT_REVIEW_REQUIRED,
    MESSAGE_STALE_PURPOSE, MESSAGE_SUGGESTED_PURPOSE_REVIEW, RECOMMENDATION_DUPLICATE_PURPOSE,
    RECOMMENDATION_MISSING_PURPOSE_QUEUE, RECOMMENDATION_PURPOSE_AGENT_REVIEW_REQUIRED,
    RECOMMENDATION_REPEATED_TEMPORARY_FOLDER, RECOMMENDATION_STALE_PURPOSE,
    RECOMMENDATION_SUGGESTED_PURPOSE_REVIEW_QUEUE, STRUCTURAL_HEALTH_CATEGORIES, Severity,
    TEMP_FOLDER_BUCKETS, finding_id,
};
use projectatlas_core::symbols::{
    CodeSymbol, CompactSymbolGraph, CompactSymbolGraphError, ParserKind, RelationKind,
    SourceParseMetadata, SymbolGraph, SymbolKind, SymbolRelation,
};
use projectatlas_core::telemetry::{
    TokenBucketOverview, TokenOverview, TokenTrendPeriod, TokenTrendReport, TokenTrendWindow,
    UsageEvent, default_estimate_method, default_token_accuracy, default_token_model,
    default_token_provider, default_token_trace, default_tokenizer_backend,
};
use projectatlas_core::{
    AGENT_REVIEWED_SOURCE_VALUES, HIGH_IMPACT_FILE_NAMES, HIGH_IMPACT_PATH_PREFIXES,
    HIGH_IMPACT_PATH_SEGMENTS, IndexedNode, LEGACY_HUMAN_PURPOSE_SOURCE, Node, NodeKind, Overview,
    Purpose, PurposeSource, PurposeStatus, normalize_native_path_display,
    normalize_repo_path_prefix,
};
use rusqlite::types::{Value, ValueRef};
use rusqlite::{
    Connection, ErrorCode, OpenFlags, OptionalExtension, Params, params, params_from_iter,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::num::TryFromIntError;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Maximum persisted text for denormalized symbol-name search summaries.
const MAX_SYMBOL_SEARCH_SUMMARY_CHARS: usize = 16_000;
/// Normal parser producer recorded on typed compatibility-graph rows.
const GRAPH_PARSER_IDENTITY: &str = "projectatlas-symbols";
/// Workspace runtime version recorded on typed compatibility-graph rows.
const GRAPH_PARSER_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Compatibility source name used for file-owned relations.
const GRAPH_FILE_SOURCE_NAME: &str = "<module>";
/// Synthetic source emitted only by the Cargo manifest dependency producer.
const GRAPH_CARGO_MANIFEST_SOURCE_NAME: &str = "cargo";
/// Language identity required for exact Cargo manifest dependency ownership.
const GRAPH_CARGO_MANIFEST_LANGUAGE: &str = "cargo-manifest";
/// Canonical Cargo manifest basename required for file-owned dependencies.
const GRAPH_CARGO_MANIFEST_FILE_NAME: &str = "Cargo.toml";
/// External namespace used by compatibility dependency targets.
const GRAPH_EXTERNAL_PACKAGE_NAMESPACE: &str = "package";
/// Typed entity table used in stable-key collision diagnostics.
const GRAPH_ENTITIES_TABLE: &str = "graph_entities";
/// Typed logical-relation table used in stable-key collision diagnostics.
const GRAPH_RELATIONS_TABLE: &str = "graph_relations";
/// Typed occurrence-evidence table used in stable-key collision diagnostics.
const GRAPH_EVIDENCE_OCCURRENCES_TABLE: &str = "graph_evidence_occurrences";
/// Typed non-traversable occurrence table used in stable-key collision diagnostics.
const GRAPH_RESOLUTION_OCCURRENCES_TABLE: &str = "graph_resolution_occurrences";
/// Typed resolution-candidate table used in graph counts and snapshot inventories.
const GRAPH_RESOLUTION_CANDIDATES_TABLE: &str = "graph_resolution_candidates";
/// Stable entity digest/canonical mismatch probe run before path invalidation.
const GRAPH_ENTITY_IDENTITY_CONFLICT_SQL: &str = "
    SELECT EXISTS(
        SELECT 1 FROM graph_entities
        WHERE structural_slot = ?1 AND stable_key_digest = ?2
          AND (stable_key_version <> ?3 OR stable_key_canonical <> ?4)
    )
";
/// Stable relation digest/canonical mismatch probe run before path invalidation.
const GRAPH_RELATION_IDENTITY_CONFLICT_SQL: &str = "
    SELECT EXISTS(
        SELECT 1 FROM graph_relations
        WHERE structural_slot = ?1 AND stable_key_digest = ?2
          AND (stable_key_version <> ?3 OR stable_key_canonical <> ?4)
    )
";
/// Stable evidence digest/canonical mismatch probe run before path invalidation.
const GRAPH_EVIDENCE_IDENTITY_CONFLICT_SQL: &str = "
    SELECT EXISTS(
        SELECT 1 FROM graph_evidence_occurrences
        WHERE structural_slot = ?1 AND stable_key_digest = ?2
          AND (stable_key_version <> ?3 OR stable_key_canonical <> ?4)
    )
";
/// Stable non-traversable occurrence digest/canonical mismatch probe run before path invalidation.
const GRAPH_RESOLUTION_IDENTITY_CONFLICT_SQL: &str = "
    SELECT EXISTS(
        SELECT 1 FROM graph_resolution_occurrences
        WHERE structural_slot = ?1 AND stable_key_digest = ?2
          AND (stable_key_version <> ?3 OR stable_key_canonical <> ?4)
    )
";
/// Active-slot entity lookup by stable digest.
const GRAPH_ENTITY_BY_STABLE_KEY_SQL: &str = "
    SELECT stable_key_digest, entity_kind, repository_path, qualified_name,
           signature, discriminator, last_changed_epoch
    FROM graph_entities
    WHERE structural_slot = ?1 AND stable_key_digest = ?2
";
/// Active-slot entity lookup by typed kind and qualified identity.
const GRAPH_ENTITIES_BY_QUALIFIED_NAME_SQL: &str = "
    SELECT stable_key_digest, entity_kind, repository_path, qualified_name,
           signature, discriminator, last_changed_epoch
    FROM graph_entities
    WHERE structural_slot = ?1 AND entity_kind = ?2 AND qualified_name = ?3
    ORDER BY stable_key_digest
    LIMIT ?4
";
/// Active-slot outbound adjacency with an exact relation-family filter.
const GRAPH_OUTBOUND_RELATIONS_BY_KIND_SQL: &str = "
    SELECT stable_key_digest, source_entity_digest, relation_kind, target_scope,
           target_entity_digest, external_target_namespace, external_target_value,
           last_changed_epoch
    FROM graph_relations
    WHERE structural_slot = ?1 AND source_entity_digest = ?2 AND relation_kind = ?3
    ORDER BY stable_key_digest
    LIMIT ?4
";
/// Active-slot outbound adjacency across relation families.
const GRAPH_OUTBOUND_RELATIONS_SQL: &str = "
    SELECT stable_key_digest, source_entity_digest, relation_kind, target_scope,
           target_entity_digest, external_target_namespace, external_target_value,
           last_changed_epoch
    FROM graph_relations
    WHERE structural_slot = ?1 AND source_entity_digest = ?2
    ORDER BY relation_kind, stable_key_digest
    LIMIT ?3
";
/// Active-slot inbound adjacency with an exact relation-family filter.
const GRAPH_INBOUND_RELATIONS_BY_KIND_SQL: &str = "
    SELECT stable_key_digest, source_entity_digest, relation_kind, target_scope,
           target_entity_digest, external_target_namespace, external_target_value,
           last_changed_epoch
    FROM graph_relations
    WHERE structural_slot = ?1 AND target_entity_digest = ?2 AND relation_kind = ?3
    ORDER BY stable_key_digest
    LIMIT ?4
";
/// Active-slot inbound adjacency across relation families.
const GRAPH_INBOUND_RELATIONS_SQL: &str = "
    SELECT stable_key_digest, source_entity_digest, relation_kind, target_scope,
           target_entity_digest, external_target_namespace, external_target_value,
           last_changed_epoch
    FROM graph_relations
    WHERE structural_slot = ?1 AND target_entity_digest = ?2
    ORDER BY relation_kind, stable_key_digest
    LIMIT ?3
";
/// Active-slot bounded relation-family lookup.
const GRAPH_RELATIONS_BY_KIND_SQL: &str = "
    SELECT stable_key_digest, source_entity_digest, relation_kind, target_scope,
           target_entity_digest, external_target_namespace, external_target_value,
           last_changed_epoch
    FROM graph_relations
    WHERE structural_slot = ?1 AND relation_kind = ?2
    ORDER BY stable_key_digest
    LIMIT ?3
";

/// Database-layer error type.
#[derive(Debug, Error)]
pub enum DbError {
    /// `SQLite` operation failed.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// `SQLite` reported corruption while materializing a result page.
    #[error(
        "database corruption stopped row iteration: {source}; preserve the database, stop writers, and run `projectatlas reset-index --dry-run` before any rebuild"
    )]
    DatabaseCorruption {
        /// Original `SQLite` corruption diagnostic.
        #[source]
        source: rusqlite::Error,
    },
    /// Schema version is not supported.
    #[error("unsupported schema version {found}, expected {expected}")]
    SchemaVersion {
        /// Version found in database.
        found: i64,
        /// Expected version.
        expected: i64,
    },
    /// Read-only schema inspection found state that is unsafe to mutate.
    #[error("schema preflight rejected the database: {message}")]
    SchemaPreflight {
        /// Actionable incompatibility or readiness detail.
        message: String,
    },
    /// An existing project root could not be resolved to one filesystem identity.
    #[error("cannot resolve project root {path}: {source}")]
    ProjectRootResolution {
        /// Project root that could not be resolved.
        path: PathBuf,
        /// Native filesystem resolution failure.
        #[source]
        source: std::io::Error,
    },
    /// Required persistent project identity metadata is missing.
    #[error("project instance identity is missing from database metadata")]
    ProjectInstanceIdMissing,
    /// Persistent project identity metadata is invalid.
    #[error("invalid project instance identity in database metadata: {value}")]
    InvalidProjectInstanceId {
        /// Invalid stored identity text.
        value: String,
        /// Domain validation failure.
        #[source]
        source: Box<GraphContractError>,
    },
    /// An expanded compatibility graph could not enter the compact persistence path.
    #[error("invalid compact symbol graph: {0}")]
    InvalidCompactSymbolGraph(#[from] CompactSymbolGraphError),
    /// A parser-produced compatibility fact could not enter the typed graph contract.
    #[error("invalid typed graph fact: {0}")]
    InvalidGraphFact(#[from] GraphContractError),
    /// Invalid enum value read from the database.
    #[error("invalid {field} value in database: {value}")]
    InvalidEnum {
        /// Field name.
        field: &'static str,
        /// Invalid value.
        value: String,
    },
    /// Count value from `SQLite` could not fit in `usize`.
    #[error("invalid count for {field}: {value}")]
    InvalidCount {
        /// Count field name.
        field: &'static str,
        /// Invalid database count.
        value: i64,
        /// Source conversion error.
        source: TryFromIntError,
    },
    /// A fixed-width graph identity read from `SQLite` has an invalid byte length.
    #[error("invalid byte length for {field}: expected {expected}, found {found}")]
    InvalidByteLength {
        /// Stored graph field.
        field: &'static str,
        /// Required byte length.
        expected: usize,
        /// Observed byte length.
        found: usize,
    },
    /// A persisted relation violates its typed target contract.
    #[error("invalid persisted graph relation: {message}")]
    InvalidGraphRelation {
        /// Broken storage invariant.
        message: String,
    },
    /// A stable digest matched different canonical identity material.
    #[error(
        "stable graph identity collision in {table} for structural slot {structural_slot:?}: digest matched different encoding version or canonical bytes"
    )]
    StableGraphIdentityCollision {
        /// Typed graph table whose digest identity conflicted.
        table: &'static str,
        /// Structural slot containing the retained identity.
        structural_slot: String,
    },
    /// A full-scan staging database could not be validated or published safely.
    #[error("structural publication failed: {message}")]
    StructuralPublication {
        /// Validation, reconciliation, or atomic-publication failure.
        message: String,
    },
    /// A caller supplied a path that is not in the current index.
    #[error("path {path:?} is not indexed; run scan, fix the path, or choose an indexed path")]
    PathNotIndexed {
        /// Repository-relative path.
        path: String,
    },
    /// A caller attempted to resolve a health finding that is not currently active.
    #[error(
        "health finding {finding_id:?} with category {category:?} and path {path:?} is not active; run health-check and use an exact finding id/path/category"
    )]
    HealthFindingNotActive {
        /// Requested finding id.
        finding_id: String,
        /// Requested category.
        category: String,
        /// Requested primary path.
        path: String,
    },
}

/// Convenient result alias for database operations.
pub type DbResult<T> = Result<T, DbError>;

/// `SQLite`-backed `ProjectAtlas` index store.
pub struct AtlasStore {
    /// Active database connection for index reads and writes.
    connection: Connection,
}

/// UTF-8 source text persisted for indexed search.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IndexedFileText {
    /// Repository-relative file path using forward slashes.
    pub path: String,
    /// BLAKE3 content hash from the scanned file node.
    pub content_hash: Option<String>,
    /// UTF-8 byte count stored for telemetry.
    pub byte_count: usize,
    /// Number of text lines stored for context extraction.
    pub line_count: usize,
    /// Full UTF-8 source text used by indexed search.
    pub content: String,
}

/// Direction of one bounded active-slot graph adjacency read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphRelationDirection {
    /// Relations whose source is the selected entity.
    Outbound,
    /// Relations whose internal target is the selected entity.
    Inbound,
}

/// Typed persisted target of a resolved graph relation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersistedGraphTarget {
    /// Active-project entity digest.
    Internal([u8; 32]),
    /// Canonical external target identity.
    External {
        /// Ecosystem, protocol, or provider namespace.
        namespace: String,
        /// Canonical identity inside the namespace.
        value: String,
    },
}

/// Bounded typed relation row read from the active structural slot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedGraphRelation {
    /// Stable logical-relation digest.
    pub stable_key_digest: [u8; 32],
    /// Stable source-entity digest.
    pub source_entity_digest: [u8; 32],
    /// Typed relation family.
    pub kind: GraphRelationKind,
    /// Typed internal or external target.
    pub target: PersistedGraphTarget,
    /// Epoch when this row last changed.
    pub last_changed_epoch: IndexEpoch,
}

/// Active-slot typed graph row counts used for reconciliation and scale evidence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GraphFactCounts {
    /// Persisted typed entities in the active structural slot.
    pub entities: usize,
    /// Persisted logical relations in the active structural slot.
    pub relations: usize,
    /// Persisted direct evidence occurrences in the active structural slot.
    pub evidence_occurrences: usize,
    /// Persisted unresolved or ambiguous occurrences in the active structural slot.
    pub resolution_occurrences: usize,
    /// Persisted candidates for ambiguous resolution occurrences in the active structural slot.
    pub resolution_candidates: usize,
}

/// Bounded typed entity row read from the active structural slot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedGraphEntity {
    /// Stable entity digest.
    pub stable_key_digest: [u8; 32],
    /// Typed entity category.
    pub kind: GraphEntityKind,
    /// Repository-relative source path when the entity is source-backed.
    pub repository_path: Option<String>,
    /// Qualified package, module, or declaration identity when available.
    pub qualified_name: Option<String>,
    /// Optional signature that distinguishes overload-shaped declarations.
    pub signature: Option<String>,
    /// Optional producer-owned stable discriminator.
    pub discriminator: Option<String>,
    /// Epoch when this row last changed.
    pub last_changed_epoch: IndexEpoch,
}

/// Agent-approved resolution for a deterministic health finding.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HealthResolution {
    /// Stable health finding id.
    pub finding_id: String,
    /// Finding category.
    pub category: String,
    /// Primary path.
    pub path: String,
    /// Related path, when any.
    pub related_path: Option<String>,
    /// Agent rationale for suppressing future repeats.
    pub rationale: String,
}

/// Bounded health query used by agent-facing adapters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HealthQuery {
    /// Pagination start index after filters are applied.
    pub start_index: usize,
    /// Maximum findings to return.
    pub limit: usize,
    /// Optional finding category filter.
    pub category: Option<String>,
    /// Optional severity filter.
    pub severity: Option<Severity>,
    /// Optional repository-relative path prefix filter.
    pub path_prefix: Option<String>,
    /// Return counts without finding rows.
    pub summary_only: bool,
    /// Health and purpose-curation scope.
    pub scope: HealthScope,
}

/// Scope controls for bounded health and purpose-curation queries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HealthScope {
    /// Include all indexed paths.
    All,
    /// Include only source files and folders with source descendants.
    SourceOnly,
    /// Include all folders plus high-impact files.
    PurposeDefault,
    /// Include all folders, high-impact files, and non-source files.
    PurposeWithAssets,
    /// Include all folders, high-impact files, and all source files.
    PurposeWithSourceFiles,
    /// Include every indexed file and folder.
    PurposeStrict,
}

impl HealthScope {
    /// Scope matching unfiltered health output.
    pub fn all() -> Self {
        Self::All
    }

    /// Scope restricted to source-relevant paths.
    pub fn source_only() -> Self {
        Self::SourceOnly
    }

    /// Default agent purpose curation scope: folders plus high-impact files.
    pub fn purpose_default() -> Self {
        Self::PurposeDefault
    }

    /// Purpose curation scope including non-source asset files.
    pub fn purpose_with_assets() -> Self {
        Self::PurposeWithAssets
    }

    /// Purpose curation scope including all source files.
    pub fn purpose_with_source_files() -> Self {
        Self::PurposeWithSourceFiles
    }

    /// Strict purpose curation scope including every indexed path.
    pub fn purpose_strict() -> Self {
        Self::PurposeStrict
    }

    /// Whether this scope should be reported as source-focused in agent payloads.
    pub fn is_source_focused(self) -> bool {
        self.source_only_filter()
    }

    /// Whether this scope uses the folder-first high-impact purpose queue.
    pub fn is_purpose_queue(self) -> bool {
        self.high_impact_queue()
    }

    /// Whether source relevance should be applied before queue-specific filters.
    fn source_only_filter(self) -> bool {
        matches!(
            self,
            Self::SourceOnly | Self::PurposeDefault | Self::PurposeWithSourceFiles
        )
    }

    /// Whether the scope should use folder-first purpose queue selection.
    fn high_impact_queue(self) -> bool {
        matches!(
            self,
            Self::PurposeDefault
                | Self::PurposeWithAssets
                | Self::PurposeWithSourceFiles
                | Self::PurposeStrict
        )
    }

    /// Whether non-source asset files should be included in queue selection.
    fn include_assets(self) -> bool {
        matches!(self, Self::PurposeWithAssets)
    }

    /// Whether all source files should be included in queue selection.
    fn include_source_files(self) -> bool {
        matches!(self, Self::PurposeWithSourceFiles | Self::PurposeStrict)
    }

    /// Whether all files should be included in queue selection.
    fn include_all_files(self) -> bool {
        matches!(self, Self::PurposeStrict)
    }
}

/// Bounded health findings page returned by the database layer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HealthFindingsPage {
    /// Findings after filters are applied.
    pub total: usize,
    /// Findings before filters are applied, after resolved findings are removed.
    pub unfiltered_total: usize,
    /// Findings returned in this page.
    pub returned: usize,
    /// Pagination start index used for this page.
    pub start_index: usize,
    /// Maximum findings requested for this page.
    pub limit: usize,
    /// Returned health finding rows.
    pub findings: Vec<HealthFinding>,
}

/// Static metadata for one purpose lifecycle health category.
#[derive(Clone, Copy, Debug)]
struct PurposeHealthSpec {
    /// Stored purpose status that emits this health category.
    status: &'static str,
    /// Health finding category for the lifecycle status.
    category: &'static str,
    /// Health finding message for every row in this lifecycle category.
    message: &'static str,
    /// Agent recommendation for resolving this lifecycle category.
    recommendation: &'static str,
}

/// Purpose lifecycle health categories that can be paged directly in `SQLite`.
const PURPOSE_HEALTH_SPECS: [PurposeHealthSpec; 3] = [
    PurposeHealthSpec {
        status: PurposeStatus::Missing.as_str(),
        category: CATEGORY_MISSING_PURPOSE,
        message: MESSAGE_MISSING_PURPOSE,
        recommendation: RECOMMENDATION_MISSING_PURPOSE_QUEUE,
    },
    PurposeHealthSpec {
        status: PurposeStatus::Suggested.as_str(),
        category: CATEGORY_SUGGESTED_PURPOSE_REVIEW,
        message: MESSAGE_SUGGESTED_PURPOSE_REVIEW,
        recommendation: RECOMMENDATION_SUGGESTED_PURPOSE_REVIEW_QUEUE,
    },
    PurposeHealthSpec {
        status: PurposeStatus::Stale.as_str(),
        category: CATEGORY_STALE_PURPOSE,
        message: MESSAGE_STALE_PURPOSE,
        recommendation: RECOMMENDATION_STALE_PURPOSE,
    },
];

impl AtlasStore {
    /// Open or create an index store.
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` setup or schema validation fails.
    pub fn open(path: &Path) -> DbResult<Self> {
        let migration_plan = schema::preflight(path)?;
        let _migration_backup = schema::create_verified_migration_backup(path, &migration_plan)?;
        let mut connection = Connection::open(path)?;
        schema::apply_migration_plan(&mut connection, &migration_plan)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(Self { connection })
    }

    /// Open an in-memory store for tests.
    ///
    /// # Errors
    ///
    /// Returns an error if schema setup fails.
    pub fn in_memory() -> DbResult<Self> {
        let migration_plan = schema::preflight_in_memory()?;
        let mut connection = Connection::open_in_memory()?;
        schema::apply_migration_plan(&mut connection, &migration_plan)?;
        Ok(Self { connection })
    }

    /// Initialize schema.
    ///
    /// # Errors
    ///
    /// Returns an error if schema creation or validation fails.
    pub fn initialize_schema(&self) -> DbResult<()> {
        schema::verify_current_schema(&self.connection)
    }

    /// Load one active-slot graph entity by stable digest.
    ///
    /// # Errors
    ///
    /// Returns an error when `SQLite` fails or persisted typed fields are invalid.
    pub fn load_graph_entity(
        &self,
        stable_key_digest: &[u8; 32],
    ) -> DbResult<Option<PersistedGraphEntity>> {
        self.with_structural_read_snapshot(|connection, publication| {
            let mut statement = connection.prepare_cached(GRAPH_ENTITY_BY_STABLE_KEY_SQL)?;
            let rows = statement.query_and_then(
                params![
                    structural_publication::slot_text(publication.active_slot),
                    &stable_key_digest[..]
                ],
                persisted_graph_entity_from_row,
            )?;
            let mut entities = Vec::new();
            visit_rows_to_terminal(rows, &mut |entity| {
                entities.push(entity);
                Ok(true)
            })?;
            if entities.len() > 1 {
                return Err(DbError::StructuralPublication {
                    message: "active structural slot returned duplicate graph stable keys"
                        .to_owned(),
                });
            }
            Ok(entities.pop())
        })
    }

    /// Explain the exact active-slot stable-key entity query used by [`Self::load_graph_entity`].
    ///
    /// # Errors
    ///
    /// Returns an error when `SQLite` cannot prepare or explain the production query.
    pub fn graph_entity_query_plan(&self, stable_key_digest: &[u8; 32]) -> DbResult<Vec<String>> {
        self.with_structural_read_snapshot(|connection, publication| {
            let sql = format!("EXPLAIN QUERY PLAN {GRAPH_ENTITY_BY_STABLE_KEY_SQL}");
            let mut statement = connection.prepare(&sql)?;
            let rows = statement.query_map(
                params![
                    structural_publication::slot_text(publication.active_slot),
                    &stable_key_digest[..]
                ],
                |row| row.get::<_, String>(3),
            )?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Load active-slot entities by typed kind and exact qualified identity.
    ///
    /// # Errors
    ///
    /// Returns an error when `SQLite` fails or persisted typed fields are invalid.
    pub fn load_graph_entities_by_qualified_name(
        &self,
        kind: GraphEntityKind,
        qualified_name: &str,
        limit: u32,
    ) -> DbResult<Vec<PersistedGraphEntity>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        self.with_structural_read_snapshot(|connection, publication| {
            let mut statement = connection.prepare_cached(GRAPH_ENTITIES_BY_QUALIFIED_NAME_SQL)?;
            let rows = statement.query_and_then(
                params![
                    structural_publication::slot_text(publication.active_slot),
                    kind.as_str(),
                    qualified_name,
                    i64::from(limit)
                ],
                persisted_graph_entity_from_row,
            )?;
            let mut entities = Vec::new();
            visit_rows_to_terminal(rows, &mut |entity| {
                entities.push(entity);
                Ok(true)
            })?;
            Ok(entities)
        })
    }

    /// Load bounded inbound or outbound active-slot adjacency for one entity.
    ///
    /// # Errors
    ///
    /// Returns an error when `SQLite` fails or persisted typed fields are invalid.
    pub fn load_graph_adjacency(
        &self,
        entity_digest: &[u8; 32],
        direction: GraphRelationDirection,
        kind: Option<GraphRelationKind>,
        limit: u32,
    ) -> DbResult<Vec<PersistedGraphRelation>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        self.with_structural_read_snapshot(|connection, publication| {
            let slot = structural_publication::slot_text(publication.active_slot);
            match (direction, kind) {
                (GraphRelationDirection::Outbound, Some(kind)) => load_graph_relations(
                    connection,
                    GRAPH_OUTBOUND_RELATIONS_BY_KIND_SQL,
                    params![slot, &entity_digest[..], kind.as_str(), i64::from(limit)],
                ),
                (GraphRelationDirection::Outbound, None) => load_graph_relations(
                    connection,
                    GRAPH_OUTBOUND_RELATIONS_SQL,
                    params![slot, &entity_digest[..], i64::from(limit)],
                ),
                (GraphRelationDirection::Inbound, Some(kind)) => load_graph_relations(
                    connection,
                    GRAPH_INBOUND_RELATIONS_BY_KIND_SQL,
                    params![slot, &entity_digest[..], kind.as_str(), i64::from(limit)],
                ),
                (GraphRelationDirection::Inbound, None) => load_graph_relations(
                    connection,
                    GRAPH_INBOUND_RELATIONS_SQL,
                    params![slot, &entity_digest[..], i64::from(limit)],
                ),
            }
        })
    }

    /// Explain the exact active-slot outbound relation-family query used by
    /// [`Self::load_graph_adjacency`].
    ///
    /// # Errors
    ///
    /// Returns an error when `SQLite` cannot prepare or explain the production query.
    pub fn graph_outbound_relations_by_kind_query_plan(
        &self,
        entity_digest: &[u8; 32],
        kind: GraphRelationKind,
        limit: u32,
    ) -> DbResult<Vec<String>> {
        self.with_structural_read_snapshot(|connection, publication| {
            let sql = format!("EXPLAIN QUERY PLAN {GRAPH_OUTBOUND_RELATIONS_BY_KIND_SQL}");
            let mut statement = connection.prepare(&sql)?;
            let rows = statement.query_map(
                params![
                    structural_publication::slot_text(publication.active_slot),
                    &entity_digest[..],
                    kind.as_str(),
                    i64::from(limit)
                ],
                |row| row.get::<_, String>(3),
            )?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Load a bounded active-slot relation family without scanning unrelated kinds.
    ///
    /// # Errors
    ///
    /// Returns an error when `SQLite` fails or persisted typed fields are invalid.
    pub fn load_graph_relations_by_kind(
        &self,
        kind: GraphRelationKind,
        limit: u32,
    ) -> DbResult<Vec<PersistedGraphRelation>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        self.with_structural_read_snapshot(|connection, publication| {
            load_graph_relations(
                connection,
                GRAPH_RELATIONS_BY_KIND_SQL,
                params![
                    structural_publication::slot_text(publication.active_slot),
                    kind.as_str(),
                    i64::from(limit)
                ],
            )
        })
    }

    /// Count every active-slot typed graph table in one read snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when `SQLite` cannot read the active publication or a count is invalid.
    pub fn graph_fact_counts(&self) -> DbResult<GraphFactCounts> {
        self.with_structural_read_snapshot(|connection, publication| {
            let slot = structural_publication::slot_text(publication.active_slot);
            let (entities, relations, evidence, resolutions, candidates) = connection.query_row(
                "SELECT
                    (SELECT COUNT(*) FROM graph_entities WHERE structural_slot = ?1),
                    (SELECT COUNT(*) FROM graph_relations WHERE structural_slot = ?1),
                    (SELECT COUNT(*) FROM graph_evidence_occurrences WHERE structural_slot = ?1),
                    (SELECT COUNT(*) FROM graph_resolution_occurrences WHERE structural_slot = ?1),
                    (SELECT COUNT(*) FROM graph_resolution_candidates WHERE structural_slot = ?1)",
                [slot],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )?;
            Ok(GraphFactCounts {
                entities: count_to_usize(GRAPH_ENTITIES_TABLE, entities)?,
                relations: count_to_usize(GRAPH_RELATIONS_TABLE, relations)?,
                evidence_occurrences: count_to_usize(GRAPH_EVIDENCE_OCCURRENCES_TABLE, evidence)?,
                resolution_occurrences: count_to_usize(
                    GRAPH_RESOLUTION_OCCURRENCES_TABLE,
                    resolutions,
                )?,
                resolution_candidates: count_to_usize(
                    GRAPH_RESOLUTION_CANDIDATES_TABLE,
                    candidates,
                )?,
            })
        })
    }

    /// Run one structural read against a transactionally captured publication.
    fn with_structural_read_snapshot<T>(
        &self,
        read: impl FnOnce(&Connection, PublicationState) -> DbResult<T>,
    ) -> DbResult<T> {
        if !self.connection.is_autocommit() {
            let publication = structural_publication::load_publication_state(&self.connection)?;
            return read(&self.connection, publication);
        }
        let transaction = self.connection.unchecked_transaction()?;
        let publication = structural_publication::load_publication_state(&transaction)?;
        let result = read(&transaction, publication)?;
        transaction.commit()?;
        Ok(result)
    }

    /// Upsert a full scan result and mark previously seen missing paths absent.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn replace_scan(&mut self, nodes: &[Node]) -> DbResult<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute("UPDATE nodes SET exists_now = 0", [])?;
        for node in nodes {
            upsert_node(&transaction, node)?;
        }
        transaction.execute(
            "DELETE FROM symbol_relations WHERE path IN (SELECT path FROM nodes WHERE exists_now = 0)",
            [],
        )?;
        transaction.execute(
            "DELETE FROM symbols WHERE path IN (SELECT path FROM nodes WHERE exists_now = 0)",
            [],
        )?;
        transaction.execute(
            "DELETE FROM source_parse_metadata WHERE path IN (SELECT path FROM nodes WHERE exists_now = 0)",
            [],
        )?;
        transaction.execute(
            "DELETE FROM file_texts
             WHERE structural_slot = (
                 SELECT active_slot FROM graph_publication_state WHERE singleton = 1
             )
             AND path IN (SELECT path FROM nodes WHERE exists_now = 0)",
            [],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Upsert a partial scan result without marking unrelated paths absent.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn upsert_scan_nodes(&mut self, nodes: &[Node]) -> DbResult<()> {
        let transaction = self.connection.transaction()?;
        for node in nodes {
            upsert_node(&transaction, node)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Mark paths and their descendants absent after filesystem delete events.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn mark_paths_absent(&mut self, paths: &[String]) -> DbResult<()> {
        let transaction = self.connection.transaction()?;
        mark_paths_absent_in_connection(&transaction, paths)?;
        transaction.commit()?;
        Ok(())
    }

    /// Replace indexed text for scanned file paths.
    ///
    /// `paths` should contain every file path considered by the scan batch.
    /// Existing indexed text for those paths is cleared first so binary,
    /// deleted, or no-longer-UTF-8 files cannot leave stale searchable content.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn replace_file_texts_for_paths(
        &mut self,
        paths: &[String],
        texts: &[IndexedFileText],
    ) -> DbResult<()> {
        let transaction = self.connection.transaction()?;
        delete_file_text_paths_from_active_slot(&transaction, paths)?;
        for text in texts {
            upsert_file_text(&transaction, text)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Load one indexed text row by repository path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or stored counts are invalid.
    pub fn load_file_text(&self, path: &str) -> DbResult<Option<IndexedFileText>> {
        self.with_structural_read_snapshot(|connection, publication| {
            let mut statement = connection.prepare(
                "
                SELECT path, content_hash, byte_count, line_count, content
                FROM file_texts
                WHERE structural_slot = ?1 AND path = ?2
                ",
            )?;
            let mut rows = statement.query_and_then(
                params![
                    structural_publication::slot_text(publication.active_slot),
                    path
                ],
                file_text_from_row,
            )?;
            let first = rows.next().transpose().map_err(database_row_error)?;
            if rows
                .next()
                .transpose()
                .map_err(database_row_error)?
                .is_some()
            {
                return Err(DbError::StructuralPublication {
                    message: format!(
                        "active structural slot returned duplicate file text for {path:?}"
                    ),
                });
            }
            Ok(first)
        })
    }

    /// Load indexed text rows for search.
    ///
    /// When `literal_pattern` is supplied, `SQLite` prefilters candidate files
    /// with a substring search before the service performs line-level matching.
    /// Regex and fuzzy searches pass `None` and still use the persisted text
    /// index instead of reopening source files from disk.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or stored counts are invalid.
    pub fn load_file_texts_for_search(
        &self,
        literal_pattern: Option<&str>,
        case_sensitive: bool,
    ) -> DbResult<Vec<IndexedFileText>> {
        let mut texts = Vec::new();
        self.visit_file_texts_for_search(literal_pattern, case_sensitive, |text| {
            texts.push(text);
            Ok(true)
        })?;
        Ok(texts)
    }

    /// Visit indexed text rows for search without materializing all rows.
    ///
    /// When `literal_pattern` is supplied, `SQLite` prefilters candidate files
    /// with a substring search before the service performs line-level matching.
    /// Returning `false` from `visitor` stops iteration early.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails, stored counts are invalid, or the
    /// visitor returns an error.
    pub fn visit_file_texts_for_search<F>(
        &self,
        literal_pattern: Option<&str>,
        case_sensitive: bool,
        mut visitor: F,
    ) -> DbResult<()>
    where
        F: FnMut(IndexedFileText) -> DbResult<bool>,
    {
        self.with_structural_read_snapshot(|connection, publication| {
            let active_slot = structural_publication::slot_text(publication.active_slot);
            if let Some(pattern) = literal_pattern.filter(|pattern| !pattern.is_empty()) {
                if case_sensitive {
                    let mut statement = connection.prepare(
                        "
                        SELECT path, content_hash, byte_count, line_count, content
                        FROM file_texts
                        WHERE structural_slot = ?1 AND instr(content, ?2) > 0
                        ORDER BY path
                        ",
                    )?;
                    let rows = statement
                        .query_and_then(params![active_slot, pattern], file_text_from_row)?;
                    visit_rows_to_terminal(rows, &mut visitor)?;
                } else {
                    let pattern = pattern.to_ascii_lowercase();
                    let mut statement = connection.prepare(
                        "
                        SELECT path, content_hash, byte_count, line_count, content
                        FROM file_texts
                        WHERE structural_slot = ?1 AND instr(lower(content), ?2) > 0
                        ORDER BY path
                        ",
                    )?;
                    let rows = statement
                        .query_and_then(params![active_slot, pattern], file_text_from_row)?;
                    visit_rows_to_terminal(rows, &mut visitor)?;
                }
            } else {
                let mut statement = connection.prepare(
                    "
                    SELECT path, content_hash, byte_count, line_count, content
                    FROM file_texts
                    WHERE structural_slot = ?1
                    ORDER BY path
                    ",
                )?;
                let rows = statement.query_and_then([active_slot], file_text_from_row)?;
                visit_rows_to_terminal(rows, &mut visitor)?;
            }
            Ok(())
        })
    }

    /// Count files with persisted UTF-8 text for indexed search.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn file_text_count(&self) -> DbResult<usize> {
        self.with_structural_read_snapshot(|connection, publication| {
            let count = connection.query_row(
                "SELECT COUNT(*) FROM file_texts WHERE structural_slot = ?1",
                [structural_publication::slot_text(publication.active_slot)],
                |row| row.get::<_, i64>(0),
            )?;
            count_to_usize("file_texts", count)
        })
    }

    /// Sum persisted UTF-8 source bytes used by indexed search.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn file_text_byte_count(&self) -> DbResult<usize> {
        self.with_structural_read_snapshot(|connection, publication| {
            let count = connection.query_row(
                "SELECT COALESCE(SUM(byte_count), 0) FROM file_texts
                 WHERE structural_slot = ?1",
                [structural_publication::slot_text(publication.active_slot)],
                |row| row.get::<_, i64>(0),
            )?;
            count_to_usize("file_text_bytes", count)
        })
    }

    /// Persist the canonical filesystem root for indexed repository files.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn set_project_root(&self, root: &Path) -> DbResult<()> {
        let value = normalize_metadata_path(root)?;
        self.connection.execute(
            "
            INSERT INTO metadata(key, value)
            VALUES('project_root', ?1)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            ",
            [value],
        )?;
        Ok(())
    }

    /// Load the canonical filesystem root for indexed repository files.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn project_root(&self) -> DbResult<Option<String>> {
        let root = self
            .connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'project_root'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(DbError::from)?;
        root.map(|value| normalize_metadata_path(Path::new(&value)))
            .transpose()
    }

    /// Load the persistent identity of this independently initialized database.
    ///
    /// # Errors
    ///
    /// Returns an error if the identity is missing, malformed, or cannot be read.
    pub fn project_instance_id(&self) -> DbResult<ProjectInstanceId> {
        schema::project_instance_id(&self.connection)
    }

    /// Replace the symbol graph for a file path.
    ///
    /// # Errors
    ///
    /// Returns an error if the graph exceeds the compact representation or persistence fails.
    pub fn replace_symbol_graph(&mut self, graph: &SymbolGraph) -> DbResult<()> {
        let compact = CompactSymbolGraph::try_from(graph.clone())?;
        self.replace_compact_symbol_graph(&compact)
    }

    /// Replace the symbol graph for a file path from compact worker storage.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn replace_compact_symbol_graph(&mut self, graph: &CompactSymbolGraph) -> DbResult<()> {
        structural_publication::publish_active_graph_mutation(
            &self.connection,
            |connection, structural_slot, last_changed_epoch| {
                replace_compact_symbol_graph_at_publication(
                    connection,
                    graph,
                    structural_slot,
                    last_changed_epoch,
                )
            },
        )?;
        Ok(())
    }

    /// Replace one parser graph inside a separate full-scan staging database.
    ///
    /// The parent publication owner assigns the final slot and epoch when it
    /// imports and activates the validated staging database.
    ///
    /// # Errors
    ///
    /// Returns an error if staging persistence fails.
    pub fn stage_compact_symbol_graph(&mut self, graph: &CompactSymbolGraph) -> DbResult<()> {
        self.stage_compact_symbol_graphs(std::iter::once(graph))
    }

    /// Persist one bounded parser-result batch inside a separate full-scan staging database.
    ///
    /// The caller retains ownership of the batch and its memory bound. Every graph is written in
    /// iterator order, and any failure rolls back the complete batch.
    ///
    /// # Errors
    ///
    /// Returns an error if any graph in the batch cannot be persisted or committed.
    pub fn stage_compact_symbol_graphs<'graph>(
        &mut self,
        graphs: impl IntoIterator<Item = &'graph CompactSymbolGraph>,
    ) -> DbResult<()> {
        let mut graphs = graphs.into_iter();
        let Some(first) = graphs.next() else {
            return Ok(());
        };
        let transaction = self.connection.transaction()?;
        for graph in std::iter::once(first).chain(graphs) {
            replace_compact_symbol_graph_in_connection(&transaction, graph)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Clear source-derived intelligence for one live file path.
    ///
    /// This removes symbols, relations, and the node-level content summary so
    /// skipped or failed parser work cannot leave stale source facts visible.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist or persistence fails.
    pub fn clear_source_index_for_path(&self, path: &str) -> DbResult<()> {
        structural_publication::publish_active_graph_mutation(
            &self.connection,
            |connection, _structural_slot, _last_changed_epoch| {
                clear_source_index_in_connection(connection, path, false)
            },
        )?;
        Ok(())
    }

    /// Clear symbols and relations for one live file path while preserving node summaries.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn clear_symbol_graph_for_path(&self, path: &str) -> DbResult<()> {
        structural_publication::publish_active_graph_mutation(
            &self.connection,
            |connection, _structural_slot, _last_changed_epoch| {
                clear_source_index_in_connection(connection, path, true)
            },
        )?;
        Ok(())
    }

    /// Clear source-derived rows inside a separate full-scan staging database.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist or staging persistence fails.
    pub fn clear_staged_source_index_for_path(&self, path: &str) -> DbResult<()> {
        let transaction = self.connection.unchecked_transaction()?;
        clear_source_index_in_connection(&transaction, path, false)?;
        transaction.commit()?;
        Ok(())
    }

    /// Clear parser graph rows inside a separate staging database while preserving summaries.
    ///
    /// # Errors
    ///
    /// Returns an error if staging persistence fails.
    pub fn clear_staged_symbol_graph_for_path(&self, path: &str) -> DbResult<()> {
        let transaction = self.connection.unchecked_transaction()?;
        clear_source_index_in_connection(&transaction, path, true)?;
        transaction.commit()?;
        Ok(())
    }

    /// Persist an observed one-line summary for an indexed node.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist or persistence fails.
    pub fn set_node_summary(&self, path: &str, summary: &str) -> DbResult<()> {
        set_node_summary_in_connection(&self.connection, path, summary)
    }

    /// Remove the observed node-level summary for an indexed node.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist or persistence fails.
    pub fn clear_node_summary(&self, path: &str) -> DbResult<()> {
        clear_node_summary_in_connection(&self.connection, path)
    }

    /// Load symbols filtered by optional file path and query.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbols(
        &self,
        file: Option<&str>,
        query: Option<&str>,
        limit: usize,
    ) -> DbResult<Vec<CodeSymbol>> {
        let max_rows = usize_to_i64(limit.max(1));
        match (file, query) {
            (Some(file), Some(query)) => self.query_symbols(
                "
                SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation
                FROM symbols
                WHERE path = ?1 AND (name LIKE ?2 OR signature LIKE ?2 OR documentation LIKE ?2)
                ORDER BY path, line_start, name
                LIMIT ?3
                ",
                params![file, like_query(query), max_rows],
            ),
            (Some(file), None) => self.query_symbols(
                "
                SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation
                FROM symbols
                WHERE path = ?1
                ORDER BY path, line_start, name
                LIMIT ?2
                ",
                params![file, max_rows],
            ),
            (None, Some(query)) => self.query_symbols(
                "
                SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation
                FROM symbols
                WHERE name LIKE ?1 OR signature LIKE ?1 OR documentation LIKE ?1 OR path LIKE ?1
                ORDER BY path, line_start, name
                LIMIT ?2
                ",
                params![like_query(query), max_rows],
            ),
            (None, None) => self.query_symbols(
                "
                SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation
                FROM symbols
                ORDER BY path, line_start, name
                LIMIT ?1
                ",
                params![max_rows],
            ),
        }
    }

    /// Load symbols for a file and one or more exact kinds.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbols_by_kinds(
        &self,
        file: &str,
        kinds: &[SymbolKind],
        limit: usize,
    ) -> DbResult<Vec<CodeSymbol>> {
        if kinds.is_empty() {
            return Ok(Vec::new());
        }
        let max_rows = usize_to_i64(limit.max(1));
        let placeholders = numbered_placeholders(2, kinds.len());
        let sql = format!(
            "
            SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation
            FROM symbols
            WHERE path = ?1 AND kind IN ({placeholders})
            ORDER BY path, line_start, name
            LIMIT {max_rows}
            "
        );
        let mut values = Vec::with_capacity(kinds.len() + 1);
        values.push(file.to_string());
        values.extend(kinds.iter().map(ToString::to_string));
        self.query_symbols(&sql, params_from_iter(values.iter()))
    }

    /// Count symbols for a file and one or more exact kinds.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn count_symbols_by_kinds(&self, file: &str, kinds: &[SymbolKind]) -> DbResult<usize> {
        if kinds.is_empty() {
            return Ok(0);
        }
        let placeholders = numbered_placeholders(2, kinds.len());
        let sql =
            format!("SELECT COUNT(*) FROM symbols WHERE path = ?1 AND kind IN ({placeholders})");
        let mut values = Vec::with_capacity(kinds.len() + 1);
        values.push(file.to_string());
        values.extend(kinds.iter().map(ToString::to_string));
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values.iter()), |row| {
                row.get::<_, i64>(0)
            })?;
        Ok(i64_to_usize(count))
    }

    /// Count indexed symbols grouped by exact name.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_name_counts(&self, names: &[String]) -> DbResult<HashMap<String, usize>> {
        if names.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders = numbered_placeholders(1, names.len());
        let sql = format!(
            "SELECT name, COUNT(*) FROM symbols WHERE name IN ({placeholders}) GROUP BY name"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(names.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut counts = HashMap::new();
        for row in rows {
            let (name, count) = row?;
            counts.insert(name, i64_to_usize(count));
        }
        Ok(counts)
    }

    /// Load symbols with exact names.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbols_by_names(&self, names: &[String]) -> DbResult<Vec<CodeSymbol>> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = numbered_placeholders(1, names.len());
        let sql = format!(
            "
            SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation
            FROM symbols
            WHERE name IN ({placeholders})
            ORDER BY path, line_start, name
            "
        );
        self.query_symbols(&sql, params_from_iter(names.iter()))
    }

    /// Load exported symbol names for one file.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_exported_symbol_names_for_path(
        &self,
        file: &str,
        limit: usize,
    ) -> DbResult<Vec<String>> {
        let max_rows = usize_to_i64(limit.max(1));
        let mut statement = self.connection.prepare(
            "
            SELECT DISTINCT name
            FROM symbols
            WHERE path = ?1 AND exported = 1
            ORDER BY name
            LIMIT ?2
            ",
        )?;
        let rows = statement.query_map(params![file, max_rows], |row| row.get::<_, String>(0))?;
        let mut names = Vec::new();
        for row in rows {
            names.push(row?);
        }
        Ok(names)
    }

    /// Count exported symbol names for one file.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn exported_symbol_count_for_path(&self, file: &str) -> DbResult<usize> {
        let count = self.connection.query_row(
            "SELECT COUNT(DISTINCT name) FROM symbols WHERE path = ?1 AND exported = 1",
            [file],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(i64_to_usize(count))
    }

    /// Load one symbol by exact file and name.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbol_by_name(&self, file: &str, name: &str) -> DbResult<Option<CodeSymbol>> {
        let mut symbols = self.load_symbols(Some(file), Some(name), 100)?;
        symbols.retain(|symbol| symbol.name == name);
        Ok(symbols.into_iter().next())
    }

    /// Load all symbols with an exact file and name.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbols_by_exact_file_and_name(
        &self,
        file: &str,
        name: &str,
    ) -> DbResult<Vec<CodeSymbol>> {
        self.query_symbols(
            "
            SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation
            FROM symbols
            WHERE path = ?1 AND name = ?2
            ORDER BY line_start, line_end, kind, parent
            ",
            params![file, name],
        )
    }

    /// Load one existing node with purpose state by repository path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading or enum conversion fails.
    pub fn load_node_by_path(&self, path: &str) -> DbResult<Option<IndexedNode>> {
        let mut statement = self.connection.prepare(
            "
            SELECT
                n.path,
                n.kind,
                n.parent_path,
                n.extension,
                n.language,
                n.size_bytes,
                n.mtime_ns,
                n.content_hash,
                p.purpose,
                p.source,
                p.status,
                s.summary
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            LEFT JOIN summaries s ON s.node_id = n.id
                AND s.summary_level = 'node'
                AND s.subject = ''
            WHERE n.exists_now = 1 AND n.path = ?1
            ",
        )?;
        let row = statement
            .query_row([path], |row| {
                let kind_value: String = row.get(1)?;
                let source_value: String = row.get(9)?;
                let status_value: String = row.get(10)?;
                Ok((
                    row.get::<_, String>(0)?,
                    kind_value,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<u64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    source_value,
                    status_value,
                    row.get::<_, Option<String>>(11)?,
                ))
            })
            .optional()?;
        row.map(indexed_node_from_parts).transpose()
    }

    /// Load existing nodes for exact repository paths.
    ///
    /// # Errors
    ///
    /// Returns an error if reading or enum conversion fails.
    pub fn load_nodes_by_paths(&self, paths: &[String]) -> DbResult<Vec<IndexedNode>> {
        let mut unique_paths = paths.to_vec();
        unique_paths.sort();
        unique_paths.dedup();
        let mut nodes = Vec::new();
        for path in unique_paths {
            if let Some(node) = self.load_node_by_path(&path)? {
                nodes.push(node);
            }
        }
        Ok(nodes)
    }

    /// Load symbol relations filtered by optional file path and query.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbol_relations(
        &self,
        file: Option<&str>,
        query: Option<&str>,
        limit: usize,
    ) -> DbResult<Vec<SymbolRelation>> {
        let max_rows = usize_to_i64(limit.max(1));
        match (file, query) {
            (Some(file), Some(query)) => self.query_relations(
                "
                SELECT path, source_name, target_name, kind, line, context, parser
                FROM symbol_relations
                WHERE path = ?1 AND (source_name LIKE ?2 OR target_name LIKE ?2 OR context LIKE ?2)
                ORDER BY path, line, source_name, target_name
                LIMIT ?3
                ",
                params![file, like_query(query), max_rows],
            ),
            (Some(file), None) => self.query_relations(
                "
                SELECT path, source_name, target_name, kind, line, context, parser
                FROM symbol_relations
                WHERE path = ?1
                ORDER BY path, line, source_name, target_name
                LIMIT ?2
                ",
                params![file, max_rows],
            ),
            (None, Some(query)) => self.query_relations(
                "
                SELECT path, source_name, target_name, kind, line, context, parser
                FROM symbol_relations
                WHERE source_name LIKE ?1 OR target_name LIKE ?1 OR context LIKE ?1 OR path LIKE ?1
                ORDER BY path, line, source_name, target_name
                LIMIT ?2
                ",
                params![like_query(query), max_rows],
            ),
            (None, None) => self.query_relations(
                "
                SELECT path, source_name, target_name, kind, line, context, parser
                FROM symbol_relations
                ORDER BY path, line, source_name, target_name
                LIMIT ?1
                ",
                params![max_rows],
            ),
        }
    }

    /// Load symbol relations for a file and exact relation kind.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbol_relations_by_kind(
        &self,
        file: &str,
        kind: RelationKind,
        limit: usize,
    ) -> DbResult<Vec<SymbolRelation>> {
        let max_rows = usize_to_i64(limit.max(1));
        self.query_relations(
            "
            SELECT path, source_name, target_name, kind, line, context, parser
            FROM symbol_relations
            WHERE path = ?1 AND kind = ?2
            ORDER BY path, line, source_name, target_name
            LIMIT ?3
            ",
            params![file, kind.to_string(), max_rows],
        )
    }

    /// Count symbol relations for a file and exact relation kind.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn count_symbol_relations_by_kind(
        &self,
        file: &str,
        kind: RelationKind,
    ) -> DbResult<usize> {
        let count = self.connection.query_row(
            "SELECT COUNT(*) FROM symbol_relations WHERE path = ?1 AND kind = ?2",
            params![file, kind.to_string()],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(i64_to_usize(count))
    }

    /// Load distinct relation targets for a file and exact relation kind.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_distinct_relation_targets_by_kind(
        &self,
        file: &str,
        kind: RelationKind,
        limit: usize,
    ) -> DbResult<Vec<String>> {
        let max_rows = usize_to_i64(limit.max(1));
        let mut statement = self.connection.prepare(
            "
            SELECT DISTINCT target_name
            FROM symbol_relations
            WHERE path = ?1 AND kind = ?2
            ORDER BY target_name
            LIMIT ?3
            ",
        )?;
        let rows = statement.query_map(params![file, kind.to_string(), max_rows], |row| {
            row.get::<_, String>(0)
        })?;
        let mut targets = Vec::new();
        for row in rows {
            targets.push(row?);
        }
        Ok(targets)
    }

    /// Count distinct relation targets for a file and exact relation kind.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn count_distinct_relation_targets_by_kind(
        &self,
        file: &str,
        kind: RelationKind,
    ) -> DbResult<usize> {
        let count = self.connection.query_row(
            "SELECT COUNT(DISTINCT target_name) FROM symbol_relations WHERE path = ?1 AND kind = ?2",
            params![file, kind.to_string()],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(i64_to_usize(count))
    }

    /// Load call relations targeting any of the requested symbol names.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_call_relations_to_targets(
        &self,
        target_names: &[String],
        limit_per_target: usize,
    ) -> DbResult<Vec<SymbolRelation>> {
        if target_names.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = numbered_placeholders(1, target_names.len());
        let limit_placeholder = target_names.len() + 1;
        let sql = format!(
            "
            SELECT path, source_name, target_name, kind, line, context, parser
            FROM (
                SELECT
                    path,
                    source_name,
                    target_name,
                    kind,
                    line,
                    context,
                    parser,
                    ROW_NUMBER() OVER (
                        PARTITION BY target_name
                        ORDER BY path, line, source_name, target_name
                    ) AS target_row
                FROM symbol_relations
                WHERE kind = 'calls' AND target_name IN ({placeholders})
            )
            WHERE target_row <= ?{limit_placeholder}
            ORDER BY path, line, source_name, target_name
            "
        );
        let mut values = target_names
            .iter()
            .map(|target| Value::Text(target.clone()))
            .collect::<Vec<_>>();
        values.push(Value::Integer(usize_to_i64(limit_per_target.max(1))));
        let mut relations = self.query_relations(&sql, params_from_iter(values.iter()))?;
        relations.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.source_name.cmp(&right.source_name))
                .then_with(|| left.target_name.cmp(&right.target_name))
        });
        relations.dedup_by(|left, right| {
            left.path == right.path
                && left.source_name == right.source_name
                && left.target_name == right.target_name
                && left.kind == right.kind
                && left.line == right.line
        });
        Ok(relations)
    }

    /// Load import relations whose persisted target text mentions any term.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_import_relations_matching_targets(
        &self,
        terms: &[String],
        limit_per_term: usize,
    ) -> DbResult<Vec<SymbolRelation>> {
        let mut unique_terms = terms.to_vec();
        unique_terms.sort();
        unique_terms.dedup();
        let mut relations = Vec::new();
        for term in unique_terms.iter().filter(|term| !term.trim().is_empty()) {
            let mut term_relations = self.query_relations(
                "
                SELECT path, source_name, target_name, kind, line, context, parser
                FROM symbol_relations
                WHERE kind = 'imports' AND target_name LIKE ?1 ESCAPE '\\'
                ORDER BY path, line, source_name, target_name
                LIMIT ?2
                ",
                params![
                    sqlite_like_pattern(term),
                    usize_to_i64(limit_per_term.max(1))
                ],
            )?;
            relations.append(&mut term_relations);
        }
        relations.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.source_name.cmp(&right.source_name))
                .then_with(|| left.target_name.cmp(&right.target_name))
        });
        relations.dedup_by(|left, right| {
            left.path == right.path
                && left.source_name == right.source_name
                && left.target_name == right.target_name
                && left.kind == right.kind
                && left.line == right.line
        });
        Ok(relations)
    }

    /// Count persisted symbols.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_count(&self) -> DbResult<usize> {
        let count = self
            .connection
            .query_row("SELECT COUNT(*) FROM symbols", [], |row| {
                row.get::<_, i64>(0)
            })?;
        Ok(i64_to_usize(count))
    }

    /// Count persisted symbol relations.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_relation_count(&self) -> DbResult<usize> {
        let count =
            self.connection
                .query_row("SELECT COUNT(*) FROM symbol_relations", [], |row| {
                    row.get::<_, i64>(0)
                })?;
        count_to_usize("symbol_relations", count)
    }

    /// Count persisted symbols for one file path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_count_for_path(&self, path: &str) -> DbResult<usize> {
        let count = self.connection.query_row(
            "SELECT COUNT(*) FROM symbols WHERE path = ?1",
            [path],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(i64_to_usize(count))
    }

    /// Count persisted symbols for a batch of file paths.
    ///
    /// Paths without symbols are omitted from the returned map.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_counts_for_paths(&self, paths: &[String]) -> DbResult<HashMap<String, usize>> {
        let mut counts = HashMap::new();
        for chunk in paths.chunks(900) {
            if chunk.is_empty() {
                continue;
            }
            let placeholders = vec!["?"; chunk.len()].join(",");
            let sql = format!(
                "SELECT path, COUNT(*) FROM symbols WHERE path IN ({placeholders}) GROUP BY path"
            );
            let mut statement = self.connection.prepare(&sql)?;
            let rows = statement.query_map(params_from_iter(chunk.iter()), |row| {
                let path = row.get::<_, String>(0)?;
                let count = row.get::<_, i64>(1)?;
                Ok((path, i64_to_usize(count)))
            })?;
            for row in rows {
                let (path, count) = row?;
                counts.insert(path, count);
            }
        }
        Ok(counts)
    }

    /// Return distinct parser strategies that produced symbols for one path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_parser_kinds_for_path(&self, path: &str) -> DbResult<Vec<ParserKind>> {
        let mut statement = self.connection.prepare(
            "
            SELECT DISTINCT parser
            FROM symbols
            WHERE path = ?1
            ORDER BY parser
            ",
        )?;
        let rows = statement.query_map([path], |row| {
            Ok(ParserKind::from_db(&row.get::<_, String>(0)?))
        })?;
        let mut parsers = Vec::new();
        for row in rows {
            parsers.push(row?);
        }
        Ok(parsers)
    }

    /// Load file-level parser metadata for one path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or stored counts are invalid.
    pub fn load_source_parse_metadata(&self, path: &str) -> DbResult<Option<SourceParseMetadata>> {
        self.connection
            .query_row(
                "
                SELECT path, language, parser, symbol_count, relation_count
                FROM source_parse_metadata
                WHERE path = ?1
                ",
                [path],
                |row| {
                    let symbol_count = row.get::<_, i64>(3)?;
                    let relation_count = row.get::<_, i64>(4)?;
                    Ok(SourceParseMetadata {
                        path: row.get(0)?,
                        language: row.get(1)?,
                        parser: ParserKind::from_db(&row.get::<_, String>(2)?),
                        symbol_count: i64_to_usize(symbol_count),
                        relation_count: i64_to_usize(relation_count),
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Load the maximum indexed symbol end line for one file path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn max_symbol_end_line_for_path(&self, path: &str) -> DbResult<usize> {
        let line = self.connection.query_row(
            "SELECT COALESCE(MAX(line_end), 0) FROM symbols WHERE path = ?1",
            [path],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(i64_to_usize(line))
    }

    /// Query symbols with a caller-provided statement and parameters.
    fn query_symbols<P>(&self, sql: &str, params: P) -> DbResult<Vec<CodeSymbol>>
    where
        P: rusqlite::Params,
    {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map(params, |row| {
            Ok(CodeSymbol {
                path: row.get(0)?,
                language: row.get(1)?,
                name: row.get(2)?,
                kind: SymbolKind::from_db(&row.get::<_, String>(3)?),
                signature: row.get(4)?,
                line_start: i64_to_usize(row.get::<_, i64>(5)?),
                line_end: i64_to_usize(row.get::<_, i64>(6)?),
                parent: row.get(7)?,
                parser: ParserKind::from_db(&row.get::<_, String>(8)?),
                detail: row.get(9)?,
                exported: row.get::<_, i64>(10)? != 0,
                documentation: row.get(11)?,
            })
        })?;
        let mut symbols = Vec::new();
        for row in rows {
            symbols.push(row?);
        }
        Ok(symbols)
    }

    /// Query relations with a caller-provided statement and parameters.
    fn query_relations<P>(&self, sql: &str, params: P) -> DbResult<Vec<SymbolRelation>>
    where
        P: rusqlite::Params,
    {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map(params, |row| {
            let kind_value: String = row.get(3)?;
            let relation_kind = RelationKind::from_db(&kind_value).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::other(format!(
                        "invalid relation kind {kind_value}"
                    ))),
                )
            })?;
            Ok(SymbolRelation {
                path: row.get(0)?,
                source_name: row.get(1)?,
                target_name: row.get(2)?,
                kind: relation_kind,
                line: i64_to_usize(row.get::<_, i64>(4)?),
                context: row.get(5)?,
                parser: ParserKind::from_db(&row.get::<_, String>(6)?),
            })
        })?;
        let mut relations = Vec::new();
        for row in rows {
            relations.push(row?);
        }
        Ok(relations)
    }

    /// Persist a purpose for a path.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist or persistence fails.
    pub fn set_purpose(&self, path: &str, purpose: &str, source: PurposeSource) -> DbResult<()> {
        set_purpose_in_connection(&self.connection, path, purpose, source)
    }

    /// Persist a non-approved purpose suggestion for a path.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist or persistence fails.
    pub fn set_suggested_purpose(&self, path: &str, purpose: &str) -> DbResult<()> {
        set_suggested_purpose_in_connection(&self.connection, path, purpose)
    }

    /// Load existing nodes with purpose state.
    ///
    /// # Errors
    ///
    /// Returns an error if reading or enum conversion fails.
    pub fn load_nodes(&self) -> DbResult<Vec<IndexedNode>> {
        let mut statement = self.connection.prepare(
            "
            SELECT
                n.path,
                n.kind,
                n.parent_path,
                n.extension,
                n.language,
                n.size_bytes,
                n.mtime_ns,
                n.content_hash,
                p.purpose,
                p.source,
                p.status,
                s.summary
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            LEFT JOIN summaries s ON s.node_id = n.id
                AND s.summary_level = 'node'
                AND s.subject = ''
            WHERE n.exists_now = 1
            ORDER BY n.path
            ",
        )?;
        let rows = statement.query_map([], |row| {
            let kind_value: String = row.get(1)?;
            let source_value: String = row.get(9)?;
            let status_value: String = row.get(10)?;
            Ok((
                row.get::<_, String>(0)?,
                kind_value,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<u64>>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                source_value,
                status_value,
                row.get::<_, Option<String>>(11)?,
            ))
        })?;
        let mut nodes = Vec::new();
        for row in rows {
            let (
                path,
                kind_value,
                parent_path,
                extension,
                language,
                size_bytes,
                mtime_ns,
                content_hash,
                purpose,
                source_value,
                status_value,
                summary,
            ) = row?;
            let kind = NodeKind::from_db(&kind_value).ok_or_else(|| DbError::InvalidEnum {
                field: "kind",
                value: kind_value,
            })?;
            let source = parse_source(&source_value)?;
            let status =
                PurposeStatus::from_db(&status_value).ok_or_else(|| DbError::InvalidEnum {
                    field: "status",
                    value: status_value,
                })?;
            nodes.push(IndexedNode {
                node: Node {
                    path: path.clone(),
                    kind,
                    parent_path,
                    extension,
                    language,
                    size_bytes,
                    mtime_ns,
                    content_hash,
                },
                purpose: Purpose {
                    path,
                    purpose,
                    source,
                    status,
                },
                summary,
            });
        }
        Ok(nodes)
    }

    /// Load a bounded ranked node list directly from `SQLite`.
    ///
    /// This is the hot path for agent orientation commands. It keeps large
    /// repositories from materializing every indexed path just to answer a
    /// top-N folder or file query.
    ///
    /// # Errors
    ///
    /// Returns an error if reading or enum conversion fails.
    pub fn load_ranked_nodes(
        &self,
        query: &str,
        kind: NodeKind,
        folder: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> DbResult<Vec<IndexedNode>> {
        let terms = normalize_query_terms(query);
        let score_expression = ranked_score_expression(terms.len());
        let mut sql = format!(
            "
            SELECT path, kind, parent_path, extension, language, size_bytes, mtime_ns,
                   content_hash, purpose, source, status, summary
            FROM (
                SELECT
                    n.path,
                    n.kind,
                    n.parent_path,
                    n.extension,
                    n.language,
                    n.size_bytes,
                    n.mtime_ns,
                    n.content_hash,
                    p.purpose,
                    p.source,
                    p.status,
                    s.summary,
                    {score_expression} AS score
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                LEFT JOIN summaries s ON s.node_id = n.id
                    AND s.summary_level = 'node'
                    AND s.subject = ''
                LEFT JOIN summaries symbol_summaries ON symbol_summaries.node_id = n.id
                    AND symbol_summaries.summary_level = 'search'
                    AND symbol_summaries.subject = 'symbols'
                WHERE n.exists_now = 1
                  AND n.kind = ?
            "
        );
        let mut values = Vec::new();
        for term in &terms {
            let pattern = sqlite_like_pattern(term);
            values.push(Value::from(pattern.clone()));
            values.push(Value::from(pattern.clone()));
            values.push(Value::from(pattern.clone()));
            values.push(Value::from(pattern));
        }
        values.push(Value::from(kind.to_string()));
        if kind == NodeKind::File
            && let Some(folder) = folder.filter(|folder| !folder.is_empty() && *folder != ".")
        {
            sql.push_str(" AND (n.parent_path = ? OR n.parent_path LIKE ? ESCAPE '\\')");
            values.push(Value::from(folder.to_string()));
            values.push(Value::from(sqlite_descendant_pattern(folder)));
        }
        sql.push_str(
            "
            )
            WHERE score > 0
            ORDER BY score DESC, path
            LIMIT ?
            OFFSET ?
            ",
        );
        values.push(Value::from(usize_to_i64(limit.max(1))));
        values.push(Value::from(usize_to_i64(offset)));

        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values))?;
        let mut nodes = Vec::new();
        while let Some(row) = rows.next()? {
            nodes.push(indexed_node_from_sql_row(row)?);
        }
        Ok(nodes)
    }

    /// Sum indexed source bytes represented by file nodes.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or the aggregate cannot fit in `usize`.
    pub fn source_file_byte_count(&self, folder: Option<&str>) -> DbResult<usize> {
        let mut sql = String::from(
            "
            SELECT COALESCE(SUM(COALESCE(size_bytes, 0)), 0)
            FROM nodes
            WHERE exists_now = 1
              AND kind = 'file'
            ",
        );
        let mut values = Vec::new();
        if let Some(folder) = folder.filter(|folder| !folder.is_empty() && *folder != ".") {
            sql.push_str(" AND (parent_path = ? OR parent_path LIKE ? ESCAPE '\\')");
            values.push(Value::from(folder.to_string()));
            values.push(Value::from(sqlite_descendant_pattern(folder)));
        }
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("source_file_bytes", count)
    }

    /// Visit indexed file paths and source sizes for exact token baselines.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails, stored counts are invalid, or the
    /// visitor returns an error.
    pub fn visit_file_token_estimates<F>(
        &self,
        folder: Option<&str>,
        mut visitor: F,
    ) -> DbResult<()>
    where
        F: FnMut(String, Option<u64>) -> DbResult<bool>,
    {
        let mut sql = String::from(
            "
            SELECT path, size_bytes
            FROM nodes
            WHERE exists_now = 1
              AND kind = 'file'
            ",
        );
        let mut values = Vec::new();
        if let Some(folder) = folder.filter(|folder| !folder.is_empty() && *folder != ".") {
            sql.push_str(" AND (parent_path = ? OR parent_path LIKE ? ESCAPE '\\')");
            values.push(Value::from(folder.to_string()));
            values.push(Value::from(sqlite_descendant_pattern(folder)));
        }
        sql.push_str(" ORDER BY path");
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_and_then(params_from_iter(values), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<u64>>(1)?))
        })?;
        visit_rows_to_terminal(rows, &mut |(path, size_bytes)| visitor(path, size_bytes))
    }

    /// Build unresolved health findings without loading the full node table.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or stored enum values are invalid.
    pub fn unresolved_health_findings(
        &self,
        resolved_ids: &[String],
    ) -> DbResult<Vec<HealthFinding>> {
        let mut findings = Vec::new();
        self.visit_unresolved_health_findings(resolved_ids, |finding| {
            findings.push(finding);
            Ok(true)
        })?;
        Ok(findings)
    }

    /// Build a bounded unresolved health findings page.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or stored enum values are invalid.
    pub fn unresolved_health_findings_page(
        &self,
        resolved_ids: &[String],
        query: &HealthQuery,
    ) -> DbResult<HealthFindingsPage> {
        let mut unfiltered_total = 0_usize;
        let mut total = 0_usize;
        let mut findings = Vec::new();

        for spec in PURPOSE_HEALTH_SPECS {
            unfiltered_total +=
                self.count_purpose_status_findings(spec, None, resolved_ids, HealthScope::all())?;
        }

        let scope = query.scope;
        if scope.high_impact_queue() && query.category.is_none() {
            let matching_count = if query
                .severity
                .is_none_or(|severity| severity == Severity::Warning)
            {
                self.count_purpose_lifecycle_findings(
                    query.path_prefix.as_deref(),
                    resolved_ids,
                    scope,
                )?
            } else {
                0
            };
            if !query.summary_only
                && findings.len() < query.limit
                && total + matching_count > query.start_index
            {
                let local_start = query.start_index.saturating_sub(total);
                let local_limit = query.limit - findings.len();
                findings.extend(self.load_purpose_lifecycle_findings_page(
                    query.path_prefix.as_deref(),
                    resolved_ids,
                    scope,
                    local_start,
                    local_limit,
                )?);
            }
            total += matching_count;
        } else {
            for spec in PURPOSE_HEALTH_SPECS {
                if !purpose_health_spec_matches_query(spec, query) {
                    continue;
                }

                let matching_count = self.count_purpose_status_findings(
                    spec,
                    query.path_prefix.as_deref(),
                    resolved_ids,
                    scope,
                )?;
                if !query.summary_only
                    && findings.len() < query.limit
                    && total + matching_count > query.start_index
                {
                    let local_start = query.start_index.saturating_sub(total);
                    let local_limit = query.limit - findings.len();
                    findings.extend(self.load_purpose_status_findings_page(
                        spec,
                        query.path_prefix.as_deref(),
                        resolved_ids,
                        scope,
                        local_start,
                        local_limit,
                    )?);
                }
                total += matching_count;
            }
        }

        for category in STRUCTURAL_HEALTH_CATEGORIES {
            let unfiltered_scope = if category == CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED {
                HealthScope::purpose_strict()
            } else {
                HealthScope::all()
            };
            let unfiltered_count = self.count_structural_health_findings(
                category,
                None,
                resolved_ids,
                unfiltered_scope,
            )?;
            unfiltered_total += unfiltered_count;
            if !health_category_matches_query(category, Severity::Warning, query) {
                continue;
            }
            let matching_count = self.count_structural_health_findings(
                category,
                query.path_prefix.as_deref(),
                resolved_ids,
                scope,
            )?;
            if !query.summary_only
                && findings.len() < query.limit
                && total + matching_count > query.start_index
            {
                let local_start = query.start_index.saturating_sub(total);
                let local_limit = query.limit - findings.len();
                findings.extend(self.load_structural_health_findings_page(
                    category,
                    query.path_prefix.as_deref(),
                    resolved_ids,
                    scope,
                    local_start,
                    local_limit,
                )?);
            }
            total += matching_count;
        }
        Ok(HealthFindingsPage {
            total,
            unfiltered_total,
            returned: findings.len(),
            start_index: query.start_index,
            limit: query.limit,
            findings,
        })
    }

    /// Visit unresolved health findings without materializing the full table.
    fn visit_unresolved_health_findings<F>(
        &self,
        resolved_ids: &[String],
        mut visitor: F,
    ) -> DbResult<()>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        let resolved = resolved_ids.iter().cloned().collect::<HashSet<_>>();
        if !self.visit_purpose_status_findings(PURPOSE_HEALTH_SPECS[0], &resolved, &mut visitor)? {
            return Ok(());
        }
        if !self.visit_purpose_status_findings(PURPOSE_HEALTH_SPECS[1], &resolved, &mut visitor)? {
            return Ok(());
        }
        if !self.visit_purpose_status_findings(PURPOSE_HEALTH_SPECS[2], &resolved, &mut visitor)? {
            return Ok(());
        }
        if !self.visit_agent_review_required_findings(&resolved, &mut visitor)? {
            return Ok(());
        }
        self.visit_structural_health_findings(&resolved, &mut visitor)
    }

    /// Visit structural health findings that are not simple purpose statuses.
    fn visit_structural_health_findings<F>(
        &self,
        resolved_ids: &HashSet<String>,
        mut visitor: F,
    ) -> DbResult<()>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        if !self.visit_duplicate_purpose_findings(resolved_ids, &mut visitor)? {
            return Ok(());
        }
        if !self.visit_repeated_temp_folder_findings(resolved_ids, &mut visitor)? {
            return Ok(());
        }
        Ok(())
    }

    /// Build findings for one purpose lifecycle status.
    fn visit_purpose_status_findings<F>(
        &self,
        spec: PurposeHealthSpec,
        resolved_ids: &HashSet<String>,
        visitor: &mut F,
    ) -> DbResult<bool>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        let mut statement = self.connection.prepare(
            "
            SELECT n.path
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE n.exists_now = 1
              AND p.status = ?1
            ORDER BY n.path
            ",
        )?;
        let rows = statement.query_map([spec.status], |row| row.get::<_, String>(0))?;
        for row in rows {
            let path = row?;
            let finding = HealthFinding {
                id: finding_id(spec.category, &path, None),
                severity: Severity::Warning,
                category: spec.category.to_string(),
                path,
                related_path: None,
                message: spec.message.to_string(),
                recommendation: spec.recommendation.to_string(),
            };
            if !emit_unresolved_finding(finding, resolved_ids, visitor)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Count unresolved purpose lifecycle findings directly in `SQLite`.
    fn count_purpose_lifecycle_findings(
        &self,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
    ) -> DbResult<usize> {
        let (where_clause, values) =
            purpose_lifecycle_where_clause(path_prefix, resolved_ids, scope);
        let sql = format!(
            "
            SELECT COUNT(*)
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE {where_clause}
            "
        );
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("health_purpose_lifecycle_count", count)
    }

    /// Load one globally ordered purpose lifecycle page directly from `SQLite`.
    fn load_purpose_lifecycle_findings_page(
        &self,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let (where_clause, mut values) =
            purpose_lifecycle_where_clause(path_prefix, resolved_ids, scope);
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let order_by = purpose_default_queue_order_expression("n", "p");
        let sql = format!(
            "
            SELECT n.path, p.status
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE {where_clause}
            ORDER BY {order_by}
            LIMIT ?{limit_placeholder} OFFSET ?{offset_placeholder}
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut findings = Vec::new();
        for row in rows {
            let (path, status) = row?;
            let spec = purpose_health_spec_for_status(&status)?;
            findings.push(HealthFinding {
                id: finding_id(spec.category, &path, None),
                severity: Severity::Warning,
                category: spec.category.to_string(),
                path,
                related_path: None,
                message: spec.message.to_string(),
                recommendation: spec.recommendation.to_string(),
            });
        }
        Ok(findings)
    }

    /// Count unresolved purpose lifecycle findings directly in `SQLite`.
    fn count_purpose_status_findings(
        &self,
        spec: PurposeHealthSpec,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
    ) -> DbResult<usize> {
        let (where_clause, values) =
            purpose_status_where_clause(spec, path_prefix, resolved_ids, scope);
        let sql = format!(
            "
            SELECT COUNT(*)
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE {where_clause}
            "
        );
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("health_purpose_status_count", count)
    }

    /// Load one bounded unresolved purpose lifecycle page directly from `SQLite`.
    fn load_purpose_status_findings_page(
        &self,
        spec: PurposeHealthSpec,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let (where_clause, mut values) =
            purpose_status_where_clause(spec, path_prefix, resolved_ids, scope);
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let order_by = if scope.high_impact_queue() {
            purpose_default_queue_order_expression("n", "p")
        } else {
            "n.path".to_string()
        };
        let sql = format!(
            "
            SELECT n.path
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE {where_clause}
            ORDER BY {order_by}
            LIMIT ?{limit_placeholder} OFFSET ?{offset_placeholder}
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| row.get::<_, String>(0))?;
        let mut findings = Vec::new();
        for row in rows {
            let path = row?;
            findings.push(HealthFinding {
                id: finding_id(spec.category, &path, None),
                severity: Severity::Warning,
                category: spec.category.to_string(),
                path,
                related_path: None,
                message: spec.message.to_string(),
                recommendation: spec.recommendation.to_string(),
            });
        }
        Ok(findings)
    }

    /// Count unresolved structural health findings directly in `SQLite`.
    fn count_structural_health_findings(
        &self,
        category: &str,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
    ) -> DbResult<usize> {
        match category {
            CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED => {
                self.count_agent_review_required_findings(path_prefix, resolved_ids, scope)
            }
            CATEGORY_DUPLICATE_PURPOSE => {
                self.count_duplicate_purpose_findings(path_prefix, resolved_ids, scope)
            }
            CATEGORY_REPEATED_TEMPORARY_FOLDER => {
                self.count_repeated_temp_folder_findings(path_prefix, resolved_ids, scope)
            }
            _ => Ok(0),
        }
    }

    /// Load a bounded unresolved structural health page directly from `SQLite`.
    fn load_structural_health_findings_page(
        &self,
        category: &str,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        match category {
            CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED => self
                .load_agent_review_required_findings_page(
                    path_prefix,
                    resolved_ids,
                    scope,
                    start_index,
                    limit,
                ),
            CATEGORY_DUPLICATE_PURPOSE => self.load_duplicate_purpose_findings_page(
                path_prefix,
                resolved_ids,
                scope,
                start_index,
                limit,
            ),
            CATEGORY_REPEATED_TEMPORARY_FOLDER => self.load_repeated_temp_folder_findings_page(
                path_prefix,
                resolved_ids,
                scope,
                start_index,
                limit,
            ),
            _ => Ok(Vec::new()),
        }
    }

    /// Visit approved navigation-critical purposes that still need agent review.
    fn visit_agent_review_required_findings<F>(
        &self,
        resolved_ids: &HashSet<String>,
        visitor: &mut F,
    ) -> DbResult<bool>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        let reviewed_sources = sql_string_literals(AGENT_REVIEWED_SOURCE_VALUES);
        let high_impact = high_impact_file_path_expression("lower(n.path)");
        let approved_status = PurposeStatus::Approved.as_str();
        let sql = format!(
            "
            SELECT n.path
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE n.exists_now = 1
              AND p.status = '{approved_status}'
              AND p.source NOT IN ({reviewed_sources})
              AND (n.kind = 'folder' OR (n.kind = 'file' AND {high_impact}))
            ORDER BY CASE WHEN n.kind = 'folder' THEN 0 ELSE 1 END, n.path
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            let finding = agent_review_required_finding(row?);
            if !emit_unresolved_finding(finding, resolved_ids, visitor)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Count approved navigation-critical purposes that still need agent review.
    fn count_agent_review_required_findings(
        &self,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
    ) -> DbResult<usize> {
        let (where_clause, values) = structural_finding_where_clause(
            CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED,
            path_prefix,
            resolved_ids,
            scope,
            1,
        );
        let source_relevant = source_relevant_node_expression("n");
        let reviewed_sources = sql_string_literals(AGENT_REVIEWED_SOURCE_VALUES);
        let review_candidate = purpose_review_candidate_expression("n", scope);
        let approved_status = PurposeStatus::Approved.as_str();
        let sql = format!(
            "
            WITH findings AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       '' AS related_path,
                       {source_relevant} AS source_relevant
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                WHERE n.exists_now = 1
                  AND p.status = '{approved_status}'
                  AND p.source NOT IN ({reviewed_sources})
                  AND {review_candidate}
            )
            SELECT COUNT(*)
            FROM findings
            {where_clause}
            "
        );
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("health_agent_review_required_count", count)
    }

    /// Load approved navigation-critical purposes that still need agent review.
    fn load_agent_review_required_findings_page(
        &self,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let (where_clause, mut values) = structural_finding_where_clause(
            CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED,
            path_prefix,
            resolved_ids,
            scope,
            1,
        );
        let source_relevant = source_relevant_node_expression("n");
        let reviewed_sources = sql_string_literals(AGENT_REVIEWED_SOURCE_VALUES);
        let review_candidate = purpose_review_candidate_expression("n", scope);
        let approved_status = PurposeStatus::Approved.as_str();
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let sql = format!(
            "
            WITH findings AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       '' AS related_path,
                       {source_relevant} AS source_relevant
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                WHERE n.exists_now = 1
                  AND p.status = '{approved_status}'
                  AND p.source NOT IN ({reviewed_sources})
                  AND {review_candidate}
            )
            SELECT path
            FROM findings
            {where_clause}
            ORDER BY CASE WHEN kind = 'folder' THEN 0 ELSE 1 END, path
            LIMIT ?{limit_placeholder} OFFSET ?{offset_placeholder}
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| row.get::<_, String>(0))?;
        let mut findings = Vec::new();
        for row in rows {
            findings.push(agent_review_required_finding(row?));
        }
        Ok(findings)
    }

    /// Count duplicate-purpose findings directly in `SQLite`.
    fn count_duplicate_purpose_findings(
        &self,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
    ) -> DbResult<usize> {
        let (where_clause, values) = structural_finding_where_clause(
            CATEGORY_DUPLICATE_PURPOSE,
            path_prefix,
            resolved_ids,
            scope,
            1,
        );
        let source_relevant = source_relevant_node_expression("n");
        let duplicate_scope =
            "CASE WHEN n.kind = 'folder' THEN COALESCE(n.parent_path, '') ELSE '' END";
        let approved_status = PurposeStatus::Approved.as_str();
        let sql = format!(
            "
            WITH duplicate_rows AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       p.purpose,
                       {source_relevant} AS source_relevant,
                       FIRST_VALUE(n.path) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS related_path,
                       ROW_NUMBER() OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS duplicate_rank,
                       COUNT(*) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                       ) AS duplicate_count
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                WHERE n.exists_now = 1
                  AND p.status = '{approved_status}'
                  AND p.purpose IS NOT NULL
            ),
            findings AS (
                SELECT path, kind, language, purpose, related_path, source_relevant
                FROM duplicate_rows
                WHERE duplicate_count > 1
                  AND duplicate_rank > 1
            )
            SELECT COUNT(*)
            FROM findings
            {where_clause}
            "
        );
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("health_duplicate_purpose_count", count)
    }

    /// Load a bounded duplicate-purpose findings page directly in `SQLite`.
    fn load_duplicate_purpose_findings_page(
        &self,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let (where_clause, mut values) = structural_finding_where_clause(
            CATEGORY_DUPLICATE_PURPOSE,
            path_prefix,
            resolved_ids,
            scope,
            1,
        );
        let source_relevant = source_relevant_node_expression("n");
        let duplicate_scope =
            "CASE WHEN n.kind = 'folder' THEN COALESCE(n.parent_path, '') ELSE '' END";
        let approved_status = PurposeStatus::Approved.as_str();
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let sql = format!(
            "
            WITH duplicate_rows AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       p.purpose,
                       {source_relevant} AS source_relevant,
                       FIRST_VALUE(n.path) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS related_path,
                       ROW_NUMBER() OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS duplicate_rank,
                       COUNT(*) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                       ) AS duplicate_count
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                WHERE n.exists_now = 1
                  AND p.status = '{approved_status}'
                  AND p.purpose IS NOT NULL
            ),
            findings AS (
                SELECT path, kind, language, purpose, related_path, source_relevant
                FROM duplicate_rows
                WHERE duplicate_count > 1
                  AND duplicate_rank > 1
            )
            SELECT path, kind, related_path
            FROM findings
            {where_clause}
            ORDER BY kind, lower(purpose), path
            LIMIT ?{limit_placeholder} OFFSET ?{offset_placeholder}
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut findings = Vec::new();
        for row in rows {
            let (path, kind_value, related_path) = row?;
            let kind = NodeKind::from_db(&kind_value).ok_or_else(|| DbError::InvalidEnum {
                field: "kind",
                value: kind_value,
            })?;
            findings.push(HealthFinding {
                id: finding_id(CATEGORY_DUPLICATE_PURPOSE, &path, Some(&related_path)),
                severity: Severity::Warning,
                category: CATEGORY_DUPLICATE_PURPOSE.to_string(),
                path,
                related_path: Some(related_path),
                message: format!("Multiple {kind} nodes share the same purpose."),
                recommendation: RECOMMENDATION_DUPLICATE_PURPOSE.to_string(),
            });
        }
        Ok(findings)
    }

    /// Count repeated temporary-folder findings directly in `SQLite`.
    fn count_repeated_temp_folder_findings(
        &self,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
    ) -> DbResult<usize> {
        let mut total = 0_usize;
        for bucket in TEMP_FOLDER_BUCKETS {
            total += self.count_repeated_temp_folder_bucket_findings(
                bucket,
                path_prefix,
                resolved_ids,
                scope,
            )?;
        }
        Ok(total)
    }

    /// Count one repeated temporary-folder bucket directly in `SQLite`.
    fn count_repeated_temp_folder_bucket_findings(
        &self,
        bucket: &str,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
    ) -> DbResult<usize> {
        let exact = bucket.to_string();
        let suffix = format!("%/{bucket}");
        let (where_clause, mut filter_values) = structural_finding_where_clause(
            CATEGORY_REPEATED_TEMPORARY_FOLDER,
            path_prefix,
            resolved_ids,
            scope,
            3,
        );
        let mut values = vec![Value::from(exact), Value::from(suffix)];
        values.append(&mut filter_values);
        let source_relevant = source_relevant_node_expression("n");
        let sql = format!(
            "
            WITH bucket_rows AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       {source_relevant} AS source_relevant,
                       FIRST_VALUE(n.path) OVER (ORDER BY n.path) AS related_path,
                       ROW_NUMBER() OVER (ORDER BY path) AS duplicate_rank,
                       COUNT(*) OVER () AS duplicate_count
                FROM nodes n
                WHERE n.exists_now = 1
                  AND n.kind = 'folder'
                  AND (lower(n.path) = ?1 OR lower(n.path) LIKE ?2)
            ),
            findings AS (
                SELECT path, kind, language, related_path, source_relevant
                FROM bucket_rows
                WHERE duplicate_count > 1
                  AND duplicate_rank > 1
            )
            SELECT COUNT(*)
            FROM findings
            {where_clause}
            "
        );
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("health_repeated_temp_count", count)
    }

    /// Load a bounded repeated temporary-folder findings page directly in `SQLite`.
    fn load_repeated_temp_folder_findings_page(
        &self,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut total = 0_usize;
        let mut findings = Vec::new();
        for bucket in TEMP_FOLDER_BUCKETS {
            let matching_count = self.count_repeated_temp_folder_bucket_findings(
                bucket,
                path_prefix,
                resolved_ids,
                scope,
            )?;
            if findings.len() < limit && total + matching_count > start_index {
                let local_start = start_index.saturating_sub(total);
                let local_limit = limit - findings.len();
                findings.extend(self.load_repeated_temp_folder_bucket_findings_page(
                    bucket,
                    path_prefix,
                    resolved_ids,
                    scope,
                    local_start,
                    local_limit,
                )?);
            }
            total += matching_count;
            if findings.len() >= limit {
                break;
            }
        }
        Ok(findings)
    }

    /// Load one repeated temporary-folder bucket directly in `SQLite`.
    fn load_repeated_temp_folder_bucket_findings_page(
        &self,
        bucket: &str,
        path_prefix: Option<&str>,
        resolved_ids: &[String],
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let exact = bucket.to_string();
        let suffix = format!("%/{bucket}");
        let (where_clause, mut filter_values) = structural_finding_where_clause(
            CATEGORY_REPEATED_TEMPORARY_FOLDER,
            path_prefix,
            resolved_ids,
            scope,
            3,
        );
        let mut values = vec![Value::from(exact), Value::from(suffix)];
        values.append(&mut filter_values);
        let source_relevant = source_relevant_node_expression("n");
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let sql = format!(
            "
            WITH bucket_rows AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       {source_relevant} AS source_relevant,
                       FIRST_VALUE(n.path) OVER (ORDER BY n.path) AS related_path,
                       ROW_NUMBER() OVER (ORDER BY n.path) AS duplicate_rank,
                       COUNT(*) OVER () AS duplicate_count
                FROM nodes n
                WHERE n.exists_now = 1
                  AND n.kind = 'folder'
                  AND (lower(n.path) = ?1 OR lower(n.path) LIKE ?2)
            ),
            findings AS (
                SELECT path, kind, language, related_path, source_relevant
                FROM bucket_rows
                WHERE duplicate_count > 1
                  AND duplicate_rank > 1
            )
            SELECT path, related_path
            FROM findings
            {where_clause}
            ORDER BY path
            LIMIT ?{limit_placeholder} OFFSET ?{offset_placeholder}
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut findings = Vec::new();
        for row in rows {
            let (path, related_path) = row?;
            findings.push(HealthFinding {
                id: finding_id(
                    CATEGORY_REPEATED_TEMPORARY_FOLDER,
                    &path,
                    Some(&related_path),
                ),
                severity: Severity::Warning,
                category: CATEGORY_REPEATED_TEMPORARY_FOLDER.to_string(),
                path,
                related_path: Some(related_path),
                message: format!("Repeated temporary/generated folder name `{bucket}` found."),
                recommendation: RECOMMENDATION_REPEATED_TEMPORARY_FOLDER.to_string(),
            });
        }
        Ok(findings)
    }

    /// Visit duplicate-purpose health findings through grouped SQL candidates.
    fn visit_duplicate_purpose_findings<F>(
        &self,
        resolved_ids: &HashSet<String>,
        visitor: &mut F,
    ) -> DbResult<bool>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        let duplicate_scope =
            "CASE WHEN n.kind = 'folder' THEN COALESCE(n.parent_path, '') ELSE '' END";
        let approved_status = PurposeStatus::Approved.as_str();
        let sql = format!(
            "
            WITH duplicate_rows AS (
                SELECT n.path,
                       n.kind,
                       p.purpose,
                       FIRST_VALUE(n.path) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS related_path,
                       ROW_NUMBER() OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS duplicate_rank,
                       COUNT(*) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                       ) AS duplicate_count
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                WHERE n.exists_now = 1
                  AND p.status = '{approved_status}'
                  AND p.purpose IS NOT NULL
            )
            SELECT path, kind, purpose, related_path
            FROM duplicate_rows
            WHERE duplicate_count > 1
              AND duplicate_rank > 1
            ORDER BY kind, lower(purpose), path
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (path, kind_value, _purpose, related_path) = row?;
            let kind = NodeKind::from_db(&kind_value).ok_or_else(|| DbError::InvalidEnum {
                field: "kind",
                value: kind_value.clone(),
            })?;
            let finding = HealthFinding {
                id: finding_id(CATEGORY_DUPLICATE_PURPOSE, &path, Some(&related_path)),
                severity: Severity::Warning,
                category: CATEGORY_DUPLICATE_PURPOSE.to_string(),
                path,
                related_path: Some(related_path),
                message: format!("Multiple {kind} nodes share the same purpose."),
                recommendation: RECOMMENDATION_DUPLICATE_PURPOSE.to_string(),
            };
            if !emit_unresolved_finding(finding, resolved_ids, visitor)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Visit repeated temporary/generated folder findings.
    fn visit_repeated_temp_folder_findings<F>(
        &self,
        resolved_ids: &HashSet<String>,
        visitor: &mut F,
    ) -> DbResult<bool>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        for bucket in TEMP_FOLDER_BUCKETS {
            let exact = bucket.to_string();
            let suffix = format!("%/{bucket}");
            let mut statement = self.connection.prepare(
                "
                SELECT path
                FROM nodes
                WHERE exists_now = 1
                  AND kind = 'folder'
                  AND (lower(path) = ?1 OR lower(path) LIKE ?2)
                ORDER BY path
                ",
            )?;
            let rows =
                statement.query_map(params![exact, suffix], |row| row.get::<_, String>(0))?;
            let mut first_path = None;
            for row in rows {
                let path = row?;
                let Some(first_path) = first_path.as_ref() else {
                    first_path = Some(path);
                    continue;
                };
                let finding = HealthFinding {
                    id: finding_id(
                        CATEGORY_REPEATED_TEMPORARY_FOLDER,
                        &path,
                        Some(first_path.as_str()),
                    ),
                    severity: Severity::Warning,
                    category: CATEGORY_REPEATED_TEMPORARY_FOLDER.to_string(),
                    path,
                    related_path: Some(first_path.clone()),
                    message: format!("Repeated temporary/generated folder name `{bucket}` found."),
                    recommendation: RECOMMENDATION_REPEATED_TEMPORARY_FOLDER.to_string(),
                };
                if !emit_unresolved_finding(finding, resolved_ids, visitor)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// Compute an overview from the current index.
    ///
    /// # Errors
    ///
    /// Returns an error if the aggregate query fails or a count is invalid.
    pub fn overview(&self) -> DbResult<Overview> {
        let missing_status = PurposeStatus::Missing.as_str();
        let stale_status = PurposeStatus::Stale.as_str();
        let approved_status = PurposeStatus::Approved.as_str();
        let suggested_status = PurposeStatus::Suggested.as_str();
        let sql = format!(
            "
            SELECT
                COALESCE(SUM(CASE WHEN n.kind = 'file' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN n.kind = 'folder' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN p.status = '{missing_status}' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN p.status = '{stale_status}' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN p.status = '{approved_status}' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN p.status = '{suggested_status}' THEN 1 ELSE 0 END), 0)
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE n.exists_now = 1
            "
        );
        let counts = self.connection.query_row(&sql, [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        Ok(Overview {
            files: count_to_usize("files", counts.0)?,
            folders: count_to_usize("folders", counts.1)?,
            missing_purposes: count_to_usize("missing_purposes", counts.2)?,
            stale_purposes: count_to_usize("stale_purposes", counts.3)?,
            approved_purposes: count_to_usize("approved_purposes", counts.4)?,
            suggested_purposes: count_to_usize("suggested_purposes", counts.5)?,
        })
    }

    /// Record a usage event.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn record_usage(&self, event: &UsageEvent) -> DbResult<()> {
        self.connection.execute(
            "
            INSERT INTO usage_events(
                session_id,
                command,
                path,
                query,
                estimated_tokens_without_projectatlas,
                estimated_tokens_with_projectatlas,
                estimated_tokens_saved,
                token_savings_bucket,
                provider,
                model,
                tokenizer_backend,
                accuracy,
                baseline_kind,
                confidence,
                calculation_trace,
                accounting_layer,
                estimate_method,
                denominator_kind,
                baseline_identity,
                baseline_fingerprint,
                dedupe_scope,
                created_at
            )
            VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, CURRENT_TIMESTAMP)
            ",
            params![
                event.session_id,
                event.command,
                event.path,
                event.query,
                event.estimated_tokens_without_projectatlas,
                event.estimated_tokens_with_projectatlas,
                event.estimated_tokens_saved,
                event.token_savings_bucket,
                event.provider,
                event.model,
                event.tokenizer_backend,
                event.accuracy,
                event.baseline_kind,
                event.confidence,
                event.calculation_trace,
                event.accounting_layer,
                event.estimate_method,
                event.denominator_kind,
                event.baseline_identity,
                event.baseline_fingerprint,
                event.dedupe_scope
            ],
        )?;
        Ok(())
    }

    /// Load usage events.
    ///
    /// # Errors
    ///
    /// Returns an error if loading fails.
    pub fn usage_events(&self, session_id: Option<&str>) -> DbResult<Vec<UsageEvent>> {
        let sql = if session_id.is_some() {
            "
            SELECT session_id, command, path, query, estimated_tokens_without_projectatlas,
                   estimated_tokens_with_projectatlas, estimated_tokens_saved,
                   token_savings_bucket, provider, model, tokenizer_backend,
                   accuracy, baseline_kind, confidence, calculation_trace,
                   accounting_layer, estimate_method, denominator_kind,
                   baseline_identity, baseline_fingerprint, dedupe_scope
            FROM usage_events
            WHERE session_id = ?1
            ORDER BY id
            "
        } else {
            "
            SELECT session_id, command, path, query, estimated_tokens_without_projectatlas,
                   estimated_tokens_with_projectatlas, estimated_tokens_saved,
                   token_savings_bucket, provider, model, tokenizer_backend,
                   accuracy, baseline_kind, confidence, calculation_trace,
                   accounting_layer, estimate_method, denominator_kind,
                   baseline_identity, baseline_fingerprint, dedupe_scope
            FROM usage_events
            ORDER BY id
            "
        };
        let mut statement = self.connection.prepare(sql)?;
        let mapper = |row: &rusqlite::Row<'_>| {
            Ok(UsageEvent {
                session_id: row.get(0)?,
                command: row.get(1)?,
                path: row.get(2)?,
                query: row.get(3)?,
                estimated_tokens_without_projectatlas: row.get(4)?,
                estimated_tokens_with_projectatlas: row.get(5)?,
                estimated_tokens_saved: row.get(6)?,
                token_savings_bucket: row.get(7)?,
                provider: row.get(8)?,
                model: row.get(9)?,
                tokenizer_backend: row.get(10)?,
                accuracy: row.get(11)?,
                baseline_kind: row.get(12)?,
                confidence: row.get(13)?,
                calculation_trace: row.get(14)?,
                accounting_layer: row.get(15)?,
                estimate_method: row.get(16)?,
                denominator_kind: row.get(17)?,
                baseline_identity: row.get(18)?,
                baseline_fingerprint: row.get(19)?,
                dedupe_scope: row.get(20)?,
            })
        };
        let rows = if let Some(session) = session_id {
            statement.query_map([session], mapper)?
        } else {
            statement.query_map([], mapper)?
        };
        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    /// Load the narrow raw-event fields required for token accounting dedupe.
    fn token_accounting_events(&self, session_id: Option<&str>) -> DbResult<Vec<UsageEvent>> {
        let sql = if session_id.is_some() {
            "
            SELECT session_id, command, path, query,
                   estimated_tokens_without_projectatlas,
                   estimated_tokens_with_projectatlas,
                   token_savings_bucket, baseline_kind, confidence,
                   accounting_layer, denominator_kind,
                   baseline_identity, baseline_fingerprint, dedupe_scope
            FROM usage_events
            WHERE session_id = ?1
              AND estimated_tokens_without_projectatlas IS NOT NULL
              AND estimated_tokens_with_projectatlas IS NOT NULL
            ORDER BY id
            "
        } else {
            "
            SELECT session_id, command, path, query,
                   estimated_tokens_without_projectatlas,
                   estimated_tokens_with_projectatlas,
                   token_savings_bucket, baseline_kind, confidence,
                   accounting_layer, denominator_kind,
                   baseline_identity, baseline_fingerprint, dedupe_scope
            FROM usage_events
            WHERE estimated_tokens_without_projectatlas IS NOT NULL
              AND estimated_tokens_with_projectatlas IS NOT NULL
            ORDER BY id
            "
        };
        let mut statement = self.connection.prepare(sql)?;
        let mapper = |row: &rusqlite::Row<'_>| {
            Ok(UsageEvent {
                session_id: row.get(0)?,
                command: row.get(1)?,
                path: row.get(2)?,
                query: row.get(3)?,
                estimated_tokens_without_projectatlas: row.get(4)?,
                estimated_tokens_with_projectatlas: row.get(5)?,
                estimated_tokens_saved: None,
                token_savings_bucket: row.get(6)?,
                provider: default_token_provider(),
                model: default_token_model(),
                tokenizer_backend: default_tokenizer_backend(),
                accuracy: default_token_accuracy(),
                baseline_kind: row.get(7)?,
                confidence: row.get(8)?,
                calculation_trace: default_token_trace(),
                accounting_layer: row.get(9)?,
                estimate_method: default_estimate_method(),
                denominator_kind: row.get(10)?,
                baseline_identity: row.get(11)?,
                baseline_fingerprint: row.get(12)?,
                dedupe_scope: row.get(13)?,
            })
        };
        let rows = if let Some(session) = session_id {
            statement.query_map([session], mapper)?
        } else {
            statement.query_map([], mapper)?
        };
        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    /// Build a token overview.
    ///
    /// # Errors
    ///
    /// Returns an error if loading events fails.
    pub fn token_overview(&self, session_id: Option<&str>) -> DbResult<TokenOverview> {
        let sql = if session_id.is_some() {
            "
            SELECT
                token_savings_bucket,
                provider,
                model,
                tokenizer_backend,
                accuracy,
                baseline_kind,
                confidence,
                accounting_layer,
                estimate_method,
                denominator_kind,
                dedupe_scope,
                COUNT(*),
                TOTAL(estimated_tokens_without_projectatlas),
                TOTAL(estimated_tokens_with_projectatlas)
            FROM usage_events
            WHERE session_id = ?1
              AND estimated_tokens_without_projectatlas IS NOT NULL
              AND estimated_tokens_with_projectatlas IS NOT NULL
            GROUP BY token_savings_bucket, provider, model, tokenizer_backend,
                     accuracy, baseline_kind, confidence, accounting_layer,
                     estimate_method, denominator_kind, dedupe_scope
            ORDER BY token_savings_bucket, accuracy, baseline_kind, confidence,
                     accounting_layer, estimate_method, denominator_kind, dedupe_scope
            "
        } else {
            "
            SELECT
                token_savings_bucket,
                provider,
                model,
                tokenizer_backend,
                accuracy,
                baseline_kind,
                confidence,
                accounting_layer,
                estimate_method,
                denominator_kind,
                dedupe_scope,
                COUNT(*),
                TOTAL(estimated_tokens_without_projectatlas),
                TOTAL(estimated_tokens_with_projectatlas)
            FROM usage_events
            WHERE estimated_tokens_without_projectatlas IS NOT NULL
              AND estimated_tokens_with_projectatlas IS NOT NULL
            GROUP BY token_savings_bucket, provider, model, tokenizer_backend,
                     accuracy, baseline_kind, confidence, accounting_layer,
                     estimate_method, denominator_kind, dedupe_scope
            ORDER BY token_savings_bucket, accuracy, baseline_kind, confidence,
                     accounting_layer, estimate_method, denominator_kind, dedupe_scope
            "
        };
        let mapper = |row: &rusqlite::Row<'_>| {
            let calls = row.get::<_, i64>(11)?.max(0) as u128;
            Ok(TokenBucketOverview::from_totals(
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                calls,
                token_total_from_sql("estimated_tokens_without_projectatlas", row.get(12)?),
                token_total_from_sql("estimated_tokens_with_projectatlas", row.get(13)?),
            ))
        };
        let mut statement = self.connection.prepare(sql)?;
        let rows = if let Some(session) = session_id {
            statement.query_map([session], mapper)?
        } else {
            statement.query_map([], mapper)?
        };
        let mut buckets = Vec::new();
        for row in rows {
            buckets.push(row?);
        }
        let mut overview = TokenOverview::from_buckets(buckets);
        overview.apply_accounting_from_events(&self.token_accounting_events(session_id)?);
        Ok(overview)
    }

    /// Build token trend aggregates grouped by day, week, month, or year.
    ///
    /// # Errors
    ///
    /// Returns an error if the window is unsupported or loading events fails.
    pub fn token_trends(
        &self,
        session_id: Option<&str>,
        window: TokenTrendWindow,
    ) -> DbResult<TokenTrendReport> {
        let period_expr = token_trend_period_expression(window);
        let sql = if session_id.is_some() {
            format!(
                "
            SELECT
                {period_expr} AS period,
                token_savings_bucket,
                provider,
                model,
                tokenizer_backend,
                accuracy,
                baseline_kind,
                confidence,
                accounting_layer,
                estimate_method,
                denominator_kind,
                dedupe_scope,
                COUNT(*),
                TOTAL(estimated_tokens_without_projectatlas),
                TOTAL(estimated_tokens_with_projectatlas)
            FROM usage_events
            WHERE session_id = ?1
              AND estimated_tokens_without_projectatlas IS NOT NULL
              AND estimated_tokens_with_projectatlas IS NOT NULL
            GROUP BY period, token_savings_bucket, provider, model, tokenizer_backend,
                     accuracy, baseline_kind, confidence, accounting_layer, estimate_method,
                     denominator_kind, dedupe_scope
            ORDER BY period, token_savings_bucket, accuracy, baseline_kind, confidence,
                     accounting_layer, estimate_method, denominator_kind, dedupe_scope
            "
            )
        } else {
            format!(
                "
            SELECT
                {period_expr} AS period,
                token_savings_bucket,
                provider,
                model,
                tokenizer_backend,
                accuracy,
                baseline_kind,
                confidence,
                accounting_layer,
                estimate_method,
                denominator_kind,
                dedupe_scope,
                COUNT(*),
                TOTAL(estimated_tokens_without_projectatlas),
                TOTAL(estimated_tokens_with_projectatlas)
            FROM usage_events
            WHERE estimated_tokens_without_projectatlas IS NOT NULL
              AND estimated_tokens_with_projectatlas IS NOT NULL
            GROUP BY period, token_savings_bucket, provider, model, tokenizer_backend,
                     accuracy, baseline_kind, confidence, accounting_layer, estimate_method,
                     denominator_kind, dedupe_scope
            ORDER BY period, token_savings_bucket, accuracy, baseline_kind, confidence,
                     accounting_layer, estimate_method, denominator_kind, dedupe_scope
            "
            )
        };
        let mapper = |row: &rusqlite::Row<'_>| {
            let period = row.get::<_, String>(0)?;
            let calls = row.get::<_, i64>(12)?.max(0) as u128;
            let bucket = TokenBucketOverview::from_totals(
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
                calls,
                token_total_from_sql("estimated_tokens_without_projectatlas", row.get(13)?),
                token_total_from_sql("estimated_tokens_with_projectatlas", row.get(14)?),
            );
            Ok((period, bucket))
        };
        let mut statement = self.connection.prepare(&sql)?;
        let rows = if let Some(session) = session_id {
            statement.query_map([session], mapper)?
        } else {
            statement.query_map([], mapper)?
        };
        let mut buckets_by_period = BTreeMap::<String, Vec<TokenBucketOverview>>::new();
        for row in rows {
            let (period, bucket) = row?;
            buckets_by_period.entry(period).or_default().push(bucket);
        }
        let periods = buckets_by_period
            .into_iter()
            .map(|(period, buckets)| TokenTrendPeriod::from_buckets(period, buckets))
            .collect();
        Ok(TokenTrendReport::new(
            session_id.map(ToString::to_string),
            window,
            periods,
        ))
    }

    /// Mark a deterministic health finding as agent-resolved.
    ///
    /// # Errors
    ///
    /// Returns an error if the finding is not active or persistence fails.
    pub fn resolve_health_finding(&self, resolution: &HealthResolution) -> DbResult<()> {
        let resolved_ids = self.resolved_health_ids()?;
        if !self.active_health_finding_matches(&resolved_ids, resolution)? {
            return Err(DbError::HealthFindingNotActive {
                finding_id: resolution.finding_id.clone(),
                category: resolution.category.clone(),
                path: resolution.path.clone(),
            });
        }
        self.connection.execute(
            "
            INSERT INTO health_resolutions(
                finding_id,
                category,
                path,
                related_path,
                rationale,
                resolved_by,
                resolved_at
            )
            VALUES(?1, ?2, ?3, ?4, ?5, 'agent', CURRENT_TIMESTAMP)
            ON CONFLICT(finding_id) DO UPDATE SET
                category = excluded.category,
                path = excluded.path,
                related_path = excluded.related_path,
                rationale = excluded.rationale,
                resolved_by = 'agent',
                resolved_at = CURRENT_TIMESTAMP
            ",
            params![
                resolution.finding_id,
                resolution.category,
                resolution.path,
                resolution.related_path,
                resolution.rationale,
            ],
        )?;
        Ok(())
    }

    /// Return whether the visible SQL health surface contains the exact finding.
    fn active_health_finding_matches(
        &self,
        resolved_ids: &[String],
        resolution: &HealthResolution,
    ) -> DbResult<bool> {
        const PAGE_SIZE: usize = 256;
        let mut start_index = 0_usize;
        loop {
            let page = self.unresolved_health_findings_page(
                resolved_ids,
                &HealthQuery {
                    start_index,
                    limit: PAGE_SIZE,
                    category: Some(resolution.category.clone()),
                    severity: Some(Severity::Warning),
                    path_prefix: Some(resolution.path.clone()),
                    summary_only: false,
                    scope: HealthScope::all(),
                },
            )?;
            if page.findings.iter().any(|finding| {
                finding.id == resolution.finding_id
                    && finding.category == resolution.category
                    && finding.path == resolution.path
                    && finding.related_path == resolution.related_path
            }) {
                return Ok(true);
            }
            if page.returned == 0 || start_index + page.returned >= page.total {
                return Ok(false);
            }
            start_index += page.returned;
        }
    }

    /// Load resolved health finding ids.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn resolved_health_ids(&self) -> DbResult<Vec<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT finding_id FROM health_resolutions ORDER BY finding_id")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row?);
        }
        Ok(ids)
    }
}

/// Read the recorded project root without creating or migrating a database.
///
/// # Errors
///
/// Returns an error if `SQLite` cannot open or query the database read-only.
pub fn read_project_root_read_only(path: &Path) -> DbResult<Option<String>> {
    let uri = sqlite_read_uri(path, true);
    let connection = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;
    let root = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = 'project_root'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    root.map(|value| normalize_metadata_path(Path::new(&value)))
        .transpose()
}

/// Build a read-only `SQLite` URI, optionally treating the database as immutable.
pub(crate) fn sqlite_read_uri(path: &Path, immutable: bool) -> String {
    let normalized = normalize_native_path_display(path);
    let uri_path = if normalized.as_bytes().get(1) == Some(&b':') {
        format!("/{normalized}")
    } else {
        normalized
    };
    let immutable = if immutable { "&immutable=1" } else { "" };
    format!(
        "file:{}?mode=ro{immutable}",
        sqlite_uri_escape_path(&uri_path)
    )
}

/// Percent-escape a path component while preserving path separators and drive colons.
fn sqlite_uri_escape_path(path: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut escaped = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'.' | b'-' | b'_' | b'~' => {
                escaped.push(char::from(byte));
            }
            other => {
                escaped.push('%');
                escaped.push(char::from(HEX[usize::from(other >> 4)]));
                escaped.push(char::from(HEX[usize::from(other & 0x0F)]));
            }
        }
    }
    escaped
}

/// Resolve an existing filesystem path for metadata identity, retaining lexical
/// behavior only for deliberately nonexistent roots such as in-memory fixtures.
fn normalize_metadata_path(path: &Path) -> DbResult<String> {
    match std::fs::canonicalize(path) {
        Ok(canonical) => Ok(normalize_native_path_display(canonical)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(normalize_native_path_display(path))
        }
        Err(source) => Err(DbError::ProjectRootResolution {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Mark selected paths and descendants absent within an existing write boundary.
fn mark_paths_absent_in_connection(connection: &Connection, paths: &[String]) -> DbResult<()> {
    let mut update_nodes = connection.prepare_cached(
        "UPDATE nodes SET exists_now = 0 WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
    )?;
    let mut delete_relations = connection.prepare_cached(
        "DELETE FROM symbol_relations WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
    )?;
    let mut delete_symbols = connection
        .prepare_cached("DELETE FROM symbols WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'")?;
    let mut delete_parse_metadata = connection.prepare_cached(
        "DELETE FROM source_parse_metadata WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
    )?;
    let mut delete_texts = connection.prepare_cached(
        "DELETE FROM file_texts
         WHERE structural_slot = (
             SELECT active_slot FROM graph_publication_state WHERE singleton = 1
         )
         AND (path = ?1 OR path LIKE ?2 ESCAPE '\\')",
    )?;
    for path in paths {
        if path == "." || path.is_empty() {
            continue;
        }
        let descendant_pattern = sqlite_descendant_pattern(path);
        update_nodes.execute(params![path, descendant_pattern])?;
        delete_relations.execute(params![path, descendant_pattern])?;
        delete_symbols.execute(params![path, descendant_pattern])?;
        delete_parse_metadata.execute(params![path, descendant_pattern])?;
        delete_texts.execute(params![path, descendant_pattern])?;
    }
    Ok(())
}

/// Clear active-slot lexical rows for exact affected paths.
fn delete_file_text_paths_from_active_slot(
    connection: &Connection,
    paths: &[String],
) -> DbResult<()> {
    let mut delete = connection.prepare_cached(
        "DELETE FROM file_texts
         WHERE structural_slot = (
             SELECT active_slot FROM graph_publication_state WHERE singleton = 1
         )
         AND path = ?1",
    )?;
    for path in paths {
        delete.execute([path])?;
    }
    Ok(())
}

/// Load one live node id within a caller-owned connection or transaction.
fn node_id_for_path_in_connection(connection: &Connection, path: &str) -> DbResult<i64> {
    connection
        .query_row(
            "SELECT id FROM nodes WHERE path = ?1 AND exists_now = 1",
            [path],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .ok_or_else(|| DbError::PathNotIndexed {
            path: path.to_string(),
        })
}

/// Persist one observed node summary within an existing write boundary.
fn set_node_summary_in_connection(
    connection: &Connection,
    path: &str,
    summary: &str,
) -> DbResult<()> {
    let node_id = node_id_for_path_in_connection(connection, path)?;
    connection
        .prepare_cached(
            "INSERT INTO summaries(node_id, summary_level, subject, summary, updated_at)
         VALUES(?1, 'node', '', ?2, CURRENT_TIMESTAMP)
         ON CONFLICT(node_id, summary_level, subject) DO UPDATE SET
             summary_level = 'node',
             subject = '',
             summary = excluded.summary,
             updated_at = CURRENT_TIMESTAMP",
        )?
        .execute(params![node_id, summary])?;
    Ok(())
}

/// Remove one observed node summary within an existing write boundary.
fn clear_node_summary_in_connection(connection: &Connection, path: &str) -> DbResult<()> {
    let node_id = node_id_for_path_in_connection(connection, path)?;
    connection
        .prepare_cached(
            "DELETE FROM summaries
         WHERE node_id = ?1 AND summary_level = 'node' AND subject = ''",
        )?
        .execute([node_id])?;
    Ok(())
}

/// Persist one approved purpose within an existing write boundary.
fn set_purpose_in_connection(
    connection: &Connection,
    path: &str,
    purpose: &str,
    source: PurposeSource,
) -> DbResult<()> {
    let node_id = node_id_for_path_in_connection(connection, path)?;
    connection
        .prepare_cached(
            "INSERT INTO purposes(node_id, purpose, source, status, updated_at)
         VALUES(?1, ?2, ?3, 'approved', CURRENT_TIMESTAMP)
         ON CONFLICT(node_id) DO UPDATE SET
             purpose = excluded.purpose,
             source = excluded.source,
             status = 'approved',
             updated_at = CURRENT_TIMESTAMP",
        )?
        .execute(params![node_id, purpose, source.to_string()])?;
    Ok(())
}

/// Persist a generated purpose only while no approved purpose owns the path.
fn set_suggested_purpose_in_connection(
    connection: &Connection,
    path: &str,
    purpose: &str,
) -> DbResult<()> {
    let node_id = node_id_for_path_in_connection(connection, path)?;
    connection
        .prepare_cached(
            "INSERT INTO purposes(node_id, purpose, source, status, updated_at)
         VALUES(?1, ?2, 'generated', 'suggested', CURRENT_TIMESTAMP)
         ON CONFLICT(node_id) DO UPDATE SET
             purpose = excluded.purpose,
             source = 'generated',
             status = 'suggested',
             updated_at = CURRENT_TIMESTAMP
         WHERE purposes.status IN ('missing', 'suggested')",
        )?
        .execute(params![node_id, purpose])?;
    Ok(())
}

/// Clear parser-owned compatibility rows within an existing write boundary.
fn clear_source_index_in_connection(
    connection: &Connection,
    path: &str,
    preserve_node_summary: bool,
) -> DbResult<()> {
    let node_id = node_id_for_path_in_connection(connection, path)?;
    clear_typed_graph_for_path(connection, path)?;
    connection
        .prepare_cached("DELETE FROM symbols WHERE path = ?1")?
        .execute([path])?;
    connection
        .prepare_cached("DELETE FROM symbol_relations WHERE path = ?1")?
        .execute([path])?;
    connection
        .prepare_cached("DELETE FROM source_parse_metadata WHERE path = ?1")?
        .execute([path])?;
    if preserve_node_summary {
        connection
            .prepare_cached(
                "DELETE FROM summaries
             WHERE node_id = ?1 AND summary_level = 'search' AND subject = 'symbols'",
            )?
            .execute([node_id])?;
    } else {
        connection
            .prepare_cached(
                "DELETE FROM summaries
             WHERE node_id = ?1
               AND ((summary_level = 'node' AND subject = '')
                    OR (summary_level = 'search' AND subject = 'symbols'))",
            )?
            .execute([node_id])?;
    }
    Ok(())
}

/// Borrowed or shared-arena text retained without one allocation per entity.
#[derive(Clone, Copy, Debug)]
enum CompactTypedGraphText<'a> {
    /// Text already interned by the compact parser graph.
    Borrowed(&'a str),
    /// One contiguous range in the batch-owned qualified-name arena.
    Shared {
        /// Inclusive byte offset in the shared qualified-name text.
        start: usize,
        /// Byte length of this UTF-8 name in the shared text.
        length: usize,
    },
}

impl CompactTypedGraphText<'_> {
    /// Borrow this text from its original graph or the shared batch arena.
    fn as_str<'a>(&'a self, shared: &'a str) -> &'a str {
        match self {
            Self::Borrowed(value) => value,
            Self::Shared { start, length } => &shared[*start..*start + *length],
        }
    }
}

/// Parser-produced entity retained long enough to persist relations without expanding the graph.
struct CompactTypedGraphEntity<'a> {
    /// Compatibility name used to resolve same-file source and target references.
    lookup_name: &'a str,
    /// Stable project-scoped entity identity.
    key: GraphEntityKeyHandle,
    /// Typed entity category.
    kind: GraphEntityKind,
    /// Repository path that owns invalidation for this fact.
    repository_path: &'a str,
    /// Qualified package, module, or declaration identity.
    qualified_name: CompactTypedGraphText<'a>,
    /// Optional overload/signature discriminator retained for inspection.
    signature: Option<&'a str>,
    /// External namespace for package-shaped selectors.
    external_namespace: Option<&'static str>,
    /// External value for package-shaped selectors.
    external_value: Option<&'a str>,
    /// Detected language or file family.
    language: Option<&'a str>,
    /// Parser family that produced the fact.
    parser: ParserKind,
}

/// Borrowed internal or external target retained by one compact relation row.
#[derive(Clone, Copy, Debug)]
enum CompactTypedGraphTarget<'a> {
    /// Exact same-project entity key stored in the shared key arena.
    Internal(GraphEntityKeyHandle),
    /// Exact namespaced identity borrowed from the compact parser graph.
    External {
        /// Ecosystem namespace for the external target.
        namespace: &'static str,
        /// Canonical target value inside the namespace.
        value: &'a str,
    },
}

impl<'a> CompactTypedGraphTarget<'a> {
    /// Return the borrowed selector accepted by the core key arena.
    const fn key_input(self) -> GraphRelationTargetInput<'a> {
        match self {
            Self::Internal(target) => GraphRelationTargetInput::Internal(target),
            Self::External { namespace, value } => {
                GraphRelationTargetInput::External { namespace, value }
            }
        }
    }
}

/// One resolved parser relation and its exact source occurrence ready for typed persistence.
struct CompactTypedGraphRelation<'a> {
    /// Stable logical-edge identity.
    key: GraphLogicalEdgeKeyHandle,
    /// Index of the source in the owning compact entity batch.
    source_index: usize,
    /// Accepted typed relation family.
    kind: GraphRelationKind,
    /// Exact internal or external target.
    target: CompactTypedGraphTarget<'a>,
    /// Parser family that produced the relation.
    parser: ParserKind,
    /// Truthful confidence for the bounded resolver rule that established the target.
    confidence: ConfidenceClass,
    /// Stable evidence-occurrence identity stored in the shared key arena.
    evidence_key: GraphEvidenceKeyHandle,
    /// Content-anchored source occurrence fingerprint.
    span_fingerprint: ContentSpanFingerprint,
    /// Stable discriminator for repeated occurrences at the same content span.
    occurrence_discriminator: u32,
    /// Optional validated source context borrowed from the compact graph.
    explanation: Option<&'a str>,
}

/// Non-traversable resolution state retained without fabricating a logical edge.
#[derive(Clone, Copy, Debug)]
enum CompactTypedGraphResolutionState {
    /// More than one same-file target has the exact compatibility name.
    Ambiguous {
        /// First retained candidate in the sorted entity-name index.
        candidate_start: usize,
        /// Exclusive end of the retained candidate page.
        candidate_end: usize,
        /// Complete viable-candidate count before the hard retention bound.
        candidate_total: usize,
        /// Whether the retained candidate page is complete or truncated.
        candidate_completeness: Completeness,
    },
    /// No traversable target was established under the current bounded resolver.
    Unresolved {
        /// Bounded diagnostic retained with the source occurrence.
        reason: &'static str,
    },
}

/// Shared source and provenance inputs for one parser relation observation.
#[derive(Clone, Copy, Debug)]
struct CompactTypedGraphOccurrence<'a> {
    /// Index of the resolved source or containing file in the entity batch.
    source_index: usize,
    /// Stable source identity stored in the shared key arena.
    source_key: GraphEntityKeyHandle,
    /// Accepted typed relation family.
    kind: GraphRelationKind,
    /// Parser family that observed the relation.
    parser: ParserKind,
    /// Project-owned repository origin for the parser observation.
    origin: GraphEvidenceOriginInput<'a>,
    /// Versioned resolver identity for stable occurrence keys.
    resolver: GraphResolverInput<'static>,
    /// Content-anchored source occurrence fingerprint.
    span_fingerprint: ContentSpanFingerprint,
}

impl CompactTypedGraphOccurrence<'_> {
    /// Materialize one non-traversable occurrence in the shared key arena.
    fn resolution(
        self,
        key_arena: &mut GraphKeyArena,
        occurrence_discriminator: u32,
        state: CompactTypedGraphResolutionState,
        confidence: ConfidenceClass,
    ) -> DbResult<CompactTypedGraphResolution> {
        Ok(CompactTypedGraphResolution {
            key: key_arena.resolution_key(
                self.source_key,
                self.kind,
                self.origin,
                self.resolver,
                self.span_fingerprint,
                occurrence_discriminator,
            )?,
            source_index: self.source_index,
            kind: self.kind,
            parser: self.parser,
            span_fingerprint: self.span_fingerprint,
            occurrence_discriminator,
            state,
            confidence,
        })
    }
}

/// One ambiguous or unresolved parser observation ready for typed persistence.
struct CompactTypedGraphResolution {
    /// Stable target-free occurrence identity stored in the shared key arena.
    key: GraphResolutionKeyHandle,
    /// Resolved containing source, or the owning file for an unresolved source name.
    source_index: usize,
    /// Accepted typed relation family.
    kind: GraphRelationKind,
    /// Parser family that observed the relation.
    parser: ParserKind,
    /// Content-anchored source occurrence fingerprint.
    span_fingerprint: ContentSpanFingerprint,
    /// Stable discriminator for equal unresolved observations.
    occurrence_discriminator: u32,
    /// Ambiguous candidate page or unresolved reason.
    state: CompactTypedGraphResolutionState,
    /// Finite confidence in the non-traversable observation.
    confidence: ConfidenceClass,
}

/// Resolved edges, non-traversable observations, and their allocation-bounded name index.
struct CompactTypedGraphRelationPlan<'a> {
    /// Traversable relations and direct evidence.
    relations: Vec<CompactTypedGraphRelation<'a>>,
    /// Ambiguous and unresolved observations excluded from adjacency.
    resolutions: Vec<CompactTypedGraphResolution>,
    /// One contiguous deterministic index over compatibility names.
    entity_indices: Vec<(&'a str, usize)>,
}

/// Delete typed facts owned by one active-slot source path.
fn clear_typed_graph_for_path(connection: &Connection, path: &str) -> DbResult<()> {
    let active_slot = connection.query_row(
        "SELECT active_slot FROM graph_publication_state WHERE singleton = 1",
        [],
        |row| row.get::<_, String>(0),
    )?;
    clear_typed_graph_for_path_in_slot(connection, path, &active_slot)
}

/// Delete typed facts owned by one source path in the selected structural slot.
fn clear_typed_graph_for_path_in_slot(
    connection: &Connection,
    path: &str,
    structural_slot: &str,
) -> DbResult<()> {
    connection
        .prepare_cached(
            "DELETE FROM graph_entities
             WHERE structural_slot = ?1 AND repository_path = ?2",
        )?
        .execute(params![structural_slot, path])?;
    Ok(())
}

/// Require one guarded stable-key insert or update to preserve canonical identity.
fn require_stable_graph_identity_upsert(
    affected_rows: usize,
    table: &'static str,
    structural_slot: &str,
) -> DbResult<()> {
    if affected_rows == 1 {
        Ok(())
    } else {
        Err(DbError::StableGraphIdentityCollision {
            table,
            structural_slot: structural_slot.to_owned(),
        })
    }
}

/// Reject an existing digest whose version or canonical identity does not match.
fn require_stable_graph_identity_available(
    connection: &Connection,
    sql: &str,
    table: &'static str,
    structural_slot: &str,
    digest: &[u8; 32],
    encoding_version: u16,
    canonical_identity: &[u8],
) -> DbResult<()> {
    let conflicts = connection.query_row(
        sql,
        params![
            structural_slot,
            &digest[..],
            i64::from(encoding_version),
            canonical_identity
        ],
        |row| row.get::<_, i64>(0),
    )?;
    if conflicts == 0 {
        Ok(())
    } else {
        Err(DbError::StableGraphIdentityCollision {
            table,
            structural_slot: structural_slot.to_owned(),
        })
    }
}

/// Replace one compact parser graph in normalized active-slot typed storage.
fn replace_typed_graph_for_compact_symbol_graph(
    connection: &Connection,
    graph: &CompactSymbolGraph,
    structural_slot: &str,
    last_changed_epoch: i64,
) -> DbResult<()> {
    let project = schema::project_instance_id(connection)?;
    let project_bytes = project.as_bytes();
    let graph_path = RepositoryFilePath::try_from(graph.path())?;
    let evidence_origin_path = RepositoryPath::try_from(graph.path())?;
    let mut key_arena = GraphKeyArena::default();
    let mut qualified_name_arena = String::new();
    let entities = compact_typed_graph_entities(
        project,
        graph,
        &graph_path,
        &mut key_arena,
        &mut qualified_name_arena,
    )?;
    let CompactTypedGraphRelationPlan {
        relations,
        resolutions,
        entity_indices,
    } = compact_typed_graph_relations(
        project,
        graph,
        &entities,
        &evidence_origin_path,
        &mut key_arena,
    )?;
    let mut checked_entities = HashSet::with_capacity(entities.len());
    for entity in &entities {
        if checked_entities.insert(entity.key.digest()) {
            let digest = entity.key.digest();
            require_stable_graph_identity_available(
                connection,
                GRAPH_ENTITY_IDENTITY_CONFLICT_SQL,
                GRAPH_ENTITIES_TABLE,
                structural_slot,
                &digest,
                entity.key.encoding_version(),
                key_arena.entity_canonical_identity(entity.key),
            )?;
        }
    }
    let mut checked_relations = HashSet::with_capacity(relations.len());
    let mut checked_evidence = HashSet::with_capacity(relations.len());
    for relation in &relations {
        if checked_relations.insert(relation.key.digest()) {
            let digest = relation.key.digest();
            require_stable_graph_identity_available(
                connection,
                GRAPH_RELATION_IDENTITY_CONFLICT_SQL,
                GRAPH_RELATIONS_TABLE,
                structural_slot,
                &digest,
                relation.key.encoding_version(),
                key_arena.logical_edge_canonical_identity(relation.key),
            )?;
        }
        if checked_evidence.insert(relation.evidence_key.digest()) {
            let digest = relation.evidence_key.digest();
            require_stable_graph_identity_available(
                connection,
                GRAPH_EVIDENCE_IDENTITY_CONFLICT_SQL,
                GRAPH_EVIDENCE_OCCURRENCES_TABLE,
                structural_slot,
                &digest,
                relation.evidence_key.encoding_version(),
                key_arena.evidence_canonical_identity(relation.evidence_key),
            )?;
        }
    }
    let mut checked_resolutions = HashSet::with_capacity(resolutions.len());
    for resolution in &resolutions {
        if checked_resolutions.insert(resolution.key.digest()) {
            let digest = resolution.key.digest();
            require_stable_graph_identity_available(
                connection,
                GRAPH_RESOLUTION_IDENTITY_CONFLICT_SQL,
                GRAPH_RESOLUTION_OCCURRENCES_TABLE,
                structural_slot,
                &digest,
                resolution.key.encoding_version(),
                key_arena.resolution_canonical_identity(resolution.key),
            )?;
        }
    }
    clear_typed_graph_for_path_in_slot(connection, graph.path(), structural_slot)?;
    let mut insert_entity = connection.prepare_cached(
        "INSERT INTO graph_entities(
             stable_key_digest, stable_key_version, stable_key_canonical,
             project_instance_id, entity_kind, repository_path, qualified_name,
             signature, discriminator, external_namespace, external_value, language,
             source_start_byte, source_end_byte, source_start_line, source_end_line,
             parser_kind, parser_identity, parser_version,
             structural_slot, last_changed_epoch
         ) VALUES(
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, ?9, ?10, ?11,
             NULL, NULL, NULL, NULL, ?12, ?13, ?14, ?15, ?16
         )
         ON CONFLICT(structural_slot, stable_key_digest) DO UPDATE SET
             stable_key_version = excluded.stable_key_version,
             stable_key_canonical = excluded.stable_key_canonical,
             project_instance_id = excluded.project_instance_id,
             entity_kind = excluded.entity_kind,
             repository_path = excluded.repository_path,
             qualified_name = excluded.qualified_name,
             signature = excluded.signature,
             discriminator = excluded.discriminator,
             external_namespace = excluded.external_namespace,
             external_value = excluded.external_value,
             language = excluded.language,
             source_start_byte = excluded.source_start_byte,
             source_end_byte = excluded.source_end_byte,
             source_start_line = excluded.source_start_line,
             source_end_line = excluded.source_end_line,
             parser_kind = excluded.parser_kind,
             parser_identity = excluded.parser_identity,
             parser_version = excluded.parser_version,
             last_changed_epoch = excluded.last_changed_epoch
         WHERE graph_entities.stable_key_version = excluded.stable_key_version
           AND graph_entities.stable_key_canonical = excluded.stable_key_canonical",
    )?;
    for entity in &entities {
        let digest = entity.key.digest();
        let affected_rows = insert_entity.execute(params![
            &digest[..],
            i64::from(entity.key.encoding_version()),
            key_arena.entity_canonical_identity(entity.key),
            &project_bytes[..],
            entity.kind.as_str(),
            entity.repository_path,
            entity.qualified_name.as_str(&qualified_name_arena),
            entity.signature,
            entity.external_namespace,
            entity.external_value,
            entity.language,
            entity.parser.as_str(),
            GRAPH_PARSER_IDENTITY,
            GRAPH_PARSER_VERSION,
            structural_slot,
            last_changed_epoch,
        ])?;
        require_stable_graph_identity_upsert(affected_rows, GRAPH_ENTITIES_TABLE, structural_slot)?;
    }

    let mut insert_relation = connection.prepare_cached(
        "INSERT INTO graph_relations(
             stable_key_digest, stable_key_version, stable_key_canonical,
             source_entity_digest, relation_kind, resolution_status, target_scope,
             target_entity_digest, external_target_namespace, external_target_value,
             confidence, parser_kind, parser_identity, parser_version,
             structural_slot, last_changed_epoch
         ) VALUES(
             ?1, ?2, ?3, ?4, ?5, 'resolved', ?6, ?7, ?8, ?9,
             ?10, ?11, ?12, ?13, ?14, ?15
         )
         ON CONFLICT(structural_slot, stable_key_digest) DO UPDATE SET
             stable_key_version = excluded.stable_key_version,
             stable_key_canonical = excluded.stable_key_canonical,
             source_entity_digest = excluded.source_entity_digest,
             relation_kind = excluded.relation_kind,
             resolution_status = excluded.resolution_status,
             target_scope = excluded.target_scope,
             target_entity_digest = excluded.target_entity_digest,
             external_target_namespace = excluded.external_target_namespace,
             external_target_value = excluded.external_target_value,
             confidence = excluded.confidence,
             parser_kind = excluded.parser_kind,
             parser_identity = excluded.parser_identity,
             parser_version = excluded.parser_version,
             last_changed_epoch = excluded.last_changed_epoch
         WHERE graph_relations.stable_key_version = excluded.stable_key_version
           AND graph_relations.stable_key_canonical = excluded.stable_key_canonical",
    )?;
    let mut insert_evidence = connection.prepare_cached(
        "INSERT INTO graph_evidence_occurrences(
             stable_key_digest, stable_key_version, stable_key_canonical,
             relation_digest, origin_kind, origin_entity_digest,
             origin_project_instance_id, origin_repository_path,
             origin_external_namespace, origin_external_value,
             source_start_byte, source_end_byte, source_start_line, source_end_line,
             resolver_name, resolver_version, content_span_fingerprint,
             occurrence_discriminator, evidence_class, confidence, completeness,
             explanation, structural_slot, last_changed_epoch
         ) VALUES(
             ?1, ?2, ?3, ?4, 'repository-path', NULL, ?5, ?6, NULL, NULL,
             NULL, NULL, NULL, NULL, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16
         )
         ON CONFLICT(structural_slot, stable_key_digest) DO UPDATE SET
             stable_key_version = excluded.stable_key_version,
             stable_key_canonical = excluded.stable_key_canonical,
             relation_digest = excluded.relation_digest,
             origin_kind = excluded.origin_kind,
             origin_entity_digest = excluded.origin_entity_digest,
             origin_project_instance_id = excluded.origin_project_instance_id,
             origin_repository_path = excluded.origin_repository_path,
             origin_external_namespace = excluded.origin_external_namespace,
             origin_external_value = excluded.origin_external_value,
             source_start_byte = excluded.source_start_byte,
             source_end_byte = excluded.source_end_byte,
             source_start_line = excluded.source_start_line,
             source_end_line = excluded.source_end_line,
             resolver_name = excluded.resolver_name,
             resolver_version = excluded.resolver_version,
             content_span_fingerprint = excluded.content_span_fingerprint,
             occurrence_discriminator = excluded.occurrence_discriminator,
             evidence_class = excluded.evidence_class,
             confidence = excluded.confidence,
             completeness = excluded.completeness,
             explanation = excluded.explanation,
             last_changed_epoch = excluded.last_changed_epoch
         WHERE graph_evidence_occurrences.stable_key_version = excluded.stable_key_version
           AND graph_evidence_occurrences.stable_key_canonical = excluded.stable_key_canonical",
    )?;
    let mut insert_resolution = connection.prepare_cached(
        "INSERT INTO graph_resolution_occurrences(
             stable_key_digest, stable_key_version, stable_key_canonical,
             source_entity_digest, relation_kind, origin_kind, origin_entity_digest,
             origin_project_instance_id, origin_repository_path,
             origin_external_namespace, origin_external_value,
             source_start_byte, source_end_byte, source_start_line, source_end_line,
             resolver_name, resolver_version, content_span_fingerprint,
             occurrence_discriminator, resolution_status, candidate_total,
             candidate_completeness, unresolved_reason, evidence_class, confidence,
             completeness, parser_kind, parser_identity, parser_version,
             structural_slot, last_changed_epoch
         ) VALUES(
             ?1, ?2, ?3, ?4, ?5, 'repository-path', NULL, ?6, ?7, NULL, NULL,
             NULL, NULL, NULL, NULL, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
             ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23
         )
         ON CONFLICT(structural_slot, stable_key_digest) DO UPDATE SET
             stable_key_version = excluded.stable_key_version,
             stable_key_canonical = excluded.stable_key_canonical,
             source_entity_digest = excluded.source_entity_digest,
             relation_kind = excluded.relation_kind,
             origin_kind = excluded.origin_kind,
             origin_entity_digest = excluded.origin_entity_digest,
             origin_project_instance_id = excluded.origin_project_instance_id,
             origin_repository_path = excluded.origin_repository_path,
             origin_external_namespace = excluded.origin_external_namespace,
             origin_external_value = excluded.origin_external_value,
             source_start_byte = excluded.source_start_byte,
             source_end_byte = excluded.source_end_byte,
             source_start_line = excluded.source_start_line,
             source_end_line = excluded.source_end_line,
             resolver_name = excluded.resolver_name,
             resolver_version = excluded.resolver_version,
             content_span_fingerprint = excluded.content_span_fingerprint,
             occurrence_discriminator = excluded.occurrence_discriminator,
             resolution_status = excluded.resolution_status,
             candidate_total = excluded.candidate_total,
             candidate_completeness = excluded.candidate_completeness,
             unresolved_reason = excluded.unresolved_reason,
             evidence_class = excluded.evidence_class,
             confidence = excluded.confidence,
             completeness = excluded.completeness,
             parser_kind = excluded.parser_kind,
             parser_identity = excluded.parser_identity,
             parser_version = excluded.parser_version,
             last_changed_epoch = excluded.last_changed_epoch
         WHERE graph_resolution_occurrences.stable_key_version = excluded.stable_key_version
           AND graph_resolution_occurrences.stable_key_canonical = excluded.stable_key_canonical",
    )?;
    let mut insert_resolution_candidate = connection.prepare_cached(
        "INSERT INTO graph_resolution_candidates(
             resolution_occurrence_digest, candidate_ordinal, target_scope,
             target_entity_digest, external_target_namespace, external_target_value,
             confidence, explanation, structural_slot, last_changed_epoch
         ) VALUES(?1, ?2, 'internal', ?3, NULL, NULL, ?4, ?5, ?6, ?7)
         ON CONFLICT(structural_slot, resolution_occurrence_digest, candidate_ordinal)
         DO UPDATE SET
             target_scope = excluded.target_scope,
             target_entity_digest = excluded.target_entity_digest,
             external_target_namespace = excluded.external_target_namespace,
             external_target_value = excluded.external_target_value,
             confidence = excluded.confidence,
             explanation = excluded.explanation,
             last_changed_epoch = excluded.last_changed_epoch",
    )?;
    for relation in &relations {
        let source =
            entities
                .get(relation.source_index)
                .ok_or_else(|| DbError::InvalidGraphRelation {
                    message: "compact relation source index exceeded the entity batch".to_owned(),
                })?;
        let target_digest = match relation.target {
            CompactTypedGraphTarget::Internal(target) => Some(target.digest()),
            CompactTypedGraphTarget::External { .. } => None,
        };
        let (target_scope, external_namespace, external_value) = match relation.target {
            CompactTypedGraphTarget::Internal(_) => ("internal", None, None),
            CompactTypedGraphTarget::External { namespace, value } => {
                ("external", Some(namespace), Some(value))
            }
        };
        let relation_digest = relation.key.digest();
        let source_digest = source.key.digest();
        let affected_rows = insert_relation.execute(params![
            &relation_digest[..],
            i64::from(relation.key.encoding_version()),
            key_arena.logical_edge_canonical_identity(relation.key),
            &source_digest[..],
            relation.kind.as_str(),
            target_scope,
            target_digest.as_ref().map(|digest| &digest[..]),
            external_namespace,
            external_value,
            relation.confidence.as_str(),
            relation.parser.as_str(),
            GRAPH_PARSER_IDENTITY,
            GRAPH_PARSER_VERSION,
            structural_slot,
            last_changed_epoch,
        ])?;
        require_stable_graph_identity_upsert(
            affected_rows,
            GRAPH_RELATIONS_TABLE,
            structural_slot,
        )?;

        let evidence_digest = relation.evidence_key.digest();
        let fingerprint = relation.span_fingerprint.as_bytes();
        let affected_rows = insert_evidence.execute(params![
            &evidence_digest[..],
            i64::from(relation.evidence_key.encoding_version()),
            key_arena.evidence_canonical_identity(relation.evidence_key),
            &relation_digest[..],
            &project_bytes[..],
            graph.path(),
            GRAPH_PARSER_IDENTITY,
            GRAPH_PARSER_VERSION,
            &fingerprint[..],
            i64::from(relation.occurrence_discriminator),
            EvidenceClass::Direct.as_str(),
            relation.confidence.as_str(),
            Completeness::Complete.as_str(),
            relation.explanation,
            structural_slot,
            last_changed_epoch,
        ])?;
        require_stable_graph_identity_upsert(
            affected_rows,
            GRAPH_EVIDENCE_OCCURRENCES_TABLE,
            structural_slot,
        )?;
    }
    for resolution in &resolutions {
        let source =
            entities
                .get(resolution.source_index)
                .ok_or_else(|| DbError::InvalidGraphRelation {
                    message: "compact resolution source index exceeded the entity batch".to_owned(),
                })?;
        let (status, candidate_total, candidate_completeness, unresolved_reason) =
            match resolution.state {
                CompactTypedGraphResolutionState::Ambiguous {
                    candidate_total,
                    candidate_completeness,
                    ..
                } => (
                    "ambiguous",
                    Some(usize_to_i64(candidate_total)),
                    Some(candidate_completeness.as_str()),
                    None,
                ),
                CompactTypedGraphResolutionState::Unresolved { reason } => {
                    ("unresolved", None, None, Some(reason))
                }
            };
        let resolution_digest = resolution.key.digest();
        let source_digest = source.key.digest();
        let fingerprint = resolution.span_fingerprint.as_bytes();
        let affected_rows = insert_resolution.execute(params![
            &resolution_digest[..],
            i64::from(resolution.key.encoding_version()),
            key_arena.resolution_canonical_identity(resolution.key),
            &source_digest[..],
            resolution.kind.as_str(),
            &project_bytes[..],
            graph.path(),
            GRAPH_PARSER_IDENTITY,
            GRAPH_PARSER_VERSION,
            &fingerprint[..],
            i64::from(resolution.occurrence_discriminator),
            status,
            candidate_total,
            candidate_completeness,
            unresolved_reason,
            EvidenceClass::Direct.as_str(),
            resolution.confidence.as_str(),
            Completeness::Complete.as_str(),
            resolution.parser.as_str(),
            GRAPH_PARSER_IDENTITY,
            GRAPH_PARSER_VERSION,
            structural_slot,
            last_changed_epoch,
        ])?;
        require_stable_graph_identity_upsert(
            affected_rows,
            GRAPH_RESOLUTION_OCCURRENCES_TABLE,
            structural_slot,
        )?;

        if let CompactTypedGraphResolutionState::Ambiguous {
            candidate_start,
            candidate_end,
            ..
        } = resolution.state
        {
            for (ordinal, (_, entity_index)) in entity_indices[candidate_start..candidate_end]
                .iter()
                .enumerate()
            {
                let candidate =
                    entities
                        .get(*entity_index)
                        .ok_or_else(|| DbError::InvalidGraphRelation {
                            message: "compact resolution candidate index exceeded the entity batch"
                                .to_owned(),
                        })?;
                let candidate_digest = candidate.key.digest();
                insert_resolution_candidate.execute(params![
                    &resolution_digest[..],
                    usize_to_i64(ordinal),
                    &candidate_digest[..],
                    ConfidenceClass::High.as_str(),
                    "same-file exact-name candidate",
                    structural_slot,
                    last_changed_epoch,
                ])?;
            }
        }
    }
    Ok(())
}

/// Derive typed stable entities directly from one compact parser graph.
fn compact_typed_graph_entities<'graph>(
    project: ProjectInstanceId,
    graph: &'graph CompactSymbolGraph,
    graph_path: &RepositoryFilePath,
    key_arena: &mut GraphKeyArena,
    qualified_name_arena: &mut String,
) -> DbResult<Vec<CompactTypedGraphEntity<'graph>>> {
    let mut entities = Vec::with_capacity(graph.symbol_count().saturating_add(1));
    entities.push(CompactTypedGraphEntity {
        lookup_name: GRAPH_FILE_SOURCE_NAME,
        key: key_arena.entity_key(project, GraphEntityKeyInput::File { path: graph_path })?,
        kind: GraphEntityKind::File,
        repository_path: graph.path(),
        qualified_name: CompactTypedGraphText::Borrowed(graph.path()),
        signature: None,
        external_namespace: None,
        external_value: None,
        language: graph.language(),
        parser: graph.parser(),
    });
    for symbol in graph.symbols().filter(|symbol| {
        !matches!(
            symbol.kind(),
            SymbolKind::Dependency | SymbolKind::Import | SymbolKind::Unknown
        )
    }) {
        let qualified_name = compact_typed_graph_qualified_name(
            symbol.parent(),
            symbol.name(),
            qualified_name_arena,
        );
        let qualified_name_value = qualified_name.as_str(qualified_name_arena);
        let signature = (!symbol.signature().is_empty()).then(|| symbol.signature());
        let (key_input, external_namespace, external_value) = match symbol.kind() {
            SymbolKind::Package | SymbolKind::Workspace => {
                let namespace = package_entity_namespace(symbol.kind(), symbol.language());
                (
                    GraphEntityKeyInput::Package {
                        namespace,
                        value: symbol.name(),
                    },
                    Some(namespace),
                    Some(symbol.name()),
                )
            }
            SymbolKind::Module => (
                GraphEntityKeyInput::Module {
                    path: graph_path,
                    qualified_name: qualified_name_value,
                },
                None,
                None,
            ),
            _ => (
                GraphEntityKeyInput::Declaration {
                    path: graph_path,
                    qualified_name: qualified_name_value,
                    signature,
                },
                None,
                None,
            ),
        };
        entities.push(CompactTypedGraphEntity {
            lookup_name: symbol.name(),
            key: key_arena.entity_key(project, key_input)?,
            kind: key_input.entity_kind(),
            repository_path: symbol.path(),
            qualified_name,
            signature,
            external_namespace,
            external_value,
            language: symbol.language(),
            parser: symbol.parser(),
        });
    }
    Ok(entities)
}

/// Retain one qualified name in borrowed form or the shared batch text arena.
fn compact_typed_graph_qualified_name<'graph>(
    parent: Option<&'graph str>,
    name: &'graph str,
    shared: &mut String,
) -> CompactTypedGraphText<'graph> {
    let Some(parent) = parent else {
        return CompactTypedGraphText::Borrowed(name);
    };
    let start = shared.len();
    shared.push_str(parent);
    shared.push_str("::");
    shared.push_str(name);
    CompactTypedGraphText::Shared {
        start,
        length: shared.len() - start,
    }
}

/// Derive traversable edges plus typed abstentions without expanding compact parser facts.
fn compact_typed_graph_relations<'graph>(
    project: ProjectInstanceId,
    graph: &'graph CompactSymbolGraph,
    entities: &[CompactTypedGraphEntity<'graph>],
    evidence_origin_path: &RepositoryPath,
    key_arena: &mut GraphKeyArena,
) -> DbResult<CompactTypedGraphRelationPlan<'graph>> {
    let entity_indices = compact_typed_graph_entity_indices(entities);
    let evidence_origin = GraphEvidenceOriginInput::RepositoryPath {
        project,
        path: evidence_origin_path,
    };
    let evidence_resolver = GraphResolverInput {
        name: GRAPH_PARSER_IDENTITY,
        version: GRAPH_PARSER_VERSION,
    };
    let mut evidence_discriminators = HashMap::<([u8; 32], [u8; 32]), u32>::new();
    let mut resolution_discriminators =
        HashMap::<([u8; 32], GraphRelationKind, [u8; 32]), u32>::new();
    let mut relations = Vec::with_capacity(graph.relation_count());
    let mut resolutions = Vec::new();
    let candidate_limit = usize::try_from(
        DefaultCoreBudgetKind::ResolutionCandidates
            .default_budget()
            .value(),
    )
    .map_err(|_source| DbError::InvalidGraphRelation {
        message: "resolution-candidate budget exceeded this platform's index width".to_owned(),
    })?;
    for relation in graph.relations() {
        let kind = GraphRelationKind::from(relation.kind());
        let span_fingerprint = ContentSpanFingerprint::from_content(relation.context().as_bytes());
        let (source_start, source_end) =
            compact_typed_graph_entity_match_range(&entity_indices, relation.source_name());
        let source_count = source_end - source_start;
        let source_index = match source_count {
            1 => entity_indices[source_start].1,
            0 if compact_relation_has_file_owned_manifest_source(
                graph,
                relation.source_name(),
                relation.kind(),
                relation.parser(),
            ) =>
            {
                0
            }
            _ => {
                let source = entities
                    .first()
                    .ok_or_else(|| DbError::InvalidGraphRelation {
                        message: "compact entity batch omitted its containing file".to_owned(),
                    })?;
                let occurrence = CompactTypedGraphOccurrence {
                    source_index: 0,
                    source_key: source.key,
                    kind,
                    parser: relation.parser(),
                    origin: evidence_origin,
                    resolver: evidence_resolver,
                    span_fingerprint,
                };
                let occurrence_discriminator = next_compact_typed_graph_occurrence_discriminator(
                    &mut resolution_discriminators,
                    (source.key.digest(), kind, span_fingerprint.as_bytes()),
                )?;
                let (reason, confidence) = if source_count == 0 {
                    (
                        "relation source was not resolved in the containing file",
                        ConfidenceClass::Low,
                    )
                } else {
                    (
                        "relation source name matched multiple entities in the containing file",
                        ConfidenceClass::Medium,
                    )
                };
                resolutions.push(occurrence.resolution(
                    key_arena,
                    occurrence_discriminator,
                    CompactTypedGraphResolutionState::Unresolved { reason },
                    confidence,
                )?);
                continue;
            }
        };
        let source = entities
            .get(source_index)
            .ok_or_else(|| DbError::InvalidGraphRelation {
                message: "compact relation source index exceeded the entity batch".to_owned(),
            })?;
        let occurrence = CompactTypedGraphOccurrence {
            source_index,
            source_key: source.key,
            kind,
            parser: relation.parser(),
            origin: evidence_origin,
            resolver: evidence_resolver,
            span_fingerprint,
        };
        let target = match relation.kind() {
            RelationKind::DependsOn => CompactTypedGraphTarget::External {
                namespace: GRAPH_EXTERNAL_PACKAGE_NAMESPACE,
                value: relation.target_name(),
            },
            RelationKind::Imports => {
                let occurrence_discriminator = next_compact_typed_graph_occurrence_discriminator(
                    &mut resolution_discriminators,
                    (source.key.digest(), kind, span_fingerprint.as_bytes()),
                )?;
                resolutions.push(occurrence.resolution(
                    key_arena,
                    occurrence_discriminator,
                    CompactTypedGraphResolutionState::Unresolved {
                        reason:
                            "import target is not resolved by the same-file compatibility resolver",
                    },
                    ConfidenceClass::Low,
                )?);
                continue;
            }
            RelationKind::Contains | RelationKind::Calls => {
                let (target_start, target_end) =
                    compact_typed_graph_entity_match_range(&entity_indices, relation.target_name());
                let target_count = target_end - target_start;
                if target_count == 1 {
                    let target = entities
                        .get(entity_indices[target_start].1)
                        .ok_or_else(|| DbError::InvalidGraphRelation {
                            message: "compact relation target index exceeded the entity batch"
                                .to_owned(),
                        })?;
                    CompactTypedGraphTarget::Internal(target.key)
                } else {
                    let occurrence_discriminator =
                        next_compact_typed_graph_occurrence_discriminator(
                            &mut resolution_discriminators,
                            (source.key.digest(), kind, span_fingerprint.as_bytes()),
                        )?;
                    let (state, confidence) = if target_count == 0 {
                        let reason = match relation.kind() {
                            RelationKind::Contains => {
                                "contained target was not resolved in the containing file"
                            }
                            RelationKind::Calls => {
                                "call target was not resolved in the containing file"
                            }
                            RelationKind::Imports | RelationKind::DependsOn => unreachable!(
                                "target absence is handled only for containment and calls"
                            ),
                        };
                        (
                            CompactTypedGraphResolutionState::Unresolved { reason },
                            ConfidenceClass::Low,
                        )
                    } else {
                        let candidate_end = target_start + target_count.min(candidate_limit);
                        let candidate_completeness = if candidate_end == target_end {
                            Completeness::Complete
                        } else {
                            Completeness::Truncated
                        };
                        (
                            CompactTypedGraphResolutionState::Ambiguous {
                                candidate_start: target_start,
                                candidate_end,
                                candidate_total: target_count,
                                candidate_completeness,
                            },
                            ConfidenceClass::Medium,
                        )
                    };
                    resolutions.push(occurrence.resolution(
                        key_arena,
                        occurrence_discriminator,
                        state,
                        confidence,
                    )?);
                    continue;
                }
            }
        };
        let confidence = match relation.kind() {
            RelationKind::Calls => ConfidenceClass::High,
            RelationKind::Contains | RelationKind::DependsOn => ConfidenceClass::Exact,
            RelationKind::Imports => unreachable!("imports remain non-traversable"),
        };
        let key = key_arena.logical_edge_key(source.key, target.key_input(), kind)?;
        let occurrence_discriminator = next_compact_typed_graph_occurrence_discriminator(
            &mut evidence_discriminators,
            (key.digest(), span_fingerprint.as_bytes()),
        )?;
        let evidence_key = key_arena.evidence_key(
            key,
            evidence_origin,
            evidence_resolver,
            span_fingerprint,
            occurrence_discriminator,
        )?;
        relations.push(CompactTypedGraphRelation {
            key,
            source_index,
            kind,
            target,
            parser: relation.parser(),
            confidence,
            evidence_key,
            span_fingerprint,
            occurrence_discriminator,
            explanation: IdentityText::validate(relation.context())
                .is_ok()
                .then_some(relation.context()),
        });
    }
    Ok(CompactTypedGraphRelationPlan {
        relations,
        resolutions,
        entity_indices,
    })
}

/// Accept file ownership only for the exact Cargo manifest producer contract.
fn compact_relation_has_file_owned_manifest_source(
    graph: &CompactSymbolGraph,
    source_name: &str,
    relation_kind: RelationKind,
    relation_parser: ParserKind,
) -> bool {
    relation_kind == RelationKind::DependsOn
        && source_name == GRAPH_CARGO_MANIFEST_SOURCE_NAME
        && graph.parser() == ParserKind::Manifest
        && relation_parser == ParserKind::Manifest
        && graph.language() == Some(GRAPH_CARGO_MANIFEST_LANGUAGE)
        && graph.path().rsplit('/').next() == Some(GRAPH_CARGO_MANIFEST_FILE_NAME)
}

/// Index unique same-file logical entities by compatibility name and stable key.
fn compact_typed_graph_entity_indices<'graph>(
    entities: &[CompactTypedGraphEntity<'graph>],
) -> Vec<(&'graph str, usize)> {
    let mut indices = entities
        .iter()
        .enumerate()
        .map(|(index, entity)| (entity.lookup_name, index))
        .collect::<Vec<_>>();
    indices.sort_unstable_by(|left, right| {
        left.0
            .cmp(right.0)
            .then_with(|| {
                entities[left.1]
                    .key
                    .digest()
                    .cmp(&entities[right.1].key.digest())
            })
            .then(left.1.cmp(&right.1))
    });
    indices.dedup_by(|left, right| {
        left.0 == right.0 && entities[left.1].key.digest() == entities[right.1].key.digest()
    });
    indices
}

/// Return the half-open range for one compatibility name in the sorted entity index.
fn compact_typed_graph_entity_match_range(indices: &[(&str, usize)], name: &str) -> (usize, usize) {
    let start = indices.partition_point(|(candidate, _)| *candidate < name);
    let end = indices.partition_point(|(candidate, _)| *candidate <= name);
    (start, end)
}

/// Increment one occurrence discriminator without losing its current value.
fn next_compact_typed_graph_occurrence_discriminator<K>(
    discriminators: &mut HashMap<K, u32>,
    identity: K,
) -> DbResult<u32>
where
    K: Eq + std::hash::Hash,
{
    let discriminator = discriminators.entry(identity).or_default();
    let current = *discriminator;
    *discriminator = discriminator
        .checked_add(1)
        .ok_or_else(|| DbError::InvalidGraphRelation {
            message: "one graph fact exceeded the occurrence discriminator bound".to_owned(),
        })?;
    Ok(current)
}

/// Return a stable ecosystem namespace for package and workspace entities.
fn package_entity_namespace(kind: SymbolKind, language: Option<&str>) -> &'static str {
    match (kind, language) {
        (SymbolKind::Workspace, Some("cargo-manifest" | "cargo-lock")) => "cargo-workspace",
        (SymbolKind::Package, Some("cargo-manifest" | "cargo-lock")) => "cargo",
        (SymbolKind::Workspace, _) => "workspace",
        _ => "package",
    }
}

/// Replace one parser-owned compatibility graph within an existing write boundary.
fn replace_compact_symbol_graph_in_connection(
    connection: &Connection,
    graph: &CompactSymbolGraph,
) -> DbResult<()> {
    let (active_slot, active_epoch) = connection.query_row(
        "SELECT active_slot, active_epoch
         FROM graph_publication_state WHERE singleton = 1",
        [],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    )?;
    replace_compact_symbol_graph_at_publication(connection, graph, &active_slot, active_epoch)
}

/// Replace one parser graph at an already selected slot and publication epoch.
pub(crate) fn replace_compact_symbol_graph_at_publication(
    connection: &Connection,
    graph: &CompactSymbolGraph,
    structural_slot: &str,
    last_changed_epoch: i64,
) -> DbResult<()> {
    let path = graph.path();
    replace_typed_graph_for_compact_symbol_graph(
        connection,
        graph,
        structural_slot,
        last_changed_epoch,
    )?;
    connection
        .prepare_cached("DELETE FROM symbols WHERE path = ?1")?
        .execute([path])?;
    connection
        .prepare_cached("DELETE FROM symbol_relations WHERE path = ?1")?
        .execute([path])?;
    connection
        .prepare_cached(
            "INSERT INTO source_parse_metadata(
             path, language, parser, symbol_count, relation_count, updated_at
         )
         VALUES(?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP)
         ON CONFLICT(path) DO UPDATE SET
             language = excluded.language,
             parser = excluded.parser,
             symbol_count = excluded.symbol_count,
             relation_count = excluded.relation_count,
             updated_at = CURRENT_TIMESTAMP",
        )?
        .execute(params![
            path,
            graph.language(),
            graph.parser().as_str(),
            usize_to_i64(graph.symbol_count()),
            usize_to_i64(graph.relation_count()),
        ])?;
    let node_id = connection
        .query_row(
            "SELECT id FROM nodes WHERE path = ?1 AND exists_now = 1",
            [path],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let mut insert_symbol = connection.prepare_cached(
        "INSERT INTO symbols(
             path, language, name, kind, signature, exported, documentation,
             line_start, line_end, parent, parser, detail
         )
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )?;
    for symbol in graph.symbols() {
        insert_symbol.execute(params![
            symbol.path(),
            symbol.language(),
            symbol.name(),
            symbol.kind().as_str(),
            symbol.signature(),
            symbol.exported(),
            symbol.documentation(),
            i64::from(symbol.line_start()),
            i64::from(symbol.line_end()),
            symbol.parent(),
            symbol.parser().as_str(),
            symbol.detail(),
        ])?;
    }
    let mut insert_relation = connection.prepare_cached(
        "INSERT INTO symbol_relations(
             path, source_name, target_name, kind, line, context, parser
         )
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    for relation in graph.relations() {
        insert_relation.execute(params![
            relation.path(),
            relation.source_name(),
            relation.target_name(),
            relation.kind().as_str(),
            i64::from(relation.line()),
            relation.context(),
            relation.parser().as_str(),
        ])?;
    }
    if let Some(node_id) = node_id {
        replace_symbol_search_summary(
            connection,
            node_id,
            symbol_search_summary(graph).as_deref(),
        )?;
    }
    Ok(())
}

/// Upsert one scanned node into an existing transaction.
fn upsert_node(connection: &Connection, node: &Node) -> DbResult<()> {
    let existing = connection
        .prepare_cached(
            "
            SELECT n.content_hash, p.status
            FROM nodes n
            LEFT JOIN purposes p ON p.node_id = n.id
            WHERE n.path = ?1
            ",
        )?
        .query_row([&node.path], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
            ))
        })
        .optional()?;
    let content_changed = existing.as_ref().is_some_and(|(old_hash, _)| {
        node.kind == NodeKind::File
            && old_hash.is_some()
            && node.content_hash.is_some()
            && old_hash != &node.content_hash
    });
    let should_mark_stale = content_changed
        && existing.as_ref().and_then(|(_, status)| status.as_deref())
            == Some(PurposeStatus::Approved.as_str());
    connection
        .prepare_cached(
            "
        INSERT INTO nodes(path, kind, parent_path, extension, language, size_bytes, mtime_ns, content_hash, exists_now)
        VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)
        ON CONFLICT(path) DO UPDATE SET
            kind = excluded.kind,
            parent_path = excluded.parent_path,
            extension = excluded.extension,
            language = excluded.language,
            size_bytes = excluded.size_bytes,
            mtime_ns = excluded.mtime_ns,
            content_hash = excluded.content_hash,
            exists_now = 1,
            last_seen_at = CURRENT_TIMESTAMP,
            last_indexed_at = CURRENT_TIMESTAMP
        ",
        )?
        .execute(params![
                node.path,
                node.kind.to_string(),
                node.parent_path,
                node.extension,
                node.language,
                node.size_bytes,
                node.mtime_ns,
                node.content_hash
            ])?;
    let node_id = connection
        .prepare_cached("SELECT id FROM nodes WHERE path = ?1")?
        .query_row([&node.path], |row| row.get::<_, i64>(0))?;
    connection
        .prepare_cached(
            "
        INSERT INTO purposes(node_id, purpose, source, status)
        VALUES(?1, NULL, 'missing', 'missing')
        ON CONFLICT(node_id) DO NOTHING
        ",
        )?
        .execute([node_id])?;
    let summary = generate_node_summary(node);
    connection
        .prepare_cached(
            "
        INSERT INTO summaries(node_id, summary_level, subject, summary, updated_at)
        VALUES(?1, 'node', '', ?2, CURRENT_TIMESTAMP)
        ON CONFLICT(node_id, summary_level, subject) DO UPDATE SET
            summary = CASE WHEN ?3 THEN excluded.summary ELSE summaries.summary END,
            updated_at = CURRENT_TIMESTAMP
        ",
        )?
        .execute(params![node_id, summary, content_changed])?;
    if should_mark_stale {
        connection
            .prepare_cached(
                "
            UPDATE purposes
            SET status = 'stale',
                updated_at = CURRENT_TIMESTAMP
            WHERE node_id = ?1
            ",
            )?
            .execute([node_id])?;
    }
    Ok(())
}

/// Upsert one persisted UTF-8 source-text row for indexed search.
fn upsert_file_text(connection: &Connection, text: &IndexedFileText) -> DbResult<()> {
    let (active_slot, active_epoch) = connection.query_row(
        "SELECT active_slot, active_epoch
         FROM graph_publication_state WHERE singleton = 1",
        [],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    )?;
    upsert_file_text_for_publication(connection, text, &active_slot, active_epoch)
}

/// Upsert one lexical row with an explicit transactionally owned publication tuple.
fn upsert_file_text_for_publication(
    connection: &Connection,
    text: &IndexedFileText,
    structural_slot: &str,
    last_changed_epoch: i64,
) -> DbResult<()> {
    connection
        .prepare_cached(
            "
        INSERT INTO file_texts(
            path, content_hash, byte_count, line_count, content, updated_at,
            structural_slot, last_changed_epoch
        )
        VALUES(?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP, ?6, ?7)
        ON CONFLICT(structural_slot, path) DO UPDATE SET
            content_hash = excluded.content_hash,
            byte_count = excluded.byte_count,
            line_count = excluded.line_count,
            content = excluded.content,
            updated_at = CURRENT_TIMESTAMP,
            last_changed_epoch = excluded.last_changed_epoch
        ",
        )?
        .execute(params![
            text.path,
            text.content_hash.as_deref(),
            usize_to_i64(text.byte_count),
            usize_to_i64(text.line_count),
            text.content,
            structural_slot,
            last_changed_epoch,
        ])?;
    Ok(())
}

/// Read one persisted indexed text row.
fn file_text_from_row(row: &rusqlite::Row<'_>) -> DbResult<IndexedFileText> {
    let byte_count = count_to_usize("file_texts.byte_count", row.get::<_, i64>(2)?)?;
    let line_count = count_to_usize("file_texts.line_count", row.get::<_, i64>(3)?)?;
    Ok(IndexedFileText {
        path: row.get(0)?,
        content_hash: row.get(1)?,
        byte_count,
        line_count,
        content: row.get(4)?,
    })
}

/// Read one typed persisted graph entity from the selected column order.
fn persisted_graph_entity_from_row(row: &rusqlite::Row<'_>) -> DbResult<PersistedGraphEntity> {
    let kind_value = graph_text(row, 1, "graph entity kind")?;
    let kind = GraphEntityKind::ALL
        .into_iter()
        .find(|kind| kind.as_str() == kind_value)
        .ok_or_else(|| DbError::InvalidEnum {
            field: "graph entity kind",
            value: kind_value.to_owned(),
        })?;
    Ok(PersistedGraphEntity {
        stable_key_digest: graph_digest_from_row(row, 0, "graph entity stable key")?,
        kind,
        repository_path: row.get(2)?,
        qualified_name: row.get(3)?,
        signature: row.get(4)?,
        discriminator: row.get(5)?,
        last_changed_epoch: graph_epoch(row.get(6)?, "graph entity last changed epoch")?,
    })
}

/// Read one typed persisted graph relation from the selected column order.
fn persisted_graph_relation_from_row(row: &rusqlite::Row<'_>) -> DbResult<PersistedGraphRelation> {
    let kind_value = graph_text(row, 2, "graph relation kind")?;
    let kind = GraphRelationKind::ALL
        .into_iter()
        .find(|kind| kind.as_str() == kind_value)
        .ok_or_else(|| DbError::InvalidEnum {
            field: "graph relation kind",
            value: kind_value.to_owned(),
        })?;
    let target_scope = graph_text(row, 3, "graph relation target scope")?;
    let target_digest = graph_optional_digest_from_row(row, 4, "graph relation target entity")?;
    let external_namespace = row.get::<_, Option<String>>(5)?;
    let external_value = row.get::<_, Option<String>>(6)?;
    let target = match (
        target_scope,
        target_digest,
        external_namespace,
        external_value,
    ) {
        ("internal", Some(digest), None, None) => PersistedGraphTarget::Internal(digest),
        ("external", None, Some(namespace), Some(value)) => {
            PersistedGraphTarget::External { namespace, value }
        }
        (scope, _, _, _) => {
            return Err(DbError::InvalidGraphRelation {
                message: format!("target fields do not match scope {scope:?}"),
            });
        }
    };
    Ok(PersistedGraphRelation {
        stable_key_digest: graph_digest_from_row(row, 0, "graph relation stable key")?,
        source_entity_digest: graph_digest_from_row(row, 1, "graph relation source entity")?,
        kind,
        target,
        last_changed_epoch: graph_epoch(row.get(7)?, "graph relation last changed epoch")?,
    })
}

/// Run one prepared bounded relation query and preserve terminal row status.
fn load_graph_relations(
    connection: &Connection,
    sql: &str,
    parameters: impl Params,
) -> DbResult<Vec<PersistedGraphRelation>> {
    let mut statement = connection.prepare_cached(sql)?;
    let rows = statement.query_and_then(parameters, persisted_graph_relation_from_row)?;
    let mut relations = Vec::new();
    visit_rows_to_terminal(rows, &mut |relation| {
        relations.push(relation);
        Ok(true)
    })?;
    Ok(relations)
}

/// Borrow one `SQLite` text value without allocating an owned scalar.
fn graph_text<'row>(
    row: &'row rusqlite::Row<'_>,
    index: usize,
    field: &'static str,
) -> DbResult<&'row str> {
    match row.get_ref(index)? {
        ValueRef::Text(value) => {
            std::str::from_utf8(value).map_err(|source| DbError::InvalidGraphRelation {
                message: format!("{field} is not valid UTF-8: {source}"),
            })
        }
        value => Err(DbError::InvalidGraphRelation {
            message: format!(
                "{field} has SQLite type {:?}, expected text",
                value.data_type()
            ),
        }),
    }
}

/// Copy one borrowed fixed-width `SQLite` graph digest.
fn graph_digest_from_row(
    row: &rusqlite::Row<'_>,
    index: usize,
    field: &'static str,
) -> DbResult<[u8; 32]> {
    match row.get_ref(index)? {
        ValueRef::Blob(value) => graph_digest(value, field),
        value => Err(DbError::InvalidGraphRelation {
            message: format!(
                "{field} has SQLite type {:?}, expected blob",
                value.data_type()
            ),
        }),
    }
}

/// Copy one optional borrowed fixed-width `SQLite` graph digest.
fn graph_optional_digest_from_row(
    row: &rusqlite::Row<'_>,
    index: usize,
    field: &'static str,
) -> DbResult<Option<[u8; 32]>> {
    match row.get_ref(index)? {
        ValueRef::Null => Ok(None),
        ValueRef::Blob(value) => graph_digest(value, field).map(Some),
        value => Err(DbError::InvalidGraphRelation {
            message: format!(
                "{field} has SQLite type {:?}, expected blob",
                value.data_type()
            ),
        }),
    }
}

/// Convert a checked `SQLite` graph digest into its fixed-width representation.
fn graph_digest(value: &[u8], field: &'static str) -> DbResult<[u8; 32]> {
    let found = value.len();
    if found != 32 {
        return Err(DbError::InvalidByteLength {
            field,
            expected: 32,
            found,
        });
    }
    let mut digest = [0_u8; 32];
    digest.copy_from_slice(value);
    Ok(digest)
}

/// Convert a non-negative `SQLite` epoch into the typed graph contract.
fn graph_epoch(value: i64, field: &'static str) -> DbResult<IndexEpoch> {
    u64::try_from(value)
        .map(IndexEpoch::new)
        .map_err(|source| DbError::InvalidCount {
            field,
            value,
            source,
        })
}

/// Consume a started result page through its terminal `SQLite` status.
fn visit_rows_to_terminal<T>(
    rows: impl Iterator<Item = DbResult<T>>,
    visitor: &mut impl FnMut(T) -> DbResult<bool>,
) -> DbResult<()> {
    let mut visit = true;
    for row in rows {
        let value = row.map_err(database_row_error)?;
        if visit {
            visit = visitor(value)?;
        }
    }
    Ok(())
}

/// Attach recovery guidance without erasing the original `SQLite` code.
fn database_row_error(error: DbError) -> DbError {
    match error {
        DbError::Sqlite(source)
            if matches!(
                source.sqlite_error_code(),
                Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase)
            ) =>
        {
            DbError::DatabaseCorruption { source }
        }
        other => other,
    }
}

/// Build an indexed node from the standard node select column order.
fn indexed_node_from_sql_row(row: &rusqlite::Row<'_>) -> DbResult<IndexedNode> {
    let kind_value: String = row.get(1)?;
    let source_value: String = row.get(9)?;
    let status_value: String = row.get(10)?;
    indexed_node_from_parts((
        row.get::<_, String>(0)?,
        kind_value,
        row.get::<_, Option<String>>(2)?,
        row.get::<_, Option<String>>(3)?,
        row.get::<_, Option<String>>(4)?,
        row.get::<_, Option<u64>>(5)?,
        row.get::<_, Option<i64>>(6)?,
        row.get::<_, Option<String>>(7)?,
        row.get::<_, Option<String>>(8)?,
        source_value,
        status_value,
        row.get::<_, Option<String>>(11)?,
    ))
}

/// Build an indexed node from database row parts.
fn indexed_node_from_parts(
    row: (
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<u64>,
        Option<i64>,
        Option<String>,
        Option<String>,
        String,
        String,
        Option<String>,
    ),
) -> DbResult<IndexedNode> {
    let (
        path,
        kind_value,
        parent_path,
        extension,
        language,
        size_bytes,
        mtime_ns,
        content_hash,
        purpose,
        source_value,
        status_value,
        summary,
    ) = row;
    let kind = NodeKind::from_db(&kind_value).ok_or_else(|| DbError::InvalidEnum {
        field: "kind",
        value: kind_value,
    })?;
    let source = parse_source(&source_value)?;
    let status = PurposeStatus::from_db(&status_value).ok_or_else(|| DbError::InvalidEnum {
        field: "status",
        value: status_value,
    })?;
    Ok(IndexedNode {
        node: Node {
            path: path.clone(),
            kind,
            parent_path,
            extension,
            language,
            size_bytes,
            mtime_ns,
            content_hash,
        },
        purpose: Purpose {
            path,
            purpose,
            source,
            status,
        },
        summary,
    })
}

/// Split a user query into lowercase terms for SQL ranking.
fn normalize_query_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Build the SQL score expression for ranked node lookup.
fn ranked_score_expression(term_count: usize) -> String {
    if term_count == 0 {
        return "1".to_string();
    }
    (0..term_count)
        .map(|_| {
            "(CASE WHEN lower(n.path) LIKE ? ESCAPE '\\' THEN 20 ELSE 0 END \
             + CASE WHEN lower(COALESCE(p.purpose, '')) LIKE ? ESCAPE '\\' THEN 30 ELSE 0 END \
             + CASE WHEN lower(COALESCE(s.summary, '')) LIKE ? ESCAPE '\\' THEN 10 ELSE 0 END \
             + CASE WHEN lower(COALESCE(symbol_summaries.summary, '')) LIKE ? ESCAPE '\\' THEN 25 ELSE 0 END)"
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

/// Convert a normalized term into a `SQLite` LIKE pattern.
fn sqlite_like_pattern(term: &str) -> String {
    format!("%{}%", sqlite_like_escape(term))
}

/// Build a `SQLite` LIKE descendant pattern for a repository path prefix.
fn sqlite_descendant_pattern(path: &str) -> String {
    format!("{}/%", sqlite_like_escape(path))
}

/// Build binary-collation bounds for normalized descendants of one repository path.
fn sqlite_descendant_bounds(path: &str) -> (String, String) {
    (format!("{path}/"), format!("{path}0"))
}

/// Escape user or path text for `SQLite` LIKE patterns with backslash escaping.
fn sqlite_like_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Replace the denormalized symbol-name search summary for one file node.
fn replace_symbol_search_summary(
    connection: &Connection,
    node_id: i64,
    summary: Option<&str>,
) -> DbResult<()> {
    if let Some(summary) = summary {
        connection.execute(
            "
            INSERT INTO summaries(node_id, summary_level, subject, summary, updated_at)
            VALUES(?1, 'search', 'symbols', ?2, CURRENT_TIMESTAMP)
            ON CONFLICT(node_id, summary_level, subject) DO UPDATE SET
                summary = excluded.summary,
                updated_at = CURRENT_TIMESTAMP
            ",
            params![node_id, summary],
        )?;
    } else {
        connection.execute(
            "
            DELETE FROM summaries
            WHERE node_id = ?1
              AND summary_level = 'search'
              AND subject = 'symbols'
            ",
            [node_id],
        )?;
    }
    Ok(())
}

/// Build a bounded search-only summary from symbol names.
fn symbol_search_summary(graph: &CompactSymbolGraph) -> Option<String> {
    let mut names = BTreeSet::new();
    for name in graph
        .symbols()
        .filter(|symbol| !matches!(symbol.kind(), SymbolKind::Import | SymbolKind::Unknown))
        .map(|symbol| symbol.name().trim())
        .filter(|name| !name.is_empty())
    {
        names.insert(name);
        if names.len() > MAX_SYMBOL_SEARCH_SUMMARY_CHARS {
            // Every retained name consumes at least one output character, so
            // later lexical candidates cannot contribute to the bounded prefix.
            names.pop_last();
        }
    }
    if names.is_empty() {
        return None;
    }
    let mut summary = String::with_capacity(MAX_SYMBOL_SEARCH_SUMMARY_CHARS);
    summary.push_str("symbols ");
    let mut used_chars = summary.chars().count();
    for (index, name) in names.into_iter().enumerate() {
        if index > 0 {
            if used_chars == MAX_SYMBOL_SEARCH_SUMMARY_CHARS {
                break;
            }
            summary.push(' ');
            used_chars += 1;
        }
        for character in name.chars() {
            if used_chars == MAX_SYMBOL_SEARCH_SUMMARY_CHARS {
                return Some(summary);
            }
            summary.push(character);
            used_chars += 1;
        }
    }
    Some(summary)
}

/// Parse a stored purpose source value into the domain enum.
fn parse_source(value: &str) -> DbResult<PurposeSource> {
    let source = match value {
        value if value == PurposeSource::Missing.as_str() => PurposeSource::Missing,
        value if value == PurposeSource::Imported.as_str() => PurposeSource::Imported,
        value if value == PurposeSource::Generated.as_str() => PurposeSource::Generated,
        // Older databases could contain `human`; ProjectAtlas now treats
        // explicit approval as agent-owned and serializes new writes as `agent`.
        value if value == PurposeSource::Agent.as_str() || value == LEGACY_HUMAN_PURPOSE_SOURCE => {
            PurposeSource::Agent
        }
        _ => {
            return Err(DbError::InvalidEnum {
                field: "source",
                value: value.to_string(),
            });
        }
    };
    Ok(source)
}

/// Convert an aggregate database count into a platform `usize`.
fn count_to_usize(field: &'static str, value: i64) -> DbResult<usize> {
    usize::try_from(value).map_err(|source| DbError::InvalidCount {
        field,
        value,
        source,
    })
}

/// Convert a `SQLite` REAL aggregate token total to a saturating wide integer.
fn token_total_from_sql(_field: &'static str, value: f64) -> u128 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else if value >= u128::MAX as f64 {
        u128::MAX
    } else {
        value.round() as u128
    }
}

/// Return the `SQLite` period expression for one token trend window.
fn token_trend_period_expression(window: TokenTrendWindow) -> &'static str {
    match window {
        TokenTrendWindow::Day => "substr(COALESCE(created_at, CURRENT_TIMESTAMP), 1, 10)",
        TokenTrendWindow::Week => "strftime('%Y-W%W', COALESCE(created_at, CURRENT_TIMESTAMP))",
        TokenTrendWindow::Month => "substr(COALESCE(created_at, CURRENT_TIMESTAMP), 1, 7)",
        TokenTrendWindow::Year => "substr(COALESCE(created_at, CURRENT_TIMESTAMP), 1, 4)",
    }
}

/// Convert a usize to i64 with saturation for database storage.
fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Convert a non-negative i64 to usize for database reads.
fn i64_to_usize(value: i64) -> usize {
    usize::try_from(value.max(0)).unwrap_or(usize::MAX)
}

/// Wrap a query string for a SQL LIKE expression.
fn like_query(query: &str) -> String {
    format!("%{query}%")
}

/// Build numbered SQL placeholders starting at a caller-selected index.
fn numbered_placeholders(start: usize, count: usize) -> String {
    (start..start + count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Generate the durable node-level content summary.
fn generate_node_summary(node: &Node) -> String {
    match node.kind {
        NodeKind::Folder => format!("Folder for {}", path_label(&node.path)),
        NodeKind::File => file_summary(node),
    }
}

/// Generate a one-line observed file summary from scan metadata.
fn file_summary(node: &Node) -> String {
    let language = node
        .language
        .as_deref()
        .or(node.extension.as_deref())
        .unwrap_or("unknown");
    let size = node.size_bytes.map_or_else(
        || "unknown size".to_string(),
        |bytes| format!("{bytes} bytes"),
    );
    format!("{language} file, {size}")
}

/// Return a readable label for a repository-relative path.
fn path_label(path: &str) -> String {
    if path == "." {
        return "repository root".to_string();
    }
    path.rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(path)
        .replace(['-', '_'], " ")
}

/// Emit a health finding when it has not already been resolved.
fn emit_unresolved_finding<F>(
    finding: HealthFinding,
    resolved_ids: &HashSet<String>,
    visitor: &mut F,
) -> DbResult<bool>
where
    F: FnMut(HealthFinding) -> DbResult<bool>,
{
    if resolved_ids.contains(&finding.id) {
        return Ok(true);
    }
    visitor(finding)
}

/// Return whether a purpose-status source can match a bounded health query.
fn purpose_health_spec_matches_query(spec: PurposeHealthSpec, query: &HealthQuery) -> bool {
    health_category_matches_query(spec.category, Severity::Warning, query)
}

/// Return whether a health category/severity can match a bounded query.
fn health_category_matches_query(category: &str, severity: Severity, query: &HealthQuery) -> bool {
    query
        .category
        .as_deref()
        .is_none_or(|requested| category.eq_ignore_ascii_case(requested))
        && query.severity.is_none_or(|requested| severity == requested)
}

/// Return purpose health metadata for a stored purpose status.
fn purpose_health_spec_for_status(status: &str) -> DbResult<PurposeHealthSpec> {
    PURPOSE_HEALTH_SPECS
        .iter()
        .copied()
        .find(|spec| spec.status == status)
        .ok_or_else(|| DbError::InvalidEnum {
            field: "status",
            value: status.to_string(),
        })
}

/// Build the health finding for an approved purpose that still needs agent review.
fn agent_review_required_finding(path: String) -> HealthFinding {
    HealthFinding {
        id: finding_id(CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED, &path, None),
        severity: Severity::Warning,
        category: CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED.to_string(),
        path,
        related_path: None,
        message: MESSAGE_PURPOSE_AGENT_REVIEW_REQUIRED.to_string(),
        recommendation: RECOMMENDATION_PURPOSE_AGENT_REVIEW_REQUIRED.to_string(),
    }
}

/// Build the shared SQL filter for globally ordered purpose lifecycle findings.
fn purpose_lifecycle_where_clause(
    path_prefix: Option<&str>,
    resolved_ids: &[String],
    scope: HealthScope,
) -> (String, Vec<Value>) {
    let statuses = PURPOSE_HEALTH_SPECS
        .iter()
        .map(|spec| format!("'{}'", spec.status))
        .collect::<Vec<_>>()
        .join(", ");
    let mut clauses = vec![
        "n.exists_now = 1".to_string(),
        format!("p.status IN ({statuses})"),
    ];
    let mut values = Vec::new();

    if source_filter_applies_before_queue(scope) {
        clauses.push(source_relevant_node_expression("n"));
    }
    if scope.high_impact_queue() {
        clauses.push(purpose_default_queue_node_expression("n", "p", scope));
    }

    let normalized_prefix = path_prefix
        .map(normalize_repo_path_prefix)
        .filter(|prefix| prefix != ".");
    if let Some(prefix) = normalized_prefix {
        clauses.push(format!(
            "(n.path = ?{} OR n.path LIKE ?{} ESCAPE '\\')",
            values.len() + 1,
            values.len() + 2
        ));
        values.push(Value::from(prefix.clone()));
        values.push(Value::from(sqlite_descendant_pattern(&prefix)));
    }

    for spec in PURPOSE_HEALTH_SPECS {
        let resolved_paths = resolved_purpose_paths(resolved_ids, spec.category);
        if !resolved_paths.is_empty() {
            clauses.push(format!(
                "NOT (p.status = '{}' AND n.path IN ({}))",
                spec.status,
                numbered_placeholders(values.len() + 1, resolved_paths.len())
            ));
            values.extend(resolved_paths.into_iter().map(Value::from));
        }
    }

    (clauses.join(" AND "), values)
}

/// Build the shared SQL filter for purpose lifecycle health findings.
fn purpose_status_where_clause(
    spec: PurposeHealthSpec,
    path_prefix: Option<&str>,
    resolved_ids: &[String],
    scope: HealthScope,
) -> (String, Vec<Value>) {
    let mut clauses = vec!["n.exists_now = 1".to_string(), "p.status = ?1".to_string()];
    let mut values = vec![Value::from(spec.status.to_string())];

    if source_filter_applies_before_queue(scope) {
        clauses.push(source_relevant_node_expression("n"));
    }
    if scope.high_impact_queue() {
        clauses.push(purpose_default_queue_node_expression("n", "p", scope));
    }

    let normalized_prefix = path_prefix
        .map(normalize_repo_path_prefix)
        .filter(|prefix| prefix != ".");
    if let Some(prefix) = normalized_prefix {
        clauses.push(format!(
            "(n.path = ?{} OR n.path LIKE ?{} ESCAPE '\\')",
            values.len() + 1,
            values.len() + 2
        ));
        values.push(Value::from(prefix.clone()));
        values.push(Value::from(sqlite_descendant_pattern(&prefix)));
    }

    let resolved_paths = resolved_purpose_paths(resolved_ids, spec.category);
    if !resolved_paths.is_empty() {
        clauses.push(format!(
            "n.path NOT IN ({})",
            numbered_placeholders(values.len() + 1, resolved_paths.len())
        ));
        values.extend(resolved_paths.into_iter().map(Value::from));
    }

    (clauses.join(" AND "), values)
}

/// Build a structural-health SQL filter over `findings` CTE columns.
fn structural_finding_where_clause(
    category: &str,
    path_prefix: Option<&str>,
    resolved_ids: &[String],
    scope: HealthScope,
    first_placeholder: usize,
) -> (String, Vec<Value>) {
    let mut placeholder = first_placeholder;
    let mut clauses = Vec::new();
    let mut values = Vec::new();

    if source_filter_applies_before_queue(scope) {
        clauses.push("source_relevant = 1".to_string());
    }
    if scope.high_impact_queue() {
        clauses.push(purpose_default_queue_finding_expression(scope));
    }

    let normalized_prefix = path_prefix
        .map(normalize_repo_path_prefix)
        .filter(|prefix| prefix != ".");
    if let Some(prefix) = normalized_prefix {
        clauses.push(format!(
            "((path = ?{path_exact} OR path LIKE ?{path_descendant} ESCAPE '\\') \
              OR (related_path = ?{related_exact} OR related_path LIKE ?{related_descendant} ESCAPE '\\'))",
            path_exact = placeholder,
            path_descendant = placeholder + 1,
            related_exact = placeholder + 2,
            related_descendant = placeholder + 3
        ));
        values.push(Value::from(prefix.clone()));
        values.push(Value::from(sqlite_descendant_pattern(&prefix)));
        values.push(Value::from(prefix.clone()));
        values.push(Value::from(sqlite_descendant_pattern(&prefix)));
        placeholder += 4;
    }

    let resolved_ids = resolved_ids_for_category(resolved_ids, category);
    if !resolved_ids.is_empty() {
        clauses.push(format!(
            "('{category}:' || path || ':' || related_path) NOT IN ({})",
            numbered_placeholders(placeholder, resolved_ids.len())
        ));
        values.extend(resolved_ids.into_iter().map(Value::from));
    }

    if clauses.is_empty() {
        (String::new(), values)
    } else {
        (format!("WHERE {}", clauses.join(" AND ")), values)
    }
}

/// SQL expression for approved purposes that need agent review at the requested scope.
fn purpose_review_candidate_expression(node_alias: &str, scope: HealthScope) -> String {
    let scope = match scope {
        HealthScope::All => HealthScope::PurposeStrict,
        other => other,
    };
    purpose_default_queue_node_expression(node_alias, "p", scope)
}

/// SQL expression for paths that belong in the default purpose queue.
fn purpose_default_queue_node_expression(
    node_alias: &str,
    purpose_alias: &str,
    scope: HealthScope,
) -> String {
    let asset_clause = if scope.include_assets() {
        format!(
            " OR ({node_alias}.kind = 'file' AND NOT ({}))",
            source_relevant_node_expression(node_alias)
        )
    } else {
        String::new()
    };
    let source_file_clause = if scope.include_source_files() {
        format!(" OR ({node_alias}.kind = 'file' AND COALESCE({node_alias}.language, '') <> '')")
    } else {
        String::new()
    };
    let all_file_clause = if scope.include_all_files() {
        format!(" OR {node_alias}.kind = 'file'")
    } else {
        String::new()
    };
    let stale_queue_sources = sql_string_literals(STALE_FILE_PURPOSE_QUEUE_SOURCE_VALUES);
    format!(
        "({node_alias}.kind = 'folder' \
          OR ({node_alias}.kind = 'file' \
              AND {purpose_alias}.status = 'stale' \
              AND {purpose_alias}.source IN ({stale_queue_sources})) \
          OR ({node_alias}.kind = 'file' AND {}){source_file_clause}{all_file_clause}{asset_clause})",
        high_impact_file_path_expression(&format!("lower({node_alias}.path)")),
    )
}

/// SQL expression for finding CTE columns that belong in the default purpose queue.
fn purpose_default_queue_finding_expression(scope: HealthScope) -> String {
    let asset_clause = if scope.include_assets() {
        " OR (kind = 'file' AND COALESCE(language, '') = '')"
    } else {
        ""
    };
    let source_file_clause = if scope.include_source_files() {
        " OR (kind = 'file' AND COALESCE(language, '') <> '')"
    } else {
        ""
    };
    let all_file_clause = if scope.include_all_files() {
        " OR kind = 'file'"
    } else {
        ""
    };
    format!(
        "(kind = 'folder' OR (kind = 'file' AND {}){source_file_clause}{all_file_clause}{asset_clause})",
        high_impact_file_path_expression("lower(path)")
    )
}

/// SQL ORDER BY expression that keeps folder-purpose work ahead of file cleanup.
fn purpose_default_queue_order_expression(node_alias: &str, purpose_alias: &str) -> String {
    let stale_queue_sources = sql_string_literals(STALE_FILE_PURPOSE_QUEUE_SOURCE_VALUES);
    format!(
        "CASE \
            WHEN {node_alias}.kind = 'folder' THEN 0 \
            WHEN {node_alias}.kind = 'file' \
                AND {purpose_alias}.status = 'stale' \
                AND ({purpose_alias}.source IN ({stale_queue_sources}) OR {}) THEN 1 \
            WHEN {node_alias}.kind = 'file' AND {} THEN 2 \
            ELSE 3 \
        END, {node_alias}.path",
        high_impact_file_path_expression(&format!("lower({node_alias}.path)")),
        high_impact_file_path_expression(&format!("lower({node_alias}.path)"))
    )
}

/// Purpose sources whose stale file purposes stay in the default queue regardless of path impact.
const STALE_FILE_PURPOSE_QUEUE_SOURCE_VALUES: &[&str] = &["human", "imported"];

/// Return whether `source_only` should run before queue-specific folder/file selection.
fn source_filter_applies_before_queue(scope: HealthScope) -> bool {
    scope.source_only_filter() && !scope.high_impact_queue()
}

/// Render trusted static strings as SQL string literals.
fn sql_string_literals(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| format!("'{}'", value.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// SQL expression mirroring the path-based high-impact file heuristic.
fn high_impact_file_path_expression(lower_path: &str) -> String {
    let name_matches = HIGH_IMPACT_FILE_NAMES
        .iter()
        .map(|name| format!("{lower_path} = '{name}' OR {lower_path} LIKE '%/{name}'"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let prefix_matches = HIGH_IMPACT_PATH_PREFIXES
        .iter()
        .map(|prefix| format!("{lower_path} LIKE '{prefix}%'"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let segment_matches = HIGH_IMPACT_PATH_SEGMENTS
        .iter()
        .map(|segment| format!("{lower_path} LIKE '%{segment}%'"))
        .collect::<Vec<_>>()
        .join(" OR ");
    format!("({name_matches} OR {prefix_matches} OR {segment_matches})")
}

/// Return a SQL expression that treats source files and folders with source descendants as source-relevant.
fn source_relevant_node_expression(alias: &str) -> String {
    format!(
        "(({alias}.kind = 'file' AND COALESCE({alias}.language, '') <> '') \
          OR ({alias}.kind = 'folder' AND EXISTS (\
              SELECT 1 FROM nodes source_child \
              WHERE source_child.exists_now = 1 \
                AND source_child.kind = 'file' \
                AND COALESCE(source_child.language, '') <> '' \
                AND (\
                    {alias}.path = '.' \
                    OR source_child.parent_path = {alias}.path \
                    OR substr(source_child.parent_path, 1, length({alias}.path) + 1) = {alias}.path || '/'\
                )\
          )))"
    )
}

/// Extract resolved primary paths for lifecycle categories without related paths.
fn resolved_purpose_paths(resolved_ids: &[String], category: &str) -> Vec<String> {
    let prefix = format!("{category}:");
    resolved_ids
        .iter()
        .filter_map(|id| {
            id.strip_prefix(&prefix)
                .and_then(|rest| rest.strip_suffix(':'))
                .filter(|path| !path.is_empty())
                .map(ToOwned::to_owned)
        })
        .collect()
}

/// Extract resolved full ids for categories that include related paths.
fn resolved_ids_for_category(resolved_ids: &[String], category: &str) -> Vec<String> {
    let prefix = format!("{category}:");
    resolved_ids
        .iter()
        .filter(|id| id.starts_with(&prefix))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectatlas_core::telemetry::{
        READ_AVOIDANCE_CONFIDENCE_MODELED, READ_AVOIDANCE_CONFIDENCE_NOT_RECORDED,
        READ_AVOIDANCE_CONFIDENCE_OBSERVED, TOKEN_ACCOUNTING_MODELED_AVOIDANCE,
        TOKEN_ACCURACY_HEURISTIC, TOKEN_BASELINE_SELECTED_CANDIDATES,
        TOKEN_BUCKET_FULL_FILE_COMPRESSION, TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
        TOKEN_COMMAND_SEARCH, TOKEN_CONFIDENCE_INFERRED, TOKEN_DEDUPE_SCOPE_EVENT,
        usage_from_estimates, usage_from_estimates_with_accounting, usage_from_text,
    };
    use projectatlas_core::{NodeKind, normalized_parent};
    use std::error::Error;
    use std::fmt::Debug;
    use std::fs;
    use std::io;

    #[test]
    fn stores_nodes_and_overview() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let node = Node {
            path: "src/main.rs".to_string(),
            kind: NodeKind::File,
            parent_path: normalized_parent("src/main.rs"),
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(12),
            mtime_ns: Some(10),
            content_hash: Some("abc".to_string()),
        };
        store.replace_scan(&[node])?;
        let overview = store.overview()?;
        require_eq(&overview.files, &1, "file count")?;
        require_eq(&overview.missing_purposes, &1, "missing purpose count")?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.purpose,
            &None,
            "purpose remains separate from summary",
        )?;
        require_eq(
            &nodes[0].summary,
            &Some("rust file, 12 bytes".to_string()),
            "node-level summary",
        )?;
        let loaded = store
            .load_node_by_path("src/main.rs")?
            .ok_or_else(|| io::Error::other("indexed node was not found by path"))?;
        require_eq(
            &loaded.node.path,
            &"src/main.rs".to_string(),
            "targeted path lookup",
        )?;
        require_eq(
            &store.load_node_by_path("src/missing.rs")?.is_none(),
            &true,
            "missing targeted path lookup",
        )?;
        Ok(())
    }

    #[test]
    fn records_token_overview() -> Result<(), Box<dyn Error>> {
        let store = AtlasStore::in_memory()?;
        let mut session_event = usage_from_estimates(
            "session",
            "outline",
            Some("src/main.rs".to_string()),
            None,
            100,
            20,
        );
        session_event.estimated_tokens_saved = Some(1);
        store.record_usage(&session_event)?;
        let mut unknown_event = usage_from_estimates("session", "unknown", None, None, 0, 0);
        unknown_event.estimated_tokens_without_projectatlas = None;
        unknown_event.estimated_tokens_with_projectatlas = None;
        unknown_event.estimated_tokens_saved = None;
        store.record_usage(&unknown_event)?;
        store.record_usage(&usage_from_estimates(
            "other-session",
            "outline",
            Some("src/lib.rs".to_string()),
            None,
            200,
            50,
        ))?;
        let overview = store.token_overview(Some("session"))?;
        require_eq(&overview.calls, &1, "usage call count")?;
        require_eq(&overview.estimated_saved, &80, "saved token count")?;
        require_eq(&overview.buckets.len(), &1, "usage bucket count")?;
        require_eq(
            &overview.buckets[0].accuracy,
            &TOKEN_ACCURACY_HEURISTIC.to_string(),
            "usage bucket accuracy",
        )?;
        let all_sessions = store.token_overview(None)?;
        require_eq(&all_sessions.calls, &2, "all-session usage call count")?;
        require_eq(
            &all_sessions.estimated_without_projectatlas,
            &300,
            "all-session baseline tokens",
        )?;
        require_eq(
            &all_sessions.estimated_with_projectatlas,
            &70,
            "all-session atlas tokens",
        )?;
        require_eq(
            &all_sessions.estimated_saved,
            &230,
            "all-session saved tokens",
        )?;
        require_eq(
            &all_sessions.likely_file_reads_avoided,
            &0,
            "non-search estimate events do not count as avoided file reads",
        )?;
        require_eq(
            &all_sessions.read_avoidance_confidence,
            &READ_AVOIDANCE_CONFIDENCE_NOT_RECORDED.to_string(),
            "non-search estimate read avoidance confidence",
        )?;

        store.record_usage(&usage_from_text(
            "bucketed",
            "summary",
            Some("src/main.rs".to_string()),
            None,
            "abcdefghijkl",
            "abcd",
        ))?;
        store.record_usage(&usage_from_estimates(
            "bucketed", "folders", None, None, 100, 20,
        ))?;
        let bucketed = store.token_overview(Some("bucketed"))?;
        require_eq(&bucketed.buckets.len(), &2, "bucketed overview count")?;
        require_eq(
            &bucketed.buckets[0].token_savings_bucket,
            &TOKEN_BUCKET_FULL_FILE_COMPRESSION.to_string(),
            "source compression bucket",
        )?;
        require_eq(
            &bucketed.buckets[1].token_savings_bucket,
            &TOKEN_BUCKET_NAVIGATION_AVOIDANCE.to_string(),
            "navigation bucket",
        )?;
        require_eq(
            &bucketed.observed_file_read_replacements,
            &1,
            "observed read replacement count",
        )?;
        require_eq(
            &bucketed.modeled_file_reads_avoided,
            &0,
            "folder navigation does not count as modeled file-read avoidance",
        )?;
        require_eq(
            &bucketed.likely_file_reads_avoided,
            &1,
            "observed-only likely file reads avoided",
        )?;
        require_eq(
            &bucketed.read_avoidance_confidence,
            &READ_AVOIDANCE_CONFIDENCE_OBSERVED.to_string(),
            "observed-only read avoidance confidence",
        )?;

        store.record_usage(&usage_from_text(
            "deduped",
            "summary",
            Some("src/lib.rs".to_string()),
            None,
            "abcdabcd",
            "ab",
        ))?;
        store.record_usage(&usage_from_estimates(
            "deduped",
            TOKEN_COMMAND_SEARCH,
            None,
            Some("token".to_string()),
            400,
            40,
        ))?;
        store.record_usage(&usage_from_estimates(
            "deduped",
            TOKEN_COMMAND_SEARCH,
            None,
            Some("token".to_string()),
            400,
            30,
        ))?;
        let deduped = store.token_overview(Some("deduped"))?;
        require_eq(
            &deduped.legacy_gross_estimated_saved,
            &731,
            "legacy gross saved tokens remains available",
        )?;
        require_eq(
            &deduped.measured_tokens_saved,
            &1,
            "measured saved tokens remain separate",
        )?;
        require_eq(
            &deduped.gross_modeled_tokens_avoided,
            &730,
            "gross modeled avoided tokens remains available",
        )?;
        require_eq(
            &deduped.deduped_modeled_tokens_avoided,
            &330,
            "modeled avoided tokens are deduped by baseline",
        )?;
        require_eq(
            &deduped.tokens_avoided,
            &331,
            "headline avoided tokens use measured plus deduped modeled",
        )?;
        require_eq(
            &deduped.observed_file_read_replacements,
            &1,
            "deduped observed read replacements",
        )?;
        require_eq(
            &deduped.modeled_file_reads_avoided,
            &2,
            "deduped raw search events remain likely file reads avoided",
        )?;
        require_eq(
            &deduped.likely_file_reads_avoided,
            &3,
            "deduped likely file reads avoided",
        )?;
        require_eq(
            &deduped.read_avoidance_confidence,
            &READ_AVOIDANCE_CONFIDENCE_MODELED.to_string(),
            "deduped read avoidance confidence",
        )?;

        store.record_usage(&usage_from_estimates_with_accounting(
            "event-scoped",
            "folders",
            None,
            Some("token".to_string()),
            400,
            40,
            TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
            TOKEN_BASELINE_SELECTED_CANDIDATES,
            TOKEN_CONFIDENCE_INFERRED,
            TOKEN_ACCOUNTING_MODELED_AVOIDANCE,
            TOKEN_BASELINE_SELECTED_CANDIDATES,
            TOKEN_DEDUPE_SCOPE_EVENT,
        ))?;
        store.record_usage(&usage_from_estimates_with_accounting(
            "event-scoped",
            "folders",
            None,
            Some("token".to_string()),
            400,
            30,
            TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
            TOKEN_BASELINE_SELECTED_CANDIDATES,
            TOKEN_CONFIDENCE_INFERRED,
            TOKEN_ACCOUNTING_MODELED_AVOIDANCE,
            TOKEN_BASELINE_SELECTED_CANDIDATES,
            TOKEN_DEDUPE_SCOPE_EVENT,
        ))?;
        let event_scoped = store.token_overview(Some("event-scoped"))?;
        require_eq(
            &event_scoped.gross_modeled_tokens_avoided,
            &730,
            "event-scoped gross modeled avoided tokens",
        )?;
        require_eq(
            &event_scoped.deduped_modeled_tokens_avoided,
            &730,
            "event-scoped modeled events are not collapsed",
        )?;
        require_eq(
            &event_scoped.repeated_baselines_deduped,
            &0,
            "event-scoped modeled events do not count as deduped repeats",
        )?;
        require_eq(
            &event_scoped.likely_file_reads_avoided,
            &0,
            "folder navigation events do not count as likely file reads avoided",
        )?;
        require_eq(
            &event_scoped.read_avoidance_confidence,
            &READ_AVOIDANCE_CONFIDENCE_NOT_RECORDED.to_string(),
            "folder navigation read avoidance confidence",
        )?;

        let mut negative_event = usage_from_estimates("negative", "outline", None, None, 20, 50);
        negative_event.estimated_tokens_saved = Some(999);
        store.record_usage(&negative_event)?;
        let negative = store.token_overview(Some("negative"))?;
        require_eq(&negative.calls, &1, "negative session call count")?;
        require_eq(
            &negative.estimated_saved,
            &-30,
            "negative session recomputed delta",
        )?;
        require_eq(
            &negative.savings_rate,
            &Some(-1.5),
            "negative session savings rate",
        )?;

        let mut zero_event = usage_from_estimates("zero-baseline", "outline", None, None, 0, 12);
        zero_event.estimated_tokens_saved = Some(999);
        store.record_usage(&zero_event)?;
        let zero_baseline = store.token_overview(Some("zero-baseline"))?;
        require_eq(&zero_baseline.calls, &1, "zero baseline call count")?;
        require_eq(
            &zero_baseline.estimated_saved,
            &-12,
            "zero baseline recomputed delta",
        )?;
        require_eq(
            &zero_baseline.savings_rate,
            &None,
            "zero baseline savings rate",
        )?;

        for index in 0..3 {
            store.connection.execute(
                "
                INSERT INTO usage_events(
                    session_id,
                    command,
                    path,
                    query,
                    estimated_tokens_without_projectatlas,
                    estimated_tokens_with_projectatlas,
                    estimated_tokens_saved
                )
                VALUES(?1, 'large', NULL, NULL, ?2, 0, ?2)
                ",
                params![format!("large-{index}"), i64::MAX,],
            )?;
        }
        let large = store.token_overview(None)?;
        require_eq(
            &large.estimated_saved,
            &isize::MAX,
            "large aggregate saturates without sqlite SUM overflow",
        )?;
        Ok(())
    }

    #[test]
    fn token_trends_group_usage_by_period_and_bucket() -> Result<(), Box<dyn Error>> {
        let store = AtlasStore::in_memory()?;
        for (session, created_at, bucket, baseline_kind, confidence, without, with) in [
            (
                "session",
                "2026-06-01 00:00:00",
                TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
                "selected_candidates",
                "inferred",
                100_i64,
                25_i64,
            ),
            (
                "session",
                "2026-06-10 00:00:00",
                TOKEN_BUCKET_FULL_FILE_COMPRESSION,
                "full_file",
                "observed",
                50_i64,
                10_i64,
            ),
            (
                "session",
                "2026-07-01 00:00:00",
                TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
                "selected_candidates",
                "inferred",
                80_i64,
                20_i64,
            ),
            (
                "other",
                "2026-06-03 00:00:00",
                TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
                "selected_candidates",
                "inferred",
                999_i64,
                1_i64,
            ),
        ] {
            store.connection.execute(
                "
                INSERT INTO usage_events(
                    session_id,
                    command,
                    estimated_tokens_without_projectatlas,
                    estimated_tokens_with_projectatlas,
                    estimated_tokens_saved,
                    token_savings_bucket,
                    baseline_kind,
                    confidence,
                    created_at
                )
                VALUES(?1, 'trend', ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ",
                params![
                    session,
                    without,
                    with,
                    without - with,
                    bucket,
                    baseline_kind,
                    confidence,
                    created_at
                ],
            )?;
        }

        let trends = store.token_trends(Some("session"), TokenTrendWindow::Month)?;
        require_eq(&trends.periods.len(), &2, "monthly periods")?;
        require_eq(
            &trends.periods[0].period,
            &"2026-06".to_string(),
            "first month",
        )?;
        require_eq(&trends.periods[0].calls, &2, "june call count")?;
        require_eq(
            &trends.periods[0].estimated_saved,
            &115,
            "june saved tokens",
        )?;
        require_eq(
            &trends.periods[0].buckets.len(),
            &2,
            "june preserves evidence buckets",
        )?;
        require_eq(
            &trends.periods[0].buckets[0].token_savings_bucket,
            &TOKEN_BUCKET_FULL_FILE_COMPRESSION.to_string(),
            "full-file bucket remains visible",
        )?;
        require_eq(
            &trends.periods[0].buckets[0].confidence,
            &"observed".to_string(),
            "bucket confidence remains visible",
        )?;
        require_eq(
            &trends.periods[1].period,
            &"2026-07".to_string(),
            "second month",
        )?;
        require_eq(&trends.periods[1].calls, &1, "july call count")?;
        Ok(())
    }

    #[test]
    fn token_trends_backfill_created_at_for_upgraded_databases() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let db_path = temp.path().join("legacy.db");
        {
            let connection = Connection::open(&db_path)?;
            connection.execute_batch(
                "
                CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
                INSERT INTO metadata(key, value) VALUES('schema_version', '7');
                CREATE TABLE usage_events (
                    id INTEGER PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    command TEXT NOT NULL,
                    path TEXT,
                    query TEXT,
                    estimated_tokens_without_projectatlas INTEGER,
                    estimated_tokens_with_projectatlas INTEGER,
                    estimated_tokens_saved INTEGER,
                    token_savings_bucket TEXT NOT NULL DEFAULT 'navigation_avoidance',
                    provider TEXT NOT NULL DEFAULT 'heuristic',
                    model TEXT NOT NULL DEFAULT 'unknown',
                    tokenizer_backend TEXT NOT NULL DEFAULT 'chars_div_4',
                    accuracy TEXT NOT NULL DEFAULT 'heuristic_estimate',
                    baseline_kind TEXT NOT NULL DEFAULT 'selected_candidates',
                    confidence TEXT NOT NULL DEFAULT 'inferred',
                    calculation_trace TEXT NOT NULL DEFAULT 'heuristic=ceil(chars_or_bytes/4)'
                );
                INSERT INTO usage_events(
                    session_id,
                    command,
                    estimated_tokens_without_projectatlas,
                    estimated_tokens_with_projectatlas,
                    estimated_tokens_saved
                )
                VALUES('legacy-session', 'legacy', 100, 20, 80);
                ",
            )?;
        }

        let store = AtlasStore::open(&db_path)?;
        let null_created_at = store.connection.query_row(
            "SELECT COUNT(*) FROM usage_events WHERE created_at IS NULL OR created_at = ''",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        require_eq(&null_created_at, &0, "legacy created_at values backfilled")?;
        store.record_usage(&usage_from_estimates(
            "legacy-session",
            "new-call",
            None,
            None,
            50,
            10,
        ))?;
        let null_created_at = store.connection.query_row(
            "SELECT COUNT(*) FROM usage_events WHERE created_at IS NULL OR created_at = ''",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        require_eq(&null_created_at, &0, "new created_at values populated")?;
        let trends = store.token_trends(Some("legacy-session"), TokenTrendWindow::Month)?;
        require_eq(&trends.periods.is_empty(), &false, "trend periods exist")?;
        require_eq(
            &trends
                .periods
                .iter()
                .any(|period| period.period.starts_with("1970")),
            &false,
            "upgraded telemetry does not aggregate under 1970",
        )?;
        Ok(())
    }

    #[test]
    fn stores_project_root_in_metadata() -> Result<(), Box<dyn Error>> {
        let store = AtlasStore::in_memory()?;
        store.set_project_root(Path::new("C:/workspace/example"))?;
        require_eq(
            &store.project_root()?,
            &Some("C:/workspace/example".to_string()),
            "project root metadata",
        )?;
        store.set_project_root(Path::new(r"\\?\C:\workspace\example"))?;
        require_eq(
            &store.project_root()?,
            &Some("C:/workspace/example".to_string()),
            "windows extended project root metadata",
        )?;
        store.set_project_root(Path::new(r"\\?\UNC\server\share\repo"))?;
        require_eq(
            &store.project_root()?,
            &Some("//server/share/repo".to_string()),
            "windows unc project root metadata",
        )?;
        Ok(())
    }

    #[test]
    fn project_root_metadata_resolves_existing_filesystem_aliases() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        let child = root.join("child");
        fs::create_dir_all(&child)?;
        let alias = child.join("..");
        let canonical = normalize_native_path_display(fs::canonicalize(&root)?);
        let store = AtlasStore::in_memory()?;

        store.set_project_root(&alias)?;
        require_eq(
            &store.project_root()?,
            &Some(canonical.clone()),
            "existing project root alias",
        )?;

        store.connection.execute(
            "UPDATE metadata SET value = ?1 WHERE key = 'project_root'",
            [normalize_native_path_display(&alias)],
        )?;
        require_eq(
            &store.project_root()?,
            &Some(canonical),
            "legacy lexical project root alias",
        )?;
        Ok(())
    }

    #[test]
    fn project_root_metadata_rejects_non_missing_resolution_failures() {
        let invalid_root = Path::new("invalid\0root");
        let result = normalize_metadata_path(invalid_root);

        assert!(matches!(
            result,
            Err(DbError::ProjectRootResolution { path, source })
                if path == invalid_root && source.kind() == io::ErrorKind::InvalidInput
        ));
    }

    #[test]
    fn read_project_root_read_only_does_not_create_wal_sidecars() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo with spaces");
        let atlas_dir = root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let db_path = atlas_dir.join("projectatlas.db");
        {
            let store = AtlasStore::open(&db_path)?;
            store.set_project_root(&root)?;
            store
                .connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        }
        let wal_path = db_path.with_extension("db-wal");
        let shm_path = db_path.with_extension("db-shm");
        for sidecar_path in [&wal_path, &shm_path] {
            match fs::remove_file(sidecar_path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        let db_len_before = fs::metadata(&db_path)?.len();

        require_eq(
            &read_project_root_read_only(&db_path)?,
            &Some(normalize_native_path_display(fs::canonicalize(&root)?)),
            "immutable read-only project root",
        )?;
        require_eq(
            &fs::metadata(&db_path)?.len(),
            &db_len_before,
            "immutable read-only DB length",
        )?;
        require_eq(&wal_path.exists(), &false, "read-only WAL sidecar")?;
        require_eq(&shm_path.exists(), &false, "read-only SHM sidecar")?;
        Ok(())
    }

    #[test]
    fn partial_scan_updates_and_absents_paths() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.upsert_scan_nodes(&[test_file_node("src/a.rs", "hash-a2")])?;
        let updated = store
            .load_node_by_path("src/a.rs")?
            .ok_or_else(|| io::Error::other("updated node missing"))?;
        require_eq(
            &updated.node.content_hash,
            &Some("hash-a2".to_string()),
            "partial content hash",
        )?;
        require_eq(
            &store.load_node_by_path("src/b.rs")?.is_some(),
            &true,
            "unrelated node remains indexed",
        )?;
        store.mark_paths_absent(&["src/b.rs".to_string()])?;
        require_eq(
            &store.load_node_by_path("src/b.rs")?.is_none(),
            &true,
            "absent path is no longer returned",
        )?;
        Ok(())
    }

    #[test]
    fn approved_purpose_becomes_stale_when_file_hash_changes() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash-a")])?;
        store.set_purpose(
            "src/main.rs",
            "Application entry point",
            PurposeSource::Agent,
        )?;
        store.upsert_scan_nodes(&[test_file_node("src/main.rs", "hash-b")])?;

        let node = store
            .load_node_by_path("src/main.rs")?
            .ok_or_else(|| io::Error::other("stale node missing"))?;
        require_eq(
            &node.purpose.status,
            &PurposeStatus::Stale,
            "changed approved file purpose status",
        )?;
        Ok(())
    }

    #[test]
    fn ranked_nodes_are_loaded_bounded_from_sql() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let mut gradle_task_node = test_file_node("build.gradle.kts", "hash-gradle");
        gradle_task_node.extension = Some(".kts".to_string());
        gradle_task_node.language = Some("kotlin".to_string());
        store.replace_scan(&[
            test_folder_node("src/auth"),
            test_folder_node("src/ui"),
            test_file_node("src/auth/login.rs", "hash-login"),
            test_file_node("src/ui/button.rs", "hash-button"),
            gradle_task_node,
        ])?;
        store.set_purpose(
            "src/auth",
            "Authentication workflow folder",
            PurposeSource::Agent,
        )?;
        store.set_purpose("src/ui", "User interface folder", PurposeSource::Agent)?;
        store.set_node_summary("src/auth/login.rs", "rust source defining login flow")?;

        let folders = store.load_ranked_nodes("authentication", NodeKind::Folder, None, 1, 0)?;
        require_eq(&folders.len(), &1, "bounded folder ranking")?;
        require_eq(
            &folders[0].node.path,
            &"src/auth".to_string(),
            "semantic folder ranking",
        )?;

        let files = store.load_ranked_nodes("login", NodeKind::File, Some("src/auth"), 10, 0)?;
        require_eq(&files.len(), &1, "folder-constrained file ranking")?;
        require_eq(
            &files[0].node.path,
            &"src/auth/login.rs".to_string(),
            "ranked file path",
        )?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "build.gradle.kts".to_string(),
            language: Some("kotlin".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![CodeSymbol {
                path: "build.gradle.kts".to_string(),
                language: Some("kotlin".to_string()),
                name: "bootRunE2E".to_string(),
                kind: SymbolKind::Function,
                signature: "tasks.register<BootRun>(\"bootRunE2E\")".to_string(),
                exported: false,
                documentation: None,
                line_start: 1,
                line_end: 1,
                parent: None,
                parser: ParserKind::TreeSitter,
                detail: Some("gradle-kotlin-dsl-task".to_string()),
            }],
            relations: Vec::new(),
        })?;
        let gradle_files = store.load_ranked_nodes("bootRunE2E", NodeKind::File, None, 10, 0)?;
        require_eq(&gradle_files.len(), &1, "symbol-ranked file count")?;
        require_eq(
            &gradle_files[0].node.path,
            &"build.gradle.kts".to_string(),
            "symbol-ranked file path",
        )?;
        store.clear_symbol_graph_for_path("build.gradle.kts")?;
        let cleared_gradle_files =
            store.load_ranked_nodes("bootRunE2E", NodeKind::File, None, 10, 0)?;
        require_eq(
            &cleared_gradle_files.len(),
            &0,
            "cleared symbol-ranked file count",
        )?;
        Ok(())
    }

    #[test]
    fn folder_like_filters_treat_wildcards_as_literal_path_text() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("src/a%b"),
            test_folder_node("src/axb"),
            test_folder_node("src/a_b"),
            test_folder_node("src/acb"),
            test_file_node("src/a%b/target.rs", "hash-percent-target"),
            test_file_node("src/axb/false.rs", "hash-percent-false"),
            test_file_node("src/a_b/target.rs", "hash-underscore-target"),
            test_file_node("src/acb/false.rs", "hash-underscore-false"),
        ])?;
        for path in [
            "src/a%b/target.rs",
            "src/axb/false.rs",
            "src/a_b/target.rs",
            "src/acb/false.rs",
        ] {
            store.set_node_summary(path, "needle indexed summary")?;
        }

        let percent_files =
            store.load_ranked_nodes("needle", NodeKind::File, Some("src/a%b"), 10, 0)?;
        require_eq(&percent_files.len(), &1, "percent folder ranked count")?;
        require_eq(
            &percent_files[0].node.path,
            &"src/a%b/target.rs".to_string(),
            "percent folder ranked path",
        )?;
        require_eq(
            &store.source_file_byte_count(Some("src/a%b"))?,
            &12,
            "percent folder byte count",
        )?;

        let mut visited = Vec::new();
        store.visit_file_token_estimates(Some("src/a_b"), |path, _size| {
            visited.push(path);
            Ok(true)
        })?;
        require_eq(
            &visited,
            &vec!["src/a_b/target.rs".to_string()],
            "underscore folder token paths",
        )?;

        store.mark_paths_absent(&["src/a%b".to_string(), "src/a_b".to_string()])?;
        require_eq(
            &store.load_node_by_path("src/axb/false.rs")?.is_some(),
            &true,
            "percent-like sibling remains indexed",
        )?;
        require_eq(
            &store.load_node_by_path("src/acb/false.rs")?.is_some(),
            &true,
            "underscore-like sibling remains indexed",
        )?;
        require_eq(
            &store.load_node_by_path("src/a%b/target.rs")?.is_none(),
            &true,
            "percent folder target removed",
        )?;
        require_eq(
            &store.load_node_by_path("src/a_b/target.rs")?.is_none(),
            &true,
            "underscore folder target removed",
        )?;
        Ok(())
    }

    #[test]
    fn sql_health_findings_match_resolution_ids() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.set_purpose("src/a.rs", "Shared purpose", PurposeSource::Agent)?;
        store.set_purpose("src/b.rs", "Shared purpose", PurposeSource::Agent)?;

        let findings = store.unresolved_health_findings(&[])?;
        let duplicate = findings
            .iter()
            .find(|finding| finding.category == "duplicate-purpose")
            .ok_or_else(|| io::Error::other("duplicate-purpose finding missing"))?;
        store.resolve_health_finding(&HealthResolution {
            finding_id: duplicate.id.clone(),
            category: duplicate.category.clone(),
            path: duplicate.path.clone(),
            related_path: duplicate.related_path.clone(),
            rationale: "Intentional mirror for test.".to_string(),
        })?;
        let remaining = store.unresolved_health_findings(&store.resolved_health_ids()?)?;
        require_eq(&remaining.is_empty(), &true, "resolved SQL health finding")?;
        Ok(())
    }

    #[test]
    fn unresolved_health_findings_page_filters_and_bounds_rows() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("docs/a.rs", "hash-doc"),
        ])?;
        let query = HealthQuery {
            start_index: 1,
            limit: 1,
            category: Some("missing-purpose".to_string()),
            severity: Some(Severity::Warning),
            path_prefix: Some("src".to_string()),
            summary_only: false,
            scope: HealthScope::all(),
        };

        let page = store.unresolved_health_findings_page(&[], &query)?;
        require_eq(&page.unfiltered_total, &4, "unfiltered health total")?;
        require_eq(&page.total, &2, "filtered health total")?;
        require_eq(&page.returned, &1, "returned health rows")?;
        require_eq(
            &page.findings[0].path,
            &"src/a.rs".to_string(),
            "paged path",
        )?;

        let summary_page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                summary_only: true,
                ..query
            },
        )?;
        require_eq(&summary_page.total, &2, "summary-only total")?;
        require_eq(
            &summary_page.findings.is_empty(),
            &true,
            "summary-only rows",
        )?;
        Ok(())
    }

    #[test]
    fn unresolved_health_findings_page_skips_resolved_lifecycle_rows_before_paging()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
            test_file_node("src/c.rs", "hash-c"),
            test_file_node("src/d.rs", "hash-d"),
        ])?;
        store.resolve_health_finding(&HealthResolution {
            finding_id: finding_id("missing-purpose", "src/b.rs", None),
            category: "missing-purpose".to_string(),
            path: "src/b.rs".to_string(),
            related_path: None,
            rationale: "Resolved for pagination regression.".to_string(),
        })?;

        let page = store.unresolved_health_findings_page(
            &store.resolved_health_ids()?,
            &HealthQuery {
                start_index: 0,
                limit: 2,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some("src".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;

        require_eq(&page.total, &3, "filtered unresolved missing total")?;
        require_eq(&page.returned, &2, "returned unresolved missing rows")?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec!["src/a.rs", "src/c.rs"],
            "resolved row skipped before limit",
        )?;
        Ok(())
    }

    #[test]
    fn unresolved_health_findings_page_streams_duplicate_and_temp_rows()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("tmp"),
            test_folder_node("src"),
            test_folder_node("src/tmp"),
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.set_purpose(".", "Repository root", PurposeSource::Agent)?;
        store.set_purpose("src", "Source folder", PurposeSource::Agent)?;
        store.set_purpose("tmp", "Temporary output", PurposeSource::Agent)?;
        store.set_purpose("src/tmp", "Source temporary output", PurposeSource::Agent)?;
        store.set_purpose("src/a.rs", "Shared implementation", PurposeSource::Agent)?;
        store.set_purpose("src/b.rs", "Shared implementation", PurposeSource::Agent)?;

        let duplicate_page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 1,
                category: Some("duplicate-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some("src".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        require_eq(&duplicate_page.total, &1, "duplicate total")?;
        require_eq(&duplicate_page.returned, &1, "duplicate returned")?;
        require_eq(
            &duplicate_page.findings[0].category,
            &"duplicate-purpose".to_string(),
            "duplicate category",
        )?;

        let temp_page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 1,
                category: Some("repeated-temporary-folder".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        require_eq(&temp_page.total, &1, "temp total")?;
        require_eq(&temp_page.returned, &1, "temp returned")?;
        require_eq(
            &temp_page.findings[0].category,
            &"repeated-temporary-folder".to_string(),
            "temp category",
        )?;
        Ok(())
    }

    #[test]
    fn unresolved_health_findings_page_source_only_filters_asset_noise()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let asset_file = Node {
            path: "assets/logo.png".to_string(),
            kind: NodeKind::File,
            parent_path: Some("assets".to_string()),
            extension: Some(".png".to_string()),
            language: None,
            size_bytes: Some(42),
            mtime_ns: Some(10),
            content_hash: Some("hash-logo".to_string()),
        };
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/main.rs", "hash-main"),
            test_folder_node("assets"),
            asset_file,
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::source_only(),
            },
        )?;

        require_eq(&page.unfiltered_total, &5, "all unresolved rows")?;
        require_eq(&page.total, &3, "source-only missing total")?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec![".", "src", "src/main.rs"],
            "source-only paths",
        )?;
        Ok(())
    }

    #[test]
    fn high_impact_purpose_queue_filters_low_priority_files() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/main.rs", "hash-main"),
            test_file_node("src/helper.rs", "hash-helper"),
            test_file_node("build.gradle.kts", "hash-gradle"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(&page.unfiltered_total, &5, "all missing rows")?;
        require_eq(&page.total, &4, "default actionable rows")?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec![".", "src", "build.gradle.kts", "src/main.rs"],
            "folder-first high-impact queue paths",
        )?;

        let broad_page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        require_eq(&broad_page.total, &5, "explicit broad queue rows")?;
        Ok(())
    }

    #[test]
    fn high_impact_purpose_queue_keeps_asset_only_folders_without_asset_files()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let asset_file = Node {
            path: "assets/logo.svg".to_string(),
            kind: NodeKind::File,
            parent_path: Some("assets".to_string()),
            extension: Some(".svg".to_string()),
            language: None,
            size_bytes: Some(42),
            mtime_ns: Some(10),
            content_hash: Some("hash-logo".to_string()),
        };
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("assets"),
            test_folder_node("src"),
            asset_file,
            test_file_node("src/helper.rs", "hash-helper"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(&page.unfiltered_total, &5, "all missing rows")?;
        require_eq(&page.total, &3, "all folders without asset files")?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec![".", "assets", "src"],
            "folder-first default queue keeps asset-only folders",
        )?;
        Ok(())
    }

    #[test]
    fn high_impact_purpose_queue_pages_folders_before_files() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("Cargo.toml", "hash-cargo"),
            test_file_node("package.json", "hash-package"),
            test_file_node("pyproject.toml", "hash-python"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 2,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(&page.total, &5, "default actionable total")?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec![".", "src"],
            "small page keeps folders first",
        )?;
        Ok(())
    }

    #[test]
    fn high_impact_purpose_queue_omits_low_value_stale_files() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/helper.rs", "hash-a"),
            test_file_node("Cargo.toml", "hash-cargo"),
            test_file_node("package.json", "hash-package"),
        ])?;
        store.set_purpose(
            "src/helper.rs",
            "Reviewed helper implementation.",
            PurposeSource::Agent,
        )?;
        store.set_purpose(
            "Cargo.toml",
            "Reviewed Cargo manifest.",
            PurposeSource::Agent,
        )?;
        store.replace_scan(&[
            test_file_node("src/helper.rs", "hash-b"),
            test_file_node("Cargo.toml", "hash-cargo-new"),
            test_file_node("package.json", "hash-package"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 1,
                category: None,
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(&page.total, &2, "default actionable total")?;
        require_eq(&page.returned, &1, "small page returned")?;
        require_eq(
            &page.findings[0].category,
            &"stale-purpose".to_string(),
            "stale high-impact file is still prioritized",
        )?;
        require_eq(
            &page.findings[0].path,
            &"Cargo.toml".to_string(),
            "stale high-impact file path",
        )?;

        let broad_page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some(CATEGORY_STALE_PURPOSE.to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_with_source_files(),
            },
        )?;
        require_eq(&broad_page.total, &2, "broad source stale rows")?;
        require_eq(
            &health_paths(&broad_page),
            &vec!["Cargo.toml", "src/helper.rs"],
            "broad scope includes low-value stale source files",
        )?;
        Ok(())
    }

    #[test]
    fn include_assets_queue_includes_asset_files_not_low_priority_source()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let asset_file = Node {
            path: "assets/logo.svg".to_string(),
            kind: NodeKind::File,
            parent_path: Some("assets".to_string()),
            extension: Some(".svg".to_string()),
            language: None,
            size_bytes: Some(42),
            mtime_ns: Some(10),
            content_hash: Some("hash-logo".to_string()),
        };
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("assets"),
            test_folder_node("src"),
            asset_file,
            test_file_node("src/helper.rs", "hash-helper"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_with_assets(),
            },
        )?;

        require_eq(&page.unfiltered_total, &5, "all missing rows")?;
        require_eq(
            &page.total,
            &4,
            "assets included without broad source cleanup",
        )?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec![".", "assets", "src", "assets/logo.svg"],
            "asset files included and low-priority source omitted",
        )?;
        Ok(())
    }

    #[test]
    fn legacy_human_stale_files_remain_in_default_queue() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/helper.rs", "hash-a")])?;
        store.set_purpose(
            "src/helper.rs",
            "Legacy reviewed helper implementation.",
            PurposeSource::Agent,
        )?;
        store.connection.execute(
            "
            UPDATE purposes
            SET source = 'human'
            WHERE node_id = (SELECT id FROM nodes WHERE path = 'src/helper.rs')
            ",
            [],
        )?;
        store.replace_scan(&[test_file_node("src/helper.rs", "hash-b")])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("stale-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(&page.total, &1, "legacy reviewed stale row total")?;
        require_eq(
            &page.findings[0].path,
            &"src/helper.rs".to_string(),
            "legacy reviewed stale file",
        )?;
        Ok(())
    }

    #[test]
    fn stale_imported_files_remain_in_default_queue() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/imported.rs", "hash-a"),
            test_file_node("Cargo.toml", "hash-cargo"),
        ])?;
        store.set_purpose(
            "src/imported.rs",
            "Imported helper implementation.",
            PurposeSource::Imported,
        )?;
        store.replace_scan(&[
            test_file_node("src/imported.rs", "hash-b"),
            test_file_node("Cargo.toml", "hash-cargo"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: None,
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(
            &page.total,
            &2,
            "default queue includes stale imported file",
        )?;
        require_eq(
            &health_paths(&page),
            &vec!["src/imported.rs", "Cargo.toml"],
            "stale imported file is queued before high-impact files",
        )?;
        require_eq(
            &page.findings[0].category,
            &"stale-purpose".to_string(),
            "stale imported finding category",
        )?;
        Ok(())
    }

    #[test]
    fn duplicate_purpose_health_is_contextual_for_folders() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("customers"),
            test_folder_node("customers/service"),
            test_folder_node("settings"),
            test_folder_node("settings/service"),
        ])?;
        store.set_purpose("customers/service", "Service layer", PurposeSource::Agent)?;
        store.set_purpose("settings/service", "Service layer", PurposeSource::Agent)?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("duplicate-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;

        require_eq(&page.total, &0, "folder duplicates scoped by parent")?;
        Ok(())
    }

    #[test]
    fn contextual_folder_duplicate_identity_matches_unpaged_health_and_resolution()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("customers"),
            test_folder_node("customers/service"),
            test_folder_node("settings"),
            test_folder_node("settings/service"),
            test_folder_node("settings/worker"),
        ])?;
        store.set_purpose("customers/service", "Service layer", PurposeSource::Agent)?;
        store.set_purpose("settings/service", "Service layer", PurposeSource::Agent)?;
        store.set_purpose("settings/worker", "Service layer", PurposeSource::Agent)?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("duplicate-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        require_eq(&page.total, &1, "contextual duplicate total")?;
        let paged_finding = page
            .findings
            .first()
            .ok_or_else(|| io::Error::other("paged duplicate missing"))?;
        require_eq(
            &paged_finding.path,
            &"settings/worker".to_string(),
            "paged duplicate path",
        )?;
        require_eq(
            &paged_finding.related_path,
            &Some("settings/service".to_string()),
            "paged related path",
        )?;

        let unpaged_duplicates = store
            .unresolved_health_findings(&[])?
            .into_iter()
            .filter(|finding| finding.category == "duplicate-purpose")
            .collect::<Vec<_>>();
        require_eq(
            &unpaged_duplicates,
            &page.findings,
            "unpaged duplicate identity",
        )?;

        store.resolve_health_finding(&HealthResolution {
            finding_id: paged_finding.id.clone(),
            category: paged_finding.category.clone(),
            path: paged_finding.path.clone(),
            related_path: paged_finding.related_path.clone(),
            rationale: "Settings service and worker intentionally share a layer purpose."
                .to_string(),
        })?;
        let has_remaining_duplicate = store
            .unresolved_health_findings(&store.resolved_health_ids()?)?
            .into_iter()
            .any(|finding| finding.category == "duplicate-purpose");
        require_eq(
            &has_remaining_duplicate,
            &false,
            "resolved contextual duplicate",
        )?;
        Ok(())
    }

    #[test]
    fn agent_review_required_scope_expands_from_low_to_strict() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let asset_file = Node {
            path: "assets/logo.svg".to_string(),
            kind: NodeKind::File,
            parent_path: Some("assets".to_string()),
            extension: Some(".svg".to_string()),
            language: None,
            size_bytes: Some(42),
            mtime_ns: Some(10),
            content_hash: Some("hash-logo".to_string()),
        };
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("assets"),
            test_folder_node("src"),
            test_file_node("Cargo.toml", "hash-cargo"),
            test_file_node("src/detail.rs", "hash-detail"),
            asset_file,
        ])?;
        for (path, purpose) in [
            (".", "Imported repository root"),
            ("assets", "Imported asset folder"),
            ("src", "Imported Rust source folder"),
            ("Cargo.toml", "Imported Rust manifest"),
            ("src/detail.rs", "Imported implementation detail"),
            ("assets/logo.svg", "Imported SVG brand asset"),
        ] {
            store.set_purpose(path, purpose, PurposeSource::Imported)?;
        }

        let low = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 20,
                category: Some(CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED.to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;
        require_eq(
            &health_paths(&low),
            &vec![".", "assets", "src", "Cargo.toml"],
            "low purpose review scope",
        )?;
        require_eq(
            &low.unfiltered_total,
            &6,
            "agent-review findings are counted once in unfiltered total",
        )?;

        let asset_scope = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                scope: HealthScope::purpose_with_assets(),
                ..low_query()
            },
        )?;
        require_eq(
            &health_paths(&asset_scope),
            &vec![".", "assets", "src", "Cargo.toml", "assets/logo.svg"],
            "asset purpose review scope",
        )?;

        let medium = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                scope: HealthScope::purpose_with_source_files(),
                ..low_query()
            },
        )?;
        require_eq(
            &health_paths(&medium),
            &vec![".", "assets", "src", "Cargo.toml", "src/detail.rs"],
            "medium purpose review scope",
        )?;

        let strict = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                scope: HealthScope::purpose_strict(),
                ..low_query()
            },
        )?;
        require_eq(
            &health_paths(&strict),
            &vec![
                ".",
                "assets",
                "src",
                "Cargo.toml",
                "assets/logo.svg",
                "src/detail.rs",
            ],
            "strict purpose review scope",
        )?;

        let all = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                scope: HealthScope::all(),
                ..low_query()
            },
        )?;
        require_eq(
            &health_paths(&all),
            &health_paths(&strict),
            "all health scope should include every purpose review candidate",
        )?;
        Ok(())
    }

    #[test]
    fn replace_scan_preserves_curated_purposes_and_reconciles_changed_paths()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/main.rs", "hash-a"),
        ])?;
        store.set_purpose(".", "Agent-reviewed repository root", PurposeSource::Agent)?;
        store.set_purpose(
            "src",
            "Agent-reviewed Rust source folder",
            PurposeSource::Agent,
        )?;
        store.set_purpose(
            "src/main.rs",
            "Agent-reviewed Rust entry point",
            PurposeSource::Agent,
        )?;

        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/main.rs", "hash-a"),
            test_file_node("src/new.rs", "hash-new"),
        ])?;
        let nodes = store.load_nodes_by_paths(&[
            ".".to_string(),
            "src".to_string(),
            "src/main.rs".to_string(),
            "src/new.rs".to_string(),
        ])?;
        let by_path = nodes
            .iter()
            .map(|node| (node.node.path.as_str(), node))
            .collect::<HashMap<_, _>>();
        require_eq(
            &by_path["src/main.rs"].purpose.purpose,
            &Some("Agent-reviewed Rust entry point".to_string()),
            "unchanged file purpose preserved",
        )?;
        require_eq(
            &by_path["src/main.rs"].purpose.status,
            &PurposeStatus::Approved,
            "unchanged file purpose stays approved",
        )?;
        require_eq(
            &by_path["src/new.rs"].purpose.status,
            &PurposeStatus::Missing,
            "new file starts missing",
        )?;

        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/main.rs", "hash-b"),
            test_file_node("src/new.rs", "hash-new"),
        ])?;
        let changed = store
            .load_nodes_by_paths(&["src/main.rs".to_string()])?
            .pop()
            .ok_or_else(|| io::Error::other("changed node missing"))?;
        require_eq(
            &changed.purpose.purpose,
            &Some("Agent-reviewed Rust entry point".to_string()),
            "changed file purpose text preserved",
        )?;
        require_eq(
            &changed.purpose.status,
            &PurposeStatus::Stale,
            "changed file purpose becomes stale",
        )?;

        store.replace_scan(&[test_folder_node("."), test_folder_node("src")])?;
        let removed = store.load_nodes_by_paths(&["src/main.rs".to_string()])?;
        require_eq(&removed.is_empty(), &true, "removed file is inactive")?;
        Ok(())
    }

    #[test]
    fn file_token_estimates_are_visited_without_loading_nodes() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("tests/b.rs", "hash-b"),
        ])?;

        let mut visited = Vec::new();
        store.visit_file_token_estimates(Some("src"), |path, size_bytes| {
            visited.push((path, size_bytes));
            Ok(true)
        })?;
        require_eq(
            &visited,
            &vec![("src/a.rs".to_string(), Some(12))],
            "folder-scoped token estimate rows",
        )?;
        Ok(())
    }

    #[test]
    fn indexed_file_text_replaces_and_clears_stale_rows() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash-a")])?;
        store.replace_file_texts_for_paths(
            &["src/main.rs".to_string()],
            &[IndexedFileText {
                path: "src/main.rs".to_string(),
                content_hash: Some("hash-a".to_string()),
                byte_count: 12,
                line_count: 1,
                content: "needle old\n".to_string(),
            }],
        )?;
        let texts = store.load_file_texts_for_search(Some("needle"), true)?;
        require_eq(&texts.len(), &1, "indexed text row count")?;

        store.replace_file_texts_for_paths(&["src/main.rs".to_string()], &[])?;
        let missing = store.load_file_text("src/main.rs")?;
        require_eq(&missing.is_none(), &true, "cleared stale indexed text")?;
        Ok(())
    }

    #[test]
    fn indexed_file_text_search_stops_visiting_but_finishes_the_page() -> Result<(), Box<dyn Error>>
    {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.replace_file_texts_for_paths(
            &["src/a.rs".to_string(), "src/b.rs".to_string()],
            &[
                IndexedFileText {
                    path: "src/a.rs".to_string(),
                    content_hash: Some("hash-a".to_string()),
                    byte_count: 14,
                    line_count: 1,
                    content: "needle first\n".to_string(),
                },
                IndexedFileText {
                    path: "src/b.rs".to_string(),
                    content_hash: Some("hash-b".to_string()),
                    byte_count: 15,
                    line_count: 1,
                    content: "needle second\n".to_string(),
                },
            ],
        )?;

        let mut visited = Vec::new();
        store.visit_file_texts_for_search(Some("needle"), true, |text| {
            visited.push(text.path);
            Ok(false)
        })?;
        require_eq(&visited, &vec!["src/a.rs".to_string()], "early stop rows")?;
        Ok(())
    }

    #[test]
    fn task_arri_ut_arri_4_17() -> Result<(), Box<dyn Error>> {
        let corrupt = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
            Some("injected corruption after one row".to_owned()),
        );
        let rows = vec![Ok::<_, DbError>("first"), Err(DbError::Sqlite(corrupt))];
        let mut visited = Vec::new();
        let Err(error) = visit_rows_to_terminal(rows.into_iter(), &mut |value| {
            visited.push(value);
            Ok(false)
        }) else {
            return Err(
                io::Error::other("corruption after a returned row unexpectedly succeeded").into(),
            );
        };
        require_eq(&visited, &vec!["first"], "rows exposed before corruption")?;
        if !matches!(error, DbError::DatabaseCorruption { .. })
            || !error.to_string().contains("reset-index --dry-run")
        {
            return Err(io::Error::other(format!(
                "corruption lost its typed recovery guidance: {error}"
            ))
            .into());
        }

        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.replace_file_texts_for_paths(
            &["src/a.rs".to_owned(), "src/b.rs".to_owned()],
            &[
                IndexedFileText {
                    path: "src/a.rs".to_owned(),
                    content_hash: Some("hash-a".to_owned()),
                    byte_count: 5,
                    line_count: 1,
                    content: "first".to_owned(),
                },
                IndexedFileText {
                    path: "src/b.rs".to_owned(),
                    content_hash: Some("hash-b".to_owned()),
                    byte_count: 6,
                    line_count: 1,
                    content: "second".to_owned(),
                },
            ],
        )?;
        let interrupt = store.connection.get_interrupt_handle();
        let mut interrupted_rows = Vec::new();
        let Err(error) = store.visit_file_texts_for_search(None, false, |text| {
            interrupted_rows.push(text.path);
            interrupt.interrupt();
            Ok(false)
        }) else {
            return Err(
                io::Error::other("an interrupted terminal step unexpectedly succeeded").into(),
            );
        };
        require_eq(
            &interrupted_rows,
            &vec!["src/a.rs".to_owned()],
            "rows exposed before interruption",
        )?;
        if !matches!(
            error,
            DbError::Sqlite(ref source)
                if source.sqlite_error_code() == Some(ErrorCode::OperationInterrupted)
        ) {
            return Err(io::Error::other(format!(
                "interrupted row iteration returned the wrong failure: {error}"
            ))
            .into());
        }
        Ok(())
    }

    #[test]
    fn suggested_purpose_is_not_approved() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let node = Node {
            path: "src/main.rs".to_string(),
            kind: NodeKind::File,
            parent_path: normalized_parent("src/main.rs"),
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(12),
            mtime_ns: Some(10),
            content_hash: Some("abc".to_string()),
        };
        store.replace_scan(&[node])?;
        store.set_suggested_purpose("src/main.rs", "Maybe application entry point")?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.source,
            &PurposeSource::Generated,
            "suggested source",
        )?;
        require_eq(
            &nodes[0].purpose.status,
            &PurposeStatus::Suggested,
            "suggested status",
        )?;
        store.set_purpose(
            "src/main.rs",
            "Application entry point",
            PurposeSource::Agent,
        )?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.status,
            &PurposeStatus::Approved,
            "agent-approved status",
        )?;
        Ok(())
    }

    #[test]
    fn agent_reviewed_marker_depends_on_agent_approved_source() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash-a")])?;

        store.set_suggested_purpose("src/main.rs", "Maybe application entry point")?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.agent_reviewed(),
            &false,
            "generated suggestion is not agent reviewed",
        )?;

        store.set_purpose(
            "src/main.rs",
            "Imported application entry point",
            PurposeSource::Imported,
        )?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.agent_reviewed(),
            &false,
            "imported purpose is not agent reviewed",
        )?;

        store.set_purpose(
            "src/main.rs",
            "Agent-reviewed application entry point",
            PurposeSource::Agent,
        )?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.agent_reviewed(),
            &true,
            "agent-approved purpose is agent reviewed",
        )?;

        store.connection.execute(
            "
            UPDATE purposes
            SET source = 'human'
            WHERE node_id = (SELECT id FROM nodes WHERE path = 'src/main.rs')
            ",
            [],
        )?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.source,
            &PurposeSource::Agent,
            "legacy human source normalizes to agent",
        )?;
        require_eq(
            &nodes[0].purpose.agent_reviewed(),
            &true,
            "legacy approved human row remains reviewed",
        )?;

        store.replace_scan(&[test_file_node("src/main.rs", "hash-b")])?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.status,
            &PurposeStatus::Stale,
            "changed reviewed purpose becomes stale",
        )?;
        require_eq(
            &nodes[0].purpose.agent_reviewed(),
            &false,
            "stale purpose is not agent reviewed",
        )?;
        Ok(())
    }

    #[test]
    fn updates_content_summary_without_approving_purpose() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let node = Node {
            path: "src/lib.rs".to_string(),
            kind: NodeKind::File,
            parent_path: normalized_parent("src/lib.rs"),
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(24),
            mtime_ns: Some(10),
            content_hash: Some("def".to_string()),
        };
        store.replace_scan(&[node])?;
        store.set_node_summary(
            "src/lib.rs",
            "rust source defining library entry functions.",
        )?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].summary,
            &Some("rust source defining library entry functions.".to_string()),
            "updated content summary",
        )?;
        require_eq(
            &nodes[0].purpose.status,
            &PurposeStatus::Missing,
            "summary update does not approve purpose",
        )?;
        Ok(())
    }

    #[test]
    fn task_arri_ut_arri_4_21_persists_compact_symbol_graph() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let graph = SymbolGraph {
            path: "src/main.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![CodeSymbol {
                path: "src/main.rs".to_string(),
                language: Some("rust".to_string()),
                name: "main".to_string(),
                kind: SymbolKind::Function,
                signature: "fn main()".to_string(),
                exported: true,
                documentation: Some("Run the application.".to_string()),
                line_start: 1,
                line_end: 3,
                parent: None,
                parser: ParserKind::TreeSitter,
                detail: Some("function_item".to_string()),
            }],
            relations: vec![SymbolRelation {
                path: "src/main.rs".to_string(),
                source_name: "main".to_string(),
                target_name: "println!".to_string(),
                kind: RelationKind::Calls,
                line: 2,
                context: "println!(\"hello\")".to_string(),
                parser: ParserKind::TreeSitter,
            }],
        };

        let compact = CompactSymbolGraph::try_from(graph.clone())?;
        store.replace_compact_symbol_graph(&compact)?;
        store.replace_symbol_graph(&graph)?;
        let symbols = store.load_symbols(Some("src/main.rs"), Some("main"), 10)?;
        let relations = store.load_symbol_relations(Some("src/main.rs"), Some("println"), 10)?;
        let metadata = store
            .load_source_parse_metadata("src/main.rs")?
            .ok_or_else(|| io::Error::other("missing source parse metadata"))?;
        require_eq(&symbols.len(), &1, "symbol count after replace")?;
        require_eq(&relations.len(), &1, "relation count after replace")?;
        require_eq(&metadata.parser, &ParserKind::TreeSitter, "metadata parser")?;
        require_eq(&metadata.symbol_count, &1, "metadata symbol count")?;
        require_eq(&metadata.relation_count, &1, "metadata relation count")?;
        require_eq(&symbols[0].exported, &true, "exported metadata")?;
        require_eq(
            &symbols[0].documentation,
            &Some("Run the application.".to_string()),
            "documentation metadata",
        )?;
        if let Ok(out_of_range) = usize::try_from(u64::from(u32::MAX) + 1) {
            let mut invalid = graph.clone();
            invalid.symbols[0].line_end = out_of_range;
            require_eq(
                &matches!(
                    store.replace_symbol_graph(&invalid),
                    Err(DbError::InvalidCompactSymbolGraph(
                        CompactSymbolGraphError::LineOutOfRange {
                            field: "symbol.line_end",
                            value,
                        }
                    )) if value == out_of_range
                ),
                &true,
                "out-of-range compatibility graph did not fail before persistence",
            )?;
            let persisted = store.load_symbols(Some("src/main.rs"), Some("main"), 10)?;
            require_eq(
                &persisted[0].line_end,
                &3,
                "invalid compatibility graph left the existing row intact",
            )?;
        }
        Ok(())
    }

    #[test]
    fn task_arri_ut_arri_4_22_uses_indexed_graph_adjacency() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let source_graph = projectatlas_symbols::extract_symbol_graph(
            "src/lib.rs",
            Some("rust"),
            "pub mod nested {}\n\
             pub fn source() { middle(); middle(); external_api(); }\n\
             fn middle() { target(); }\n\
             fn target() {}\n",
        );
        let source_graph = CompactSymbolGraph::try_from(source_graph)?;
        store.replace_compact_symbol_graph(&source_graph)?;
        let manifest_graph = projectatlas_symbols::extract_symbol_graph(
            "Cargo.toml",
            Some("cargo-manifest"),
            "[package]\nname = \"projectatlas-db\"\nversion = \"0.1.0\"\n\
             [dependencies]\nrusqlite = \"0.32\"\n",
        );
        let manifest_graph = CompactSymbolGraph::try_from(manifest_graph)?;
        store.replace_compact_symbol_graph(&manifest_graph)?;
        let unbound_manifest_path = "fixtures/Cargo.toml";
        let unbound_manifest_graph = CompactSymbolGraph::try_from(SymbolGraph {
            path: unbound_manifest_path.to_owned(),
            language: Some(GRAPH_CARGO_MANIFEST_LANGUAGE.to_owned()),
            parser: ParserKind::Manifest,
            symbols: Vec::new(),
            relations: vec![SymbolRelation {
                path: unbound_manifest_path.to_owned(),
                source_name: "unknown-manifest-owner".to_owned(),
                target_name: "fabricated-target".to_owned(),
                kind: RelationKind::DependsOn,
                line: 1,
                context: "fabricated-target = \"1\"".to_owned(),
                parser: ParserKind::Manifest,
            }],
        })?;
        store.replace_compact_symbol_graph(&unbound_manifest_graph)?;
        let overloaded_graph = CompactSymbolGraph::try_from(SymbolGraph {
            path: "src/overloaded.rs".to_owned(),
            language: Some("rust".to_owned()),
            parser: ParserKind::TreeSitter,
            symbols: vec![
                CodeSymbol {
                    path: "src/overloaded.rs".to_owned(),
                    language: Some("rust".to_owned()),
                    name: "render".to_owned(),
                    kind: SymbolKind::Method,
                    signature: "fn render(&self)".to_owned(),
                    exported: false,
                    documentation: None,
                    line_start: 2,
                    line_end: 2,
                    parent: Some("Renderer".to_owned()),
                    parser: ParserKind::TreeSitter,
                    detail: Some("function_item".to_owned()),
                },
                CodeSymbol {
                    path: "src/overloaded.rs".to_owned(),
                    language: Some("rust".to_owned()),
                    name: "render".to_owned(),
                    kind: SymbolKind::Method,
                    signature: "fn render(&self, value: usize)".to_owned(),
                    exported: false,
                    documentation: None,
                    line_start: 3,
                    line_end: 3,
                    parent: Some("Renderer".to_owned()),
                    parser: ParserKind::TreeSitter,
                    detail: Some("function_item".to_owned()),
                },
            ],
            relations: Vec::new(),
        })?;
        store.replace_compact_symbol_graph(&overloaded_graph)?;
        let ambiguous_symbol = |name: &str, parent: Option<&str>, line: usize| CodeSymbol {
            path: "src/ambiguous.rs".to_owned(),
            language: Some("rust".to_owned()),
            name: name.to_owned(),
            kind: SymbolKind::Function,
            signature: format!("fn {name}()"),
            exported: false,
            documentation: None,
            line_start: line,
            line_end: line,
            parent: parent.map(ToOwned::to_owned),
            parser: ParserKind::TreeSitter,
            detail: Some("function_item".to_owned()),
        };
        let ambiguous_graph = CompactSymbolGraph::try_from(SymbolGraph {
            path: "src/ambiguous.rs".to_owned(),
            language: Some("rust".to_owned()),
            parser: ParserKind::TreeSitter,
            symbols: vec![
                ambiguous_symbol("ambiguous_source", None, 1),
                ambiguous_symbol("duplicate", Some("Left"), 2),
                ambiguous_symbol("duplicate", Some("Right"), 3),
            ],
            relations: vec![SymbolRelation {
                path: "src/ambiguous.rs".to_owned(),
                source_name: "ambiguous_source".to_owned(),
                target_name: "duplicate".to_owned(),
                kind: RelationKind::Calls,
                line: 1,
                context: "duplicate();".to_owned(),
                parser: ParserKind::TreeSitter,
            }],
        })?;
        store.replace_compact_symbol_graph(&ambiguous_graph)?;

        insert_graph_entity(
            &store,
            "b",
            250,
            GraphEntityKind::Declaration,
            Some("src/retained.rs"),
            "retained::only",
        )?;

        let source_entities = store.load_graph_entities_by_qualified_name(
            GraphEntityKind::Declaration,
            "source",
            4,
        )?;
        let middle_entities = store.load_graph_entities_by_qualified_name(
            GraphEntityKind::Declaration,
            "middle",
            4,
        )?;
        let target_entities = store.load_graph_entities_by_qualified_name(
            GraphEntityKind::Declaration,
            "target",
            4,
        )?;
        require_eq(
            &source_entities.len(),
            &1,
            "normal extraction source entity",
        )?;
        require_eq(
            &middle_entities.len(),
            &1,
            "normal extraction middle entity",
        )?;
        require_eq(
            &target_entities.len(),
            &1,
            "normal extraction target entity",
        )?;
        let source = source_entities[0].stable_key_digest;
        let middle = middle_entities[0].stable_key_digest;
        let target = target_entities[0].stable_key_digest;
        let active_source = store
            .load_graph_entity(&source)?
            .ok_or_else(|| io::Error::other("active source graph entity was not found"))?;
        require_eq(
            &active_source.qualified_name,
            &Some("source".to_owned()),
            "normal persistence stable-key lookup",
        )?;

        let packages = store.load_graph_entities_by_qualified_name(
            GraphEntityKind::Package,
            "projectatlas-db",
            4,
        )?;
        let modules =
            store.load_graph_entities_by_qualified_name(GraphEntityKind::Module, "nested", 4)?;
        require_eq(&packages.len(), &1, "normal package extraction and lookup")?;
        require_eq(&modules.len(), &1, "normal module extraction and lookup")?;
        store.connection.execute(
            "UPDATE graph_entities
             SET discriminator = 'receiver-only'
             WHERE structural_slot = (
                 SELECT active_slot FROM graph_publication_state WHERE singleton = 1
             )
               AND repository_path = 'src/overloaded.rs'
               AND signature = 'fn render(&self)'",
            [],
        )?;
        let overloads = store.load_graph_entities_by_qualified_name(
            GraphEntityKind::Declaration,
            "Renderer::render",
            4,
        )?;
        require_eq(&overloads.len(), &2, "duplicate qualified-name overloads")?;
        require_eq(
            &overloads
                .iter()
                .map(|entity| entity.stable_key_digest)
                .collect::<BTreeSet<_>>()
                .len(),
            &2,
            "overload stable-key distinction",
        )?;
        require_eq(
            &overloads
                .iter()
                .filter_map(|entity| entity.signature.as_deref())
                .collect::<BTreeSet<_>>(),
            &BTreeSet::from(["fn render(&self)", "fn render(&self, value: usize)"]),
            "overload semantic distinction",
        )?;
        require_eq(
            &overloads
                .iter()
                .map(|entity| entity.discriminator.as_deref())
                .collect::<BTreeSet<_>>(),
            &BTreeSet::from([None, Some("receiver-only")]),
            "overload discriminator decoding",
        )?;
        let ambiguous_source = store.load_graph_entities_by_qualified_name(
            GraphEntityKind::Declaration,
            "ambiguous_source",
            4,
        )?;
        require_eq(
            &ambiguous_source.len(),
            &1,
            "ambiguous-target fixture source entity",
        )?;
        require_eq(
            &store
                .load_graph_adjacency(
                    &ambiguous_source[0].stable_key_digest,
                    GraphRelationDirection::Outbound,
                    Some(GraphRelationKind::Calls),
                    8,
                )?
                .is_empty(),
            &true,
            "ambiguous compatibility target does not become an exact edge",
        )?;
        let ambiguous_resolution = store.connection.query_row(
            "SELECT stable_key_digest, source_entity_digest, resolution_status,
                    candidate_total, candidate_completeness, unresolved_reason, confidence
             FROM graph_resolution_occurrences
             WHERE structural_slot = 'a'
               AND origin_repository_path = 'src/ambiguous.rs'
               AND relation_kind = 'calls'",
            [],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )?;
        require_eq(
            &ambiguous_resolution.1,
            &ambiguous_source[0].stable_key_digest.to_vec(),
            "ambiguous occurrence source identity",
        )?;
        require_eq(
            &(
                ambiguous_resolution.2.as_str(),
                ambiguous_resolution.3,
                ambiguous_resolution.4.as_str(),
                ambiguous_resolution.5.as_deref(),
                ambiguous_resolution.6.as_str(),
            ),
            &("ambiguous", 2, "complete", None, "medium"),
            "typed ambiguous occurrence state",
        )?;
        let ambiguous_candidates = store
            .connection
            .prepare(
                "SELECT entity.qualified_name, candidate.target_entity_digest
                 FROM graph_resolution_candidates AS candidate
                 JOIN graph_entities AS entity
                   ON entity.structural_slot = candidate.structural_slot
                  AND entity.stable_key_digest = candidate.target_entity_digest
                 WHERE candidate.structural_slot = 'a'
                   AND candidate.resolution_occurrence_digest = ?1
                 ORDER BY candidate.candidate_ordinal",
            )?
            .query_map([ambiguous_resolution.0.as_slice()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        require_eq(
            &ambiguous_candidates
                .iter()
                .map(|candidate| candidate.0.as_str())
                .collect::<BTreeSet<_>>(),
            &BTreeSet::from(["Left::duplicate", "Right::duplicate"]),
            "bounded ambiguous candidate identities",
        )?;
        require_eq(
            &ambiguous_candidates
                .windows(2)
                .all(|pair| pair[0].1 < pair[1].1),
            &true,
            "deterministic stable-key candidate order",
        )?;
        require_eq(
            &store.load_graph_entity(&graph_test_digest(250))?.is_none(),
            &true,
            "retained slot entity stays invisible",
        )?;

        let outbound_calls = store.load_graph_adjacency(
            &source,
            GraphRelationDirection::Outbound,
            Some(GraphRelationKind::Calls),
            8,
        )?;
        require_eq(&outbound_calls.len(), &1, "typed outbound adjacency")?;
        require_eq(
            &outbound_calls
                .iter()
                .any(|relation| relation.target == PersistedGraphTarget::Internal(middle)),
            &true,
            "outbound internal target",
        )?;
        let middle_edges = outbound_calls
            .iter()
            .filter(|relation| relation.target == PersistedGraphTarget::Internal(middle))
            .collect::<Vec<_>>();
        require_eq(
            &middle_edges.len(),
            &1,
            "repeated calls share one logical relation",
        )?;
        let call_confidence = store.connection.query_row(
            "SELECT confidence FROM graph_relations
             WHERE structural_slot = 'a' AND stable_key_digest = ?1",
            [&middle_edges[0].stable_key_digest[..]],
            |row| row.get::<_, String>(0),
        )?;
        require_eq(
            &call_confidence.as_str(),
            &"high",
            "name-only call relation confidence",
        )?;
        let evidence_rows = store
            .connection
            .prepare(
                "SELECT origin_kind, origin_repository_path, resolver_name, resolver_version,
                        content_span_fingerprint, occurrence_discriminator, evidence_class,
                        confidence, completeness, structural_slot, last_changed_epoch, explanation
                 FROM graph_evidence_occurrences
                 WHERE structural_slot = 'a' AND relation_digest = ?1
                 ORDER BY occurrence_discriminator",
            )?
            .query_map([&middle_edges[0].stable_key_digest[..]], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, Option<String>>(11)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        require_eq(
            &evidence_rows.len(),
            &2,
            "repeated call evidence occurrence count",
        )?;
        let expected_epoch = i64::try_from(middle_edges[0].last_changed_epoch.get())?;
        for (index, evidence) in evidence_rows.iter().enumerate() {
            require_eq(
                &evidence.0.as_str(),
                &"repository-path",
                "evidence origin kind",
            )?;
            require_eq(&evidence.1.as_str(), &"src/lib.rs", "evidence origin path")?;
            require_eq(
                &evidence.2.as_str(),
                &GRAPH_PARSER_IDENTITY,
                "evidence resolver",
            )?;
            require_eq(
                &evidence.3.as_str(),
                &GRAPH_PARSER_VERSION,
                "evidence resolver version",
            )?;
            require_eq(&evidence.4.len(), &32, "evidence fingerprint width")?;
            require_eq(
                &evidence.5,
                &i64::try_from(index)?,
                "evidence occurrence discriminator",
            )?;
            require_eq(&evidence.6.as_str(), &"direct", "evidence class")?;
            require_eq(&evidence.7.as_str(), &"high", "evidence confidence")?;
            require_eq(&evidence.8.as_str(), &"complete", "evidence completeness")?;
            require_eq(&evidence.9.as_str(), &"a", "evidence structural slot")?;
            require_eq(&evidence.10, &expected_epoch, "evidence structural epoch")?;
            require_eq(&evidence.11.is_some(), &true, "evidence source context")?;
        }
        let outbound =
            store.load_graph_adjacency(&source, GraphRelationDirection::Outbound, None, 8)?;
        require_eq(&outbound.len(), &1, "unfiltered outbound adjacency")?;
        require_eq(
            &store
                .load_symbol_relations(Some("src/lib.rs"), Some("external_api"), 8)?
                .len(),
            &1,
            "unresolved call remains in the compatibility projection",
        )?;
        let unresolved_call = store.connection.query_row(
            "SELECT source_entity_digest, resolution_status, candidate_total,
                    candidate_completeness, unresolved_reason, confidence,
                    occurrence_discriminator
             FROM graph_resolution_occurrences
             WHERE structural_slot = 'a'
               AND origin_repository_path = 'src/lib.rs'
               AND relation_kind = 'calls'
               AND unresolved_reason =
                   'call target was not resolved in the containing file'",
            [],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )?;
        require_eq(
            &unresolved_call.0,
            &source.to_vec(),
            "unresolved call containing source identity",
        )?;
        require_eq(
            &(
                unresolved_call.1.as_str(),
                unresolved_call.2,
                unresolved_call.3.as_deref(),
                unresolved_call.4.as_deref(),
                unresolved_call.5.as_str(),
                unresolved_call.6,
            ),
            &(
                "unresolved",
                None,
                None,
                Some("call target was not resolved in the containing file"),
                "low",
                0,
            ),
            "typed unresolved call occurrence",
        )?;

        let inbound_calls = store.load_graph_adjacency(
            &target,
            GraphRelationDirection::Inbound,
            Some(GraphRelationKind::Calls),
            8,
        )?;
        require_eq(&inbound_calls.len(), &1, "typed inbound adjacency")?;
        require_eq(
            &inbound_calls[0].source_entity_digest,
            &middle,
            "inbound source digest",
        )?;
        let inbound =
            store.load_graph_adjacency(&target, GraphRelationDirection::Inbound, None, 8)?;
        require_eq(&inbound, &inbound_calls, "unfiltered inbound adjacency")?;
        let calls = store.load_graph_relations_by_kind(GraphRelationKind::Calls, 1)?;
        require_eq(&calls.len(), &1, "relation-family row bound")?;
        let dependencies = store.load_graph_relations_by_kind(GraphRelationKind::DependsOn, 8)?;
        if !dependencies.iter().any(|relation| {
            relation.target
                == PersistedGraphTarget::External {
                    namespace: "package".to_owned(),
                    value: "rusqlite".to_owned(),
                }
        }) {
            let manifest_symbols = manifest_graph
                .symbols()
                .map(|symbol| (symbol.name(), symbol.kind(), symbol.parent()))
                .collect::<Vec<_>>();
            let manifest_relations = manifest_graph
                .relations()
                .map(|relation| {
                    (
                        relation.source_name(),
                        relation.target_name(),
                        relation.kind(),
                    )
                })
                .collect::<Vec<_>>();
            return Err(io::Error::other(format!(
                "normal manifest extraction lost the typed external dependency: persisted={dependencies:?}, symbols={manifest_symbols:?}, relations={manifest_relations:?}"
            ))
            .into());
        }
        require_eq(
            &dependencies.iter().any(|relation| {
                relation.target
                    == PersistedGraphTarget::External {
                        namespace: GRAPH_EXTERNAL_PACKAGE_NAMESPACE.to_owned(),
                        value: "fabricated-target".to_owned(),
                    }
            }),
            &false,
            "unbound manifest source abstains from exact dependency ownership",
        )?;
        require_eq(
            &store
                .load_symbol_relations(Some(unbound_manifest_path), Some("fabricated-target"), 8)?
                .len(),
            &1,
            "unbound manifest dependency remains in the compatibility projection",
        )?;
        let manifest_confidence = store.connection.query_row(
            "SELECT relation.confidence, evidence.confidence
             FROM graph_relations AS relation
             JOIN graph_evidence_occurrences AS evidence
               ON evidence.structural_slot = relation.structural_slot
              AND evidence.relation_digest = relation.stable_key_digest
             WHERE relation.structural_slot = 'a'
               AND relation.relation_kind = 'depends-on'
               AND relation.external_target_namespace = 'package'
               AND relation.external_target_value = 'rusqlite'",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?;
        require_eq(
            &(
                manifest_confidence.0.as_str(),
                manifest_confidence.1.as_str(),
            ),
            &("exact", "exact"),
            "explicit manifest dependency confidence",
        )?;
        let unbound_manifest_resolution = store.connection.query_row(
            "SELECT resolution_status, candidate_total, candidate_completeness,
                    unresolved_reason, confidence,
                    (SELECT COUNT(*) FROM graph_resolution_candidates AS candidate
                     WHERE candidate.structural_slot = occurrence.structural_slot
                       AND candidate.resolution_occurrence_digest =
                           occurrence.stable_key_digest)
             FROM graph_resolution_occurrences AS occurrence
             WHERE structural_slot = 'a'
               AND origin_repository_path = ?1
               AND relation_kind = 'depends-on'",
            [unbound_manifest_path],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )?;
        require_eq(
            &(
                unbound_manifest_resolution.0.as_str(),
                unbound_manifest_resolution.1,
                unbound_manifest_resolution.2.as_deref(),
                unbound_manifest_resolution.3.as_deref(),
                unbound_manifest_resolution.4.as_str(),
                unbound_manifest_resolution.5,
            ),
            &(
                "unresolved",
                None,
                None,
                Some("relation source was not resolved in the containing file"),
                "low",
                0,
            ),
            "unknown dependency source abstention",
        )?;
        require_eq(
            &store
                .load_graph_adjacency(&source, GraphRelationDirection::Outbound, None, 0)?
                .is_empty(),
            &true,
            "zero row bound",
        )?;

        let publication = store.publication_state()?;
        let mut visited = BTreeSet::from([source]);
        let mut frontier = vec![source];
        for _ in 0..2 {
            let mut next = Vec::new();
            for entity in frontier {
                for relation in store.load_graph_adjacency(
                    &entity,
                    GraphRelationDirection::Outbound,
                    Some(GraphRelationKind::Calls),
                    8,
                )? {
                    if let PersistedGraphTarget::Internal(target) = relation.target
                        && visited.insert(target)
                    {
                        next.push(target);
                    }
                }
            }
            frontier = next;
        }
        require_eq(
            &visited,
            &BTreeSet::from([source, middle, target]),
            "bounded adjacency composition under an unchanged publication",
        )?;
        require_eq(
            &store.publication_state()?,
            &publication,
            "adjacency composition publication remained unchanged",
        )?;

        require_query_plan_search(
            &store.connection,
            GRAPH_ENTITY_BY_STABLE_KEY_SQL,
            &[Value::Text("a".to_owned()), Value::Blob(source.to_vec())],
            GRAPH_ENTITIES_TABLE,
        )?;
        require_primary_key_columns(
            &store.connection,
            GRAPH_ENTITIES_TABLE,
            &["structural_slot", "stable_key_digest"],
        )?;
        require_query_plan_index(
            &store.connection,
            GRAPH_ENTITIES_BY_QUALIFIED_NAME_SQL,
            &[
                Value::Text("a".to_owned()),
                Value::Text(GraphEntityKind::Package.as_str().to_owned()),
                Value::Text("projectatlas-db".to_owned()),
                Value::Integer(4),
            ],
            "idx_graph_entities_slot_kind_qualified_name_stable_key",
        )?;
        require_query_plan_index(
            &store.connection,
            GRAPH_OUTBOUND_RELATIONS_BY_KIND_SQL,
            &[
                Value::Text("a".to_owned()),
                Value::Blob(source.to_vec()),
                Value::Text(GraphRelationKind::Calls.as_str().to_owned()),
                Value::Integer(8),
            ],
            "idx_graph_relations_slot_source_kind_stable_key",
        )?;
        require_query_plan_index(
            &store.connection,
            GRAPH_OUTBOUND_RELATIONS_SQL,
            &[
                Value::Text("a".to_owned()),
                Value::Blob(source.to_vec()),
                Value::Integer(8),
            ],
            "idx_graph_relations_slot_source_kind_stable_key",
        )?;
        require_query_plan_index(
            &store.connection,
            GRAPH_INBOUND_RELATIONS_BY_KIND_SQL,
            &[
                Value::Text("a".to_owned()),
                Value::Blob(target.to_vec()),
                Value::Text(GraphRelationKind::Calls.as_str().to_owned()),
                Value::Integer(8),
            ],
            "idx_graph_relations_slot_target_kind_stable_key",
        )?;
        require_query_plan_index(
            &store.connection,
            GRAPH_INBOUND_RELATIONS_SQL,
            &[
                Value::Text("a".to_owned()),
                Value::Blob(target.to_vec()),
                Value::Integer(8),
            ],
            "idx_graph_relations_slot_target_kind_stable_key",
        )?;
        require_query_plan_index(
            &store.connection,
            GRAPH_RELATIONS_BY_KIND_SQL,
            &[
                Value::Text("a".to_owned()),
                Value::Text(GraphRelationKind::Calls.as_str().to_owned()),
                Value::Integer(8),
            ],
            "idx_graph_relations_slot_kind_stable_key",
        )?;
        for sql in [
            GRAPH_ENTITY_BY_STABLE_KEY_SQL,
            GRAPH_ENTITIES_BY_QUALIFIED_NAME_SQL,
            GRAPH_OUTBOUND_RELATIONS_BY_KIND_SQL,
            GRAPH_OUTBOUND_RELATIONS_SQL,
            GRAPH_INBOUND_RELATIONS_BY_KIND_SQL,
            GRAPH_INBOUND_RELATIONS_SQL,
            GRAPH_RELATIONS_BY_KIND_SQL,
        ] {
            if sql.to_ascii_lowercase().contains("json") {
                return Err(io::Error::other(format!(
                    "typed graph query unexpectedly invokes JSON: {sql}"
                ))
                .into());
            }
        }
        Ok(())
    }

    #[test]
    fn task_arri_ut_arri_4_22_deduplicates_logical_resolution_candidates()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(&root)?;
        let symbol = |name: &str, signature: &str, line: usize| CodeSymbol {
            path: "src/candidate-set.rs".to_owned(),
            language: Some("rust".to_owned()),
            name: name.to_owned(),
            kind: SymbolKind::Function,
            signature: signature.to_owned(),
            exported: false,
            documentation: None,
            line_start: line,
            line_end: line,
            parent: None,
            parser: ParserKind::TreeSitter,
            detail: Some("function_item".to_owned()),
        };
        let graph = CompactSymbolGraph::try_from(SymbolGraph {
            path: "src/candidate-set.rs".to_owned(),
            language: Some("rust".to_owned()),
            parser: ParserKind::TreeSitter,
            symbols: vec![
                symbol("source", "fn source()", 1),
                symbol("target", "fn target()", 2),
                symbol("target", "fn target()", 3),
                symbol("target", "fn target(value: usize)", 4),
            ],
            relations: vec![SymbolRelation {
                path: "src/candidate-set.rs".to_owned(),
                source_name: "source".to_owned(),
                target_name: "target".to_owned(),
                kind: RelationKind::Calls,
                line: 1,
                context: "target();".to_owned(),
                parser: ParserKind::TreeSitter,
            }],
        })?;
        store.replace_compact_symbol_graph(&graph)?;

        let targets = store.load_graph_entities_by_qualified_name(
            GraphEntityKind::Declaration,
            "target",
            8,
        )?;
        require_eq(
            &targets.len(),
            &2,
            "equivalent parser entities collapse while overloads remain distinct",
        )?;
        let (occurrence_digest, candidate_total) = store.connection.query_row(
            "SELECT stable_key_digest, candidate_total
             FROM graph_resolution_occurrences
             WHERE structural_slot = 'a'
               AND origin_repository_path = 'src/candidate-set.rs'
               AND relation_kind = 'calls'",
            [],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)),
        )?;
        let candidates = store
            .connection
            .prepare(
                "SELECT candidate_ordinal, target_entity_digest
                 FROM graph_resolution_candidates
                 WHERE structural_slot = 'a'
                   AND resolution_occurrence_digest = ?1
                 ORDER BY candidate_ordinal",
            )?
            .query_map([occurrence_digest.as_slice()], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        require_eq(&candidate_total, &2, "logical candidate total")?;
        require_eq(
            &candidates
                .iter()
                .map(|candidate| candidate.0)
                .collect::<Vec<_>>(),
            &vec![0, 1],
            "logical candidate ordinals",
        )?;
        require_eq(
            &candidates
                .iter()
                .map(|candidate| candidate.1.as_slice())
                .collect::<BTreeSet<_>>()
                .len(),
            &2,
            "logical candidate targets are unique",
        )?;
        let root_text = store
            .project_root()?
            .ok_or_else(|| io::Error::other("candidate fixture root was not persisted"))?;
        schema::reconcile_full_structural_publication(
            &store.connection,
            &root_text,
            store.publication_state()?,
        )?;
        Ok(())
    }

    #[test]
    fn task_arri_ut_arri_4_22_evidence_failure_rolls_back_graph_and_compatibility_rows()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let baseline = compact_call_graph("src/evidence-rollback.rs", false)?;
        store.replace_compact_symbol_graph(&baseline)?;
        let before = graph_write_snapshot(&store.connection)?;
        let publication = store.publication_state()?;
        store.connection.execute_batch(
            "CREATE TEMP TRIGGER reject_graph_evidence
             BEFORE INSERT ON graph_evidence_occurrences
             BEGIN
                 SELECT RAISE(ABORT, 'injected graph evidence failure');
             END;",
        )?;

        let replacement = compact_call_graph("src/evidence-rollback.rs", true)?;
        let Err(error) = store.replace_compact_symbol_graph(&replacement) else {
            return Err(io::Error::other(
                "injected evidence failure did not abort graph replacement",
            )
            .into());
        };
        require_eq(
            &matches!(error, DbError::Sqlite(ref source) if source.to_string().contains("injected graph evidence failure")),
            &true,
            "typed evidence failure",
        )?;
        require_eq(
            &graph_write_snapshot(&store.connection)?,
            &before,
            "evidence failure transaction rollback",
        )?;
        require_eq(
            &store.publication_state()?,
            &publication,
            "evidence failure publication state",
        )?;
        Ok(())
    }

    #[test]
    fn task_arri_ut_arri_4_23_staging_batch_is_ordered_and_atomic() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let retained = compact_call_graph("src/retained.rs", false)?;
        store.stage_compact_symbol_graph(&retained)?;
        let before = graph_write_snapshot(&store.connection)?;
        let publication = store.publication_state()?;
        let first = compact_call_graph("src/first.rs", false)?;
        let second = compact_call_graph("src/second.rs", false)?;
        store.connection.execute_batch(
            "CREATE TEMP TRIGGER reject_second_staged_graph
             BEFORE INSERT ON graph_evidence_occurrences
             WHEN NEW.origin_repository_path = 'src/second.rs'
             BEGIN
                 SELECT RAISE(ABORT, 'injected second staged graph failure');
             END;",
        )?;

        let Err(error) = store.stage_compact_symbol_graphs([&first, &second]) else {
            return Err(io::Error::other(
                "injected second-graph failure did not abort the staging batch",
            )
            .into());
        };
        require_eq(
            &matches!(error, DbError::Sqlite(ref source) if source.to_string().contains("injected second staged graph failure")),
            &true,
            "typed staging batch failure",
        )?;
        require_eq(
            &graph_write_snapshot(&store.connection)?,
            &before,
            "failed staging batch retained its complete preexisting state",
        )?;
        require_eq(
            &store.publication_state()?,
            &publication,
            "failed staging batch publication state",
        )?;

        store
            .connection
            .execute_batch("DROP TRIGGER reject_second_staged_graph")?;
        store.stage_compact_symbol_graphs([&first, &second])?;
        let inserted_paths = store
            .connection
            .prepare("SELECT path FROM source_parse_metadata ORDER BY rowid")?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        require_eq(
            &inserted_paths,
            &vec![
                "src/retained.rs".to_owned(),
                "src/first.rs".to_owned(),
                "src/second.rs".to_owned(),
            ],
            "successful staging batch iterator order",
        )?;
        require_eq(
            &store.publication_state()?,
            &publication,
            "successful staging batch publication state",
        )?;
        Ok(())
    }

    #[test]
    fn task_arri_ut_arri_4_22_rejects_stable_digest_identity_collisions()
    -> Result<(), Box<dyn Error>> {
        let graph = compact_call_graph("src/identity-collision.rs", false)?;

        let mut entity_store = AtlasStore::in_memory()?;
        entity_store.replace_compact_symbol_graph(&graph)?;
        entity_store.connection.execute(
            "UPDATE graph_entities
             SET stable_key_canonical = ?1
             WHERE structural_slot = 'a' AND entity_kind = 'file'
               AND repository_path = 'src/identity-collision.rs'",
            [b"forged-entity-canonical".as_slice()],
        )?;
        let entity_before = graph_write_snapshot(&entity_store.connection)?;
        let entity_publication = entity_store.publication_state()?;
        let Err(entity_error) = entity_store.replace_compact_symbol_graph(&graph) else {
            return Err(io::Error::other(
                "entity digest collision did not abort graph replacement",
            )
            .into());
        };
        require_eq(
            &matches!(
                entity_error,
                DbError::StableGraphIdentityCollision {
                    table: GRAPH_ENTITIES_TABLE,
                    ..
                }
            ),
            &true,
            "typed entity identity collision",
        )?;
        require_eq(
            &graph_write_snapshot(&entity_store.connection)?,
            &entity_before,
            "entity collision byte-for-byte rollback",
        )?;
        require_eq(
            &entity_store.publication_state()?,
            &entity_publication,
            "entity collision publication state",
        )?;

        let mut relation_store = AtlasStore::in_memory()?;
        relation_store.replace_compact_symbol_graph(&graph)?;
        relation_store.connection.execute(
            "UPDATE graph_relations
             SET stable_key_canonical = ?1
             WHERE structural_slot = 'a'",
            [b"forged-relation-canonical".as_slice()],
        )?;
        let relation_before = graph_write_snapshot(&relation_store.connection)?;
        let relation_publication = relation_store.publication_state()?;
        let Err(relation_error) = relation_store.replace_compact_symbol_graph(&graph) else {
            return Err(io::Error::other(
                "relation digest collision did not abort graph replacement",
            )
            .into());
        };
        require_eq(
            &matches!(
                relation_error,
                DbError::StableGraphIdentityCollision {
                    table: GRAPH_RELATIONS_TABLE,
                    ..
                }
            ),
            &true,
            "typed relation identity collision",
        )?;
        require_eq(
            &graph_write_snapshot(&relation_store.connection)?,
            &relation_before,
            "relation collision byte-for-byte rollback",
        )?;
        require_eq(
            &relation_store.publication_state()?,
            &relation_publication,
            "relation collision publication state",
        )?;

        let mut evidence_store = AtlasStore::in_memory()?;
        evidence_store.replace_compact_symbol_graph(&graph)?;
        evidence_store.connection.execute(
            "UPDATE graph_evidence_occurrences
             SET stable_key_version = stable_key_version + 1,
                 stable_key_canonical = ?1
             WHERE structural_slot = 'a'",
            [b"forged-evidence-canonical".as_slice()],
        )?;
        let evidence_before = graph_write_snapshot(&evidence_store.connection)?;
        let evidence_publication = evidence_store.publication_state()?;
        let Err(evidence_error) = evidence_store.replace_compact_symbol_graph(&graph) else {
            return Err(io::Error::other(
                "evidence digest collision did not abort graph replacement",
            )
            .into());
        };
        require_eq(
            &matches!(
                evidence_error,
                DbError::StableGraphIdentityCollision {
                    table: GRAPH_EVIDENCE_OCCURRENCES_TABLE,
                    ..
                }
            ),
            &true,
            "typed evidence identity collision",
        )?;
        require_eq(
            &graph_write_snapshot(&evidence_store.connection)?,
            &evidence_before,
            "evidence collision byte-for-byte rollback",
        )?;
        require_eq(
            &evidence_store.publication_state()?,
            &evidence_publication,
            "evidence collision publication state",
        )?;

        let resolution_graph =
            CompactSymbolGraph::try_from(projectatlas_symbols::extract_symbol_graph(
                "src/resolution-identity-collision.rs",
                Some("rust"),
                "fn source() { unresolved_target(); }\n",
            ))?;
        let mut resolution_store = AtlasStore::in_memory()?;
        resolution_store.replace_compact_symbol_graph(&resolution_graph)?;
        resolution_store.connection.execute(
            "UPDATE graph_resolution_occurrences
             SET stable_key_version = stable_key_version + 1,
                 stable_key_canonical = ?1
             WHERE structural_slot = 'a'
               AND origin_repository_path = 'src/resolution-identity-collision.rs'",
            [b"forged-resolution-canonical".as_slice()],
        )?;
        let resolution_before = graph_write_snapshot(&resolution_store.connection)?;
        let resolution_publication = resolution_store.publication_state()?;
        let Err(resolution_error) =
            resolution_store.replace_compact_symbol_graph(&resolution_graph)
        else {
            return Err(io::Error::other(
                "resolution digest collision did not abort graph replacement",
            )
            .into());
        };
        require_eq(
            &matches!(
                resolution_error,
                DbError::StableGraphIdentityCollision {
                    table: GRAPH_RESOLUTION_OCCURRENCES_TABLE,
                    ..
                }
            ),
            &true,
            "typed resolution identity collision",
        )?;
        require_eq(
            &graph_write_snapshot(&resolution_store.connection)?,
            &resolution_before,
            "resolution collision byte-for-byte rollback",
        )?;
        require_eq(
            &resolution_store.publication_state()?,
            &resolution_publication,
            "resolution collision publication state",
        )?;
        Ok(())
    }

    #[test]
    fn full_scan_removal_clears_source_parse_metadata() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/a.rs", "hash-a")])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/a.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::new(),
            relations: Vec::new(),
        })?;
        require_eq(
            &store.load_source_parse_metadata("src/a.rs")?.is_some(),
            &true,
            "metadata exists before removal",
        )?;

        store.replace_scan(&[test_file_node("src/b.rs", "hash-b")])?;
        require_eq(
            &store.load_source_parse_metadata("src/a.rs")?,
            &None,
            "metadata cleared after full scan removal",
        )?;
        Ok(())
    }

    #[test]
    fn call_relations_are_limited_per_target() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let mut relations = Vec::new();
        for index in 0..5 {
            relations.push(SymbolRelation {
                path: format!("src/a{index}.rs"),
                source_name: format!("alpha_caller_{index}"),
                target_name: "alpha".to_string(),
                kind: RelationKind::Calls,
                line: index + 1,
                context: "alpha();".to_string(),
                parser: ParserKind::TreeSitter,
            });
        }
        relations.push(SymbolRelation {
            path: "src/z.rs".to_string(),
            source_name: "beta_caller".to_string(),
            target_name: "beta".to_string(),
            kind: RelationKind::Calls,
            line: 99,
            context: "beta();".to_string(),
            parser: ParserKind::TreeSitter,
        });
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::new(),
            relations,
        })?;

        let loaded =
            store.load_call_relations_to_targets(&["alpha".to_string(), "beta".to_string()], 2)?;
        let alpha_count = loaded
            .iter()
            .filter(|relation| relation.target_name == "alpha")
            .count();
        let beta_count = loaded
            .iter()
            .filter(|relation| relation.target_name == "beta")
            .count();
        require_eq(&alpha_count, &2, "alpha per-target limit")?;
        require_eq(&beta_count, &1, "beta preserved despite alpha skew")?;
        Ok(())
    }

    #[test]
    fn stores_health_resolution_ids() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.set_purpose("src/a.rs", "Shared purpose", PurposeSource::Agent)?;
        store.set_purpose("src/b.rs", "Shared purpose", PurposeSource::Agent)?;
        let duplicate = store
            .unresolved_health_findings(&[])?
            .into_iter()
            .find(|finding| finding.category == "duplicate-purpose")
            .ok_or_else(|| io::Error::other("duplicate-purpose finding missing"))?;
        let duplicate_id = duplicate.id.clone();
        store.resolve_health_finding(&HealthResolution {
            finding_id: duplicate_id.clone(),
            category: duplicate.category,
            path: duplicate.path,
            related_path: duplicate.related_path,
            rationale: "Paths intentionally mirror agent skill variants.".to_string(),
        })?;
        let ids = store.resolved_health_ids()?;
        require_eq(&ids, &vec![duplicate_id], "resolved ids")?;
        Ok(())
    }

    #[test]
    fn health_resolution_accepts_all_scope_agent_review_findings() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let mut asset_file = test_file_node("assets/logo.svg", "hash-logo");
        asset_file.extension = Some(".svg".to_string());
        asset_file.language = None;
        store.replace_scan(&[test_folder_node("assets"), asset_file])?;
        store.set_purpose(
            "assets/logo.svg",
            "Imported SVG brand asset purpose",
            PurposeSource::Imported,
        )?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 20,
                category: Some(CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED.to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        let finding = page
            .findings
            .iter()
            .find(|finding| finding.path == "assets/logo.svg")
            .ok_or_else(|| io::Error::other("asset review finding missing"))?;
        store.resolve_health_finding(&HealthResolution {
            finding_id: finding.id.clone(),
            category: finding.category.clone(),
            path: finding.path.clone(),
            related_path: finding.related_path.clone(),
            rationale: "Asset purpose imported from legacy metadata and intentionally accepted."
                .to_string(),
        })?;

        let remaining = store.unresolved_health_findings_page(
            &store.resolved_health_ids()?,
            &HealthQuery {
                start_index: 0,
                limit: 20,
                category: Some(CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED.to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        require_eq(
            &health_paths(&remaining).contains(&"assets/logo.svg"),
            &false,
            "resolved all-scope asset review finding",
        )?;
        Ok(())
    }

    #[test]
    fn purpose_set_reports_unindexed_path_without_sqlite_leak() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash")])?;
        let error = match store.set_purpose("no/such/file.rs", "Missing file", PurposeSource::Agent)
        {
            Ok(()) => return Err(io::Error::other("missing path should fail").into()),
            Err(error) => error,
        };

        require_eq(
            &error.to_string().contains("no/such/file.rs"),
            &true,
            "path named in error",
        )?;
        require_eq(
            &error.to_string().contains("sqlite error"),
            &false,
            "raw sqlite error hidden",
        )?;
        store.replace_scan(&[])?;
        let error = match store.set_purpose("src/main.rs", "Removed file", PurposeSource::Agent) {
            Ok(()) => return Err(io::Error::other("stale indexed path should fail").into()),
            Err(error) => error,
        };
        require_eq(
            &error.to_string().contains("src/main.rs"),
            &true,
            "stale path named in error",
        )?;
        Ok(())
    }

    #[test]
    fn health_resolution_requires_active_finding_tuple() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash")])?;
        let error = match store.resolve_health_finding(&HealthResolution {
            finding_id: "missing-id".to_string(),
            category: "duplicate-purpose".to_string(),
            path: "no/such/file.rs".to_string(),
            related_path: None,
            rationale: "typo".to_string(),
        }) {
            Ok(()) => {
                return Err(io::Error::other("nonexistent health finding should fail").into());
            }
            Err(error) => error,
        };

        require_eq(
            &error.to_string().contains("not active"),
            &true,
            "inactive finding rejected",
        )?;
        Ok(())
    }

    /// Build a representative Rust file node for store tests.
    fn test_file_node(path: &str, hash: &str) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::File,
            parent_path: normalized_parent(path),
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(12),
            mtime_ns: Some(10),
            content_hash: Some(hash.to_string()),
        }
    }

    fn compact_call_graph(
        path: &str,
        include_second_target: bool,
    ) -> Result<CompactSymbolGraph, CompactSymbolGraphError> {
        let symbol = |name: &str, line: usize| CodeSymbol {
            path: path.to_owned(),
            language: Some("rust".to_owned()),
            name: name.to_owned(),
            kind: SymbolKind::Function,
            signature: format!("fn {name}()"),
            exported: false,
            documentation: None,
            line_start: line,
            line_end: line,
            parent: None,
            parser: ParserKind::TreeSitter,
            detail: Some("function_item".to_owned()),
        };
        let mut symbols = vec![symbol("source", 1), symbol("target", 2)];
        let mut relations = vec![SymbolRelation {
            path: path.to_owned(),
            source_name: "source".to_owned(),
            target_name: "target".to_owned(),
            kind: RelationKind::Calls,
            line: 1,
            context: "target();".to_owned(),
            parser: ParserKind::TreeSitter,
        }];
        if include_second_target {
            symbols.push(symbol("replacement", 3));
            relations.push(SymbolRelation {
                path: path.to_owned(),
                source_name: "source".to_owned(),
                target_name: "replacement".to_owned(),
                kind: RelationKind::Calls,
                line: 1,
                context: "replacement();".to_owned(),
                parser: ParserKind::TreeSitter,
            });
        }
        CompactSymbolGraph::try_from(SymbolGraph {
            path: path.to_owned(),
            language: Some("rust".to_owned()),
            parser: ParserKind::TreeSitter,
            symbols,
            relations,
        })
    }

    fn graph_write_snapshot(
        connection: &Connection,
    ) -> Result<Vec<(&'static str, Vec<Vec<Value>>)>, rusqlite::Error> {
        [
            GRAPH_ENTITIES_TABLE,
            GRAPH_RELATIONS_TABLE,
            GRAPH_EVIDENCE_OCCURRENCES_TABLE,
            GRAPH_RESOLUTION_OCCURRENCES_TABLE,
            GRAPH_RESOLUTION_CANDIDATES_TABLE,
            "symbols",
            "symbol_relations",
            "source_parse_metadata",
        ]
        .into_iter()
        .map(|table| {
            let sql = match table {
                GRAPH_ENTITIES_TABLE => "SELECT * FROM graph_entities ORDER BY rowid",
                GRAPH_RELATIONS_TABLE => "SELECT * FROM graph_relations ORDER BY rowid",
                GRAPH_EVIDENCE_OCCURRENCES_TABLE => {
                    "SELECT * FROM graph_evidence_occurrences ORDER BY rowid"
                }
                GRAPH_RESOLUTION_OCCURRENCES_TABLE => {
                    "SELECT * FROM graph_resolution_occurrences ORDER BY rowid"
                }
                GRAPH_RESOLUTION_CANDIDATES_TABLE => {
                    "SELECT * FROM graph_resolution_candidates ORDER BY rowid"
                }
                "symbols" => "SELECT * FROM symbols ORDER BY rowid",
                "symbol_relations" => "SELECT * FROM symbol_relations ORDER BY rowid",
                "source_parse_metadata" => "SELECT * FROM source_parse_metadata ORDER BY rowid",
                _ => unreachable!("graph snapshot table inventory is closed"),
            };
            let mut statement = connection.prepare(sql)?;
            let column_count = statement.column_count();
            let rows = statement
                .query_map([], |row| {
                    (0..column_count)
                        .map(|index| row.get::<_, Value>(index))
                        .collect::<Result<Vec<_>, _>>()
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok((table, rows))
        })
        .collect()
    }

    fn graph_test_digest(value: u8) -> [u8; 32] {
        [value; 32]
    }

    fn insert_graph_entity(
        store: &AtlasStore,
        slot: &str,
        key: u8,
        kind: GraphEntityKind,
        repository_path: Option<&str>,
        qualified_name: &str,
    ) -> DbResult<()> {
        let digest = graph_test_digest(key);
        let canonical = [b'e', key];
        let project = store.project_instance_id()?.as_bytes();
        store.connection.execute(
            "INSERT INTO graph_entities(
                 stable_key_digest, stable_key_version, stable_key_canonical,
                 project_instance_id, entity_kind, repository_path, qualified_name,
                 parser_kind, parser_identity, parser_version,
                 structural_slot, last_changed_epoch
             ) VALUES(?1, 1, ?2, ?3, ?4, ?5, ?6, 'structural', 'task-arri-4.22', '1', ?7, 1)",
            params![
                &digest[..],
                &canonical[..],
                &project[..],
                kind.as_str(),
                repository_path,
                qualified_name,
                slot,
            ],
        )?;
        Ok(())
    }

    fn require_query_plan_search(
        connection: &Connection,
        sql: &str,
        values: &[Value],
        expected_table: &str,
    ) -> Result<(), Box<dyn Error>> {
        let mut statement = connection.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
        let details = statement
            .query_map(params_from_iter(values.iter()), |row| {
                row.get::<_, String>(3)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if !details
            .iter()
            .any(|detail| detail.contains(&format!("SEARCH {expected_table}")))
        {
            return Err(io::Error::other(format!(
                "query did not use an indexed search of {expected_table}: {details:?}"
            ))
            .into());
        }
        if details
            .iter()
            .any(|detail| detail.contains(&format!("SCAN {expected_table}")))
        {
            return Err(io::Error::other(format!(
                "query plan scanned {expected_table}: {details:?}"
            ))
            .into());
        }
        Ok(())
    }

    fn require_primary_key_columns(
        connection: &Connection,
        table: &str,
        expected_columns: &[&str],
    ) -> Result<(), Box<dyn Error>> {
        let pragma = match table {
            GRAPH_ENTITIES_TABLE => "PRAGMA table_info(graph_entities)",
            GRAPH_RELATIONS_TABLE => "PRAGMA table_info(graph_relations)",
            _ => {
                return Err(io::Error::other(format!(
                    "primary-key helper does not accept table {table:?}"
                ))
                .into());
            }
        };
        let mut columns = connection
            .prepare(pragma)?
            .query_map([], |row| {
                Ok((row.get::<_, i64>(5)?, row.get::<_, String>(1)?))
            })?
            .filter_map(|row| match row {
                Ok((ordinal, column)) if ordinal > 0 => Some(Ok((ordinal, column))),
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>, _>>()?;
        columns.sort_by_key(|(ordinal, _)| *ordinal);
        let columns = columns
            .iter()
            .map(|(_, column)| column.as_str())
            .collect::<Vec<_>>();
        require_eq(
            &columns.as_slice(),
            &expected_columns,
            "primary-key columns",
        )
    }

    fn require_query_plan_index(
        connection: &Connection,
        sql: &str,
        values: &[Value],
        expected_index: &str,
    ) -> Result<(), Box<dyn Error>> {
        let mut statement = connection.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
        let details = statement
            .query_map(params_from_iter(values.iter()), |row| {
                row.get::<_, String>(3)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if !details
            .iter()
            .any(|detail| detail.contains(&format!("USING INDEX {expected_index}")))
        {
            return Err(io::Error::other(format!(
                "query did not use {expected_index}: {details:?}"
            ))
            .into());
        }
        if details.iter().any(|detail| {
            detail.contains("SCAN graph_relations") || detail.contains("SCAN graph_entities")
        }) {
            return Err(io::Error::other(format!(
                "query plan scanned an unrelated graph table: {details:?}"
            ))
            .into());
        }
        Ok(())
    }

    /// Build a representative folder node for store tests.
    fn test_folder_node(path: &str) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::Folder,
            parent_path: normalized_parent(path),
            extension: None,
            language: None,
            size_bytes: None,
            mtime_ns: Some(10),
            content_hash: None,
        }
    }

    /// Return the default low-cost purpose review query used by agent linting.
    fn low_query() -> HealthQuery {
        HealthQuery {
            start_index: 0,
            limit: 20,
            category: Some(CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED.to_string()),
            severity: Some(Severity::Warning),
            path_prefix: Some(".".to_string()),
            summary_only: false,
            scope: HealthScope::purpose_default(),
        }
    }

    /// Collect health finding paths in returned order.
    fn health_paths(page: &HealthFindingsPage) -> Vec<&str> {
        page.findings
            .iter()
            .map(|finding| finding.path.as_str())
            .collect()
    }

    /// Require two test values to be equal without panicking.
    fn require_eq<T>(actual: &T, expected: &T, label: &str) -> Result<(), Box<dyn Error>>
    where
        T: Debug + PartialEq,
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
}
