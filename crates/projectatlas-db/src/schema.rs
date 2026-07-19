//! Own the durable `SQLite` schema, compatibility preflight, and migrations.

use crate::{DbError, DbResult};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::path::Path;
use std::sync::OnceLock;

#[cfg(test)]
use projectatlas_core::normalize_native_path_display;
#[cfg(test)]
use std::path::PathBuf;

/// Current `SQLite` schema version supported by this crate.
pub(crate) const SCHEMA_VERSION: i64 = 10;
/// Released 0.3.26 schema accepted by the migration inventory.
pub(crate) const PREVIOUS_SCHEMA_VERSION: i64 = 8;
/// First internal schema with explicit publication invalidation.
const PUBLICATION_SCHEMA_VERSION: i64 = 9;
/// Metadata key for the durable schema version.
pub(crate) const SCHEMA_VERSION_KEY: &str = "schema_version";
/// Metadata key for the owning project root.
pub(crate) const PROJECT_ROOT_KEY: &str = "project_root";
/// Metadata key for the current derived-index publication state.
pub(crate) const INDEX_PUBLICATION_STATE_KEY: &str = "index_publication_state";
/// Metadata key for the completed derived-index contract fingerprint.
pub(crate) const INDEX_PUBLICATION_FINGERPRINT_KEY: &str = "index_publication_fingerprint";
/// Metadata key for the monotonically increasing complete index generation.
pub(crate) const INDEX_PUBLICATION_GENERATION_KEY: &str = "index_publication_generation";

/// Released schema state accepted by the storage owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SchemaState {
    /// No durable `ProjectAtlas` schema exists yet.
    Fresh,
    /// The database already matches this runtime.
    Current,
    /// A supported predecessor can be upgraded transactionally.
    UpgradeRequired,
}

/// Result of a non-mutating compatibility inspection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SchemaPreflight {
    /// Closed compatibility state.
    pub(crate) state: SchemaState,
    /// Existing normalized project identity, when present.
    pub(crate) project_root: Option<String>,
}

/// One append-only supported schema transition.
struct Migration {
    /// Durable source version.
    from: i64,
    /// Durable target version.
    to: i64,
    /// Transaction-owned migration body.
    apply: fn(&Connection) -> DbResult<()>,
}

/// Closed migration inventory. New transitions append to this list.
const MIGRATIONS: &[Migration] = &[
    Migration {
        from: PREVIOUS_SCHEMA_VERSION,
        to: PUBLICATION_SCHEMA_VERSION,
        apply: migrate_8_to_9,
    },
    Migration {
        from: PUBLICATION_SCHEMA_VERSION,
        to: SCHEMA_VERSION,
        apply: migrate_9_to_10,
    },
];

/// Behavior-relevant schema contract derived from the authoritative DDL.
#[derive(Debug, Eq, PartialEq)]
struct SchemaContract {
    /// Exact durable user tables and their columns.
    tables: Vec<TableContract>,
    /// Required declared and constraint-owned indexes.
    indexes: Vec<IndexContract>,
    /// Exact foreign-key behavior for every durable table.
    foreign_keys: Vec<ForeignKeyContract>,
}

/// One durable table and its complete column layout.
#[derive(Debug, Eq, PartialEq)]
struct TableContract {
    /// Durable table name.
    name: String,
    /// Normalized authoritative table definition, including table constraints.
    definition: String,
    /// Columns in declared order.
    columns: Vec<ColumnContract>,
}

/// One column row returned by `PRAGMA table_xinfo`.
#[derive(Debug, Eq, PartialEq)]
struct ColumnContract {
    /// Declared ordinal.
    position: i64,
    /// Durable column name.
    name: String,
    /// Declared `SQLite` type.
    declared_type: String,
    /// Whether inserts must provide a non-null value.
    not_null: bool,
    /// Normalized SQL default expression.
    default_value: Option<String>,
    /// One-based primary-key position, or zero when not in the key.
    primary_key_position: i64,
    /// `SQLite` hidden/generated-column state.
    hidden: i64,
}

/// One logical index returned by `PRAGMA index_list`.
#[derive(Debug, Eq, PartialEq)]
struct IndexContract {
    /// Table whose rows are indexed.
    table: String,
    /// Durable index identity, including generated constraint indexes.
    name: String,
    /// Whether the index enforces uniqueness.
    unique: bool,
    /// `SQLite` origin (`c`, `u`, or `pk`).
    origin: String,
    /// Whether a predicate limits indexed rows.
    partial: bool,
    /// Key and auxiliary columns in `PRAGMA index_xinfo` order.
    columns: Vec<IndexColumnContract>,
}

/// One key or auxiliary index column returned by `PRAGMA index_xinfo`.
#[derive(Debug, Eq, PartialEq)]
struct IndexColumnContract {
    /// Index-column ordinal.
    sequence: i64,
    /// Table-column ordinal, or a negative `SQLite` sentinel.
    column_id: i64,
    /// Column name when the entry is not an expression or row identifier.
    name: Option<String>,
    /// Whether this key is descending.
    descending: bool,
    /// Selected collation, when reported.
    collation: Option<String>,
    /// Whether this entry participates in the index key.
    key: bool,
}

/// One foreign-key column mapping returned by `PRAGMA foreign_key_list`.
#[derive(Debug, Eq, PartialEq)]
struct ForeignKeyContract {
    /// Local table that owns the constraint.
    table: String,
    /// Constraint identity within the table.
    id: i64,
    /// Column position within a composite constraint.
    sequence: i64,
    /// Referenced table.
    target_table: String,
    /// Local column.
    source_column: String,
    /// Referenced column, or the referenced primary key when absent.
    target_column: Option<String>,
    /// Update action.
    on_update: String,
    /// Delete action.
    on_delete: String,
    /// Match policy.
    match_policy: String,
}

/// Process-local immutable current contract derived from authoritative DDL.
static SCHEMA_CONTRACT: OnceLock<SchemaContract> = OnceLock::new();
/// Process-local immutable predecessor contract derived from base DDL.
static PREDECESSOR_SCHEMA_CONTRACT: OnceLock<SchemaContract> = OnceLock::new();

/// Immutable physical schema emitted by the released 0.3.26 runtime.
#[cfg(test)]
const RELEASED_SCHEMA_EIGHT_SQL: &str = include_str!("../tests/fixtures/released-schema-8.sql");

/// Create the historical released schema without consulting current DDL.
#[cfg(test)]
pub(crate) fn create_released_schema_eight(connection: &Connection) -> DbResult<()> {
    connection.execute_batch(RELEASED_SCHEMA_EIGHT_SQL)?;
    set_metadata(
        connection,
        SCHEMA_VERSION_KEY,
        &PREVIOUS_SCHEMA_VERSION.to_string(),
    )
}

/// Base schema shared by supported predecessors and fresh databases.
const BASE_SCHEMA_SQL: &str = "
    CREATE TABLE metadata (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    CREATE TABLE nodes (
        id INTEGER PRIMARY KEY,
        path TEXT UNIQUE NOT NULL,
        kind TEXT NOT NULL,
        parent_path TEXT,
        extension TEXT,
        language TEXT,
        size_bytes INTEGER,
        mtime_ns INTEGER,
        content_hash TEXT,
        exists_now INTEGER NOT NULL DEFAULT 1,
        first_seen_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        last_seen_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        last_indexed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE purposes (
        node_id INTEGER PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
        purpose TEXT,
        source TEXT NOT NULL,
        status TEXT NOT NULL,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_by TEXT
    );

    CREATE TABLE summaries (
        id INTEGER PRIMARY KEY,
        node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
        summary_level TEXT NOT NULL DEFAULT 'node',
        subject TEXT NOT NULL DEFAULT '',
        summary TEXT,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        UNIQUE(node_id, summary_level, subject)
    );

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
        calculation_trace TEXT NOT NULL DEFAULT 'heuristic=ceil(chars_or_bytes/4)',
        accounting_layer TEXT NOT NULL DEFAULT 'modeled_avoidance',
        estimate_method TEXT NOT NULL DEFAULT 'heuristic_chars_or_bytes_div_ceil_4',
        denominator_kind TEXT NOT NULL DEFAULT 'selected_candidates',
        baseline_identity TEXT NOT NULL DEFAULT '',
        baseline_fingerprint TEXT NOT NULL DEFAULT '',
        dedupe_scope TEXT NOT NULL DEFAULT 'session',
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE symbols (
        id INTEGER PRIMARY KEY,
        path TEXT NOT NULL,
        language TEXT,
        name TEXT NOT NULL,
        kind TEXT NOT NULL,
        signature TEXT NOT NULL,
        exported INTEGER NOT NULL DEFAULT 0,
        documentation TEXT,
        line_start INTEGER NOT NULL,
        line_end INTEGER NOT NULL,
        parent TEXT,
        parser TEXT NOT NULL,
        detail TEXT,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE source_parse_metadata (
        path TEXT PRIMARY KEY,
        language TEXT,
        parser TEXT NOT NULL,
        symbol_count INTEGER NOT NULL,
        relation_count INTEGER NOT NULL,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE symbol_relations (
        id INTEGER PRIMARY KEY,
        path TEXT NOT NULL,
        source_name TEXT NOT NULL,
        target_name TEXT NOT NULL,
        kind TEXT NOT NULL,
        line INTEGER NOT NULL,
        context TEXT NOT NULL,
        parser TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE health_resolutions (
        finding_id TEXT PRIMARY KEY,
        category TEXT NOT NULL,
        path TEXT NOT NULL,
        related_path TEXT,
        rationale TEXT NOT NULL,
        resolved_by TEXT NOT NULL DEFAULT 'agent',
        resolved_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE file_texts (
        path TEXT PRIMARY KEY,
        content_hash TEXT,
        byte_count INTEGER NOT NULL,
        line_count INTEGER NOT NULL,
        content TEXT NOT NULL,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE INDEX idx_nodes_kind ON nodes(kind);
    CREATE INDEX idx_nodes_parent ON nodes(parent_path);
    CREATE INDEX idx_purposes_status ON purposes(status);
    CREATE INDEX idx_summaries_level ON summaries(summary_level);
    CREATE INDEX idx_summaries_summary ON summaries(summary);
    CREATE INDEX idx_usage_session ON usage_events(session_id);
    CREATE INDEX idx_symbols_path ON symbols(path);
    CREATE INDEX idx_symbols_name ON symbols(name);
    CREATE INDEX idx_symbols_kind ON symbols(kind);
    CREATE INDEX idx_source_parse_metadata_parser ON source_parse_metadata(parser);
    CREATE INDEX idx_symbol_relations_path ON symbol_relations(path);
    CREATE INDEX idx_symbol_relations_target ON symbol_relations(target_name);
    CREATE INDEX idx_health_resolutions_category ON health_resolutions(category);
    CREATE INDEX idx_file_texts_hash ON file_texts(content_hash);
    CREATE INDEX idx_usage_created_at ON usage_events(created_at);
    CREATE INDEX idx_usage_session_created_at ON usage_events(session_id, created_at);
";

/// Normalized repository graph schema introduced after publication schema 9.
const GRAPH_SCHEMA_SQL: &str = "
    CREATE TABLE project_identity (
        singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
        project_instance_id BLOB NOT NULL UNIQUE
            CHECK(
                typeof(project_instance_id) = 'blob'
                AND length(project_instance_id) = 16
                AND project_instance_id <> X'00000000000000000000000000000000'
            ),
        active_generation INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(active_generation) = 'integer' AND active_generation >= 0)
    );

    CREATE TABLE graph_entities (
        entity_key BLOB PRIMARY KEY NOT NULL
            CHECK(typeof(entity_key) = 'blob' AND length(entity_key) = 32),
        project_instance_id BLOB NOT NULL
            CHECK(typeof(project_instance_id) = 'blob' AND length(project_instance_id) = 16),
        canonical_identity TEXT NOT NULL UNIQUE
            CHECK(typeof(canonical_identity) = 'text' AND length(canonical_identity) > 0),
        entity_kind TEXT NOT NULL
            CHECK(entity_kind IN ('project', 'folder', 'file', 'package', 'symbol', 'external')),
        repository_path TEXT,
        package_manager TEXT,
        package_name TEXT,
        manifest_path TEXT,
        symbol_name TEXT,
        symbol_kind TEXT,
        symbol_parent TEXT,
        symbol_signature TEXT,
        external_system TEXT,
        external_identity TEXT,
        UNIQUE(project_instance_id, entity_key),
        FOREIGN KEY(project_instance_id)
            REFERENCES project_identity(project_instance_id) ON DELETE RESTRICT,
        FOREIGN KEY(repository_path) REFERENCES nodes(path) ON DELETE CASCADE,
        FOREIGN KEY(manifest_path) REFERENCES nodes(path) ON DELETE CASCADE,
        CHECK(repository_path IS NULL OR (typeof(repository_path) = 'text' AND length(repository_path) > 0)),
        CHECK(package_manager IS NULL OR (typeof(package_manager) = 'text' AND length(package_manager) > 0)),
        CHECK(package_name IS NULL OR (typeof(package_name) = 'text' AND length(package_name) > 0)),
        CHECK(manifest_path IS NULL OR (typeof(manifest_path) = 'text' AND length(manifest_path) > 0)),
        CHECK(symbol_name IS NULL OR (typeof(symbol_name) = 'text' AND length(symbol_name) > 0)),
        CHECK(symbol_kind IS NULL OR symbol_kind IN (
            'function', 'method', 'class', 'struct', 'enum', 'trait', 'interface',
            'module', 'type', 'value', 'import', 'package', 'workspace', 'dependency', 'unknown'
        )),
        CHECK(symbol_parent IS NULL OR (typeof(symbol_parent) = 'text' AND length(symbol_parent) > 0)),
        CHECK(symbol_signature IS NULL OR (typeof(symbol_signature) = 'text' AND length(symbol_signature) > 0)),
        CHECK(external_system IS NULL OR (typeof(external_system) = 'text' AND length(external_system) > 0)),
        CHECK(external_identity IS NULL OR (typeof(external_identity) = 'text' AND length(external_identity) > 0)),
        CHECK(
            (entity_kind = 'project'
                AND repository_path IS NULL
                AND package_manager IS NULL AND package_name IS NULL AND manifest_path IS NULL
                AND symbol_name IS NULL AND symbol_kind IS NULL AND symbol_parent IS NULL
                AND symbol_signature IS NULL AND external_system IS NULL AND external_identity IS NULL)
            OR (entity_kind IN ('folder', 'file')
                AND repository_path IS NOT NULL
                AND package_manager IS NULL AND package_name IS NULL AND manifest_path IS NULL
                AND symbol_name IS NULL AND symbol_kind IS NULL AND symbol_parent IS NULL
                AND symbol_signature IS NULL AND external_system IS NULL AND external_identity IS NULL)
            OR (entity_kind = 'package'
                AND repository_path IS NULL
                AND package_manager IS NOT NULL AND package_name IS NOT NULL AND manifest_path IS NOT NULL
                AND symbol_name IS NULL AND symbol_kind IS NULL AND symbol_parent IS NULL
                AND symbol_signature IS NULL AND external_system IS NULL AND external_identity IS NULL)
            OR (entity_kind = 'symbol'
                AND repository_path IS NOT NULL
                AND package_manager IS NULL AND package_name IS NULL AND manifest_path IS NULL
                AND symbol_name IS NOT NULL AND symbol_kind IS NOT NULL AND symbol_signature IS NOT NULL
                AND external_system IS NULL AND external_identity IS NULL)
            OR (entity_kind = 'external'
                AND repository_path IS NULL
                AND package_manager IS NULL AND package_name IS NULL AND manifest_path IS NULL
                AND symbol_name IS NULL AND symbol_kind IS NULL AND symbol_parent IS NULL
                AND symbol_signature IS NULL AND external_system IS NOT NULL AND external_identity IS NOT NULL)
        )
    );

    CREATE TABLE graph_relations (
        relation_key BLOB PRIMARY KEY NOT NULL
            CHECK(typeof(relation_key) = 'blob' AND length(relation_key) = 32),
        project_instance_id BLOB NOT NULL
            CHECK(typeof(project_instance_id) = 'blob' AND length(project_instance_id) = 16),
        canonical_identity TEXT NOT NULL UNIQUE
            CHECK(typeof(canonical_identity) = 'text' AND length(canonical_identity) > 0),
        source_entity_key BLOB NOT NULL
            CHECK(typeof(source_entity_key) = 'blob' AND length(source_entity_key) = 32),
        relation_scope TEXT NOT NULL CHECK(relation_scope IN ('legacy', 'extended')),
        relation_kind TEXT NOT NULL,
        resolution_status TEXT NOT NULL
            CHECK(resolution_status IN ('resolved', 'ambiguous', 'unresolved', 'external')),
        target_entity_key BLOB
            CHECK(target_entity_key IS NULL OR (typeof(target_entity_key) = 'blob' AND length(target_entity_key) = 32)),
        reference_text TEXT
            CHECK(reference_text IS NULL OR (typeof(reference_text) = 'text' AND length(reference_text) > 0)),
        candidate_count INTEGER
            CHECK(candidate_count IS NULL OR (typeof(candidate_count) = 'integer' AND candidate_count > 0)),
        confidence TEXT NOT NULL CHECK(confidence IN ('exact', 'high', 'medium', 'low')),
        completeness TEXT NOT NULL CHECK(completeness IN ('complete', 'partial')),
        FOREIGN KEY(project_instance_id)
            REFERENCES project_identity(project_instance_id) ON DELETE RESTRICT,
        FOREIGN KEY(project_instance_id, source_entity_key)
            REFERENCES graph_entities(project_instance_id, entity_key) ON DELETE CASCADE,
        FOREIGN KEY(project_instance_id, target_entity_key)
            REFERENCES graph_entities(project_instance_id, entity_key) ON DELETE CASCADE,
        CHECK(
            (relation_scope = 'legacy'
                AND relation_kind IN ('contains', 'imports', 'calls', 'depends-on'))
            OR (relation_scope = 'extended'
                AND relation_kind IN ('references', 'tests', 'routes-to', 'configures', 'reads', 'writes'))
        ),
        CHECK(
            (resolution_status IN ('resolved', 'external')
                AND target_entity_key IS NOT NULL
                AND reference_text IS NULL AND candidate_count IS NULL)
            OR (resolution_status = 'ambiguous'
                AND target_entity_key IS NULL
                AND reference_text IS NOT NULL AND candidate_count IS NOT NULL)
            OR (resolution_status = 'unresolved'
                AND target_entity_key IS NULL
                AND reference_text IS NOT NULL AND candidate_count IS NULL)
        )
    );

    CREATE TABLE graph_relation_occurrences (
        id INTEGER PRIMARY KEY,
        relation_key BLOB NOT NULL
            CHECK(typeof(relation_key) = 'blob' AND length(relation_key) = 32),
        file_path TEXT NOT NULL
            CHECK(typeof(file_path) = 'text' AND length(file_path) > 0),
        start_line INTEGER NOT NULL
            CHECK(typeof(start_line) = 'integer' AND start_line > 0),
        start_column INTEGER NOT NULL
            CHECK(typeof(start_column) = 'integer' AND start_column >= 0),
        end_line INTEGER NOT NULL
            CHECK(typeof(end_line) = 'integer' AND end_line > 0),
        end_column INTEGER NOT NULL
            CHECK(typeof(end_column) = 'integer' AND end_column >= 0),
        FOREIGN KEY(relation_key) REFERENCES graph_relations(relation_key) ON DELETE CASCADE,
        FOREIGN KEY(file_path) REFERENCES nodes(path) ON DELETE CASCADE,
        UNIQUE(relation_key, file_path, start_line, start_column, end_line, end_column),
        CHECK(end_line > start_line OR (end_line = start_line AND end_column >= start_column))
    );

    CREATE TABLE graph_coverage (
        id INTEGER PRIMARY KEY,
        project_instance_id BLOB NOT NULL
            CHECK(typeof(project_instance_id) = 'blob' AND length(project_instance_id) = 16),
        scope_kind TEXT NOT NULL CHECK(scope_kind IN ('project', 'path')),
        scope_path TEXT
            CHECK(scope_path IS NULL OR (typeof(scope_path) = 'text' AND length(scope_path) > 0)),
        relation_scope TEXT CHECK(relation_scope IS NULL OR relation_scope IN ('legacy', 'extended')),
        relation_kind TEXT,
        state TEXT NOT NULL
            CHECK(state IN ('complete', 'partial', 'failed', 'ignored', 'oversized', 'quarantined', 'stale')),
        total INTEGER NOT NULL CHECK(typeof(total) = 'integer' AND total >= 0),
        covered INTEGER NOT NULL CHECK(typeof(covered) = 'integer' AND covered >= 0),
        omitted INTEGER NOT NULL CHECK(typeof(omitted) = 'integer' AND omitted >= 0),
        reason TEXT CHECK(reason IS NULL OR (typeof(reason) = 'text' AND length(reason) > 0)),
        reached_limit TEXT
            CHECK(reached_limit IS NULL OR reached_limit IN ('rows', 'occurrences', 'depth', 'output_bytes')),
        FOREIGN KEY(project_instance_id)
            REFERENCES project_identity(project_instance_id) ON DELETE RESTRICT,
        CHECK(
            (scope_kind = 'project' AND scope_path IS NULL)
            OR (scope_kind = 'path' AND scope_path IS NOT NULL)
        ),
        CHECK(
            (relation_scope IS NULL AND relation_kind IS NULL)
            OR (relation_scope = 'legacy'
                AND relation_kind IN ('contains', 'imports', 'calls', 'depends-on'))
            OR (relation_scope = 'extended'
                AND relation_kind IN ('references', 'tests', 'routes-to', 'configures', 'reads', 'writes'))
        ),
        CHECK(total = covered + omitted),
        CHECK(
            (state = 'complete' AND omitted = 0 AND reason IS NULL AND reached_limit IS NULL)
            OR (state = 'partial' AND covered > 0 AND omitted > 0 AND reason IS NOT NULL)
            OR (state IN ('failed', 'ignored', 'oversized', 'quarantined', 'stale')
                AND covered = 0 AND omitted > 0 AND reason IS NOT NULL)
        )
    );

    CREATE INDEX idx_graph_entities_path
        ON graph_entities(repository_path, entity_kind, canonical_identity, entity_key);
    CREATE INDEX idx_graph_entities_package
        ON graph_entities(package_manager, package_name, manifest_path, entity_key);
    CREATE INDEX idx_graph_entities_manifest_path
        ON graph_entities(manifest_path, entity_key);
    CREATE INDEX idx_graph_entities_symbol
        ON graph_entities(repository_path, symbol_name, symbol_kind, symbol_parent, symbol_signature, entity_key);
    CREATE INDEX idx_graph_entities_external
        ON graph_entities(external_system, external_identity, entity_key);
    CREATE INDEX idx_graph_relations_source_kind
        ON graph_relations(source_entity_key, relation_scope, relation_kind, canonical_identity, relation_key);
    CREATE INDEX idx_graph_relations_target_kind
        ON graph_relations(target_entity_key, relation_scope, relation_kind, canonical_identity, relation_key);
    CREATE INDEX idx_graph_relations_kind_order
        ON graph_relations(relation_scope, relation_kind, canonical_identity, relation_key);
    CREATE INDEX idx_graph_relations_kind_resolution
        ON graph_relations(relation_scope, relation_kind, resolution_status, relation_key);
    CREATE INDEX idx_graph_occurrences_file_span
        ON graph_relation_occurrences(file_path, start_line, start_column, relation_key);
    CREATE UNIQUE INDEX idx_graph_coverage_identity
        ON graph_coverage(
            project_instance_id,
            scope_kind,
            ifnull(scope_path, ''),
            ifnull(relation_scope, ''),
            ifnull(relation_kind, '')
        );
    CREATE INDEX idx_graph_coverage_scope_state
        ON graph_coverage(scope_kind, scope_path, state, id);
    CREATE INDEX idx_graph_coverage_scope_order
        ON graph_coverage(scope_kind, scope_path, relation_scope, relation_kind, state, id);
    CREATE INDEX idx_graph_coverage_path
        ON graph_coverage(scope_path, id);
    CREATE INDEX idx_graph_coverage_relation_state
        ON graph_coverage(relation_scope, relation_kind, state, id);
";

/// Inspect an existing database without creating, migrating, or repairing it.
pub(crate) fn preflight(path: &Path, expected_root: Option<&str>) -> DbResult<SchemaPreflight> {
    preflight_with_integrity(path, expected_root, true)
}

/// Inspect one stable read snapshot with caller-selected integrity depth.
fn preflight_with_integrity(
    path: &Path,
    expected_root: Option<&str>,
    run_integrity_check: bool,
) -> DbResult<SchemaPreflight> {
    if !path.exists() {
        return Ok(SchemaPreflight {
            state: SchemaState::Fresh,
            project_root: None,
        });
    }
    let connection = open_read_only_connection(path)?;
    connection.execute_batch("BEGIN DEFERRED")?;
    let inspected = inspect_connection(&connection, expected_root, run_integrity_check);
    match inspected {
        Ok(preflight) => {
            connection.execute_batch("COMMIT")?;
            Ok(preflight)
        }
        Err(error) => Err(rollback_after_error(&connection, error)),
    }
}

/// Initialize or migrate one already-open writable connection.
pub(crate) fn initialize(connection: &Connection, expected_root: Option<&str>) -> DbResult<()> {
    configure_writable(connection)?;
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        let preflight = inspect_connection(connection, expected_root, false)?;
        match preflight.state {
            SchemaState::Fresh => create_fresh(connection, expected_root)?,
            SchemaState::Current => {}
            SchemaState::UpgradeRequired => {
                validate_integrity(connection)?;
                apply_migrations(connection, stored_schema_version(connection)?)?;
            }
        }
        if expected_root.is_some() {
            crate::project_identity::ensure_project_identity(connection)?;
        }
        let current = inspect_connection(connection, expected_root, false)?;
        if current.state != SchemaState::Current {
            return Err(DbError::SchemaPostcondition {
                expected: SCHEMA_VERSION,
            });
        }
        Ok(())
    })();
    match result {
        Ok(()) => connection.execute_batch("COMMIT").map_err(Into::into),
        Err(error) => Err(rollback_after_error(connection, error)),
    }
}

/// Enable the connection-local integrity rules required for every write path.
pub(crate) fn configure_writable(connection: &Connection) -> DbResult<()> {
    connection.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}

/// Open and validate one current read-only snapshot.
pub(crate) fn open_current_read_only(
    path: &Path,
    expected_root: Option<&str>,
) -> DbResult<(Connection, SchemaPreflight)> {
    let connection = open_read_only_connection(path)?;
    connection.execute_batch("BEGIN DEFERRED")?;
    match inspect_current(&connection, expected_root) {
        Ok(preflight) => Ok((connection, preflight)),
        Err(error) => Err(rollback_after_error(&connection, error)),
    }
}

/// Validate the current schema and optional project identity in an active transaction.
pub(crate) fn validate_current(
    connection: &Connection,
    expected_root: Option<&str>,
) -> DbResult<()> {
    inspect_current(connection, expected_root).map(drop)
}

/// Return the validated current schema state from an active transaction.
fn inspect_current(
    connection: &Connection,
    expected_root: Option<&str>,
) -> DbResult<SchemaPreflight> {
    let preflight = inspect_connection(connection, expected_root, false)?;
    if preflight.state != SchemaState::Current {
        return Err(DbError::SchemaVersion {
            found: stored_schema_version(connection)?,
            expected: SCHEMA_VERSION,
        });
    }
    Ok(preflight)
}

/// Read the existing project identity without mutating or migrating the database.
pub(crate) fn read_project_root(path: &Path) -> DbResult<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let connection = open_read_only_connection(path)?;
    connection.execute_batch("BEGIN DEFERRED")?;
    let inspected = inspect_connection(&connection, None, false);
    match inspected {
        Ok(preflight) => {
            connection.execute_batch("COMMIT")?;
            Ok(preflight.project_root)
        }
        Err(error) => Err(rollback_after_error(&connection, error)),
    }
}

/// Open a database-non-mutating connection without ignoring committed WAL state.
///
/// `SQLite` may materialize its own WAL/SHM support files for a locked read.
/// That native coordination is intentional: `immutable=1` would avoid the
/// sidecars by bypassing locks even though another local process may write.
fn open_read_only_connection(path: &Path) -> DbResult<Connection> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.execute_batch("PRAGMA query_only = ON")?;
    Ok(connection)
}

/// Inspect compatibility, integrity, object shape, and root identity.
fn inspect_connection(
    connection: &Connection,
    expected_root: Option<&str>,
    run_integrity_check: bool,
) -> DbResult<SchemaPreflight> {
    if run_integrity_check {
        validate_integrity(connection)?;
    }
    let state = schema_state(connection)?;
    if matches!(state, SchemaState::Current | SchemaState::UpgradeRequired) {
        validate_schema_shape(connection, state)?;
    }
    let project_root = if state == SchemaState::Fresh {
        None
    } else {
        read_metadata(connection, PROJECT_ROOT_KEY)?
    };
    if let Some(expected) = expected_root {
        match project_root.as_deref() {
            Some(found) if found == expected => {}
            Some(found) => {
                return Err(DbError::ProjectRootMismatch {
                    expected: expected.to_string(),
                    found: found.to_string(),
                });
            }
            None if state == SchemaState::Fresh => {}
            None => return Err(DbError::ProjectRootMissing),
        }
    }
    Ok(SchemaPreflight {
        state,
        project_root,
    })
}

/// Determine the closed schema state from durable metadata.
fn schema_state(connection: &Connection) -> DbResult<SchemaState> {
    let metadata_kind = object_kind(connection, "metadata")?;
    match metadata_kind.as_deref() {
        None => {
            let user_objects = connection.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get::<_, i64>(0),
            )?;
            if user_objects == 0 {
                return Ok(SchemaState::Fresh);
            }
            return Err(DbError::SchemaShape {
                object: "metadata".to_string(),
                expected: "table".to_string(),
                found: "missing".to_string(),
            });
        }
        Some("table") => {}
        Some(found) => {
            return Err(DbError::SchemaShape {
                object: "metadata".to_string(),
                expected: "table".to_string(),
                found: found.to_string(),
            });
        }
    }
    let found = stored_schema_version(connection)?;
    match found {
        SCHEMA_VERSION => Ok(SchemaState::Current),
        PREVIOUS_SCHEMA_VERSION | PUBLICATION_SCHEMA_VERSION => Ok(SchemaState::UpgradeRequired),
        _ => Err(DbError::SchemaVersion {
            found,
            expected: SCHEMA_VERSION,
        }),
    }
}

/// Create a new schema and stamp identity/version only after all DDL succeeds.
fn create_fresh(connection: &Connection, expected_root: Option<&str>) -> DbResult<()> {
    connection.execute_batch(BASE_SCHEMA_SQL)?;
    connection.execute_batch(GRAPH_SCHEMA_SQL)?;
    if let Some(root) = expected_root {
        set_metadata(connection, PROJECT_ROOT_KEY, root)?;
    }
    set_metadata(connection, SCHEMA_VERSION_KEY, &SCHEMA_VERSION.to_string())
}

/// Apply the fixed migration path without skipping or synthesizing versions.
fn apply_migrations(connection: &Connection, mut version: i64) -> DbResult<()> {
    while version != SCHEMA_VERSION {
        let migration = MIGRATIONS
            .iter()
            .find(|migration| migration.from == version)
            .ok_or(DbError::SchemaVersion {
                found: version,
                expected: SCHEMA_VERSION,
            })?;
        if migration.to <= migration.from {
            return Err(DbError::SchemaPostcondition {
                expected: SCHEMA_VERSION,
            });
        }
        (migration.apply)(connection)?;
        version = migration.to;
    }
    set_metadata(connection, SCHEMA_VERSION_KEY, &SCHEMA_VERSION.to_string())
}

/// Invalidate schema-8 derived publication trust while preserving authored state.
fn migrate_8_to_9(connection: &Connection) -> DbResult<()> {
    invalidate_derived_publication(connection)
}

/// Add normalized graph storage and invalidate predecessor publication trust.
fn migrate_9_to_10(connection: &Connection) -> DbResult<()> {
    connection.execute_batch(GRAPH_SCHEMA_SQL)?;
    if read_metadata(connection, PROJECT_ROOT_KEY)?.is_some() {
        crate::project_identity::ensure_project_identity(connection)?;
    }
    invalidate_derived_publication(connection)
}

/// Invalidate derived rows without deleting authored local state.
pub(crate) fn invalidate_derived_publication(connection: &Connection) -> DbResult<()> {
    connection.execute(
        "DELETE FROM metadata WHERE key IN (?1, ?2, ?3)",
        params![
            INDEX_PUBLICATION_STATE_KEY,
            INDEX_PUBLICATION_FINGERPRINT_KEY,
            INDEX_PUBLICATION_GENERATION_KEY,
        ],
    )?;
    Ok(())
}

/// Verify `SQLite` and foreign-key integrity through the selected snapshot.
fn validate_integrity(connection: &Connection) -> DbResult<()> {
    let result =
        connection.query_row("PRAGMA quick_check(1)", [], |row| row.get::<_, String>(0))?;
    if result != "ok" {
        return Err(DbError::IntegrityCheck { message: result });
    }
    let foreign_key_failure = connection
        .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
        .optional()?;
    if foreign_key_failure.is_some() {
        return Err(DbError::IntegrityCheck {
            message: "foreign key check failed".to_string(),
        });
    }
    Ok(())
}

/// Validate the exact tables, columns, constraints, and required indexes without repair DDL.
fn validate_schema_shape(connection: &Connection, state: SchemaState) -> DbResult<()> {
    let expected = match state {
        SchemaState::Current => schema_contract()?,
        SchemaState::UpgradeRequired => predecessor_schema_contract()?,
        SchemaState::Fresh => {
            return Err(DbError::SchemaPostcondition {
                expected: SCHEMA_VERSION,
            });
        }
    };
    let found = read_schema_contract(connection)?;
    if expected.tables != found.tables {
        return Err(schema_shape_error(
            "tables",
            &expected.tables,
            &found.tables,
        ));
    }
    if expected.foreign_keys != found.foreign_keys {
        return Err(schema_shape_error(
            "foreign_keys",
            &expected.foreign_keys,
            &found.foreign_keys,
        ));
    }
    for required in &expected.indexes {
        let Some(actual) = found
            .indexes
            .iter()
            .find(|index| index.name == required.name)
        else {
            return Err(DbError::SchemaShape {
                object: required.name.clone(),
                expected: format!("{required:?}"),
                found: "missing".to_string(),
            });
        };
        if actual != required {
            return Err(schema_shape_error(&required.name, required, actual));
        }
    }
    for extra in found
        .indexes
        .iter()
        .filter(|index| !expected.indexes.iter().any(|item| item.name == index.name))
    {
        if !is_compatible_extension_index(extra) {
            return Err(DbError::SchemaShape {
                object: extra.name.clone(),
                expected: "optional non-unique full index over declared columns".to_string(),
                found: format!("{extra:?}"),
            });
        }
    }
    validate_extension_objects(connection)?;
    Ok(())
}

/// Return whether an extra index is a behavior-neutral lookup optimization.
fn is_compatible_extension_index(index: &IndexContract) -> bool {
    index.origin == "c"
        && !index.unique
        && !index.partial
        && index.columns.iter().all(|column| {
            !column.key
                || (column.column_id >= 0
                    && column.name.is_some()
                    && matches!(column.collation.as_deref(), None | Some("BINARY")))
        })
}

/// Build the immutable expected contract from the same DDL used for fresh databases.
fn schema_contract() -> DbResult<&'static SchemaContract> {
    if let Some(contract) = SCHEMA_CONTRACT.get() {
        return Ok(contract);
    }
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(BASE_SCHEMA_SQL)?;
    connection.execute_batch(GRAPH_SCHEMA_SQL)?;
    let contract = read_schema_contract(&connection)?;
    Ok(SCHEMA_CONTRACT.get_or_init(|| contract))
}

/// Build the immutable schema-8/9 contract from the unchanged base DDL.
fn predecessor_schema_contract() -> DbResult<&'static SchemaContract> {
    if let Some(contract) = PREDECESSOR_SCHEMA_CONTRACT.get() {
        return Ok(contract);
    }
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(BASE_SCHEMA_SQL)?;
    let contract = read_schema_contract(&connection)?;
    Ok(PREDECESSOR_SCHEMA_CONTRACT.get_or_init(|| contract))
}

/// Read all behavior-relevant schema state through `SQLite` introspection.
fn read_schema_contract(connection: &Connection) -> DbResult<SchemaContract> {
    let table_names = user_table_names(connection)?;
    let mut tables = Vec::with_capacity(table_names.len());
    let mut indexes = Vec::new();
    let mut foreign_keys = Vec::new();
    for table in table_names {
        tables.push(TableContract {
            definition: table_definition(connection, &table)?,
            columns: table_columns(connection, &table)?,
            name: table.clone(),
        });
        indexes.extend(table_indexes(connection, &table)?);
        foreign_keys.extend(table_foreign_keys(connection, &table)?);
    }
    indexes.sort_by(|left, right| left.name.cmp(&right.name));
    foreign_keys.sort_by(|left, right| {
        (&left.table, left.id, left.sequence).cmp(&(&right.table, right.id, right.sequence))
    });
    Ok(SchemaContract {
        tables,
        indexes,
        foreign_keys,
    })
}

/// Return a whitespace-normalized table definition from the durable schema.
fn table_definition(connection: &Connection, table: &str) -> DbResult<String> {
    let definition = connection.query_row(
        "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get::<_, String>(0),
    )?;
    Ok(definition.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Return exact user table names in deterministic order.
fn user_table_names(connection: &Connection) -> DbResult<Vec<String>> {
    let mut statement = connection.prepare(
        "SELECT name FROM sqlite_schema \
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Return complete columns for one `SQLite`-validated table name.
fn table_columns(connection: &Connection, table: &str) -> DbResult<Vec<ColumnContract>> {
    let mut statement = connection.prepare(
        "SELECT cid, name, type, \"notnull\", dflt_value, pk, hidden \
         FROM pragma_table_xinfo(?1) ORDER BY cid",
    )?;
    let rows = statement.query_map([table], |row| {
        Ok(ColumnContract {
            position: row.get(0)?,
            name: row.get(1)?,
            declared_type: row.get(2)?,
            not_null: row.get::<_, i64>(3)? != 0,
            default_value: row.get(4)?,
            primary_key_position: row.get(5)?,
            hidden: row.get(6)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Return complete logical indexes for one table.
fn table_indexes(connection: &Connection, table: &str) -> DbResult<Vec<IndexContract>> {
    let mut statement = connection.prepare(
        "SELECT name, \"unique\", origin, partial FROM pragma_index_list(?1) ORDER BY name",
    )?;
    let rows = statement.query_map([table], |row| {
        let name = row.get::<_, String>(0)?;
        Ok(IndexContract {
            columns: index_columns(connection, &name)?,
            table: table.to_string(),
            name,
            unique: row.get::<_, i64>(1)? != 0,
            origin: row.get(2)?,
            partial: row.get::<_, i64>(3)? != 0,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Return key and auxiliary column behavior for one index.
fn index_columns(
    connection: &Connection,
    index: &str,
) -> rusqlite::Result<Vec<IndexColumnContract>> {
    let mut statement = connection.prepare(
        "SELECT seqno, cid, name, \"desc\", coll, key \
         FROM pragma_index_xinfo(?1) ORDER BY seqno",
    )?;
    let rows = statement.query_map([index], |row| {
        Ok(IndexColumnContract {
            sequence: row.get(0)?,
            column_id: row.get(1)?,
            name: row.get(2)?,
            descending: row.get::<_, i64>(3)? != 0,
            collation: row.get(4)?,
            key: row.get::<_, i64>(5)? != 0,
        })
    })?;
    rows.collect()
}

/// Return complete foreign-key behavior for one table.
fn table_foreign_keys(connection: &Connection, table: &str) -> DbResult<Vec<ForeignKeyContract>> {
    let mut statement = connection.prepare(
        "SELECT id, seq, \"table\", \"from\", \"to\", on_update, on_delete, match \
         FROM pragma_foreign_key_list(?1) ORDER BY id, seq",
    )?;
    let rows = statement.query_map([table], |row| {
        Ok(ForeignKeyContract {
            table: table.to_string(),
            id: row.get(0)?,
            sequence: row.get(1)?,
            target_table: row.get(2)?,
            source_column: row.get(3)?,
            target_column: row.get(4)?,
            on_update: row.get(5)?,
            on_delete: row.get(6)?,
            match_policy: row.get(7)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Reject user objects outside exact tables and compatible non-unique indexes.
fn validate_extension_objects(connection: &Connection) -> DbResult<()> {
    let mut statement = connection.prepare(
        "SELECT type, name, tbl_name, sql FROM sqlite_schema \
         WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
    )?;
    let objects = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    for object in objects {
        let (kind, name, table, sql) = object?;
        match kind.as_str() {
            "table" | "index" => {}
            _ => {
                return Err(DbError::SchemaShape {
                    object: name,
                    expected: "contract table or compatible non-unique index".to_string(),
                    found: format!("{kind} on {table} with definition {sql:?}"),
                });
            }
        }
    }
    Ok(())
}

/// Build one readable typed schema mismatch.
fn schema_shape_error(
    object: &str,
    expected: &impl std::fmt::Debug,
    found: &impl std::fmt::Debug,
) -> DbError {
    DbError::SchemaShape {
        object: object.to_string(),
        expected: format!("{expected:?}"),
        found: format!("{found:?}"),
    }
}

/// Read one metadata value from the validated metadata table.
fn read_metadata(connection: &Connection, key: &str) -> DbResult<Option<String>> {
    connection
        .query_row("SELECT value FROM metadata WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()
        .map_err(Into::into)
}

/// Parse the durable schema version from validated metadata.
fn stored_schema_version(connection: &Connection) -> DbResult<i64> {
    let stored =
        read_metadata(connection, SCHEMA_VERSION_KEY)?.ok_or(DbError::SchemaVersionMissing)?;
    stored
        .parse::<i64>()
        .map_err(|source| DbError::InvalidInteger {
            field: SCHEMA_VERSION_KEY,
            value: stored,
            source,
        })
}

/// Upsert one metadata value inside the caller-owned transaction.
fn set_metadata(connection: &Connection, key: &str, value: &str) -> DbResult<()> {
    connection.execute(
        "
        INSERT INTO metadata(key, value)
        VALUES(?1, ?2)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        ",
        [key, value],
    )?;
    Ok(())
}

/// Preserve the initiating typed error when explicit rollback also fails.
pub(crate) fn rollback_after_error(connection: &Connection, operation: DbError) -> DbError {
    match connection.execute_batch("ROLLBACK") {
        Ok(()) => operation,
        Err(rollback) => DbError::TransactionRollback {
            operation: Box::new(operation),
            rollback,
        },
    }
}

/// Return an object's `SQLite` kind.
fn object_kind(connection: &Connection, name: &str) -> DbResult<Option<String>> {
    connection
        .query_row(
            "SELECT type FROM sqlite_master WHERE name = ?1",
            [name],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

/// Return a `SQLite` sidecar path for a database path.
#[cfg(test)]
pub(crate) fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(suffix);
    PathBuf::from(sidecar)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AtlasStore, DbError};
    use std::error::Error;
    use std::fs;
    use std::io;
    use std::sync::{Arc, Barrier};
    use std::thread;

    #[test]
    fn incompatible_database_preflight_preserves_durable_state() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let malformed_path = temp.path().join("malformed.db");
        {
            let connection = Connection::open(&malformed_path)?;
            connection.execute_batch(
                "CREATE VIEW metadata AS
                 SELECT 'schema_version' AS key, '9' AS value
                 UNION ALL
                 SELECT 'project_root', 'C:/plausible/repository';",
            )?;
        }
        let malformed_bytes = fs::read(&malformed_path)?;
        let malformed_inventory = directory_entry_names(temp.path())?;
        let Err(malformed_error) = AtlasStore::open(&malformed_path) else {
            return Err(io::Error::other("metadata view unexpectedly opened").into());
        };
        if !matches!(malformed_error, DbError::SchemaShape { .. }) {
            return Err(io::Error::other("metadata view returned the wrong error").into());
        }
        let Err(root_error) = read_project_root(&malformed_path) else {
            return Err(io::Error::other("unvalidated metadata selected a project root").into());
        };
        if !matches!(root_error, DbError::SchemaShape { .. }) {
            return Err(io::Error::other("root discovery returned the wrong error").into());
        }
        require_unchanged(
            temp.path(),
            &malformed_path,
            &malformed_bytes,
            &malformed_inventory,
        )?;

        let corrupt_path = temp.path().join("corrupt.db");
        fs::write(&corrupt_path, b"not a sqlite database")?;
        let corrupt_bytes = fs::read(&corrupt_path)?;
        let corrupt_inventory = directory_entry_names(temp.path())?;
        if AtlasStore::open(&corrupt_path).is_ok() {
            return Err(io::Error::other("corrupt database unexpectedly opened").into());
        }
        require_unchanged(
            temp.path(),
            &corrupt_path,
            &corrupt_bytes,
            &corrupt_inventory,
        )?;
        Ok(())
    }

    #[test]
    fn invalid_schema_metadata_is_refused_without_mutation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        for (name, value) in [
            ("missing-version.db", None),
            ("invalid-version.db", Some("nine")),
        ] {
            let db_path = temp.path().join(name);
            {
                let connection = Connection::open(&db_path)?;
                connection.execute_batch(
                    "CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);",
                )?;
                if let Some(value) = value {
                    connection.execute(
                        "INSERT INTO metadata(key, value) VALUES(?1, ?2)",
                        params![SCHEMA_VERSION_KEY, value],
                    )?;
                }
            }
            let database_before = fs::read(&db_path)?;
            let inventory_before = directory_entry_names(temp.path())?;
            if AtlasStore::open(&db_path).is_ok() {
                return Err(io::Error::other("invalid schema metadata unexpectedly opened").into());
            }
            require_unchanged(temp.path(), &db_path, &database_before, &inventory_before)?;
        }
        Ok(())
    }

    #[test]
    fn schema_lookalikes_are_refused_without_mutation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let cases = [
            (
                "wrong-type",
                "        path TEXT UNIQUE NOT NULL,\n",
                "        path BLOB UNIQUE NOT NULL,\n",
            ),
            (
                "wrong-nullability",
                "        kind TEXT NOT NULL,\n        parent_path TEXT,",
                "        kind TEXT,\n        parent_path TEXT,",
            ),
            (
                "wrong-primary-key",
                "        key TEXT PRIMARY KEY,",
                "        key TEXT UNIQUE,",
            ),
            (
                "missing-unique-constraint",
                "        path TEXT UNIQUE NOT NULL,\n",
                "        path TEXT NOT NULL,\n",
            ),
            (
                "wrong-named-index-column",
                "    CREATE INDEX idx_nodes_kind ON nodes(kind);",
                "    CREATE INDEX idx_nodes_kind ON nodes(parent_path);",
            ),
            (
                "missing-foreign-key",
                "        node_id INTEGER PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,",
                "        node_id INTEGER PRIMARY KEY,",
            ),
            (
                "wrong-foreign-key-action",
                "        node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,",
                "        node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE RESTRICT,",
            ),
            (
                "added-check-constraint",
                "        kind TEXT NOT NULL,\n        parent_path TEXT,",
                "        kind TEXT NOT NULL CHECK(kind <> 'forbidden'),\n        parent_path TEXT,",
            ),
        ];
        for (name, needle, replacement) in cases {
            let db_path = temp.path().join(format!("{name}.db"));
            write_schema_lookalike(&db_path, &root, needle, replacement)?;
            let database_before = fs::read(&db_path)?;
            let inventory_before = directory_entry_names(temp.path())?;
            let Err(error) = AtlasStore::open_for_project(&db_path, &root) else {
                return Err(io::Error::other(format!("{name} schema unexpectedly opened")).into());
            };
            if !matches!(error, DbError::SchemaShape { .. }) {
                return Err(io::Error::other(format!(
                    "{name} schema returned a non-shape error: {error}"
                ))
                .into());
            }
            require_unchanged(temp.path(), &db_path, &database_before, &inventory_before)?;
        }
        Ok(())
    }

    #[test]
    fn compatible_indexes_remain_extensible_and_triggers_are_refused() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let db_path = temp.path().join("projectatlas.db");
        drop(AtlasStore::open_for_project(&db_path, &root)?);
        {
            let connection = Connection::open(&db_path)?;
            connection.execute_batch(
                "
                CREATE INDEX local_nodes_extension ON nodes(extension);
                PRAGMA wal_checkpoint(TRUNCATE);
                ",
            )?;
        }
        drop(AtlasStore::open_for_project(&db_path, &root)?);

        {
            let connection = Connection::open(&db_path)?;
            connection.execute_batch(
                "
                CREATE INDEX local_nodes_partial ON nodes(path) WHERE exists_now = 1;
                PRAGMA wal_checkpoint(TRUNCATE);
                ",
            )?;
        }
        let partial_bytes = fs::read(&db_path)?;
        let partial_inventory = directory_entry_names(temp.path())?;
        let Err(partial_error) = AtlasStore::open_for_project(&db_path, &root) else {
            return Err(
                io::Error::other("partial local index unexpectedly passed preflight").into(),
            );
        };
        if !matches!(partial_error, DbError::SchemaShape { .. }) {
            return Err(io::Error::other("partial local index returned the wrong error").into());
        }
        require_unchanged(temp.path(), &db_path, &partial_bytes, &partial_inventory)?;

        {
            let connection = Connection::open(&db_path)?;
            connection.execute_batch(
                "
                DROP INDEX local_nodes_partial;
                CREATE TRIGGER local_nodes_observer
                AFTER INSERT ON nodes BEGIN SELECT 1; END;
                PRAGMA wal_checkpoint(TRUNCATE);
                ",
            )?;
        }
        let database_before = fs::read(&db_path)?;
        let inventory_before = directory_entry_names(temp.path())?;
        let Err(error) = AtlasStore::open_for_project(&db_path, &root) else {
            return Err(io::Error::other("attached trigger unexpectedly passed preflight").into());
        };
        if !matches!(error, DbError::SchemaShape { .. }) {
            return Err(io::Error::other("attached trigger returned the wrong error").into());
        }
        require_unchanged(temp.path(), &db_path, &database_before, &inventory_before)?;
        Ok(())
    }

    #[test]
    fn writable_connections_enforce_declared_foreign_keys() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let db_path = temp.path().join("projectatlas.db");
        let store = AtlasStore::open_for_project(&db_path, &root)?;

        let enabled = store
            .connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))?;
        if enabled != 1 {
            return Err(io::Error::other("foreign-key enforcement is disabled").into());
        }
        if store
            .connection
            .execute(
                "INSERT INTO purposes(node_id, source, status) VALUES(999, 'agent', 'approved')",
                [],
            )
            .is_ok()
        {
            return Err(io::Error::other("orphaned purpose row was accepted").into());
        }

        store.connection.execute(
            "INSERT INTO nodes(path, kind) VALUES('src/lib.rs', 'file')",
            [],
        )?;
        let node_id = store.connection.last_insert_rowid();
        store.connection.execute(
            "INSERT INTO purposes(node_id, source, status) VALUES(?1, 'agent', 'approved')",
            [node_id],
        )?;
        store.connection.execute(
            "INSERT INTO summaries(node_id, summary) VALUES(?1, 'source summary')",
            [node_id],
        )?;
        store
            .connection
            .execute("DELETE FROM nodes WHERE id = ?1", [node_id])?;
        for table in ["purposes", "summaries"] {
            let remaining = store.connection.query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE node_id = ?1"),
                [node_id],
                |row| row.get::<_, i64>(0),
            )?;
            if remaining != 0 {
                return Err(io::Error::other(format!(
                    "deleting a node did not cascade into {table}"
                ))
                .into());
            }
        }
        Ok(())
    }

    #[test]
    fn supported_schema_upgrades_preserve_authored_state_and_invalidate_publication()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;

        for version in [PREVIOUS_SCHEMA_VERSION, PUBLICATION_SCHEMA_VERSION] {
            let db_path = temp.path().join(format!("schema-{version}.db"));
            let connection = Connection::open(&db_path)?;
            configure_writable(&connection)?;
            create_released_schema_eight(&connection)?;
            set_metadata(&connection, SCHEMA_VERSION_KEY, &version.to_string())?;
            set_metadata(
                &connection,
                PROJECT_ROOT_KEY,
                &normalize_native_path_display(&root),
            )?;
            set_metadata(&connection, "agent_setting", "kept")?;
            set_metadata(&connection, INDEX_PUBLICATION_STATE_KEY, "complete")?;
            set_metadata(
                &connection,
                INDEX_PUBLICATION_FINGERPRINT_KEY,
                "predecessor-publication",
            )?;
            set_metadata(&connection, INDEX_PUBLICATION_GENERATION_KEY, "23")?;
            connection.execute_batch(
                "
                INSERT INTO nodes(id, path, kind) VALUES(1, 'src/lib.rs', 'file');
                INSERT INTO purposes(node_id, purpose, source, status, updated_by)
                    VALUES(1, 'Own the library.', 'agent', 'approved', 'agent');
                INSERT INTO summaries(node_id, summary) VALUES(1, 'derived summary');
                INSERT INTO usage_events(session_id, command) VALUES('session', 'files');
                INSERT INTO health_resolutions(finding_id, category, path, rationale)
                    VALUES('finding', 'purpose', 'src/lib.rs', 'reviewed');
                INSERT INTO symbols(
                    path, name, kind, signature, line_start, line_end, parser
                ) VALUES('src/lib.rs', 'run', 'function', 'run()', 1, 1, 'rust');
                INSERT INTO file_texts(path, byte_count, line_count, content)
                    VALUES('src/lib.rs', 2, 1, 'fn');
                ",
            )?;
            drop(connection);

            let store = AtlasStore::open_for_project(&db_path, &root)?;
            if read_metadata(&store.connection, SCHEMA_VERSION_KEY)?
                != Some(SCHEMA_VERSION.to_string())
                || read_metadata(&store.connection, PROJECT_ROOT_KEY)?
                    != Some(normalize_native_path_display(&root))
                || read_metadata(&store.connection, "agent_setting")? != Some("kept".to_string())
            {
                return Err(io::Error::other(format!(
                    "schema {version} upgrade changed durable metadata"
                ))
                .into());
            }
            let publication_keys = store.connection.query_row(
                "SELECT COUNT(*) FROM metadata WHERE key IN (?1, ?2, ?3)",
                params![
                    INDEX_PUBLICATION_STATE_KEY,
                    INDEX_PUBLICATION_FINGERPRINT_KEY,
                    INDEX_PUBLICATION_GENERATION_KEY,
                ],
                |row| row.get::<_, i64>(0),
            )?;
            if publication_keys != 0 {
                return Err(io::Error::other(format!(
                    "schema {version} upgrade retained derived publication trust"
                ))
                .into());
            }
            for table in [
                "nodes",
                "purposes",
                "summaries",
                "usage_events",
                "health_resolutions",
                "symbols",
                "file_texts",
            ] {
                let rows = store.connection.query_row(
                    &format!("SELECT COUNT(*) FROM {table}"),
                    [],
                    |row| row.get::<_, i64>(0),
                )?;
                if rows != 1 {
                    return Err(io::Error::other(format!(
                        "schema {version} upgrade changed {table} rows"
                    ))
                    .into());
                }
            }
            let identity = store.connection.query_row(
                "SELECT length(project_instance_id), active_generation FROM project_identity
                  WHERE singleton = 1",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )?;
            if identity != (16, 0) {
                return Err(io::Error::other(format!(
                    "schema {version} upgrade did not initialize a valid project identity"
                ))
                .into());
            }
            for table in [
                "graph_entities",
                "graph_relations",
                "graph_relation_occurrences",
                "graph_coverage",
            ] {
                let rows = store.connection.query_row(
                    &format!("SELECT COUNT(*) FROM {table}"),
                    [],
                    |row| row.get::<_, i64>(0),
                )?;
                if rows != 0 {
                    return Err(io::Error::other(format!(
                        "schema {version} upgrade synthesized {table} rows"
                    ))
                    .into());
                }
            }
        }
        Ok(())
    }

    #[test]
    fn graph_schema_constraints_reject_inconsistent_typed_rows() -> Result<(), Box<dyn Error>> {
        let store = AtlasStore::in_memory()?;
        let project = vec![1_u8; 16];
        let other_project = vec![2_u8; 16];
        let source_key = vec![3_u8; 32];
        let target_key = vec![4_u8; 32];
        let relation_key = vec![5_u8; 32];
        let package_key = vec![6_u8; 32];

        store.connection.execute(
            "INSERT INTO project_identity(singleton, project_instance_id) VALUES(1, ?1)",
            [&project],
        )?;
        if store
            .connection
            .execute(
                "INSERT INTO project_identity(singleton, project_instance_id) VALUES(2, ?1)",
                [&other_project],
            )
            .is_ok()
        {
            return Err(
                io::Error::other("project identity singleton accepted a second row").into(),
            );
        }
        store.connection.execute_batch(
            "INSERT INTO nodes(path, kind) VALUES('src/lib.rs', 'file');
             INSERT INTO nodes(path, kind) VALUES('src/target.rs', 'file');",
        )?;
        if store
            .connection
            .execute(
                "INSERT INTO graph_entities(
                    entity_key, project_instance_id, canonical_identity, entity_kind, repository_path
                 ) VALUES(zeroblob(31), ?1, 'bad-key', 'file', 'src/lib.rs')",
                [&project],
            )
            .is_ok()
        {
            return Err(io::Error::other("short entity key bypassed the schema contract").into());
        }
        if store
            .connection
            .execute(
                "INSERT INTO graph_entities(
                    entity_key, project_instance_id, canonical_identity, entity_kind, repository_path
                 ) VALUES(?1, ?2, 'wrong-project', 'file', 'src/lib.rs')",
                params![&source_key, &other_project],
            )
            .is_ok()
        {
            return Err(io::Error::other("entity bypassed project identity ownership").into());
        }
        if store
            .connection
            .execute(
                "INSERT INTO graph_entities(
                    entity_key, project_instance_id, canonical_identity, entity_kind, repository_path
                 ) VALUES(?1, ?2, 'invalid-symbol', 'symbol', 'src/lib.rs')",
                params![&source_key, &project],
            )
            .is_ok()
        {
            return Err(io::Error::other("incomplete symbol selector was accepted").into());
        }
        if store
            .connection
            .execute(
                "INSERT INTO graph_entities(
                    entity_key, project_instance_id, canonical_identity, entity_kind,
                    package_manager, package_name, manifest_path
                 ) VALUES(?1, ?2, 'missing-manifest', 'package',
                    'cargo', 'missing', 'missing/Cargo.toml')",
                params![&package_key, &project],
            )
            .is_ok()
        {
            return Err(io::Error::other("package accepted a missing manifest node").into());
        }

        store.connection.execute(
            "INSERT INTO graph_entities(
                entity_key, project_instance_id, canonical_identity, entity_kind, repository_path
             ) VALUES(?1, ?2, 'source-file', 'file', 'src/lib.rs')",
            params![&source_key, &project],
        )?;
        store.connection.execute(
            "INSERT INTO graph_entities(
                entity_key, project_instance_id, canonical_identity, entity_kind, repository_path,
                symbol_name, symbol_kind, symbol_signature
             ) VALUES(?1, ?2, 'target-symbol', 'symbol', 'src/target.rs',
                'target', 'function', 'target()')",
            params![&target_key, &project],
        )?;

        if store
            .connection
            .execute(
                "INSERT INTO graph_relations(
                    relation_key, project_instance_id, canonical_identity, source_entity_key,
                    relation_scope, relation_kind, resolution_status, target_entity_key,
                    confidence, completeness
                 ) VALUES(?1, ?2, 'invalid-kind', ?3, 'extended', 'calls', 'resolved', ?4,
                    'exact', 'complete')",
                params![&relation_key, &project, &source_key, &target_key],
            )
            .is_ok()
        {
            return Err(io::Error::other("relation scope accepted a foreign family").into());
        }
        if store
            .connection
            .execute(
                "INSERT INTO graph_relations(
                    relation_key, project_instance_id, canonical_identity, source_entity_key,
                    relation_scope, relation_kind, resolution_status, confidence, completeness
                 ) VALUES(?1, ?2, 'missing-target', ?3, 'legacy', 'calls', 'resolved',
                    'exact', 'complete')",
                params![&relation_key, &project, &source_key],
            )
            .is_ok()
        {
            return Err(io::Error::other("resolved relation without a target was accepted").into());
        }

        store.connection.execute(
            "INSERT INTO graph_relations(
                relation_key, project_instance_id, canonical_identity, source_entity_key,
                relation_scope, relation_kind, resolution_status, target_entity_key,
                confidence, completeness
             ) VALUES(?1, ?2, 'source-calls-target', ?3, 'legacy', 'calls', 'resolved', ?4,
                'exact', 'complete')",
            params![&relation_key, &project, &source_key, &target_key],
        )?;
        if store
            .connection
            .execute(
                "INSERT INTO graph_relation_occurrences(
                    relation_key, file_path, start_line, start_column, end_line, end_column
                 ) VALUES(?1, 'missing.rs', 1, 0, 1, 1)",
                [&relation_key],
            )
            .is_ok()
        {
            return Err(io::Error::other("source occurrence accepted a missing file node").into());
        }
        if store
            .connection
            .execute(
                "INSERT INTO graph_relation_occurrences(
                    relation_key, file_path, start_line, start_column, end_line, end_column
                 ) VALUES(?1, 'src/lib.rs', 9, 4, 8, 4)",
                [&relation_key],
            )
            .is_ok()
        {
            return Err(io::Error::other("reversed source occurrence was accepted").into());
        }
        store.connection.execute(
            "INSERT INTO graph_relation_occurrences(
                relation_key, file_path, start_line, start_column, end_line, end_column
             ) VALUES(?1, 'src/lib.rs', 9, 4, 9, 10)",
            [&relation_key],
        )?;

        if store
            .connection
            .execute(
                "INSERT INTO graph_coverage(
                    project_instance_id, scope_kind, state, total, covered, omitted, reason
                 ) VALUES(?1, 'project', 'partial', 1, 1, 0, 'truncated')",
                [&project],
            )
            .is_ok()
        {
            return Err(io::Error::other("contradictory coverage counts were accepted").into());
        }
        store.connection.execute(
            "INSERT INTO graph_coverage(
                project_instance_id, scope_kind, state, total, covered, omitted
             ) VALUES(?1, 'project', 'complete', 1, 1, 0)",
            [&project],
        )?;
        if store
            .connection
            .execute(
                "INSERT INTO graph_coverage(
                    project_instance_id, scope_kind, state, total, covered, omitted
                 ) VALUES(?1, 'project', 'complete', 1, 1, 0)",
                [&project],
            )
            .is_ok()
        {
            return Err(io::Error::other("duplicate coverage scope was accepted").into());
        }

        store
            .connection
            .execute("DELETE FROM nodes WHERE path = 'src/target.rs'", [])?;
        let target_rows = store.connection.query_row(
            "SELECT COUNT(*) FROM graph_entities WHERE entity_key = ?1",
            [&target_key],
            |row| row.get::<_, i64>(0),
        )?;
        let relation_rows =
            store
                .connection
                .query_row("SELECT COUNT(*) FROM graph_relations", [], |row| {
                    row.get::<_, i64>(0)
                })?;
        let occurrence_rows = store.connection.query_row(
            "SELECT COUNT(*) FROM graph_relation_occurrences",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        if target_rows != 0 || relation_rows != 0 || occurrence_rows != 0 {
            return Err(io::Error::other(
                "node deletion did not remove its dependent graph closure",
            )
            .into());
        }
        Ok(())
    }

    #[test]
    fn required_graph_index_drift_is_refused_without_mutation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let db_path = temp.path().join("projectatlas.db");
        drop(AtlasStore::open_for_project(&db_path, &root)?);
        {
            let connection = Connection::open(&db_path)?;
            connection.execute_batch(
                "DROP INDEX idx_graph_relations_target_kind;
                 PRAGMA wal_checkpoint(TRUNCATE);",
            )?;
        }
        let database_before = fs::read(&db_path)?;
        let inventory_before = directory_entry_names(temp.path())?;
        let Err(error) = AtlasStore::open_for_project(&db_path, &root) else {
            return Err(
                io::Error::other("missing graph index unexpectedly passed preflight").into(),
            );
        };
        if !matches!(error, DbError::SchemaShape { .. }) {
            return Err(io::Error::other("missing graph index returned the wrong error").into());
        }
        require_unchanged(temp.path(), &db_path, &database_before, &inventory_before)?;
        Ok(())
    }

    #[test]
    fn root_bound_open_rejects_rebind_without_mutation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("root-a");
        let other = temp.path().join("root-b");
        fs::create_dir_all(&root)?;
        fs::create_dir_all(&other)?;
        let db_path = temp.path().join("projectatlas.db");
        {
            let _store = AtlasStore::open_for_project(&db_path, &root)?;
        }
        let database_before = fs::read(&db_path)?;
        let inventory_before = directory_entry_names(temp.path())?;
        let Err(error) = AtlasStore::open_for_project(&db_path, &other) else {
            return Err(io::Error::other("wrong root unexpectedly rebound database").into());
        };
        if !matches!(error, DbError::ProjectRootMismatch { .. }) {
            return Err(io::Error::other("wrong root returned the wrong error").into());
        }
        require_unchanged(temp.path(), &db_path, &database_before, &inventory_before)?;

        let unbound_path = temp.path().join("unbound.db");
        drop(AtlasStore::open(&unbound_path)?);
        let unbound_bytes = fs::read(&unbound_path)?;
        let unbound_inventory = directory_entry_names(temp.path())?;
        let Err(unbound_error) = AtlasStore::open_for_project(&unbound_path, &root) else {
            return Err(io::Error::other("unbound existing database unexpectedly opened").into());
        };
        if !matches!(unbound_error, DbError::ProjectRootMissing) {
            return Err(io::Error::other("unbound database returned the wrong error").into());
        }
        require_unchanged(
            temp.path(),
            &unbound_path,
            &unbound_bytes,
            &unbound_inventory,
        )?;
        Ok(())
    }

    #[test]
    fn current_read_snapshot_is_write_free_for_reserved_paths() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo % # ü");
        let atlas_dir = root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let db_path = atlas_dir.join("project atlas % # ü.db");
        {
            let _store = AtlasStore::open_for_project(&db_path, &root)?;
        }
        let database_before = fs::read(&db_path)?;
        let inventory_before = directory_entry_names(&atlas_dir)?;
        let reader = AtlasStore::open_read_only_for_project(&db_path, &root)?;
        if reader.project_root()? != Some(normalize_native_path_display(&root)) {
            return Err(io::Error::other("reserved URI path changed project identity").into());
        }
        drop(reader);
        require_unchanged(&atlas_dir, &db_path, &database_before, &inventory_before)?;
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn windows_extended_drive_database_path_uses_native_sqlite_path() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let db_path = temp.path().join("projectatlas.db");
        let extended_path = PathBuf::from(format!(r"\\?\{}", db_path.display()));

        drop(AtlasStore::open_for_project(&extended_path, &root)?);
        if !db_path.exists() {
            return Err(
                io::Error::other("extended path did not create the selected database").into(),
            );
        }
        let reader = AtlasStore::open_read_only_for_project(&extended_path, &root)?;
        if reader.project_root()? != Some(normalize_native_path_display(&root)) {
            return Err(io::Error::other("extended path changed project identity").into());
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_database_path_supports_read_snapshots() -> Result<(), Box<dyn Error>> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let db_path = temp
            .path()
            .join(OsString::from_vec(b"project-atlas-\xFF.db".to_vec()));
        drop(AtlasStore::open_for_project(&db_path, &root)?);
        let database_before = fs::read(&db_path)?;
        let inventory_before = directory_entry_names(temp.path())?;
        drop(AtlasStore::open_read_only_for_project(&db_path, &root)?);
        require_unchanged(temp.path(), &db_path, &database_before, &inventory_before)?;
        Ok(())
    }

    #[test]
    fn active_wal_schema_eight_is_refused_without_migration() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let db_path = temp.path().join("projectatlas.db");
        let connection = Connection::open(&db_path)?;
        configure_writable(&connection)?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA wal_autocheckpoint = 0;",
        )?;
        create_released_schema_eight(&connection)?;
        set_metadata(
            &connection,
            PROJECT_ROOT_KEY,
            &normalize_native_path_display(&root),
        )?;
        set_metadata(&connection, INDEX_PUBLICATION_STATE_KEY, "complete")?;
        set_metadata(
            &connection,
            INDEX_PUBLICATION_FINGERPRINT_KEY,
            "active-wal-contract",
        )?;
        set_metadata(&connection, INDEX_PUBLICATION_GENERATION_KEY, "17")?;
        let wal_path = sqlite_sidecar_path(&db_path, "-wal");
        if !wal_path.exists() {
            return Err(io::Error::other("schema-8 fixture did not retain an active WAL").into());
        }
        let database_before = fs::read(&db_path)?;
        let wal_before = fs::read(&wal_path)?;

        let Err(error) = AtlasStore::open_read_only_for_project(&db_path, &root) else {
            return Err(io::Error::other("schema 8 unexpectedly opened read-only").into());
        };
        if !matches!(
            error,
            DbError::SchemaVersion {
                found: PREVIOUS_SCHEMA_VERSION,
                expected: SCHEMA_VERSION,
            }
        ) {
            return Err(io::Error::other("active-WAL refusal returned the wrong error").into());
        }
        if fs::read(&db_path)? != database_before || fs::read(&wal_path)? != wal_before {
            return Err(io::Error::other("read-only refusal changed main or WAL bytes").into());
        }
        if read_metadata(&connection, SCHEMA_VERSION_KEY)?
            != Some(PREVIOUS_SCHEMA_VERSION.to_string())
        {
            return Err(io::Error::other("read-only refusal migrated schema metadata").into());
        }
        Ok(())
    }

    #[test]
    fn late_migration_failure_rolls_back_graph_schema_and_authored_state()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir_all(&root)?;
        let db_path = temp.path().join("projectatlas.db");
        write_schema_eight_fixture(&db_path, &root)?;
        let connection = Connection::open(&db_path)?;
        configure_writable(&connection)?;
        set_metadata(
            &connection,
            SCHEMA_VERSION_KEY,
            &PUBLICATION_SCHEMA_VERSION.to_string(),
        )?;
        connection.execute_batch(
            "
            INSERT INTO nodes(id, path, kind) VALUES(1, 'src/lib.rs', 'file');
            INSERT INTO purposes(node_id, purpose, source, status)
                VALUES(1, 'Own the library.', 'agent', 'approved');
            INSERT INTO usage_events(session_id, command) VALUES('session', 'files');
            CREATE TEMP TRIGGER abort_final_schema_version
            BEFORE UPDATE OF value ON metadata
            WHEN OLD.key = 'schema_version' AND NEW.value = '10'
            BEGIN
                SELECT RAISE(ABORT, 'forced final schema-version failure');
            END;
            ",
        )?;
        let Err(error) = initialize(&connection, Some(&normalize_native_path_display(&root)))
        else {
            return Err(io::Error::other("late migration failure unexpectedly committed").into());
        };
        if !matches!(error, DbError::Sqlite(_)) {
            return Err(io::Error::other("late migration failure returned the wrong error").into());
        }
        let version = read_metadata(&connection, SCHEMA_VERSION_KEY)?;
        if version != Some(PUBLICATION_SCHEMA_VERSION.to_string()) {
            return Err(io::Error::other("failed migration changed schema version").into());
        }
        let graph_tables = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_schema
             WHERE type = 'table' AND name IN (
                'project_identity', 'graph_entities', 'graph_relations',
                'graph_relation_occurrences', 'graph_coverage'
             )",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        if graph_tables != 0 {
            return Err(io::Error::other("failed migration retained graph schema objects").into());
        }
        let publication_keys = connection.query_row(
            "SELECT COUNT(*) FROM metadata WHERE key IN (?1, ?2, ?3)",
            params![
                INDEX_PUBLICATION_STATE_KEY,
                INDEX_PUBLICATION_FINGERPRINT_KEY,
                INDEX_PUBLICATION_GENERATION_KEY,
            ],
            |row| row.get::<_, i64>(0),
        )?;
        if publication_keys != 3 {
            return Err(
                io::Error::other("failed migration exposed partial metadata deletion").into(),
            );
        }
        for table in ["nodes", "purposes", "usage_events"] {
            let rows =
                connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get::<_, i64>(0)
                })?;
            if rows != 1 {
                return Err(io::Error::other(format!(
                    "failed migration changed authored {table} rows"
                ))
                .into());
            }
        }
        if read_metadata(&connection, PROJECT_ROOT_KEY)?
            != Some(normalize_native_path_display(&root))
        {
            return Err(io::Error::other("failed migration changed project root identity").into());
        }
        Ok(())
    }

    #[test]
    fn rollback_failure_preserves_the_initiating_typed_error() -> Result<(), Box<dyn Error>> {
        let connection = Connection::open_in_memory()?;
        let error = rollback_after_error(
            &connection,
            DbError::SchemaPostcondition {
                expected: SCHEMA_VERSION,
            },
        );
        let DbError::TransactionRollback {
            operation,
            rollback,
        } = error
        else {
            return Err(io::Error::other("rollback failure did not preserve both errors").into());
        };
        if !matches!(*operation, DbError::SchemaPostcondition { .. })
            || rollback.to_string().is_empty()
        {
            return Err(io::Error::other("rollback error lost typed failure context").into());
        }
        Ok(())
    }

    #[test]
    fn concurrent_migrators_converge_without_global_coordination() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir_all(&root)?;
        let db_path = temp.path().join("projectatlas.db");
        write_schema_eight_fixture(&db_path, &root)?;
        let barrier = Arc::new(Barrier::new(2));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let barrier = Arc::clone(&barrier);
            let db_path = db_path.clone();
            let root = root.clone();
            workers.push(thread::spawn(move || {
                barrier.wait();
                AtlasStore::open_for_project(&db_path, &root).map(drop)
            }));
        }
        for worker in workers {
            worker
                .join()
                .map_err(|_panic_payload| io::Error::other("migration worker panicked"))??;
        }
        let store = AtlasStore::open_for_project(&db_path, &root)?;
        let version = read_metadata(&store.connection, SCHEMA_VERSION_KEY)?;
        if version != Some(SCHEMA_VERSION.to_string()) {
            return Err(io::Error::other("concurrent migration did not converge").into());
        }
        Ok(())
    }

    /// Write a released schema-8 fixture with publication metadata.
    fn write_schema_eight_fixture(db_path: &Path, root: &Path) -> Result<(), Box<dyn Error>> {
        let connection = Connection::open(db_path)?;
        create_released_schema_eight(&connection)?;
        set_metadata(
            &connection,
            PROJECT_ROOT_KEY,
            &normalize_native_path_display(root),
        )?;
        set_metadata(&connection, INDEX_PUBLICATION_STATE_KEY, "complete")?;
        set_metadata(
            &connection,
            INDEX_PUBLICATION_FINGERPRINT_KEY,
            "schema-eight-contract",
        )?;
        set_metadata(&connection, INDEX_PUBLICATION_GENERATION_KEY, "7")?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        Ok(())
    }

    /// Write one schema-8 database whose DDL differs in one behavior-relevant way.
    fn write_schema_lookalike(
        db_path: &Path,
        root: &Path,
        needle: &str,
        replacement: &str,
    ) -> Result<(), Box<dyn Error>> {
        let ddl = BASE_SCHEMA_SQL.replacen(needle, replacement, 1);
        if ddl == BASE_SCHEMA_SQL {
            return Err(io::Error::other("schema lookalike replacement did not match").into());
        }
        let connection = Connection::open(db_path)?;
        connection.execute_batch(&ddl)?;
        set_metadata(
            &connection,
            SCHEMA_VERSION_KEY,
            &PREVIOUS_SCHEMA_VERSION.to_string(),
        )?;
        set_metadata(
            &connection,
            PROJECT_ROOT_KEY,
            &normalize_native_path_display(root),
        )?;
        drop(connection);
        Ok(())
    }

    /// Assert both database bytes and directory contents stayed unchanged.
    fn require_unchanged(
        directory: &Path,
        db_path: &Path,
        expected_bytes: &[u8],
        expected_inventory: &[String],
    ) -> Result<(), Box<dyn Error>> {
        if fs::read(db_path)? != expected_bytes {
            return Err(io::Error::other("rejected database bytes changed").into());
        }
        let database_name = db_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(|| io::Error::other("database path has no file name"))?;
        let is_sqlite_sidecar = |name: &str| {
            name == format!("{database_name}-wal") || name == format!("{database_name}-shm")
        };
        let actual_inventory = directory_entry_names(directory)?
            .into_iter()
            .filter(|name| !is_sqlite_sidecar(name))
            .collect::<Vec<_>>();
        let filtered_expected = expected_inventory
            .iter()
            .filter(|name| !is_sqlite_sidecar(name))
            .cloned()
            .collect::<Vec<_>>();
        if actual_inventory != filtered_expected {
            return Err(io::Error::other("rejected database changed directory contents").into());
        }
        Ok(())
    }

    /// Return a deterministic directory inventory.
    fn directory_entry_names(path: &Path) -> Result<Vec<String>, Box<dyn Error>> {
        let mut names = fs::read_dir(path)?
            .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
            .collect::<Result<Vec<_>, _>>()?;
        names.sort();
        Ok(names)
    }
}
