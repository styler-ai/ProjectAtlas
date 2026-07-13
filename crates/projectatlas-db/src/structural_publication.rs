//! Parent-owned staging and atomic publication for full structural scans.

use crate::{AtlasStore, DbError, DbResult, normalize_metadata_path, schema, sqlite_read_uri};
use projectatlas_core::graph::{IndexEpoch, ProjectInstanceId, PublicationState, StructuralSlot};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Attached-database schema name reserved for one validated staging source.
const STAGING_SCHEMA: &str = "structural_stage";
/// Slot-bound typed graph entity table.
const GRAPH_ENTITIES_TABLE: &str = "graph_entities";
/// Slot-bound typed graph relation table.
const GRAPH_RELATIONS_TABLE: &str = "graph_relations";
/// Slot-bound resolved-relation evidence table.
const GRAPH_EVIDENCE_OCCURRENCES_TABLE: &str = "graph_evidence_occurrences";
/// Slot-bound ambiguous and unresolved occurrence table.
const GRAPH_RESOLUTION_OCCURRENCES_TABLE: &str = "graph_resolution_occurrences";
/// Slot-bound candidates for ambiguous occurrences.
const GRAPH_RESOLUTION_CANDIDATES_TABLE: &str = "graph_resolution_candidates";
/// Slot-bound structural coverage table.
const GRAPH_COVERAGE_TABLE: &str = "graph_coverage";
/// Slot-bound authoritative lexical source table.
const FILE_TEXTS_TABLE: &str = "file_texts";

/// Every core structural table whose source and target counts must reconcile.
const STRUCTURAL_TABLES: [&str; 7] = [
    GRAPH_ENTITIES_TABLE,
    GRAPH_RELATIONS_TABLE,
    GRAPH_EVIDENCE_OCCURRENCES_TABLE,
    GRAPH_RESOLUTION_OCCURRENCES_TABLE,
    GRAPH_RESOLUTION_CANDIDATES_TABLE,
    GRAPH_COVERAGE_TABLE,
    FILE_TEXTS_TABLE,
];

/// Reverse dependency order for clearing one inactive or staging slot.
const STRUCTURAL_DELETE_ORDER: [&str; 7] = [
    GRAPH_RESOLUTION_CANDIDATES_TABLE,
    GRAPH_EVIDENCE_OCCURRENCES_TABLE,
    GRAPH_RESOLUTION_OCCURRENCES_TABLE,
    GRAPH_RELATIONS_TABLE,
    GRAPH_COVERAGE_TABLE,
    GRAPH_ENTITIES_TABLE,
    FILE_TEXTS_TABLE,
];

/// Parent-owned identity and authored-state snapshot for one staging database.
pub struct StructuralStaging {
    /// Separate `SQLite` file created from the captured live database.
    path: PathBuf,
    /// Slot and epoch that both live and staged databases must retain until import.
    base_publication: PublicationState,
    /// Persistent database identity shared by the live and staged files.
    project_instance_id: ProjectInstanceId,
    /// Project-root metadata observed before staging, when already initialized.
    base_project_root: Option<String>,
    /// Canonical project root that the stage is allowed to publish into.
    project_root: String,
    /// Authored purpose rows used to detect and preserve concurrent updates.
    base_purposes: BTreeMap<String, PurposeRow>,
}

impl StructuralStaging {
    /// Return the separate database path owned by the full-scan parent.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Complete persisted purpose state for concurrency-safe publication merging.
#[derive(Clone, Debug, Eq, PartialEq)]
struct PurposeRow {
    /// Authored or suggested one-line purpose.
    purpose: Option<String>,
    /// Persisted purpose source value.
    source: String,
    /// Persisted purpose lifecycle status.
    status: String,
    /// Last durable purpose update timestamp.
    updated_at: String,
    /// Optional durable purpose author identity.
    updated_by: Option<String>,
}

impl PurposeRow {
    /// Whether this is the default row created for a newly imported node.
    fn is_default_missing(&self) -> bool {
        self.purpose.is_none()
            && self.source == "missing"
            && self.status == "missing"
            && self.updated_by.is_none()
    }
}

impl AtlasStore {
    /// Create a coherent separate snapshot for a parent-owned full scan.
    ///
    /// # Errors
    ///
    /// Returns an error when paths alias, the target already exists, the live
    /// database is invalid, the snapshot cannot be created, or its identity
    /// differs from the captured live base.
    pub fn create_structural_staging(
        &self,
        live_path: &Path,
        staging_path: &Path,
        project_root: &Path,
    ) -> DbResult<StructuralStaging> {
        require_live_connection_path(&self.connection, live_path)?;
        require_separate_paths(live_path, staging_path)?;
        require_available_staging_path(staging_path)?;
        verify_database(&self.connection)?;

        let base_publication = load_publication_state(&self.connection)?;
        let project_instance_id = schema::project_instance_id(&self.connection)?;
        let project_root = normalize_metadata_path(project_root);
        let base_project_root = self.project_root()?;
        if base_project_root
            .as_ref()
            .is_some_and(|root| root != &project_root)
        {
            return Err(publication_error(format!(
                "live project root does not match the requested scan root: live={base_project_root:?}, requested={project_root:?}"
            )));
        }
        let base_purposes = load_purpose_rows(&self.connection, None)?;

        let staging_text = staging_path.to_str().ok_or_else(|| {
            publication_error(format!(
                "staging path is not valid UTF-8: {}",
                staging_path.display()
            ))
        })?;
        self.connection
            .execute("VACUUM main INTO ?1", [staging_text])?;

        let staging = StructuralStaging {
            path: staging_path.to_path_buf(),
            base_publication,
            project_instance_id,
            base_project_root,
            project_root,
            base_purposes,
        };
        let connection = open_immutable(staging_path)?;
        validate_staging_base_identity(&connection, &staging)?;
        Ok(staging)
    }

    /// Clear only the staging database's active structural rows before rebuild.
    ///
    /// # Errors
    ///
    /// Returns an error if schema validation or the staging transaction fails.
    pub fn prepare_structural_full_scan(&mut self) -> DbResult<()> {
        verify_database(&self.connection)?;
        let active_slot = load_publication_state(&self.connection)?.active_slot;
        let transaction = self.connection.transaction()?;
        for table in STRUCTURAL_DELETE_ORDER {
            transaction.execute(
                &format!("DELETE FROM {table} WHERE structural_slot = ?1"),
                [slot_text(active_slot)],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Validate and checkpoint a completed staging build before parent import.
    ///
    /// # Errors
    ///
    /// Returns an error when identity, schema, integrity, or WAL checkpointing
    /// does not reconcile with the staging-start base.
    pub fn seal_structural_staging(&self, staging: &StructuralStaging) -> DbResult<()> {
        validate_staging_identity(&self.connection, staging)?;
        let busy = self
            .connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                row.get::<_, i64>(0)
            })?;
        if busy != 0 {
            return Err(publication_error(format!(
                "staging WAL checkpoint remained busy: {busy}"
            )));
        }
        verify_database(&self.connection)
    }

    /// Atomically import a sealed stage into the inactive slot and publish it.
    ///
    /// The previous active structural slot and every authored table remain
    /// intact. Purpose changes produced by the scan are applied only when the
    /// live purpose still equals the staging-start row.
    ///
    /// # Errors
    ///
    /// Returns an error if validation, import, reconciliation, publication, or
    /// staging detachment fails. Any transaction error preserves both live
    /// slots and the prior publication state.
    pub fn publish_structural_staging(
        &mut self,
        staging: &StructuralStaging,
    ) -> DbResult<PublicationState> {
        verify_database(&self.connection)?;
        let staged_connection = open_immutable(staging.path())?;
        validate_staging_identity(&staged_connection, staging)?;
        drop(staged_connection);

        attach_structural_staging_read_only(&self.connection, staging.path())?;
        let result = publish_attached(&mut self.connection, staging);
        let detach = self
            .connection
            .execute_batch(&format!("DETACH DATABASE {STAGING_SCHEMA}"));
        match (result, detach) {
            (Ok(publication), Ok(())) => Ok(publication),
            (Ok(_), Err(error)) => Err(DbError::Sqlite(error)),
            (Err(error), _) => Err(error),
        }
    }

    /// Read the atomically observed active structural slot and epoch.
    ///
    /// # Errors
    ///
    /// Returns an error when the singleton publication row is invalid.
    pub fn publication_state(&self) -> DbResult<PublicationState> {
        load_publication_state(&self.connection)
    }
}

/// Attach a sealed staging database without granting publication code write access.
fn attach_structural_staging_read_only(
    connection: &Connection,
    staging_path: &Path,
) -> DbResult<()> {
    connection.execute(
        &format!("ATTACH DATABASE ?1 AS {STAGING_SCHEMA}"),
        [sqlite_read_uri(staging_path, true)],
    )?;
    Ok(())
}

/// Run the one parent transaction against an already attached validated stage.
fn publish_attached(
    connection: &mut Connection,
    staging: &StructuralStaging,
) -> DbResult<PublicationState> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let live_publication = load_publication_state(&transaction)?;
    if live_publication != staging.base_publication {
        return Err(publication_error(format!(
            "live publication changed after staging began: expected {:?}, found {:?}",
            staging.base_publication, live_publication
        )));
    }
    validate_attached_identity(&transaction, staging)?;
    bind_live_project_root(&transaction, staging)?;

    let next_publication = live_publication
        .next_full()
        .map_err(|error| publication_error(error.to_string()))?;
    let next_epoch = i64::try_from(next_publication.active_epoch.get()).map_err(|source| {
        publication_error(format!(
            "publication epoch exceeds SQLite INTEGER: {source}"
        ))
    })?;
    let source_slot = slot_text(staging.base_publication.active_slot);
    let target_slot = slot_text(next_publication.active_slot);

    reconcile_compatibility_rows(&transaction, staging)?;
    replace_structural_rows(&transaction, source_slot, target_slot, next_epoch)?;
    reconcile_structural_counts(&transaction, source_slot, target_slot)?;
    schema::verify_current_schema(&transaction)?;

    let changed = transaction.execute(
        "UPDATE graph_publication_state
         SET active_slot = ?1, active_epoch = ?2
         WHERE singleton = 1 AND active_slot = ?3 AND active_epoch = ?4",
        params![
            target_slot,
            next_epoch,
            source_slot,
            i64::try_from(live_publication.active_epoch.get()).map_err(|source| {
                publication_error(format!("base epoch exceeds SQLite INTEGER: {source}"))
            })?
        ],
    )?;
    if changed != 1 {
        return Err(publication_error(
            "publication singleton changed before the atomic flip",
        ));
    }
    if load_publication_state(&transaction)? != next_publication {
        return Err(publication_error(
            "publication singleton did not reconcile after the atomic flip",
        ));
    }
    transaction.commit()?;
    Ok(next_publication)
}

/// Merge compatibility tables while preserving concurrent authored purposes.
fn reconcile_compatibility_rows(
    transaction: &Transaction<'_>,
    staging: &StructuralStaging,
) -> DbResult<()> {
    transaction.execute("UPDATE nodes SET exists_now = 0", [])?;
    transaction.execute_batch(&format!(
        "INSERT INTO nodes(
             path, kind, parent_path, extension, language, size_bytes, mtime_ns,
             content_hash, exists_now, first_seen_at, last_seen_at, last_indexed_at
         )
         SELECT
             path, kind, parent_path, extension, language, size_bytes, mtime_ns,
             content_hash, exists_now, first_seen_at, last_seen_at, last_indexed_at
         FROM {STAGING_SCHEMA}.nodes
         WHERE 1
         ON CONFLICT(path) DO UPDATE SET
             kind = excluded.kind,
             parent_path = excluded.parent_path,
             extension = excluded.extension,
             language = excluded.language,
             size_bytes = excluded.size_bytes,
             mtime_ns = excluded.mtime_ns,
             content_hash = excluded.content_hash,
             exists_now = excluded.exists_now,
             last_seen_at = excluded.last_seen_at,
             last_indexed_at = excluded.last_indexed_at;"
    ))?;
    transaction.execute(
        "INSERT INTO purposes(node_id, purpose, source, status)
         SELECT node.id, NULL, 'missing', 'missing'
         FROM nodes AS node
         WHERE NOT EXISTS(
             SELECT 1 FROM purposes AS purpose WHERE purpose.node_id = node.id
         )",
        [],
    )?;

    let current_purposes = load_purpose_rows(transaction, None)?;
    let staged_purposes = load_purpose_rows(transaction, Some(STAGING_SCHEMA))?;
    {
        let mut update = transaction.prepare(
            "UPDATE purposes
                 SET purpose = ?1, source = ?2, status = ?3,
                     updated_at = ?4, updated_by = ?5
                 WHERE node_id = (SELECT id FROM nodes WHERE path = ?6)",
        )?;
        for (path, staged_row) in staged_purposes {
            let current_row = current_purposes.get(&path).ok_or_else(|| {
                publication_error(format!(
                    "live purpose row is missing after node import: {path}"
                ))
            })?;
            let may_apply = staging.base_purposes.get(&path).map_or_else(
                || current_row.is_default_missing(),
                |base| current_row == base,
            );
            if may_apply && current_row != &staged_row {
                update.execute(params![
                    staged_row.purpose,
                    staged_row.source,
                    staged_row.status,
                    staged_row.updated_at,
                    staged_row.updated_by,
                    path
                ])?;
            }
        }
    }

    transaction.execute("DELETE FROM summaries", [])?;
    transaction.execute_batch(&format!(
        "INSERT INTO summaries(node_id, summary_level, subject, summary, updated_at)
         SELECT live_node.id, summary.summary_level, summary.subject,
                summary.summary, summary.updated_at
         FROM {STAGING_SCHEMA}.summaries AS summary
         JOIN {STAGING_SCHEMA}.nodes AS staged_node ON staged_node.id = summary.node_id
         JOIN nodes AS live_node ON live_node.path = staged_node.path;

         DELETE FROM symbol_relations;
         INSERT INTO symbol_relations(
             path, source_name, target_name, kind, line, context, parser, created_at
         )
         SELECT path, source_name, target_name, kind, line, context, parser, created_at
         FROM {STAGING_SCHEMA}.symbol_relations;

         DELETE FROM symbols;
         INSERT INTO symbols(
             path, language, name, kind, signature, exported, documentation,
             line_start, line_end, parent, parser, detail, created_at, updated_at
         )
         SELECT
             path, language, name, kind, signature, exported, documentation,
             line_start, line_end, parent, parser, detail, created_at, updated_at
         FROM {STAGING_SCHEMA}.symbols;

         DELETE FROM source_parse_metadata;
         INSERT INTO source_parse_metadata(
             path, language, parser, symbol_count, relation_count, updated_at
         )
         SELECT path, language, parser, symbol_count, relation_count, updated_at
         FROM {STAGING_SCHEMA}.source_parse_metadata;"
    ))?;
    Ok(())
}

/// Replace the inactive slot from staged active rows in foreign-key-safe order.
fn replace_structural_rows(
    transaction: &Transaction<'_>,
    source_slot: &str,
    target_slot: &str,
    next_epoch: i64,
) -> DbResult<()> {
    for table in STRUCTURAL_DELETE_ORDER {
        transaction.execute(
            &format!("DELETE FROM {table} WHERE structural_slot = ?1"),
            [target_slot],
        )?;
    }

    transaction.execute(
        &format!(
            "INSERT INTO graph_entities(
                 stable_key_digest, stable_key_version, stable_key_canonical,
                 project_instance_id, entity_kind, repository_path, qualified_name,
                 signature, discriminator, external_namespace, external_value, language,
                 source_start_byte, source_end_byte, source_start_line, source_end_line,
                 parser_kind, parser_identity, parser_version,
                 structural_slot, last_changed_epoch
             )
             SELECT
                 stable_key_digest, stable_key_version, stable_key_canonical,
                 project_instance_id, entity_kind, repository_path, qualified_name,
                 signature, discriminator, external_namespace, external_value, language,
                 source_start_byte, source_end_byte, source_start_line, source_end_line,
                 parser_kind, parser_identity, parser_version, ?2, ?3
             FROM {STAGING_SCHEMA}.graph_entities
             WHERE structural_slot = ?1"
        ),
        params![source_slot, target_slot, next_epoch],
    )?;
    transaction.execute(
        &format!(
            "INSERT INTO graph_relations(
                 stable_key_digest, stable_key_version, stable_key_canonical,
                 source_entity_digest, relation_kind, resolution_status, target_scope,
                 target_entity_digest, external_target_namespace, external_target_value,
                 confidence, parser_kind, parser_identity, parser_version,
                 structural_slot, last_changed_epoch
             )
             SELECT
                 stable_key_digest, stable_key_version, stable_key_canonical,
                 source_entity_digest, relation_kind, resolution_status, target_scope,
                 target_entity_digest, external_target_namespace, external_target_value,
                 confidence, parser_kind, parser_identity, parser_version, ?2, ?3
             FROM {STAGING_SCHEMA}.graph_relations
             WHERE structural_slot = ?1"
        ),
        params![source_slot, target_slot, next_epoch],
    )?;
    transaction.execute(
        &format!(
            "INSERT INTO graph_evidence_occurrences(
                 stable_key_digest, stable_key_version, stable_key_canonical,
                 relation_digest, origin_kind, origin_entity_digest,
                 origin_project_instance_id, origin_repository_path,
                 origin_external_namespace, origin_external_value,
                 source_start_byte, source_end_byte, source_start_line, source_end_line,
                 resolver_name, resolver_version, content_span_fingerprint,
                 occurrence_discriminator, evidence_class, confidence, completeness,
                 explanation, structural_slot, last_changed_epoch
             )
             SELECT
                 stable_key_digest, stable_key_version, stable_key_canonical,
                 relation_digest, origin_kind, origin_entity_digest,
                 origin_project_instance_id, origin_repository_path,
                 origin_external_namespace, origin_external_value,
                 source_start_byte, source_end_byte, source_start_line, source_end_line,
                 resolver_name, resolver_version, content_span_fingerprint,
                 occurrence_discriminator, evidence_class, confidence, completeness,
                 explanation, ?2, ?3
             FROM {STAGING_SCHEMA}.graph_evidence_occurrences
             WHERE structural_slot = ?1"
        ),
        params![source_slot, target_slot, next_epoch],
    )?;
    transaction.execute(
        &format!(
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
             )
             SELECT
                 stable_key_digest, stable_key_version, stable_key_canonical,
                 source_entity_digest, relation_kind, origin_kind, origin_entity_digest,
                 origin_project_instance_id, origin_repository_path,
                 origin_external_namespace, origin_external_value,
                 source_start_byte, source_end_byte, source_start_line, source_end_line,
                 resolver_name, resolver_version, content_span_fingerprint,
                 occurrence_discriminator, resolution_status, candidate_total,
                 candidate_completeness, unresolved_reason, evidence_class, confidence,
                 completeness, parser_kind, parser_identity, parser_version, ?2, ?3
             FROM {STAGING_SCHEMA}.graph_resolution_occurrences
             WHERE structural_slot = ?1"
        ),
        params![source_slot, target_slot, next_epoch],
    )?;
    transaction.execute(
        &format!(
            "INSERT INTO graph_resolution_candidates(
                 resolution_occurrence_digest, candidate_ordinal, target_scope,
                 target_entity_digest, external_target_namespace, external_target_value,
                 confidence, explanation, structural_slot, last_changed_epoch
             )
             SELECT
                 resolution_occurrence_digest, candidate_ordinal, target_scope,
                 target_entity_digest, external_target_namespace, external_target_value,
                 confidence, explanation, ?2, ?3
             FROM {STAGING_SCHEMA}.graph_resolution_candidates
             WHERE structural_slot = ?1"
        ),
        params![source_slot, target_slot, next_epoch],
    )?;
    transaction.execute(
        &format!(
            "INSERT INTO graph_coverage(
                 scope_kind, repository_path, pass_identity, relation_kind,
                 coverage_state, produced_count, omitted_count, reached_limit, reason,
                 structural_slot, last_changed_epoch
             )
             SELECT
                 scope_kind, repository_path, pass_identity, relation_kind,
                 coverage_state, produced_count, omitted_count, reached_limit, reason,
                 ?2, ?3
             FROM {STAGING_SCHEMA}.graph_coverage
             WHERE structural_slot = ?1"
        ),
        params![source_slot, target_slot, next_epoch],
    )?;
    transaction.execute(
        &format!(
            "INSERT INTO file_texts(
                 path, content_hash, byte_count, line_count, content, updated_at,
                 structural_slot, last_changed_epoch
             )
             SELECT path, content_hash, byte_count, line_count, content, updated_at, ?2, ?3
             FROM {STAGING_SCHEMA}.file_texts
             WHERE structural_slot = ?1"
        ),
        params![source_slot, target_slot, next_epoch],
    )?;
    Ok(())
}

/// Require exact per-table counts between the staged source and inactive target.
fn reconcile_structural_counts(
    transaction: &Transaction<'_>,
    source_slot: &str,
    target_slot: &str,
) -> DbResult<()> {
    for table in STRUCTURAL_TABLES {
        let source_count = transaction.query_row(
            &format!("SELECT COUNT(*) FROM {STAGING_SCHEMA}.{table} WHERE structural_slot = ?1"),
            [source_slot],
            |row| row.get::<_, i64>(0),
        )?;
        let target_count = transaction.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE structural_slot = ?1"),
            [target_slot],
            |row| row.get::<_, i64>(0),
        )?;
        if source_count != target_count {
            return Err(publication_error(format!(
                "structural table {table} did not reconcile: source={source_count}, target={target_count}"
            )));
        }
    }
    Ok(())
}

/// Validate one closed staging file against its captured live base.
fn validate_staging_identity(connection: &Connection, staging: &StructuralStaging) -> DbResult<()> {
    verify_database(connection)?;
    let observed_publication = load_publication_state(connection)?;
    let observed_instance = schema::project_instance_id(connection)?;
    let observed_root = load_project_root(connection, None)?.ok_or_else(|| {
        publication_error("staging database is missing its canonical project root")
    })?;
    if observed_publication != staging.base_publication {
        return Err(publication_error(format!(
            "staging publication base changed: expected {:?}, found {:?}",
            staging.base_publication, observed_publication
        )));
    }
    if observed_instance != staging.project_instance_id {
        return Err(publication_error(
            "staging project instance does not match the live base",
        ));
    }
    if observed_root != staging.project_root {
        return Err(publication_error(format!(
            "staging project root does not match the live base: expected {:?}, found {:?}",
            staging.project_root, observed_root
        )));
    }
    Ok(())
}

/// Recheck live and attached identities inside the publication transaction.
fn validate_attached_identity(
    transaction: &Transaction<'_>,
    staging: &StructuralStaging,
) -> DbResult<()> {
    let live_instance = load_metadata(transaction, None, "project_instance_id")?
        .ok_or(DbError::ProjectInstanceIdMissing)?;
    let live_root = load_project_root(transaction, None)?;
    let observed_publication = load_attached_publication_state(transaction)?;
    let observed_instance =
        load_metadata(transaction, Some(STAGING_SCHEMA), "project_instance_id")?
            .ok_or(DbError::ProjectInstanceIdMissing)?;
    let observed_root = load_project_root(transaction, Some(STAGING_SCHEMA))?.ok_or_else(|| {
        publication_error("attached staging database is missing its canonical project root")
    })?;
    if live_instance != staging.project_instance_id.to_string()
        || live_root != staging.base_project_root
        || observed_publication != staging.base_publication
        || observed_instance != staging.project_instance_id.to_string()
        || observed_root != staging.project_root
    {
        return Err(publication_error(
            "live or attached staging identity changed after read-only validation",
        ));
    }
    Ok(())
}

/// Bind a previously uninitialized live root inside the publication transaction.
fn bind_live_project_root(
    transaction: &Transaction<'_>,
    staging: &StructuralStaging,
) -> DbResult<()> {
    if staging.base_project_root.is_none() {
        transaction.execute(
            "INSERT INTO metadata(key, value) VALUES('project_root', ?1)",
            [&staging.project_root],
        )?;
    }
    Ok(())
}

/// Validate the unmodified snapshot against live state captured before rebuild.
fn validate_staging_base_identity(
    connection: &Connection,
    staging: &StructuralStaging,
) -> DbResult<()> {
    verify_database(connection)?;
    let observed_publication = load_publication_state(connection)?;
    let observed_instance = schema::project_instance_id(connection)?;
    let observed_root = load_project_root(connection, None)?;
    if observed_publication != staging.base_publication
        || observed_instance != staging.project_instance_id
        || observed_root != staging.base_project_root
    {
        return Err(publication_error(
            "staging snapshot does not match the captured live base",
        ));
    }
    Ok(())
}

/// Verify physical integrity, schema history, publication state, and row bindings.
fn verify_database(connection: &Connection) -> DbResult<()> {
    verify_quick_check(connection)?;
    schema::verify_current_schema(connection)
}

/// Require the complete `SQLite` quick-check result to be exactly `ok`.
fn verify_quick_check(connection: &Connection) -> DbResult<()> {
    let mut statement = connection.prepare("PRAGMA quick_check").map_err(|error| {
        publication_error(format!("SQLite quick_check could not start: {error}"))
    })?;
    let mapped = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| {
            publication_error(format!(
                "SQLite quick_check could not read results: {error}"
            ))
        })?;
    let rows = mapped.collect::<Result<Vec<_>, _>>().map_err(|error| {
        publication_error(format!("SQLite quick_check result was invalid: {error}"))
    })?;
    if rows != ["ok"] {
        return Err(publication_error(format!(
            "SQLite quick_check failed: {}",
            rows.join("; ")
        )));
    }
    Ok(())
}

/// Load the typed singleton publication state from the main database.
fn load_publication_state(connection: &Connection) -> DbResult<PublicationState> {
    let (slot, epoch) = connection.query_row(
        "SELECT active_slot, active_epoch
         FROM graph_publication_state
         WHERE singleton = 1",
        [],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    )?;
    publication_state_from_sql(&slot, epoch)
}

/// Load the typed singleton publication state from the attached stage.
fn load_attached_publication_state(transaction: &Transaction<'_>) -> DbResult<PublicationState> {
    let (slot, epoch) = transaction.query_row(
        &format!(
            "SELECT active_slot, active_epoch
             FROM {STAGING_SCHEMA}.graph_publication_state
             WHERE singleton = 1"
        ),
        [],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    )?;
    publication_state_from_sql(&slot, epoch)
}

/// Convert validated `SQLite` scalar values into the core publication domain.
fn publication_state_from_sql(slot: &str, epoch: i64) -> DbResult<PublicationState> {
    let active_slot = match slot {
        "a" => StructuralSlot::A,
        "b" => StructuralSlot::B,
        value => {
            return Err(DbError::InvalidEnum {
                field: "graph_publication_state.active_slot",
                value: value.to_owned(),
            });
        }
    };
    let epoch = u64::try_from(epoch)
        .map_err(|source| publication_error(format!("publication epoch is invalid: {source}")))?;
    Ok(PublicationState {
        active_slot,
        active_epoch: IndexEpoch::new(epoch),
    })
}

/// Return the `SQLite` encoding for one closed structural slot.
fn slot_text(slot: StructuralSlot) -> &'static str {
    match slot {
        StructuralSlot::A => "a",
        StructuralSlot::B => "b",
    }
}

/// Load all purpose rows by repository path from main or an attached schema.
fn load_purpose_rows(
    connection: &Connection,
    schema_name: Option<&str>,
) -> DbResult<BTreeMap<String, PurposeRow>> {
    let prefix = schema_name.map_or_else(String::new, |name| format!("{name}."));
    let mut statement = connection.prepare(&format!(
        "SELECT node.path, purpose.purpose, purpose.source, purpose.status,
                purpose.updated_at, purpose.updated_by
         FROM {prefix}purposes AS purpose
         JOIN {prefix}nodes AS node ON node.id = purpose.node_id
         ORDER BY node.path"
    ))?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            PurposeRow {
                purpose: row.get(1)?,
                source: row.get(2)?,
                status: row.get(3)?,
                updated_at: row.get(4)?,
                updated_by: row.get(5)?,
            },
        ))
    })?;
    rows.collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(DbError::from)
}

/// Load canonical project-root metadata from main or an attached schema.
fn load_project_root(
    connection: &Connection,
    schema_name: Option<&str>,
) -> DbResult<Option<String>> {
    load_metadata(connection, schema_name, "project_root")
}

/// Load one metadata value from main or an attached schema.
fn load_metadata(
    connection: &Connection,
    schema_name: Option<&str>,
    key: &str,
) -> DbResult<Option<String>> {
    let prefix = schema_name.map_or_else(String::new, |name| format!("{name}."));
    connection
        .query_row(
            &format!("SELECT value FROM {prefix}metadata WHERE key = ?1"),
            [key],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(DbError::from)
}

/// Open a checkpointed staging database without creating files or sidecars.
fn open_immutable(path: &Path) -> DbResult<Connection> {
    Connection::open_with_flags(
        sqlite_read_uri(path, true),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(DbError::from)
}

/// Prove the caller-supplied live path names the store's main database.
fn require_live_connection_path(connection: &Connection, live_path: &Path) -> DbResult<()> {
    let observed = connection.query_row("PRAGMA database_list", [], |row| {
        let name = row.get::<_, String>(1)?;
        let path = row.get::<_, String>(2)?;
        Ok((name, path))
    })?;
    if observed.0 != "main" || observed.1.is_empty() {
        return Err(publication_error(
            "live store does not expose a file-backed main database",
        ));
    }
    let observed = fs::canonicalize(&observed.1).map_err(|error| {
        publication_error(format!(
            "cannot resolve the store's live database {}: {error}",
            observed.1
        ))
    })?;
    let requested = fs::canonicalize(live_path).map_err(|error| {
        publication_error(format!(
            "cannot resolve requested live database {}: {error}",
            live_path.display()
        ))
    })?;
    if observed != requested {
        return Err(publication_error(format!(
            "live database path does not match the open store: store={}, requested={}",
            observed.display(),
            requested.display()
        )));
    }
    Ok(())
}

/// Prove the resolved staging target does not alias the live database.
fn require_separate_paths(live_path: &Path, staging_path: &Path) -> DbResult<()> {
    let live = fs::canonicalize(live_path).map_err(|error| {
        publication_error(format!(
            "cannot resolve live database {}: {error}",
            live_path.display()
        ))
    })?;
    let parent = staging_path.parent().ok_or_else(|| {
        publication_error(format!(
            "staging path has no parent: {}",
            staging_path.display()
        ))
    })?;
    let parent = fs::canonicalize(parent).map_err(|error| {
        publication_error(format!(
            "cannot resolve staging parent {}: {error}",
            parent.display()
        ))
    })?;
    let target = parent.join(staging_path.file_name().ok_or_else(|| {
        publication_error(format!(
            "staging path has no file name: {}",
            staging_path.display()
        ))
    })?);
    if live == target {
        return Err(publication_error(
            "staging path must differ from the live database path",
        ));
    }
    Ok(())
}

/// Reject a staging target when its database or exact `SQLite` sidecars exist.
fn require_available_staging_path(staging_path: &Path) -> DbResult<()> {
    for path in [
        staging_path.to_path_buf(),
        sqlite_sidecar_path(staging_path, "-wal"),
        sqlite_sidecar_path(staging_path, "-shm"),
    ] {
        if path.exists() {
            return Err(publication_error(format!(
                "staging database path already exists: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

/// Append an `SQLite` sidecar suffix without changing the database extension.
fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

/// Build one database-layer structural publication failure.
fn publication_error(message: impl Into<String>) -> DbError {
    DbError::StructuralPublication {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IndexedFileText;
    use projectatlas_core::{Node, NodeKind, PurposeSource};
    use std::error::Error;
    use std::io;

    #[test]
    fn task_arri_ut_arri_4_12() -> Result<(), Box<dyn Error>> {
        prove_parent_owned_full_scan_publication()
    }

    #[test]
    fn task_arri_ut_arri_7_6() -> Result<(), Box<dyn Error>> {
        prove_parent_owned_full_scan_publication()
    }

    fn prove_parent_owned_full_scan_publication() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let live_path = temp.path().join("live.db");
        let staging_path = temp.path().join("stage.db");
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;

        let mut live = AtlasStore::open(&live_path)?;
        live.set_project_root(&root)?;
        live.replace_scan(&[file_node("src/old.rs", "old-hash")])?;
        live.set_purpose(
            "src/old.rs",
            "Preserve authored intent.",
            PurposeSource::Agent,
        )?;
        live.replace_file_texts_for_paths(
            &["src/old.rs".to_owned()],
            &[file_text("src/old.rs", "old lexical marker")],
        )?;
        live.connection.execute(
            "INSERT INTO metadata(key, value) VALUES('authored_note', 'preserved')",
            [],
        )?;
        insert_graph_entity(&live.connection, "old", "src/old.rs")?;

        let staging = live.create_structural_staging(&live_path, &staging_path, &root)?;
        let mut stage = AtlasStore::open(staging.path())?;
        stage.prepare_structural_full_scan()?;
        stage.replace_scan(&[file_node("src/new.rs", "new-hash")])?;
        stage.replace_file_texts_for_paths(
            &["src/new.rs".to_owned()],
            &[file_text("src/new.rs", "new lexical marker")],
        )?;
        insert_graph_entity(&stage.connection, "new", "src/new.rs")?;
        let staged_counts = structural_counts(&stage.connection, StructuralSlot::A)?;
        stage.seal_structural_staging(&staging)?;
        drop(stage);
        prove_staging_attachment_is_read_only(&live, &staging)?;
        live.set_purpose(
            "src/old.rs",
            "Concurrent authored intent.",
            PurposeSource::Agent,
        )?;

        let publication = live.publish_structural_staging(&staging)?;
        require_eq(
            &publication,
            &PublicationState {
                active_slot: StructuralSlot::B,
                active_epoch: IndexEpoch::new(1),
            },
            "published slot and epoch",
        )?;
        let new_text = require_some(live.load_file_text("src/new.rs")?, "new active lexical row")?;
        require_eq(
            &new_text.content,
            &"new lexical marker".to_owned(),
            "new active text",
        )?;
        require(
            live.load_file_text("src/old.rs")?.is_none(),
            "old lexical row remained visible through the active reader",
        )?;
        require_eq(
            &structural_counts(&live.connection, StructuralSlot::B)?,
            &staged_counts,
            "staged and published structural counts",
        )?;
        require_eq(
            &live.connection.query_row(
                "SELECT content FROM file_texts
                 WHERE structural_slot = 'a' AND path = 'src/old.rs'",
                [],
                |row| row.get::<_, String>(0),
            )?,
            &"old lexical marker".to_owned(),
            "retained rollback lexical row",
        )?;
        require_eq(
            &live.connection.query_row(
                "SELECT value FROM metadata WHERE key = 'authored_note'",
                [],
                |row| row.get::<_, String>(0),
            )?,
            &"preserved".to_owned(),
            "authored metadata",
        )?;
        require_eq(
            &live.connection.query_row(
                "SELECT purpose.purpose
                 FROM purposes AS purpose
                 JOIN nodes AS node ON node.id = purpose.node_id
                 WHERE node.path = 'src/old.rs'",
                [],
                |row| row.get::<_, String>(0),
            )?,
            &"Concurrent authored intent.".to_owned(),
            "concurrent authored purpose",
        )?;

        prove_staging_rejections(temp.path())?;
        prove_import_rollback(temp.path())?;

        Ok(())
    }

    fn prove_staging_attachment_is_read_only(
        live: &AtlasStore,
        staging: &StructuralStaging,
    ) -> Result<(), Box<dyn Error>> {
        attach_structural_staging_read_only(&live.connection, staging.path())?;
        let write = live
            .connection
            .execute(&format!("DELETE FROM {STAGING_SCHEMA}.nodes"), []);
        live.connection
            .execute_batch(&format!("DETACH DATABASE {STAGING_SCHEMA}"))?;

        match write {
            Err(rusqlite::Error::SqliteFailure(error, _))
                if error.code == rusqlite::ErrorCode::ReadOnly =>
            {
                Ok(())
            }
            Err(error) => Err(format!(
                "read-only staging write failed with the wrong SQLite error: {error}"
            )
            .into()),
            Ok(rows) => {
                Err(format!("read-only staging attachment unexpectedly deleted {rows} rows").into())
            }
        }
    }

    fn prove_staging_rejections(root: &Path) -> Result<(), Box<dyn Error>> {
        for case in [
            "wrong-root",
            "wrong-instance",
            "wrong-base",
            "corrupt-schema",
            "failed-quick-check",
        ] {
            let case_dir = root.join(case);
            fs::create_dir(&case_dir)?;
            let live_path = case_dir.join("live.db");
            let stage_path = case_dir.join("stage.db");
            let repo_root = case_dir.join("repo");
            fs::create_dir(&repo_root)?;
            let mut live = AtlasStore::open(&live_path)?;
            live.set_project_root(&repo_root)?;
            live.replace_scan(&[file_node("src/live.rs", "live-hash")])?;
            live.replace_file_texts_for_paths(
                &["src/live.rs".to_owned()],
                &[file_text("src/live.rs", "last valid generation")],
            )?;
            let base = live.publication_state()?;
            let staging = live.create_structural_staging(&live_path, &stage_path, &repo_root)?;
            let stage = AtlasStore::open(staging.path())?;
            match case {
                "wrong-root" => stage.set_project_root(&case_dir.join("foreign-repo"))?,
                "wrong-instance" => {
                    stage.connection.execute(
                        "UPDATE metadata SET value = '11111111111111111111111111111111'
                         WHERE key = 'project_instance_id'",
                        [],
                    )?;
                }
                "wrong-base" => {
                    stage.connection.execute(
                        "UPDATE graph_publication_state
                         SET active_slot = 'b', active_epoch = 1
                         WHERE singleton = 1",
                        [],
                    )?;
                }
                "corrupt-schema" => {
                    stage
                        .connection
                        .execute("DROP TRIGGER graph_publication_state_delete_guard", [])?;
                }
                "failed-quick-check" => {
                    let schema_version =
                        stage
                            .connection
                            .query_row("PRAGMA schema_version", [], |row| row.get::<_, i64>(0))?;
                    stage
                        .connection
                        .execute_batch("PRAGMA writable_schema = ON;")?;
                    stage.connection.execute(
                        "UPDATE sqlite_schema SET rootpage = 2147483647
                         WHERE type = 'index' AND name = 'idx_nodes_kind'",
                        [],
                    )?;
                    stage
                        .connection
                        .execute_batch("PRAGMA writable_schema = OFF;")?;
                    stage.connection.pragma_update(
                        None,
                        "schema_version",
                        schema_version.saturating_add(1),
                    )?;
                }
                other => return Err(format!("unknown staging rejection case: {other}").into()),
            }
            checkpoint(&stage.connection)?;
            drop(stage);

            let error = match live.publish_structural_staging(&staging) {
                Err(error) => error,
                Ok(publication) => {
                    return Err(format!(
                        "staging rejection case {case} published unexpectedly: {publication:?}"
                    )
                    .into());
                }
            };
            if case == "failed-quick-check" && !error.to_string().contains("quick_check") {
                return Err(format!("quick-check rejection lost its cause: {error}").into());
            }
            require_eq(
                &live.publication_state()?,
                &base,
                &format!("publication state after rejection case {case}"),
            )?;
            let active = require_some(
                live.load_file_text("src/live.rs")?,
                &format!("active text after rejection case {case}"),
            )?;
            require_eq(
                &active.content,
                &"last valid generation".to_owned(),
                &format!("active generation after rejection case {case}"),
            )?;
        }
        Ok(())
    }

    fn prove_import_rollback(root: &Path) -> Result<(), Box<dyn Error>> {
        let case_dir = root.join("import-rollback");
        fs::create_dir(&case_dir)?;
        let live_path = case_dir.join("live.db");
        let stage_path = case_dir.join("stage.db");
        let repo_root = case_dir.join("repo");
        fs::create_dir(&repo_root)?;
        let mut live = AtlasStore::open(&live_path)?;
        live.set_project_root(&repo_root)?;
        live.replace_scan(&[file_node("src/live.rs", "live-hash")])?;
        live.replace_file_texts_for_paths(
            &["src/live.rs".to_owned()],
            &[file_text("src/live.rs", "last valid generation")],
        )?;
        live.connection.execute(
            "INSERT INTO metadata(key, value) VALUES('authored_note', 'rollback-preserved')",
            [],
        )?;
        let base = live.publication_state()?;
        let inactive_before = structural_counts(&live.connection, StructuralSlot::B)?;

        let staging = live.create_structural_staging(&live_path, &stage_path, &repo_root)?;
        let mut stage = AtlasStore::open(staging.path())?;
        stage.prepare_structural_full_scan()?;
        stage.replace_scan(&[file_node("src/candidate.rs", "candidate-hash")])?;
        stage.replace_file_texts_for_paths(
            &["src/candidate.rs".to_owned()],
            &[file_text("src/candidate.rs", "candidate generation")],
        )?;
        stage.seal_structural_staging(&staging)?;
        drop(stage);

        let has_fts = live.connection.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_schema
                 WHERE type = 'table' AND name = 'file_text_fts'
             )",
            [],
            |row| row.get::<_, i64>(0),
        )? != 0;
        require(
            has_fts,
            "bundled SQLite must expose the tested FTS import boundary",
        )?;
        live.connection.execute_batch(
            "DROP TRIGGER file_text_fts_insert;
             CREATE TRIGGER file_text_fts_insert
             AFTER INSERT ON file_texts
             BEGIN
                 SELECT CASE
                     WHEN new.structural_slot != (
                         SELECT active_slot FROM graph_publication_state WHERE singleton = 1
                     )
                     THEN RAISE(ABORT, 'injected inactive-slot import failure')
                 END;
                 INSERT INTO file_text_fts(
                     structural_slot, last_changed_epoch, path, content
                 ) VALUES(
                     new.structural_slot, new.last_changed_epoch, new.path, new.content
                 );
             END;",
        )?;

        let error = match live.publish_structural_staging(&staging) {
            Err(error) => error,
            Ok(publication) => {
                return Err(format!(
                    "injected inactive-slot import failure published unexpectedly: {publication:?}"
                )
                .into());
            }
        };
        require(
            error
                .to_string()
                .contains("injected inactive-slot import failure"),
            "inactive-slot import failure lost its root cause",
        )?;
        require_eq(
            &live.publication_state()?,
            &base,
            "publication state after import rollback",
        )?;
        let active = require_some(
            live.load_file_text("src/live.rs")?,
            "active text after import rollback",
        )?;
        require_eq(
            &active.content,
            &"last valid generation".to_owned(),
            "active text after import rollback",
        )?;
        require_eq(
            &structural_counts(&live.connection, StructuralSlot::B)?,
            &inactive_before,
            "inactive structural counts after rollback",
        )?;
        require_eq(
            &live.connection.query_row(
                "SELECT value FROM metadata WHERE key = 'authored_note'",
                [],
                |row| row.get::<_, String>(0),
            )?,
            &"rollback-preserved".to_owned(),
            "authored metadata after rollback",
        )?;
        Ok(())
    }

    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }

    fn require_eq<T>(actual: &T, expected: &T, field: &str) -> Result<(), Box<dyn Error>>
    where
        T: std::fmt::Debug + PartialEq,
    {
        if actual == expected {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "{field} mismatch: expected {expected:?}, found {actual:?}"
            ))
            .into())
        }
    }

    fn require_some<T>(value: Option<T>, field: &str) -> Result<T, Box<dyn Error>> {
        value.ok_or_else(|| io::Error::other(format!("{field} is missing")).into())
    }

    fn checkpoint(connection: &Connection) -> DbResult<()> {
        let busy = connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            row.get::<_, i64>(0)
        })?;
        if busy != 0 {
            return Err(publication_error(format!(
                "test checkpoint remained busy: {busy}"
            )));
        }
        Ok(())
    }

    fn file_node(path: &str, hash: &str) -> Node {
        Node {
            path: path.to_owned(),
            kind: NodeKind::File,
            parent_path: Some("src".to_owned()),
            extension: Some(".rs".to_owned()),
            language: Some("rust".to_owned()),
            size_bytes: Some(16),
            mtime_ns: Some(1),
            content_hash: Some(hash.to_owned()),
        }
    }

    fn file_text(path: &str, content: &str) -> IndexedFileText {
        IndexedFileText {
            path: path.to_owned(),
            content_hash: Some(format!("{path}-hash")),
            byte_count: content.len(),
            line_count: 1,
            content: content.to_owned(),
        }
    }

    fn insert_graph_entity(
        connection: &Connection,
        discriminator: &str,
        path: &str,
    ) -> DbResult<()> {
        let digest = blake3::hash(discriminator.as_bytes());
        let instance = schema::project_instance_id(connection)?;
        connection.execute(
            "INSERT INTO graph_entities(
                 stable_key_digest, stable_key_version, stable_key_canonical,
                 project_instance_id, entity_kind, repository_path, qualified_name,
                 parser_kind, parser_identity, parser_version,
                 structural_slot, last_changed_epoch
             )
             SELECT ?1, 1, ?2, ?3, 'file', ?4, ?5,
                    'structural', 'publication-test', '1', active_slot, active_epoch
             FROM graph_publication_state WHERE singleton = 1",
            params![
                digest.as_bytes().as_slice(),
                discriminator.as_bytes(),
                instance.as_bytes().as_slice(),
                path,
                format!("crate::{discriminator}")
            ],
        )?;
        Ok(())
    }

    fn structural_counts(connection: &Connection, slot: StructuralSlot) -> DbResult<Vec<i64>> {
        STRUCTURAL_TABLES
            .iter()
            .map(|table| {
                connection
                    .query_row(
                        &format!("SELECT COUNT(*) FROM {table} WHERE structural_slot = ?1"),
                        [slot_text(slot)],
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(DbError::from)
            })
            .collect()
    }
}
