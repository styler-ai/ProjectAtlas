//! Portable, derived-only repository graph snapshots.

use crate::project_identity::{load_graph_generation, load_project_identity};
use crate::repository_graph;
use crate::schema::SCHEMA_VERSION;
use crate::{AtlasStore, DbError, DbResult, IndexPublicationState, load_index_publication};
use blake3::Hasher;
use projectatlas_core::IndexGeneration;
use projectatlas_core::graph::{
    CanonicalResolutionKey, Completeness, ConfidenceClass, CoverageRecord, CoverageScope,
    CoverageState, EntityResolutionKey, EntitySelector, GraphEntity, GraphIdentityText,
    GraphLimitKind, GraphRelationKind, LogicalRelation, PortableResolutionKey, ProjectInstanceId,
    RelationDependencyKey, RelationOccurrence, RelationResolution, RepositoryFilePath, SourceSpan,
};
use rusqlite::Connection;
use rusqlite::backup::Backup;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::time::Duration;

/// Stable portable payload version.
const DERIVED_SNAPSHOT_FORMAT_VERSION: u32 = 1;
/// Logical repository root used by portable paths.
const DERIVED_SNAPSHOT_ROOT: &str = ".";
/// Maximum encoded JSON accepted before deserialization.
pub const MAX_DERIVED_SNAPSHOT_JSON_BYTES: u64 = 512 * 1024 * 1024;
/// Maximum decoded graph rows admitted to one explicit snapshot operation.
const MAX_DERIVED_SNAPSHOT_ROWS: u64 = 1_000_000;
/// Maximum decoded row payload retained while constructing a snapshot.
const MAX_DERIVED_SNAPSHOT_RETAINED_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum private `SQLite` capture size.
const MAX_PRIVATE_CAPTURE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
/// Maximum live node rows hashed into one source-state identity.
const MAX_SOURCE_STATE_ROWS: u64 = 5_000_000;
/// Maximum source-state metadata bytes hashed by one operation.
const MAX_SOURCE_STATE_BYTES: u64 = 512 * 1024 * 1024;
/// Fixed BLAKE3 lowercase hexadecimal length.
const BLAKE3_HEX_BYTES: usize = 64;

/// Portable snapshot metadata that contains no project or machine identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DerivedGraphSnapshotMetadata {
    /// Runtime version that wrote the portable contract.
    pub runtime_version: String,
    /// `SQLite` schema understood by the writer.
    pub schema_version: i64,
    /// Logical archive root; always `.`.
    pub root: String,
    /// Complete source graph generation captured privately.
    pub source_generation: IndexGeneration,
    /// Digest of current repository-relative node identities and content hashes.
    pub source_state_digest: String,
    /// Complete index capability/registry contract fingerprint.
    pub capability_fingerprint: String,
}

/// Exact portable columns and row count exported from one derived owner.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DerivedSnapshotContent {
    /// Source derived table.
    pub table: String,
    /// Portable allowlisted columns or transformed equivalents.
    pub columns: Vec<String>,
    /// Number of exported logical rows.
    pub rows: u64,
}

/// Integrity-checked, project-independent graph snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DerivedGraphSnapshot {
    /// Stable payload format.
    format_version: u32,
    /// BLAKE3 digest of metadata, inventory, and graph rows.
    digest: String,
    /// Portable source/capability identity.
    metadata: DerivedGraphSnapshotMetadata,
    /// Exact allowlist and row inventory.
    content: Vec<DerivedSnapshotContent>,
    /// Typed portable graph.
    graph: PortableGraph,
}

impl DerivedGraphSnapshot {
    /// Borrow validated portable metadata.
    #[must_use]
    pub const fn metadata(&self) -> &DerivedGraphSnapshotMetadata {
        &self.metadata
    }

    /// Borrow the exact derived content inventory.
    #[must_use]
    pub fn content(&self) -> &[DerivedSnapshotContent] {
        &self.content
    }

    /// Borrow the lowercase content digest.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Encode a validated snapshot as deterministic JSON.
    ///
    /// # Errors
    ///
    /// Returns an error when the snapshot was mutated into an invalid shape or
    /// its encoded representation exceeds the declared limit.
    pub fn to_json(&self) -> DbResult<Vec<u8>> {
        self.validate()?;
        let encoded = serde_json::to_vec(self)?;
        require_limit(
            "encoded JSON bytes",
            usize_to_u64(encoded.len())?,
            MAX_DERIVED_SNAPSHOT_JSON_BYTES,
        )?;
        Ok(encoded)
    }

    /// Decode and validate one bounded JSON snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error for oversized, malformed, incompatible, or
    /// integrity-mismatched payloads.
    pub fn from_json(encoded: &[u8]) -> DbResult<Self> {
        require_limit(
            "encoded JSON bytes",
            usize_to_u64(encoded.len())?,
            MAX_DERIVED_SNAPSHOT_JSON_BYTES,
        )?;
        let snapshot = serde_json::from_slice::<Self>(encoded)?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Validate versions, inventory, referential shape, and content digest.
    fn validate(&self) -> DbResult<()> {
        if self.format_version != DERIVED_SNAPSHOT_FORMAT_VERSION {
            return invalid("unsupported portable format version");
        }
        if self.metadata.runtime_version != env!("CARGO_PKG_VERSION") {
            return invalid("snapshot runtime version does not match this runtime");
        }
        if self.metadata.schema_version != SCHEMA_VERSION {
            return invalid("snapshot schema version does not match this runtime");
        }
        if self.metadata.root != DERIVED_SNAPSHOT_ROOT {
            return invalid("snapshot root is not the portable repository root");
        }
        if self.metadata.source_generation == IndexGeneration::ZERO {
            return invalid("snapshot source generation is zero");
        }
        if !valid_digest(&self.metadata.source_state_digest) {
            return invalid("snapshot source-state digest is malformed");
        }
        if self.metadata.capability_fingerprint.is_empty()
            || self.metadata.capability_fingerprint.len() > 4_096
            || self
                .metadata
                .capability_fingerprint
                .chars()
                .any(char::is_control)
        {
            return invalid("snapshot capability fingerprint is invalid");
        }
        self.graph.validate()?;
        if self.content != expected_content(&self.graph)? {
            return invalid("snapshot content inventory does not match the portable graph");
        }
        let digest = snapshot_digest(&self.metadata, &self.content, &self.graph)?;
        if self.digest != digest {
            return invalid("snapshot content digest does not match");
        }
        Ok(())
    }
}

/// Result of one normal projection publication from a portable snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DerivedGraphSnapshotImport {
    /// Complete generation visible before import.
    pub previous_generation: IndexGeneration,
    /// Complete generation published by import.
    pub published_generation: IndexGeneration,
    /// Snapshot content digest that was activated.
    pub digest: String,
    /// Portable derived row inventory.
    pub content: Vec<DerivedSnapshotContent>,
}

/// Complete typed graph collected from one private `SQLite` backup.
pub(crate) struct CapturedGraph {
    /// Captured graph entities.
    pub(crate) entities: Vec<GraphEntity>,
    /// Captured logical relations.
    pub(crate) relations: Vec<LogicalRelation>,
    /// Captured exact relation occurrences.
    pub(crate) occurrences: Vec<RelationOccurrence>,
    /// Captured graph coverage.
    pub(crate) coverage: Vec<CoverageRecord>,
    /// Captured entity resolution exports.
    pub(crate) entity_exports: Vec<([u8; 32], CanonicalResolutionKey)>,
    /// Captured relation resolution dependencies.
    pub(crate) relation_dependencies: Vec<([u8; 32], CanonicalResolutionKey)>,
}

/// Shared construction budget used while decoding the private backup.
pub(crate) struct SnapshotBudget {
    /// Rows admitted so far.
    rows: u64,
    /// Estimated retained bytes admitted so far.
    retained_bytes: u64,
}

impl SnapshotBudget {
    /// Create an empty snapshot budget.
    pub(crate) const fn new() -> Self {
        Self {
            rows: 0,
            retained_bytes: 0,
        }
    }

    /// Admit one decoded row of the supplied retained size.
    pub(crate) fn admit(&mut self, bytes: u64) -> DbResult<()> {
        self.rows = self
            .rows
            .checked_add(1)
            .ok_or(DbError::DerivedSnapshotInvalid {
                reason: "snapshot row count overflowed",
            })?;
        require_limit("decoded rows", self.rows, MAX_DERIVED_SNAPSHOT_ROWS)?;
        self.retained_bytes = self
            .retained_bytes
            .checked_add(bytes)
            .and_then(|value| value.checked_add(128))
            .ok_or(DbError::DerivedSnapshotInvalid {
                reason: "snapshot retained byte count overflowed",
            })?;
        require_limit(
            "decoded retained bytes",
            self.retained_bytes,
            MAX_DERIVED_SNAPSHOT_RETAINED_BYTES,
        )
    }
}

impl AtlasStore {
    /// Build a portable graph snapshot from a private `SQLite` backup.
    ///
    /// # Errors
    ///
    /// Returns an error for incomplete publications, corrupt or oversized
    /// captures, private temporary-file failures, or invalid graph rows.
    pub fn export_derived_graph_snapshot(&self) -> DbResult<DerivedGraphSnapshot> {
        let capture_dir = tempfile::tempdir().map_err(|source| DbError::DerivedSnapshotIo {
            path: std::env::temp_dir(),
            source,
        })?;
        let capture_path = capture_dir.path().join("derived-graph-capture.sqlite");
        let mut capture = Connection::open(&capture_path).map_err(DbError::from)?;
        require_private_capture_size(&self.connection)?;
        {
            let backup = Backup::new(&self.connection, &mut capture)?;
            backup.run_to_completion(256, Duration::from_millis(1), None)?;
        }
        let quick_check =
            capture.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))?;
        if quick_check != "ok" {
            return invalid("private SQLite capture failed integrity check");
        }

        let publication = load_index_publication(&capture)?
            .filter(|publication| {
                publication.state == IndexPublicationState::Complete
                    && publication.generation != IndexGeneration::ZERO
            })
            .ok_or(DbError::GraphPublicationUnavailable)?;
        let capability_fingerprint = publication
            .contract_fingerprint
            .filter(|value| !value.is_empty())
            .ok_or(DbError::DerivedSnapshotInvalid {
                reason: "complete publication has no capability fingerprint",
            })?;
        let project =
            load_project_identity(&capture)?.ok_or(DbError::ProjectInstanceIdentityMissing)?;
        if load_graph_generation(&capture)? != Some(publication.generation) {
            return invalid("private capture graph generation is not complete");
        }
        let source_state_digest = source_state_digest(&capture)?;
        let mut budget = SnapshotBudget::new();
        let captured = repository_graph::capture_derived_graph(
            &capture,
            project,
            publication.generation,
            &mut budget,
        )?;
        DerivedGraphSnapshot::from_capture(
            captured,
            publication.generation,
            source_state_digest,
            capability_fingerprint,
        )
    }

    /// Validate and atomically publish a portable graph into this project.
    ///
    /// The destination must already have the same current source state and full
    /// capability contract. Only derived graph rows are replaced; destination
    /// identity, source projections, purposes, health state, settings, and
    /// telemetry stay owned by the destination database.
    ///
    /// # Errors
    ///
    /// Returns an error before publication for incompatible source/capability
    /// state or malformed content. Publication conflicts and `SQLite` failures
    /// roll the existing generation back through the normal guard.
    pub fn import_derived_graph_snapshot(
        &mut self,
        snapshot: &DerivedGraphSnapshot,
    ) -> DbResult<DerivedGraphSnapshotImport> {
        snapshot.validate()?;
        let publication = self
            .index_publication()?
            .filter(|publication| {
                publication.state == IndexPublicationState::Complete
                    && publication.generation != IndexGeneration::ZERO
            })
            .ok_or(DbError::GraphPublicationUnavailable)?;
        if publication.contract_fingerprint.as_deref()
            != Some(snapshot.metadata.capability_fingerprint.as_str())
        {
            return invalid("destination capability fingerprint does not match the snapshot");
        }
        if source_state_digest(&self.connection)? != snapshot.metadata.source_state_digest {
            return invalid("destination source state does not match the snapshot");
        }
        let project = self
            .project_instance_id()?
            .ok_or(DbError::ProjectInstanceIdentityMissing)?;
        let next_generation = publication
            .generation
            .checked_next()
            .ok_or(DbError::PublicationGenerationOverflow)?;
        let graph = snapshot.graph.bind(project, next_generation)?;
        let mut guard = self.begin_index_projection_refresh_from(
            &snapshot.metadata.capability_fingerprint,
            publication.generation,
        )?;
        guard.replace_repository_graph_with_resolution_keys(
            project,
            &graph.entities,
            &graph.relations,
            &graph.occurrences,
            &graph.coverage,
            &graph.entity_exports,
            &graph.relation_dependencies,
        )?;
        guard.complete()?;
        Ok(DerivedGraphSnapshotImport {
            previous_generation: publication.generation,
            published_generation: next_generation,
            digest: snapshot.digest.clone(),
            content: snapshot.content.clone(),
        })
    }
}

impl DerivedGraphSnapshot {
    /// Assemble and validate one snapshot from a private typed capture.
    fn from_capture(
        captured: CapturedGraph,
        source_generation: IndexGeneration,
        source_state_digest: String,
        capability_fingerprint: String,
    ) -> DbResult<Self> {
        let graph = PortableGraph::from_capture(captured)?;
        let metadata = DerivedGraphSnapshotMetadata {
            runtime_version: env!("CARGO_PKG_VERSION").to_string(),
            schema_version: SCHEMA_VERSION,
            root: DERIVED_SNAPSHOT_ROOT.to_string(),
            source_generation,
            source_state_digest,
            capability_fingerprint,
        };
        let content = expected_content(&graph)?;
        let digest = snapshot_digest(&metadata, &content, &graph)?;
        let snapshot = Self {
            format_version: DERIVED_SNAPSHOT_FORMAT_VERSION,
            digest,
            metadata,
            content,
            graph,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
}

/// Project-independent normalized graph.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PortableGraph {
    /// Entity selectors in portable index order.
    entities: Vec<EntitySelector>,
    /// Logical relations using portable entity indexes.
    relations: Vec<PortableRelation>,
    /// Exact source occurrences using portable relation indexes.
    occurrences: Vec<PortableOccurrence>,
    /// Coverage without a source publication generation.
    coverage: Vec<PortableCoverage>,
    /// Entity exports using portable entity indexes.
    entity_exports: Vec<PortableEntityResolutionKey>,
    /// Relation dependencies using portable relation indexes.
    relation_dependencies: Vec<PortableRelationResolutionKey>,
}

impl PortableGraph {
    /// Convert a project-bound graph capture into a portable graph.
    fn from_capture(captured: CapturedGraph) -> DbResult<Self> {
        let mut entity_indexes = BTreeMap::new();
        for (index, entity) in captured.entities.iter().enumerate() {
            let index = usize_to_u32(index)?;
            if entity_indexes
                .insert(entity.key().digest_bytes()?, index)
                .is_some()
            {
                return invalid("snapshot contains duplicate entity identities");
            }
        }
        let mut relation_indexes = BTreeMap::new();
        for (index, relation) in captured.relations.iter().enumerate() {
            let index = usize_to_u32(index)?;
            if relation_indexes
                .insert(relation.key().digest_bytes()?, index)
                .is_some()
            {
                return invalid("snapshot contains duplicate relation identities");
            }
        }
        let entities = captured
            .entities
            .iter()
            .map(|entity| entity.selector().clone())
            .collect();
        let relations = captured
            .relations
            .iter()
            .map(|relation| PortableRelation::from_relation(relation, &entity_indexes))
            .collect::<DbResult<Vec<_>>>()?;
        let occurrences = captured
            .occurrences
            .iter()
            .map(|occurrence| {
                Ok(PortableOccurrence {
                    relation: required_index(
                        &relation_indexes,
                        occurrence.relation().digest_bytes()?,
                        "snapshot occurrence owner relation is absent",
                    )?,
                    file: occurrence.file().clone(),
                    span: occurrence.span(),
                })
            })
            .collect::<DbResult<Vec<_>>>()?;
        let coverage = captured
            .coverage
            .iter()
            .map(PortableCoverage::from)
            .collect();
        let entity_exports = captured
            .entity_exports
            .into_iter()
            .map(|(entity, key)| {
                Ok(PortableEntityResolutionKey {
                    entity: required_index(
                        &entity_indexes,
                        entity,
                        "snapshot resolution export entity is absent",
                    )?,
                    key: key.portable()?,
                })
            })
            .collect::<DbResult<Vec<_>>>()?;
        let relation_dependencies = captured
            .relation_dependencies
            .into_iter()
            .map(|(relation, key)| {
                Ok(PortableRelationResolutionKey {
                    relation: required_index(
                        &relation_indexes,
                        relation,
                        "snapshot resolution dependency relation is absent",
                    )?,
                    key: key.portable()?,
                })
            })
            .collect::<DbResult<Vec<_>>>()?;
        Ok(Self {
            entities,
            relations,
            occurrences,
            coverage,
            entity_exports,
            relation_dependencies,
        })
    }

    /// Validate row limits and all portable indexes.
    fn validate(&self) -> DbResult<()> {
        let total = [
            self.entities.len(),
            self.relations.len(),
            self.occurrences.len(),
            self.coverage.len(),
            self.entity_exports.len(),
            self.relation_dependencies.len(),
        ]
        .into_iter()
        .try_fold(0_u64, |total, rows| {
            total
                .checked_add(usize_to_u64(rows)?)
                .ok_or(DbError::DerivedSnapshotInvalid {
                    reason: "snapshot row count overflowed",
                })
        })?;
        require_limit("decoded rows", total, MAX_DERIVED_SNAPSHOT_ROWS)?;
        for relation in &self.relations {
            require_vector_index(
                relation.source,
                self.entities.len(),
                "snapshot relation source index is invalid",
            )?;
            match relation.resolution {
                PortableRelationResolution::Resolved { target }
                | PortableRelationResolution::External { target } => require_vector_index(
                    target,
                    self.entities.len(),
                    "snapshot relation target index is invalid",
                )?,
                PortableRelationResolution::Ambiguous { .. }
                | PortableRelationResolution::Unresolved { .. } => {}
            }
        }
        for occurrence in &self.occurrences {
            require_vector_index(
                occurrence.relation,
                self.relations.len(),
                "snapshot occurrence relation index is invalid",
            )?;
        }
        for export in &self.entity_exports {
            require_vector_index(
                export.entity,
                self.entities.len(),
                "snapshot export entity index is invalid",
            )?;
        }
        for dependency in &self.relation_dependencies {
            require_vector_index(
                dependency.relation,
                self.relations.len(),
                "snapshot dependency relation index is invalid",
            )?;
        }
        Ok(())
    }

    /// Rebind the portable graph to one destination project and generation.
    fn bind(
        &self,
        project: ProjectInstanceId,
        generation: IndexGeneration,
    ) -> DbResult<BoundGraph> {
        self.validate()?;
        let entities = self
            .entities
            .iter()
            .cloned()
            .map(|selector| GraphEntity::new(project, selector, generation).map_err(Into::into))
            .collect::<DbResult<Vec<_>>>()?;
        let relations = self
            .relations
            .iter()
            .map(|relation| relation.bind(&entities, generation))
            .collect::<DbResult<Vec<_>>>()?;
        let occurrences = self
            .occurrences
            .iter()
            .map(|occurrence| {
                let relation = indexed(
                    &relations,
                    occurrence.relation,
                    "snapshot occurrence relation index is invalid",
                )?;
                RelationOccurrence::new(
                    relation,
                    occurrence.file.clone(),
                    occurrence.span,
                    generation,
                )
                .map_err(Into::into)
            })
            .collect::<DbResult<Vec<_>>>()?;
        let coverage = self
            .coverage
            .iter()
            .map(|coverage| coverage.bind(generation))
            .collect::<DbResult<Vec<_>>>()?;
        let entity_exports = self
            .entity_exports
            .iter()
            .map(|export| {
                EntityResolutionKey::new(
                    indexed(
                        &entities,
                        export.entity,
                        "snapshot export entity index is invalid",
                    )?
                    .key()
                    .clone(),
                    export.key.bind(project),
                )
                .map_err(Into::into)
            })
            .collect::<DbResult<Vec<_>>>()?;
        let relation_dependencies = self
            .relation_dependencies
            .iter()
            .map(|dependency| {
                RelationDependencyKey::new(
                    indexed(
                        &relations,
                        dependency.relation,
                        "snapshot dependency relation index is invalid",
                    )?
                    .key()
                    .clone(),
                    dependency.key.bind(project),
                )
                .map_err(Into::into)
            })
            .collect::<DbResult<Vec<_>>>()?;
        Ok(BoundGraph {
            entities,
            relations,
            occurrences,
            coverage,
            entity_exports,
            relation_dependencies,
        })
    }
}

/// Portable logical relation using entity indexes instead of project keys.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PortableRelation {
    /// Source entity index.
    source: u32,
    /// Typed relation kind.
    kind: GraphRelationKind,
    /// Project-independent resolution state.
    resolution: PortableRelationResolution,
    /// Relation confidence.
    confidence: ConfidenceClass,
    /// Relation completeness.
    completeness: Completeness,
}

impl PortableRelation {
    /// Convert one project-bound relation to portable entity indexes.
    fn from_relation(
        relation: &LogicalRelation,
        entity_indexes: &BTreeMap<[u8; 32], u32>,
    ) -> DbResult<Self> {
        let resolution = match relation.resolution() {
            RelationResolution::Resolved { target, .. } => PortableRelationResolution::Resolved {
                target: required_index(
                    entity_indexes,
                    target.digest_bytes()?,
                    "snapshot resolved target is absent",
                )?,
            },
            RelationResolution::Ambiguous {
                reference,
                candidates,
            } => PortableRelationResolution::Ambiguous {
                reference: reference.clone(),
                candidates: *candidates,
            },
            RelationResolution::Unresolved { reference } => {
                PortableRelationResolution::Unresolved {
                    reference: reference.clone(),
                }
            }
            RelationResolution::External { target, .. } => PortableRelationResolution::External {
                target: required_index(
                    entity_indexes,
                    target.digest_bytes()?,
                    "snapshot external target is absent",
                )?,
            },
        };
        Ok(Self {
            source: required_index(
                entity_indexes,
                relation.source().digest_bytes()?,
                "snapshot relation source is absent",
            )?,
            kind: relation.kind(),
            resolution,
            confidence: relation.confidence(),
            completeness: relation.completeness(),
        })
    }

    /// Bind one portable relation to destination entities.
    fn bind(
        &self,
        entities: &[GraphEntity],
        generation: IndexGeneration,
    ) -> DbResult<LogicalRelation> {
        let source = indexed(
            entities,
            self.source,
            "snapshot relation source index is invalid",
        )?;
        let resolution = match &self.resolution {
            PortableRelationResolution::Resolved { target } => {
                RelationResolution::resolved(indexed(
                    entities,
                    *target,
                    "snapshot resolved target index is invalid",
                )?)?
            }
            PortableRelationResolution::Ambiguous {
                reference,
                candidates,
            } => RelationResolution::Ambiguous {
                reference: reference.clone(),
                candidates: *candidates,
            },
            PortableRelationResolution::Unresolved { reference } => {
                RelationResolution::Unresolved {
                    reference: reference.clone(),
                }
            }
            PortableRelationResolution::External { target } => {
                RelationResolution::external(indexed(
                    entities,
                    *target,
                    "snapshot external target index is invalid",
                )?)?
            }
        };
        LogicalRelation::new(
            source,
            self.kind,
            resolution,
            self.confidence,
            self.completeness,
            generation,
        )
        .map_err(Into::into)
    }
}

/// Portable resolution state without project-qualified stable keys.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum PortableRelationResolution {
    /// Relation resolved to one indexed entity.
    Resolved {
        /// Target entity index.
        target: u32,
    },
    /// Relation has multiple candidate targets.
    Ambiguous {
        /// Original unresolved reference.
        reference: GraphIdentityText,
        /// Number of candidate targets.
        candidates: NonZeroU32,
    },
    /// Relation has no resolved target.
    Unresolved {
        /// Original unresolved reference.
        reference: GraphIdentityText,
    },
    /// Relation resolves to an external indexed entity.
    External {
        /// Target entity index.
        target: u32,
    },
}

/// Portable exact source occurrence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PortableOccurrence {
    /// Owning relation index.
    relation: u32,
    /// Repository-relative evidence file.
    file: RepositoryFilePath,
    /// Exact source span.
    span: SourceSpan,
}

/// Portable graph coverage without a source publication generation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PortableCoverage {
    /// Covered graph scope.
    scope: CoverageScope,
    /// Optional covered relation kind.
    relation: Option<GraphRelationKind>,
    /// Coverage state.
    state: CoverageState,
    /// Covered row count.
    covered: u64,
    /// Omitted row count.
    omitted: u64,
    /// Optional omission reason.
    reason: Option<GraphIdentityText>,
    /// Optional reached graph limit.
    reached_limit: Option<GraphLimitKind>,
}

impl From<&CoverageRecord> for PortableCoverage {
    fn from(coverage: &CoverageRecord) -> Self {
        Self {
            scope: coverage.scope().clone(),
            relation: coverage.relation(),
            state: coverage.state(),
            covered: coverage.covered(),
            omitted: coverage.omitted(),
            reason: coverage.reason().cloned(),
            reached_limit: coverage.reached_limit(),
        }
    }
}

impl PortableCoverage {
    /// Bind portable coverage to the destination generation.
    fn bind(&self, generation: IndexGeneration) -> DbResult<CoverageRecord> {
        CoverageRecord::new(
            self.scope.clone(),
            self.relation,
            self.state,
            self.covered,
            self.omitted,
            generation,
            self.reason.clone(),
            self.reached_limit,
        )
        .map_err(Into::into)
    }
}

/// Portable entity export key.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PortableEntityResolutionKey {
    /// Exporting entity index.
    entity: u32,
    /// Project-independent canonical key.
    key: PortableResolutionKey,
}

/// Portable relation dependency key.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PortableRelationResolutionKey {
    /// Dependent relation index.
    relation: u32,
    /// Project-independent canonical key.
    key: PortableResolutionKey,
}

/// Destination-bound graph ready for the existing publication transaction.
struct BoundGraph {
    /// Destination-bound graph entities.
    entities: Vec<GraphEntity>,
    /// Destination-bound logical relations.
    relations: Vec<LogicalRelation>,
    /// Destination-bound exact occurrences.
    occurrences: Vec<RelationOccurrence>,
    /// Destination-bound coverage.
    coverage: Vec<CoverageRecord>,
    /// Destination-bound entity exports.
    entity_exports: Vec<EntityResolutionKey>,
    /// Destination-bound relation dependencies.
    relation_dependencies: Vec<RelationDependencyKey>,
}

/// Digest body used to avoid hashing the digest field itself.
#[derive(Serialize)]
struct SnapshotDigestBody<'a> {
    /// Portable source and capability metadata.
    metadata: &'a DerivedGraphSnapshotMetadata,
    /// Exact exported content inventory.
    content: &'a [DerivedSnapshotContent],
    /// Portable graph body.
    graph: &'a PortableGraph,
}

/// Compute the deterministic digest over snapshot content.
fn snapshot_digest(
    metadata: &DerivedGraphSnapshotMetadata,
    content: &[DerivedSnapshotContent],
    graph: &PortableGraph,
) -> DbResult<String> {
    let encoded = serde_json::to_vec(&SnapshotDigestBody {
        metadata,
        content,
        graph,
    })?;
    require_limit(
        "digest body bytes",
        usize_to_u64(encoded.len())?,
        MAX_DERIVED_SNAPSHOT_JSON_BYTES,
    )?;
    Ok(blake3::hash(&encoded).to_hex().to_string())
}

/// Construct the exact allowlisted content inventory.
fn expected_content(graph: &PortableGraph) -> DbResult<Vec<DerivedSnapshotContent>> {
    Ok(vec![
        content("graph_entities", &["entity_selector"], graph.entities.len())?,
        content(
            "graph_relations",
            &[
                "source_entity",
                "relation_kind",
                "resolution",
                "confidence",
                "completeness",
            ],
            graph.relations.len(),
        )?,
        content(
            "graph_relation_occurrences",
            &["relation", "file_path", "source_span"],
            graph.occurrences.len(),
        )?,
        content(
            "graph_coverage",
            &[
                "scope",
                "relation_kind",
                "state",
                "covered",
                "omitted",
                "reason",
                "reached_limit",
            ],
            graph.coverage.len(),
        )?,
        content(
            "graph_entity_exports+graph_resolution_keys",
            &["entity", "resolution_domain", "portable_canonical_identity"],
            graph.entity_exports.len(),
        )?,
        content(
            "graph_relation_dependencies+graph_resolution_keys",
            &[
                "relation",
                "resolution_domain",
                "portable_canonical_identity",
            ],
            graph.relation_dependencies.len(),
        )?,
    ])
}

/// Construct one content inventory row.
fn content(table: &str, columns: &[&str], rows: usize) -> DbResult<DerivedSnapshotContent> {
    Ok(DerivedSnapshotContent {
        table: table.to_string(),
        columns: columns.iter().map(|column| (*column).to_string()).collect(),
        rows: usize_to_u64(rows)?,
    })
}

/// Hash the current repository-relative source state.
fn source_state_digest(connection: &Connection) -> DbResult<String> {
    let mut statement = connection.prepare(
        "SELECT path, kind, extension, language, size_bytes, content_hash
           FROM nodes
          WHERE exists_now = 1
          ORDER BY path",
    )?;
    let mut rows = statement.query([])?;
    let mut hasher = Hasher::new();
    hasher.update(b"projectatlas.derived-snapshot.source-state.v1");
    let mut row_count = 0_u64;
    let mut byte_count = 0_u64;
    while let Some(row) = rows.next()? {
        row_count = row_count
            .checked_add(1)
            .ok_or(DbError::DerivedSnapshotInvalid {
                reason: "source-state row count overflowed",
            })?;
        require_limit("source-state rows", row_count, MAX_SOURCE_STATE_ROWS)?;
        let fields = [
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            row.get::<_, Option<i64>>(4)?
                .map_or_else(String::new, |value| value.to_string()),
            row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        ];
        for field in fields {
            byte_count = byte_count.checked_add(usize_to_u64(field.len())?).ok_or(
                DbError::DerivedSnapshotInvalid {
                    reason: "source-state byte count overflowed",
                },
            )?;
            require_limit(
                "source-state metadata bytes",
                byte_count,
                MAX_SOURCE_STATE_BYTES,
            )?;
            hash_field(&mut hasher, field.as_bytes())?;
        }
    }
    hasher.update(&row_count.to_le_bytes());
    Ok(hasher.finalize().to_hex().to_string())
}

/// Hash one length-framed source-state field.
fn hash_field(hasher: &mut Hasher, value: &[u8]) -> DbResult<()> {
    let length = usize_to_u64(value.len())?;
    hasher.update(&length.to_le_bytes());
    hasher.update(value);
    Ok(())
}

/// Reject a private capture larger than the explicit snapshot ceiling.
fn require_private_capture_size(connection: &Connection) -> DbResult<()> {
    let page_count = connection.query_row("PRAGMA page_count", [], |row| row.get::<_, u64>(0))?;
    let page_size = connection.query_row("PRAGMA page_size", [], |row| row.get::<_, u64>(0))?;
    let bytes = page_count
        .checked_mul(page_size)
        .ok_or(DbError::DerivedSnapshotInvalid {
            reason: "private SQLite capture size overflowed",
        })?;
    require_limit(
        "private SQLite capture bytes",
        bytes,
        MAX_PRIVATE_CAPTURE_BYTES,
    )
}

/// Resolve a captured digest to its portable index.
fn required_index(
    indexes: &BTreeMap<[u8; 32], u32>,
    key: [u8; 32],
    reason: &'static str,
) -> DbResult<u32> {
    indexes
        .get(&key)
        .copied()
        .ok_or(DbError::DerivedSnapshotInvalid { reason })
}

/// Validate that a portable index addresses the supplied vector.
fn require_vector_index(index: u32, length: usize, reason: &'static str) -> DbResult<()> {
    if usize::try_from(index)
        .ok()
        .is_some_and(|index| index < length)
    {
        Ok(())
    } else {
        invalid(reason)
    }
}

/// Return one vector item addressed by a validated portable index.
fn indexed<'a, T>(values: &'a [T], index: u32, reason: &'static str) -> DbResult<&'a T> {
    values
        .get(usize::try_from(index).map_err(|_source| DbError::DerivedSnapshotInvalid { reason })?)
        .ok_or(DbError::DerivedSnapshotInvalid { reason })
}

/// Enforce one named resource ceiling.
fn require_limit(resource: &'static str, found: u64, maximum: u64) -> DbResult<()> {
    if found <= maximum {
        Ok(())
    } else {
        Err(DbError::DerivedSnapshotLimit {
            resource,
            found,
            maximum,
        })
    }
}

/// Return whether a digest is lowercase BLAKE3 hexadecimal.
fn valid_digest(value: &str) -> bool {
    value.len() == BLAKE3_HEX_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Convert an in-memory length into a portable count.
fn usize_to_u64(value: usize) -> DbResult<u64> {
    u64::try_from(value).map_err(|_source| DbError::DerivedSnapshotInvalid {
        reason: "snapshot size cannot be represented",
    })
}

/// Convert an in-memory index into its portable representation.
fn usize_to_u32(value: usize) -> DbResult<u32> {
    u32::try_from(value).map_err(|_source| DbError::DerivedSnapshotLimit {
        resource: "portable row indexes",
        found: u64::try_from(value).unwrap_or(u64::MAX),
        maximum: u64::from(u32::MAX),
    })
}

/// Return a typed invalid-snapshot error.
fn invalid<T>(reason: &'static str) -> DbResult<T> {
    Err(DbError::DerivedSnapshotInvalid { reason })
}

#[cfg(test)]
mod tests {
    use super::{DerivedGraphSnapshot, invalid, snapshot_digest};
    use crate::AtlasStore;
    use projectatlas_core::graph::{
        CanonicalResolutionKey, Completeness, ConfidenceClass, CoverageRecord, CoverageScope,
        CoverageState, EntityResolutionKey, EntitySelector, GraphEntity, GraphIdentityText,
        GraphRelationKind, LogicalRelation, RelationDependencyKey, RelationOccurrence,
        RelationResolution, RepositoryFilePath, ResolutionKeyDomain, SourceSpan, SymbolSelector,
    };
    use projectatlas_core::symbols::{RelationKind, SymbolKind};
    use projectatlas_core::telemetry::UsageEvent;
    use projectatlas_core::{IndexGeneration, Node, NodeKind, PurposeSource};
    use serde_json::json;
    use std::error::Error;
    use std::fs;
    use std::path::Path;

    const CONTRACT: &str = "snapshot-test-contract";
    const PRIVATE_SENTINEL: &str = "TOP_SECRET_SNAPSHOT_SENTINEL";
    const DELETED_PRIVATE_SENTINEL: &str = "DELETED_SNAPSHOT_PAGE_SENTINEL";

    fn node(
        path: &str,
        kind: NodeKind,
        parent_path: Option<&str>,
        content_hash: Option<&str>,
    ) -> Node {
        Node {
            path: path.to_string(),
            kind,
            parent_path: parent_path.map(str::to_string),
            extension: (kind == NodeKind::File).then(|| ".rs".to_string()),
            language: (kind == NodeKind::File).then(|| "rust".to_string()),
            size_bytes: (kind == NodeKind::File).then_some(32),
            mtime_ns: (kind == NodeKind::File).then_some(1),
            content_hash: content_hash.map(str::to_string),
        }
    }

    fn open_store(root: &Path) -> Result<AtlasStore, Box<dyn Error>> {
        let atlas = root.join(".projectatlas");
        fs::create_dir_all(&atlas)?;
        Ok(AtlasStore::open_for_project(
            &atlas.join("projectatlas.db"),
            root,
        )?)
    }

    fn publish_fixture(
        store: &mut AtlasStore,
        content_hash: &str,
        with_graph: bool,
    ) -> Result<(), Box<dyn Error>> {
        let project = store
            .project_instance_id()?
            .ok_or("fixture project identity is missing")?;
        let generation = IndexGeneration::new(1);
        let mut publication = store.begin_index_publication(CONTRACT)?;
        publication.begin_scan_replacement()?;
        publication.upsert_scan_node_batch(&[
            node(".", NodeKind::Folder, None, None),
            node("src", NodeKind::Folder, Some("."), None),
            node(
                "src/lib.rs",
                NodeKind::File,
                Some("src"),
                Some(content_hash),
            ),
        ])?;
        publication.finish_scan_replacement()?;
        if with_graph {
            let project_entity = GraphEntity::new(project, EntitySelector::Project, generation)?;
            let file = GraphEntity::new(
                project,
                EntitySelector::File {
                    path: RepositoryFilePath::new(Path::new("src/lib.rs"))?,
                },
                generation,
            )?;
            let symbol = GraphEntity::new(
                project,
                EntitySelector::Symbol {
                    symbol: SymbolSelector {
                        file: RepositoryFilePath::new(Path::new("src/lib.rs"))?,
                        name: GraphIdentityText::new("answer")?,
                        kind: SymbolKind::Function,
                        parent: None,
                        signature: GraphIdentityText::new("fn answer() -> u32")?,
                    },
                },
                generation,
            )?;
            let relation = LogicalRelation::new(
                &file,
                GraphRelationKind::Legacy(RelationKind::Calls),
                RelationResolution::resolved(&symbol)?,
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?;
            let occurrence = RelationOccurrence::new(
                &relation,
                RepositoryFilePath::new(Path::new("src/lib.rs"))?,
                SourceSpan::new(1, 0, 1, 6)?,
                generation,
            )?;
            let coverage = CoverageRecord::new(
                CoverageScope::Project,
                None,
                CoverageState::Complete,
                1,
                0,
                generation,
                None,
                None,
            )?;
            let key = CanonicalResolutionKey::new(
                project,
                ResolutionKeyDomain::Declaration,
                &GraphIdentityText::new("tree-sitter")?,
                &GraphIdentityText::new("rust")?,
                None,
                Some(&GraphIdentityText::new("crate")?),
                Some(GraphRelationKind::Legacy(RelationKind::Calls)),
                &GraphIdentityText::new("answer")?,
            );
            publication.replace_repository_graph_with_resolution_keys(
                project,
                &[project_entity, file, symbol.clone()],
                std::slice::from_ref(&relation),
                &[occurrence],
                &[coverage],
                &[EntityResolutionKey::new(symbol.key().clone(), key.clone())?],
                &[RelationDependencyKey::new(relation.key().clone(), key)?],
            )?;
        } else {
            publication.replace_repository_graph_with_resolution_keys(
                project,
                &[],
                &[],
                &[],
                &[],
                &[],
                &[],
            )?;
        }
        publication.complete()?;
        Ok(())
    }

    fn seed_private_state(store: &AtlasStore) -> Result<(), Box<dyn Error>> {
        store.set_purpose("src/lib.rs", PRIVATE_SENTINEL, PurposeSource::Agent)?;
        store.connection.execute(
            "INSERT INTO health_resolutions(
                 finding_id, category, path, related_path, rationale
             ) VALUES('snapshot-private-health', 'snapshot-private', 'src/lib.rs', NULL, ?1)",
            [PRIVATE_SENTINEL],
        )?;
        let usage = serde_json::from_value::<UsageEvent>(json!({
            "session_id": "snapshot-private",
            "command": "summary",
            "query": PRIVATE_SENTINEL
        }))?;
        store.record_usage(&usage)?;
        store.connection.execute(
            "INSERT INTO metadata(key, value) VALUES('snapshot.private.setting', ?1)",
            [PRIVATE_SENTINEL],
        )?;
        store.connection.execute_batch(
            "CREATE TABLE future_memory_atlas(secret TEXT);
             INSERT INTO future_memory_atlas(secret)
             VALUES('TOP_SECRET_SNAPSHOT_SENTINEL');",
        )?;
        store.connection.execute(
            "INSERT INTO metadata(key, value) VALUES('snapshot.deleted.secret', ?1)",
            [DELETED_PRIVATE_SENTINEL],
        )?;
        store.connection.execute(
            "DELETE FROM metadata WHERE key = 'snapshot.deleted.secret'",
            [],
        )?;
        Ok(())
    }

    #[test]
    fn malformed_snapshot_json_is_rejected_before_use() {
        let result = DerivedGraphSnapshot::from_json(br#"{"format_version":1}"#);
        assert!(result.is_err());
        assert!(invalid::<()>("fixture").is_err());
    }

    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn derived_snapshot_excludes_private_state_and_rebinds_atomically() -> Result<(), Box<dyn Error>>
    {
        let source_root = tempfile::tempdir()?;
        let destination_root = tempfile::tempdir()?;
        let mut source = open_store(source_root.path())?;
        let mut destination = open_store(destination_root.path())?;
        publish_fixture(&mut source, "same-content", true)?;
        publish_fixture(&mut destination, "same-content", false)?;
        seed_private_state(&source)?;
        seed_private_state(&destination)?;

        let source_identity = source
            .project_instance_id()?
            .ok_or("source identity is missing")?;
        let destination_identity = destination
            .project_instance_id()?
            .ok_or("destination identity is missing")?;
        assert_ne!(source_identity, destination_identity);

        let exported = source.export_derived_graph_snapshot()?;
        let encoded = exported.to_json()?;
        let encoded_text = String::from_utf8(encoded.clone())?;
        assert!(!encoded_text.contains(PRIVATE_SENTINEL));
        assert!(!encoded_text.contains(DELETED_PRIVATE_SENTINEL));
        assert!(!encoded_text.contains(&source_identity.to_string()));
        let escaped_source_root =
            serde_json::to_string(source_root.path().to_string_lossy().as_ref())?;
        assert!(!encoded_text.contains(escaped_source_root.trim_matches('"')));
        assert_eq!(
            exported.content().iter().map(|row| row.rows).sum::<u64>(),
            8
        );

        let decoded = DerivedGraphSnapshot::from_json(&encoded)?;
        let report = destination.import_derived_graph_snapshot(&decoded)?;
        assert_eq!(report.previous_generation, IndexGeneration::new(1));
        assert_eq!(report.published_generation, IndexGeneration::new(2));
        assert_eq!(
            destination.project_instance_id()?,
            Some(destination_identity)
        );
        let entities = destination.repository_graph_entities_by_path(
            destination_identity,
            &projectatlas_core::graph::RepositoryNodePath::new(Path::new("src/lib.rs"))?,
            10,
        )?;
        assert_eq!(entities.rows.len(), 2);
        let indexed = destination.load_nodes_by_paths(&["src/lib.rs".to_string()])?;
        assert_eq!(
            indexed[0].purpose.purpose.as_deref(),
            Some(PRIVATE_SENTINEL)
        );
        assert_eq!(
            destination.connection.query_row(
                "SELECT rationale FROM health_resolutions
                  WHERE finding_id = 'snapshot-private-health'",
                [],
                |row| row.get::<_, String>(0),
            )?,
            PRIVATE_SENTINEL
        );
        assert_eq!(
            destination.connection.query_row(
                "SELECT value FROM metadata WHERE key = 'snapshot.private.setting'",
                [],
                |row| row.get::<_, String>(0),
            )?,
            PRIVATE_SENTINEL
        );
        assert_eq!(
            destination.connection.query_row(
                "SELECT query FROM usage_events ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )?,
            PRIVATE_SENTINEL
        );
        assert_eq!(
            destination.connection.query_row(
                "SELECT secret FROM future_memory_atlas",
                [],
                |row| row.get::<_, String>(0),
            )?,
            PRIVATE_SENTINEL
        );
        Ok(())
    }

    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn derived_snapshot_rejects_tampering_and_source_mismatch_without_publication()
    -> Result<(), Box<dyn Error>> {
        let source_root = tempfile::tempdir()?;
        let destination_root = tempfile::tempdir()?;
        let mut source = open_store(source_root.path())?;
        let mut destination = open_store(destination_root.path())?;
        publish_fixture(&mut source, "source-content", true)?;
        publish_fixture(&mut destination, "different-content", false)?;

        let snapshot = source.export_derived_graph_snapshot()?;
        let mut tampered = snapshot;
        tampered.metadata.source_state_digest =
            "0".repeat(tampered.metadata.source_state_digest.len());
        assert!(tampered.to_json().is_err());

        let mut internally_consistent = tampered;
        internally_consistent.digest = snapshot_digest(
            &internally_consistent.metadata,
            &internally_consistent.content,
            &internally_consistent.graph,
        )?;
        let before = destination
            .index_publication()?
            .ok_or("publication missing")?;
        assert!(
            destination
                .import_derived_graph_snapshot(&internally_consistent)
                .is_err()
        );
        assert_eq!(
            destination
                .index_publication()?
                .ok_or("publication missing")?,
            before
        );
        assert_eq!(
            destination.repository_graph_generation()?,
            Some(before.generation)
        );
        Ok(())
    }
}
