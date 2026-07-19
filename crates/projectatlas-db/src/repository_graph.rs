//! Normalized repository-graph persistence and bounded prepared queries.

use super::{AtlasStore, DbError, DbResult, IndexPublicationGuard, IndexPublicationState};
use projectatlas_core::IndexGeneration;
use projectatlas_core::graph::{
    Completeness, ConfidenceClass, CoverageRecord, CoverageScope, CoverageState, EntitySelector,
    ExtendedRelationKind, ExternalSelector, GraphEntity, GraphEntityKey, GraphIdentityText,
    GraphLimitKind, GraphLimits, GraphRelationKind, LogicalRelation, PackageSelector,
    ProjectInstanceId, RelationOccurrence, RelationResolution, RepositoryFilePath,
    RepositoryNodePath, SourceSpan, SymbolSelector,
};
use projectatlas_core::symbols::{RelationKind, SymbolKind};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::path::Path;

/// One bounded page of typed normalized graph rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryGraphPage<T> {
    /// Fully validated rows in deterministic storage order.
    pub rows: Vec<T>,
    /// Whether at least one additional validated row exists.
    pub truncated: bool,
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
}

impl AtlasStore {
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
        load_entity_by_digest(self, key.project(), &key.digest_bytes()?, generation)
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
        let raw = {
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
            collect_entity_rows(statement.query(params![
                &project.as_bytes()[..],
                path.as_str(),
                limit_plus_one
            ])?)?
        };
        page_from_raw(raw, limit, |row| entity_from_row(row, project, generation))
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
                let raw = self.collect_relation_rows_by_key(
                    "source_entity_key",
                    &source.digest_bytes()?,
                    limit_plus_one,
                )?;
                (project, raw)
            }
            RepositoryGraphRelationQuery::Inbound { target } => {
                let project = target.project();
                if !verify_project_identity(&self.connection, project)? {
                    return Ok(empty_page());
                }
                let raw = self.collect_relation_rows_by_key(
                    "target_entity_key",
                    &target.digest_bytes()?,
                    limit_plus_one,
                )?;
                (project, raw)
            }
            RepositoryGraphRelationQuery::Family { relation } => {
                let project = load_project_identity(&self.connection)?
                    .ok_or(DbError::GraphPublicationUnavailable)?;
                let (scope, kind) = relation_parts(relation);
                let raw = {
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
                    ])?)?
                };
                (project, raw)
            }
        };
        let mut entities = HashMap::new();
        page_from_raw(raw, limit, |row| {
            relation_from_row(self, &mut entities, row, project, generation)
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
                        relation_kind, state, total, covered, omitted, reason, reached_limit
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

    /// Return the complete generation used to reconstruct normalized graph rows.
    fn repository_graph_generation(&self) -> DbResult<Option<IndexGeneration>> {
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
        let generation = self.pending_graph_generation()?;
        validate_graph_batch(
            project,
            generation,
            entities,
            relations,
            occurrences,
            coverage,
        )?;
        let savepoint = self.store.connection.savepoint()?;
        ensure_project_identity(&savepoint, project)?;
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
        update_graph_generation(&savepoint, generation)?;
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
        let generation = self.pending_graph_generation()?;
        validate_graph_batch(
            project,
            generation,
            entities,
            relations,
            occurrences,
            coverage,
        )?;
        let affected_paths = affected_paths
            .iter()
            .map(|path| RepositoryNodePath::new(Path::new(path)))
            .collect::<Result<Vec<_>, _>>()?;
        let savepoint = self.store.connection.savepoint()?;
        ensure_project_identity(&savepoint, project)?;
        if affected_paths.iter().any(|path| path.as_str() == ".") {
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
            update_graph_generation(&savepoint, generation)?;
            savepoint.commit()?;
            return Ok(());
        }
        let mut orphan_candidates = affected_external_candidates(&savepoint, &affected_paths)?;
        invalidate_repository_graph_paths(&savepoint, &affected_paths)?;
        insert_graph_batch(
            &savepoint,
            project,
            entities,
            relations,
            occurrences,
            coverage,
        )?;
        for entity in entities {
            if matches!(entity.selector(), EntitySelector::External { .. }) {
                orphan_candidates.insert(entity.key().digest_bytes()?);
            }
        }
        remove_orphan_external_candidates(&savepoint, &orphan_candidates)?;
        update_graph_generation(&savepoint, generation)?;
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
) -> DbResult<()> {
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
        occurrences.execute(params![path, descendant_start, descendant_end])?;
        coverage.execute(params![path, descendant_start, descendant_end])?;
        entities_by_path.execute(params![path, descendant_start, descendant_end])?;
        entities_by_manifest.execute(params![path, descendant_start, descendant_end])?;
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

/// Bind the normalized graph to the publication that actually replaced it.
fn update_graph_generation(connection: &Connection, generation: IndexGeneration) -> DbResult<()> {
    let generation =
        i64::try_from(generation.get()).map_err(|_source| DbError::GraphCountOverflow {
            field: "project_identity.active_generation",
            value: generation.get(),
        })?;
    connection.execute(
        "UPDATE project_identity SET active_generation = ?1 WHERE singleton = 1",
        [generation],
    )?;
    Ok(())
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

/// Reconstruct one typed logical relation through existing domain constructors.
fn relation_from_row(
    store: &AtlasStore,
    entities: &mut HashMap<[u8; 32], GraphEntity>,
    row: RelationRow,
    expected_project: ProjectInstanceId,
    generation: IndexGeneration,
) -> DbResult<LogicalRelation> {
    let project = project_from_blob("graph_relations.project_instance_id", row.project.clone())?;
    require_project(expected_project, project)?;
    let source_key = fixed_bytes::<32>("graph_relations.source_entity_key", row.source.clone())?;
    let source = load_entity_cached(store, entities, project, source_key, generation)?.ok_or(
        DbError::GraphRowShape {
            table: "graph_relations",
            reason: "source entity is missing",
        },
    )?;
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
            let target = load_entity_cached(store, entities, project, target_key, generation)?
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
            let target = load_entity_cached(store, entities, project, target_key, generation)?
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

/// Load one entity through the stable-key primary index.
fn load_entity_by_digest(
    store: &AtlasStore,
    project: ProjectInstanceId,
    digest: &[u8; 32],
    generation: IndexGeneration,
) -> DbResult<Option<GraphEntity>> {
    let raw = {
        let mut statement = store.connection.prepare_cached(
            "SELECT entity_key, project_instance_id, canonical_identity, entity_kind,
                    repository_path, package_manager, package_name, manifest_path,
                    symbol_name, symbol_kind, symbol_parent, symbol_signature,
                    external_system, external_identity
               FROM graph_entities
              WHERE project_instance_id = ?1 AND entity_key = ?2",
        )?;
        statement
            .query_row(params![&project.as_bytes()[..], &digest[..]], entity_row)
            .optional()?
    };
    raw.map(|row| entity_from_row(row, project, generation))
        .transpose()
}

/// Load an entity once per relation page and reuse its validated domain value.
fn load_entity_cached(
    store: &AtlasStore,
    entities: &mut HashMap<[u8; 32], GraphEntity>,
    project: ProjectInstanceId,
    digest: [u8; 32],
    generation: IndexGeneration,
) -> DbResult<Option<GraphEntity>> {
    if let Some(entity) = entities.get(&digest) {
        return Ok(Some(entity.clone()));
    }
    let entity = load_entity_by_digest(store, project, &digest, generation)?;
    if let Some(entity) = &entity {
        entities.insert(digest, entity.clone());
    }
    Ok(entity)
}

/// Read the graph project singleton, validating its fixed binary identity.
fn load_project_identity(connection: &Connection) -> DbResult<Option<ProjectInstanceId>> {
    let bytes = connection
        .query_row(
            "SELECT project_instance_id FROM project_identity WHERE singleton = 1",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    bytes
        .map(|value| project_from_blob("project_identity.project_instance_id", value))
        .transpose()
}

/// Read the typed graph generation owned by the project singleton.
fn load_graph_generation(connection: &Connection) -> DbResult<Option<IndexGeneration>> {
    let generation = connection
        .query_row(
            "SELECT active_generation FROM project_identity WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    generation
        .map(|value| {
            nonnegative_u64("project_identity.active_generation", value).map(IndexGeneration::new)
        })
        .transpose()
}

/// Return whether the graph singleton exists and matches the selected project.
fn verify_project_identity(connection: &Connection, expected: ProjectInstanceId) -> DbResult<bool> {
    let Some(found) = load_project_identity(connection)? else {
        return Ok(false);
    };
    require_project(expected, found)?;
    Ok(true)
}

/// Initialize or verify the one graph project identity inside publication.
fn ensure_project_identity(connection: &Connection, expected: ProjectInstanceId) -> DbResult<()> {
    connection.execute(
        "INSERT INTO project_identity(singleton, project_instance_id)
         VALUES(1, ?1) ON CONFLICT(singleton) DO NOTHING",
        [&expected.as_bytes()[..]],
    )?;
    let found = load_project_identity(connection)?.ok_or(DbError::GraphRowShape {
        table: "project_identity",
        reason: "singleton insert did not produce an identity",
    })?;
    require_project(expected, found)
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

/// Collect every raw relation row, including the truncation sentinel row.
fn collect_relation_rows(mut rows: rusqlite::Rows<'_>) -> DbResult<Vec<RelationRow>> {
    let mut collected = Vec::new();
    while let Some(row) = rows.next()? {
        collected.push(relation_row(row)?);
    }
    Ok(collected)
}

/// Read one raw relation row without interpreting enum or resolution values.
fn relation_row(row: &Row<'_>) -> rusqlite::Result<RelationRow> {
    Ok(RelationRow {
        key: row.get(0)?,
        project: row.get(1)?,
        canonical: row.get(2)?,
        source: row.get(3)?,
        relation_scope: row.get(4)?,
        relation_kind: row.get(5)?,
        resolution_status: row.get(6)?,
        target: row.get(7)?,
        reference: row.get(8)?,
        candidate_count: row.get(9)?,
        confidence: row.get(10)?,
        completeness: row.get(11)?,
    })
}

/// Read one raw relation occurrence row.
fn occurrence_row(row: &Row<'_>) -> rusqlite::Result<OccurrenceRow> {
    Ok(OccurrenceRow {
        relation: row.get(0)?,
        file_path: row.get(1)?,
        start_line: row.get(2)?,
        start_column: row.get(3)?,
        end_line: row.get(4)?,
        end_column: row.get(5)?,
    })
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
    })
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
fn parse_coverage_state(value: &str) -> DbResult<CoverageState> {
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

/// Return the normalized reached-limit spelling.
const fn limit_kind_name(limit: GraphLimitKind) -> &'static str {
    match limit {
        GraphLimitKind::Rows => "rows",
        GraphLimitKind::Occurrences => "occurrences",
        GraphLimitKind::Depth => "depth",
        GraphLimitKind::OutputBytes => "output_bytes",
    }
}

/// Parse one normalized reached-limit spelling.
fn parse_limit_kind(value: &str) -> DbResult<GraphLimitKind> {
    match value {
        "rows" => Ok(GraphLimitKind::Rows),
        "occurrences" => Ok(GraphLimitKind::Occurrences),
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
    use projectatlas_core::symbols::{ParserKind, SymbolGraph, SymbolRelation};
    use std::error::Error;
    use std::fmt::Debug;
    use std::io;

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

    /// Build one complete typed graph for the selected generation.
    fn graph_fixture(generation: IndexGeneration) -> Result<GraphFixture, Box<dyn Error>> {
        let project = ProjectInstanceId::from_bytes([0x11; 16])?;
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

    /// Seed the local-source nodes required by graph path foreign keys.
    fn seed_nodes(store: &AtlasStore) -> DbResult<()> {
        store.connection.execute_batch(
            "INSERT INTO nodes(path, kind, parent_path) VALUES('.', 'folder', NULL);
             INSERT INTO nodes(path, kind, parent_path) VALUES('src', 'folder', '.');
             INSERT INTO nodes(path, kind, parent_path) VALUES('src/Äuth.rs', 'file', 'src');
             INSERT INTO nodes(path, kind, parent_path) VALUES('Cargo.toml', 'file', '.');",
        )?;
        Ok(())
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

    /// Publish one complete fixture and its lexical source text.
    fn publish_fixture(
        store: &mut AtlasStore,
        fingerprint: &str,
    ) -> Result<GraphFixture, Box<dyn Error>> {
        seed_nodes(store)?;
        store.replace_symbol_graph(&SymbolGraph {
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
        let fixture = graph_fixture(IndexGeneration::new(1))?;
        let mut occurrences = fixture.occurrences.clone();
        occurrences.push(fixture.occurrences[0].clone());
        let mut publication = store.begin_index_publication(fingerprint)?;
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
    fn affected_graph_replacement_preserves_only_the_unaffected_closure()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.connection.execute_batch(
            "INSERT INTO nodes(path, kind, parent_path) VALUES('.', 'folder', NULL);
             INSERT INTO nodes(path, kind, parent_path) VALUES('src', 'folder', '.');
             INSERT INTO nodes(path, kind, parent_path) VALUES('src/a', 'folder', 'src');
             INSERT INTO nodes(path, kind, parent_path) VALUES('src/a/local.rs', 'file', 'src/a');
             INSERT INTO nodes(path, kind, parent_path) VALUES('src/a/new.rs', 'file', 'src/a');
             INSERT INTO nodes(path, kind, parent_path) VALUES('src/A', 'folder', 'src');
             INSERT INTO nodes(path, kind, parent_path) VALUES('src/A/keep.rs', 'file', 'src/A');
             INSERT INTO nodes(path, kind, parent_path) VALUES('packages', 'folder', '.');
             INSERT INTO nodes(path, kind, parent_path) VALUES('packages/api', 'folder', 'packages');
             INSERT INTO nodes(path, kind, parent_path) VALUES('packages/api/Cargo.toml', 'file', 'packages/api');
             INSERT INTO nodes(path, kind, parent_path) VALUES('README.md', 'file', '.');",
        )?;

        let project = ProjectInstanceId::from_bytes([0x22; 16])?;
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
                ],
                &[
                    affected_relation,
                    package_relation,
                    retained_relation,
                    project_external_relation,
                ],
                &[affected_occurrence, retained_occurrence],
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
        let removed_occurrences = store.connection.query_row(
            "SELECT COUNT(*) FROM graph_relation_occurrences WHERE file_path = 'src/a/local.rs'",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        require_eq(
            &removed_occurrences,
            &0,
            "affected source occurrence removal",
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
        let mut store = AtlasStore::in_memory()?;
        let fixture = publish_fixture(&mut store, "typed-graph")?;

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

        let mut publication = store.begin_index_publication("typed-graph")?;
        publication.replace_repository_graph_for_paths(
            fixture.project,
            &["src/unrelated.rs".to_string()],
            &[],
            &[],
            &[],
            &[],
        )?;
        publication.complete()?;
        let reused = store
            .repository_graph_entity(source.key())?
            .ok_or_else(|| io::Error::other("unchanged entity disappeared"))?;
        require_eq(
            &reused.generation(),
            &IndexGeneration::new(2),
            "unchanged graph row generation injection",
        )?;
        require_eq(
            &store
                .connection
                .query_row("SELECT COUNT(*) FROM graph_entities", [], |row| {
                    row.get::<_, i64>(0)
                })?,
            &6,
            "incremental graph row reuse",
        )?;
        assert_query_indexes(&store)?;
        Ok(())
    }

    #[test]
    fn graph_queries_fail_closed_on_corrupt_normalized_rows() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let fixture = publish_fixture(&mut store, "graph-corruption")?;
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
        let error = require_db_error(
            store.repository_graph_entity(source.key()),
            "malformed graph enum was accepted",
        )?;
        require(
            matches!(error, DbError::InvalidEnum { .. }),
            &format!("unexpected malformed-enum error: {error}"),
        )?;
        store.connection.execute(
            "UPDATE graph_entities SET entity_kind = 'file' WHERE entity_key = ?1",
            [&source_digest[..]],
        )?;

        store.connection.execute(
            "UPDATE graph_relations SET candidate_count = 0 WHERE relation_key = ?1",
            [&ambiguous_digest[..]],
        )?;
        let error = require_db_error(
            store.repository_graph_relations(
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
        store.connection.execute(
            "UPDATE graph_relations SET candidate_count = 2 WHERE relation_key = ?1",
            [&ambiguous_digest[..]],
        )?;

        store.connection.execute(
            "UPDATE graph_relations SET resolution_status = 'resolved'
              WHERE relation_key = ?1",
            [&ambiguous_digest[..]],
        )?;
        let error = require_db_error(
            store.repository_graph_relations(
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
        let error = require_db_error(
            store.repository_graph_coverage(fixture.project, &CoverageScope::Project, 10),
            "contradictory coverage total was accepted",
        )?;
        require(
            matches!(error, DbError::GraphRowShape { .. }),
            &format!("unexpected coverage-total error: {error}"),
        )?;
        store.connection.execute(
            "UPDATE graph_coverage SET total = covered + omitted
              WHERE scope_kind = 'project' AND relation_scope IS NULL",
            [],
        )?;

        store.connection.execute(
            "UPDATE project_identity SET active_generation = 99 WHERE singleton = 1",
            [],
        )?;
        let error = require_db_error(
            store.repository_graph_entity(source.key()),
            "mismatched typed graph generation was accepted",
        )?;
        require(
            matches!(error, DbError::GraphRowShape { .. }),
            &format!("unexpected typed-generation error: {error}"),
        )?;
        store.connection.execute(
            "UPDATE project_identity SET active_generation = 1 WHERE singleton = 1",
            [],
        )?;

        store.connection.execute(
            "UPDATE graph_entities SET canonical_identity = 'different-collision-witness'
              WHERE entity_key = ?1",
            [&source_digest[..]],
        )?;
        let error = require_db_error(
            store.repository_graph_entity(source.key()),
            "canonical collision witness was accepted",
        )?;
        require(
            matches!(error, DbError::GraphContract(_)),
            &format!("unexpected collision-witness error: {error}"),
        )?;
        store.connection.execute(
            "UPDATE graph_entities SET canonical_identity = ?1 WHERE entity_key = ?2",
            params![source_canonical, &source_digest[..]],
        )?;

        store.connection.execute(
            "UPDATE graph_entities SET entity_key = zeroblob(32) WHERE entity_key = ?1",
            [&folder_digest[..]],
        )?;
        let folder_path = RepositoryNodePath::new(Path::new("src"))?;
        let error = require_db_error(
            store.repository_graph_entities_by_path(fixture.project, &folder_path, 10),
            "invalid stable digest was accepted",
        )?;
        require(
            matches!(error, DbError::GraphContract(_)),
            &format!("unexpected stable-digest error: {error}"),
        )?;
        store.connection.execute(
            "UPDATE graph_entities SET entity_key = ?1 WHERE entity_key = zeroblob(32)",
            [&folder_digest[..]],
        )?;

        store.connection.execute(
            "UPDATE graph_entities SET entity_key = X'01' WHERE entity_key = ?1",
            [&folder_digest[..]],
        )?;
        let error = require_db_error(
            store.repository_graph_entities_by_path(fixture.project, &folder_path, 10),
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
        store.connection.execute(
            "UPDATE graph_entities SET entity_key = ?1 WHERE entity_key = X'01'",
            [&folder_digest[..]],
        )?;

        store.connection.execute(
            "UPDATE graph_entities SET canonical_identity = X'00' WHERE entity_key = ?1",
            [&symbol_digest[..]],
        )?;
        let source_path = RepositoryNodePath::new(Path::new("src/Äuth.rs"))?;
        let error = require_db_error(
            store.repository_graph_entities_by_path(fixture.project, &source_path, 10),
            "later row conversion failure returned a successful partial page",
        )?;
        require(
            matches!(error, DbError::Sqlite(_)),
            &format!("unexpected later-row conversion error: {error}"),
        )?;
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
        let db_path = temp.path().join("projectatlas.db");
        let mut writer = AtlasStore::open(&db_path)?;
        let fixture_v1 = publish_fixture(&mut writer, "graph-publication")?;
        let old_reader = AtlasStore::open_read_only(&db_path)?;
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
                    byte_count: 24,
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
        let rolled_back_reader = AtlasStore::open_read_only(&db_path)?;
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

        let fixture_v2 = graph_fixture(IndexGeneration::new(2))?;
        {
            let mut publication = writer.begin_index_publication("graph-publication")?;
            publication.replace_file_texts_for_paths(
                &["src/Äuth.rs".to_string()],
                &[IndexedFileText {
                    path: "src/Äuth.rs".to_string(),
                    content_hash: Some("hash-new".to_string()),
                    byte_count: 24,
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
        let new_reader = AtlasStore::open_read_only(&db_path)?;
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
