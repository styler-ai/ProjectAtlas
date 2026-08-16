//! Portable, derived-only repository graph snapshots.

use crate::project_identity::{load_graph_generation, load_project_identity};
use crate::repository_graph;
use crate::schema::SCHEMA_VERSION;
use crate::{
    AtlasStore, DbError, DbResult, FileContentClassification, IndexPublicationState,
    MAX_FILE_CONTENT_CLASSIFICATION_PATHS, load_index_publication,
};
use blake3::Hasher;
use projectatlas_core::IndexGeneration;
use projectatlas_core::graph::{
    CanonicalResolutionKey, Completeness, ConfidenceClass, CoverageRecord, CoverageScope,
    CoverageState, DocumentTargetUnresolvedReason, EntityResolutionKey, EntitySelector,
    ExtendedRelationKind, GraphEntity, GraphIdentityText, GraphLimitKind, GraphRelationKind,
    LogicalRelation, LogicalRelationKey, PortableResolutionKey, ProjectInstanceId,
    RelationDependencyKey, RelationOccurrence, RelationResolution, RepositoryFilePath, SourceSpan,
};
use projectatlas_core::language::ContentClassification;
use rusqlite::Connection;
use rusqlite::backup::Backup;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
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
/// Conservative retained allocation charged for each decoded object.
const DERIVED_SNAPSHOT_DECODE_OBJECT_BYTES: u64 = 128;
/// Retained allocation charged for each decoded sequence or string header.
const DERIVED_SNAPSHOT_DECODE_HEADER_BYTES: u64 = 24;
/// Retained allocation charged for one primitive value.
const DERIVED_SNAPSHOT_DECODE_PRIMITIVE_BYTES: u64 = 16;
/// Maximum raw bytes admitted for one JSON string before serde may allocate it.
const MAX_DERIVED_SNAPSHOT_JSON_STRING_BYTES: usize = 256 * 1024;
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
        require_decode_budget(&encoded)?;
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
        require_decode_budget(encoded)?;
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
    /// Captured file classifications in exact repository-path order.
    pub(crate) file_classifications: Vec<FileContentClassification>,
    /// Captured graph entities.
    pub(crate) entities: Vec<GraphEntity>,
    /// Captured logical relations.
    pub(crate) relations: Vec<LogicalRelation>,
    /// Closed reasons for captured unresolved document relations.
    pub(crate) document_unresolved_reasons: Vec<([u8; 32], DocumentTargetUnresolvedReason)>,
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
        snapshot_from_stable_capture(&capture)
    }

    /// Build a portable graph snapshot from a private stable database copy.
    pub(crate) fn export_derived_graph_snapshot_from_stable_copy(
        &self,
    ) -> DbResult<DerivedGraphSnapshot> {
        snapshot_from_stable_capture(&self.connection)
    }

    /// Rebind and publish a source-exact baseline into a detached hydration candidate.
    pub(crate) fn import_worktree_hydration_snapshot(
        &mut self,
        snapshot: &DerivedGraphSnapshot,
    ) -> DbResult<DerivedGraphSnapshotImport> {
        snapshot.validate()?;
        if self.index_publication()?.is_some()
            || load_graph_generation(&self.connection)? != Some(IndexGeneration::ZERO)
        {
            return invalid("worktree hydration destination is already published");
        }
        if source_state_digest(&self.connection)? != snapshot.metadata.source_state_digest {
            return invalid("worktree hydration source state changed before rebinding");
        }
        let project = self
            .project_instance_id()?
            .ok_or(DbError::ProjectInstanceIdentityMissing)?;
        self.publish_snapshot(snapshot, project, IndexGeneration::ZERO, false, || Ok(()))
    }

    /// Publish a validated graph snapshot through the full or projection contract.
    fn publish_snapshot(
        &mut self,
        snapshot: &DerivedGraphSnapshot,
        project: ProjectInstanceId,
        base_generation: IndexGeneration,
        projection: bool,
        before_publication: impl FnOnce() -> DbResult<()>,
    ) -> DbResult<DerivedGraphSnapshotImport> {
        let next_generation = base_generation
            .checked_next()
            .ok_or(DbError::PublicationGenerationOverflow)?;
        let graph = snapshot.graph.bind(project, next_generation)?;
        validate_snapshot_classification_coverage(&self.connection, &graph.file_classifications)?;
        before_publication()?;
        let mut guard = if projection {
            self.begin_index_projection_refresh_from(
                &snapshot.metadata.capability_fingerprint,
                base_generation,
            )?
        } else {
            self.begin_index_publication_from(
                &snapshot.metadata.capability_fingerprint,
                base_generation,
            )?
        };
        if source_state_digest(&guard.connection)? != snapshot.metadata.source_state_digest {
            return invalid("destination source state does not match the snapshot");
        }
        validate_snapshot_classification_coverage(&guard.connection, &graph.file_classifications)?;
        for rows in graph
            .file_classifications
            .chunks(MAX_FILE_CONTENT_CLASSIFICATION_PATHS)
        {
            guard.upsert_file_content_classification_batch(rows)?;
        }
        guard.replace_repository_graph_with_resolution_keys(
            project,
            &graph.entities,
            &graph.relations,
            &graph.occurrences,
            &graph.coverage,
            &graph.entity_exports,
            &graph.relation_dependencies,
        )?;
        guard.set_document_unresolved_reasons(&graph.document_unresolved_reasons)?;
        guard.complete()?;
        Ok(DerivedGraphSnapshotImport {
            previous_generation: base_generation,
            published_generation: next_generation,
            digest: snapshot.digest.clone(),
            content: snapshot.content.clone(),
        })
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
        self.import_derived_graph_snapshot_with_prepublication(snapshot, || Ok(()))
    }

    /// Import with one internal seam immediately before publication locking.
    fn import_derived_graph_snapshot_with_prepublication(
        &mut self,
        snapshot: &DerivedGraphSnapshot,
        before_publication: impl FnOnce() -> DbResult<()>,
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
        self.publish_snapshot(
            snapshot,
            project,
            publication.generation,
            true,
            before_publication,
        )
    }
}

/// Validate and decode one stable private `SQLite` capture.
fn snapshot_from_stable_capture(capture: &Connection) -> DbResult<DerivedGraphSnapshot> {
    require_private_capture_size(capture)?;
    let quick_check = capture.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))?;
    if quick_check != "ok" {
        return invalid("private SQLite capture failed integrity check");
    }

    let publication = load_index_publication(capture)?
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
    let project = load_project_identity(capture)?.ok_or(DbError::ProjectInstanceIdentityMissing)?;
    if load_graph_generation(capture)? != Some(publication.generation) {
        return invalid("private capture graph generation is not complete");
    }
    let source_state_digest = source_state_digest(capture)?;
    let mut budget = SnapshotBudget::new();
    let mut captured = repository_graph::capture_derived_graph(
        capture,
        project,
        publication.generation,
        &mut budget,
    )?;
    captured.file_classifications = capture_file_classifications(capture, &mut budget)?;
    DerivedGraphSnapshot::from_capture(
        captured,
        publication.generation,
        source_state_digest,
        capability_fingerprint,
    )
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
    /// One closed content role for every admitted repository file.
    file_classifications: Vec<PortableFileClassification>,
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
        let file_classifications = captured
            .file_classifications
            .into_iter()
            .map(PortableFileClassification::from)
            .collect();
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
        let document_unresolved_reasons = captured
            .document_unresolved_reasons
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        let entities = captured
            .entities
            .iter()
            .map(|entity| entity.selector().clone())
            .collect();
        let relations = captured
            .relations
            .iter()
            .map(|relation| {
                PortableRelation::from_relation(
                    relation,
                    document_unresolved_reasons
                        .get(&relation.key().digest_bytes()?)
                        .copied(),
                    &entity_indexes,
                )
            })
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
            file_classifications,
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
            self.file_classifications.len(),
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
        let mut classified_paths = BTreeSet::new();
        for row in &self.file_classifications {
            RepositoryFilePath::new(std::path::Path::new(&row.path))?;
            if !classified_paths.insert(row.path.as_str()) {
                return invalid("snapshot contains duplicate file classifications");
            }
        }
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
            let requires_reason = relation.kind
                == GraphRelationKind::Extended(ExtendedRelationKind::Documents)
                && matches!(
                    relation.resolution,
                    PortableRelationResolution::Unresolved { .. }
                );
            if requires_reason != relation.document_unresolved_reason.is_some() {
                return invalid(
                    "snapshot document reason contradicts relation family or resolution",
                );
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
        let file_classifications = self
            .file_classifications
            .iter()
            .cloned()
            .map(FileContentClassification::from)
            .collect();
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
        let document_unresolved_reasons = self
            .relations
            .iter()
            .zip(&relations)
            .filter_map(|(portable, relation)| {
                portable
                    .document_unresolved_reason
                    .map(|reason| (relation.key().clone(), reason))
            })
            .collect();
        Ok(BoundGraph {
            file_classifications,
            entities,
            relations,
            occurrences,
            coverage,
            entity_exports,
            relation_dependencies,
            document_unresolved_reasons,
        })
    }
}

/// Project-independent file classification row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PortableFileClassification {
    /// Exact repository-relative file path.
    path: String,
    /// Registry-owned closed content role.
    classification: ContentClassification,
}

impl From<FileContentClassification> for PortableFileClassification {
    fn from(row: FileContentClassification) -> Self {
        Self {
            path: row.path,
            classification: row.classification,
        }
    }
}

impl From<PortableFileClassification> for FileContentClassification {
    fn from(row: PortableFileClassification) -> Self {
        Self {
            path: row.path,
            classification: row.classification,
        }
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
    /// Closed reason retained only for an unresolved document relation.
    #[serde(skip_serializing_if = "Option::is_none")]
    document_unresolved_reason: Option<DocumentTargetUnresolvedReason>,
}

impl PortableRelation {
    /// Convert one project-bound relation to portable entity indexes.
    fn from_relation(
        relation: &LogicalRelation,
        document_unresolved_reason: Option<DocumentTargetUnresolvedReason>,
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
            document_unresolved_reason,
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
    /// Destination-path file classifications.
    file_classifications: Vec<FileContentClassification>,
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
    /// Destination-bound closed unresolved-document reasons.
    document_unresolved_reasons: Vec<(LogicalRelationKey, DocumentTargetUnresolvedReason)>,
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
        content(
            "file_content_classifications",
            &["path", "classification"],
            graph.file_classifications.len(),
        )?,
        content("graph_entities", &["entity_selector"], graph.entities.len())?,
        content(
            "graph_relations",
            &[
                "source_entity",
                "relation_kind",
                "resolution",
                "document_unresolved_reason",
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

/// Capture the complete closed file-role projection from the private backup.
fn capture_file_classifications(
    connection: &Connection,
    budget: &mut SnapshotBudget,
) -> DbResult<Vec<FileContentClassification>> {
    let mut statement = connection.prepare(
        "SELECT path, classification
           FROM file_content_classifications
          ORDER BY path",
    )?;
    let mut rows = statement.query([])?;
    let mut captured = Vec::new();
    while let Some(row) = rows.next()? {
        let path = row.get::<_, String>(0)?;
        let raw = row.get::<_, String>(1)?;
        let classification =
            ContentClassification::from_db(&raw).ok_or_else(|| DbError::InvalidEnum {
                field: "file_content_classifications.classification",
                value: raw.clone(),
            })?;
        budget.admit(
            usize_to_u64(path.len())?
                .saturating_add(usize_to_u64(raw.len())?)
                .saturating_add(DERIVED_SNAPSHOT_DECODE_OBJECT_BYTES),
        )?;
        captured.push(FileContentClassification {
            path,
            classification,
        });
    }
    Ok(captured)
}

/// Require snapshot classifications to cover the destination's exact current file set.
fn validate_snapshot_classification_coverage(
    connection: &Connection,
    classifications: &[FileContentClassification],
) -> DbResult<()> {
    let mut statement = connection.prepare(
        "SELECT path
           FROM nodes
          WHERE exists_now = 1 AND kind = 'file'
          ORDER BY path",
    )?;
    let mut rows = statement.query([])?;
    let mut index = 0_usize;
    while let Some(row) = rows.next()? {
        let path = row.get::<_, String>(0)?;
        if classifications.get(index).map(|row| row.path.as_str()) != Some(path.as_str()) {
            return invalid("snapshot classifications do not exactly cover current files");
        }
        index = index.saturating_add(1);
    }
    if index != classifications.len() {
        return invalid("snapshot classifications do not exactly cover current files");
    }
    Ok(())
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

/// Reject JSON shapes whose decoded allocation estimate exceeds the snapshot ceiling.
fn require_decode_budget(encoded: &[u8]) -> DbResult<()> {
    let mut retained_bytes = 0_u64;
    let mut string_bytes = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut in_primitive = false;

    for byte in encoded {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
                in_primitive = true;
                admit_decode_bytes(&mut retained_bytes, usize_to_u64(string_bytes)?)?;
                string_bytes = 0;
                continue;
            }
            string_bytes = string_bytes
                .checked_add(1)
                .ok_or(DbError::DerivedSnapshotInvalid {
                    reason: "snapshot JSON string size overflowed",
                })?;
            if string_bytes > MAX_DERIVED_SNAPSHOT_JSON_STRING_BYTES {
                return Err(DbError::DerivedSnapshotLimit {
                    resource: "encoded JSON string bytes",
                    found: usize_to_u64(string_bytes)?,
                    maximum: usize_to_u64(MAX_DERIVED_SNAPSHOT_JSON_STRING_BYTES)?,
                });
            }
            continue;
        }

        match *byte {
            b'"' => {
                in_string = true;
                escaped = false;
                in_primitive = false;
                admit_decode_bytes(&mut retained_bytes, DERIVED_SNAPSHOT_DECODE_HEADER_BYTES)?;
            }
            b'{' => {
                in_primitive = false;
                admit_decode_bytes(&mut retained_bytes, DERIVED_SNAPSHOT_DECODE_OBJECT_BYTES)?;
            }
            b'[' => {
                in_primitive = false;
                admit_decode_bytes(&mut retained_bytes, DERIVED_SNAPSHOT_DECODE_HEADER_BYTES)?;
            }
            b',' | b':' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n' => {
                in_primitive = false;
            }
            _ if !in_primitive => {
                in_primitive = true;
                admit_decode_bytes(&mut retained_bytes, DERIVED_SNAPSHOT_DECODE_PRIMITIVE_BYTES)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Admit one conservative decoded-allocation estimate.
fn admit_decode_bytes(retained_bytes: &mut u64, bytes: u64) -> DbResult<()> {
    *retained_bytes = retained_bytes
        .checked_add(bytes)
        .ok_or(DbError::DerivedSnapshotInvalid {
            reason: "snapshot retained byte count overflowed",
        })?;
    require_limit(
        "decoded retained bytes",
        *retained_bytes,
        MAX_DERIVED_SNAPSHOT_RETAINED_BYTES,
    )
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
    use super::{
        DerivedGraphSnapshot, MAX_DERIVED_SNAPSHOT_JSON_STRING_BYTES, expected_content, invalid,
        snapshot_digest,
    };
    use crate::{AtlasStore, DbError, FileContentClassification};
    use projectatlas_core::graph::{
        CanonicalResolutionKey, Completeness, ConfidenceClass, CoverageRecord, CoverageScope,
        CoverageState, DocumentTargetUnresolvedReason, EntityResolutionKey, EntitySelector,
        ExtendedRelationKind, GraphEntity, GraphIdentityText, GraphRelationKind, LogicalRelation,
        RelationDependencyKey, RelationOccurrence, RelationResolution, RepositoryFilePath,
        ResolutionKeyDomain, SourceSpan, SymbolSelector,
    };
    use projectatlas_core::language::ContentClassification;
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
        publication.upsert_file_content_classification_batch(&[FileContentClassification {
            path: "src/lib.rs".to_string(),
            classification: if with_graph {
                ContentClassification::Source
            } else {
                ContentClassification::Documentation
            },
        }])?;
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
            let unresolved_document = LogicalRelation::new(
                &file,
                GraphRelationKind::Extended(ExtendedRelationKind::Documents),
                RelationResolution::Unresolved {
                    reference: GraphIdentityText::new("docs/missing.md")?,
                },
                ConfidenceClass::High,
                Completeness::Complete,
                generation,
            )?;
            let occurrence = RelationOccurrence::new(
                &relation,
                RepositoryFilePath::new(Path::new("src/lib.rs"))?,
                SourceSpan::new(1, 0, 1, 6)?,
                generation,
            )?;
            let document_occurrence = RelationOccurrence::new(
                &unresolved_document,
                RepositoryFilePath::new(Path::new("src/lib.rs"))?,
                SourceSpan::new(2, 0, 2, 15)?,
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
                &[relation.clone(), unresolved_document.clone()],
                &[occurrence, document_occurrence],
                &[coverage],
                &[EntityResolutionKey::new(symbol.key().clone(), key.clone())?],
                &[RelationDependencyKey::new(relation.key().clone(), key)?],
            )?;
            publication.set_document_unresolved_reasons(&[(
                unresolved_document.key().clone(),
                DocumentTargetUnresolvedReason::Missing,
            )])?;
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
    fn snapshot_json_decode_budget_rejects_large_shapes_before_deserialization() {
        let mut wide = String::from("[");
        for index in 0..2_100_000 {
            if index != 0 {
                wide.push(',');
            }
            wide.push_str("{}");
        }
        wide.push(']');
        assert!(matches!(
            DerivedGraphSnapshot::from_json(wide.as_bytes()),
            Err(DbError::DerivedSnapshotLimit {
                resource: "decoded retained bytes",
                ..
            })
        ));

        let oversized_string = format!(
            r#"{{"value":"{}"}}"#,
            "x".repeat(MAX_DERIVED_SNAPSHOT_JSON_STRING_BYTES + 1)
        );
        assert!(matches!(
            DerivedGraphSnapshot::from_json(oversized_string.as_bytes()),
            Err(DbError::DerivedSnapshotLimit {
                resource: "encoded JSON string bytes",
                ..
            })
        ));
    }

    #[test]
    fn large_valid_snapshot_round_trips_without_charging_json_delimiters()
    -> Result<(), Box<dyn Error>> {
        let source_root = tempfile::tempdir()?;
        let mut source = open_store(source_root.path())?;
        publish_fixture(&mut source, "large-round-trip", true)?;
        let mut snapshot = source.export_derived_graph_snapshot()?;
        let coverage = snapshot
            .graph
            .coverage
            .first()
            .ok_or("fixture coverage is missing")?
            .clone();
        snapshot.graph.coverage = vec![coverage; 40_000];
        snapshot.content = expected_content(&snapshot.graph)?;
        snapshot.digest = snapshot_digest(&snapshot.metadata, &snapshot.content, &snapshot.graph)?;

        let encoded = snapshot.to_json()?;
        let decoded = DerivedGraphSnapshot::from_json(&encoded)?;
        if decoded != snapshot {
            return Err("large valid snapshot changed during round trip".into());
        }
        Ok(())
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
            11
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
        assert_eq!(
            destination.file_content_classifications_for_paths(&["src/lib.rs".to_string()])?[0]
                .classification,
            ContentClassification::Source
        );
        assert_eq!(
            destination.connection.query_row(
                "SELECT COUNT(*) FROM graph_relations
                  WHERE relation_scope = 'extended'
                    AND relation_kind = 'documents'
                    AND resolution_status = 'unresolved'
                    AND document_unresolved_reason = 'missing'",
                [],
                |row| row.get::<_, u64>(0),
            )?,
            1
        );
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
    fn derived_snapshot_rejects_document_reason_drift_and_rolls_back_update_failure()
    -> Result<(), Box<dyn Error>> {
        let source_root = tempfile::tempdir()?;
        let destination_root = tempfile::tempdir()?;
        let mut source = open_store(source_root.path())?;
        let mut destination = open_store(destination_root.path())?;
        publish_fixture(&mut source, "same-content", true)?;
        publish_fixture(&mut destination, "same-content", false)?;

        let snapshot = source.export_derived_graph_snapshot()?;
        let mut malformed = snapshot.clone();
        let document = malformed
            .graph
            .relations
            .iter_mut()
            .find(|relation| {
                relation.kind == GraphRelationKind::Extended(ExtendedRelationKind::Documents)
            })
            .ok_or("document relation is missing")?;
        document.document_unresolved_reason = None;
        malformed.content = expected_content(&malformed.graph)?;
        malformed.digest =
            snapshot_digest(&malformed.metadata, &malformed.content, &malformed.graph)?;
        assert!(matches!(
            malformed.to_json(),
            Err(DbError::DerivedSnapshotInvalid {
                reason: "snapshot document reason contradicts relation family or resolution"
            })
        ));

        let mut forbidden = snapshot.clone();
        let calls = forbidden
            .graph
            .relations
            .iter_mut()
            .find(|relation| relation.kind == GraphRelationKind::Legacy(RelationKind::Calls))
            .ok_or("calls relation is missing")?;
        calls.document_unresolved_reason = Some(DocumentTargetUnresolvedReason::Missing);
        forbidden.content = expected_content(&forbidden.graph)?;
        forbidden.digest =
            snapshot_digest(&forbidden.metadata, &forbidden.content, &forbidden.graph)?;
        assert!(matches!(
            forbidden.to_json(),
            Err(DbError::DerivedSnapshotInvalid {
                reason: "snapshot document reason contradicts relation family or resolution"
            })
        ));

        destination.connection.execute_batch(
            "CREATE TEMP TRIGGER fail_snapshot_document_reason
             BEFORE UPDATE OF document_unresolved_reason ON graph_relations
             BEGIN
                 SELECT RAISE(ABORT, 'injected snapshot reason failure');
             END;",
        )?;
        let before = destination
            .index_publication()?
            .ok_or("destination publication is missing")?;
        assert!(
            destination
                .import_derived_graph_snapshot(&snapshot)
                .is_err()
        );
        assert_eq!(
            destination
                .index_publication()?
                .ok_or("destination publication is missing")?,
            before
        );
        assert_eq!(
            destination.connection.query_row(
                "SELECT COUNT(*) FROM graph_relations",
                [],
                |row| { row.get::<_, u64>(0) }
            )?,
            0
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

    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn derived_snapshot_rechecks_source_state_inside_publication_transaction()
    -> Result<(), Box<dyn Error>> {
        let source_root = tempfile::tempdir()?;
        let destination_root = tempfile::tempdir()?;
        let mut source = open_store(source_root.path())?;
        let mut destination = open_store(destination_root.path())?;
        publish_fixture(&mut source, "same-content", true)?;
        publish_fixture(&mut destination, "same-content", false)?;
        let mut concurrent = open_store(destination_root.path())?;
        let snapshot = source.export_derived_graph_snapshot()?;
        let before = destination
            .index_publication()?
            .ok_or("destination publication is missing")?;

        let result =
            destination.import_derived_graph_snapshot_with_prepublication(&snapshot, || {
                concurrent.upsert_scan_nodes(&[node(
                    "src/lib.rs",
                    NodeKind::File,
                    Some("src"),
                    Some("concurrent-content"),
                )])
            });
        assert!(matches!(
            result,
            Err(DbError::DerivedSnapshotInvalid {
                reason: "destination source state does not match the snapshot"
            })
        ));
        assert_eq!(
            destination
                .index_publication()?
                .ok_or("destination publication is missing")?,
            before
        );
        assert_eq!(
            destination.repository_graph_generation()?,
            Some(before.generation)
        );
        assert_eq!(
            destination.load_nodes_by_paths(&["src/lib.rs".to_string()])?[0]
                .node
                .content_hash
                .as_deref(),
            Some("concurrent-content")
        );
        Ok(())
    }
}
