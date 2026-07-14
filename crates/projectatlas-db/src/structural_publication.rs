//! Parent-owned atomic publication for full scans and incremental structural deltas.

use crate::{
    AtlasStore, DbError, DbResult, IndexedFileText, clear_node_summary_in_connection,
    clear_source_index_in_connection, mark_paths_absent_in_connection,
    node_id_for_path_in_connection, normalize_metadata_path,
    replace_compact_symbol_graph_at_publication, schema, set_node_summary_in_connection,
    set_suggested_purpose_in_connection, sql_string_literals, sqlite_read_uri,
    upsert_file_text_for_publication, upsert_node,
};
use projectatlas_core::graph::{IndexEpoch, ProjectInstanceId, PublicationState, StructuralSlot};
use projectatlas_core::symbols::CompactSymbolGraph;
use projectatlas_core::{AGENT_REVIEWED_SOURCE_VALUES, Node, PurposeSource, PurposeStatus};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use std::collections::{BTreeMap, HashSet};
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
/// Versioned content and VCS identity of the last published structural generation.
const STRUCTURAL_STATE_SIGNATURE_KEY: &str = "structural_state_signature_v1";

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

/// One validated active-slot delta ready for atomic structural publication.
#[derive(Debug)]
pub struct IncrementalStructuralDelta {
    /// Canonical project root this delta is allowed to mutate.
    pub project_root: PathBuf,
    /// Slot and epoch used while the delta was planned.
    pub base_publication: PublicationState,
    /// Last-published signature used while the delta was planned.
    pub base_state_signature: Option<String>,
    /// Complete content and VCS signature represented by the delta.
    pub target_state_signature: String,
    /// Exact paths whose derived dependency closure must be invalidated.
    pub affected_paths: Vec<String>,
    /// Current scan rows to insert or update.
    pub nodes: Vec<Node>,
    /// Missing paths whose rows and descendants must become absent.
    pub absent_paths: Vec<String>,
    /// Current UTF-8 lexical rows for affected paths.
    pub file_texts: Vec<IndexedFileText>,
    /// Parser-owned compatibility mutations for affected files.
    pub source_mutations: Vec<IncrementalSourceMutation>,
    /// Structural summary mutations applied after parser mutations.
    pub summary_mutations: Vec<IncrementalSummaryMutation>,
    /// Durable built-in purposes that may replace only non-approved state.
    pub built_in_purposes: Vec<IncrementalBuiltInPurpose>,
}

/// Parser-owned compatibility mutation for one affected source file.
#[derive(Debug)]
pub enum IncrementalSourceMutation {
    /// Replace the file's parser graph and observed summary together.
    Replace {
        /// Complete parser graph for the affected path.
        graph: CompactSymbolGraph,
        /// Observed parser summary for the affected path.
        summary: String,
        /// Optional generated purpose suggestion.
        purpose_suggestion: Option<String>,
    },
    /// Clear stale parser output, optionally preserving a structural summary.
    Clear {
        /// Repository-relative affected path.
        path: String,
        /// Whether the node-level summary belongs to another structural producer.
        preserve_node_summary: bool,
    },
}

/// Structural-summary mutation for one affected file.
#[derive(Debug)]
pub enum IncrementalSummaryMutation {
    /// Set the observed summary and optional generated purpose suggestion.
    Set {
        /// Repository-relative affected path.
        path: String,
        /// Deterministic observed summary.
        summary: String,
        /// Optional generated purpose suggestion.
        purpose_suggestion: Option<String>,
    },
    /// Remove a stale observed summary.
    Clear {
        /// Repository-relative affected path.
        path: String,
    },
}

/// Built-in purpose assignment owned by runtime policy.
#[derive(Debug)]
pub struct IncrementalBuiltInPurpose {
    /// Repository-relative built-in path.
    pub path: String,
    /// Durable built-in purpose text.
    pub purpose: String,
    /// Durable source identity for the purpose.
    pub source: PurposeSource,
}

/// Outcome of attempting to publish one incremental structural delta.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IncrementalPublication {
    /// The delta committed and advanced the active epoch exactly once.
    Published(PublicationState),
    /// Another process already published the exact target signature.
    Unchanged(PublicationState),
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
        let project_root = normalize_metadata_path(project_root)?;
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
        clear_staging_structural_state_signature(staging_path)?;

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

    /// Read the versioned content and VCS signature of the active generation.
    ///
    /// # Errors
    ///
    /// Returns an error when metadata cannot be read.
    pub fn structural_state_signature(&self) -> DbResult<Option<String>> {
        load_metadata(&self.connection, None, STRUCTURAL_STATE_SIGNATURE_KEY)
    }

    /// Store a structural state signature in the exact staging database before sealing it.
    ///
    /// # Errors
    ///
    /// Returns an error when the signature is empty or metadata cannot be written.
    pub fn set_staged_structural_state_signature(
        &self,
        staging: &StructuralStaging,
        signature: &str,
    ) -> DbResult<()> {
        require_structural_state_signature(signature)?;
        validate_staging_identity(&self.connection, staging)?;
        upsert_structural_state_signature(&self.connection, signature)
    }

    /// Atomically apply one affected-row delta to the active structural slot.
    ///
    /// The retained inactive slot is never read, copied, or mutated. The target
    /// signature is checked again after acquiring the per-database write lock so
    /// concurrent processes that planned the same state coalesce without writes.
    ///
    /// # Errors
    ///
    /// Returns an error if the project root, base slot/epoch, base signature,
    /// affected rows, reconciliation, or final publication metadata is invalid.
    /// Every error rolls back all compatibility and structural row mutations.
    pub fn publish_incremental_structural_delta(
        &mut self,
        delta: &IncrementalStructuralDelta,
    ) -> DbResult<IncrementalPublication> {
        require_structural_state_signature(&delta.target_state_signature)?;
        validate_incremental_delta(delta)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let live_publication = load_publication_state(&transaction)?;
        let live_signature = load_metadata(&transaction, None, STRUCTURAL_STATE_SIGNATURE_KEY)?;
        if live_signature.as_deref() == Some(delta.target_state_signature.as_str()) {
            return Ok(IncrementalPublication::Unchanged(live_publication));
        }
        if live_publication != delta.base_publication {
            return Err(publication_error(format!(
                "live publication changed while incremental work was planned: expected {:?}, found {:?}",
                delta.base_publication, live_publication
            )));
        }
        if live_signature != delta.base_state_signature {
            return Err(publication_error(format!(
                "live structural signature changed while incremental work was planned: expected {:?}, found {:?}",
                delta.base_state_signature, live_signature
            )));
        }
        let project_root = bind_incremental_project_root(&transaction, &delta.project_root)?;

        let next_publication = live_publication
            .next_incremental()
            .map_err(|error| publication_error(error.to_string()))?;
        let active_slot = slot_text(live_publication.active_slot);
        let next_epoch = i64::try_from(next_publication.active_epoch.get()).map_err(|source| {
            publication_error(format!(
                "incremental publication epoch exceeds SQLite INTEGER: {source}"
            ))
        })?;

        apply_incremental_mutations(&transaction, delta, active_slot, next_epoch)?;

        upsert_structural_state_signature(&transaction, &delta.target_state_signature)?;
        let changed = transaction.execute(
            "UPDATE graph_publication_state
             SET active_epoch = ?1
             WHERE singleton = 1 AND active_slot = ?2 AND active_epoch = ?3",
            params![
                next_epoch,
                active_slot,
                i64::try_from(live_publication.active_epoch.get()).map_err(|source| {
                    publication_error(format!("base epoch exceeds SQLite INTEGER: {source}"))
                })?
            ],
        )?;
        if changed != 1 {
            return Err(publication_error(
                "publication singleton changed before the incremental epoch advance",
            ));
        }
        if load_publication_state(&transaction)? != next_publication {
            return Err(publication_error(
                "publication singleton did not reconcile after the incremental epoch advance",
            ));
        }
        schema::reconcile_incremental_structural_publication(
            &transaction,
            &project_root,
            next_publication,
            &delta.affected_paths,
        )?;
        transaction.commit()?;
        Ok(IncrementalPublication::Published(next_publication))
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

/// Reject an absent or ambiguous structural state identity.
fn require_structural_state_signature(signature: &str) -> DbResult<()> {
    if signature.trim().is_empty() {
        return Err(publication_error(
            "structural state signature must not be empty",
        ));
    }
    Ok(())
}

/// Remove the copied live-generation signature so every stage must set its own identity.
fn clear_staging_structural_state_signature(staging_path: &Path) -> DbResult<()> {
    let staging = Connection::open_with_flags(staging_path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    staging.execute(
        "DELETE FROM metadata WHERE key = ?1",
        [STRUCTURAL_STATE_SIGNATURE_KEY],
    )?;
    let busy = staging.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
        row.get::<_, i64>(0)
    })?;
    if busy != 0 {
        return Err(publication_error(format!(
            "staging signature reset WAL checkpoint remained busy: {busy}"
        )));
    }
    Ok(())
}

/// Store the last-published structural identity within the caller's write boundary.
fn upsert_structural_state_signature(connection: &Connection, signature: &str) -> DbResult<()> {
    connection.execute(
        "INSERT INTO metadata(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![STRUCTURAL_STATE_SIGNATURE_KEY, signature],
    )?;
    Ok(())
}

/// Reject malformed or out-of-closure incremental mutations before taking the write lock.
fn validate_incremental_delta(delta: &IncrementalStructuralDelta) -> DbResult<()> {
    let mut unique_affected_paths = HashSet::new();
    for path in &delta.affected_paths {
        require_normalized_delta_path(path, "affected path")?;
        if !unique_affected_paths.insert(path.as_str()) {
            return Err(publication_error(format!(
                "incremental delta repeats affected path {path:?}"
            )));
        }
    }

    for node in &delta.nodes {
        require_path_in_affected_closure(delta, &node.path, "node")?;
    }
    for path in &delta.absent_paths {
        require_path_in_affected_closure(delta, path, "absent path")?;
    }
    for text in &delta.file_texts {
        require_path_in_affected_closure(delta, &text.path, "lexical row")?;
    }
    for mutation in &delta.source_mutations {
        let path = match mutation {
            IncrementalSourceMutation::Replace { graph, .. } => graph.path(),
            IncrementalSourceMutation::Clear { path, .. } => path,
        };
        require_path_in_affected_closure(delta, path, "source mutation")?;
    }
    for mutation in &delta.summary_mutations {
        let path = match mutation {
            IncrementalSummaryMutation::Set { path, .. }
            | IncrementalSummaryMutation::Clear { path } => path,
        };
        require_path_in_affected_closure(delta, path, "summary mutation")?;
    }
    for purpose in &delta.built_in_purposes {
        require_path_in_affected_closure(delta, &purpose.path, "built-in purpose")?;
    }
    Ok(())
}

/// Require one canonical repository key and reject path traversal or native separators.
fn require_normalized_delta_path(path: &str, role: &str) -> DbResult<()> {
    let valid = path == "."
        || (!path.is_empty()
            && !path.starts_with('/')
            && !path.ends_with('/')
            && !path.contains('\\')
            && path
                .split('/')
                .all(|component| !component.is_empty() && component != "." && component != ".."));
    if valid {
        Ok(())
    } else {
        Err(publication_error(format!(
            "incremental delta {role} is not a normalized repository path: {path:?}"
        )))
    }
}

/// Require every prepared mutation to remain inside the declared affected closure.
fn require_path_in_affected_closure(
    delta: &IncrementalStructuralDelta,
    path: &str,
    role: &str,
) -> DbResult<()> {
    require_normalized_delta_path(path, role)?;
    if delta
        .affected_paths
        .iter()
        .any(|affected| path_is_within_affected_path(path, affected))
    {
        Ok(())
    } else {
        Err(publication_error(format!(
            "incremental delta {role} {path:?} is outside the declared affected closure"
        )))
    }
}

/// Return whether one repository key is equal to or below an affected path.
fn path_is_within_affected_path(path: &str, affected: &str) -> bool {
    affected == "."
        || path == affected
        || path
            .strip_prefix(affected)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

/// Bind or validate the canonical project root for incremental publication.
fn bind_incremental_project_root(
    transaction: &Transaction<'_>,
    project_root: &Path,
) -> DbResult<String> {
    let requested = normalize_metadata_path(project_root)?;
    match load_project_root(transaction, None)? {
        Some(live) if live != requested => Err(publication_error(format!(
            "live project root does not match the incremental delta root: live={live:?}, requested={requested:?}"
        ))),
        Some(_) => Ok(requested),
        None => {
            transaction.execute(
                "INSERT INTO metadata(key, value) VALUES('project_root', ?1)",
                [&requested],
            )?;
            Ok(requested)
        }
    }
}

/// Apply one prepared mutation batch without weakening the parent transaction.
fn apply_incremental_mutations(
    transaction: &Transaction<'_>,
    delta: &IncrementalStructuralDelta,
    active_slot: &str,
    next_epoch: i64,
) -> DbResult<()> {
    invalidate_structural_paths(transaction, active_slot, &delta.affected_paths)?;
    for node in &delta.nodes {
        upsert_node(transaction, node)?;
    }
    for purpose in &delta.built_in_purposes {
        set_built_in_purpose(transaction, purpose)?;
    }
    mark_paths_absent_in_connection(transaction, &delta.absent_paths)?;
    for text in &delta.file_texts {
        upsert_file_text_for_publication(transaction, text, active_slot, next_epoch)?;
    }
    for mutation in &delta.source_mutations {
        apply_incremental_source_mutation(transaction, mutation, active_slot, next_epoch)?;
    }
    for mutation in &delta.summary_mutations {
        apply_incremental_summary_mutation(transaction, mutation)?;
    }
    Ok(())
}

/// Invalidate only affected active-slot typed graph, coverage, evidence, and lexical rows.
fn invalidate_structural_paths(
    transaction: &Transaction<'_>,
    active_slot: &str,
    paths: &[String],
) -> DbResult<()> {
    let mut delete_evidence = transaction.prepare_cached(
        "DELETE FROM graph_evidence_occurrences
         WHERE structural_slot = ?1
           AND (origin_repository_path = ?2
                OR (origin_repository_path >= ?3 AND origin_repository_path < ?4))",
    )?;
    let mut delete_resolution = transaction.prepare_cached(
        "DELETE FROM graph_resolution_occurrences
         WHERE structural_slot = ?1
           AND (origin_repository_path = ?2
                OR (origin_repository_path >= ?3 AND origin_repository_path < ?4))",
    )?;
    let mut delete_coverage = transaction.prepare_cached(
        "DELETE FROM graph_coverage
         WHERE structural_slot = ?1
           AND (repository_path = ?2
                OR (repository_path >= ?3 AND repository_path < ?4))",
    )?;
    let mut delete_entities = transaction.prepare_cached(
        "DELETE FROM graph_entities
         WHERE structural_slot = ?1
           AND (repository_path = ?2
                OR (repository_path >= ?3 AND repository_path < ?4))",
    )?;
    let mut delete_texts = transaction.prepare_cached(
        "DELETE FROM file_texts
         WHERE structural_slot = ?1
           AND (path = ?2 OR (path >= ?3 AND path < ?4))",
    )?;
    for path in paths {
        if path.is_empty() {
            continue;
        }
        if path == "." {
            for table in STRUCTURAL_DELETE_ORDER {
                transaction.execute(
                    &format!("DELETE FROM {table} WHERE structural_slot = ?1"),
                    [active_slot],
                )?;
            }
            continue;
        }
        let (descendant_start, descendant_end) = crate::sqlite_descendant_bounds(path);
        delete_evidence.execute(params![active_slot, path, descendant_start, descendant_end])?;
        delete_resolution.execute(params![active_slot, path, descendant_start, descendant_end])?;
        delete_coverage.execute(params![active_slot, path, descendant_start, descendant_end])?;
        delete_entities.execute(params![active_slot, path, descendant_start, descendant_end])?;
        delete_texts.execute(params![active_slot, path, descendant_start, descendant_end])?;
    }
    Ok(())
}

/// Apply one runtime-owned built-in purpose without replacing approved authored state.
fn set_built_in_purpose(
    connection: &Connection,
    purpose: &IncrementalBuiltInPurpose,
) -> DbResult<()> {
    let node_id = node_id_for_path_in_connection(connection, &purpose.path)?;
    let reviewed_sources = sql_string_literals(AGENT_REVIEWED_SOURCE_VALUES);
    let missing = PurposeStatus::Missing.as_str();
    let suggested = PurposeStatus::Suggested.as_str();
    let stale = PurposeStatus::Stale.as_str();
    let sql = format!(
        "INSERT INTO purposes(node_id, purpose, source, status, updated_at)
         VALUES(?1, ?2, ?3, 'approved', CURRENT_TIMESTAMP)
         ON CONFLICT(node_id) DO UPDATE SET
             purpose = excluded.purpose,
             source = excluded.source,
             status = 'approved',
             updated_at = CURRENT_TIMESTAMP
         WHERE purposes.status IN ('{missing}', '{suggested}')
            OR (purposes.status = '{stale}'
                AND purposes.source NOT IN ({reviewed_sources}))"
    );
    connection.prepare_cached(&sql)?.execute(params![
        node_id,
        purpose.purpose,
        purpose.source.to_string()
    ])?;
    Ok(())
}

/// Apply one parser-owned compatibility mutation inside the publication transaction.
fn apply_incremental_source_mutation(
    connection: &Connection,
    mutation: &IncrementalSourceMutation,
    structural_slot: &str,
    last_changed_epoch: i64,
) -> DbResult<()> {
    match mutation {
        IncrementalSourceMutation::Replace {
            graph,
            summary,
            purpose_suggestion,
        } => {
            set_node_summary_in_connection(connection, graph.path(), summary)?;
            if let Some(suggestion) = purpose_suggestion {
                set_suggested_purpose_in_connection(connection, graph.path(), suggestion)?;
            }
            replace_compact_symbol_graph_at_publication(
                connection,
                graph,
                structural_slot,
                last_changed_epoch,
            )
        }
        IncrementalSourceMutation::Clear {
            path,
            preserve_node_summary,
        } => clear_source_index_in_connection(connection, path, *preserve_node_summary),
    }
}

/// Apply one structural-summary mutation inside the publication transaction.
fn apply_incremental_summary_mutation(
    connection: &Connection,
    mutation: &IncrementalSummaryMutation,
) -> DbResult<()> {
    match mutation {
        IncrementalSummaryMutation::Set {
            path,
            summary,
            purpose_suggestion,
        } => {
            set_node_summary_in_connection(connection, path, summary)?;
            if let Some(suggestion) = purpose_suggestion {
                set_suggested_purpose_in_connection(connection, path, suggestion)?;
            }
            Ok(())
        }
        IncrementalSummaryMutation::Clear { path } => {
            clear_node_summary_in_connection(connection, path)
        }
    }
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
    let signature = load_metadata(
        &transaction,
        Some(STAGING_SCHEMA),
        STRUCTURAL_STATE_SIGNATURE_KEY,
    )?
    .ok_or_else(|| publication_error("staging structural state signature is missing"))?;
    require_structural_state_signature(&signature)?;
    upsert_structural_state_signature(&transaction, &signature)?;

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
    schema::reconcile_full_structural_publication(
        &transaction,
        &staging.project_root,
        next_publication,
    )?;
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
    if source_slot == target_slot {
        return Err(publication_error(
            "full publication refused to clean the active structural slot",
        ));
    }
    for table in STRUCTURAL_DELETE_ORDER {
        transaction
            .execute(
                &format!("DELETE FROM {table} WHERE structural_slot = ?1"),
                [target_slot],
            )
            .map_err(|error| {
                publication_error(format!(
                    "recoverable inactive-slot cleanup failure for slot {target_slot:?} in table {table}: {error}; the publication transaction will restore the retained rollback slot and leave the active slot unchanged"
                ))
            })?;
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
    for table in schema::STRUCTURAL_DERIVED_TABLES {
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
pub(crate) fn load_publication_state(connection: &Connection) -> DbResult<PublicationState> {
    let (slot, epoch) = connection.query_row(
        "SELECT active_slot, active_epoch
         FROM graph_publication_state
         WHERE singleton = 1",
        [],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    )?;
    publication_state_from_sql(&slot, epoch)
}

/// Apply one direct graph mutation and publish its new active-slot epoch atomically.
pub(crate) fn publish_active_graph_mutation(
    connection: &Connection,
    mutate: impl FnOnce(&Connection, &str, i64) -> DbResult<()>,
) -> DbResult<PublicationState> {
    let transaction = connection.unchecked_transaction()?;
    let current = load_publication_state(&transaction)?;
    let next = current
        .next_incremental()
        .map_err(|error| publication_error(error.to_string()))?;
    let active_slot = slot_text(current.active_slot);
    let current_epoch = i64::try_from(current.active_epoch.get()).map_err(|source| {
        publication_error(format!(
            "current graph epoch exceeds SQLite INTEGER: {source}"
        ))
    })?;
    let next_epoch = i64::try_from(next.active_epoch.get()).map_err(|source| {
        publication_error(format!("next graph epoch exceeds SQLite INTEGER: {source}"))
    })?;
    mutate(&transaction, active_slot, next_epoch)?;
    let changed = transaction.execute(
        "UPDATE graph_publication_state
         SET active_epoch = ?1
         WHERE singleton = 1 AND active_slot = ?2 AND active_epoch = ?3",
        params![next_epoch, active_slot, current_epoch],
    )?;
    if changed != 1 || load_publication_state(&transaction)? != next {
        return Err(publication_error(
            "direct graph mutation did not publish its active-slot epoch",
        ));
    }
    transaction.commit()?;
    Ok(next)
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
pub(crate) fn slot_text(slot: StructuralSlot) -> &'static str {
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
    load_metadata(connection, schema_name, "project_root")?
        .map(|root| normalize_metadata_path(Path::new(&root)))
        .transpose()
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
    use std::io::Write as _;
    use std::sync::{Arc, Barrier};
    use std::thread;

    // Fixed allowance for extra B-tree interior/page-placement touches in the
    // larger fixture; it must not grow with the unrelated-row multiplier.
    const MAX_UNRELATED_SCALE_FRAME_DELTA: usize = 12;

    #[derive(Clone, Copy, Debug)]
    enum ReconciliationFault {
        CandidateUniqueness,
        MissingEndpoint,
        UnknownRelationFamily,
        CandidateCount,
        CandidateTotalBoundary,
        CandidateRetentionBudget,
        CandidateOrdinalGap,
        CoverageContract,
        CoverageCount,
        InvalidUtf8,
        WrongRoot,
        InvalidRowSlot,
        FutureRowEpoch,
        WrongPublication,
        MissingSchemaGuard,
        NoOpSchemaGuard,
        MissingUniqueConstraint,
        AlteredPathCollation,
        PartialPathIndex,
        AlteredForeignKeyAction,
    }

    impl ReconciliationFault {
        const ALL: [Self; 20] = [
            Self::CandidateUniqueness,
            Self::MissingEndpoint,
            Self::UnknownRelationFamily,
            Self::CandidateCount,
            Self::CandidateTotalBoundary,
            Self::CandidateRetentionBudget,
            Self::CandidateOrdinalGap,
            Self::CoverageContract,
            Self::CoverageCount,
            Self::InvalidUtf8,
            Self::WrongRoot,
            Self::InvalidRowSlot,
            Self::FutureRowEpoch,
            Self::WrongPublication,
            Self::MissingSchemaGuard,
            Self::NoOpSchemaGuard,
            Self::MissingUniqueConstraint,
            Self::AlteredPathCollation,
            Self::PartialPathIndex,
            Self::AlteredForeignKeyAction,
        ];

        const fn expected_diagnostic(self) -> &'static str {
            match self {
                Self::CandidateUniqueness => "candidate target uniqueness",
                Self::MissingEndpoint => "endpoint foreign-key",
                Self::UnknownRelationFamily => "relation family",
                Self::CandidateCount
                | Self::CandidateTotalBoundary
                | Self::CandidateRetentionBudget
                | Self::CandidateOrdinalGap => "candidate counts",
                Self::CoverageContract => "coverage row",
                Self::CoverageCount => "coverage counts",
                Self::InvalidUtf8 => "invalid UTF-8",
                Self::WrongRoot => "project root",
                Self::InvalidRowSlot | Self::FutureRowEpoch => "slot or epoch",
                Self::WrongPublication => "publication state",
                Self::MissingSchemaGuard => "guard",
                Self::NoOpSchemaGuard
                | Self::AlteredPathCollation
                | Self::PartialPathIndex
                | Self::AlteredForeignKeyAction => "canonical SQL",
                Self::MissingUniqueConstraint => "UNIQUE constraint",
            }
        }

        const fn disables_foreign_keys(self) -> bool {
            matches!(
                self,
                Self::MissingEndpoint
                    | Self::InvalidRowSlot
                    | Self::MissingUniqueConstraint
                    | Self::AlteredPathCollation
                    | Self::AlteredForeignKeyAction
            )
        }

        const fn ignores_check_constraints(self) -> bool {
            matches!(self, Self::UnknownRelationFamily | Self::InvalidRowSlot)
        }
    }

    #[test]
    fn task_arri_ut_arri_4_12() -> Result<(), Box<dyn Error>> {
        prove_parent_owned_full_scan_publication()
    }

    #[test]
    fn task_arri_ut_arri_4_13() -> Result<(), Box<dyn Error>> {
        prove_transactional_reader_and_rollback_retention()
    }

    #[test]
    fn task_arri_ut_arri_7_6() -> Result<(), Box<dyn Error>> {
        prove_parent_owned_full_scan_publication()
    }

    #[test]
    fn task_arri_ut_arri_4_14() -> Result<(), Box<dyn Error>> {
        prove_incremental_publication_atomicity()
    }

    #[test]
    fn task_arri_ut_arri_4_22_incremental_graph_uses_published_epoch() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(&root)?;
        store.replace_scan(&[file_node("src/lib.rs", "epoch-base")])?;
        upsert_structural_state_signature(&store.connection, "epoch-base")?;
        let base = store.publication_state()?;
        let source = "pub fn source() { target(); }\nfn target() {}\n";
        let graph = CompactSymbolGraph::try_from(projectatlas_symbols::extract_symbol_graph(
            "src/lib.rs",
            Some("rust"),
            source,
        ))?;
        let mut delta = test_incremental_delta(
            &root,
            base,
            Some("epoch-base".to_owned()),
            "epoch-next",
            "epoch-next",
            source,
        );
        delta.source_mutations = vec![IncrementalSourceMutation::Replace {
            graph,
            summary: "Rust source declaring source.".to_owned(),
            purpose_suggestion: None,
        }];

        let publication = match store.publish_incremental_structural_delta(&delta)? {
            IncrementalPublication::Published(publication) => publication,
            IncrementalPublication::Unchanged(publication) => {
                return Err(format!(
                    "incremental graph publication coalesced unexpectedly: {publication:?}"
                )
                .into());
            }
        };
        let entities = store.load_graph_entities_by_qualified_name(
            projectatlas_core::graph::GraphEntityKind::Declaration,
            "source",
            1,
        )?;
        require_eq(&entities.len(), &1, "published declaration count")?;
        require_eq(
            &entities[0].last_changed_epoch,
            &publication.active_epoch,
            "incremental graph row freshness epoch",
        )?;

        let before_direct = store.publication_state()?;
        let expected_direct = before_direct.next_incremental()?;
        let direct_graph = CompactSymbolGraph::try_from(
            projectatlas_symbols::extract_symbol_graph("src/lib.rs", Some("rust"), source),
        )?;
        store.replace_compact_symbol_graph(&direct_graph)?;
        require_eq(
            &store.publication_state()?,
            &expected_direct,
            "direct replacement publishes exactly one epoch",
        )?;
        let direct_entities = store.load_graph_entities_by_qualified_name(
            projectatlas_core::graph::GraphEntityKind::Declaration,
            "source",
            1,
        )?;
        require_eq(&direct_entities.len(), &1, "direct declaration count")?;
        require_eq(
            &direct_entities[0].last_changed_epoch,
            &expected_direct.active_epoch,
            "direct entity freshness epoch",
        )?;
        let direct_relations = store.load_graph_adjacency(
            &direct_entities[0].stable_key_digest,
            crate::GraphRelationDirection::Outbound,
            Some(projectatlas_core::graph::GraphRelationKind::Calls),
            1,
        )?;
        require_eq(&direct_relations.len(), &1, "direct relation count")?;
        require_eq(
            &direct_relations[0].last_changed_epoch,
            &expected_direct.active_epoch,
            "direct relation freshness epoch",
        )?;
        let direct_evidence_epoch = store.connection.query_row(
            "SELECT last_changed_epoch
             FROM graph_evidence_occurrences
             WHERE structural_slot = ?1 AND relation_digest = ?2",
            params![
                slot_text(expected_direct.active_slot),
                &direct_relations[0].stable_key_digest[..]
            ],
            |row| row.get::<_, i64>(0),
        )?;
        require_eq(
            &u64::try_from(direct_evidence_epoch)?,
            &expected_direct.active_epoch.get(),
            "direct evidence freshness epoch",
        )?;

        let mut staging_store = AtlasStore::in_memory()?;
        let staging_base = staging_store.publication_state()?;
        let staging_graph = CompactSymbolGraph::try_from(
            projectatlas_symbols::extract_symbol_graph("src/lib.rs", Some("rust"), source),
        )?;
        staging_store.stage_compact_symbol_graph(&staging_graph)?;
        require_eq(
            &staging_store.publication_state()?,
            &staging_base,
            "staging replacement leaves publication state unchanged",
        )?;
        let staged_entities = staging_store.load_graph_entities_by_qualified_name(
            projectatlas_core::graph::GraphEntityKind::Declaration,
            "source",
            1,
        )?;
        require_eq(&staged_entities.len(), &1, "staged declaration count")?;
        require_eq(
            &staged_entities[0].last_changed_epoch,
            &staging_base.active_epoch,
            "staged row retains the captured base epoch",
        )?;
        Ok(())
    }

    #[test]
    fn task_arri_ut_arri_4_15() -> Result<(), Box<dyn Error>> {
        prove_prepared_mutation_batch_fails_closed()
    }

    #[test]
    fn task_arri_ut_arri_4_16() -> Result<(), Box<dyn Error>> {
        prove_structural_publication_reconciliation()
    }

    #[test]
    fn task_arri_ut_arri_4_19_interrupted_publication() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        prove_import_rollback(temp.path())
    }

    #[test]
    fn task_arri_ut_arri_4_20_retained_slot_cleanup() -> Result<(), Box<dyn Error>> {
        prove_bounded_retained_slot_cleanup()
    }

    #[test]
    fn task_arri_ut_arri_7_7() -> Result<(), Box<dyn Error>> {
        prove_incremental_publication_atomicity()
    }

    #[test]
    fn task_arri_ut_arri_7_18() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        let db_path = temp.path().join("coalescing.db");
        fs::create_dir(&root)?;
        let mut setup = AtlasStore::open(&db_path)?;
        setup.set_project_root(&root)?;
        setup.replace_scan(&[file_node("src/lib.rs", "base-hash")])?;
        setup.replace_file_texts_for_paths(
            &["src/lib.rs".to_owned()],
            &[file_text("src/lib.rs", "base content")],
        )?;
        upsert_structural_state_signature(&setup.connection, "git-content-base")?;
        let base = setup.publication_state()?;
        let base_signature = setup.structural_state_signature()?;
        drop(setup);

        let first_store = AtlasStore::open(&db_path)?;
        let second_store = AtlasStore::open(&db_path)?;
        let barrier = Arc::new(Barrier::new(3));
        let spawn_publication = |worker: &'static str, mut store: AtlasStore| {
            let barrier = Arc::clone(&barrier);
            let root = root.clone();
            let base_signature = base_signature.clone();
            thread::spawn(move || -> Result<IncrementalPublication, String> {
                let delta = test_incremental_delta(
                    &root,
                    base,
                    base_signature,
                    "git-content-dirty",
                    "dirty-hash",
                    "dirty content",
                );
                barrier.wait();
                store
                    .publish_incremental_structural_delta(&delta)
                    .map_err(|error| format!("{worker} publication failed: {error}"))
            })
        };
        let first = spawn_publication("first concurrent connection", first_store);
        let second = spawn_publication("second concurrent connection", second_store);
        barrier.wait();
        let first = first
            .join()
            .map_err(|_panic| io::Error::other("first concurrent publication panicked"))?
            .map_err(io::Error::other)?;
        let second = second
            .join()
            .map_err(|_panic| io::Error::other("second concurrent publication panicked"))?
            .map_err(io::Error::other)?;
        let published_state = base.next_incremental()?;
        let published_count = [&first, &second]
            .into_iter()
            .filter(|outcome| matches!(outcome, IncrementalPublication::Published(_)))
            .count();
        let unchanged_count = [&first, &second]
            .into_iter()
            .filter(|outcome| matches!(outcome, IncrementalPublication::Unchanged(_)))
            .count();
        require_eq(&published_count, &1, "concurrent published outcome count")?;
        require_eq(&unchanged_count, &1, "concurrent unchanged outcome count")?;
        for outcome in [&first, &second] {
            let state = match outcome {
                IncrementalPublication::Published(state)
                | IncrementalPublication::Unchanged(state) => state,
            };
            require_eq(state, &published_state, "concurrent publication state")?;
        }

        let mut final_store = AtlasStore::open(&db_path)?;
        let repeated_delta = test_incremental_delta(
            &root,
            base,
            base_signature,
            "git-content-dirty",
            "dirty-hash",
            "dirty content",
        );
        require_eq(
            &final_store.publish_incremental_structural_delta(&repeated_delta)?,
            &IncrementalPublication::Unchanged(published_state),
            "repeated identical dirty-state coalescing",
        )?;
        require_eq(
            &final_store.publication_state()?,
            &published_state,
            "coalesced dirty state epoch",
        )?;
        require_eq(
            &final_store.structural_state_signature()?,
            &Some("git-content-dirty".to_owned()),
            "coalesced dirty state signature",
        )?;
        Ok(())
    }

    #[test]
    fn task_arri_ut_arri_7_19() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;
        let small =
            measure_incremental_wal_frames(&root, &temp.path().join("write-gate-small.db"), 24)?;
        let large =
            measure_incremental_wal_frames(&root, &temp.path().join("write-gate-large.db"), 400)?;
        require(
            large.database_pages >= small.database_pages.saturating_mul(8),
            "large unrelated graph fixture did not materially exceed the small fixture",
        )?;
        require(
            large.appended_frames
                <= small
                    .appended_frames
                    .saturating_add(MAX_UNRELATED_SCALE_FRAME_DELTA),
            "one-file WAL frames scaled with unrelated graph size",
        )?;
        writeln!(
            io::stdout().lock(),
            "ARRI-7.19 WAL frames: small_pages={}, small_frames={}, large_pages={}, large_frames={}, fixed_frame_allowance={MAX_UNRELATED_SCALE_FRAME_DELTA}",
            small.database_pages,
            small.appended_frames,
            large.database_pages,
            large.appended_frames,
        )?;
        Ok(())
    }

    #[derive(Clone, Copy, Debug)]
    struct WriteGateMeasurement {
        appended_frames: usize,
        database_pages: usize,
    }

    fn measure_incremental_wal_frames(
        root: &Path,
        db_path: &Path,
        unrelated_count: usize,
    ) -> Result<WriteGateMeasurement, Box<dyn Error>> {
        let mut store = AtlasStore::open(db_path)?;
        store.set_project_root(root)?;
        store
            .connection
            .pragma_update(None, "wal_autocheckpoint", 0)?;

        let mut nodes = vec![file_node("src/lib.rs", "base-hash")];
        nodes.extend(
            (0..unrelated_count)
                .map(|index| file_node(&format!("src/file-{index:03}.rs"), "base-hash")),
        );
        let texts = nodes
            .iter()
            .map(|node| file_text(&node.path, &"x".repeat(4096)))
            .collect::<Vec<_>>();
        store.replace_scan(&nodes)?;
        store.replace_file_texts_for_paths(
            &nodes
                .iter()
                .map(|node| node.path.clone())
                .collect::<Vec<_>>(),
            &texts,
        )?;
        let transaction = store.connection.transaction()?;
        for node in &nodes {
            insert_graph_entity(&transaction, &node.path, &node.path)?;
        }
        transaction.commit()?;
        upsert_structural_state_signature(&store.connection, "write-base")?;
        checkpoint(&store.connection)?;

        let base = store.publication_state()?;
        let no_change = IncrementalStructuralDelta {
            project_root: root.to_path_buf(),
            base_publication: base,
            base_state_signature: Some("write-base".to_owned()),
            target_state_signature: "write-base".to_owned(),
            affected_paths: Vec::new(),
            nodes: Vec::new(),
            absent_paths: Vec::new(),
            file_texts: Vec::new(),
            source_mutations: Vec::new(),
            summary_mutations: Vec::new(),
            built_in_purposes: Vec::new(),
        };
        let db_before = fs::read(db_path)?;
        let wal_path = sqlite_sidecar_path(db_path, "-wal");
        let wal_before = read_optional_file(&wal_path)?;
        require_eq(
            &store.publish_incremental_structural_delta(&no_change)?,
            &IncrementalPublication::Unchanged(base),
            "no-change publication outcome",
        )?;
        require_eq(
            &fs::read(db_path)?,
            &db_before,
            "no-change main database byte identity",
        )?;
        require_eq(
            &read_optional_file(&wal_path)?,
            &wal_before,
            "no-change WAL byte identity",
        )?;

        let inactive_before = structural_counts(&store.connection, StructuralSlot::B)?;
        let one_file = test_incremental_delta(
            root,
            base,
            Some("write-base".to_owned()),
            "write-one-file",
            "changed-hash",
            "changed content",
        );
        require(
            matches!(
                store.publish_incremental_structural_delta(&one_file)?,
                IncrementalPublication::Published(_)
            ),
            "one-file delta did not publish",
        )?;
        require_eq(
            &fs::read(db_path)?,
            &db_before,
            "one-file main database byte identity before checkpoint",
        )?;
        let wal_after = read_optional_file(&wal_path)?;
        require(
            wal_after.starts_with(&wal_before),
            "one-file update did not append to the checkpointed WAL",
        )?;
        let page_size = store
            .connection
            .query_row("PRAGMA page_size", [], |row| row.get::<_, usize>(0))?;
        let wal_frame_size = page_size.saturating_add(24);
        let appended_wal_bytes = wal_after.len().saturating_sub(wal_before.len());
        require(
            appended_wal_bytes >= 32 && (appended_wal_bytes - 32) % wal_frame_size == 0,
            "one-file WAL bytes did not form a header plus complete SQLite frames",
        )?;
        let appended_frames = (appended_wal_bytes - 32) / wal_frame_size;
        require(
            appended_frames > 0,
            "one-file update appended no WAL frames",
        )?;
        let whole_database_pages = db_before.len().div_ceil(page_size);
        require(
            appended_frames < whole_database_pages,
            "one-file WAL frames did not stay below a whole-database rewrite",
        )?;
        require_eq(
            &structural_counts(&store.connection, StructuralSlot::B)?,
            &inactive_before,
            "one-file update retained inactive-slot bytes",
        )?;
        require(
            store
                .load_file_text(&format!(
                    "src/file-{:03}.rs",
                    unrelated_count.saturating_sub(1)
                ))?
                .is_some(),
            "one-file update rewrote or lost an unrelated lexical row",
        )?;
        Ok(WriteGateMeasurement {
            appended_frames,
            database_pages: whole_database_pages,
        })
    }

    fn prove_incremental_publication_atomicity() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(&root)?;
        store.replace_scan(&[file_node("src/lib.rs", "base-hash")])?;
        store.replace_file_texts_for_paths(
            &["src/lib.rs".to_owned()],
            &[file_text("src/lib.rs", "base content")],
        )?;
        store.set_purpose(
            "src/lib.rs",
            "Preserve authored incremental intent.",
            PurposeSource::Agent,
        )?;
        store.connection.execute(
            "INSERT INTO file_texts(
                 path, content_hash, byte_count, line_count, content,
                 structural_slot, last_changed_epoch
             ) VALUES('rollback.rs', 'rollback', 8, 1, 'rollback', 'b', 0)",
            [],
        )?;
        insert_graph_entity(&store.connection, "affected", "src/lib.rs")?;
        upsert_structural_state_signature(&store.connection, "atomic-base")?;
        let inactive_before = structural_counts(&store.connection, StructuralSlot::B)?;
        let base = store.publication_state()?;
        let mut delta = test_incremental_delta(
            &root,
            base,
            Some("atomic-base".to_owned()),
            "atomic-next",
            "next-hash",
            "next content",
        );
        let mut out_of_closure = test_incremental_delta(
            &root,
            base,
            Some("atomic-base".to_owned()),
            "atomic-invalid-closure",
            "invalid-hash",
            "invalid content",
        );
        out_of_closure
            .file_texts
            .push(file_text("src/unrelated.rs", "out of closure"));
        let closure_error = match store.publish_incremental_structural_delta(&out_of_closure) {
            Err(error) => error,
            Ok(publication) => {
                return Err(format!(
                    "out-of-closure delta published unexpectedly: {publication:?}"
                )
                .into());
            }
        };
        require(
            closure_error.to_string().contains("affected closure"),
            "out-of-closure rejection lost its cause",
        )?;
        require_eq(
            &store.publication_state()?,
            &base,
            "publication state after out-of-closure rejection",
        )?;
        require_eq(
            &store.structural_state_signature()?,
            &Some("atomic-base".to_owned()),
            "signature after out-of-closure rejection",
        )?;
        delta.summary_mutations = vec![IncrementalSummaryMutation::Set {
            path: "src/lib.rs".to_owned(),
            summary: "Updated compatibility summary.".to_owned(),
            purpose_suggestion: Some("Describe the updated compatibility source.".to_owned()),
        }];
        delta.built_in_purposes = vec![IncrementalBuiltInPurpose {
            path: "src/lib.rs".to_owned(),
            purpose: "Replace authored intent incorrectly.".to_owned(),
            source: PurposeSource::Imported,
        }];
        let publication = match store.publish_incremental_structural_delta(&delta)? {
            IncrementalPublication::Published(state) => state,
            IncrementalPublication::Unchanged(state) => {
                return Err(format!("incremental delta coalesced unexpectedly: {state:?}").into());
            }
        };
        require_eq(
            &publication.active_slot,
            &StructuralSlot::A,
            "incremental slot",
        )?;
        require_eq(
            &publication.active_epoch,
            &IndexEpoch::new(1),
            "incremental epoch",
        )?;
        require_eq(
            &require_some(store.load_file_text("src/lib.rs")?, "published text")?.content,
            &"next content".to_owned(),
            "published lexical content",
        )?;
        require_eq(
            &structural_counts(&store.connection, StructuralSlot::B)?,
            &inactive_before,
            "retained inactive slot",
        )?;
        require_eq(
            &store.connection.query_row(
                "SELECT COUNT(*) FROM graph_entities
                 WHERE structural_slot = 'a' AND repository_path = 'src/lib.rs'",
                [],
                |row| row.get::<_, i64>(0),
            )?,
            &0,
            "affected typed graph invalidation",
        )?;
        require_eq(
            &store.structural_state_signature()?,
            &Some("atomic-next".to_owned()),
            "published structural signature",
        )?;
        let published_node =
            require_some(store.load_node_by_path("src/lib.rs")?, "published node")?;
        require_eq(
            &published_node.purpose.purpose,
            &Some("Preserve authored incremental intent.".to_owned()),
            "stale authored purpose text",
        )?;
        require_eq(
            &published_node.purpose.source,
            &PurposeSource::Agent,
            "stale authored purpose source",
        )?;
        require_eq(
            &published_node.purpose.status,
            &PurposeStatus::Stale,
            "stale authored purpose status",
        )?;

        let stable_node = require_some(store.load_node_by_path("src/lib.rs")?, "stable node")?;
        let stable_text = require_some(store.load_file_text("src/lib.rs")?, "stable text")?;
        let stable_state = store.publication_state()?;
        let stable_inactive = structural_counts(&store.connection, StructuralSlot::B)?;
        let mut failing = test_incremental_delta(
            &root,
            stable_state,
            Some("atomic-next".to_owned()),
            "atomic-failure",
            "failure-hash",
            "failure content",
        );
        failing.summary_mutations = vec![IncrementalSummaryMutation::Clear {
            path: "src/missing.rs".to_owned(),
        }];
        failing.affected_paths.push("src/missing.rs".to_owned());
        require(
            store
                .publish_incremental_structural_delta(&failing)
                .is_err(),
            "late incremental failure unexpectedly committed",
        )?;
        require_eq(
            &store.publication_state()?,
            &stable_state,
            "publication state after late rollback",
        )?;
        require_eq(
            &store.structural_state_signature()?,
            &Some("atomic-next".to_owned()),
            "signature after late rollback",
        )?;
        require_eq(
            &require_some(store.load_node_by_path("src/lib.rs")?, "rolled-back node")?.node,
            &stable_node.node,
            "compatibility node after late rollback",
        )?;
        require_eq(
            &require_some(store.load_file_text("src/lib.rs")?, "rolled-back text")?,
            &stable_text,
            "lexical row after late rollback",
        )?;
        require_eq(
            &structural_counts(&store.connection, StructuralSlot::B)?,
            &stable_inactive,
            "inactive slot after late rollback",
        )?;
        Ok(())
    }

    fn test_incremental_delta(
        root: &Path,
        base_publication: PublicationState,
        base_state_signature: Option<String>,
        target_state_signature: &str,
        hash: &str,
        content: &str,
    ) -> IncrementalStructuralDelta {
        IncrementalStructuralDelta {
            project_root: root.to_path_buf(),
            base_publication,
            base_state_signature,
            target_state_signature: target_state_signature.to_owned(),
            affected_paths: vec!["src/lib.rs".to_owned()],
            nodes: vec![file_node("src/lib.rs", hash)],
            absent_paths: Vec::new(),
            file_texts: vec![file_text("src/lib.rs", content)],
            source_mutations: Vec::new(),
            summary_mutations: Vec::new(),
            built_in_purposes: Vec::new(),
        }
    }

    fn prove_structural_publication_reconciliation() -> Result<(), Box<dyn Error>> {
        prove_affected_reconciliation_query_plans()?;
        prove_affected_path_edge_cases()?;
        prove_sqlite_enforces_graph_identity_uniqueness()?;
        prove_reconciliation_fault_matrix()?;
        prove_incremental_reconciliation_rolls_back()?;
        prove_full_reconciliation_rolls_back()
    }

    fn prove_affected_reconciliation_query_plans() -> Result<(), Box<dyn Error>> {
        let store = AtlasStore::in_memory()?;
        let (descendant_start, descendant_end) = crate::sqlite_descendant_bounds("src");
        for (table, path_column, index) in [
            (
                "graph_entities",
                "repository_path",
                "idx_graph_entities_slot_repository_path",
            ),
            (
                "graph_evidence_occurrences",
                "origin_repository_path",
                "idx_graph_evidence_slot_origin_repository_path",
            ),
            (
                "graph_resolution_occurrences",
                "origin_repository_path",
                "idx_graph_resolution_slot_origin_repository_path",
            ),
            (
                "graph_coverage",
                "repository_path",
                "idx_graph_coverage_slot_repository_path",
            ),
        ] {
            let mut statement = store.connection.prepare(&format!(
                "EXPLAIN QUERY PLAN
                 SELECT EXISTS(
                     SELECT 1 FROM {table}
                     WHERE structural_slot = ?1
                       AND ({path_column} = ?2
                            OR ({path_column} >= ?3 AND {path_column} < ?4))
                 )"
            ))?;
            let details = statement
                .query_map(
                    params!["a", "src", descendant_start, descendant_end],
                    |row| row.get::<_, String>(3),
                )?
                .collect::<Result<Vec<_>, _>>()?;
            require(
                details.iter().any(|detail| detail.contains(index)),
                &format!("affected-row reconciliation stopped using {index}: {details:?}"),
            )?;
        }

        let mut statement = store.connection.prepare(
            "EXPLAIN QUERY PLAN
             SELECT path, content FROM file_texts
             WHERE structural_slot = ?1
               AND (path = ?2 OR (path >= ?3 AND path < ?4))",
        )?;
        let details = statement
            .query_map(
                params!["a", "src", descendant_start, descendant_end],
                |row| row.get::<_, String>(3),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        require(
            details.iter().any(|detail| {
                detail.contains("SEARCH file_texts USING INDEX")
                    || detail.contains("SEARCH file_texts USING COVERING INDEX")
            }),
            &format!("affected file-text reconciliation lost its slot/path index: {details:?}"),
        )?;

        let fts_available = store.connection.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_schema
                 WHERE type = 'table' AND name = 'file_text_fts'
             )",
            [],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if fts_available {
            let mut statement = store.connection.prepare(
                "EXPLAIN QUERY PLAN
                 SELECT COUNT(*) FROM file_text_fts
                 WHERE file_text_fts MATCH ?1
                   AND structural_slot = ?2
                   AND (path = ?3 OR (path >= ?4 AND path < ?5))",
            )?;
            let details = statement
                .query_map(
                    params![
                        "path_lookup : \"pasrc\"",
                        "a",
                        "src",
                        descendant_start,
                        descendant_end
                    ],
                    |row| row.get::<_, String>(3),
                )?
                .collect::<Result<Vec<_>, _>>()?;
            require(
                details
                    .iter()
                    .any(|detail| detail.contains("VIRTUAL TABLE INDEX") && detail.contains('M')),
                &format!("affected FTS reconciliation lost its MATCH index: {details:?}"),
            )?;
        }
        Ok(())
    }

    fn prove_affected_path_edge_cases() -> Result<(), Box<dyn Error>> {
        for (affected_path, replacement_path, sentinel_path) in [
            ("a", "a", "b"),
            ("src/quo\"te.rs", "src/quo\"te.rs", "src/quote.rs"),
            (
                "src/space name.rs",
                "src/space name.rs",
                "src/space-name.rs",
            ),
            ("src/über.rs", "src/über.rs", "src/unicode.rs"),
            ("src/Case.rs", "src/Case.rs", "Src/Case.rs"),
            ("src/foo", "src/foo/file.rs", "src/foobar.rs"),
        ] {
            prove_affected_path_case(affected_path, replacement_path, sentinel_path)?;
        }

        let temp = tempfile::tempdir()?;
        let root = temp.path().join("root-affected-path");
        fs::create_dir(&root)?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(&root)?;
        store.replace_scan(&[
            file_node("src/lib.rs", "root-base"),
            file_node("src/old.rs", "root-old"),
        ])?;
        store.replace_file_texts_for_paths(
            &["src/lib.rs".to_owned(), "src/old.rs".to_owned()],
            &[
                file_text("src/lib.rs", "root base content"),
                file_text("src/old.rs", "root old content"),
            ],
        )?;
        upsert_structural_state_signature(&store.connection, "root-base")?;
        let base = store.publication_state()?;
        let delta = IncrementalStructuralDelta {
            project_root: root,
            base_publication: base,
            base_state_signature: store.structural_state_signature()?,
            target_state_signature: "root-next".to_owned(),
            affected_paths: vec![".".to_owned()],
            nodes: vec![file_node("src/lib.rs", "root-next")],
            absent_paths: vec!["src/old.rs".to_owned()],
            file_texts: vec![file_text("src/lib.rs", "root next content")],
            source_mutations: Vec::new(),
            summary_mutations: Vec::new(),
            built_in_purposes: Vec::new(),
        };
        require_eq(
            &store.publish_incremental_structural_delta(&delta)?,
            &IncrementalPublication::Published(base.next_incremental()?),
            "root affected-path publication",
        )?;
        require(
            store.load_file_text("src/old.rs")?.is_none(),
            "root affected-path publication retained obsolete text",
        )?;
        require_eq(
            &require_some(
                store.load_file_text("src/lib.rs")?,
                "root affected-path replacement text",
            )?
            .content,
            &"root next content".to_owned(),
            "root affected-path replacement content",
        )?;
        Ok(())
    }

    fn prove_affected_path_case(
        affected_path: &str,
        replacement_path: &str,
        sentinel_path: &str,
    ) -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("affected-path-case");
        fs::create_dir(&root)?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(&root)?;
        store.replace_scan(&[
            file_node(replacement_path, "edge-base"),
            file_node(sentinel_path, "sentinel-base"),
        ])?;
        store.replace_file_texts_for_paths(
            &[replacement_path.to_owned(), sentinel_path.to_owned()],
            &[
                file_text(replacement_path, "edge base content"),
                file_text(sentinel_path, "sentinel content"),
            ],
        )?;
        upsert_structural_state_signature(&store.connection, "edge-base")?;
        let base = store.publication_state()?;
        let delta = IncrementalStructuralDelta {
            project_root: root,
            base_publication: base,
            base_state_signature: store.structural_state_signature()?,
            target_state_signature: format!("edge-next-{affected_path}"),
            affected_paths: vec![affected_path.to_owned()],
            nodes: vec![file_node(replacement_path, "edge-next")],
            absent_paths: Vec::new(),
            file_texts: vec![file_text(replacement_path, "edge next content")],
            source_mutations: Vec::new(),
            summary_mutations: Vec::new(),
            built_in_purposes: Vec::new(),
        };
        require_eq(
            &store.publish_incremental_structural_delta(&delta)?,
            &IncrementalPublication::Published(base.next_incremental()?),
            &format!("affected-path publication for {affected_path:?}"),
        )?;
        require_eq(
            &require_some(
                store.load_file_text(replacement_path)?,
                "affected-path replacement text",
            )?
            .content,
            &"edge next content".to_owned(),
            &format!("affected-path replacement content for {affected_path:?}"),
        )?;
        require_eq(
            &require_some(
                store.load_file_text(sentinel_path)?,
                "affected-path sentinel text",
            )?
            .content,
            &"sentinel content".to_owned(),
            &format!("affected-path sibling/case sentinel for {affected_path:?}"),
        )?;
        Ok(())
    }

    fn prove_sqlite_enforces_graph_identity_uniqueness() -> Result<(), Box<dyn Error>> {
        let store = AtlasStore::in_memory()?;
        insert_reconciliation_graph_fixture(&store.connection)?;
        let duplicate = store.connection.execute(
            "UPDATE graph_entities
             SET stable_key_canonical = (
                 SELECT stable_key_canonical FROM graph_entities
                 WHERE repository_path = 'src/lib.rs'
             )
             WHERE repository_path = 'src/target.rs'",
            [],
        );
        let error = match duplicate {
            Ok(_) => {
                return Err(io::Error::other(
                    "SQLite accepted a duplicate canonical graph identity",
                )
                .into());
            }
            Err(error) => error.to_string(),
        };
        require(
            error.contains("UNIQUE constraint failed"),
            &format!("duplicate graph identity returned the wrong SQLite diagnostic: {error}"),
        )?;
        Ok(())
    }

    fn prove_reconciliation_fault_matrix() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        for fault in ReconciliationFault::ALL {
            let root = temp.path().join(format!("fault-{fault:?}"));
            fs::create_dir(&root)?;
            let mut store = AtlasStore::in_memory()?;
            store.set_project_root(&root)?;
            store.replace_scan(&[
                file_node("src/lib.rs", "base-hash"),
                file_node("src/target.rs", "target-hash"),
            ])?;
            store.replace_file_texts_for_paths(
                &["src/lib.rs".to_owned()],
                &[file_text("src/lib.rs", "valid utf8")],
            )?;
            insert_reconciliation_graph_fixture(&store.connection)?;
            upsert_structural_state_signature(&store.connection, "reconciled-base")?;

            let base = store.publication_state()?;
            let root_text = normalize_metadata_path(&root)?;
            schema::reconcile_full_structural_publication(&store.connection, &root_text, base)?;
            let base_signature = store.structural_state_signature()?;
            let base_counts = [
                structural_counts(&store.connection, StructuralSlot::A)?,
                structural_counts(&store.connection, StructuralSlot::B)?,
            ];

            if fault.disables_foreign_keys() {
                store
                    .connection
                    .pragma_update(None, "foreign_keys", "OFF")?;
            }
            if fault.ignores_check_constraints() {
                store
                    .connection
                    .pragma_update(None, "ignore_check_constraints", "ON")?;
            }
            let transaction = store
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            inject_reconciliation_fault(&transaction, fault)?;
            let Err(error) =
                schema::reconcile_full_structural_publication(&transaction, &root_text, base)
            else {
                return Err(
                    format!("reconciliation fault {fault:?} was accepted unexpectedly").into(),
                );
            };
            require(
                error.to_string().contains(fault.expected_diagnostic()),
                &format!("reconciliation fault {fault:?} lost its diagnostic: {error}"),
            )?;
            transaction.rollback()?;
            store
                .connection
                .pragma_update(None, "ignore_check_constraints", "OFF")?;
            store.connection.pragma_update(None, "foreign_keys", "ON")?;

            schema::reconcile_full_structural_publication(&store.connection, &root_text, base)?;
            require_eq(
                &store.publication_state()?,
                &base,
                &format!("publication after {fault:?} rollback"),
            )?;
            require_eq(
                &store.structural_state_signature()?,
                &base_signature,
                &format!("signature after {fault:?} rollback"),
            )?;
            require_eq(
                &[
                    structural_counts(&store.connection, StructuralSlot::A)?,
                    structural_counts(&store.connection, StructuralSlot::B)?,
                ],
                &base_counts,
                &format!("slot rows after {fault:?} rollback"),
            )?;
        }
        Ok(())
    }

    fn inject_reconciliation_fault(
        transaction: &Transaction<'_>,
        fault: ReconciliationFault,
    ) -> DbResult<()> {
        match fault {
            ReconciliationFault::CandidateUniqueness => {
                let occurrence = blake3::hash(b"reconciliation-resolution");
                let target = blake3::hash(b"reconciliation-target");
                transaction.execute(
                    "INSERT INTO graph_resolution_candidates(
                         resolution_occurrence_digest, candidate_ordinal, target_scope,
                         target_entity_digest, confidence, structural_slot,
                         last_changed_epoch
                     )
                     SELECT ?1, 2, 'internal', ?2, 'exact', active_slot, active_epoch
                     FROM graph_publication_state WHERE singleton = 1",
                    params![
                        occurrence.as_bytes().as_slice(),
                        target.as_bytes().as_slice()
                    ],
                )?;
                transaction.execute(
                    "UPDATE graph_resolution_occurrences SET candidate_total = 3",
                    [],
                )?;
            }
            ReconciliationFault::MissingEndpoint => {
                transaction.execute(
                    "UPDATE graph_relations SET source_entity_digest = zeroblob(32)",
                    [],
                )?;
            }
            ReconciliationFault::UnknownRelationFamily => {
                transaction.execute(
                    "UPDATE graph_relations SET relation_kind = 'untyped-relation'",
                    [],
                )?;
            }
            ReconciliationFault::CandidateCount => {
                transaction.execute(
                    "UPDATE graph_resolution_occurrences SET candidate_total = 3",
                    [],
                )?;
            }
            ReconciliationFault::CandidateTotalBoundary => {
                transaction.execute(
                    "UPDATE graph_resolution_occurrences
                     SET candidate_total = 4294967296,
                         candidate_completeness = 'partial'",
                    [],
                )?;
            }
            ReconciliationFault::CandidateRetentionBudget => {
                transaction.execute_batch(
                    "WITH RECURSIVE candidate(candidate_ordinal) AS (
                         SELECT 2
                         UNION ALL
                         SELECT candidate_ordinal + 1 FROM candidate
                         WHERE candidate_ordinal < 64
                     )
                     INSERT INTO graph_resolution_candidates(
                         resolution_occurrence_digest, candidate_ordinal,
                         target_scope, external_target_namespace,
                         external_target_value, confidence, structural_slot,
                         last_changed_epoch
                     )
                     SELECT occurrence.stable_key_digest, candidate.candidate_ordinal,
                            'external', 'publication-test',
                            printf('candidate-%d', candidate.candidate_ordinal),
                            'exact', occurrence.structural_slot,
                            occurrence.last_changed_epoch
                     FROM graph_resolution_occurrences AS occurrence
                     CROSS JOIN candidate;
                     UPDATE graph_resolution_occurrences
                     SET candidate_total = 65,
                         candidate_completeness = 'complete';",
                )?;
            }
            ReconciliationFault::CandidateOrdinalGap => {
                transaction.execute(
                    "UPDATE graph_resolution_candidates
                     SET candidate_ordinal = 2 WHERE candidate_ordinal = 1",
                    [],
                )?;
            }
            ReconciliationFault::CoverageContract => {
                transaction.execute(
                    "UPDATE graph_coverage SET omitted_count = 1
                     WHERE scope_kind = 'relation'",
                    [],
                )?;
            }
            ReconciliationFault::CoverageCount => {
                transaction.execute(
                    "UPDATE graph_coverage SET produced_count = 2
                     WHERE scope_kind = 'relation'",
                    [],
                )?;
            }
            ReconciliationFault::InvalidUtf8 => {
                transaction.execute("UPDATE file_texts SET content = CAST(x'80' AS TEXT)", [])?;
            }
            ReconciliationFault::WrongRoot => {
                transaction.execute(
                    "UPDATE metadata SET value = 'C:/wrong-root' WHERE key = 'project_root'",
                    [],
                )?;
            }
            ReconciliationFault::InvalidRowSlot => {
                transaction.execute("UPDATE file_texts SET structural_slot = 'c'", [])?;
            }
            ReconciliationFault::FutureRowEpoch => {
                transaction.execute("UPDATE file_texts SET last_changed_epoch = 1", [])?;
            }
            ReconciliationFault::WrongPublication => {
                transaction.execute(
                    "UPDATE graph_publication_state SET active_epoch = 1 WHERE singleton = 1",
                    [],
                )?;
            }
            ReconciliationFault::MissingSchemaGuard => {
                transaction.execute("DROP TRIGGER graph_publication_state_delete_guard", [])?;
            }
            ReconciliationFault::NoOpSchemaGuard => {
                transaction.execute_batch(
                    "DROP TRIGGER graph_publication_state_delete_guard;
                     CREATE TRIGGER graph_publication_state_delete_guard
                     BEFORE DELETE ON graph_publication_state
                     BEGIN
                         SELECT 1;
                     END;",
                )?;
            }
            ReconciliationFault::MissingUniqueConstraint => {
                rebuild_graph_entities_without_unique_constraint(transaction)?;
            }
            ReconciliationFault::AlteredPathCollation => {
                rebuild_graph_coverage_with_nocase_path(transaction)?;
            }
            ReconciliationFault::PartialPathIndex => {
                transaction.execute_batch(
                    "DROP INDEX idx_graph_coverage_slot_repository_path;
                     CREATE INDEX idx_graph_coverage_slot_repository_path
                     ON graph_coverage(structural_slot, repository_path)
                     WHERE repository_path IS NOT NULL;",
                )?;
            }
            ReconciliationFault::AlteredForeignKeyAction => {
                rebuild_resolution_candidates_without_cascade(transaction)?;
            }
        }
        Ok(())
    }

    fn rebuild_graph_entities_without_unique_constraint(
        transaction: &Transaction<'_>,
    ) -> DbResult<()> {
        let canonical_sql = transaction.query_row(
            "SELECT sql FROM sqlite_schema
             WHERE type = 'table' AND name = 'graph_entities'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        let rebuilt_sql = canonical_sql
            .replacen(
                "graph_entities",
                "graph_entities_without_unique_constraint",
                1,
            )
            .replacen("UNIQUE(structural_slot, stable_key_canonical),", "", 1);
        if rebuilt_sql == canonical_sql
            || rebuilt_sql.contains("UNIQUE(structural_slot, stable_key_canonical)")
        {
            return Err(publication_error(
                "failed to construct the missing-UNIQUE fault table",
            ));
        }
        transaction.execute_batch(&rebuilt_sql)?;
        transaction.execute(
            "INSERT INTO graph_entities_without_unique_constraint
             SELECT * FROM graph_entities",
            [],
        )?;
        transaction.execute("DROP TABLE graph_entities", [])?;
        transaction.execute(
            "ALTER TABLE graph_entities_without_unique_constraint
             RENAME TO graph_entities",
            [],
        )?;
        Ok(())
    }

    fn rebuild_graph_coverage_with_nocase_path(transaction: &Transaction<'_>) -> DbResult<()> {
        let canonical_sql = transaction.query_row(
            "SELECT sql FROM sqlite_schema
             WHERE type = 'table' AND name = 'graph_coverage'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        let rebuilt_sql = canonical_sql
            .replacen("graph_coverage", "graph_coverage_with_nocase_path", 1)
            .replacen(
                "repository_path TEXT NOT NULL DEFAULT '',",
                "repository_path TEXT NOT NULL COLLATE NOCASE DEFAULT '',",
                1,
            );
        if rebuilt_sql == canonical_sql || !rebuilt_sql.contains("COLLATE NOCASE") {
            return Err(publication_error(
                "failed to construct the altered-collation fault table",
            ));
        }
        transaction.execute_batch(&rebuilt_sql)?;
        transaction.execute(
            "INSERT INTO graph_coverage_with_nocase_path SELECT * FROM graph_coverage",
            [],
        )?;
        transaction.execute("DROP TABLE graph_coverage", [])?;
        transaction.execute(
            "ALTER TABLE graph_coverage_with_nocase_path RENAME TO graph_coverage",
            [],
        )?;
        transaction.execute(
            "CREATE INDEX idx_graph_coverage_slot_repository_path
             ON graph_coverage(structural_slot, repository_path)",
            [],
        )?;
        Ok(())
    }

    fn rebuild_resolution_candidates_without_cascade(
        transaction: &Transaction<'_>,
    ) -> DbResult<()> {
        let canonical_sql = transaction.query_row(
            "SELECT sql FROM sqlite_schema
             WHERE type = 'table' AND name = 'graph_resolution_candidates'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        let rebuilt_sql = canonical_sql
            .replacen(
                "graph_resolution_candidates",
                "graph_resolution_candidates_without_cascade",
                1,
            )
            .replacen("ON DELETE CASCADE", "ON DELETE RESTRICT", 1);
        if rebuilt_sql == canonical_sql || !rebuilt_sql.contains("ON DELETE RESTRICT") {
            return Err(publication_error(
                "failed to construct the altered-foreign-key fault table",
            ));
        }
        transaction.execute_batch(&rebuilt_sql)?;
        transaction.execute(
            "INSERT INTO graph_resolution_candidates_without_cascade
             SELECT * FROM graph_resolution_candidates",
            [],
        )?;
        transaction.execute("DROP TABLE graph_resolution_candidates", [])?;
        transaction.execute(
            "ALTER TABLE graph_resolution_candidates_without_cascade
             RENAME TO graph_resolution_candidates",
            [],
        )?;
        Ok(())
    }

    fn prove_incremental_reconciliation_rolls_back() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("incremental-reconciliation");
        fs::create_dir(&root)?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(&root)?;
        store.replace_scan(&[file_node("src/lib.rs", "base-hash")])?;
        store.replace_file_texts_for_paths(
            &["src/lib.rs".to_owned()],
            &[file_text("src/lib.rs", "base content")],
        )?;
        upsert_structural_state_signature(&store.connection, "incremental-base")?;
        let base = store.publication_state()?;
        let base_signature = store.structural_state_signature()?;
        let base_counts = structural_counts(&store.connection, base.active_slot)?;
        store.connection.execute_batch(
            "CREATE TRIGGER inject_incremental_root_drift
             AFTER INSERT ON file_texts
             BEGIN
                 UPDATE metadata SET value = 'C:/injected-root-drift'
                 WHERE key = 'project_root';
             END;",
        )?;

        let delta = test_incremental_delta(
            &root,
            base,
            base_signature.clone(),
            "incremental-next",
            "next-hash",
            "next content",
        );
        let error = match store.publish_incremental_structural_delta(&delta) {
            Err(error) => error,
            Ok(publication) => {
                return Err(format!(
                    "incremental root-drift fault published unexpectedly: {publication:?}"
                )
                .into());
            }
        };
        require(
            error.to_string().contains("project root"),
            &format!("incremental reconciliation lost its root diagnostic: {error}"),
        )?;
        require_eq(
            &store.publication_state()?,
            &base,
            "incremental reconciliation publication rollback",
        )?;
        require_eq(
            &store.structural_state_signature()?,
            &base_signature,
            "incremental reconciliation signature rollback",
        )?;
        require_eq(
            &structural_counts(&store.connection, base.active_slot)?,
            &base_counts,
            "incremental reconciliation active-slot rollback",
        )?;
        let text = require_some(
            store.load_file_text("src/lib.rs")?,
            "incremental reconciliation rollback text",
        )?;
        require_eq(
            &text.content,
            &"base content".to_owned(),
            "incremental reconciliation rollback content",
        )?;
        require_eq(
            &store.project_root()?,
            &Some(normalize_metadata_path(&root)?),
            "incremental reconciliation root rollback",
        )?;
        Ok(())
    }

    fn prove_full_reconciliation_rolls_back() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("full-reconciliation");
        let live_path = temp.path().join("full-live.db");
        let stage_path = temp.path().join("full-stage.db");
        fs::create_dir(&root)?;
        let mut live = AtlasStore::open(&live_path)?;
        live.set_project_root(&root)?;
        live.replace_scan(&[file_node("src/lib.rs", "base-hash")])?;
        live.replace_file_texts_for_paths(
            &["src/lib.rs".to_owned()],
            &[file_text("src/lib.rs", "base content")],
        )?;
        upsert_structural_state_signature(&live.connection, "full-base")?;
        let base = live.publication_state()?;
        let base_signature = live.structural_state_signature()?;
        let inactive_counts = structural_counts(&live.connection, base.active_slot.other())?;

        let staging = live.create_structural_staging(&live_path, &stage_path, &root)?;
        let mut stage = AtlasStore::open(staging.path())?;
        stage.prepare_structural_full_scan()?;
        stage.replace_scan(&[
            file_node("src/lib.rs", "next-hash"),
            file_node("src/target.rs", "target-hash"),
        ])?;
        stage.replace_file_texts_for_paths(
            &["src/lib.rs".to_owned()],
            &[file_text("src/lib.rs", "next content")],
        )?;
        insert_reconciliation_graph_fixture(&stage.connection)?;
        stage.connection.execute(
            "UPDATE graph_coverage SET produced_count = 2
             WHERE scope_kind = 'relation'",
            [],
        )?;
        stage.set_staged_structural_state_signature(&staging, "full-next")?;
        stage.seal_structural_staging(&staging)?;
        drop(stage);

        let error = match live.publish_structural_staging(&staging) {
            Err(error) => error,
            Ok(publication) => {
                return Err(format!(
                    "full coverage-count fault published unexpectedly: {publication:?}"
                )
                .into());
            }
        };
        require(
            error.to_string().contains("coverage counts"),
            &format!("full reconciliation lost its count diagnostic: {error}"),
        )?;
        require_eq(
            &live.publication_state()?,
            &base,
            "full reconciliation publication rollback",
        )?;
        require_eq(
            &live.structural_state_signature()?,
            &base_signature,
            "full reconciliation signature rollback",
        )?;
        require_eq(
            &structural_counts(&live.connection, base.active_slot.other())?,
            &inactive_counts,
            "full reconciliation inactive-slot rollback",
        )?;
        let text = require_some(
            live.load_file_text("src/lib.rs")?,
            "full reconciliation rollback text",
        )?;
        require_eq(
            &text.content,
            &"base content".to_owned(),
            "full reconciliation rollback content",
        )?;
        Ok(())
    }

    fn insert_reconciliation_graph_fixture(connection: &Connection) -> DbResult<()> {
        insert_graph_entity(connection, "reconciliation-source", "src/lib.rs")?;
        insert_graph_entity(connection, "reconciliation-target", "src/target.rs")?;
        let source = blake3::hash(b"reconciliation-source");
        let target = blake3::hash(b"reconciliation-target");
        let relation = blake3::hash(b"reconciliation-relation");
        let occurrence = blake3::hash(b"reconciliation-resolution");
        let fingerprint = blake3::hash(b"reconciliation-fingerprint");
        connection.execute(
            "INSERT INTO graph_relations(
                 stable_key_digest, stable_key_version, stable_key_canonical,
                 source_entity_digest, relation_kind, resolution_status,
                 target_scope, target_entity_digest, confidence, parser_kind,
                 parser_identity, parser_version, structural_slot,
                 last_changed_epoch
             )
             SELECT ?1, 1, ?2, ?3, 'calls', 'resolved', 'internal', ?4,
                    'exact', 'structural', 'publication-test', '1',
                    active_slot, active_epoch
             FROM graph_publication_state WHERE singleton = 1",
            params![
                relation.as_bytes().as_slice(),
                b"reconciliation-relation".as_slice(),
                source.as_bytes().as_slice(),
                target.as_bytes().as_slice()
            ],
        )?;
        connection.execute(
            "INSERT INTO graph_resolution_occurrences(
                 stable_key_digest, stable_key_version, stable_key_canonical,
                 source_entity_digest, relation_kind, origin_kind,
                 origin_entity_digest, resolver_name, resolver_version,
                 content_span_fingerprint, occurrence_discriminator,
                 resolution_status, candidate_total, candidate_completeness,
                 evidence_class, confidence, completeness, parser_kind,
                 parser_identity, parser_version, structural_slot,
                 last_changed_epoch
             )
             SELECT ?1, 1, ?2, ?3, 'calls', 'entity', ?3,
                    'publication-resolver', '1', ?4, 0, 'ambiguous', 2,
                    'complete', 'direct', 'exact', 'complete', 'structural',
                    'publication-test', '1', active_slot, active_epoch
             FROM graph_publication_state WHERE singleton = 1",
            params![
                occurrence.as_bytes().as_slice(),
                b"reconciliation-resolution".as_slice(),
                source.as_bytes().as_slice(),
                fingerprint.as_bytes().as_slice()
            ],
        )?;
        for (ordinal, candidate) in [(0_i64, source), (1_i64, target)] {
            connection.execute(
                "INSERT INTO graph_resolution_candidates(
                     resolution_occurrence_digest, candidate_ordinal,
                     target_scope, target_entity_digest, confidence,
                     structural_slot, last_changed_epoch
                 )
                 SELECT ?1, ?2, 'internal', ?3, 'exact', active_slot, active_epoch
                 FROM graph_publication_state WHERE singleton = 1",
                params![
                    occurrence.as_bytes().as_slice(),
                    ordinal,
                    candidate.as_bytes().as_slice()
                ],
            )?;
        }
        connection.execute(
            "INSERT INTO graph_coverage(
                 scope_kind, repository_path, relation_kind, coverage_state,
                 produced_count, omitted_count, structural_slot,
                 last_changed_epoch
             )
             SELECT 'relation', 'src/lib.rs', 'calls', 'complete', 1, 0,
                    active_slot, active_epoch
             FROM graph_publication_state WHERE singleton = 1",
            [],
        )?;
        Ok(())
    }

    fn prove_prepared_mutation_batch_fails_closed() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(&root)?;
        store.replace_scan(&[file_node("src/existing.rs", "base-hash")])?;
        store.replace_file_texts_for_paths(
            &["src/existing.rs".to_owned()],
            &[file_text("src/existing.rs", "base content")],
        )?;
        insert_graph_entity(&store.connection, "existing", "src/existing.rs")?;
        upsert_structural_state_signature(&store.connection, "batch-base")?;
        let base = store.publication_state()?;
        let base_signature = store.structural_state_signature()?;

        store.connection.execute_batch(
            "CREATE TRIGGER reject_second_batch_insert
             BEFORE INSERT ON nodes WHEN new.path = 'src/b.rs'
             BEGIN
                 SELECT RAISE(ABORT, 'injected prepared insert failure');
             END;",
        )?;
        let insert_delta = IncrementalStructuralDelta {
            project_root: root.clone(),
            base_publication: base,
            base_state_signature: base_signature.clone(),
            target_state_signature: "batch-insert".to_owned(),
            affected_paths: vec!["src".to_owned()],
            nodes: vec![
                file_node("src/a.rs", "a-hash"),
                file_node("src/b.rs", "b-hash"),
            ],
            absent_paths: Vec::new(),
            file_texts: Vec::new(),
            source_mutations: Vec::new(),
            summary_mutations: Vec::new(),
            built_in_purposes: Vec::new(),
        };
        let insert_error = require_incremental_error(
            store.publish_incremental_structural_delta(&insert_delta),
            "prepared insert failure must abort the batch",
        )?;
        require(
            insert_error
                .to_string()
                .contains("injected prepared insert failure"),
            "prepared insert failure lost its terminal status",
        )?;
        store
            .connection
            .execute("DROP TRIGGER reject_second_batch_insert", [])?;
        require_unpublished_batch(&store, base, "base-hash")?;
        require(
            store.load_node_by_path("src/a.rs")?.is_none(),
            "the successful prefix of a failed insert batch leaked",
        )?;

        store.connection.execute_batch(
            "CREATE TRIGGER reject_batch_update
             BEFORE UPDATE ON nodes WHEN new.path = 'src/existing.rs'
             BEGIN
                 SELECT RAISE(ABORT, 'injected prepared update failure');
             END;",
        )?;
        let update_delta = IncrementalStructuralDelta {
            project_root: root.clone(),
            base_publication: base,
            base_state_signature: base_signature.clone(),
            target_state_signature: "batch-update".to_owned(),
            affected_paths: vec!["src/existing.rs".to_owned()],
            nodes: vec![file_node("src/existing.rs", "updated-hash")],
            absent_paths: Vec::new(),
            file_texts: Vec::new(),
            source_mutations: Vec::new(),
            summary_mutations: Vec::new(),
            built_in_purposes: Vec::new(),
        };
        let update_error = require_incremental_error(
            store.publish_incremental_structural_delta(&update_delta),
            "prepared update failure must abort the batch",
        )?;
        require(
            update_error
                .to_string()
                .contains("injected prepared update failure"),
            "prepared update failure lost its terminal status",
        )?;
        store
            .connection
            .execute("DROP TRIGGER reject_batch_update", [])?;
        require_unpublished_batch(&store, base, "base-hash")?;

        store.connection.execute_batch(
            "CREATE TRIGGER reject_batch_delete
             BEFORE DELETE ON graph_entities
             WHEN old.repository_path = 'src/existing.rs'
             BEGIN
                 SELECT RAISE(ABORT, 'injected prepared delete failure');
             END;",
        )?;
        let delete_delta = IncrementalStructuralDelta {
            project_root: root,
            base_publication: base,
            base_state_signature: base_signature,
            target_state_signature: "batch-delete".to_owned(),
            affected_paths: vec!["src/existing.rs".to_owned()],
            nodes: Vec::new(),
            absent_paths: vec!["src/existing.rs".to_owned()],
            file_texts: Vec::new(),
            source_mutations: Vec::new(),
            summary_mutations: Vec::new(),
            built_in_purposes: Vec::new(),
        };
        let delete_error = require_incremental_error(
            store.publish_incremental_structural_delta(&delete_delta),
            "prepared delete failure must abort the batch",
        )?;
        require(
            delete_error
                .to_string()
                .contains("injected prepared delete failure"),
            "prepared delete failure lost its terminal status",
        )?;
        require_unpublished_batch(&store, base, "base-hash")?;
        Ok(())
    }

    fn require_unpublished_batch(
        store: &AtlasStore,
        base: PublicationState,
        expected_hash: &str,
    ) -> Result<(), Box<dyn Error>> {
        require_eq(
            &store.publication_state()?,
            &base,
            "publication state after failed prepared batch",
        )?;
        require_eq(
            &store.structural_state_signature()?,
            &Some("batch-base".to_owned()),
            "signature after failed prepared batch",
        )?;
        require_eq(
            &require_some(
                store.load_node_by_path("src/existing.rs")?,
                "existing node after failed prepared batch",
            )?
            .node
            .content_hash,
            &Some(expected_hash.to_owned()),
            "existing node hash after failed prepared batch",
        )?;
        require_eq(
            &store.connection.query_row(
                "SELECT COUNT(*) FROM graph_entities
                 WHERE structural_slot = 'a' AND repository_path = 'src/existing.rs'",
                [],
                |row| row.get::<_, i64>(0),
            )?,
            &1,
            "graph row after failed prepared batch",
        )?;
        Ok(())
    }

    fn read_optional_file(path: &Path) -> io::Result<Vec<u8>> {
        match fs::read(path) {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }

    fn prove_transactional_reader_and_rollback_retention() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let live_path = temp.path().join("reader-live.db");
        let staging_path = temp.path().join("reader-stage.db");
        let root = temp.path().join("repo");
        fs::create_dir(&root)?;

        let mut writer = AtlasStore::open(&live_path)?;
        writer.set_project_root(&root)?;
        writer.replace_scan(&[
            file_node("src/first.rs", "old-first-hash"),
            file_node("src/second.rs", "old-second-hash"),
        ])?;
        writer.replace_file_texts_for_paths(
            &["src/first.rs".to_owned(), "src/second.rs".to_owned()],
            &[
                file_text("src/first.rs", "old first generation"),
                file_text("src/second.rs", "old second generation"),
            ],
        )?;
        insert_graph_entity(&writer.connection, "old-first", "src/first.rs")?;
        let rollback_counts = structural_counts(&writer.connection, StructuralSlot::A)?;

        let staging = writer.create_structural_staging(&live_path, &staging_path, &root)?;
        let mut stage = AtlasStore::open(staging.path())?;
        stage.prepare_structural_full_scan()?;
        stage.replace_scan(&[
            file_node("src/first.rs", "new-first-hash"),
            file_node("src/second.rs", "new-second-hash"),
        ])?;
        stage.replace_file_texts_for_paths(
            &["src/first.rs".to_owned(), "src/second.rs".to_owned()],
            &[
                file_text("src/first.rs", "new first generation"),
                file_text("src/second.rs", "new second generation"),
            ],
        )?;
        insert_graph_entity(&stage.connection, "new-first", "src/first.rs")?;
        stage.set_staged_structural_state_signature(&staging, "reader-next")?;
        stage.seal_structural_staging(&staging)?;
        drop(stage);

        writer.connection.execute_batch(
            "CREATE TRIGGER reject_active_structural_delete
             BEFORE DELETE ON file_texts
             WHEN old.structural_slot = (
                 SELECT active_slot FROM graph_publication_state WHERE singleton = 1
             )
             BEGIN
                 SELECT RAISE(ABORT, 'active structural slot delete attempted');
             END;",
        )?;

        let reader = AtlasStore::open(&live_path)?;
        let publication_barrier = Arc::new(Barrier::new(2));
        let writer_barrier = Arc::clone(&publication_barrier);
        let publication_thread = thread::spawn(move || -> Result<PublicationState, String> {
            writer_barrier.wait();
            let result = writer
                .publish_structural_staging(&staging)
                .map_err(|error| error.to_string());
            writer_barrier.wait();
            result
        });

        let mut observed = Vec::new();
        let mut publication_started = false;
        reader.visit_file_texts_for_search(None, false, |text| {
            if !publication_started {
                publication_started = true;
                publication_barrier.wait();
                publication_barrier.wait();
            }
            observed.push((text.path, text.content));
            Ok(true)
        })?;
        let publication = publication_thread
            .join()
            .map_err(|_panic| io::Error::other("overlapping full publication panicked"))?
            .map_err(io::Error::other)?;

        require_eq(
            &publication,
            &PublicationState {
                active_slot: StructuralSlot::B,
                active_epoch: IndexEpoch::new(1),
            },
            "overlapping full publication",
        )?;
        require_eq(
            &observed,
            &vec![
                ("src/first.rs".to_owned(), "old first generation".to_owned()),
                (
                    "src/second.rs".to_owned(),
                    "old second generation".to_owned(),
                ),
            ],
            "transactional reader generation",
        )?;

        let current = reader.load_file_texts_for_search(None, false)?;
        let current = current
            .into_iter()
            .map(|text| (text.path, text.content))
            .collect::<Vec<_>>();
        require_eq(
            &current,
            &vec![
                ("src/first.rs".to_owned(), "new first generation".to_owned()),
                (
                    "src/second.rs".to_owned(),
                    "new second generation".to_owned(),
                ),
            ],
            "post-publication reader generation",
        )?;
        require_eq(
            &structural_counts(&reader.connection, StructuralSlot::A)?,
            &rollback_counts,
            "retained rollback slot",
        )?;
        require(
            structural_counts(&reader.connection, StructuralSlot::B)?
                .into_iter()
                .any(|count| count > 0),
            "active slot is empty after publication",
        )?;
        Ok(())
    }

    fn prove_bounded_retained_slot_cleanup() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        let live_path = temp.path().join("cleanup-live.db");
        let first_stage_path = temp.path().join("cleanup-first-stage.db");
        let second_stage_path = temp.path().join("cleanup-second-stage.db");
        fs::create_dir(&root)?;

        let mut live = AtlasStore::open(&live_path)?;
        live.set_project_root(&root)?;
        live.replace_scan(&[file_node("src/retained.rs", "retained-hash")])?;
        live.replace_file_texts_for_paths(
            &["src/retained.rs".to_owned()],
            &[file_text("src/retained.rs", "retained generation")],
        )?;
        insert_graph_entity(&live.connection, "retained-entity", "src/retained.rs")?;
        live.connection.execute(
            "INSERT INTO metadata(key, value) VALUES('cleanup_authored_note', 'preserved')",
            [],
        )?;

        let first_staging = live.create_structural_staging(&live_path, &first_stage_path, &root)?;
        let mut first_stage = AtlasStore::open(first_staging.path())?;
        first_stage.prepare_structural_full_scan()?;
        first_stage.replace_scan(&[file_node("src/current.rs", "current-hash")])?;
        first_stage.replace_file_texts_for_paths(
            &["src/current.rs".to_owned()],
            &[file_text("src/current.rs", "current generation")],
        )?;
        insert_graph_entity(&first_stage.connection, "current-entity", "src/current.rs")?;
        first_stage.set_staged_structural_state_signature(&first_staging, "cleanup-current")?;
        first_stage.seal_structural_staging(&first_staging)?;
        drop(first_stage);
        let current_publication = live.publish_structural_staging(&first_staging)?;
        require_eq(
            &current_publication,
            &PublicationState {
                active_slot: StructuralSlot::B,
                active_epoch: IndexEpoch::new(1),
            },
            "first cleanup publication",
        )?;
        let retained_before = structural_counts(&live.connection, StructuralSlot::A)?;
        let active_before = structural_counts(&live.connection, StructuralSlot::B)?;

        let second_staging =
            live.create_structural_staging(&live_path, &second_stage_path, &root)?;
        let mut second_stage = AtlasStore::open(second_staging.path())?;
        second_stage.prepare_structural_full_scan()?;
        second_stage.replace_scan(&[file_node("src/next.rs", "next-hash")])?;
        second_stage.replace_file_texts_for_paths(
            &["src/next.rs".to_owned()],
            &[file_text("src/next.rs", "next generation")],
        )?;
        second_stage.set_staged_structural_state_signature(&second_staging, "cleanup-next")?;
        second_stage.seal_structural_staging(&second_staging)?;
        drop(second_stage);

        live.connection.execute_batch(
            "CREATE TRIGGER reject_active_slot_cleanup
             BEFORE DELETE ON file_texts
             WHEN old.structural_slot = (
                 SELECT active_slot FROM graph_publication_state WHERE singleton = 1
             )
             BEGIN
                 SELECT RAISE(ABORT, 'active slot cleanup attempted');
             END;

             CREATE TRIGGER inject_retained_slot_cleanup_failure
             BEFORE DELETE ON file_texts
             WHEN old.structural_slot != (
                 SELECT active_slot FROM graph_publication_state WHERE singleton = 1
             )
             BEGIN
                 SELECT RAISE(ABORT, 'injected retained-slot cleanup failure');
             END;",
        )?;
        let error = match live.publish_structural_staging(&second_staging) {
            Err(error) => error,
            Ok(publication) => {
                return Err(format!(
                    "retained-slot cleanup failure published unexpectedly: {publication:?}"
                )
                .into());
            }
        };
        require(
            error
                .to_string()
                .contains("recoverable inactive-slot cleanup failure")
                && error
                    .to_string()
                    .contains("injected retained-slot cleanup failure"),
            &format!("retained-slot cleanup lost its recoverable diagnostic: {error}"),
        )?;
        require_eq(
            &live.publication_state()?,
            &current_publication,
            "publication after retained-slot cleanup rollback",
        )?;
        require_eq(
            &structural_counts(&live.connection, StructuralSlot::A)?,
            &retained_before,
            "restored retained slot after cleanup rollback",
        )?;
        require_eq(
            &structural_counts(&live.connection, StructuralSlot::B)?,
            &active_before,
            "active slot after cleanup rollback",
        )?;
        require_eq(
            &live.connection.query_row(
                "SELECT content FROM file_texts
                 WHERE structural_slot = 'a' AND path = 'src/retained.rs'",
                [],
                |row| row.get::<_, String>(0),
            )?,
            &"retained generation".to_owned(),
            "retained lexical row after cleanup rollback",
        )?;
        require_eq(
            &live.connection.query_row(
                "SELECT value FROM metadata WHERE key = 'cleanup_authored_note'",
                [],
                |row| row.get::<_, String>(0),
            )?,
            &"preserved".to_owned(),
            "authored metadata after cleanup rollback",
        )?;

        live.connection
            .execute("DROP TRIGGER inject_retained_slot_cleanup_failure", [])?;
        let next_publication = live.publish_structural_staging(&second_staging)?;
        require_eq(
            &next_publication,
            &PublicationState {
                active_slot: StructuralSlot::A,
                active_epoch: IndexEpoch::new(2),
            },
            "validated retained-slot reuse",
        )?;
        let next = require_some(live.load_file_text("src/next.rs")?, "next active text")?;
        require_eq(
            &next.content,
            &"next generation".to_owned(),
            "next active generation",
        )?;
        require_eq(
            &live.connection.query_row(
                "SELECT content FROM file_texts
                 WHERE structural_slot = 'b' AND path = 'src/current.rs'",
                [],
                |row| row.get::<_, String>(0),
            )?,
            &"current generation".to_owned(),
            "new retained rollback slot",
        )?;
        require(
            live.connection.query_row(
                "SELECT EXISTS(
                         SELECT 1 FROM file_texts
                         WHERE structural_slot = 'a' AND path = 'src/retained.rs'
                     )",
                [],
                |row| row.get::<_, i64>(0),
            )? == 0,
            "obsolete retained slot was not replaced by the validated publication",
        )?;
        Ok(())
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
        stage.set_staged_structural_state_signature(&staging, "full-next")?;
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
            &live.structural_state_signature()?,
            &Some("full-next".to_owned()),
            "full publication structural signature",
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
            "missing-signature",
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
            if case == "missing-signature" {
                upsert_structural_state_signature(&live.connection, "live-base")?;
            }
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
                "missing-signature" => {}
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
            if case == "missing-signature" && !error.to_string().contains("signature") {
                return Err(format!("missing-signature rejection lost its cause: {error}").into());
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
        stage.set_staged_structural_state_signature(&staging, "candidate-next")?;
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
        let canonical_insert_trigger_sql = live.connection.query_row(
            "SELECT sql FROM sqlite_schema
             WHERE type = 'trigger' AND name = 'file_text_fts_insert'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        live.connection.execute_batch(
            "CREATE TRIGGER inject_inactive_slot_import_failure
             AFTER INSERT ON file_texts
             BEGIN
                 SELECT CASE
                     WHEN new.structural_slot != (
                         SELECT active_slot FROM graph_publication_state WHERE singleton = 1
                     )
                     THEN RAISE(ABORT, 'injected inactive-slot import failure')
                 END;
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
        let canonical_insert_trigger_sql_after = live.connection.query_row(
            "SELECT sql FROM sqlite_schema
             WHERE type = 'trigger' AND name = 'file_text_fts_insert'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        require_eq(
            &canonical_insert_trigger_sql_after,
            &canonical_insert_trigger_sql,
            "canonical FTS insert trigger after import rollback",
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

    fn require_incremental_error(
        result: DbResult<IncrementalPublication>,
        context: &str,
    ) -> Result<DbError, Box<dyn Error>> {
        match result {
            Err(error) => Ok(error),
            Ok(publication) => Err(io::Error::other(format!(
                "{context}: published unexpectedly as {publication:?}"
            ))
            .into()),
        }
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
        schema::STRUCTURAL_DERIVED_TABLES
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
