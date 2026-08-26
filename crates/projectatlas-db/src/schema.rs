//! Own the durable `SQLite` schema, compatibility preflight, and migrations.

use crate::sqlite_profile::{
    DatabaseLocation, configure_writable_connection, inspect_database_location,
    open_read_only_connection, verify_current_read_profile,
};
use crate::{DbError, DbResult};
use projectatlas_core::CanonicalProjectRoot;
use projectatlas_core::graph::{GraphLimitKind, ProjectInstanceId};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[cfg(test)]
use projectatlas_core::normalize_native_path_display;

/// Current `SQLite` schema version supported by this crate.
pub(crate) const SCHEMA_VERSION: i64 = 20;
/// Released 0.3.26 schema accepted by the migration inventory.
pub(crate) const PREVIOUS_SCHEMA_VERSION: i64 = 8;
/// First internal schema with explicit publication invalidation.
const PUBLICATION_SCHEMA_VERSION: i64 = 9;
/// First schema with normalized repository-graph storage.
const GRAPH_SCHEMA_VERSION: i64 = 10;
/// First schema with bounded telemetry retention and aggregation.
const TELEMETRY_SCHEMA_VERSION: i64 = 11;
/// First schema with canonical entity and relation resolution keys.
const RESOLUTION_SCHEMA_VERSION: i64 = 12;
/// First schema with separate source-parser and fact-provider provenance.
const PARSER_PROVENANCE_SCHEMA_VERSION: i64 = 13;
/// First schema with bounded coverage-discovery access paths.
pub(crate) const COVERAGE_DISCOVERY_SCHEMA_VERSION: i64 = 14;
/// First schema with the rebuildable lexical candidate accelerator.
const LEXICAL_SCHEMA_VERSION: i64 = 15;
/// Released schema with compact normalized graph keys.
const COMPACT_GRAPH_SCHEMA_VERSION: i64 = 16;
/// First schema with classified documentation graph storage.
const CLASSIFIED_GRAPH_SCHEMA_VERSION: i64 = 17;
/// First schema with local worktree registration and aggregate telemetry control state.
const WORKTREE_CONTROL_SCHEMA_VERSION: i64 = 18;
/// Schema immediately before the canonical native-root identity migration.
pub(crate) const CANONICAL_ROOT_PREDECESSOR_SCHEMA_VERSION: i64 = 19;
/// First schema with a lossless native project-root identity.
const CANONICAL_ROOT_SCHEMA_VERSION: i64 = 20;
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
/// Metadata key for the latest authoritative persisted-text revision.
pub(crate) const FILE_TEXT_FTS_SOURCE_REVISION_KEY: &str = "file_text_fts_source_revision";
/// Metadata key for the latest transactionally synchronized FTS revision.
pub(crate) const FILE_TEXT_FTS_PROJECTION_REVISION_KEY: &str = "file_text_fts_projection_revision";
/// Maximum bytes retained for a schema object label in an incompatibility error.
const SCHEMA_DIAGNOSTIC_OBJECT_MAX_BYTES: usize = 256;
/// Maximum bytes retained for each schema contract value in an incompatibility error.
const SCHEMA_DIAGNOSTIC_VALUE_MAX_BYTES: usize = 2 * 1024;
/// Marker appended when a schema diagnostic exceeds its field bound.
const SCHEMA_DIAGNOSTIC_TRUNCATION_SUFFIX: &str = "...";

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
    /// Existing durable schema version, when a `ProjectAtlas` schema exists.
    pub(crate) schema_version: Option<i64>,
    /// Existing normalized project identity, when present.
    pub(crate) project_root: Option<String>,
    /// Existing project instance identity, when the predecessor schema owns
    /// the singleton. This is captured in the same read-only snapshot so
    /// transition reporting never infers it from migrated state.
    pub(crate) project_instance_id: Option<ProjectInstanceId>,
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
        to: GRAPH_SCHEMA_VERSION,
        apply: migrate_9_to_10,
    },
    Migration {
        from: GRAPH_SCHEMA_VERSION,
        to: TELEMETRY_SCHEMA_VERSION,
        apply: migrate_10_to_11,
    },
    Migration {
        from: TELEMETRY_SCHEMA_VERSION,
        to: RESOLUTION_SCHEMA_VERSION,
        apply: migrate_11_to_12,
    },
    Migration {
        from: RESOLUTION_SCHEMA_VERSION,
        to: PARSER_PROVENANCE_SCHEMA_VERSION,
        apply: migrate_12_to_13,
    },
    Migration {
        from: PARSER_PROVENANCE_SCHEMA_VERSION,
        to: COVERAGE_DISCOVERY_SCHEMA_VERSION,
        apply: migrate_13_to_14,
    },
    Migration {
        from: COVERAGE_DISCOVERY_SCHEMA_VERSION,
        to: LEXICAL_SCHEMA_VERSION,
        apply: migrate_14_to_15,
    },
    Migration {
        from: LEXICAL_SCHEMA_VERSION,
        to: COMPACT_GRAPH_SCHEMA_VERSION,
        apply: migrate_15_to_16,
    },
    Migration {
        from: COMPACT_GRAPH_SCHEMA_VERSION,
        to: CLASSIFIED_GRAPH_SCHEMA_VERSION,
        apply: migrate_16_to_17,
    },
    Migration {
        from: CLASSIFIED_GRAPH_SCHEMA_VERSION,
        to: WORKTREE_CONTROL_SCHEMA_VERSION,
        apply: migrate_17_to_18,
    },
    Migration {
        from: WORKTREE_CONTROL_SCHEMA_VERSION,
        to: 19,
        apply: migrate_18_to_19,
    },
    Migration {
        from: CANONICAL_ROOT_PREDECESSOR_SCHEMA_VERSION,
        to: CANONICAL_ROOT_SCHEMA_VERSION,
        apply: migrate_19_to_20,
    },
];

/// Return the bounded migration distance from one admitted schema to current.
pub(crate) fn migration_steps_remaining(from: i64) -> Option<u32> {
    if from == SCHEMA_VERSION {
        return Some(0);
    }
    let mut version = from;
    let mut steps = 0_u32;
    for migration in MIGRATIONS {
        if migration.from == version {
            version = migration.to;
            steps = steps.checked_add(1)?;
        }
    }
    (version == SCHEMA_VERSION).then_some(steps)
}

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
/// Process-local immutable schema-10 contract derived from graph DDL.
static GRAPH_PREDECESSOR_SCHEMA_CONTRACT: OnceLock<SchemaContract> = OnceLock::new();
/// Process-local immutable schema-11 contract derived from graph and telemetry DDL.
static TELEMETRY_PREDECESSOR_SCHEMA_CONTRACT: OnceLock<SchemaContract> = OnceLock::new();
/// Process-local immutable schema-12 contract before parser provenance separation.
static RESOLUTION_PREDECESSOR_SCHEMA_CONTRACT: OnceLock<SchemaContract> = OnceLock::new();
/// Process-local immutable schema-13 contract before coverage-discovery indexes.
static PARSER_PROVENANCE_PREDECESSOR_SCHEMA_CONTRACT: OnceLock<SchemaContract> = OnceLock::new();
/// Process-local immutable schema-14 contract before lexical acceleration.
static COVERAGE_DISCOVERY_PREDECESSOR_SCHEMA_CONTRACT: OnceLock<SchemaContract> = OnceLock::new();
/// Immutable schema-16 contract admitted before classified-document storage.
static COMPACT_GRAPH_PREDECESSOR_SCHEMA_CONTRACT: OnceLock<SchemaContract> = OnceLock::new();
/// Immutable expected schema-17 contract before worktree control storage.
static CLASSIFIED_GRAPH_PREDECESSOR_SCHEMA_CONTRACT: OnceLock<SchemaContract> = OnceLock::new();
/// Immutable expected schema-18 contract before complete graph-limit admission.
static WORKTREE_CONTROL_PREDECESSOR_SCHEMA_CONTRACT: OnceLock<SchemaContract> = OnceLock::new();
/// Immutable expected schema-19 contract before native-root identity storage.
static CANONICAL_ROOT_PREDECESSOR_SCHEMA_CONTRACT: OnceLock<SchemaContract> = OnceLock::new();

/// Immutable physical schema emitted by the released 0.3.26 runtime.
#[cfg(test)]
const RELEASED_SCHEMA_EIGHT_SQL: &str = include_str!("../tests/fixtures/released-schema-8.sql");
/// Physical schema produced by the released v0.3.11-to-v0.3.26 upgrade path.
#[cfg(test)]
const EVOLVED_RELEASED_SCHEMA_EIGHT_SQL: &str =
    include_str!("../tests/fixtures/released-schema-8-evolved.sql");
/// BLAKE3 of the complete schema-18 contract captured before schema 19 existed.
#[cfg(test)]
const RELEASED_SCHEMA_EIGHTEEN_CONTRACT_BLAKE3: &str =
    "5fbdebf57bae7e3320d000e6c419390380f266d6a426cf0ea236a2728e057673";

/// Create the historical released schema without consulting current DDL.
#[cfg(test)]
pub(crate) fn create_released_schema_eight(connection: &Connection) -> DbResult<()> {
    create_schema_eight_fixture(connection, RELEASED_SCHEMA_EIGHT_SQL)
}

/// Create the schema-8 layout evolved through released v0.3 migrations.
#[cfg(test)]
pub(crate) fn create_evolved_released_schema_eight(connection: &Connection) -> DbResult<()> {
    create_schema_eight_fixture(connection, EVOLVED_RELEASED_SCHEMA_EIGHT_SQL)
}

/// Create one captured released schema-8 fixture.
#[cfg(test)]
fn create_schema_eight_fixture(connection: &Connection, schema: &str) -> DbResult<()> {
    connection.execute_batch(schema)?;
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
    CREATE INDEX idx_symbols_path ON symbols(path);
    CREATE INDEX idx_symbols_name ON symbols(name);
    CREATE INDEX idx_symbols_kind ON symbols(kind);
    CREATE INDEX idx_source_parse_metadata_parser ON source_parse_metadata(parser);
    CREATE INDEX idx_symbol_relations_path ON symbol_relations(path);
    CREATE INDEX idx_symbol_relations_target ON symbol_relations(target_name);
    CREATE INDEX idx_health_resolutions_category ON health_resolutions(category);
    CREATE INDEX idx_file_texts_hash ON file_texts(content_hash);
";

/// Rebuild symbol storage with one optional exact parser-supplied source selector.
const SYMBOL_SOURCE_SELECTOR_SCHEMA_SQL: &str = "
    CREATE TABLE symbols_with_source_selectors (
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
        source_byte_start INTEGER,
        source_byte_end INTEGER,
        source_column_start INTEGER,
        source_column_end INTEGER,
        parent TEXT,
        parser TEXT NOT NULL,
        detail TEXT,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        CHECK (
            (
                source_byte_start IS NULL
                AND source_byte_end IS NULL
                AND source_column_start IS NULL
                AND source_column_end IS NULL
            ) OR (
                typeof(source_byte_start) = 'integer'
                AND typeof(source_byte_end) = 'integer'
                AND typeof(source_column_start) = 'integer'
                AND typeof(source_column_end) = 'integer'
                AND source_byte_start >= 0
                AND source_byte_end >= source_byte_start
                AND source_column_start >= 0
                AND source_column_end >= 0
                AND line_start >= 1
                AND line_end >= line_start
                AND (
                    line_end > line_start
                    OR source_column_end >= source_column_start
                )
            )
        )
    );

    INSERT INTO symbols_with_source_selectors(
        id, path, language, name, kind, signature, exported, documentation,
        line_start, line_end, parent, parser, detail, created_at, updated_at
    )
    SELECT
        id, path, language, name, kind, signature, exported, documentation,
        line_start, line_end, parent, parser, detail, created_at, updated_at
    FROM symbols;

    DROP TABLE symbols;
    ALTER TABLE symbols_with_source_selectors RENAME TO symbols;
    CREATE INDEX idx_symbols_path ON symbols(path);
    CREATE INDEX idx_symbols_name ON symbols(name);
    CREATE INDEX idx_symbols_kind ON symbols(kind);
";

/// Trigger-free FTS5 acceleration over authoritative persisted file text.
const FILE_TEXT_FTS_SCHEMA_SQL: &str = "
    CREATE VIRTUAL TABLE file_text_fts USING fts5(
        content,
        content='file_texts',
        content_rowid='rowid',
        tokenize='trigram case_sensitive 0'
    );
";

/// Covering access path for bounded import-alias discovery.
const SYMBOL_RELATION_LOOKUP_INDEX_NAME: &str = "idx_symbol_import_alias_lookup";
/// DDL for the bounded import-alias discovery access path.
const SYMBOL_RELATION_LOOKUP_SCHEMA_SQL: &str = "
    CREATE INDEX IF NOT EXISTS idx_symbol_import_alias_lookup
        ON symbol_relations(kind, path, line, source_name, target_name);
";

/// Separate file-level parse provenance from the parser provenance of derived facts.
const SOURCE_PARSE_PROVENANCE_SCHEMA_SQL: &str = "
    ALTER TABLE source_parse_metadata RENAME TO source_parse_metadata_legacy;

    CREATE TABLE source_parse_metadata (
        path TEXT PRIMARY KEY,
        language TEXT,
        source_parser TEXT NOT NULL,
        fact_parser TEXT NOT NULL,
        symbol_count INTEGER NOT NULL,
        relation_count INTEGER NOT NULL,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    INSERT INTO source_parse_metadata(
        path,
        language,
        source_parser,
        fact_parser,
        symbol_count,
        relation_count,
        updated_at
    )
    SELECT
        path,
        language,
        parser,
        parser,
        symbol_count,
        relation_count,
        updated_at
    FROM source_parse_metadata_legacy;

    DROP TABLE source_parse_metadata_legacy;

";

/// Usage-event layout owned by released schemas 8 through 10.
const LEGACY_USAGE_SCHEMA_SQL: &str = "
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
    CREATE INDEX idx_usage_session ON usage_events(session_id);
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
        canonical_identity TEXT NOT NULL
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
            'module', 'type', 'value', 'import', 'package', 'workspace', 'dependency'__DOCUMENT_HEADING_SYMBOL_KIND__, 'unknown'
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
        canonical_identity TEXT NOT NULL
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
        __DOCUMENT_UNRESOLVED_REASON_COLUMN__
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
                AND relation_kind IN ('references', 'tests', 'routes-to', 'configures', 'deploys', 'reads', 'writes'__DOCUMENT_RELATION_KIND__))
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
        __DOCUMENT_UNRESOLVED_REASON_CONSTRAINT__
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
            CHECK(reached_limit IS NULL OR reached_limit IN (__GRAPH_LIMIT_KINDS__)),
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
                AND relation_kind IN ('references', 'tests', 'routes-to', 'configures', 'deploys', 'reads', 'writes'__DOCUMENT_RELATION_KIND__))
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
        ON graph_entities(repository_path, entity_kind, entity_key);
    CREATE INDEX idx_graph_entities_package
        ON graph_entities(package_manager, package_name, manifest_path, entity_key);
    CREATE INDEX idx_graph_entities_manifest_path
        ON graph_entities(manifest_path, entity_key);
    CREATE INDEX idx_graph_entities_symbol
        ON graph_entities(repository_path, symbol_name, symbol_kind, symbol_parent, symbol_signature, entity_key);
    CREATE INDEX idx_graph_entities_external
        ON graph_entities(external_system, external_identity, entity_key);
    CREATE INDEX idx_graph_relations_source_kind
        ON graph_relations(source_entity_key, relation_scope, relation_kind, relation_key);
    CREATE INDEX idx_graph_relations_target_kind
        ON graph_relations(target_entity_key, relation_scope, relation_kind, relation_key);
    CREATE INDEX idx_graph_relations_kind_order
        ON graph_relations(relation_scope, relation_kind, relation_key);
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

/// Exact reached-limit domain emitted by schemas 10 through 18.
const HISTORICAL_GRAPH_LIMIT_KINDS_SQL: &str = "'rows', 'occurrences', 'depth', 'output_bytes'";

/// Closed graph DDL shapes retained for supported historical contracts.
#[derive(Clone, Copy)]
enum GraphSchemaShape {
    /// Compact graph storage without classified-document additions.
    Compact,
    /// Classified-document storage with the historical reached-limit domain.
    ClassifiedDocuments,
    /// Current classified-document storage with every closed reached-limit kind.
    Current,
}

impl GraphSchemaShape {
    /// Return whether this shape includes classified-document graph contracts.
    const fn includes_classified_documents(self) -> bool {
        matches!(self, Self::ClassifiedDocuments | Self::Current)
    }
}

/// Persist one closed content role for every currently admitted file node.
const FILE_CONTENT_CLASSIFICATION_SCHEMA_SQL: &str = "
    CREATE UNIQUE INDEX IF NOT EXISTS idx_nodes_path_kind ON nodes(path, kind);
    CREATE TABLE file_content_classifications (
        path TEXT PRIMARY KEY NOT NULL
            CHECK(typeof(path) = 'text' AND length(path) > 0),
        node_kind TEXT NOT NULL DEFAULT 'file' CHECK(node_kind = 'file'),
        classification TEXT NOT NULL CHECK(classification IN (
            'source', 'documentation', 'configuration_data', 'other_text', 'opaque'
        )),
        FOREIGN KEY(path, node_kind) REFERENCES nodes(path, kind) ON DELETE CASCADE
    );
    CREATE INDEX idx_file_content_classifications_classification_path
        ON file_content_classifications(classification, path);
";

/// Durable local worktree registrations and bounded aggregate telemetry snapshots.
const WORKTREE_CONTROL_SCHEMA_SQL: &str = "
    CREATE TABLE usage_aggregate_revisions (
        project_instance_id BLOB PRIMARY KEY NOT NULL
            CHECK(typeof(project_instance_id) = 'blob' AND length(project_instance_id) = 16),
        revision INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(revision) = 'integer' AND revision >= 0)
    ) STRICT, WITHOUT ROWID;

    CREATE TABLE worktree_registrations (
        registration_id INTEGER PRIMARY KEY,
        alias TEXT NOT NULL
            CHECK(
                typeof(alias) = 'text'
                AND length(alias) BETWEEN 1 AND 64
                AND alias <> 'main'
                AND alias GLOB '[a-z0-9]*'
                AND alias NOT GLOB '*[^a-z0-9._-]*'
            ),
        state TEXT NOT NULL DEFAULT 'active'
            CHECK(state IN ('active', 'retired')),
        git_common_directory TEXT NOT NULL
            CHECK(typeof(git_common_directory) = 'text' AND length(git_common_directory) BETWEEN 1 AND 131072),
        git_administrative_directory TEXT NOT NULL
            CHECK(typeof(git_administrative_directory) = 'text' AND length(git_administrative_directory) BETWEEN 1 AND 131072),
        git_administrative_identity TEXT NOT NULL
            CHECK(
                typeof(git_administrative_identity) = 'text'
                AND length(git_administrative_identity) = 64
                AND git_administrative_identity NOT GLOB '*[^0-9a-f]*'
            ),
        last_root TEXT NOT NULL
            CHECK(typeof(last_root) = 'text' AND length(last_root) BETWEEN 1 AND 131072),
        project_instance_id BLOB
            CHECK(project_instance_id IS NULL OR (
                typeof(project_instance_id) = 'blob' AND length(project_instance_id) = 16
            )),
        accepted_telemetry_revision INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(accepted_telemetry_revision) = 'integer' AND accepted_telemetry_revision >= 0),
        created_at_epoch INTEGER NOT NULL
            CHECK(typeof(created_at_epoch) = 'integer' AND created_at_epoch >= 0),
        retired_at_epoch INTEGER
            CHECK(retired_at_epoch IS NULL OR (
                typeof(retired_at_epoch) = 'integer' AND retired_at_epoch >= created_at_epoch
            )),
        CHECK(
            (state = 'active' AND retired_at_epoch IS NULL)
            OR (state = 'retired' AND retired_at_epoch IS NOT NULL)
        )
    ) STRICT;

    CREATE UNIQUE INDEX idx_worktree_registrations_active_alias
        ON worktree_registrations(alias) WHERE state = 'active';
    CREATE UNIQUE INDEX idx_worktree_registrations_active_administrative_directory
        ON worktree_registrations(git_administrative_directory) WHERE state = 'active';
    CREATE UNIQUE INDEX idx_worktree_registrations_active_administrative_identity
        ON worktree_registrations(git_administrative_identity) WHERE state = 'active';
    CREATE UNIQUE INDEX idx_worktree_registrations_active_project
        ON worktree_registrations(project_instance_id)
        WHERE state = 'active' AND project_instance_id IS NOT NULL;
    CREATE INDEX idx_worktree_registrations_state_alias
        ON worktree_registrations(state, alias, registration_id);

    CREATE TABLE worktree_usage_aggregates (
        registration_id INTEGER NOT NULL
            REFERENCES worktree_registrations(registration_id) ON DELETE RESTRICT,
        source_kind TEXT NOT NULL
            CHECK(source_kind IN ('routed', 'synchronized')),
        day_epoch INTEGER NOT NULL DEFAULT -1
            CHECK(typeof(day_epoch) = 'integer' AND day_epoch >= -1),
        dimension_id INTEGER NOT NULL
            REFERENCES usage_bucket_dimensions(dimension_id) ON DELETE RESTRICT,
        calls INTEGER NOT NULL DEFAULT 0 CHECK(calls >= 0),
        estimated_without INTEGER NOT NULL DEFAULT 0 CHECK(estimated_without >= 0),
        estimated_with INTEGER NOT NULL DEFAULT 0 CHECK(estimated_with >= 0),
        observed_without INTEGER NOT NULL DEFAULT 0 CHECK(observed_without >= 0),
        observed_with INTEGER NOT NULL DEFAULT 0 CHECK(observed_with >= 0),
        modeled_without INTEGER NOT NULL DEFAULT 0 CHECK(modeled_without >= 0),
        modeled_with INTEGER NOT NULL DEFAULT 0 CHECK(modeled_with >= 0),
        deduped_modeled_without INTEGER NOT NULL DEFAULT 0 CHECK(deduped_modeled_without >= 0),
        deduped_modeled_with INTEGER NOT NULL DEFAULT 0 CHECK(deduped_modeled_with >= 0),
        repeated_baselines INTEGER NOT NULL DEFAULT 0 CHECK(repeated_baselines >= 0),
        observed_file_read_replacements INTEGER NOT NULL DEFAULT 0
            CHECK(observed_file_read_replacements >= 0),
        modeled_file_reads_avoided INTEGER NOT NULL DEFAULT 0
            CHECK(modeled_file_reads_avoided >= 0),
        PRIMARY KEY(registration_id, source_kind, day_epoch, dimension_id)
    ) STRICT, WITHOUT ROWID;
    CREATE INDEX idx_worktree_usage_aggregates_day_registration
        ON worktree_usage_aggregates(day_epoch, registration_id, source_kind, dimension_id);

    CREATE TABLE usage_instance_worktree_origins (
        instance_row_id INTEGER PRIMARY KEY
            REFERENCES usage_instances(instance_row_id) ON DELETE CASCADE,
        registration_id INTEGER NOT NULL
            REFERENCES worktree_registrations(registration_id) ON DELETE RESTRICT
    ) STRICT;
    CREATE INDEX idx_usage_instance_worktree_origins_registration
        ON usage_instance_worktree_origins(registration_id, instance_row_id);
";

/// Lossless native identity for the owning project root.
const PROJECT_ROOT_IDENTITY_SCHEMA_SQL: &str = "
    CREATE TABLE project_root_identity (
        singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
        codec_version INTEGER NOT NULL CHECK(codec_version = 1),
        root BLOB NOT NULL
            CHECK(typeof(root) = 'blob' AND length(root) >= 3)
    ) STRICT;
";

/// Produce one exact historical or current graph contract from one DDL authority.
fn graph_schema_sql(shape: GraphSchemaShape) -> String {
    let heading = if shape.includes_classified_documents() {
        ", 'heading'"
    } else {
        ""
    };
    let documents = if shape.includes_classified_documents() {
        ", 'documents'"
    } else {
        ""
    };
    let reason_column = if shape.includes_classified_documents() {
        "document_unresolved_reason TEXT CHECK(document_unresolved_reason IS NULL OR document_unresolved_reason IN ('missing', 'ignored', 'outside_root', 'case_conflict', 'unsupported', 'no_static_target')),"
    } else {
        ""
    };
    let reason_constraint = if shape.includes_classified_documents() {
        ", CHECK(document_unresolved_reason IS NULL OR (relation_scope = 'extended' AND relation_kind = 'documents' AND resolution_status = 'unresolved'))"
    } else {
        ""
    };
    let reached_limit_kinds = match shape {
        GraphSchemaShape::Compact | GraphSchemaShape::ClassifiedDocuments => {
            HISTORICAL_GRAPH_LIMIT_KINDS_SQL.to_string()
        }
        GraphSchemaShape::Current => GraphLimitKind::ALL
            .iter()
            .map(|kind| format!("'{}'", kind.as_str()))
            .collect::<Vec<_>>()
            .join(", "),
    };
    GRAPH_SCHEMA_SQL
        .replace("__DOCUMENT_HEADING_SYMBOL_KIND__", heading)
        .replace("__DOCUMENT_RELATION_KIND__", documents)
        .replace("__DOCUMENT_UNRESOLVED_REASON_COLUMN__", reason_column)
        .replace(
            "__DOCUMENT_UNRESOLVED_REASON_CONSTRAINT__",
            reason_constraint,
        )
        .replace("__GRAPH_LIMIT_KINDS__", &reached_limit_kinds)
}

/// Create one historical or current normalized graph schema.
fn create_graph_schema(connection: &Connection, shape: GraphSchemaShape) -> DbResult<()> {
    let sql = graph_schema_sql(shape);
    connection.execute_batch(&sql)?;
    Ok(())
}

/// Parser lookup indexes owned by bounded project-wide coverage discovery.
const COVERAGE_DISCOVERY_SOURCE_SCHEMA_SQL: &str = "
    CREATE INDEX idx_source_parse_metadata_source_parser_path
        ON source_parse_metadata(source_parser, path);
    CREATE INDEX idx_source_parse_metadata_fact_parser_path
        ON source_parse_metadata(fact_parser, path);
";

/// Graph lookup indexes owned by bounded project-wide coverage discovery.
const COVERAGE_DISCOVERY_GRAPH_SCHEMA_SQL: &str = "
    CREATE INDEX idx_graph_coverage_discovery_state
        ON graph_coverage(state, scope_path, id);
    CREATE INDEX idx_graph_coverage_discovery_reason
        ON graph_coverage(reason, scope_path, id);
";

/// Canonical resolution identities and their graph-owner bindings.
const RESOLUTION_KEY_SCHEMA_SQL: &str = "
    CREATE UNIQUE INDEX idx_graph_relations_project_key
        ON graph_relations(project_instance_id, relation_key);

    CREATE TABLE graph_resolution_keys (
        project_instance_id BLOB NOT NULL
            CHECK(typeof(project_instance_id) = 'blob' AND length(project_instance_id) = 16),
        resolution_domain TEXT NOT NULL
            CHECK(resolution_domain IN ('declaration', 'module', 'package')),
        key_digest BLOB NOT NULL
            CHECK(typeof(key_digest) = 'blob' AND length(key_digest) = 32),
        canonical_identity TEXT NOT NULL
            CHECK(typeof(canonical_identity) = 'text' AND length(canonical_identity) > 0),
        PRIMARY KEY(project_instance_id, resolution_domain, key_digest),
        FOREIGN KEY(project_instance_id)
            REFERENCES project_identity(project_instance_id) ON DELETE RESTRICT
    ) STRICT;

    CREATE TABLE graph_entity_exports (
        project_instance_id BLOB NOT NULL
            CHECK(typeof(project_instance_id) = 'blob' AND length(project_instance_id) = 16),
        entity_key BLOB NOT NULL
            CHECK(typeof(entity_key) = 'blob' AND length(entity_key) = 32),
        owner_path TEXT NOT NULL
            CHECK(typeof(owner_path) = 'text' AND length(owner_path) > 0),
        resolution_domain TEXT NOT NULL
            CHECK(resolution_domain IN ('declaration', 'module', 'package')),
        key_digest BLOB NOT NULL
            CHECK(typeof(key_digest) = 'blob' AND length(key_digest) = 32),
        PRIMARY KEY(project_instance_id, entity_key, resolution_domain, key_digest),
        FOREIGN KEY(project_instance_id, entity_key)
            REFERENCES graph_entities(project_instance_id, entity_key) ON DELETE CASCADE,
        FOREIGN KEY(project_instance_id, resolution_domain, key_digest)
            REFERENCES graph_resolution_keys(project_instance_id, resolution_domain, key_digest)
                ON DELETE CASCADE,
        FOREIGN KEY(owner_path) REFERENCES nodes(path) ON DELETE CASCADE
    ) STRICT;

    CREATE TABLE graph_relation_dependencies (
        project_instance_id BLOB NOT NULL
            CHECK(typeof(project_instance_id) = 'blob' AND length(project_instance_id) = 16),
        relation_key BLOB NOT NULL
            CHECK(typeof(relation_key) = 'blob' AND length(relation_key) = 32),
        owner_path TEXT NOT NULL
            CHECK(typeof(owner_path) = 'text' AND length(owner_path) > 0),
        resolution_domain TEXT NOT NULL
            CHECK(resolution_domain IN ('declaration', 'module', 'package')),
        key_digest BLOB NOT NULL
            CHECK(typeof(key_digest) = 'blob' AND length(key_digest) = 32),
        PRIMARY KEY(project_instance_id, relation_key, resolution_domain, key_digest),
        FOREIGN KEY(project_instance_id, relation_key)
            REFERENCES graph_relations(project_instance_id, relation_key) ON DELETE CASCADE,
        FOREIGN KEY(project_instance_id, resolution_domain, key_digest)
            REFERENCES graph_resolution_keys(project_instance_id, resolution_domain, key_digest)
                ON DELETE CASCADE,
        FOREIGN KEY(owner_path) REFERENCES nodes(path) ON DELETE CASCADE
    ) STRICT;

    CREATE INDEX idx_graph_entity_exports_key
        ON graph_entity_exports(
            project_instance_id, resolution_domain, key_digest, entity_key
        );
    CREATE INDEX idx_graph_entity_exports_owner
        ON graph_entity_exports(
            project_instance_id, owner_path, resolution_domain, key_digest, entity_key
        );
    CREATE INDEX idx_graph_relation_dependencies_key
        ON graph_relation_dependencies(
            project_instance_id, resolution_domain, key_digest, owner_path, relation_key
        );
    CREATE INDEX idx_graph_relation_dependencies_owner
        ON graph_relation_dependencies(
            project_instance_id, owner_path, resolution_domain, key_digest, relation_key
        );
";

/// Bounded telemetry state shared by fresh databases and the 10-to-11 migration.
const TELEMETRY_STORAGE_SCHEMA_SQL: &str = "
    CREATE TABLE usage_instances (
        instance_row_id INTEGER PRIMARY KEY,
        project_instance_id BLOB NOT NULL CHECK(length(project_instance_id) = 16),
        runtime_instance_id BLOB NOT NULL
            CHECK(
                length(runtime_instance_id) = 16
                AND runtime_instance_id <> X'00000000000000000000000000000000'
            ),
        owner TEXT NOT NULL
            CHECK(owner IN ('cli_invocation', 'mcp_process', 'library_handle', 'migrated_legacy')),
        caller_label TEXT
            CHECK(caller_label IS NULL OR (typeof(caller_label) = 'text' AND length(caller_label) > 0)),
        state TEXT NOT NULL DEFAULT 'active'
            CHECK(state IN ('active', 'sealed', 'expired')),
        started_at_epoch INTEGER NOT NULL
            CHECK(typeof(started_at_epoch) = 'integer' AND started_at_epoch >= 0),
        last_seen_at_epoch INTEGER NOT NULL
            CHECK(typeof(last_seen_at_epoch) = 'integer' AND last_seen_at_epoch >= started_at_epoch),
        sealed_at_epoch INTEGER
            CHECK(sealed_at_epoch IS NULL OR (typeof(sealed_at_epoch) = 'integer' AND sealed_at_epoch >= started_at_epoch)),
        raw_detail_complete INTEGER NOT NULL DEFAULT 1
            CHECK(raw_detail_complete IN (0, 1)),
        clock_anomaly INTEGER NOT NULL DEFAULT 0
            CHECK(clock_anomaly IN (0, 1)),
        CHECK(
            (state = 'active' AND sealed_at_epoch IS NULL)
            OR (state IN ('sealed', 'expired') AND sealed_at_epoch IS NOT NULL)
        ),
        UNIQUE(project_instance_id, runtime_instance_id)
    ) STRICT;

    CREATE TABLE usage_bucket_dimensions (
        dimension_id INTEGER PRIMARY KEY,
        token_savings_bucket TEXT NOT NULL,
        provider TEXT NOT NULL,
        model TEXT NOT NULL,
        tokenizer_backend TEXT NOT NULL,
        accuracy TEXT NOT NULL,
        baseline_kind TEXT NOT NULL,
        confidence TEXT NOT NULL,
        accounting_layer TEXT NOT NULL,
        estimate_method TEXT NOT NULL,
        denominator_kind TEXT NOT NULL,
        dedupe_scope TEXT NOT NULL,
        overflow INTEGER NOT NULL DEFAULT 0 CHECK(overflow IN (0, 1)),
        UNIQUE(
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
            overflow
        )
    ) STRICT;

    CREATE TABLE usage_instance_baselines (
        instance_row_id INTEGER NOT NULL
            REFERENCES usage_instances(instance_row_id) ON DELETE CASCADE,
        baseline_key BLOB NOT NULL
            CHECK(length(baseline_key) = 32),
        baseline_identity TEXT NOT NULL,
        baseline_fingerprint TEXT NOT NULL,
        denominator_kind TEXT NOT NULL,
        maximum_without INTEGER NOT NULL
            CHECK(typeof(maximum_without) = 'integer' AND maximum_without >= 0),
        emitted_with INTEGER NOT NULL
            CHECK(typeof(emitted_with) = 'integer' AND emitted_with >= 0),
        calls INTEGER NOT NULL
            CHECK(typeof(calls) = 'integer' AND calls > 0),
        witness_logical_bytes INTEGER NOT NULL
            CHECK(typeof(witness_logical_bytes) = 'integer' AND witness_logical_bytes >= 0),
        PRIMARY KEY(instance_row_id, baseline_key)
    ) STRICT, WITHOUT ROWID;

    CREATE TABLE usage_labels (
        project_instance_id BLOB NOT NULL CHECK(length(project_instance_id) = 16),
        caller_label TEXT NOT NULL CHECK(length(caller_label) > 0),
        last_seen_at_epoch INTEGER NOT NULL CHECK(last_seen_at_epoch >= 0),
        detail_complete INTEGER NOT NULL DEFAULT 1 CHECK(detail_complete IN (0, 1)),
        PRIMARY KEY(project_instance_id, caller_label)
    ) STRICT, WITHOUT ROWID;

    CREATE TABLE usage_global_aggregates (
        project_instance_id BLOB NOT NULL CHECK(length(project_instance_id) = 16),
        dimension_id INTEGER NOT NULL
            REFERENCES usage_bucket_dimensions(dimension_id) ON DELETE RESTRICT,
        calls INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(calls) = 'integer' AND calls >= 0),
        estimated_without INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(estimated_without) = 'integer' AND estimated_without >= 0),
        estimated_with INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(estimated_with) = 'integer' AND estimated_with >= 0),
        observed_without INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(observed_without) = 'integer' AND observed_without >= 0),
        observed_with INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(observed_with) = 'integer' AND observed_with >= 0),
        modeled_without INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(modeled_without) = 'integer' AND modeled_without >= 0),
        modeled_with INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(modeled_with) = 'integer' AND modeled_with >= 0),
        deduped_modeled_without INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(deduped_modeled_without) = 'integer' AND deduped_modeled_without >= 0),
        deduped_modeled_with INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(deduped_modeled_with) = 'integer' AND deduped_modeled_with >= 0),
        repeated_baselines INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(repeated_baselines) = 'integer' AND repeated_baselines >= 0),
        observed_file_read_replacements INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(observed_file_read_replacements) = 'integer' AND observed_file_read_replacements >= 0),
        modeled_file_reads_avoided INTEGER NOT NULL DEFAULT 0
            CHECK(typeof(modeled_file_reads_avoided) = 'integer' AND modeled_file_reads_avoided >= 0),
        PRIMARY KEY(project_instance_id, dimension_id)
    ) STRICT, WITHOUT ROWID;

    CREATE TABLE usage_instance_aggregates (
        instance_row_id INTEGER NOT NULL
            REFERENCES usage_instances(instance_row_id) ON DELETE CASCADE,
        dimension_id INTEGER NOT NULL
            REFERENCES usage_bucket_dimensions(dimension_id) ON DELETE RESTRICT,
        calls INTEGER NOT NULL DEFAULT 0 CHECK(calls >= 0),
        estimated_without INTEGER NOT NULL DEFAULT 0 CHECK(estimated_without >= 0),
        estimated_with INTEGER NOT NULL DEFAULT 0 CHECK(estimated_with >= 0),
        observed_without INTEGER NOT NULL DEFAULT 0 CHECK(observed_without >= 0),
        observed_with INTEGER NOT NULL DEFAULT 0 CHECK(observed_with >= 0),
        modeled_without INTEGER NOT NULL DEFAULT 0 CHECK(modeled_without >= 0),
        modeled_with INTEGER NOT NULL DEFAULT 0 CHECK(modeled_with >= 0),
        deduped_modeled_without INTEGER NOT NULL DEFAULT 0 CHECK(deduped_modeled_without >= 0),
        deduped_modeled_with INTEGER NOT NULL DEFAULT 0 CHECK(deduped_modeled_with >= 0),
        repeated_baselines INTEGER NOT NULL DEFAULT 0 CHECK(repeated_baselines >= 0),
        observed_file_read_replacements INTEGER NOT NULL DEFAULT 0
            CHECK(observed_file_read_replacements >= 0),
        modeled_file_reads_avoided INTEGER NOT NULL DEFAULT 0
            CHECK(modeled_file_reads_avoided >= 0),
        PRIMARY KEY(instance_row_id, dimension_id)
    ) STRICT, WITHOUT ROWID;

    CREATE TABLE usage_daily_aggregates (
        project_instance_id BLOB NOT NULL CHECK(length(project_instance_id) = 16),
        day_epoch INTEGER NOT NULL CHECK(day_epoch >= 0),
        dimension_id INTEGER NOT NULL
            REFERENCES usage_bucket_dimensions(dimension_id) ON DELETE RESTRICT,
        calls INTEGER NOT NULL DEFAULT 0 CHECK(calls >= 0),
        estimated_without INTEGER NOT NULL DEFAULT 0 CHECK(estimated_without >= 0),
        estimated_with INTEGER NOT NULL DEFAULT 0 CHECK(estimated_with >= 0),
        observed_without INTEGER NOT NULL DEFAULT 0 CHECK(observed_without >= 0),
        observed_with INTEGER NOT NULL DEFAULT 0 CHECK(observed_with >= 0),
        modeled_without INTEGER NOT NULL DEFAULT 0 CHECK(modeled_without >= 0),
        modeled_with INTEGER NOT NULL DEFAULT 0 CHECK(modeled_with >= 0),
        deduped_modeled_without INTEGER NOT NULL DEFAULT 0 CHECK(deduped_modeled_without >= 0),
        deduped_modeled_with INTEGER NOT NULL DEFAULT 0 CHECK(deduped_modeled_with >= 0),
        repeated_baselines INTEGER NOT NULL DEFAULT 0 CHECK(repeated_baselines >= 0),
        observed_file_read_replacements INTEGER NOT NULL DEFAULT 0
            CHECK(observed_file_read_replacements >= 0),
        modeled_file_reads_avoided INTEGER NOT NULL DEFAULT 0
            CHECK(modeled_file_reads_avoided >= 0),
        PRIMARY KEY(project_instance_id, day_epoch, dimension_id)
    ) STRICT, WITHOUT ROWID;

    CREATE TABLE usage_instance_daily_aggregates (
        instance_row_id INTEGER NOT NULL
            REFERENCES usage_instances(instance_row_id) ON DELETE CASCADE,
        day_epoch INTEGER NOT NULL CHECK(day_epoch >= 0),
        dimension_id INTEGER NOT NULL
            REFERENCES usage_bucket_dimensions(dimension_id) ON DELETE RESTRICT,
        calls INTEGER NOT NULL DEFAULT 0 CHECK(calls >= 0),
        estimated_without INTEGER NOT NULL DEFAULT 0 CHECK(estimated_without >= 0),
        estimated_with INTEGER NOT NULL DEFAULT 0 CHECK(estimated_with >= 0),
        observed_without INTEGER NOT NULL DEFAULT 0 CHECK(observed_without >= 0),
        observed_with INTEGER NOT NULL DEFAULT 0 CHECK(observed_with >= 0),
        modeled_without INTEGER NOT NULL DEFAULT 0 CHECK(modeled_without >= 0),
        modeled_with INTEGER NOT NULL DEFAULT 0 CHECK(modeled_with >= 0),
        deduped_modeled_without INTEGER NOT NULL DEFAULT 0 CHECK(deduped_modeled_without >= 0),
        deduped_modeled_with INTEGER NOT NULL DEFAULT 0 CHECK(deduped_modeled_with >= 0),
        repeated_baselines INTEGER NOT NULL DEFAULT 0 CHECK(repeated_baselines >= 0),
        observed_file_read_replacements INTEGER NOT NULL DEFAULT 0
            CHECK(observed_file_read_replacements >= 0),
        modeled_file_reads_avoided INTEGER NOT NULL DEFAULT 0
            CHECK(modeled_file_reads_avoided >= 0),
        PRIMARY KEY(instance_row_id, day_epoch, dimension_id)
    ) STRICT, WITHOUT ROWID;

    CREATE TABLE usage_retention_state (
        singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
        policy_version INTEGER NOT NULL CHECK(policy_version > 0),
        logical_byte_version INTEGER NOT NULL CHECK(logical_byte_version > 0),
        raw_rows INTEGER NOT NULL DEFAULT 0 CHECK(raw_rows >= 0),
        raw_logical_bytes INTEGER NOT NULL DEFAULT 0 CHECK(raw_logical_bytes >= 0),
        baseline_rows INTEGER NOT NULL DEFAULT 0 CHECK(baseline_rows >= 0),
        baseline_logical_bytes INTEGER NOT NULL DEFAULT 0 CHECK(baseline_logical_bytes >= 0),
        dimension_rows INTEGER NOT NULL DEFAULT 0 CHECK(dimension_rows >= 0),
        instance_rows INTEGER NOT NULL DEFAULT 0 CHECK(instance_rows >= 0),
        label_rows INTEGER NOT NULL DEFAULT 0 CHECK(label_rows >= 0),
        daily_rows INTEGER NOT NULL DEFAULT 0 CHECK(daily_rows >= 0),
        label_tombstone_rows INTEGER NOT NULL DEFAULT 0 CHECK(label_tombstone_rows >= 0),
        instance_tombstone_rows INTEGER NOT NULL DEFAULT 0 CHECK(instance_tombstone_rows >= 0),
        pruned_raw_rows INTEGER NOT NULL DEFAULT 0 CHECK(pruned_raw_rows >= 0),
        pruned_instance_rows INTEGER NOT NULL DEFAULT 0 CHECK(pruned_instance_rows >= 0),
        evicted_tombstones INTEGER NOT NULL DEFAULT 0 CHECK(evicted_tombstones >= 0),
        writes_since_checkpoint INTEGER NOT NULL DEFAULT 0 CHECK(writes_since_checkpoint >= 0),
        last_maintenance_epoch INTEGER NOT NULL DEFAULT 0 CHECK(last_maintenance_epoch >= 0),
        last_checkpoint_epoch INTEGER NOT NULL DEFAULT 0 CHECK(last_checkpoint_epoch >= 0),
        oldest_retained_epoch INTEGER,
        raw_detail_complete INTEGER NOT NULL DEFAULT 1 CHECK(raw_detail_complete IN (0, 1)),
        dimension_detail_complete INTEGER NOT NULL DEFAULT 1
            CHECK(dimension_detail_complete IN (0, 1)),
        label_history_complete INTEGER NOT NULL DEFAULT 1
            CHECK(label_history_complete IN (0, 1)),
        maintenance_pending INTEGER NOT NULL DEFAULT 0 CHECK(maintenance_pending IN (0, 1)),
        clock_anomaly INTEGER NOT NULL DEFAULT 0 CHECK(clock_anomaly IN (0, 1)),
        spill_state TEXT NOT NULL DEFAULT 'not_applicable'
            CHECK(spill_state = 'not_applicable'),
        checkpoint_state TEXT NOT NULL DEFAULT 'not_due'
            CHECK(checkpoint_state IN ('not_due', 'completed', 'busy', 'error'))
    ) STRICT;

    CREATE TABLE usage_label_tombstones (
        project_instance_id BLOB NOT NULL CHECK(length(project_instance_id) = 16),
        caller_label TEXT NOT NULL,
        expired_at_epoch INTEGER NOT NULL CHECK(expired_at_epoch >= 0),
        last_instance_id BLOB
            CHECK(last_instance_id IS NULL OR length(last_instance_id) = 16),
        PRIMARY KEY(project_instance_id, caller_label)
    ) STRICT, WITHOUT ROWID;

    CREATE TABLE usage_instance_tombstones (
        project_instance_id BLOB NOT NULL CHECK(length(project_instance_id) = 16),
        runtime_instance_id BLOB NOT NULL CHECK(length(runtime_instance_id) = 16),
        retired_at_epoch INTEGER NOT NULL CHECK(retired_at_epoch >= 0),
        PRIMARY KEY(project_instance_id, runtime_instance_id)
    ) STRICT, WITHOUT ROWID;

    CREATE INDEX idx_usage_instances_label_state
        ON usage_instances(project_instance_id, caller_label, state, started_at_epoch, instance_row_id);
    CREATE INDEX idx_usage_instances_state_seen
        ON usage_instances(project_instance_id, state, last_seen_at_epoch, instance_row_id);
    CREATE INDEX idx_usage_instances_retention
        ON usage_instances(state, last_seen_at_epoch, instance_row_id);
    CREATE INDEX idx_usage_labels_seen
        ON usage_labels(project_instance_id, last_seen_at_epoch, caller_label);
    CREATE INDEX idx_usage_labels_retention
        ON usage_labels(last_seen_at_epoch, project_instance_id, caller_label);
    CREATE INDEX idx_usage_daily_retention
        ON usage_daily_aggregates(day_epoch, project_instance_id, dimension_id);
    CREATE INDEX idx_usage_instance_daily_retention
        ON usage_instance_daily_aggregates(day_epoch, instance_row_id, dimension_id);
    CREATE INDEX idx_usage_label_tombstones_expiry
        ON usage_label_tombstones(project_instance_id, expired_at_epoch, caller_label);
    CREATE INDEX idx_usage_instance_tombstones_expiry
        ON usage_instance_tombstones(project_instance_id, retired_at_epoch, runtime_instance_id);
    CREATE INDEX idx_usage_label_tombstones_retention
        ON usage_label_tombstones(expired_at_epoch, project_instance_id, caller_label);
    CREATE INDEX idx_usage_instance_tombstones_retention
        ON usage_instance_tombstones(retired_at_epoch, project_instance_id, runtime_instance_id);

    INSERT INTO usage_retention_state(
        singleton,
        policy_version,
        logical_byte_version
    ) VALUES(1, 1, 1);
";

/// Final schema-11 raw event table, created directly for fresh databases.
const CURRENT_USAGE_SCHEMA_SQL: &str = "
    CREATE TABLE usage_events (
        id INTEGER PRIMARY KEY,
        instance_row_id INTEGER NOT NULL
            REFERENCES usage_instances(instance_row_id) ON DELETE CASCADE,
        dimension_id INTEGER NOT NULL
            REFERENCES usage_bucket_dimensions(dimension_id) ON DELETE RESTRICT,
        command TEXT NOT NULL,
        path TEXT,
        query TEXT,
        estimated_tokens_without_projectatlas INTEGER,
        estimated_tokens_with_projectatlas INTEGER,
        estimated_tokens_saved INTEGER,
        calculation_trace TEXT NOT NULL,
        baseline_identity TEXT NOT NULL,
        baseline_fingerprint TEXT NOT NULL,
        created_at_epoch INTEGER NOT NULL CHECK(created_at_epoch >= 0),
        logical_bytes INTEGER NOT NULL CHECK(logical_bytes >= 0)
    ) STRICT;
    CREATE INDEX idx_usage_created_at
        ON usage_events(created_at_epoch, id);
    CREATE INDEX idx_usage_instance_created
        ON usage_events(instance_row_id, created_at_epoch, id);
";

/// Rename schema-10 raw events before rebuilding them into the strict current shape.
const PREPARE_TELEMETRY_MIGRATION_SQL: &str = "
    DROP INDEX idx_usage_created_at;
    DROP INDEX idx_usage_session_created_at;
    ALTER TABLE usage_events RENAME TO usage_events_legacy;
";

/// Inspect an existing database without creating, migrating, or repairing it.
pub(crate) fn preflight(
    path: &Path,
    expected_root: Option<&str>,
) -> DbResult<(SchemaPreflight, DatabaseLocation)> {
    let preflight = preflight_with_integrity(path, expected_root, false)?;
    if preflight.0.state == SchemaState::UpgradeRequired {
        preflight_with_integrity(path, expected_root, true)
    } else {
        Ok(preflight)
    }
}

/// Inspect one database against a native project root before writable access.
///
/// Current databases use the typed root identity. A missing typed identity is
/// admitted only for the narrow recovery path that proves unambiguous legacy
/// metadata names the same existing native root; no binding is created by this
/// helper.
pub(crate) fn preflight_for_project(
    path: &Path,
    expected_root: &CanonicalProjectRoot,
) -> DbResult<(SchemaPreflight, DatabaseLocation)> {
    let (preflight, location) = preflight(path, None)?;
    if preflight.state == SchemaState::Fresh {
        return Ok((preflight, location));
    }
    let connection = open_read_only_connection(path, &location)?;
    let found_identity = if preflight.state == SchemaState::Current {
        crate::project_identity::load_project_root_identity(&connection)?
    } else {
        None
    };
    if let Some(found) = found_identity.as_ref() {
        crate::project_identity::prove_existing_root_equivalence(
            expected_root.as_path(),
            found.as_path(),
        )?;
    } else {
        validate_legacy_project_root_binding(&connection, expected_root)?;
    }
    Ok((preflight, location))
}

/// Revalidate a predecessor's legacy root against the caller's native root.
///
/// This check is intentionally repeated from the writer connection after its
/// transaction begins. The outer read-only preflight is only an admission
/// hint; it cannot authorize a database or metadata replacement raced between
/// preflight and migration.
fn validate_legacy_project_root_binding(
    connection: &Connection,
    expected_root: &CanonicalProjectRoot,
) -> DbResult<CanonicalProjectRoot> {
    let legacy = read_metadata(connection, PROJECT_ROOT_KEY)?.ok_or(DbError::ProjectRootMissing)?;
    if legacy.contains('\u{fffd}') {
        // A replacement character may represent either a real character or a
        // lossy raw path byte. Without a native predecessor authority, the
        // spelling is ambiguous and must be rejected to prevent a collision.
        return Err(DbError::ProjectRootMismatch {
            expected: expected_root.display_string_lossy(),
            found: legacy,
        });
    }
    crate::project_identity::prove_existing_root_equivalence(
        expected_root.as_path(),
        Path::new(&legacy),
    )
}

/// Inspect compatibility without the full migration-admission integrity scan.
pub(crate) fn inspect_compatibility(
    path: &Path,
    expected_root: Option<&str>,
) -> DbResult<(SchemaPreflight, DatabaseLocation)> {
    preflight_with_integrity(path, expected_root, false)
}

/// Run explicit current-schema integrity verification without writable access.
#[cfg(test)]
pub(crate) fn verify_current_integrity(path: &Path, expected_root: Option<&str>) -> DbResult<()> {
    let (preflight, _) = preflight_with_integrity(path, expected_root, true)?;
    match preflight.state {
        SchemaState::Current => {
            if let Some(expected) = expected_root {
                let expected = CanonicalProjectRoot::from_path(Path::new(expected))?;
                let location = inspect_database_location(path)?;
                let connection = open_read_only_connection(path, &location)?;
                let found = crate::project_identity::load_project_root_identity(&connection)?
                    .ok_or(DbError::ProjectRootIdentityMissing)?;
                crate::project_identity::prove_existing_root_equivalence(
                    expected.as_path(),
                    found.as_path(),
                )?;
            }
            Ok(())
        }
        SchemaState::Fresh => Err(DbError::SchemaVersion {
            found: 0,
            expected: SCHEMA_VERSION,
        }),
        SchemaState::UpgradeRequired => Err(DbError::SchemaVersion {
            found: PREVIOUS_SCHEMA_VERSION,
            expected: SCHEMA_VERSION,
        }),
    }
}

/// Verify one current database against the caller's already-canonical native root.
pub(crate) fn verify_current_integrity_for_project(
    path: &Path,
    expected_root: &CanonicalProjectRoot,
) -> DbResult<()> {
    let (preflight, _) = preflight_with_integrity(path, None, true)?;
    if preflight.state != SchemaState::Current {
        return match preflight.state {
            SchemaState::Fresh => Err(DbError::SchemaVersion {
                found: 0,
                expected: SCHEMA_VERSION,
            }),
            SchemaState::UpgradeRequired => Err(DbError::SchemaVersion {
                found: PREVIOUS_SCHEMA_VERSION,
                expected: SCHEMA_VERSION,
            }),
            SchemaState::Current => unreachable!("current schema state was checked above"),
        };
    }
    let location = inspect_database_location(path)?;
    let connection = open_read_only_connection(path, &location)?;
    let found = crate::project_identity::load_project_root_identity(&connection)?
        .ok_or(DbError::ProjectRootIdentityMissing)?;
    crate::project_identity::prove_existing_root_equivalence(
        expected_root.as_path(),
        found.as_path(),
    )?;
    Ok(())
}

/// Inspect one stable read snapshot with caller-selected integrity depth.
fn preflight_with_integrity(
    path: &Path,
    expected_root: Option<&str>,
    run_integrity_check: bool,
) -> DbResult<(SchemaPreflight, DatabaseLocation)> {
    let location = inspect_database_location(path)?;
    if !location.database_exists {
        return Ok((
            SchemaPreflight {
                state: SchemaState::Fresh,
                schema_version: None,
                project_root: None,
                project_instance_id: None,
            },
            location,
        ));
    }
    let connection = open_read_only_connection(path, &location)?;
    connection.execute_batch("BEGIN DEFERRED")?;
    let inspected = inspect_connection(&connection, expected_root, run_integrity_check);
    match inspected {
        Ok(preflight) => {
            connection.execute_batch("COMMIT")?;
            Ok((preflight, location))
        }
        Err(error) => Err(rollback_after_error(&connection, error)),
    }
}

/// Initialize or migrate one already-open writable connection.
pub(crate) fn initialize(connection: &Connection, expected_root: Option<&str>) -> DbResult<()> {
    initialize_with_project_root(connection, expected_root, None)
}

/// Initialize or migrate with the caller's lossless native root identity.
pub(crate) fn initialize_with_project_root(
    connection: &Connection,
    expected_root: Option<&str>,
    expected_identity: Option<&CanonicalProjectRoot>,
) -> DbResult<()> {
    configure_writable(connection)?;
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        let preflight = if expected_identity.is_some() {
            inspect_connection_native(connection, expected_identity, false)?
        } else {
            inspect_connection(connection, expected_root, false)?
        };
        let predecessor_native_identity = if let Some(expected_identity) = expected_identity
            && preflight.state == SchemaState::UpgradeRequired
        {
            Some(validate_legacy_project_root_binding(
                connection,
                expected_identity,
            )?)
        } else {
            None
        };
        match preflight.state {
            SchemaState::Fresh => create_fresh(connection, expected_root, expected_identity)?,
            SchemaState::Current => {}
            SchemaState::UpgradeRequired => {
                validate_integrity(connection)?;
                apply_migrations(connection, stored_schema_version(connection)?)?;
            }
        }
        if let Some(expected) = expected_identity {
            crate::project_identity::ensure_project_identity(connection)?;
            if let Some(predecessor_identity) = predecessor_native_identity {
                // The writer-connection legacy binding recheck above proved
                // the predecessor metadata still names this root. Seed from
                // the caller's native authority rather than reconstructing a
                // possibly lossy display.
                crate::project_identity::set_project_root_identity(
                    connection,
                    &predecessor_identity,
                )?;
                crate::project_identity::set_project_root_metadata(
                    connection,
                    &predecessor_identity,
                )?;
            } else {
                crate::project_identity::ensure_project_root_identity_in_transaction(
                    connection, expected,
                )?;
            }
        } else if let Some(expected_root) = expected_root {
            let expected = CanonicalProjectRoot::from_path(Path::new(expected_root))?;
            crate::project_identity::ensure_project_identity(connection)?;
            crate::project_identity::ensure_project_root_identity_in_transaction(
                connection, &expected,
            )?;
        }
        let current = if expected_identity.is_some() {
            inspect_connection_native(connection, expected_identity, false)?
        } else {
            inspect_connection(connection, expected_root, false)?
        };
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
    configure_writable_connection(connection)
}

/// Open and validate one current read-only snapshot.
pub(crate) fn open_current_read_only(
    path: &Path,
    expected_root: Option<&str>,
) -> DbResult<(Connection, SchemaPreflight)> {
    let location = inspect_database_location(path)?;
    let connection = open_read_only_connection(path, &location)?;
    connection.execute_batch("BEGIN DEFERRED")?;
    match inspect_current(&connection, expected_root) {
        Ok(preflight) => {
            verify_current_read_profile(&connection)?;
            Ok((connection, preflight))
        }
        Err(error) => Err(rollback_after_error(&connection, error)),
    }
}

/// Revalidate the exact current root binding on a newly opened writer.
#[cfg(test)]
pub(crate) fn revalidate_current_binding(
    connection: &Connection,
    expected_root: Option<&str>,
    require_identity: bool,
) -> DbResult<Option<ProjectInstanceId>> {
    let transaction =
        rusqlite::Transaction::new_unchecked(connection, TransactionBehavior::Deferred)?;
    let result = (|| {
        let identity = current_binding(&transaction, expected_root, require_identity)?;
        if require_identity {
            let root_identity = crate::project_identity::load_project_root_identity(&transaction)?;
            if expected_root.is_some() && root_identity.is_none() {
                return Err(DbError::ProjectRootIdentityMissing);
            }
        }
        Ok(identity)
    })();
    match result {
        Ok(identity) => {
            transaction.commit()?;
            Ok(identity)
        }
        Err(operation) => match transaction.rollback() {
            Ok(()) => Err(operation),
            Err(rollback) => Err(DbError::TransactionRollback {
                operation: Box::new(operation),
                rollback,
            }),
        },
    }
}

/// Revalidate one current binding through the lossless native root identity.
pub(crate) fn revalidate_current_native_binding(
    connection: &Connection,
    expected_root: &CanonicalProjectRoot,
    require_identity: bool,
) -> DbResult<Option<ProjectInstanceId>> {
    let transaction =
        rusqlite::Transaction::new_unchecked(connection, TransactionBehavior::Deferred)?;
    let result = (|| {
        let found_root = crate::project_identity::load_project_root_identity(&transaction)?
            .ok_or(DbError::ProjectRootIdentityMissing)?;
        crate::project_identity::prove_existing_root_equivalence(
            expected_root.as_path(),
            found_root.as_path(),
        )?;
        let identity = crate::project_identity::load_project_identity(&transaction)?;
        if require_identity && identity.is_none() {
            return Err(DbError::ProjectInstanceIdentityMissing);
        }
        Ok(identity)
    })();
    match result {
        Ok(identity) => {
            transaction.commit()?;
            Ok(identity)
        }
        Err(operation) => match transaction.rollback() {
            Ok(()) => Err(operation),
            Err(rollback) => Err(DbError::TransactionRollback {
                operation: Box::new(operation),
                rollback,
            }),
        },
    }
}

/// Require the exact root and project identity captured by this store.
#[cfg(test)]
pub(crate) fn validate_active_binding(
    connection: &Connection,
    expected_root: Option<&str>,
    expected_identity: Option<ProjectInstanceId>,
) -> DbResult<()> {
    let found_identity = current_binding(connection, expected_root, false)?;
    if found_identity != expected_identity {
        return Err(DbError::ProjectRootTransitionChanged {
            expected_root: expected_root.map(str::to_owned),
            found_root: expected_root.map(str::to_owned),
            expected_identity: expected_identity.map(|identity| identity.to_string()),
            found_identity: found_identity.map(|identity| identity.to_string()),
        });
    }
    Ok(())
}

/// Require the exact native root and project identity captured by one store.
pub(crate) fn validate_active_native_binding(
    connection: &Connection,
    expected_root: Option<&CanonicalProjectRoot>,
    expected_identity: Option<ProjectInstanceId>,
) -> DbResult<()> {
    let found_root = crate::project_identity::load_project_root_identity(connection)?;
    if let (Some(expected), Some(found)) = (expected_root, found_root.as_ref()) {
        if crate::project_identity::prove_existing_root_equivalence(
            expected.as_path(),
            found.as_path(),
        )
        .is_err()
        {
            return Err(DbError::ProjectRootTransitionChanged {
                expected_root: expected_root.map(CanonicalProjectRoot::display_string_lossy),
                found_root: found_root.map(|root| root.display_string_lossy()),
                expected_identity: expected_identity.map(|identity| identity.to_string()),
                found_identity: crate::project_identity::load_project_identity(connection)?
                    .map(|identity| identity.to_string()),
            });
        }
    } else if found_root.as_ref() != expected_root {
        return Err(DbError::ProjectRootTransitionChanged {
            expected_root: expected_root.map(CanonicalProjectRoot::display_string_lossy),
            found_root: found_root.map(|root| root.display_string_lossy()),
            expected_identity: expected_identity.map(|identity| identity.to_string()),
            found_identity: crate::project_identity::load_project_identity(connection)?
                .map(|identity| identity.to_string()),
        });
    }
    let found_identity = crate::project_identity::load_project_identity(connection)?;
    if found_identity != expected_identity {
        return Err(DbError::ProjectRootTransitionChanged {
            expected_root: expected_root.map(CanonicalProjectRoot::display_string_lossy),
            found_root: found_root.map(|root| root.display_string_lossy()),
            expected_identity: expected_identity.map(|identity| identity.to_string()),
            found_identity: found_identity.map(|identity| identity.to_string()),
        });
    }
    Ok(())
}

/// Load one current binding while enforcing the selected root and identity depth.
#[cfg(test)]
fn current_binding(
    connection: &Connection,
    expected_root: Option<&str>,
    require_identity: bool,
) -> DbResult<Option<ProjectInstanceId>> {
    let found_version = stored_schema_version(connection)?;
    if found_version != SCHEMA_VERSION {
        return Err(DbError::SchemaVersion {
            found: found_version,
            expected: SCHEMA_VERSION,
        });
    }
    let found_root = read_metadata(connection, PROJECT_ROOT_KEY)?;
    if !project_roots_match(expected_root, found_root.as_deref()) {
        return match (expected_root, found_root) {
            (Some(expected), Some(found)) => Err(DbError::ProjectRootMismatch {
                expected: expected.to_string(),
                found,
            }),
            (Some(_), None) => Err(DbError::ProjectRootMissing),
            (None, found_root) => Err(DbError::ProjectRootTransitionChanged {
                expected_root: None,
                found_root,
                expected_identity: None,
                found_identity: None,
            }),
        };
    }
    let identity = crate::project_identity::load_project_identity(connection)?;
    validate_binding_completeness(expected_root, identity, require_identity)?;
    Ok(identity)
}

/// Validate the durable root/identity pair without repairing incomplete state.
pub(crate) fn validate_binding_completeness(
    project_root: Option<&str>,
    identity: Option<ProjectInstanceId>,
    require_identity: bool,
) -> DbResult<()> {
    match (project_root, identity) {
        (None, Some(found_identity)) => Err(DbError::ProjectRootTransitionChanged {
            expected_root: None,
            found_root: None,
            expected_identity: None,
            found_identity: Some(found_identity.to_string()),
        }),
        (Some(_), None) if require_identity => Err(DbError::ProjectInstanceIdentityMissing),
        _ => Ok(()),
    }
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
    let location = inspect_database_location(path)?;
    if !location.database_exists {
        return Ok(None);
    }
    let connection = open_read_only_connection(path, &location)?;
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
    let project_instance_id = if state == SchemaState::Fresh {
        None
    } else {
        read_project_instance_id_if_present(connection)?
    };
    if let Some(expected) = expected_root {
        match project_root.as_deref() {
            Some(found) if project_roots_match(Some(expected), Some(found)) => {}
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
    if state == SchemaState::Current
        && expected_root.is_some()
        && crate::project_identity::load_project_identity(connection)?.is_none()
    {
        return Err(DbError::ProjectInstanceIdentityMissing);
    }
    Ok(SchemaPreflight {
        state,
        schema_version: (state != SchemaState::Fresh)
            .then(|| stored_schema_version(connection))
            .transpose()?,
        project_root,
        project_instance_id,
    })
}

/// Read the predecessor project identity only when its singleton table exists.
///
/// Schema 19 still owns this identity even though it predates the native-root
/// table. Older released schemas do not, so absence remains a truthful
/// "created/changed" transition result rather than a fabricated identity.
fn read_project_instance_id_if_present(
    connection: &Connection,
) -> DbResult<Option<ProjectInstanceId>> {
    if object_kind(connection, "project_identity")?.as_deref() != Some("table") {
        return Ok(None);
    }
    crate::project_identity::load_project_identity(connection)
}

/// Inspect schema state without deriving identity from a display projection.
fn inspect_connection_native(
    connection: &Connection,
    expected_root: Option<&CanonicalProjectRoot>,
    run_integrity_check: bool,
) -> DbResult<SchemaPreflight> {
    let preflight = inspect_connection(connection, None, run_integrity_check)?;
    if preflight.state == SchemaState::Current
        && let Some(expected_root) = expected_root
    {
        let found_root = crate::project_identity::load_project_root_identity(connection)?
            .ok_or(DbError::ProjectRootIdentityMissing)?;
        crate::project_identity::prove_existing_root_equivalence(
            expected_root.as_path(),
            found_root.as_path(),
        )?;
    }
    Ok(preflight)
}

/// Compare root metadata through native canonical identity before rejecting an
/// equivalent macOS alias such as `/var` and `/private/var`.
fn project_roots_match(expected: Option<&str>, found: Option<&str>) -> bool {
    match (expected, found) {
        (None, None) => true,
        (Some(expected), Some(found)) => crate::project_identity::prove_existing_root_equivalence(
            Path::new(expected),
            Path::new(found),
        )
        .is_ok(),
        _ => false,
    }
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
            return Err(schema_shape_error("metadata", &"table", &"missing"));
        }
        Some("table") => {}
        Some(found) => {
            return Err(schema_shape_error("metadata", &"table", &found));
        }
    }
    let found = stored_schema_version(connection)?;
    match found {
        SCHEMA_VERSION => Ok(SchemaState::Current),
        supported if migration_steps_remaining(supported).is_some() => {
            Ok(SchemaState::UpgradeRequired)
        }
        _ => Err(DbError::SchemaVersion {
            found,
            expected: SCHEMA_VERSION,
        }),
    }
}

/// Create a new schema and stamp identity/version only after all DDL succeeds.
fn create_fresh(
    connection: &Connection,
    expected_root: Option<&str>,
    expected_identity: Option<&CanonicalProjectRoot>,
) -> DbResult<()> {
    connection.execute_batch(BASE_SCHEMA_SQL)?;
    add_symbol_source_selector_storage(connection)?;
    connection.execute_batch(SOURCE_PARSE_PROVENANCE_SCHEMA_SQL)?;
    create_graph_schema(connection, GraphSchemaShape::Current)?;
    create_coverage_discovery_schema(connection)?;
    connection.execute_batch(RESOLUTION_KEY_SCHEMA_SQL)?;
    connection.execute_batch(TELEMETRY_STORAGE_SCHEMA_SQL)?;
    connection.execute_batch(CURRENT_USAGE_SCHEMA_SQL)?;
    connection.execute_batch(FILE_TEXT_FTS_SCHEMA_SQL)?;
    connection.execute_batch(SYMBOL_RELATION_LOOKUP_SCHEMA_SQL)?;
    connection.execute_batch(FILE_CONTENT_CLASSIFICATION_SCHEMA_SQL)?;
    connection.execute_batch(WORKTREE_CONTROL_SCHEMA_SQL)?;
    connection.execute_batch(PROJECT_ROOT_IDENTITY_SCHEMA_SQL)?;
    set_metadata(connection, FILE_TEXT_FTS_SOURCE_REVISION_KEY, "0")?;
    set_metadata(connection, FILE_TEXT_FTS_PROJECTION_REVISION_KEY, "0")?;
    crate::telemetry::initialize_empty_storage(connection)?;
    if let Some(root) = expected_root {
        set_metadata(connection, PROJECT_ROOT_KEY, root)?;
    }
    if let Some(identity) = expected_identity {
        crate::project_identity::set_project_root_identity(connection, identity)?;
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
    create_graph_schema(connection, GraphSchemaShape::Compact)?;
    if read_metadata(connection, PROJECT_ROOT_KEY)?.is_some() {
        crate::project_identity::ensure_project_identity(connection)?;
    }
    invalidate_derived_publication(connection)
}

/// Add bounded telemetry instances, dimensions, aggregates, and retention state.
fn migrate_10_to_11(connection: &Connection) -> DbResult<()> {
    connection.execute_batch(PREPARE_TELEMETRY_MIGRATION_SQL)?;
    connection.execute_batch(TELEMETRY_STORAGE_SCHEMA_SQL)?;
    connection.execute_batch(CURRENT_USAGE_SCHEMA_SQL)?;
    crate::telemetry::migrate_legacy_usage(connection)?;
    connection.execute_batch("DROP TABLE usage_events_legacy")?;
    Ok(())
}

/// Add canonical resolution keys, normalize legacy purpose approval, and invalidate graph trust.
fn migrate_11_to_12(connection: &Connection) -> DbResult<()> {
    connection.execute_batch(RESOLUTION_KEY_SCHEMA_SQL)?;
    connection.execute(
        "UPDATE purposes SET status = 'approved' WHERE status = 'stale'",
        [],
    )?;
    invalidate_derived_publication(connection)
}

/// Persist source-parse and derived-fact parser provenance independently.
fn migrate_12_to_13(connection: &Connection) -> DbResult<()> {
    connection.execute_batch(SOURCE_PARSE_PROVENANCE_SCHEMA_SQL)?;
    Ok(())
}

/// Create both bounded coverage-discovery lookup families.
fn create_coverage_discovery_schema(connection: &Connection) -> DbResult<()> {
    connection.execute_batch(COVERAGE_DISCOVERY_SOURCE_SCHEMA_SQL)?;
    connection.execute_batch(COVERAGE_DISCOVERY_GRAPH_SCHEMA_SQL)?;
    Ok(())
}

/// Add the indexed filter paths used by bounded coverage discovery.
fn migrate_13_to_14(connection: &Connection) -> DbResult<()> {
    create_coverage_discovery_schema(connection)
}

/// Add and backfill the rebuildable lexical candidate accelerator.
fn migrate_14_to_15(connection: &Connection) -> DbResult<()> {
    connection.execute_batch(FILE_TEXT_FTS_SCHEMA_SQL)?;
    connection.execute(
        "INSERT INTO file_text_fts(file_text_fts) VALUES('rebuild')",
        [],
    )?;
    set_metadata(connection, FILE_TEXT_FTS_SOURCE_REVISION_KEY, "0")?;
    set_metadata(connection, FILE_TEXT_FTS_PROJECTION_REVISION_KEY, "0")?;
    Ok(())
}

/// Recreate disposable graph projections with compact stable-key ordering.
fn migrate_15_to_16(connection: &Connection) -> DbResult<()> {
    recreate_disposable_graph_projection(connection, false)
}

/// Add classified document constraints and file-role persistence.
fn migrate_16_to_17(connection: &Connection) -> DbResult<()> {
    recreate_disposable_graph_projection(connection, true)?;
    add_symbol_source_selector_storage(connection)?;
    connection.execute_batch(FILE_CONTENT_CLASSIFICATION_SCHEMA_SQL)?;
    invalidate_derived_publication(connection)
}

/// Add local worktree registration and aggregate telemetry control state.
fn migrate_17_to_18(connection: &Connection) -> DbResult<()> {
    connection.execute_batch(WORKTREE_CONTROL_SCHEMA_SQL)?;
    connection.execute(
        "INSERT INTO usage_aggregate_revisions(project_instance_id, revision)
         SELECT project_instance_id, SUM(calls)
         FROM usage_global_aggregates
         GROUP BY project_instance_id",
        [],
    )?;
    Ok(())
}

/// Admit every closed graph limit while rebuilding only disposable graph state.
fn migrate_18_to_19(connection: &Connection) -> DbResult<()> {
    recreate_disposable_graph_projection_with_shape(connection, GraphSchemaShape::Current)
}

/// Add the lossless native project-root identity without rewriting authored state.
fn migrate_19_to_20(connection: &Connection) -> DbResult<()> {
    connection.execute_batch(PROJECT_ROOT_IDENTITY_SCHEMA_SQL)?;
    Ok(())
}

/// Add selector columns by rebuilding the derived symbol table in the caller transaction.
fn add_symbol_source_selector_storage(connection: &Connection) -> DbResult<()> {
    connection.execute_batch(SYMBOL_SOURCE_SELECTOR_SCHEMA_SQL)?;
    Ok(())
}

/// Rebuild disposable normalized graph state while retaining project identity.
pub(crate) fn recreate_disposable_graph_projection(
    connection: &Connection,
    include_classified_documents: bool,
) -> DbResult<()> {
    let shape = if include_classified_documents {
        GraphSchemaShape::ClassifiedDocuments
    } else {
        GraphSchemaShape::Compact
    };
    recreate_disposable_graph_projection_with_shape(connection, shape)
}

/// Rebuild disposable normalized graph state under one exact schema contract.
fn recreate_disposable_graph_projection_with_shape(
    connection: &Connection,
    shape: GraphSchemaShape,
) -> DbResult<()> {
    let project_identity = connection
        .query_row(
            "SELECT project_instance_id FROM project_identity WHERE singleton = 1",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    connection.execute_batch(
        "
        DROP TABLE graph_relation_dependencies;
        DROP TABLE graph_entity_exports;
        DROP TABLE graph_resolution_keys;
        DROP TABLE graph_relation_occurrences;
        DROP TABLE graph_coverage;
        DROP TABLE graph_relations;
        DROP TABLE graph_entities;
        DROP TABLE project_identity;
        ",
    )?;
    create_graph_schema(connection, shape)?;
    if let Some(project_identity) = project_identity {
        connection.execute(
            "INSERT INTO project_identity(singleton, project_instance_id, active_generation)
             VALUES(1, ?1, 0)",
            [project_identity],
        )?;
    }
    connection.execute_batch(COVERAGE_DISCOVERY_GRAPH_SCHEMA_SQL)?;
    connection.execute_batch(RESOLUTION_KEY_SCHEMA_SQL)?;
    connection.execute_batch(SYMBOL_RELATION_LOOKUP_SCHEMA_SQL)?;
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

/// Validate tables, columns, constraints, and required indexes without repair DDL.
fn validate_schema_shape(connection: &Connection, state: SchemaState) -> DbResult<()> {
    let stored_version =
        (state == SchemaState::UpgradeRequired).then(|| stored_schema_version(connection));
    let stored_version = stored_version.transpose()?;
    if stored_version == Some(LEXICAL_SCHEMA_VERSION) {
        return validate_disposable_graph_predecessor_shape(connection);
    }
    let expected = match state {
        SchemaState::Current => schema_contract()?,
        SchemaState::UpgradeRequired => {
            match stored_version.ok_or(DbError::SchemaPostcondition {
                expected: SCHEMA_VERSION,
            })? {
                PREVIOUS_SCHEMA_VERSION | PUBLICATION_SCHEMA_VERSION => {
                    predecessor_schema_contract()?
                }
                GRAPH_SCHEMA_VERSION => graph_predecessor_schema_contract()?,
                TELEMETRY_SCHEMA_VERSION => telemetry_predecessor_schema_contract()?,
                RESOLUTION_SCHEMA_VERSION => resolution_predecessor_schema_contract()?,
                PARSER_PROVENANCE_SCHEMA_VERSION => {
                    parser_provenance_predecessor_schema_contract()?
                }
                COVERAGE_DISCOVERY_SCHEMA_VERSION => {
                    coverage_discovery_predecessor_schema_contract()?
                }
                COMPACT_GRAPH_SCHEMA_VERSION => compact_graph_predecessor_schema_contract()?,
                CLASSIFIED_GRAPH_SCHEMA_VERSION => classified_graph_predecessor_schema_contract()?,
                WORKTREE_CONTROL_SCHEMA_VERSION => worktree_control_predecessor_schema_contract()?,
                CANONICAL_ROOT_PREDECESSOR_SCHEMA_VERSION => {
                    canonical_root_predecessor_schema_contract()?
                }
                found => {
                    return Err(DbError::SchemaVersion {
                        found,
                        expected: SCHEMA_VERSION,
                    });
                }
            }
        }
        SchemaState::Fresh => {
            return Err(DbError::SchemaPostcondition {
                expected: SCHEMA_VERSION,
            });
        }
    };
    let found = read_schema_contract(connection)?;
    let released_schema_eight = stored_version == Some(PREVIOUS_SCHEMA_VERSION);
    let tables_match = if released_schema_eight {
        released_schema_tables_match(&expected.tables, &found.tables)
    } else {
        expected.tables == found.tables
    };
    if !tables_match {
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
            return Err(schema_shape_error(&required.name, required, &"missing"));
        };
        let index_matches = if released_schema_eight {
            released_schema_index_matches(required, actual)
        } else {
            actual == required
        };
        if !index_matches {
            return Err(schema_shape_error(&required.name, required, actual));
        }
    }
    for extra in found
        .indexes
        .iter()
        .filter(|index| !expected.indexes.iter().any(|item| item.name == index.name))
    {
        if !is_compatible_extension_index(extra) {
            return Err(schema_shape_error(
                &extra.name,
                &"optional non-unique full index over declared columns",
                extra,
            ));
        }
    }
    validate_extension_objects(connection)?;
    Ok(())
}

/// Compare released schema-8 tables by named semantics rather than physical order.
fn released_schema_tables_match(expected: &[TableContract], found: &[TableContract]) -> bool {
    expected.len() == found.len()
        && expected.iter().zip(found).all(|(required, actual)| {
            required.name == actual.name
                && canonical_table_definition(&required.definition)
                    .zip(canonical_table_definition(&actual.definition))
                    .is_some_and(|(required, actual)| required == actual)
                && required.columns.len() == actual.columns.len()
                && required.columns.iter().all(|required_column| {
                    actual.columns.iter().any(|actual_column| {
                        released_schema_column_matches(required_column, actual_column)
                    })
                })
        })
}

/// Compare one released column while ignoring only its physical table ordinal.
fn released_schema_column_matches(expected: &ColumnContract, found: &ColumnContract) -> bool {
    expected.name == found.name
        && expected.declared_type == found.declared_type
        && expected.not_null == found.not_null
        && expected.default_value == found.default_value
        && expected.primary_key_position == found.primary_key_position
        && expected.hidden == found.hidden
}

/// Compare one released index through named key references instead of table ordinals.
fn released_schema_index_matches(expected: &IndexContract, found: &IndexContract) -> bool {
    expected.table == found.table
        && expected.name == found.name
        && expected.unique == found.unique
        && expected.origin == found.origin
        && expected.partial == found.partial
        && expected.columns.len() == found.columns.len()
        && expected
            .columns
            .iter()
            .zip(&found.columns)
            .all(|(required, actual)| {
                required.sequence == actual.sequence
                    && required.name == actual.name
                    && required.descending == actual.descending
                    && required.collation == actual.collation
                    && required.key == actual.key
                    && (required.name.is_some() || required.column_id == actual.column_id)
            })
}

/// Canonicalize top-level table clauses while preserving each exact constraint.
fn canonical_table_definition(definition: &str) -> Option<(String, Vec<String>, String)> {
    let open = definition.find('(')?;
    let close = definition.rfind(')')?;
    if close <= open {
        return None;
    }
    let mut clauses = top_level_sql_clauses(&definition[open + 1..close])?;
    clauses.sort();
    Some((
        normalize_sql_fragment(&definition[..open]),
        clauses,
        normalize_sql_fragment(&definition[close + 1..]),
    ))
}

/// Split one table body on commas outside nested expressions and quoted values.
fn top_level_sql_clauses(body: &str) -> Option<Vec<String>> {
    let mut clauses = Vec::new();
    let mut start = 0;
    let mut depth = 0_u32;
    let mut quote = None;
    let mut characters = body.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if let Some(end_quote) = quote {
            if character == end_quote {
                if characters
                    .peek()
                    .is_some_and(|(_, next)| *next == end_quote)
                {
                    characters.next();
                } else {
                    quote = None;
                }
            }
            continue;
        }
        match character {
            '\'' | '"' | '`' => quote = Some(character),
            '[' => quote = Some(']'),
            '(' => depth = depth.checked_add(1)?,
            ')' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                let clause = normalize_sql_fragment(&body[start..index]);
                if clause.is_empty() {
                    return None;
                }
                clauses.push(clause);
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    if quote.is_some() || depth != 0 {
        return None;
    }
    let final_clause = normalize_sql_fragment(&body[start..]);
    if final_clause.is_empty() {
        return None;
    }
    clauses.push(final_clause);
    Some(clauses)
}

/// Normalize incidental whitespace inside one SQLite-owned schema fragment.
fn normalize_sql_fragment(fragment: &str) -> String {
    fragment.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Validate schema 15 outside objects introduced or rebuilt by its migration.
fn validate_disposable_graph_predecessor_shape(connection: &Connection) -> DbResult<()> {
    let expected = compact_graph_predecessor_schema_contract()?;
    let found = read_schema_contract(connection)?;
    let expected_graph_tables = expected
        .tables
        .iter()
        .filter(|table| table.name.starts_with("graph_"))
        .map(|table| table.name.as_str())
        .collect::<Vec<_>>();
    let found_graph_tables = found
        .tables
        .iter()
        .filter(|table| table.name.starts_with("graph_"))
        .map(|table| table.name.as_str())
        .collect::<Vec<_>>();
    if expected_graph_tables != found_graph_tables {
        return Err(schema_shape_error(
            "disposable graph tables",
            &expected_graph_tables,
            &found_graph_tables,
        ));
    }
    let expected_tables = expected
        .tables
        .iter()
        .filter(|table| !table.name.starts_with("graph_"))
        .collect::<Vec<_>>();
    let found_tables = found
        .tables
        .iter()
        .filter(|table| !table.name.starts_with("graph_"))
        .collect::<Vec<_>>();
    if expected_tables != found_tables {
        return Err(schema_shape_error(
            "non-graph tables",
            &expected_tables,
            &found_tables,
        ));
    }
    let expected_foreign_keys = expected
        .foreign_keys
        .iter()
        .filter(|foreign_key| !foreign_key.table.starts_with("graph_"))
        .collect::<Vec<_>>();
    let found_foreign_keys = found
        .foreign_keys
        .iter()
        .filter(|foreign_key| !foreign_key.table.starts_with("graph_"))
        .collect::<Vec<_>>();
    if expected_foreign_keys != found_foreign_keys {
        return Err(schema_shape_error(
            "non-graph foreign_keys",
            &expected_foreign_keys,
            &found_foreign_keys,
        ));
    }
    let expected_indexes = expected
        .indexes
        .iter()
        .filter(|index| {
            !index.table.starts_with("graph_") && index.name != SYMBOL_RELATION_LOOKUP_INDEX_NAME
        })
        .collect::<Vec<_>>();
    let found_indexes = found
        .indexes
        .iter()
        .filter(|index| !index.table.starts_with("graph_"))
        .collect::<Vec<_>>();
    for required in &expected_indexes {
        let Some(actual) = found_indexes
            .iter()
            .find(|index| index.name == required.name)
        else {
            return Err(schema_shape_error(&required.name, required, &"missing"));
        };
        if actual != required {
            return Err(schema_shape_error(&required.name, required, actual));
        }
    }
    for extra in found_indexes.iter().filter(|index| {
        !expected_indexes
            .iter()
            .any(|expected| expected.name == index.name)
    }) {
        if !is_compatible_extension_index(extra) {
            return Err(schema_shape_error(
                &extra.name,
                &"optional non-unique full index over declared columns",
                extra,
            ));
        }
    }
    validate_extension_objects(connection)
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
    add_symbol_source_selector_storage(&connection)?;
    connection.execute_batch(SOURCE_PARSE_PROVENANCE_SCHEMA_SQL)?;
    create_graph_schema(&connection, GraphSchemaShape::Current)?;
    create_coverage_discovery_schema(&connection)?;
    connection.execute_batch(RESOLUTION_KEY_SCHEMA_SQL)?;
    connection.execute_batch(TELEMETRY_STORAGE_SCHEMA_SQL)?;
    connection.execute_batch(CURRENT_USAGE_SCHEMA_SQL)?;
    connection.execute_batch(FILE_TEXT_FTS_SCHEMA_SQL)?;
    connection.execute_batch(SYMBOL_RELATION_LOOKUP_SCHEMA_SQL)?;
    connection.execute_batch(FILE_CONTENT_CLASSIFICATION_SCHEMA_SQL)?;
    connection.execute_batch(WORKTREE_CONTROL_SCHEMA_SQL)?;
    connection.execute_batch(PROJECT_ROOT_IDENTITY_SCHEMA_SQL)?;
    let contract = read_schema_contract(&connection)?;
    Ok(SCHEMA_CONTRACT.get_or_init(|| contract))
}

/// Build the immutable schema-10 contract from base plus graph DDL.
fn graph_predecessor_schema_contract() -> DbResult<&'static SchemaContract> {
    if let Some(contract) = GRAPH_PREDECESSOR_SCHEMA_CONTRACT.get() {
        return Ok(contract);
    }
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(BASE_SCHEMA_SQL)?;
    connection.execute_batch(LEGACY_USAGE_SCHEMA_SQL)?;
    create_graph_schema(&connection, GraphSchemaShape::Compact)?;
    let contract = read_schema_contract(&connection)?;
    Ok(GRAPH_PREDECESSOR_SCHEMA_CONTRACT.get_or_init(|| contract))
}

/// Build the immutable schema-11 contract before resolution-key storage.
fn telemetry_predecessor_schema_contract() -> DbResult<&'static SchemaContract> {
    if let Some(contract) = TELEMETRY_PREDECESSOR_SCHEMA_CONTRACT.get() {
        return Ok(contract);
    }
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(BASE_SCHEMA_SQL)?;
    create_graph_schema(&connection, GraphSchemaShape::Compact)?;
    connection.execute_batch(TELEMETRY_STORAGE_SCHEMA_SQL)?;
    connection.execute_batch(CURRENT_USAGE_SCHEMA_SQL)?;
    let contract = read_schema_contract(&connection)?;
    Ok(TELEMETRY_PREDECESSOR_SCHEMA_CONTRACT.get_or_init(|| contract))
}

/// Build the immutable schema-12 contract before parser provenance separation.
fn resolution_predecessor_schema_contract() -> DbResult<&'static SchemaContract> {
    if let Some(contract) = RESOLUTION_PREDECESSOR_SCHEMA_CONTRACT.get() {
        return Ok(contract);
    }
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(BASE_SCHEMA_SQL)?;
    create_graph_schema(&connection, GraphSchemaShape::Compact)?;
    connection.execute_batch(RESOLUTION_KEY_SCHEMA_SQL)?;
    connection.execute_batch(TELEMETRY_STORAGE_SCHEMA_SQL)?;
    connection.execute_batch(CURRENT_USAGE_SCHEMA_SQL)?;
    let contract = read_schema_contract(&connection)?;
    Ok(RESOLUTION_PREDECESSOR_SCHEMA_CONTRACT.get_or_init(|| contract))
}

/// Build the immutable schema-13 contract before coverage-discovery indexes.
fn parser_provenance_predecessor_schema_contract() -> DbResult<&'static SchemaContract> {
    if let Some(contract) = PARSER_PROVENANCE_PREDECESSOR_SCHEMA_CONTRACT.get() {
        return Ok(contract);
    }
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(BASE_SCHEMA_SQL)?;
    connection.execute_batch(SOURCE_PARSE_PROVENANCE_SCHEMA_SQL)?;
    create_graph_schema(&connection, GraphSchemaShape::Compact)?;
    connection.execute_batch(RESOLUTION_KEY_SCHEMA_SQL)?;
    connection.execute_batch(TELEMETRY_STORAGE_SCHEMA_SQL)?;
    connection.execute_batch(CURRENT_USAGE_SCHEMA_SQL)?;
    let contract = read_schema_contract(&connection)?;
    Ok(PARSER_PROVENANCE_PREDECESSOR_SCHEMA_CONTRACT.get_or_init(|| contract))
}

/// Build the immutable schema-14 contract before lexical acceleration.
fn coverage_discovery_predecessor_schema_contract() -> DbResult<&'static SchemaContract> {
    if let Some(contract) = COVERAGE_DISCOVERY_PREDECESSOR_SCHEMA_CONTRACT.get() {
        return Ok(contract);
    }
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(BASE_SCHEMA_SQL)?;
    connection.execute_batch(SOURCE_PARSE_PROVENANCE_SCHEMA_SQL)?;
    create_graph_schema(&connection, GraphSchemaShape::Compact)?;
    create_coverage_discovery_schema(&connection)?;
    connection.execute_batch(RESOLUTION_KEY_SCHEMA_SQL)?;
    connection.execute_batch(TELEMETRY_STORAGE_SCHEMA_SQL)?;
    connection.execute_batch(CURRENT_USAGE_SCHEMA_SQL)?;
    let contract = read_schema_contract(&connection)?;
    Ok(COVERAGE_DISCOVERY_PREDECESSOR_SCHEMA_CONTRACT.get_or_init(|| contract))
}

/// Build the immutable schema-16 contract before classified-document storage.
fn compact_graph_predecessor_schema_contract() -> DbResult<&'static SchemaContract> {
    if let Some(contract) = COMPACT_GRAPH_PREDECESSOR_SCHEMA_CONTRACT.get() {
        return Ok(contract);
    }
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(BASE_SCHEMA_SQL)?;
    connection.execute_batch(SOURCE_PARSE_PROVENANCE_SCHEMA_SQL)?;
    create_graph_schema(&connection, GraphSchemaShape::Compact)?;
    create_coverage_discovery_schema(&connection)?;
    connection.execute_batch(RESOLUTION_KEY_SCHEMA_SQL)?;
    connection.execute_batch(TELEMETRY_STORAGE_SCHEMA_SQL)?;
    connection.execute_batch(CURRENT_USAGE_SCHEMA_SQL)?;
    connection.execute_batch(FILE_TEXT_FTS_SCHEMA_SQL)?;
    connection.execute_batch(SYMBOL_RELATION_LOOKUP_SCHEMA_SQL)?;
    let contract = read_schema_contract(&connection)?;
    Ok(COMPACT_GRAPH_PREDECESSOR_SCHEMA_CONTRACT.get_or_init(|| contract))
}

/// Build the immutable schema-17 contract before worktree control storage.
fn classified_graph_predecessor_schema_contract() -> DbResult<&'static SchemaContract> {
    if let Some(contract) = CLASSIFIED_GRAPH_PREDECESSOR_SCHEMA_CONTRACT.get() {
        return Ok(contract);
    }
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(BASE_SCHEMA_SQL)?;
    add_symbol_source_selector_storage(&connection)?;
    connection.execute_batch(SOURCE_PARSE_PROVENANCE_SCHEMA_SQL)?;
    create_graph_schema(&connection, GraphSchemaShape::ClassifiedDocuments)?;
    create_coverage_discovery_schema(&connection)?;
    connection.execute_batch(RESOLUTION_KEY_SCHEMA_SQL)?;
    connection.execute_batch(TELEMETRY_STORAGE_SCHEMA_SQL)?;
    connection.execute_batch(CURRENT_USAGE_SCHEMA_SQL)?;
    connection.execute_batch(FILE_TEXT_FTS_SCHEMA_SQL)?;
    connection.execute_batch(SYMBOL_RELATION_LOOKUP_SCHEMA_SQL)?;
    connection.execute_batch(FILE_CONTENT_CLASSIFICATION_SCHEMA_SQL)?;
    let contract = read_schema_contract(&connection)?;
    Ok(CLASSIFIED_GRAPH_PREDECESSOR_SCHEMA_CONTRACT.get_or_init(|| contract))
}

/// Build the immutable schema-18 contract before complete graph-limit admission.
fn worktree_control_predecessor_schema_contract() -> DbResult<&'static SchemaContract> {
    if let Some(contract) = WORKTREE_CONTROL_PREDECESSOR_SCHEMA_CONTRACT.get() {
        return Ok(contract);
    }
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(BASE_SCHEMA_SQL)?;
    add_symbol_source_selector_storage(&connection)?;
    connection.execute_batch(SOURCE_PARSE_PROVENANCE_SCHEMA_SQL)?;
    create_graph_schema(&connection, GraphSchemaShape::ClassifiedDocuments)?;
    create_coverage_discovery_schema(&connection)?;
    connection.execute_batch(RESOLUTION_KEY_SCHEMA_SQL)?;
    connection.execute_batch(TELEMETRY_STORAGE_SCHEMA_SQL)?;
    connection.execute_batch(CURRENT_USAGE_SCHEMA_SQL)?;
    connection.execute_batch(FILE_TEXT_FTS_SCHEMA_SQL)?;
    connection.execute_batch(SYMBOL_RELATION_LOOKUP_SCHEMA_SQL)?;
    connection.execute_batch(FILE_CONTENT_CLASSIFICATION_SCHEMA_SQL)?;
    connection.execute_batch(WORKTREE_CONTROL_SCHEMA_SQL)?;
    let contract = read_schema_contract(&connection)?;
    Ok(WORKTREE_CONTROL_PREDECESSOR_SCHEMA_CONTRACT.get_or_init(|| contract))
}

/// Build the schema-19 contract immediately before native-root identity storage.
fn canonical_root_predecessor_schema_contract() -> DbResult<&'static SchemaContract> {
    if let Some(contract) = CANONICAL_ROOT_PREDECESSOR_SCHEMA_CONTRACT.get() {
        return Ok(contract);
    }
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(BASE_SCHEMA_SQL)?;
    add_symbol_source_selector_storage(&connection)?;
    connection.execute_batch(SOURCE_PARSE_PROVENANCE_SCHEMA_SQL)?;
    create_graph_schema(&connection, GraphSchemaShape::Current)?;
    create_coverage_discovery_schema(&connection)?;
    connection.execute_batch(RESOLUTION_KEY_SCHEMA_SQL)?;
    connection.execute_batch(TELEMETRY_STORAGE_SCHEMA_SQL)?;
    connection.execute_batch(CURRENT_USAGE_SCHEMA_SQL)?;
    connection.execute_batch(FILE_TEXT_FTS_SCHEMA_SQL)?;
    connection.execute_batch(SYMBOL_RELATION_LOOKUP_SCHEMA_SQL)?;
    connection.execute_batch(FILE_CONTENT_CLASSIFICATION_SCHEMA_SQL)?;
    connection.execute_batch(WORKTREE_CONTROL_SCHEMA_SQL)?;
    let contract = read_schema_contract(&connection)?;
    Ok(CANONICAL_ROOT_PREDECESSOR_SCHEMA_CONTRACT.get_or_init(|| contract))
}

/// Build the immutable schema-8/9 contract from the unchanged base DDL.
fn predecessor_schema_contract() -> DbResult<&'static SchemaContract> {
    if let Some(contract) = PREDECESSOR_SCHEMA_CONTRACT.get() {
        return Ok(contract);
    }
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(BASE_SCHEMA_SQL)?;
    connection.execute_batch(LEGACY_USAGE_SCHEMA_SQL)?;
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
                return Err(schema_shape_error(
                    &name,
                    &"contract table or compatible non-unique index",
                    &(kind.as_str(), table.as_str(), sql.as_deref()),
                ));
            }
        }
    }
    Ok(())
}

/// Retain only the bounded prefix of one formatted schema diagnostic.
struct BoundedSchemaDiagnostic {
    /// Retained formatted prefix.
    value: String,
    /// Maximum UTF-8 bytes retained after the truncation marker is appended.
    max_bytes: usize,
    /// Whether formatting exceeded the configured byte ceiling.
    truncated: bool,
}

impl BoundedSchemaDiagnostic {
    /// Create one empty diagnostic field with a fixed byte ceiling.
    fn new(max_bytes: usize) -> Self {
        Self {
            value: String::new(),
            max_bytes,
            truncated: false,
        }
    }

    /// Append the truncation marker without splitting a UTF-8 code point.
    fn finish(mut self) -> String {
        if !self.truncated {
            return self.value;
        }
        let target = self
            .max_bytes
            .saturating_sub(SCHEMA_DIAGNOSTIC_TRUNCATION_SUFFIX.len());
        let mut boundary = target.min(self.value.len());
        while !self.value.is_char_boundary(boundary) {
            boundary = boundary.saturating_sub(1);
        }
        self.value.truncate(boundary);
        self.value.push_str(SCHEMA_DIAGNOSTIC_TRUNCATION_SUFFIX);
        self.value
    }
}

impl fmt::Write for BoundedSchemaDiagnostic {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if self.truncated {
            return Err(fmt::Error);
        }
        let remaining = self.max_bytes.saturating_sub(self.value.len());
        if value.len() <= remaining {
            self.value.push_str(value);
            return Ok(());
        }
        let mut boundary = remaining;
        while !value.is_char_boundary(boundary) {
            boundary = boundary.saturating_sub(1);
        }
        self.value.push_str(&value[..boundary]);
        self.truncated = true;
        Err(fmt::Error)
    }
}

/// Format one schema diagnostic without first allocating its complete representation.
fn bounded_schema_diagnostic(max_bytes: usize, arguments: fmt::Arguments<'_>) -> String {
    let mut output = BoundedSchemaDiagnostic::new(max_bytes);
    if fmt::write(&mut output, arguments).is_err() {
        output.truncated = true;
    }
    output.finish()
}

/// Build one readable typed schema mismatch.
fn schema_shape_error(
    object: &str,
    expected: &impl std::fmt::Debug,
    found: &impl std::fmt::Debug,
) -> DbError {
    DbError::SchemaShape {
        object: bounded_schema_diagnostic(
            SCHEMA_DIAGNOSTIC_OBJECT_MAX_BYTES,
            format_args!("{object}"),
        ),
        expected: bounded_schema_diagnostic(
            SCHEMA_DIAGNOSTIC_VALUE_MAX_BYTES,
            format_args!("{expected:?}"),
        ),
        found: bounded_schema_diagnostic(
            SCHEMA_DIAGNOSTIC_VALUE_MAX_BYTES,
            format_args!("{found:?}"),
        ),
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
pub(crate) fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(suffix);
    PathBuf::from(sidecar)
}

/// Restore the exact selector-free symbol shape owned by predecessor fixtures.
#[cfg(test)]
pub(crate) fn recreate_pre_selector_symbol_storage_for_test(
    connection: &Connection,
) -> DbResult<()> {
    connection.execute_batch(
        "CREATE TABLE symbols_without_source_selectors (
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
         INSERT INTO symbols_without_source_selectors(
             id, path, language, name, kind, signature, exported, documentation,
             line_start, line_end, parent, parser, detail, created_at, updated_at
         )
         SELECT
             id, path, language, name, kind, signature, exported, documentation,
             line_start, line_end, parent, parser, detail, created_at, updated_at
         FROM symbols;
         DROP TABLE symbols;
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
         INSERT INTO symbols(
             id, path, language, name, kind, signature, exported, documentation,
             line_start, line_end, parent, parser, detail, created_at, updated_at
         )
         SELECT
             id, path, language, name, kind, signature, exported, documentation,
             line_start, line_end, parent, parser, detail, created_at, updated_at
         FROM symbols_without_source_selectors;
         DROP TABLE symbols_without_source_selectors;
         CREATE INDEX idx_symbols_path ON symbols(path);
         CREATE INDEX idx_symbols_name ON symbols(name);
         CREATE INDEX idx_symbols_kind ON symbols(kind);",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AtlasStore, DbError};
    use projectatlas_core::graph::{CoverageScope, RepositoryNodePath};
    use projectatlas_core::telemetry::{
        TokenOverview, TokenTrendPeriod, TokenTrendWindow, UsageEvent, usage_from_estimates,
    };
    use rusqlite::types::Value;
    use std::error::Error;
    use std::fs;
    use std::io;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;

    /// Complete logical snapshot of the durable schema-8 state used by rollback tests.
    #[derive(Debug, PartialEq)]
    struct ReleasedSchemaDurableState {
        metadata: Vec<Vec<Value>>,
        nodes: Vec<Vec<Value>>,
        purposes: Vec<Vec<Value>>,
        summaries: Vec<Vec<Value>>,
        health_resolutions: Vec<Vec<Value>>,
        file_texts: Vec<Vec<Value>>,
        usage_events: Vec<Vec<Value>>,
    }

    /// Read every column from the durable tables in deterministic key order.
    fn released_schema_durable_state(
        connection: &Connection,
    ) -> DbResult<ReleasedSchemaDurableState> {
        Ok(ReleasedSchemaDurableState {
            metadata: query_all_values(connection, "SELECT * FROM metadata ORDER BY key")?,
            nodes: query_all_values(connection, "SELECT * FROM nodes ORDER BY id")?,
            purposes: query_all_values(connection, "SELECT * FROM purposes ORDER BY node_id")?,
            summaries: query_all_values(connection, "SELECT * FROM summaries ORDER BY id")?,
            health_resolutions: query_all_values(
                connection,
                "SELECT * FROM health_resolutions ORDER BY finding_id",
            )?,
            file_texts: query_all_values(connection, "SELECT * FROM file_texts ORDER BY path")?,
            usage_events: query_all_values(connection, "SELECT * FROM usage_events ORDER BY id")?,
        })
    }

    /// Read one deterministic result set without weakening its `SQLite` value types.
    fn query_all_values(connection: &Connection, sql: &str) -> DbResult<Vec<Vec<Value>>> {
        let mut statement = connection.prepare(sql)?;
        let column_count = statement.column_count();
        let rows = statement.query_map([], |row| {
            (0..column_count)
                .map(|column| row.get::<_, Value>(column))
                .collect::<Result<Vec<_>, _>>()
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Complete current-schema object and logical-row snapshot used by refusal tests.
    #[derive(Debug, PartialEq)]
    struct CurrentSchemaSnapshot {
        schema_objects: Vec<Vec<Value>>,
        tables: Vec<(String, Vec<Vec<Value>>)>,
    }

    /// Read every user table and schema object without checkpointing the live database.
    fn current_schema_snapshot(
        connection: &Connection,
    ) -> Result<CurrentSchemaSnapshot, Box<dyn Error>> {
        let schema_objects = query_all_values(
            connection,
            "SELECT type, name, tbl_name, sql
             FROM sqlite_master
             WHERE name NOT LIKE 'sqlite_%'
             ORDER BY type, name",
        )?;
        let table_names = query_all_values(
            connection,
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )?;
        let mut tables = Vec::with_capacity(table_names.len());
        for row in table_names {
            let [Value::Text(table_name)] = row.as_slice() else {
                return Err(io::Error::other(format!(
                    "sqlite_master returned an invalid table name: {row:?}"
                ))
                .into());
            };
            let quoted_name = table_name.replace('"', "\"\"");
            let select = format!("SELECT * FROM \"{quoted_name}\"");
            let column_count = connection.prepare(&select)?.column_count();
            let ordering = (1..=column_count)
                .map(|column| column.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let rows = query_all_values(connection, &format!("{select} ORDER BY {ordering}"))?;
            tables.push((table_name.clone(), rows));
        }
        Ok(CurrentSchemaSnapshot {
            schema_objects,
            tables,
        })
    }

    #[test]
    fn newer_schema_active_wal_is_refused_without_mutating_durable_state()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let db_path = temp.path().join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&db_path, &root)?;
        store
            .connection
            .execute_batch("PRAGMA wal_autocheckpoint = 0")?;
        crate::tests::populate_schema_compatibility_fixture(&mut store, "future")?;
        let navigation_store = AtlasStore::open_read_only_for_project(&db_path, &root)?;

        let future_version = SCHEMA_VERSION + 1;
        set_metadata(
            &store.connection,
            SCHEMA_VERSION_KEY,
            &future_version.to_string(),
        )?;
        let wal_path = sqlite_sidecar_path(&db_path, "-wal");
        let shm_path = sqlite_sidecar_path(&db_path, "-shm");
        let wal_before = fs::read(&wal_path)?;
        if wal_before.is_empty() || !shm_path.is_file() {
            return Err(io::Error::other(
                "future-schema fixture did not retain a live WAL and SHM sidecar",
            )
            .into());
        }
        let database_before = fs::read(&db_path)?;
        let inventory_before = directory_entry_names(temp.path())?;
        let snapshot_before = current_schema_snapshot(&store.connection)?;
        for required_table in [
            "project_identity",
            "purposes",
            "health_resolutions",
            "usage_events",
            "nodes",
            "summaries",
            "symbols",
            "file_texts",
        ] {
            if !snapshot_before
                .tables
                .iter()
                .any(|(table, rows)| table == required_table && !rows.is_empty())
            {
                return Err(io::Error::other(format!(
                    "future-schema fixture did not populate {required_table}"
                ))
                .into());
            }
        }

        let Err(error) = AtlasStore::open_for_project(&db_path, &root) else {
            return Err(io::Error::other("newer schema unexpectedly opened writable").into());
        };
        if !matches!(
            error,
            DbError::SchemaVersion {
                found,
                expected: SCHEMA_VERSION,
            } if found == future_version
        ) {
            return Err(io::Error::other(format!(
                "newer-schema refusal returned the wrong error: {error}"
            ))
            .into());
        }

        let Err(telemetry_error) = navigation_store.record_usage(&usage_from_estimates(
            "future-schema-session",
            "summary",
            Some("src/lib.rs".to_string()),
            None,
            100,
            20,
        )) else {
            return Err(io::Error::other(
                "newer schema unexpectedly opened writable for telemetry",
            )
            .into());
        };
        if !matches!(
            telemetry_error,
            DbError::SchemaVersion {
                found,
                expected: SCHEMA_VERSION,
            } if found == future_version
        ) {
            return Err(io::Error::other(format!(
                "newer-schema telemetry refusal returned the wrong error: {telemetry_error}"
            ))
            .into());
        }

        if fs::read(&db_path)? != database_before
            || fs::read(&wal_path)? != wal_before
            || directory_entry_names(temp.path())? != inventory_before
        {
            return Err(io::Error::other(
                "newer-schema refusal changed database bytes, WAL bytes, or sidecar inventory",
            )
            .into());
        }
        let snapshot_after = current_schema_snapshot(&store.connection)?;
        if snapshot_after != snapshot_before {
            return Err(io::Error::other(
                "newer-schema refusal changed schema objects or logical database state",
            )
            .into());
        }
        if read_metadata(&store.connection, SCHEMA_VERSION_KEY)? != Some(future_version.to_string())
        {
            return Err(io::Error::other("live owner could not read the retained schema").into());
        }
        Ok(())
    }

    #[test]
    fn newer_schema_preflight_work_is_independent_of_derived_row_count()
    -> Result<(), Box<dyn Error>> {
        let small_steps = incompatible_preflight_vm_steps(1)?;
        let large_steps = incompatible_preflight_vm_steps(10_000)?;
        if small_steps == 0 || large_steps != small_steps {
            return Err(io::Error::other(format!(
                "newer-schema inspection scaled with derived rows: small={small_steps}, large={large_steps}"
            ))
            .into());
        }
        Ok(())
    }

    /// Count the actual `SQLite` virtual-machine work used by incompatible preflight.
    fn incompatible_preflight_vm_steps(row_count: i64) -> Result<usize, Box<dyn Error>> {
        let store = AtlasStore::in_memory()?;
        store.connection.execute(
            "WITH RECURSIVE sequence(value) AS (
                 VALUES(1)
                 UNION ALL
                 SELECT value + 1 FROM sequence WHERE value < ?1
             )
             INSERT INTO nodes(path, kind)
             SELECT printf('src/file-%08d.rs', value), 'file' FROM sequence",
            [row_count],
        )?;
        let inserted = store
            .connection
            .query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get::<_, i64>(0))?;
        if inserted != row_count {
            return Err(io::Error::other("row-count fixture was not populated").into());
        }
        let future_version = SCHEMA_VERSION + 1;
        set_metadata(
            &store.connection,
            SCHEMA_VERSION_KEY,
            &future_version.to_string(),
        )?;
        let vm_steps = Arc::new(AtomicUsize::new(0));
        let observed_steps = Arc::clone(&vm_steps);
        store.connection.progress_handler(
            1,
            Some(move || {
                observed_steps.fetch_add(1, Ordering::Relaxed);
                false
            }),
        )?;
        let result = inspect_connection(&store.connection, None, false);
        store.connection.progress_handler(0, None::<fn() -> bool>)?;
        let Err(error) = result else {
            return Err(io::Error::other("newer schema unexpectedly passed inspection").into());
        };
        if !matches!(
            error,
            DbError::SchemaVersion {
                found,
                expected: SCHEMA_VERSION,
            } if found == future_version
        ) {
            return Err(io::Error::other("bounded inspection returned the wrong error").into());
        }
        Ok(vm_steps.load(Ordering::Relaxed))
    }

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
        let DbError::SchemaShape {
            object,
            expected,
            found,
        } = &malformed_error
        else {
            return Err(io::Error::other("metadata view returned the wrong error").into());
        };
        if object.len() > SCHEMA_DIAGNOSTIC_OBJECT_MAX_BYTES
            || expected.len() > SCHEMA_DIAGNOSTIC_VALUE_MAX_BYTES
            || found.len() > SCHEMA_DIAGNOSTIC_VALUE_MAX_BYTES
        {
            return Err(io::Error::other("metadata view returned an unbounded diagnostic").into());
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
    fn released_schema_eight_layouts_are_admitted_without_mutation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let layouts: [(&str, fn(&Connection) -> DbResult<()>); 2] = [
            ("fresh-v0.3.26", create_released_schema_eight),
            (
                "evolved-v0.3.11-to-v0.3.26",
                create_evolved_released_schema_eight,
            ),
        ];
        for (label, create) in layouts {
            let case = temp.path().join(label);
            fs::create_dir_all(&case)?;
            let root = case.join("repository");
            fs::create_dir_all(&root)?;
            let db_path = case.join("projectatlas.db");
            let connection = Connection::open(&db_path)?;
            create(&connection)?;
            set_metadata(
                &connection,
                PROJECT_ROOT_KEY,
                &normalize_native_path_display(&root),
            )?;
            drop(connection);
            let database_before = fs::read(&db_path)?;
            let inventory_before = directory_entry_names(&case)?;

            let (preflight, _) = preflight(&db_path, Some(&normalize_native_path_display(&root)))?;
            if preflight.state != SchemaState::UpgradeRequired
                || preflight.schema_version != Some(PREVIOUS_SCHEMA_VERSION)
            {
                return Err(io::Error::other(format!(
                    "{label} did not report a supported predecessor"
                ))
                .into());
            }
            require_unchanged(&case, &db_path, &database_before, &inventory_before)?;
        }
        Ok(())
    }

    #[test]
    fn evolved_released_schema_drift_is_refused_without_mutation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let oversized_constraint = format!(
            "    command TEXT NOT NULL CHECK(command <> '{}'),\n",
            "x".repeat(SCHEMA_DIAGNOSTIC_VALUE_MAX_BYTES * 16)
        );
        let cases = [
            (
                "missing-column",
                "    model TEXT NOT NULL DEFAULT 'unknown',\n",
                String::new(),
            ),
            (
                "renamed-column",
                "    model TEXT NOT NULL DEFAULT 'unknown',\n",
                "    model_name TEXT NOT NULL DEFAULT 'unknown',\n".to_string(),
            ),
            (
                "extra-column",
                "    model TEXT NOT NULL DEFAULT 'unknown',\n",
                "    model TEXT NOT NULL DEFAULT 'unknown',\n    extra_metric TEXT,\n".to_string(),
            ),
            (
                "changed-default",
                "    model TEXT NOT NULL DEFAULT 'unknown',\n",
                "    model TEXT NOT NULL DEFAULT 'other',\n".to_string(),
            ),
            (
                "changed-index-column",
                "CREATE INDEX IF NOT EXISTS idx_usage_session ON usage_events(session_id);",
                "CREATE INDEX IF NOT EXISTS idx_usage_session ON usage_events(command);"
                    .to_string(),
            ),
            (
                "added-check-constraint",
                "    command TEXT NOT NULL,\n",
                "    command TEXT NOT NULL CHECK(command <> ''),\n".to_string(),
            ),
            (
                "oversized-check-constraint",
                "    command TEXT NOT NULL,\n",
                oversized_constraint,
            ),
        ];
        for (label, needle, replacement) in cases {
            let case = temp.path().join(label);
            fs::create_dir_all(&case)?;
            let root = case.join("repository");
            fs::create_dir_all(&root)?;
            let db_path = case.join("projectatlas.db");
            write_evolved_schema_lookalike(&db_path, &root, needle, &replacement)?;
            let database_before = fs::read(&db_path)?;
            let inventory_before = directory_entry_names(&case)?;

            let Err(error) = preflight(&db_path, Some(&normalize_native_path_display(&root)))
            else {
                return Err(io::Error::other(format!(
                    "{label} evolved schema unexpectedly passed preflight"
                ))
                .into());
            };
            let DbError::SchemaShape {
                object,
                expected,
                found,
            } = &error
            else {
                return Err(io::Error::other(format!(
                    "{label} evolved schema returned the wrong error: {error}"
                ))
                .into());
            };
            if object.len() > SCHEMA_DIAGNOSTIC_OBJECT_MAX_BYTES
                || expected.len() > SCHEMA_DIAGNOSTIC_VALUE_MAX_BYTES
                || found.len() > SCHEMA_DIAGNOSTIC_VALUE_MAX_BYTES
            {
                return Err(io::Error::other(format!(
                    "{label} evolved schema returned an unbounded diagnostic"
                ))
                .into());
            }
            if label == "oversized-check-constraint"
                && !found.ends_with(SCHEMA_DIAGNOSTIC_TRUNCATION_SUFFIX)
            {
                return Err(io::Error::other(
                    "oversized evolved schema diagnostic was not truncated",
                )
                .into());
            }
            require_unchanged(&case, &db_path, &database_before, &inventory_before)?;
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
    fn ordinary_current_open_skips_full_scan_while_explicit_verify_checks_integrity()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let database = temp.path().join("projectatlas.db");
        drop(AtlasStore::open_for_project(&database, &root)?);

        let connection = Connection::open(&database)?;
        connection.pragma_update(None, "foreign_keys", false)?;
        connection.execute(
            "INSERT INTO purposes(node_id, source, status) VALUES(999, 'agent', 'approved')",
            [],
        )?;
        drop(connection);

        let writer = Connection::open(&database)?;
        writer.execute_batch("BEGIN IMMEDIATE")?;
        drop(AtlasStore::open_for_project(&database, &root)?);
        writer.execute_batch("ROLLBACK")?;

        if verify_current_integrity(&database, Some(&normalize_native_path_display(&root))).is_ok()
        {
            return Err(io::Error::other(
                "explicit verification accepted a foreign-key integrity failure",
            )
            .into());
        }
        Ok(())
    }

    #[test]
    fn current_schema_requires_complete_project_binding_without_repair()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let database = temp.path().join("projectatlas.db");
        let store = AtlasStore::open_for_project(&database, &root)?;
        store
            .connection
            .execute("DELETE FROM project_identity", [])?;
        store
            .connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        drop(store);
        let bytes_before = fs::read(&database)?;

        let Err(error) = AtlasStore::open_for_project(&database, &root) else {
            return Err(io::Error::other("bound database without identity was reopened").into());
        };
        if !matches!(error, DbError::ProjectInstanceIdentityMissing) {
            return Err(io::Error::other(format!(
                "missing identity returned the wrong error: {error}"
            ))
            .into());
        }
        if fs::read(&database)? != bytes_before {
            return Err(io::Error::other("ordinary open repaired the missing identity").into());
        }

        let orphan_database = temp.path().join("orphan-identity.db");
        let orphan = AtlasStore::open(&orphan_database)?;
        let orphan_identity = ProjectInstanceId::from_bytes([0x51; 16])?;
        orphan.connection.execute(
            "INSERT INTO project_identity(singleton, project_instance_id, active_generation)
             VALUES(1, ?1, 0)",
            [&orphan_identity.as_bytes()[..]],
        )?;
        orphan
            .connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        drop(orphan);
        let orphan_bytes = fs::read(&orphan_database)?;

        for result in [
            AtlasStore::open(&orphan_database),
            AtlasStore::open_read_only(&orphan_database),
        ] {
            let Err(error) = result else {
                return Err(
                    io::Error::other("unbound database accepted an orphan identity").into(),
                );
            };
            if !matches!(error, DbError::ProjectRootTransitionChanged { .. }) {
                return Err(io::Error::other(format!(
                    "orphan identity returned the wrong error: {error}"
                ))
                .into());
            }
        }
        if fs::read(&orphan_database)? != orphan_bytes {
            return Err(io::Error::other("rejected orphan identity changed the database").into());
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
            store.connection.query_row(
                "SELECT COUNT(*)
                 FROM symbol_relations INDEXED BY idx_symbol_import_alias_lookup
                 WHERE kind = 'imports'",
                [],
                |row| row.get::<_, i64>(0),
            )?;
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
            let migrated_usage = store.connection.query_row(
                "SELECT i.caller_label, i.owner, i.state, e.command
                 FROM usage_events AS e
                 JOIN usage_instances AS i USING(instance_row_id)",
                [],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )?;
            if migrated_usage
                != (
                    Some("session".to_string()),
                    "migrated_legacy".to_string(),
                    "sealed".to_string(),
                    "files".to_string(),
                )
            {
                return Err(io::Error::other(format!(
                    "schema {version} upgrade changed migrated telemetry identity or content"
                ))
                .into());
            }
            let retention = store.connection.query_row(
                "SELECT raw_rows, instance_rows, label_rows, dimension_rows,
                        baseline_rows, daily_rows
                 FROM usage_retention_state WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )?;
            if retention != (1, 1, 1, 2, 0, 0) {
                let mut statement = store.connection.prepare(
                    "SELECT token_savings_bucket, provider, overflow
                     FROM usage_bucket_dimensions ORDER BY dimension_id",
                )?;
                let dimensions = statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                return Err(io::Error::other(format!(
                    "schema {version} upgrade reconciled incorrect telemetry counters: {retention:?}; dimensions={dimensions:?}"
                ))
                .into());
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
    fn released_schema_eight_upgrade_preserves_token_impact_across_reopen()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let db_path = temp.path().join("projectatlas.db");
        let connection = Connection::open(&db_path)?;
        configure_writable(&connection)?;
        create_released_schema_eight(&connection)?;
        set_metadata(
            &connection,
            PROJECT_ROOT_KEY,
            &normalize_native_path_display(&root),
        )?;

        let events = [
            UsageEvent {
                session_id: "preserved-session".to_string(),
                command: "summary".to_string(),
                path: Some("src/lib.rs".to_string()),
                query: None,
                estimated_tokens_without_projectatlas: Some(1_000),
                estimated_tokens_with_projectatlas: Some(100),
                estimated_tokens_saved: Some(900),
                token_savings_bucket: "full_file_compression".to_string(),
                provider: "heuristic".to_string(),
                model: "unknown".to_string(),
                tokenizer_backend: "chars_div_4".to_string(),
                accuracy: "heuristic_estimate".to_string(),
                baseline_kind: "full_file".to_string(),
                confidence: "observed".to_string(),
                calculation_trace: "released-schema-observed".to_string(),
                accounting_layer: "observed_delta".to_string(),
                estimate_method: "heuristic_chars_or_bytes_div_ceil_4".to_string(),
                denominator_kind: "full_file".to_string(),
                baseline_identity: String::new(),
                baseline_fingerprint: String::new(),
                dedupe_scope: "event".to_string(),
            },
            UsageEvent {
                session_id: "preserved-session".to_string(),
                command: "files".to_string(),
                path: None,
                query: Some("src".to_string()),
                estimated_tokens_without_projectatlas: Some(600),
                estimated_tokens_with_projectatlas: Some(40),
                estimated_tokens_saved: Some(560),
                token_savings_bucket: "navigation_avoidance".to_string(),
                provider: "heuristic".to_string(),
                model: "unknown".to_string(),
                tokenizer_backend: "chars_div_4".to_string(),
                accuracy: "heuristic_estimate".to_string(),
                baseline_kind: "selected_candidates".to_string(),
                confidence: "inferred".to_string(),
                calculation_trace: "released-schema-modeled".to_string(),
                accounting_layer: "modeled_avoidance".to_string(),
                estimate_method: "heuristic_chars_or_bytes_div_ceil_4".to_string(),
                denominator_kind: "selected_candidates".to_string(),
                baseline_identity: "files:src".to_string(),
                baseline_fingerprint: "files:src:v1".to_string(),
                dedupe_scope: "session".to_string(),
            },
            UsageEvent {
                session_id: "preserved-session".to_string(),
                command: "files".to_string(),
                path: None,
                query: Some("src".to_string()),
                estimated_tokens_without_projectatlas: Some(600),
                estimated_tokens_with_projectatlas: Some(40),
                estimated_tokens_saved: Some(560),
                token_savings_bucket: "navigation_avoidance".to_string(),
                provider: "heuristic".to_string(),
                model: "unknown".to_string(),
                tokenizer_backend: "chars_div_4".to_string(),
                accuracy: "heuristic_estimate".to_string(),
                baseline_kind: "selected_candidates".to_string(),
                confidence: "inferred".to_string(),
                calculation_trace: "released-schema-modeled".to_string(),
                accounting_layer: "modeled_avoidance".to_string(),
                estimate_method: "heuristic_chars_or_bytes_div_ceil_4".to_string(),
                denominator_kind: "selected_candidates".to_string(),
                baseline_identity: "files:src".to_string(),
                baseline_fingerprint: "files:src:v1".to_string(),
                dedupe_scope: "session".to_string(),
            },
        ];
        let created_at = [
            "2026-06-15 10:00:00",
            "2026-06-16 10:00:00",
            "2026-07-01 10:00:00",
        ];
        let mut insert = connection.prepare_cached(
            "INSERT INTO usage_events(
                 session_id, command, path, query,
                 estimated_tokens_without_projectatlas,
                 estimated_tokens_with_projectatlas,
                 estimated_tokens_saved,
                 token_savings_bucket, provider, model, tokenizer_backend,
                 accuracy, baseline_kind, confidence, calculation_trace,
                 accounting_layer, estimate_method, denominator_kind,
                 baseline_identity, baseline_fingerprint, dedupe_scope, created_at
             ) VALUES(
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                 ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22
             )",
        )?;
        for (event, created_at) in events.iter().zip(created_at) {
            insert.execute(params![
                event.session_id,
                event.command,
                event.path,
                event.query,
                event
                    .estimated_tokens_without_projectatlas
                    .map(i64::try_from)
                    .transpose()?,
                event
                    .estimated_tokens_with_projectatlas
                    .map(i64::try_from)
                    .transpose()?,
                event
                    .estimated_tokens_saved
                    .map(i64::try_from)
                    .transpose()?,
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
                event.dedupe_scope,
                created_at,
            ])?;
        }
        drop(insert);
        drop(connection);

        let expected_overview = TokenOverview::from_events(&events);
        let expected_periods = vec![
            TokenTrendPeriod::from_buckets(
                "2026-06".to_string(),
                TokenOverview::from_events(&events[..2]).buckets,
            ),
            TokenTrendPeriod::from_buckets(
                "2026-07".to_string(),
                TokenOverview::from_events(&events[2..]).buckets,
            ),
        ];
        let store = AtlasStore::open_for_project(&db_path, &root)?;
        let migrated_overview = store.token_overview(Some("preserved-session"))?;
        let migrated_totals = (
            migrated_overview.calls,
            migrated_overview.estimated_without_projectatlas,
            migrated_overview.estimated_with_projectatlas,
            migrated_overview.estimated_saved,
            migrated_overview.tokens_avoided,
            migrated_overview.repeated_baselines_deduped,
            migrated_overview.likely_file_reads_avoided,
            &migrated_overview.buckets,
        );
        let expected_totals = (
            expected_overview.calls,
            expected_overview.estimated_without_projectatlas,
            expected_overview.estimated_with_projectatlas,
            expected_overview.estimated_saved,
            expected_overview.tokens_avoided,
            expected_overview.repeated_baselines_deduped,
            expected_overview.likely_file_reads_avoided,
            &expected_overview.buckets,
        );
        if migrated_totals != expected_totals {
            return Err(io::Error::other(
                "released schema-8 token overview changed during migration",
            )
            .into());
        }
        let migrated_trends =
            store.token_trends(Some("preserved-session"), TokenTrendWindow::Month)?;
        if migrated_trends.periods != expected_periods {
            return Err(io::Error::other(
                "released schema-8 token trends changed during migration",
            )
            .into());
        }
        drop(store);

        let reopened = AtlasStore::open_for_project(&db_path, &root)?;
        if reopened.token_overview(Some("preserved-session"))? != migrated_overview {
            return Err(io::Error::other("reopen changed the migrated token overview").into());
        }
        if reopened.token_trends(Some("preserved-session"), TokenTrendWindow::Month)?
            != migrated_trends
        {
            return Err(io::Error::other("reopen changed the migrated token trends").into());
        }
        Ok(())
    }

    #[test]
    fn lexical_schema_upgrade_preserves_identity_and_authored_purpose_only()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let database = temp.path().join("projectatlas.db");
        let store = AtlasStore::open_for_project(&database, &root)?;
        let project_identity = store.connection.query_row(
            "SELECT project_instance_id FROM project_identity WHERE singleton = 1",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        )?;
        store.connection.execute_batch(
            "DROP TABLE file_content_classifications;
             DROP INDEX idx_nodes_path_kind;
             DROP TABLE usage_instance_worktree_origins;
             DROP TABLE worktree_usage_aggregates;
             DROP TABLE worktree_registrations;
             DROP TABLE usage_aggregate_revisions;
             DROP TABLE project_root_identity;",
        )?;
        recreate_disposable_graph_projection(&store.connection, false)?;
        recreate_pre_selector_symbol_storage_for_test(&store.connection)?;
        store
            .connection
            .execute_batch("DROP INDEX idx_symbol_import_alias_lookup")?;
        store.connection.execute_batch(
            "
            INSERT INTO nodes(id, path, kind) VALUES(1, 'src/lib.rs', 'file');
            INSERT INTO purposes(node_id, purpose, source, status, updated_by)
                VALUES(1, 'Own the library.', 'agent', 'approved', 'agent');
            INSERT INTO graph_entities(
                entity_key, project_instance_id, canonical_identity, entity_kind, repository_path
            ) SELECT zeroblob(32), project_instance_id, 'derived-file', 'file', 'src/lib.rs'
                FROM project_identity WHERE singleton = 1;
            UPDATE project_identity SET active_generation = 7 WHERE singleton = 1;
            ",
        )?;
        set_metadata(
            &store.connection,
            SCHEMA_VERSION_KEY,
            &LEXICAL_SCHEMA_VERSION.to_string(),
        )?;
        set_metadata(&store.connection, INDEX_PUBLICATION_STATE_KEY, "complete")?;
        set_metadata(
            &store.connection,
            INDEX_PUBLICATION_FINGERPRINT_KEY,
            "lexical-publication",
        )?;
        set_metadata(&store.connection, INDEX_PUBLICATION_GENERATION_KEY, "7")?;
        drop(store);

        let expected_root = normalize_native_path_display(&root);
        let (preflight, _) = preflight(&database, Some(&expected_root))?;
        if preflight.state != SchemaState::UpgradeRequired
            || preflight.schema_version != Some(LEXICAL_SCHEMA_VERSION)
        {
            return Err(io::Error::other(
                "schema-15 preflight did not admit the released index shape",
            )
            .into());
        }
        let reopened = AtlasStore::open_for_project(&database, &root)?;
        reopened.connection.query_row(
            "SELECT COUNT(*)
             FROM symbol_relations INDEXED BY idx_symbol_import_alias_lookup
             WHERE kind = 'imports'",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        let migrated = reopened.connection.query_row(
            "SELECT project_instance_id, active_generation FROM project_identity
              WHERE singleton = 1",
            [],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)),
        )?;
        if migrated != (project_identity, 0) {
            return Err(io::Error::other(
                "schema-15 migration changed project identity or retained graph generation",
            )
            .into());
        }
        let purpose = reopened.connection.query_row(
            "SELECT purpose, source, status, updated_by FROM purposes WHERE node_id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?;
        if purpose
            != (
                "Own the library.".to_string(),
                "agent".to_string(),
                "approved".to_string(),
                "agent".to_string(),
            )
        {
            return Err(io::Error::other("schema-15 migration changed authored purpose").into());
        }
        let graph_rows =
            reopened
                .connection
                .query_row("SELECT COUNT(*) FROM graph_entities", [], |row| {
                    row.get::<_, i64>(0)
                })?;
        let publication_keys = reopened.connection.query_row(
            "SELECT COUNT(*) FROM metadata WHERE key IN (?1, ?2, ?3)",
            params![
                INDEX_PUBLICATION_STATE_KEY,
                INDEX_PUBLICATION_FINGERPRINT_KEY,
                INDEX_PUBLICATION_GENERATION_KEY,
            ],
            |row| row.get::<_, i64>(0),
        )?;
        if graph_rows != 0
            || publication_keys != 0
            || read_metadata(&reopened.connection, SCHEMA_VERSION_KEY)?
                != Some(SCHEMA_VERSION.to_string())
        {
            return Err(io::Error::other(
                "schema-15 migration retained disposable graph state or publication trust",
            )
            .into());
        }
        drop(reopened);
        drop(AtlasStore::open_for_project(&database, &root)?);
        Ok(())
    }

    #[test]
    fn schema_sixteen_upgrade_rolls_back_then_preserves_authored_state_and_reopens()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir(&root)?;
        let database = temp.path().join("projectatlas.db");
        let store = AtlasStore::open_for_project(&database, &root)?;
        let project_identity = store.connection.query_row(
            "SELECT project_instance_id FROM project_identity WHERE singleton = 1",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        )?;
        store.connection.execute_batch(
            "DROP TABLE file_content_classifications;
             DROP INDEX idx_nodes_path_kind;
             DROP TABLE usage_instance_worktree_origins;
             DROP TABLE worktree_usage_aggregates;
             DROP TABLE worktree_registrations;
             DROP TABLE usage_aggregate_revisions;",
        )?;
        recreate_disposable_graph_projection(&store.connection, false)?;
        recreate_pre_selector_symbol_storage_for_test(&store.connection)?;
        store.connection.execute_batch(
            "INSERT INTO nodes(id, path, kind, exists_now)
                VALUES(1, 'src/lib.rs', 'file', 1);
             INSERT INTO purposes(node_id, purpose, source, status, updated_by)
                VALUES(1, 'Own the library.', 'agent', 'approved', 'agent');
             INSERT INTO graph_entities(
                entity_key, project_instance_id, canonical_identity,
                entity_kind, repository_path
             ) SELECT zeroblob(32), project_instance_id, 'schema-16-file',
                      'file', 'src/lib.rs'
                 FROM project_identity WHERE singleton = 1;
             INSERT INTO symbols(
                 path, language, name, kind, signature, exported, documentation,
                 line_start, line_end, parent, parser, detail
             ) VALUES(
                 'src/lib.rs', 'rust', 'run', 'function', 'fn run()', 1,
                 'Run the library.', 4, 4, NULL, 'tree-sitter', NULL
             );
             UPDATE project_identity SET active_generation = 3 WHERE singleton = 1;",
        )?;
        store
            .connection
            .execute_batch("DROP TABLE project_root_identity")?;
        set_metadata(&store.connection, INDEX_PUBLICATION_STATE_KEY, "complete")?;
        set_metadata(
            &store.connection,
            INDEX_PUBLICATION_FINGERPRINT_KEY,
            "schema-16-publication",
        )?;
        set_metadata(&store.connection, INDEX_PUBLICATION_GENERATION_KEY, "3")?;
        set_metadata(
            &store.connection,
            SCHEMA_VERSION_KEY,
            &COMPACT_GRAPH_SCHEMA_VERSION.to_string(),
        )?;
        drop(store);

        let expected_root = normalize_native_path_display(&root);
        let (before, _) = preflight(&database, Some(&expected_root))?;
        if before.state != SchemaState::UpgradeRequired
            || before.schema_version != Some(COMPACT_GRAPH_SCHEMA_VERSION)
        {
            return Err(io::Error::other("schema-16 fixture was not admitted").into());
        }

        let connection = Connection::open(&database)?;
        configure_writable(&connection)?;
        let assert_schema_sixteen_state = |stage: &str| -> Result<(), Box<dyn Error>> {
            let state = connection.query_row(
                "SELECT
                    (SELECT value FROM metadata WHERE key = 'schema_version'),
                    (SELECT COUNT(*) FROM sqlite_schema
                      WHERE type = 'table' AND name = 'file_content_classifications'),
                    (SELECT COUNT(*) FROM graph_entities),
                    (SELECT COUNT(*) FROM purposes WHERE node_id = 1),
                    (SELECT COUNT(*) FROM pragma_table_info('symbols')
                      WHERE name LIKE 'source_%'),
                    (SELECT COUNT(*) FROM symbols WHERE path = 'src/lib.rs' AND name = 'run')",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )?;
            if state != (COMPACT_GRAPH_SCHEMA_VERSION.to_string(), 0, 1, 1, 0, 1) {
                return Err(io::Error::other(format!(
                    "schema-17 {stage} exposed partial state: {state:?}"
                ))
                .into());
            }
            Ok(())
        };

        let blocker = Connection::open(&database)?;
        configure_writable(&blocker)?;
        blocker.execute_batch("BEGIN IMMEDIATE")?;
        connection.busy_timeout(std::time::Duration::ZERO)?;
        let busy_failure = initialize(&connection, Some(&expected_root));
        blocker.execute_batch("ROLLBACK")?;
        let Err(DbError::Sqlite(busy_error)) = busy_failure else {
            return Err(io::Error::other("schema-17 busy migration did not fail in SQLite").into());
        };
        if !matches!(
            busy_error.sqlite_error_code(),
            Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
        ) {
            return Err(io::Error::other(format!(
                "schema-17 busy migration returned the wrong SQLite error: {busy_error}"
            ))
            .into());
        }
        assert_schema_sixteen_state("busy failure")?;

        let prefix_steps = Arc::new(AtomicUsize::new(0));
        let observed_prefix_steps = Arc::clone(&prefix_steps);
        connection.progress_handler(
            1,
            Some(move || {
                observed_prefix_steps.fetch_add(1, Ordering::Relaxed);
                false
            }),
        )?;
        connection.execute_batch("BEGIN IMMEDIATE")?;
        let prefix_result = (|| -> DbResult<()> {
            let inspected = inspect_connection(&connection, Some(&expected_root), false)?;
            if inspected.state != SchemaState::UpgradeRequired {
                return Err(DbError::SchemaPostcondition {
                    expected: SCHEMA_VERSION,
                });
            }
            validate_integrity(&connection)?;
            let _version = stored_schema_version(&connection)?;
            Ok(())
        })();
        connection.execute_batch("ROLLBACK")?;
        connection.progress_handler(0, None::<fn() -> bool>)?;
        prefix_result?;

        let interrupt_after = prefix_steps
            .load(Ordering::Relaxed)
            .saturating_sub(1)
            .max(1);
        let interrupted_steps = Arc::new(AtomicUsize::new(0));
        let observed_interrupted_steps = Arc::clone(&interrupted_steps);
        connection.progress_handler(
            1,
            Some(move || {
                observed_interrupted_steps.fetch_add(1, Ordering::Relaxed) == interrupt_after
            }),
        )?;
        let interrupted_failure = initialize(&connection, Some(&expected_root));
        connection.progress_handler(0, None::<fn() -> bool>)?;
        let interrupted_error = match interrupted_failure {
            Err(DbError::Sqlite(error)) => error,
            Err(error) => {
                return Err(io::Error::other(format!(
                    "schema-17 interrupted migration returned the wrong error: {error}"
                ))
                .into());
            }
            Ok(()) => {
                return Err(io::Error::other(
                    "schema-17 interrupted migration unexpectedly committed",
                )
                .into());
            }
        };
        if interrupted_error.sqlite_error_code() != Some(rusqlite::ErrorCode::OperationInterrupted)
            || interrupted_steps.load(Ordering::Relaxed) <= interrupt_after
        {
            return Err(io::Error::other(format!(
                "schema-17 interrupted migration returned the wrong SQLite evidence: error={interrupted_error} steps={} threshold={interrupt_after}",
                interrupted_steps.load(Ordering::Relaxed)
            ))
            .into());
        }
        assert_schema_sixteen_state("interruption")?;

        connection.execute_batch(&format!(
            "CREATE TEMP TRIGGER abort_schema_seventeen
             BEFORE UPDATE OF value ON metadata
             WHEN OLD.key = 'schema_version' AND NEW.value = '{SCHEMA_VERSION}'
             BEGIN SELECT RAISE(ABORT, 'injected schema-17 failure'); END;"
        ))?;
        let failure = match initialize(&connection, Some(&expected_root)) {
            Ok(()) => {
                return Err(
                    io::Error::other("injected schema-17 migration failure committed").into(),
                );
            }
            Err(error) => error,
        };
        if !matches!(failure, DbError::Sqlite(_)) {
            return Err(io::Error::other("schema-17 rollback returned the wrong error").into());
        }
        assert_schema_sixteen_state("constraint rollback")?;
        connection.execute_batch("DROP TRIGGER abort_schema_seventeen")?;
        initialize(&connection, Some(&expected_root))?;

        let migrated = connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'schema_version'),
                (SELECT COUNT(*) FROM file_content_classifications),
                (SELECT COUNT(*) FROM graph_entities),
                (SELECT COUNT(*) FROM metadata WHERE key IN (
                    'index_publication_state',
                    'index_publication_fingerprint',
                    'index_publication_generation'
                )),
                (SELECT active_generation FROM project_identity WHERE singleton = 1),
                (SELECT COUNT(*) FROM purposes
                  WHERE node_id = 1 AND purpose = 'Own the library.'
                    AND source = 'agent' AND status = 'approved'),
                (SELECT COUNT(*) FROM pragma_table_info('symbols')
                  WHERE name LIKE 'source_%'),
                (SELECT COUNT(*) FROM symbols
                  WHERE path = 'src/lib.rs' AND name = 'run'
                    AND source_byte_start IS NULL
                    AND source_byte_end IS NULL
                    AND source_column_start IS NULL
                    AND source_column_end IS NULL)",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )?;
        if migrated != (SCHEMA_VERSION.to_string(), 0, 0, 0, 0, 1, 4, 1) {
            return Err(io::Error::other(format!(
                "schema-17 migration changed its preservation boundary: {migrated:?}"
            ))
            .into());
        }
        let migrated_identity = connection.query_row(
            "SELECT project_instance_id FROM project_identity WHERE singleton = 1",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        )?;
        if migrated_identity != project_identity {
            return Err(io::Error::other("schema-17 migration changed project identity").into());
        }
        let foreign_key_errors = connection
            .prepare("PRAGMA foreign_key_check")?
            .query_map([], |_| Ok(()))?
            .collect::<Result<Vec<_>, _>>()?;
        if !foreign_key_errors.is_empty() {
            return Err(io::Error::other("schema-17 migration broke foreign keys").into());
        }
        drop(connection);

        let reopened = AtlasStore::open_for_project(&database, &root)?;
        let reopened_symbols = reopened.load_symbols(Some("src/lib.rs"), None, 10)?;
        if reopened_symbols.len() != 1 || reopened_symbols[0].source_selector.is_some() {
            return Err(io::Error::other(
                "schema-17 writable reopen did not preserve the selector-free symbol",
            )
            .into());
        }
        drop(reopened);
        let read_only = AtlasStore::open_read_only_for_project(&database, &root)?;
        let read_only_symbols = read_only.load_symbols(Some("src/lib.rs"), None, 10)?;
        if read_only_symbols.len() != 1 || read_only_symbols[0].source_selector.is_some() {
            return Err(io::Error::other(
                "schema-17 read-only reopen did not preserve the selector-free symbol",
            )
            .into());
        }
        drop(read_only);
        Ok(())
    }

    #[test]
    fn schema_seventeen_upgrade_preserves_main_telemetry_and_matches_fresh_schema()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir(&root)?;
        let database = temp.path().join("projectatlas.db");
        let store = AtlasStore::open_for_project(&database, &root)?;
        let project_identity = store.connection.query_row(
            "SELECT project_instance_id FROM project_identity WHERE singleton = 1",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        )?;
        store.connection.execute(
            "INSERT INTO usage_bucket_dimensions(
                token_savings_bucket, provider, model, tokenizer_backend, accuracy,
                baseline_kind, confidence, accounting_layer, estimate_method,
                denominator_kind, dedupe_scope, overflow
             ) VALUES('summary', 'offline', 'heuristic', 'chars-div-4', 'estimated',
                      'file', 'high', 'source', 'observed', 'tokens', 'file', 0)",
            [],
        )?;
        let dimension_id = store.connection.last_insert_rowid();
        store.connection.execute(
            "INSERT INTO usage_global_aggregates(
                project_instance_id, dimension_id, calls, estimated_without,
                estimated_with
             ) VALUES(?1, ?2, 7, 700, 70)",
            params![project_identity.as_slice(), dimension_id],
        )?;
        recreate_disposable_graph_projection(&store.connection, true)?;
        store.connection.execute_batch(
            "DROP TABLE usage_instance_worktree_origins;
             DROP TABLE worktree_usage_aggregates;
             DROP TABLE worktree_registrations;
             DROP TABLE usage_aggregate_revisions;
             DROP TABLE project_root_identity;",
        )?;
        set_metadata(
            &store.connection,
            SCHEMA_VERSION_KEY,
            &CLASSIFIED_GRAPH_SCHEMA_VERSION.to_string(),
        )?;
        drop(store);

        let expected_root = normalize_native_path_display(&root);
        let (before, _) = preflight(&database, Some(&expected_root))?;
        if before.state != SchemaState::UpgradeRequired
            || before.schema_version != Some(CLASSIFIED_GRAPH_SCHEMA_VERSION)
        {
            return Err(io::Error::other("schema-17 fixture was not admitted").into());
        }

        let migrated = AtlasStore::open_for_project(&database, &root)?;
        let state = migrated.connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'schema_version'),
                (SELECT revision FROM usage_aggregate_revisions
                  WHERE project_instance_id = ?1),
                (SELECT calls FROM usage_global_aggregates
                  WHERE project_instance_id = ?1 AND dimension_id = ?2),
                (SELECT COUNT(*) FROM worktree_registrations),
                (SELECT COUNT(*) FROM worktree_usage_aggregates)",
            params![project_identity.as_slice(), dimension_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )?;
        if state != (SCHEMA_VERSION.to_string(), 7, 7, 0, 0) {
            return Err(io::Error::other(format!(
                "schema-17 migration changed main telemetry or control defaults: {state:?}"
            ))
            .into());
        }
        if read_schema_contract(&migrated.connection)? != *schema_contract()? {
            return Err(io::Error::other("migrated and fresh schema contracts differ").into());
        }
        let foreign_key_errors = migrated
            .connection
            .prepare("PRAGMA foreign_key_check")?
            .query_map([], |_| Ok(()))?
            .collect::<Result<Vec<_>, _>>()?;
        if !foreign_key_errors.is_empty() {
            return Err(io::Error::other("schema-17 migration broke foreign keys").into());
        }
        Ok(())
    }

    #[test]
    fn schema_eighteen_upgrade_is_atomic_and_preserves_only_authored_graph_state()
    -> Result<(), Box<dyn Error>> {
        let predecessor_contract = worktree_control_predecessor_schema_contract()?;
        let predecessor_digest =
            blake3::hash(format!("{predecessor_contract:?}").as_bytes()).to_hex();
        if predecessor_digest.as_str() != RELEASED_SCHEMA_EIGHTEEN_CONTRACT_BLAKE3 {
            return Err(io::Error::other(format!(
                "captured schema-18 contract drifted: {predecessor_digest}"
            ))
            .into());
        }
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir(&root)?;
        let database = temp.path().join("projectatlas.db");
        let store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("schema-18 fixture identity is missing"))?;
        store.connection.execute(
            "INSERT INTO nodes(path, kind) VALUES('src/lib.rs', 'file')",
            [],
        )?;
        store.connection.execute(
            "INSERT INTO purposes(node_id, purpose, source, status, updated_by)
             SELECT id, 'Own the library.', 'agent', 'approved', 'schema-test'
               FROM nodes WHERE path = 'src/lib.rs'",
            [],
        )?;
        recreate_disposable_graph_projection(&store.connection, true)?;
        store.connection.execute(
            "UPDATE project_identity SET active_generation = 7 WHERE singleton = 1",
            [],
        )?;
        for limit in ["rows", "occurrences", "depth", "output_bytes"] {
            store.connection.execute(
                "INSERT INTO graph_coverage(
                    project_instance_id, scope_kind, scope_path, state,
                    total, covered, omitted, reason, reached_limit
                 ) VALUES(?1, 'path', ?2, 'partial', 2, 1, 1, 'graph limit', ?3)",
                params![
                    &project.as_bytes()[..],
                    format!("historical/{limit}.rs"),
                    limit,
                ],
            )?;
        }
        if store
            .connection
            .execute(
                "INSERT INTO graph_coverage(
                    project_instance_id, scope_kind, scope_path, state,
                    total, covered, omitted, reason, reached_limit
                 ) VALUES(?1, 'path', 'historical/nodes.rs', 'partial',
                          2, 1, 1, 'graph limit', 'nodes')",
                [&project.as_bytes()[..]],
            )
            .is_ok()
        {
            return Err(io::Error::other("schema-18 admitted a schema-19 graph limit").into());
        }
        for (key, value) in [
            (INDEX_PUBLICATION_STATE_KEY, "complete"),
            (INDEX_PUBLICATION_FINGERPRINT_KEY, "schema-18-fixture"),
            (INDEX_PUBLICATION_GENERATION_KEY, "7"),
            (SCHEMA_VERSION_KEY, "18"),
        ] {
            set_metadata(&store.connection, key, value)?;
        }
        store
            .connection
            .execute_batch("DROP TABLE project_root_identity")?;
        if read_schema_contract(&store.connection)?
            != *worktree_control_predecessor_schema_contract()?
        {
            return Err(io::Error::other("schema-18 fixture shape drifted").into());
        }
        drop(store);

        let expected_root = normalize_native_path_display(&root);
        let (before, _) = preflight(&database, Some(&expected_root))?;
        if before.state != SchemaState::UpgradeRequired
            || before.schema_version != Some(WORKTREE_CONTROL_SCHEMA_VERSION)
        {
            return Err(io::Error::other("exact schema-18 fixture was not admitted").into());
        }

        let connection = Connection::open(&database)?;
        configure_writable(&connection)?;
        connection.execute_batch(&format!(
            "CREATE TEMP TRIGGER abort_schema_nineteen
             BEFORE UPDATE OF value ON metadata
             WHEN OLD.key = 'schema_version' AND NEW.value = '{SCHEMA_VERSION}'
             BEGIN SELECT RAISE(ABORT, 'injected schema-19 failure'); END;"
        ))?;
        let failed = initialize(&connection, Some(&expected_root));
        if !matches!(failed, Err(DbError::Sqlite(_))) {
            return Err(io::Error::other(format!(
                "schema-19 injected failure returned the wrong result: {failed:?}"
            ))
            .into());
        }
        let rolled_back = connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'schema_version'),
                (SELECT project_instance_id FROM project_identity WHERE singleton = 1),
                (SELECT active_generation FROM project_identity WHERE singleton = 1),
                (SELECT COUNT(*) FROM purposes
                  WHERE purpose = 'Own the library.' AND source = 'agent'
                    AND status = 'approved' AND updated_by = 'schema-test'),
                (SELECT COUNT(*) FROM graph_coverage
                  WHERE reached_limit IN ('rows', 'occurrences', 'depth', 'output_bytes')
                    AND state = 'partial'),
                (SELECT COUNT(*) FROM metadata WHERE key IN (
                    'index_publication_state',
                    'index_publication_fingerprint',
                    'index_publication_generation'
                ))",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )?;
        if rolled_back
            != (
                WORKTREE_CONTROL_SCHEMA_VERSION.to_string(),
                project.as_bytes().to_vec(),
                7,
                1,
                4,
                3,
            )
            || read_schema_contract(&connection)?
                != *worktree_control_predecessor_schema_contract()?
        {
            return Err(io::Error::other(format!(
                "schema-19 failure exposed partial migration state: {rolled_back:?}"
            ))
            .into());
        }
        drop(connection);

        let mut migrated = AtlasStore::open_for_project(&database, &root)?;
        let migrated_state = migrated.connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'schema_version'),
                (SELECT project_instance_id FROM project_identity WHERE singleton = 1),
                (SELECT active_generation FROM project_identity WHERE singleton = 1),
                (SELECT COUNT(*) FROM purposes
                  WHERE purpose = 'Own the library.' AND source = 'agent'
                    AND status = 'approved' AND updated_by = 'schema-test'),
                (SELECT COUNT(*) FROM graph_coverage),
                (SELECT COUNT(*) FROM metadata WHERE key IN (
                    'index_publication_state',
                    'index_publication_fingerprint',
                    'index_publication_generation'
                ))",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )?;
        if migrated_state
            != (
                SCHEMA_VERSION.to_string(),
                project.as_bytes().to_vec(),
                0,
                1,
                0,
                0,
            )
            || read_schema_contract(&migrated.connection)? != *schema_contract()?
        {
            return Err(io::Error::other(format!(
                "schema-19 migration changed its authority boundary: {migrated_state:?}"
            ))
            .into());
        }

        let transaction = migrated.connection.transaction()?;
        transaction.execute(
            "UPDATE project_identity SET active_generation = 1 WHERE singleton = 1",
            [],
        )?;
        for kind in GraphLimitKind::ALL {
            transaction.execute(
                "INSERT INTO graph_coverage(
                    project_instance_id, scope_kind, scope_path, state,
                    total, covered, omitted, reason, reached_limit
                 ) VALUES(?1, 'path', ?2, 'partial', 2, 1, 1, 'graph limit', ?3)",
                params![
                    &project.as_bytes()[..],
                    format!("limits/{}.rs", kind.as_str()),
                    kind.as_str(),
                ],
            )?;
        }
        set_metadata(&transaction, INDEX_PUBLICATION_STATE_KEY, "complete")?;
        set_metadata(
            &transaction,
            INDEX_PUBLICATION_FINGERPRINT_KEY,
            "schema-19-limit-round-trip",
        )?;
        set_metadata(&transaction, INDEX_PUBLICATION_GENERATION_KEY, "1")?;
        transaction.commit()?;

        for kind in GraphLimitKind::ALL {
            let coverage = migrated.repository_graph_coverage(
                project,
                &CoverageScope::Path {
                    path: RepositoryNodePath::new(Path::new(&format!(
                        "limits/{}.rs",
                        kind.as_str()
                    )))?,
                },
                1,
            )?;
            if coverage.rows.len() != 1 || coverage.rows[0].reached_limit() != Some(kind) {
                return Err(io::Error::other(format!(
                    "migrated schema did not round-trip graph limit {}",
                    kind.as_str()
                ))
                .into());
            }
        }
        drop(migrated);

        let reopened = AtlasStore::open_read_only_for_project(&database, &root)?;
        if read_schema_contract(&reopened.connection)? != *schema_contract()? {
            return Err(io::Error::other("reopened schema-19 contract drifted").into());
        }
        let last = GraphLimitKind::OutputBytes;
        let coverage = reopened.repository_graph_coverage(
            project,
            &CoverageScope::Path {
                path: RepositoryNodePath::new(Path::new("limits/output_bytes.rs"))?,
            },
            1,
        )?;
        if coverage.rows.len() != 1 || coverage.rows[0].reached_limit() != Some(last) {
            return Err(io::Error::other("reopen changed migrated graph-limit rows").into());
        }
        Ok(())
    }

    #[test]
    fn schema_nineteen_upgrade_repairs_native_root_atomically_and_retries()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("schema-19-root");
        let wrong_root = temp.path().join("schema-19-wrong");
        let regular_root = temp.path().join("schema-19-file");
        let missing_root = temp.path().join("schema-19-missing");
        fs::create_dir(&root)?;
        fs::create_dir(&wrong_root)?;
        fs::write(&regular_root, b"not a project directory")?;
        let database = temp.path().join("schema-19.db");

        let store = AtlasStore::open_for_project(&database, &root)?;
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("schema-19 fixture identity is missing"))?;
        store.connection.execute(
            "INSERT INTO nodes(path, kind) VALUES('src/lib.rs', 'file')",
            [],
        )?;
        store.connection.execute(
            "INSERT INTO purposes(node_id, purpose, source, status, updated_by)
             SELECT id, 'Schema 19 authored purpose', 'agent', 'approved', 'schema-test'
               FROM nodes WHERE path = 'src/lib.rs'",
            [],
        )?;
        store.record_usage(&usage_from_estimates(
            "schema-19-fixture",
            "migration",
            Some("src/lib.rs".to_string()),
            None,
            10,
            4,
        ))?;
        store.connection.execute(
            "UPDATE project_identity SET active_generation = 7 WHERE singleton = 1",
            [],
        )?;
        store.connection.execute_batch(
            "DROP TABLE project_root_identity;
             UPDATE metadata SET value = '19' WHERE key = 'schema_version';",
        )?;
        if read_schema_contract(&store.connection)?
            != *canonical_root_predecessor_schema_contract()?
        {
            return Err(io::Error::other("schema-19 fixture shape drifted").into());
        }
        drop(store);

        let expected_identity = CanonicalProjectRoot::from_path(&root)?;
        let expected_root = expected_identity.display_string()?;
        let predecessor = AtlasStore::open_read_only(&database);
        if !matches!(
            predecessor,
            Err(DbError::SchemaVersion {
                found: CANONICAL_ROOT_PREDECESSOR_SCHEMA_VERSION,
                expected: SCHEMA_VERSION,
            })
        ) {
            return Err(io::Error::other(
                "schema-19 read-only open did not refuse predecessor state",
            )
            .into());
        }
        let predecessor_project = AtlasStore::open_read_only_for_project(&database, &root);
        if !matches!(
            predecessor_project,
            Err(DbError::SchemaVersion {
                found: CANONICAL_ROOT_PREDECESSOR_SCHEMA_VERSION,
                expected: SCHEMA_VERSION,
            })
        ) {
            return Err(io::Error::other(
                "schema-19 project read-only open did not refuse predecessor state",
            )
            .into());
        }

        for (label, candidate, expected_error) in [
            (
                "wrong root",
                wrong_root.as_path(),
                "schema-19 wrong-root admission changed the database",
            ),
            (
                "missing root",
                missing_root.as_path(),
                "schema-19 missing-root admission changed the database",
            ),
            (
                "regular-file root",
                regular_root.as_path(),
                "schema-19 regular-file admission changed the database",
            ),
        ] {
            let before = fs::read(&database)?;
            let result = AtlasStore::open_for_project(&database, candidate);
            if result.is_ok() {
                return Err(io::Error::other(format!("schema-19 {label} was admitted")).into());
            }
            if fs::read(&database)? != before {
                return Err(io::Error::other(expected_error).into());
            }
        }

        let connection = Connection::open(&database)?;
        configure_writable(&connection)?;
        let before_failure = fs::read(&database)?;
        let wal_path = sqlite_sidecar_path(&database, "-wal");
        let wal_before = fs::read(&wal_path).ok();
        connection.execute_batch(
            "CREATE TEMP TRIGGER fail_schema_19_root_row
             BEFORE UPDATE OF value ON metadata
             WHEN OLD.key = 'project_root'
             BEGIN SELECT RAISE(ABORT, 'injected schema-19 root-row failure'); END;",
        )?;
        let failed = initialize_with_project_root(
            &connection,
            Some(&expected_root),
            Some(&expected_identity),
        );
        if !matches!(failed, Err(DbError::Sqlite(_))) {
            return Err(io::Error::other(format!(
                "schema-19 root-row failure returned the wrong result: {failed:?}"
            ))
            .into());
        }
        let rolled_back = connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'schema_version'),
                (SELECT project_instance_id FROM project_identity WHERE singleton = 1),
                (SELECT active_generation FROM project_identity WHERE singleton = 1),
                (SELECT purpose FROM purposes WHERE purpose = 'Schema 19 authored purpose'),
                (SELECT COUNT(*) FROM usage_events),
                (SELECT COUNT(*) FROM usage_instances),
                (SELECT COUNT(*) FROM sqlite_master WHERE name = 'project_root_identity')",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )?;
        if rolled_back
            != (
                CANONICAL_ROOT_PREDECESSOR_SCHEMA_VERSION.to_string(),
                project.as_bytes().to_vec(),
                7,
                "Schema 19 authored purpose".to_string(),
                1,
                1,
                0,
            )
            || fs::read(&database)? != before_failure
            || fs::read(&wal_path).ok() != wal_before
            || read_schema_contract(&connection)? != *canonical_root_predecessor_schema_contract()?
        {
            return Err(io::Error::other(
                "schema-19 root-row failure exposed partial migration state",
            )
            .into());
        }
        connection.execute_batch("DROP TRIGGER fail_schema_19_root_row")?;
        initialize_with_project_root(&connection, Some(&expected_root), Some(&expected_identity))?;
        drop(connection);

        let migrated = AtlasStore::open_for_project(&database, &root)?;
        let migrated_state = migrated.connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'schema_version'),
                (SELECT project_instance_id FROM project_identity WHERE singleton = 1),
                (SELECT active_generation FROM project_identity WHERE singleton = 1),
                (SELECT purpose FROM purposes WHERE purpose = 'Schema 19 authored purpose'),
                (SELECT COUNT(*) FROM usage_events),
                (SELECT COUNT(*) FROM usage_instances),
                (SELECT codec_version FROM project_root_identity WHERE singleton = 1)",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )?;
        if migrated_state
            != (
                SCHEMA_VERSION.to_string(),
                project.as_bytes().to_vec(),
                7,
                "Schema 19 authored purpose".to_string(),
                1,
                1,
                i64::from(projectatlas_core::project_root::CANONICAL_PROJECT_ROOT_CODEC_VERSION),
            )
            || migrated.project_root_identity()? != Some(expected_identity.clone())
        {
            return Err(io::Error::other(
                "schema-19 retry changed durable identity or authored state",
            )
            .into());
        }
        drop(migrated);
        let reopened = AtlasStore::open_read_only_for_project(&database, &root)?;
        if reopened.project_root_identity()? != Some(expected_identity)
            || read_schema_contract(&reopened.connection)? != *schema_contract()?
        {
            return Err(io::Error::other(
                "schema-19 reopened state did not retain current contract",
            )
            .into());
        }
        Ok(())
    }

    #[test]
    fn schema_nineteen_conventional_copy_refuses_historical_rebinding() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root_a = temp.path().join("root-a");
        let root_b = temp.path().join("root-b");
        fs::create_dir_all(root_a.join(".projectatlas"))?;
        fs::create_dir_all(root_b.join(".projectatlas"))?;
        let database_a = root_a.join(".projectatlas/projectatlas.db");
        let database_b = root_b.join(".projectatlas/projectatlas.db");

        let store = AtlasStore::open_for_project(&database_a, &root_a)?;
        store.connection.execute(
            "INSERT INTO nodes(path, kind) VALUES('src/lib.rs', 'file')",
            [],
        )?;
        store.connection.execute(
            "INSERT INTO purposes(node_id, purpose, source, status, updated_by)
             SELECT id, 'authored purpose', 'agent', 'approved', 'schema-test'
               FROM nodes WHERE path = 'src/lib.rs'",
            [],
        )?;
        store.record_usage(&usage_from_estimates(
            "conventional-copy",
            "migration",
            Some("src/lib.rs".to_string()),
            None,
            10,
            4,
        ))?;
        store.connection.execute(
            "UPDATE project_identity SET active_generation = 9 WHERE singleton = 1",
            [],
        )?;
        store.connection.execute_batch(
            "DROP TABLE project_root_identity;
             UPDATE metadata SET value = '19' WHERE key = 'schema_version';
             PRAGMA wal_checkpoint(TRUNCATE);",
        )?;
        drop(store);

        let source_before = fs::read(&database_a)?;
        let source_inventory = directory_entry_names(&root_a)?;
        fs::copy(&database_a, &database_b)?;
        let destination_before = fs::read(&database_b)?;
        let destination_inventory = directory_entry_names(&root_b)?;
        let result = AtlasStore::open_for_project(&database_b, &root_b);
        if !matches!(result, Err(DbError::ProjectRootMismatch { .. })) {
            return Err(io::Error::other(
                "copied conventional schema-19 database was admitted under a new root",
            )
            .into());
        }
        require_unchanged(&root_a, &database_a, &source_before, &source_inventory)?;
        require_unchanged(
            &root_b,
            &database_b,
            &destination_before,
            &destination_inventory,
        )?;

        let connection = Connection::open(&database_b)?;
        let preserved = connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'schema_version'),
                (SELECT COUNT(*) FROM purposes WHERE purpose = 'authored purpose'),
                (SELECT COUNT(*) FROM usage_events),
                (SELECT active_generation FROM project_identity WHERE singleton = 1),
                (SELECT COUNT(*) FROM sqlite_master WHERE name = 'project_root_identity')",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )?;
        if preserved != ("19".to_string(), 1, 1, 9, 0) {
            return Err(io::Error::other(
                "copied conventional predecessor changed durable state while refusing",
            )
            .into());
        }
        Ok(())
    }

    #[test]
    fn schema_nineteen_unicode_replacement_root_is_intentionally_ambiguous()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("unicode-�-root");
        fs::create_dir_all(&root)?;
        let database = temp.path().join("unicode-replacement.db");
        let store = AtlasStore::open_for_project(&database, &root)?;
        store.connection.execute_batch(
            "DROP TABLE project_root_identity;
             UPDATE metadata SET value = '19' WHERE key = 'schema_version';",
        )?;
        drop(store);

        let before = fs::read(&database)?;
        let inventory = directory_entry_names(temp.path())?;
        if AtlasStore::open_for_project(&database, &root).is_ok() {
            return Err(io::Error::other(
                "genuine Unicode replacement-character predecessor was admitted",
            )
            .into());
        }
        require_unchanged(temp.path(), &database, &before, &inventory)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn schema_nineteen_recovery_rejects_lossy_conventional_root_without_authority()
    -> Result<(), Box<dyn Error>> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        fn create_predecessor(database: &Path, root: &Path) -> Result<(), Box<dyn Error>> {
            fs::create_dir_all(root)?;
            if let Some(parent) = database.parent() {
                fs::create_dir_all(parent)?;
            }
            let store = AtlasStore::open_for_project(database, root)?;
            store.connection.execute_batch(
                "DROP TABLE project_root_identity;
                 UPDATE metadata SET value = '19' WHERE key = 'schema_version';",
            )?;
            Ok(())
        }

        let temp = tempfile::tempdir()?;
        let raw_root = temp
            .path()
            .join(OsString::from_vec(b"raw-root-\x80".to_vec()));
        let raw_database = raw_root.join(".projectatlas/projectatlas.db");
        create_predecessor(&raw_database, &raw_root)?;
        let raw_database_before = fs::read(&raw_database)?;
        let raw_inventory = directory_entry_names(&raw_root)?;
        if AtlasStore::open_for_project(&raw_database, &raw_root).is_ok() {
            return Err(io::Error::other(
                "lossy conventional predecessor was admitted without historical authority",
            )
            .into());
        }
        require_unchanged(
            &raw_root,
            &raw_database,
            &raw_database_before,
            &raw_inventory,
        )?;

        let replacement_root = PathBuf::from(raw_root.to_string_lossy().into_owned());
        fs::create_dir_all(replacement_root.join(".projectatlas"))?;
        let replacement_database = replacement_root.join(".projectatlas/projectatlas.db");
        fs::copy(&raw_database, &replacement_database)?;
        let replacement_before = fs::read(&replacement_database)?;
        let replacement_inventory = directory_entry_names(&replacement_root)?;
        if AtlasStore::open_for_project(&replacement_database, &replacement_root).is_ok() {
            return Err(io::Error::other(
                "copied predecessor beneath a replacement-character sibling was admitted",
            )
            .into());
        }
        require_unchanged(
            &replacement_root,
            &replacement_database,
            &replacement_before,
            &replacement_inventory,
        )?;

        let ambiguous_root = temp
            .path()
            .join(OsString::from_vec(b"ambiguous-root-\x81".to_vec()));
        let ambiguous_database = temp.path().join("custom-predecessor.db");
        create_predecessor(&ambiguous_database, &ambiguous_root)?;
        let ambiguous_before = fs::read(&ambiguous_database)?;
        let ambiguous_replacement = PathBuf::from(ambiguous_root.to_string_lossy().into_owned());
        fs::create_dir_all(&ambiguous_replacement)?;
        let ambiguous_inventory = directory_entry_names(temp.path())?;
        if AtlasStore::open_for_project(&ambiguous_database, &ambiguous_root).is_ok() {
            return Err(io::Error::other(
                "ambiguous custom predecessor was admitted without native authority",
            )
            .into());
        }
        if AtlasStore::open_for_project(&ambiguous_database, &ambiguous_replacement).is_ok() {
            return Err(io::Error::other(
                "ambiguous custom predecessor was admitted under a replacement sibling",
            )
            .into());
        }
        require_unchanged(
            temp.path(),
            &ambiguous_database,
            &ambiguous_before,
            &ambiguous_inventory,
        )?;
        if fs::read(&raw_database)? != raw_database_before {
            return Err(io::Error::other(
                "rejected conventional predecessor changed durable state",
            )
            .into());
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn schema_nineteen_rootless_open_rejects_ambiguous_root_without_mutation()
    -> Result<(), Box<dyn Error>> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        fn create_predecessor(database: &Path, root: &Path) -> Result<(), Box<dyn Error>> {
            fs::create_dir_all(root)?;
            if let Some(parent) = database.parent() {
                fs::create_dir_all(parent)?;
            }
            let store = AtlasStore::open_for_project(database, root)?;
            store.connection.execute_batch(
                "DROP TABLE project_root_identity;
                 UPDATE metadata SET value = '19' WHERE key = 'schema_version';",
            )?;
            Ok(())
        }

        let temp = tempfile::tempdir()?;
        let raw_root = temp
            .path()
            .join(OsString::from_vec(b"rootless-raw-\x80".to_vec()));
        let raw_database = raw_root.join(".projectatlas/projectatlas.db");
        create_predecessor(&raw_database, &raw_root)?;
        let raw_database_parent = raw_database
            .parent()
            .ok_or_else(|| io::Error::other("raw predecessor has no parent"))?;
        let raw_before = fs::read(&raw_database)?;
        let raw_inventory = directory_entry_names(raw_database_parent)?;
        if !matches!(
            AtlasStore::open(&raw_database),
            Err(DbError::ProjectRootIdentityMissing)
        ) {
            return Err(
                "rootless raw predecessor was not refused before writable admission".into(),
            );
        }
        require_unchanged(
            raw_database_parent,
            &raw_database,
            &raw_before,
            &raw_inventory,
        )?;

        let replacement_root = PathBuf::from(raw_root.to_string_lossy().into_owned());
        let replacement_database = replacement_root.join(".projectatlas/projectatlas.db");
        let replacement_database_parent = replacement_database
            .parent()
            .ok_or_else(|| io::Error::other("replacement predecessor has no parent"))?;
        fs::create_dir_all(replacement_database_parent)?;
        fs::copy(&raw_database, &replacement_database)?;
        let replacement_before = fs::read(&replacement_database)?;
        let replacement_inventory = directory_entry_names(replacement_database_parent)?;
        if !matches!(
            AtlasStore::open(&replacement_database),
            Err(DbError::ProjectRootIdentityMissing)
        ) {
            return Err("rootless replacement predecessor was not refused".into());
        }
        require_unchanged(
            replacement_database_parent,
            &replacement_database,
            &replacement_before,
            &replacement_inventory,
        )?;

        let utf8_root = temp.path().join("rootless-utf8");
        let utf8_database = temp.path().join("rootless-utf8.db");
        create_predecessor(&utf8_database, &utf8_root)?;
        let expected = CanonicalProjectRoot::from_path(&utf8_root)?;
        let migrated = AtlasStore::open(&utf8_database)?;
        if migrated.project_root_identity()? != Some(expected) {
            return Err(
                "ordinary UTF-8 rootless predecessor did not retain native identity".into(),
            );
        }
        Ok(())
    }

    #[test]
    fn resolution_schema_migrates_parser_provenance_without_weakening_existing_facts()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let database = temp.path().join("projectatlas.db");
        let connection = Connection::open(&database)?;
        configure_writable(&connection)?;
        connection.execute_batch(BASE_SCHEMA_SQL)?;
        create_graph_schema(&connection, GraphSchemaShape::Compact)?;
        connection.execute_batch(RESOLUTION_KEY_SCHEMA_SQL)?;
        connection.execute_batch(TELEMETRY_STORAGE_SCHEMA_SQL)?;
        connection.execute_batch(CURRENT_USAGE_SCHEMA_SQL)?;
        crate::telemetry::initialize_empty_storage(&connection)?;
        set_metadata(
            &connection,
            SCHEMA_VERSION_KEY,
            &RESOLUTION_SCHEMA_VERSION.to_string(),
        )?;
        set_metadata(&connection, INDEX_PUBLICATION_STATE_KEY, "complete")?;
        set_metadata(
            &connection,
            INDEX_PUBLICATION_FINGERPRINT_KEY,
            "resolution-schema-publication",
        )?;
        set_metadata(&connection, INDEX_PUBLICATION_GENERATION_KEY, "4")?;
        connection.execute_batch(
            "INSERT INTO source_parse_metadata(
                 path, language, parser, symbol_count, relation_count, updated_at
             ) VALUES(
                 'src/lib.rs', 'rust', 'tree-sitter', 1, 1, '2026-01-02 03:04:05'
             );
             INSERT INTO symbols(
                 path, language, name, kind, signature, line_start, line_end, parser
             ) VALUES(
                 'src/lib.rs', 'rust', 'run', 'function', 'fn run()', 1, 1, 'tree-sitter'
             );
             INSERT INTO symbol_relations(
                 path, source_name, target_name, kind, line, context, parser
             ) VALUES(
                 'src/lib.rs', 'run', 'helper', 'calls', 1, 'helper()', 'tree-sitter'
             );",
        )?;
        drop(connection);

        let store = AtlasStore::open(&database)?;
        let provenance = store.connection.query_row(
            "SELECT source_parser, fact_parser, symbol_count, relation_count, updated_at
               FROM source_parse_metadata WHERE path = 'src/lib.rs'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )?;
        if provenance
            != (
                "tree-sitter".to_string(),
                "tree-sitter".to_string(),
                1,
                1,
                "2026-01-02 03:04:05".to_string(),
            )
        {
            return Err(io::Error::other(format!(
                "schema-12 parser provenance changed during migration: {provenance:?}"
            ))
            .into());
        }
        if read_metadata(&store.connection, SCHEMA_VERSION_KEY)? != Some(SCHEMA_VERSION.to_string())
            || read_metadata(&store.connection, INDEX_PUBLICATION_STATE_KEY)?.is_some()
            || read_metadata(&store.connection, INDEX_PUBLICATION_FINGERPRINT_KEY)?.is_some()
            || read_metadata(&store.connection, INDEX_PUBLICATION_GENERATION_KEY)?.is_some()
        {
            return Err(io::Error::other(
                "parser-provenance migration did not invalidate rebuilt graph publication trust",
            )
            .into());
        }
        for index in [
            "idx_source_parse_metadata_source_parser_path",
            "idx_source_parse_metadata_fact_parser_path",
            "idx_graph_coverage_discovery_state",
            "idx_graph_coverage_discovery_reason",
        ] {
            let exists = store.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'index' AND name = ?1)",
                [index],
                |row| row.get::<_, bool>(0),
            )?;
            if !exists {
                return Err(io::Error::other(format!(
                    "coverage-discovery migration omitted {index}"
                ))
                .into());
            }
        }
        Ok(())
    }

    #[test]
    fn telemetry_schema_migrates_resolution_keys_and_legacy_purpose_state_atomically()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let db_path = temp.path().join("projectatlas.db");
        let connection = Connection::open(&db_path)?;
        configure_writable(&connection)?;
        connection.execute_batch(BASE_SCHEMA_SQL)?;
        create_graph_schema(&connection, GraphSchemaShape::Compact)?;
        connection.execute_batch(TELEMETRY_STORAGE_SCHEMA_SQL)?;
        connection.execute_batch(CURRENT_USAGE_SCHEMA_SQL)?;
        crate::telemetry::initialize_empty_storage(&connection)?;
        set_metadata(
            &connection,
            PROJECT_ROOT_KEY,
            &normalize_native_path_display(&root),
        )?;
        set_metadata(
            &connection,
            SCHEMA_VERSION_KEY,
            &TELEMETRY_SCHEMA_VERSION.to_string(),
        )?;
        set_metadata(&connection, INDEX_PUBLICATION_STATE_KEY, "complete")?;
        set_metadata(
            &connection,
            INDEX_PUBLICATION_FINGERPRINT_KEY,
            "telemetry-schema-publication",
        )?;
        set_metadata(&connection, INDEX_PUBLICATION_GENERATION_KEY, "7")?;
        crate::project_identity::ensure_project_identity(&connection)?;
        connection.execute_batch(
            "INSERT INTO nodes(id, path, kind) VALUES(1, 'src/lib.rs', 'file');
             INSERT INTO purposes(node_id, purpose, source, status, updated_at, updated_by)
                 VALUES(
                     1,
                     'Own the library.',
                     'agent',
                     'stale',
                     '2026-01-02 03:04:05',
                     'review-agent'
                 );",
        )?;
        drop(connection);

        let store = AtlasStore::open_for_project(&db_path, &root)?;
        let objects = store.connection.query_row(
            "SELECT COUNT(*) FROM sqlite_schema
              WHERE type = 'table' AND name IN (
                  'graph_resolution_keys',
                  'graph_entity_exports',
                  'graph_relation_dependencies'
              )",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        if objects != 3 {
            return Err(io::Error::other("resolution-key migration is incomplete").into());
        }
        if read_metadata(&store.connection, SCHEMA_VERSION_KEY)? != Some(SCHEMA_VERSION.to_string())
        {
            return Err(io::Error::other("resolution-key migration version is stale").into());
        }
        if read_metadata(&store.connection, INDEX_PUBLICATION_STATE_KEY)?.is_some()
            || read_metadata(&store.connection, INDEX_PUBLICATION_FINGERPRINT_KEY)?.is_some()
            || read_metadata(&store.connection, INDEX_PUBLICATION_GENERATION_KEY)?.is_some()
        {
            return Err(io::Error::other("migration retained stale publication trust").into());
        }
        let authored = store.connection.query_row(
            "SELECT purpose, source, status, updated_at, updated_by
               FROM purposes WHERE node_id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )?;
        if authored
            != (
                "Own the library.".to_string(),
                "agent".to_string(),
                "approved".to_string(),
                "2026-01-02 03:04:05".to_string(),
                Some("review-agent".to_string()),
            )
        {
            return Err(io::Error::other(format!(
                "migration changed authored purpose fields: {authored:?}"
            ))
            .into());
        }
        drop(store);
        drop(AtlasStore::open_read_only_for_project(&db_path, &root)?);
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
    fn required_index_drift_is_refused_without_mutation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        for missing_index in [
            "idx_graph_relations_target_kind",
            SYMBOL_RELATION_LOOKUP_INDEX_NAME,
        ] {
            let case = temp.path().join(missing_index);
            let root = case.join("repository");
            fs::create_dir_all(&root)?;
            let db_path = case.join("projectatlas.db");
            drop(AtlasStore::open_for_project(&db_path, &root)?);
            {
                let connection = Connection::open(&db_path)?;
                connection.execute_batch(&format!(
                    "DROP INDEX {missing_index};
                     PRAGMA wal_checkpoint(TRUNCATE);"
                ))?;
            }
            let database_before = fs::read(&db_path)?;
            let inventory_before = directory_entry_names(&case)?;
            let Err(error) = AtlasStore::open_for_project(&db_path, &root) else {
                return Err(io::Error::other(format!(
                    "missing required index {missing_index} unexpectedly passed preflight"
                ))
                .into());
            };
            if !matches!(error, DbError::SchemaShape { .. }) {
                return Err(io::Error::other(format!(
                    "missing required index {missing_index} returned the wrong error"
                ))
                .into());
            }
            require_unchanged(&case, &db_path, &database_before, &inventory_before)?;
        }
        Ok(())
    }

    #[test]
    fn released_schema_layouts_reject_wrong_root_without_migration_or_rebinding()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let layouts: [(&str, fn(&Path, &Path) -> Result<(), Box<dyn Error>>); 2] = [
            ("fresh-v0.3.26", write_schema_eight_fixture),
            (
                "evolved-v0.3.11-to-v0.3.26",
                write_evolved_schema_eight_fixture,
            ),
        ];
        for (label, write_fixture) in layouts {
            let case = temp.path().join(label);
            let root = case.join("root-a");
            let other = case.join("root-b");
            fs::create_dir_all(&root)?;
            fs::create_dir_all(&other)?;
            let db_path = case.join("projectatlas.db");
            write_fixture(&db_path, &root)?;
            let database_before = fs::read(&db_path)?;
            let inventory_before = directory_entry_names(&case)?;

            let Err(error) = AtlasStore::open_for_project(&db_path, &other) else {
                return Err(io::Error::other(format!(
                    "{label} wrong root unexpectedly migrated or rebound the database"
                ))
                .into());
            };
            match error {
                DbError::ProjectRootMismatch { expected, found }
                    if expected == normalize_native_path_display(&other)
                        && found == normalize_native_path_display(&root) => {}
                other => {
                    return Err(io::Error::other(format!(
                        "{label} wrong root returned the wrong error: {other}"
                    ))
                    .into());
                }
            }
            require_unchanged(&case, &db_path, &database_before, &inventory_before)?;
        }
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
    fn current_writer_revalidates_binding_captured_by_preflight() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("root-a");
        let replacement_root = temp.path().join("root-b");
        fs::create_dir_all(&root)?;
        fs::create_dir_all(&replacement_root)?;
        let db_path = temp.path().join("projectatlas.db");
        drop(AtlasStore::open_for_project(&db_path, &root)?);

        let expected_root = crate::normalize_metadata_path(&root);
        let replacement_root = crate::normalize_metadata_path(&replacement_root);
        let (preflight, location) = preflight(&db_path, Some(&expected_root))?;
        let transition = Connection::open(&db_path)?;
        crate::set_metadata(&transition, PROJECT_ROOT_KEY, &replacement_root)?;
        drop(transition);

        let writer = crate::sqlite_profile::open_writable_connection(
            &db_path,
            crate::writable_open_flags(preflight.state, location.database_exists),
            &location,
            crate::SQLITE_BUSY_TIMEOUT,
            crate::writable_journal_policy(preflight.state),
        )?;
        let Err(error) =
            revalidate_current_binding(&writer, preflight.project_root.as_deref(), true)
        else {
            return Err(io::Error::other("writer accepted a replaced project binding").into());
        };
        if !matches!(error, DbError::ProjectRootMismatch { .. }) {
            return Err(io::Error::other("binding race returned the wrong error").into());
        }
        if read_metadata(&writer, PROJECT_ROOT_KEY)? != Some(replacement_root) {
            return Err(io::Error::other("rejected binding changed the database").into());
        }
        Ok(())
    }

    #[test]
    fn predecessor_writer_revalidates_legacy_binding_inside_transaction()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root_a = temp.path().join("predecessor-root-a");
        let root_b = temp.path().join("predecessor-root-b");
        fs::create_dir_all(&root_a)?;
        fs::create_dir_all(&root_b)?;
        let database = temp.path().join("predecessor.db");
        let store = AtlasStore::open_for_project(&database, &root_a)?;
        store.connection.execute_batch(
            "DROP TABLE project_root_identity;
             UPDATE metadata SET value = '19' WHERE key = 'schema_version';",
        )?;
        drop(store);

        let expected = CanonicalProjectRoot::from_path(&root_a)?;
        let (preflight, location) = preflight_for_project(&database, &expected)?;
        if preflight.state != SchemaState::UpgradeRequired {
            return Err(io::Error::other("predecessor preflight did not require migration").into());
        }
        let replacement_metadata = crate::normalize_metadata_path(&root_b);
        let transition = Connection::open(&database)?;
        set_metadata(&transition, PROJECT_ROOT_KEY, &replacement_metadata)?;
        drop(transition);

        let before_failure = fs::read(&database)?;
        let inventory_before = directory_entry_names(temp.path())?;
        let writer = crate::sqlite_profile::open_writable_connection(
            &database,
            crate::writable_open_flags(preflight.state, location.database_exists),
            &location,
            crate::SQLITE_BUSY_TIMEOUT,
            crate::writable_journal_policy(preflight.state),
        )?;
        let result = initialize_with_project_root(
            &writer,
            Some(&expected.display_string()?),
            Some(&expected),
        );
        if !matches!(result, Err(DbError::ProjectRootMismatch { .. })) {
            return Err(io::Error::other(
                "predecessor writer accepted metadata changed after preflight",
            )
            .into());
        }
        drop(writer);
        require_unchanged(temp.path(), &database, &before_failure, &inventory_before)?;

        let connection = Connection::open(&database)?;
        let state = connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'schema_version'),
                (SELECT COUNT(*) FROM sqlite_master WHERE name = 'project_root_identity')",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?;
        if state != ("19".to_string(), 0) {
            return Err(
                io::Error::other("predecessor writer exposed partial schema-20 state").into(),
            );
        }
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
    fn released_schema_malformed_telemetry_rolls_back_unchanged_and_retries()
    -> Result<(), Box<dyn Error>> {
        let layouts: [(&str, fn(&Path, &Path) -> Result<(), Box<dyn Error>>); 2] = [
            ("fresh-v0.3.26", write_schema_eight_fixture),
            (
                "evolved-v0.3.11-to-v0.3.26",
                write_evolved_schema_eight_fixture,
            ),
        ];
        for (label, write_fixture) in layouts {
            assert_released_schema_malformed_telemetry_rolls_back(label, write_fixture)?;
        }
        Ok(())
    }

    /// Verify one released layout rolls back a late migration failure and retries.
    fn assert_released_schema_malformed_telemetry_rolls_back(
        label: &str,
        write_fixture: fn(&Path, &Path) -> Result<(), Box<dyn Error>>,
    ) -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let db_path = temp.path().join("projectatlas.db");
        write_fixture(&db_path, &root)?;
        let independent_dir = temp.path().join("independent");
        let independent_root = independent_dir.join("repository");
        fs::create_dir_all(&independent_root)?;
        let independent_db = independent_dir.join("projectatlas.db");
        write_evolved_schema_eight_fixture(&independent_db, &independent_root)?;
        let independent_connection = Connection::open(&independent_db)?;
        seed_released_schema_durable_state(&independent_connection)?;
        drop(independent_connection);
        let independent_bytes = fs::read(&independent_db)?;
        let independent_inventory = directory_entry_names(&independent_dir)?;
        let connection = Connection::open(&db_path)?;
        seed_released_schema_durable_state(&connection)?;
        connection.execute(
            "INSERT INTO usage_events(
                 session_id, command,
                 estimated_tokens_without_projectatlas,
                 estimated_tokens_with_projectatlas,
                 estimated_tokens_saved,
                 created_at
             ) VALUES('legacy', 'summary', 100, 10, 90, 'not-a-timestamp')",
            [],
        )?;
        let durable_before = released_schema_durable_state(&connection)?;
        drop(connection);

        let Err(error) = AtlasStore::open_for_project(&db_path, &root) else {
            return Err(
                io::Error::other("malformed released telemetry unexpectedly migrated").into(),
            );
        };
        if !matches!(error, DbError::InvalidEnum { .. }) {
            return Err(io::Error::other(format!(
                "{label} malformed released telemetry returned the wrong error: {error}"
            ))
            .into());
        }
        require_unchanged(
            &independent_dir,
            &independent_db,
            &independent_bytes,
            &independent_inventory,
        )?;

        let normalized_root = normalize_native_path_display(&root);
        let (preflight, _) = preflight(&db_path, Some(&normalized_root))?;
        if preflight.state != SchemaState::UpgradeRequired
            || preflight.schema_version != Some(PREVIOUS_SCHEMA_VERSION)
            || preflight.project_root.as_deref() != Some(normalized_root.as_str())
        {
            return Err(
                io::Error::other("failed migration changed released-schema identity").into(),
            );
        }
        let connection = Connection::open(&db_path)?;
        let durable_after = released_schema_durable_state(&connection)?;
        if durable_after != durable_before {
            return Err(
                io::Error::other("failed released-schema migration changed durable state").into(),
            );
        }
        connection.execute("UPDATE usage_events SET created_at = CURRENT_TIMESTAMP", [])?;
        drop(connection);
        let store = AtlasStore::open_for_project(&db_path, &root)?;
        if read_metadata(&store.connection, SCHEMA_VERSION_KEY)? != Some(SCHEMA_VERSION.to_string())
        {
            return Err(
                io::Error::other("released-schema retry did not advance the schema").into(),
            );
        }
        let events =
            store
                .connection
                .query_row("SELECT COUNT(*) FROM usage_events", [], |row| {
                    row.get::<_, i64>(0)
                })?;
        if events != 2 {
            return Err(
                io::Error::other("released-schema retry did not preserve telemetry").into(),
            );
        }
        drop(store);
        verify_current_integrity(&db_path, Some(&normalized_root))?;
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
        connection.execute_batch(&format!(
            "
            INSERT INTO nodes(id, path, kind) VALUES(1, 'src/lib.rs', 'file');
            INSERT INTO purposes(node_id, purpose, source, status)
                VALUES(1, 'Own the library.', 'agent', 'stale');
            INSERT INTO usage_events(session_id, command) VALUES('session', 'files');
            CREATE TEMP TRIGGER abort_final_schema_version
            BEFORE UPDATE OF value ON metadata
            WHEN OLD.key = 'schema_version' AND NEW.value = '{SCHEMA_VERSION}'
            BEGIN
                SELECT RAISE(ABORT, 'forced final schema-version failure');
            END;
            ",
        ))?;
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
        let migrated_tables = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_schema
             WHERE type = 'table' AND name IN (
                'project_identity', 'graph_entities', 'graph_relations',
                'graph_relation_occurrences', 'graph_coverage',
                'graph_resolution_keys', 'graph_entity_exports',
                'graph_relation_dependencies',
                'usage_instances', 'usage_bucket_dimensions',
                'usage_instance_baselines', 'usage_labels',
                'usage_global_aggregates', 'usage_instance_aggregates',
                'usage_daily_aggregates', 'usage_instance_daily_aggregates',
                'usage_retention_state', 'usage_label_tombstones',
                'usage_instance_tombstones'
             )",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        if migrated_tables != 0 {
            return Err(io::Error::other(
                "failed migration retained graph or telemetry schema objects",
            )
            .into());
        }
        let migrated_index = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_schema
             WHERE type = 'index' AND name = 'idx_symbol_import_alias_lookup'",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        if migrated_index != 0 {
            return Err(
                io::Error::other("failed migration retained the target lookup index").into(),
            );
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
        let purpose_status =
            connection.query_row("SELECT status FROM purposes WHERE node_id = 1", [], |row| {
                row.get::<_, String>(0)
            })?;
        if purpose_status != "stale" {
            return Err(io::Error::other(
                "failed migration exposed partial purpose-state normalization",
            )
            .into());
        }
        if read_metadata(&connection, PROJECT_ROOT_KEY)?
            != Some(normalize_native_path_display(&root))
        {
            return Err(io::Error::other("failed migration changed project root identity").into());
        }
        connection.execute_batch("DROP TRIGGER abort_final_schema_version")?;
        initialize(&connection, Some(&normalize_native_path_display(&root)))?;
        if read_metadata(&connection, SCHEMA_VERSION_KEY)? != Some(SCHEMA_VERSION.to_string()) {
            return Err(io::Error::other("retry did not advance the final schema version").into());
        }
        let migrated_index = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_schema
             WHERE type = 'index' AND name = 'idx_symbol_import_alias_lookup'",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        if migrated_index != 1 {
            return Err(
                io::Error::other("migration retry did not create the target lookup index").into(),
            );
        }
        let migrated = connection.query_row(
            "SELECT
                 (SELECT COUNT(*) FROM usage_events),
                 (SELECT COUNT(*) FROM usage_instances),
                 (SELECT raw_rows FROM usage_retention_state WHERE singleton = 1)",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        if migrated != (1, 1, 1) {
            return Err(io::Error::other("retry did not migrate legacy telemetry exactly").into());
        }
        Ok(())
    }

    #[test]
    fn schema_ten_telemetry_migration_rolls_back_malformed_rows_and_retries()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo");
        fs::create_dir_all(&root)?;
        let db_path = temp.path().join("projectatlas.db");
        let connection = Connection::open(&db_path)?;
        configure_writable(&connection)?;
        create_released_schema_eight(&connection)?;
        set_metadata(
            &connection,
            PROJECT_ROOT_KEY,
            &normalize_native_path_display(&root),
        )?;
        migrate_8_to_9(&connection)?;
        set_metadata(
            &connection,
            SCHEMA_VERSION_KEY,
            &PUBLICATION_SCHEMA_VERSION.to_string(),
        )?;
        migrate_9_to_10(&connection)?;
        set_metadata(
            &connection,
            SCHEMA_VERSION_KEY,
            &GRAPH_SCHEMA_VERSION.to_string(),
        )?;
        connection.execute(
            "INSERT INTO usage_events(
                 session_id, command,
                 estimated_tokens_without_projectatlas,
                 estimated_tokens_with_projectatlas,
                 estimated_tokens_saved,
                 created_at
             ) VALUES('legacy', 'summary', 100, 10, 90, 'not-a-timestamp')",
            [],
        )?;
        let identity_before = connection.query_row(
            "SELECT project_instance_id FROM project_identity WHERE singleton = 1",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        )?;

        let Err(error) = initialize(&connection, Some(&normalize_native_path_display(&root)))
        else {
            return Err(io::Error::other("malformed telemetry unexpectedly migrated").into());
        };
        if !matches!(error, DbError::InvalidEnum { .. }) {
            return Err(io::Error::other(format!(
                "malformed telemetry returned the wrong error: {error}"
            ))
            .into());
        }
        if read_metadata(&connection, SCHEMA_VERSION_KEY)? != Some(GRAPH_SCHEMA_VERSION.to_string())
        {
            return Err(io::Error::other("failed telemetry migration advanced schema").into());
        }
        let legacy_shape = connection.query_row(
            "SELECT
                 (SELECT COUNT(*) FROM usage_events),
                 EXISTS(
                     SELECT 1 FROM pragma_table_info('usage_events')
                     WHERE name = 'created_at'
                 ),
                 EXISTS(
                     SELECT 1 FROM sqlite_schema
                     WHERE type = 'table' AND name = 'usage_instances'
                 )",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        if legacy_shape != (1, 1, 0) {
            return Err(io::Error::other("failed telemetry migration changed legacy rows").into());
        }
        let identity_after = connection.query_row(
            "SELECT project_instance_id FROM project_identity WHERE singleton = 1",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        )?;
        if identity_after != identity_before {
            return Err(io::Error::other("failed telemetry migration changed identity").into());
        }

        let policy = crate::telemetry::TelemetryRetentionPolicy::default();
        let oversized_label = "λ".repeat(policy.max_label_bytes + 1);
        let oversized_path = "道".repeat(policy.max_path_bytes + 1);
        let oversized_query = "ß".repeat(policy.max_query_bytes + 1);
        let oversized_identity = "界".repeat(policy.max_baseline_witness_bytes + 1);
        let oversized_fingerprint = "ø".repeat(256.min(policy.max_baseline_witness_bytes) + 1);
        connection.execute(
            "UPDATE usage_events
             SET created_at = CURRENT_TIMESTAMP,
                 session_id = ?1,
                 command = '',
                 path = ?2,
                 query = ?3,
                 token_savings_bucket = '',
                 calculation_trace = '',
                 baseline_identity = ?4,
                 baseline_fingerprint = ?5",
            params![
                oversized_label,
                oversized_path,
                oversized_query,
                oversized_identity,
                oversized_fingerprint,
            ],
        )?;
        connection.execute(
            "INSERT INTO usage_events(
                 session_id, command,
                 estimated_tokens_without_projectatlas,
                 estimated_tokens_with_projectatlas,
                 estimated_tokens_saved,
                 created_at
             ) VALUES('', 'files', 200, 20, 180, CURRENT_TIMESTAMP)",
            [],
        )?;
        connection.execute(
            "INSERT INTO usage_events(
                 session_id, command, path, query,
                 estimated_tokens_without_projectatlas,
                 estimated_tokens_with_projectatlas,
                 estimated_tokens_saved,
                 calculation_trace, baseline_identity, baseline_fingerprint,
                 created_at
             ) VALUES(
                 'valid-label', 'slice', 'src/lib.rs', 'needle',
                 300, 30, 270,
                 'trace=valid', 'valid-baseline', 'valid-fingerprint',
                 CURRENT_TIMESTAMP
             )",
            [],
        )?;
        initialize(&connection, Some(&normalize_native_path_display(&root)))?;
        let migrated = connection.query_row(
            "SELECT
                 (SELECT COUNT(*) FROM usage_events),
                 (SELECT COUNT(*) FROM usage_instances),
                 (SELECT SUM(calls) FROM usage_global_aggregates),
                 (SELECT SUM(deduped_modeled_without - deduped_modeled_with)
                    FROM usage_global_aggregates),
                 (SELECT raw_rows FROM usage_retention_state WHERE singleton = 1),
                 (SELECT daily_rows FROM usage_retention_state WHERE singleton = 1)",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )?;
        if migrated != (3, 3, 3, 540, 3, 5) {
            return Err(io::Error::other(format!(
                "schema-ten telemetry retry produced incorrect state: {migrated:?}"
            ))
            .into());
        }
        let bounded = connection.query_row(
            "SELECT
                 length(CAST(i.caller_label AS BLOB)), i.raw_detail_complete,
                 length(CAST(e.command AS BLOB)),
                 length(CAST(e.path AS BLOB)),
                 length(CAST(e.query AS BLOB)),
                 length(CAST(e.calculation_trace AS BLOB)),
                 length(CAST(e.baseline_identity AS BLOB)),
                 length(CAST(e.baseline_fingerprint AS BLOB)), d.overflow,
                 s.raw_detail_complete, s.dimension_detail_complete,
                 s.label_history_complete
             FROM usage_events AS e
             JOIN usage_instances AS i USING(instance_row_id)
             JOIN usage_bucket_dimensions AS d USING(dimension_id)
             CROSS JOIN usage_retention_state AS s
             WHERE d.overflow = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, i64>(11)?,
                ))
            },
        )?;
        if bounded.0 == 0
            || bounded.0 > policy.max_label_bytes as i64
            || bounded.2 == 0
            || bounded.2 > policy.max_command_bytes as i64
            || bounded.3 > policy.max_path_bytes as i64
            || bounded.4 > policy.max_query_bytes as i64
            || bounded.5 == 0
            || bounded.5 > 256.min(policy.max_baseline_witness_bytes) as i64
            || bounded.6 == 0
            || bounded.6 > policy.max_baseline_witness_bytes as i64
            || bounded.7 == 0
            || bounded.7 > 256.min(policy.max_baseline_witness_bytes) as i64
            || bounded.8 != 1
            || (bounded.1, bounded.9, bounded.10, bounded.11) != (0, 0, 0, 0)
        {
            return Err(io::Error::other(format!(
                "schema-ten telemetry retry retained unbounded or falsely complete detail: {bounded:?}"
            ))
            .into());
        }
        let valid = connection.query_row(
            "SELECT i.caller_label, e.command, e.path, e.query,
                    e.calculation_trace, e.baseline_identity,
                    e.baseline_fingerprint, d.overflow
             FROM usage_events AS e
             JOIN usage_instances AS i USING(instance_row_id)
             JOIN usage_bucket_dimensions AS d USING(dimension_id)
             WHERE e.command = 'slice'",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )?;
        if valid
            != (
                Some("valid-label".to_string()),
                "slice".to_string(),
                Some("src/lib.rs".to_string()),
                Some("needle".to_string()),
                "trace=valid".to_string(),
                "valid-baseline".to_string(),
                "valid-fingerprint".to_string(),
                0,
            )
        {
            return Err(io::Error::other(format!(
                "schema-ten telemetry migration changed valid detail: {valid:?}"
            ))
            .into());
        }
        let valid_dimension = connection.query_row(
            "SELECT d.token_savings_bucket, d.provider, d.model,
                    d.tokenizer_backend, d.accuracy, d.baseline_kind,
                    d.confidence, d.accounting_layer, d.estimate_method,
                    d.denominator_kind, d.dedupe_scope, d.overflow
             FROM usage_events AS e
             JOIN usage_bucket_dimensions AS d USING(dimension_id)
             WHERE e.command = 'slice'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, i64>(11)?,
                ))
            },
        )?;
        if valid_dimension
            != (
                "navigation_avoidance".to_string(),
                "heuristic".to_string(),
                "unknown".to_string(),
                "chars_div_4".to_string(),
                "heuristic_estimate".to_string(),
                "selected_candidates".to_string(),
                "inferred".to_string(),
                "modeled_avoidance".to_string(),
                "heuristic_chars_or_bytes_div_ceil_4".to_string(),
                "selected_candidates".to_string(),
                "session".to_string(),
                0,
            )
        {
            return Err(io::Error::other(format!(
                "schema-ten telemetry migration changed valid dimensions: {valid_dimension:?}"
            ))
            .into());
        }
        let empty_labels = connection.query_row(
            "SELECT COUNT(*) FROM usage_instances WHERE caller_label IS NULL",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        if empty_labels != 1 {
            return Err(
                io::Error::other("empty predecessor label was not migrated as absent").into(),
            );
        }
        drop(connection);
        let reopened = Connection::open(&db_path)?;
        configure_writable(&reopened)?;
        initialize(&reopened, Some(&normalize_native_path_display(&root)))?;
        let reopened_totals = reopened.query_row(
            "SELECT
                 (SELECT COUNT(*) FROM usage_events),
                 (SELECT COUNT(*) FROM usage_instances),
                 (SELECT SUM(calls) FROM usage_global_aggregates),
                 (SELECT SUM(deduped_modeled_without - deduped_modeled_with)
                    FROM usage_global_aggregates)",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )?;
        if reopened_totals != (3, 3, 3, 540) {
            return Err(io::Error::other(format!(
                "reopened telemetry migration changed totals: {reopened_totals:?}"
            ))
            .into());
        }
        Ok(())
    }

    #[test]
    fn schema_ten_migration_preserves_exact_totals_above_runtime_baseline_capacity() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            const LEGACY_BASELINES: i64 = 1_025;
            const OVERSIZED_BASELINE_EVENTS: i64 = 3;

            let temp = tempfile::tempdir()?;
            let root = temp.path().join("repo");
            fs::create_dir_all(&root)?;
            let connection = Connection::open(temp.path().join("projectatlas.db"))?;
            configure_writable(&connection)?;
            create_released_schema_eight(&connection)?;
            set_metadata(
                &connection,
                PROJECT_ROOT_KEY,
                &normalize_native_path_display(&root),
            )?;
            migrate_8_to_9(&connection)?;
            set_metadata(
                &connection,
                SCHEMA_VERSION_KEY,
                &PUBLICATION_SCHEMA_VERSION.to_string(),
            )?;
            migrate_9_to_10(&connection)?;
            set_metadata(
                &connection,
                SCHEMA_VERSION_KEY,
                &GRAPH_SCHEMA_VERSION.to_string(),
            )?;
            let mut insert = connection.prepare_cached(
                "INSERT INTO usage_events(
                 session_id, command,
                 estimated_tokens_without_projectatlas,
                 estimated_tokens_with_projectatlas,
                 estimated_tokens_saved,
                 baseline_identity,
                 baseline_fingerprint
             ) VALUES('large-legacy-session', 'summary', 10, 1, 9, ?1, ?2)",
            )?;
            for index in 0..LEGACY_BASELINES {
                insert.execute(params![
                    format!("source:src/file-{index}.rs"),
                    format!("source:src/file-{index}.rs:v1"),
                ])?;
            }
            let policy = crate::telemetry::TelemetryRetentionPolicy::default();
            let shared_identity = "a".repeat(policy.max_baseline_witness_bytes + 1);
            let shared_fingerprint = "b".repeat(256.min(policy.max_baseline_witness_bytes) + 1);
            let repeated_identity = format!("{shared_identity}:repeated");
            let repeated_fingerprint = format!("{shared_fingerprint}:repeated");
            let distinct_identity = format!("{shared_identity}:distinct");
            let distinct_fingerprint = format!("{shared_fingerprint}:distinct");
            insert.execute(params![repeated_identity, repeated_fingerprint])?;
            insert.execute(params![repeated_identity, repeated_fingerprint])?;
            insert.execute(params![distinct_identity, distinct_fingerprint])?;
            drop(insert);

            initialize(&connection, Some(&normalize_native_path_display(&root)))?;
            let migrated = connection.query_row(
                "SELECT
                 (SELECT COUNT(*) FROM usage_events),
                 (SELECT COUNT(*) FROM usage_instances),
                 (SELECT COUNT(*) FROM usage_instance_baselines),
                 (SELECT state FROM usage_instances),
                 (SELECT calls FROM usage_global_aggregates),
                 (SELECT deduped_modeled_without - deduped_modeled_with
                    FROM usage_global_aggregates),
                 (SELECT repeated_baselines FROM usage_global_aggregates),
                 raw_rows,
                 baseline_rows,
                 raw_detail_complete,
                 dimension_detail_complete,
                 label_history_complete,
                 (SELECT COUNT(DISTINCT baseline_identity) FROM usage_events),
                 (SELECT MAX(length(CAST(baseline_identity AS BLOB))) FROM usage_events),
                 (SELECT MAX(length(CAST(baseline_fingerprint AS BLOB))) FROM usage_events)
             FROM usage_retention_state WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, i64>(9)?,
                        row.get::<_, i64>(10)?,
                        row.get::<_, i64>(11)?,
                        row.get::<_, i64>(12)?,
                        row.get::<_, i64>(13)?,
                        row.get::<_, i64>(14)?,
                    ))
                },
            )?;
            let (
                raw_events,
                instances,
                baselines,
                instance_state,
                calls,
                deduped_saved,
                repeated_baseline_count,
                raw_rows,
                baseline_rows,
                raw_detail_complete,
                dimension_detail_complete,
                label_history_complete,
                distinct_baseline_identities,
                maximum_identity_bytes,
                maximum_fingerprint_bytes,
            ) = migrated;
            assert_eq!(
                (
                    raw_events,
                    instances,
                    baselines,
                    instance_state,
                    calls,
                    deduped_saved,
                    repeated_baseline_count,
                    raw_rows,
                ),
                (
                    LEGACY_BASELINES + OVERSIZED_BASELINE_EVENTS,
                    1,
                    0,
                    "sealed".to_string(),
                    LEGACY_BASELINES + OVERSIZED_BASELINE_EVENTS,
                    LEGACY_BASELINES * 9 + 17,
                    1,
                    LEGACY_BASELINES + OVERSIZED_BASELINE_EVENTS,
                )
            );
            assert_eq!(
                (
                    baseline_rows,
                    raw_detail_complete,
                    dimension_detail_complete,
                    label_history_complete,
                    distinct_baseline_identities,
                ),
                (0, 0, 1, 1, LEGACY_BASELINES + 2,)
            );
            assert!(maximum_identity_bytes <= policy.max_baseline_witness_bytes as i64);
            assert!(maximum_fingerprint_bytes <= 256.min(policy.max_baseline_witness_bytes) as i64);
            assert_eq!(
                read_metadata(&connection, SCHEMA_VERSION_KEY)?,
                Some(SCHEMA_VERSION.to_string())
            );
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "over-budget telemetry migration test failed: {result:?}"
        );
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
        write_released_schema_eight_fixture(db_path, root, create_released_schema_eight)
    }

    /// Write an evolved released schema-8 fixture with publication metadata.
    fn write_evolved_schema_eight_fixture(
        db_path: &Path,
        root: &Path,
    ) -> Result<(), Box<dyn Error>> {
        write_released_schema_eight_fixture(db_path, root, create_evolved_released_schema_eight)
    }

    /// Write one captured released schema-8 fixture with publication metadata.
    fn write_released_schema_eight_fixture(
        db_path: &Path,
        root: &Path,
        create_schema: fn(&Connection) -> DbResult<()>,
    ) -> Result<(), Box<dyn Error>> {
        let connection = Connection::open(db_path)?;
        create_schema(&connection)?;
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

    /// Seed representative durable rows for released-schema rollback coverage.
    fn seed_released_schema_durable_state(connection: &Connection) -> DbResult<()> {
        set_metadata(connection, "custom_setting", "preserved")?;
        connection.execute_batch(
            "
            INSERT INTO nodes(
                id, path, kind, parent_path, extension, language,
                size_bytes, mtime_ns, content_hash
            )
            VALUES(1, 'src/lib.rs', 'file', 'src', '.rs', 'rust', 12, 10, 'hash-legacy');

            INSERT INTO purposes(node_id, purpose, source, status, updated_by)
            VALUES(1, 'Schema compatibility source', 'agent', 'approved', 'agent');

            INSERT INTO summaries(node_id, summary_level, subject, summary)
            VALUES(1, 'node', '', 'released schema source');

            INSERT INTO health_resolutions(finding_id, category, path, rationale)
            VALUES('schema-review', 'purpose', 'src/lib.rs', 'Reviewed schema fixture');

            INSERT INTO file_texts(path, content_hash, byte_count, line_count, content)
            VALUES('src/lib.rs', 'hash-legacy', 6, 1, 'legacy');

            INSERT INTO usage_events(
                session_id, command,
                estimated_tokens_without_projectatlas,
                estimated_tokens_with_projectatlas,
                estimated_tokens_saved
            )
            VALUES('retained', 'files', 40, 10, 30);
            ",
        )?;
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

    /// Write one evolved released schema whose DDL differs in one semantic way.
    fn write_evolved_schema_lookalike(
        db_path: &Path,
        root: &Path,
        needle: &str,
        replacement: &str,
    ) -> Result<(), Box<dyn Error>> {
        let ddl = EVOLVED_RELEASED_SCHEMA_EIGHT_SQL.replacen(needle, replacement, 1);
        if ddl == EVOLVED_RELEASED_SCHEMA_EIGHT_SQL {
            return Err(io::Error::other("evolved schema replacement did not match").into());
        }
        let connection = Connection::open(db_path)?;
        create_schema_eight_fixture(&connection, &ddl)?;
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
