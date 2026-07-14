//! `SQLite` schema initialization and legacy repair behind the store facade.

use crate::{DbError, DbResult, sqlite_read_uri};
use projectatlas_core::budget::DefaultCoreBudgetKind;
use projectatlas_core::graph::{
    CoverageRecord, CoverageScope, CoverageState, GraphRelationKind, IdentityText, IndexEpoch,
    OmittedFactCount, ProjectInstanceId, PublicationState, RepositoryFilePath, StructuralSlot,
};
use projectatlas_core::normalize_native_path_display;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params, types::ValueRef};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Last schema version released before the runtime-owned migration ledger.
const MIGRATION_BASE_SCHEMA_VERSION: i64 = 9;
/// Current `SQLite` schema version allocated from the accepted migration inventory.
const SCHEMA_VERSION: i64 = MIGRATION_BASE_SCHEMA_VERSION + MIGRATIONS.len() as i64;
/// Metadata key for the schema contract version.
const SCHEMA_VERSION_METADATA_KEY: &str = "schema_version";
/// Metadata key for one independently initialized database identity.
const PROJECT_INSTANCE_ID_METADATA_KEY: &str = "project_instance_id";
/// Runtime-owned append-only migration history table.
const MIGRATION_LEDGER_TABLE: &str = "schema_migrations";
/// Optional trigram candidate index over authoritative file text.
const FILE_TEXT_FTS_TABLE: &str = "file_text_fts";
/// Authoritative UTF-8 source text used by deterministic lexical search.
const FILE_TEXTS_TABLE: &str = "file_texts";
/// Trigger that mirrors inserted source text into the optional candidate index.
const FILE_TEXT_FTS_INSERT_TRIGGER: &str = "file_text_fts_insert";
/// Trigger that removes deleted source text from the optional candidate index.
const FILE_TEXT_FTS_DELETE_TRIGGER: &str = "file_text_fts_delete";
/// Trigger that mirrors source-text updates into the optional candidate index.
const FILE_TEXT_FTS_UPDATE_TRIGGER: &str = "file_text_fts_update";
/// Fixed trigram-safe prefix for indexed repository-path lookup.
const FILE_TEXT_FTS_PATH_LOOKUP_PREFIX: &str = "pa";
/// Canonical current-schema trigger that mirrors inserted source text.
fn file_text_fts_insert_trigger_sql() -> String {
    format!(
        r"CREATE TRIGGER file_text_fts_insert
AFTER INSERT ON file_texts
BEGIN
    INSERT INTO file_text_fts(
        structural_slot, last_changed_epoch, path, path_lookup, content
    ) VALUES(
        new.structural_slot, new.last_changed_epoch, new.path, '{FILE_TEXT_FTS_PATH_LOOKUP_PREFIX}' || new.path, new.content
    );
END;"
    )
}
/// Canonical current-schema trigger that removes deleted source text.
const FILE_TEXT_FTS_DELETE_TRIGGER_SQL: &str = r"CREATE TRIGGER file_text_fts_delete
AFTER DELETE ON file_texts
BEGIN
    DELETE FROM file_text_fts
    WHERE structural_slot = old.structural_slot AND path = old.path;
END;";
/// Canonical current-schema trigger that mirrors source-text updates.
fn file_text_fts_update_trigger_sql() -> String {
    format!(
        r"CREATE TRIGGER file_text_fts_update
AFTER UPDATE OF structural_slot, last_changed_epoch, path, content ON file_texts
BEGIN
    DELETE FROM file_text_fts
    WHERE structural_slot = old.structural_slot AND path = old.path;
    INSERT INTO file_text_fts(
        structural_slot, last_changed_epoch, path, path_lookup, content
    ) VALUES(
        new.structural_slot, new.last_changed_epoch, new.path, '{FILE_TEXT_FTS_PATH_LOOKUP_PREFIX}' || new.path, new.content
    );
END;"
    )
}
/// Typed stable repository-graph entity rows.
const GRAPH_ENTITIES_TABLE: &str = "graph_entities";
/// Typed resolved logical repository-graph relations.
const GRAPH_RELATIONS_TABLE: &str = "graph_relations";
/// Evidence occurrences owned by resolved logical relations.
const GRAPH_EVIDENCE_OCCURRENCES_TABLE: &str = "graph_evidence_occurrences";
/// Non-traversable ambiguous and unresolved graph occurrences.
const GRAPH_RESOLUTION_OCCURRENCES_TABLE: &str = "graph_resolution_occurrences";
/// Normalized bounded candidates owned by ambiguous graph occurrences.
const GRAPH_RESOLUTION_CANDIDATES_TABLE: &str = "graph_resolution_candidates";
/// Typed structural graph coverage rows.
const GRAPH_COVERAGE_TABLE: &str = "graph_coverage";
/// The two immutable physical structural derived-data slot identities.
const GRAPH_STRUCTURAL_SLOTS_TABLE: &str = "graph_structural_slots";
/// Singleton active structural slot and publication epoch metadata.
const GRAPH_PUBLICATION_STATE_TABLE: &str = "graph_publication_state";
/// Guard that prevents either structural slot identity from being removed.
const GRAPH_STRUCTURAL_SLOTS_DELETE_GUARD: &str = "graph_structural_slots_delete_guard";
/// Guard that prevents either structural slot identity from being rewritten.
const GRAPH_STRUCTURAL_SLOTS_UPDATE_GUARD: &str = "graph_structural_slots_update_guard";
/// Guard that preserves the singleton publication metadata row.
const GRAPH_PUBLICATION_STATE_DELETE_GUARD: &str = "graph_publication_state_delete_guard";
/// Affected-row lookup index for graph entities.
const GRAPH_ENTITIES_SLOT_PATH_INDEX: &str = "idx_graph_entities_slot_repository_path";
/// Affected-row lookup index for graph evidence.
const GRAPH_EVIDENCE_SLOT_PATH_INDEX: &str = "idx_graph_evidence_slot_origin_repository_path";
/// Affected-row lookup index for unresolved graph occurrences.
const GRAPH_RESOLUTION_SLOT_PATH_INDEX: &str = "idx_graph_resolution_slot_origin_repository_path";
/// Affected-row lookup index for structural coverage.
const GRAPH_COVERAGE_SLOT_PATH_INDEX: &str = "idx_graph_coverage_slot_repository_path";
/// Schema-owned indexes that keep non-root reconciliation proportional to its path closure.
const AFFECTED_RECONCILIATION_INDEX_SQL: &str = r"CREATE INDEX IF NOT EXISTS idx_graph_entities_slot_repository_path
          ON graph_entities(structural_slot, repository_path);
      CREATE INDEX IF NOT EXISTS idx_graph_evidence_slot_origin_repository_path
          ON graph_evidence_occurrences(structural_slot, origin_repository_path);
      CREATE INDEX IF NOT EXISTS idx_graph_resolution_slot_origin_repository_path
          ON graph_resolution_occurrences(structural_slot, origin_repository_path);
      CREATE INDEX IF NOT EXISTS idx_graph_coverage_slot_repository_path
          ON graph_coverage(structural_slot, repository_path);";
/// Canonical current-schema guard for the immutable slot inventory.
const GRAPH_STRUCTURAL_SLOTS_DELETE_GUARD_SQL: &str = r"CREATE TRIGGER graph_structural_slots_delete_guard
BEFORE DELETE ON graph_structural_slots
BEGIN
    SELECT RAISE(ABORT, 'structural slot identities are immutable');
END;";
/// Canonical current-schema guard for immutable slot spellings.
const GRAPH_STRUCTURAL_SLOTS_UPDATE_GUARD_SQL: &str = r"CREATE TRIGGER graph_structural_slots_update_guard
BEFORE UPDATE OF slot ON graph_structural_slots
BEGIN
    SELECT RAISE(ABORT, 'structural slot identities are immutable');
END;";
/// Canonical current-schema guard for the required publication singleton.
const GRAPH_PUBLICATION_STATE_DELETE_GUARD_SQL: &str = r"CREATE TRIGGER graph_publication_state_delete_guard
BEFORE DELETE ON graph_publication_state
BEGIN
    SELECT RAISE(ABORT, 'publication state is a required singleton');
END;";
/// Accepted relation-kind values owned by the typed graph schema.
const GRAPH_RELATION_KIND_VALUES: &str = "'calls', 'channel', 'co-changes', 'configures', 'contains', 'cross-repository', 'declares', 'depends-on', 'deploys', 'exports', 'generates', 'implements', 'imports', 'inherits', 'overrides', 'reads', 'references', 'routes', 'rpc', 'similar', 'tests', 'writes'";
/// Accepted parser-origin values owned by the typed graph schema.
const GRAPH_PARSER_KIND_VALUES: &str =
    "'tree-sitter', 'manifest', 'structural', 'fallback', 'parser-pack'";
/// Finite graph confidence values.
const GRAPH_CONFIDENCE_VALUES: &str = "'low', 'medium', 'high', 'exact'";
/// Independent graph completeness values.
const GRAPH_COMPLETENESS_VALUES: &str = "'complete', 'partial', 'truncated'";
/// Direct or inferred graph evidence values.
const GRAPH_EVIDENCE_CLASS_VALUES: &str = "'direct', 'inferred'";
/// Typed evidence-origin values.
const GRAPH_EVIDENCE_ORIGIN_VALUES: &str = "'entity', 'repository-path', 'external'";
/// Resolved target-scope values.
const GRAPH_TARGET_SCOPE_VALUES: &str = "'internal', 'external'";
/// Structural slot values already required by the typed coverage contract.
const GRAPH_STRUCTURAL_SLOT_VALUES: &str = "'a', 'b'";

/// Complete core structural table inventory owned by the current schema.
pub(crate) const STRUCTURAL_DERIVED_TABLES: [&str; 7] = [
    GRAPH_ENTITIES_TABLE,
    GRAPH_RELATIONS_TABLE,
    GRAPH_EVIDENCE_OCCURRENCES_TABLE,
    GRAPH_RESOLUTION_OCCURRENCES_TABLE,
    GRAPH_RESOLUTION_CANDIDATES_TABLE,
    GRAPH_COVERAGE_TABLE,
    FILE_TEXTS_TABLE,
];
/// Required structural objects whose normalized DDL is a publication invariant.
const STRUCTURAL_SCHEMA_SQL_OBJECTS: [&str; 16] = [
    GRAPH_STRUCTURAL_SLOTS_TABLE,
    GRAPH_PUBLICATION_STATE_TABLE,
    GRAPH_ENTITIES_TABLE,
    GRAPH_RELATIONS_TABLE,
    GRAPH_EVIDENCE_OCCURRENCES_TABLE,
    GRAPH_RESOLUTION_OCCURRENCES_TABLE,
    GRAPH_RESOLUTION_CANDIDATES_TABLE,
    GRAPH_COVERAGE_TABLE,
    FILE_TEXTS_TABLE,
    GRAPH_ENTITIES_SLOT_PATH_INDEX,
    GRAPH_EVIDENCE_SLOT_PATH_INDEX,
    GRAPH_RESOLUTION_SLOT_PATH_INDEX,
    GRAPH_COVERAGE_SLOT_PATH_INDEX,
    GRAPH_STRUCTURAL_SLOTS_DELETE_GUARD,
    GRAPH_STRUCTURAL_SLOTS_UPDATE_GUARD,
    GRAPH_PUBLICATION_STATE_DELETE_GUARD,
];
/// Optional FTS objects that join the exact contract only when FTS is available.
const OPTIONAL_STRUCTURAL_SCHEMA_SQL_OBJECTS: [&str; 4] = [
    FILE_TEXT_FTS_TABLE,
    FILE_TEXT_FTS_INSERT_TRIGGER,
    FILE_TEXT_FTS_DELETE_TRIGGER,
    FILE_TEXT_FTS_UPDATE_TRIGGER,
];
/// Process-local immutable DDL reference built from the authoritative initializer.
static STRUCTURAL_SCHEMA_SQL_REFERENCE: OnceLock<Result<BTreeMap<String, String>, String>> =
    OnceLock::new();

/// Earliest metadata schema version that the current repair path accepts.
const MIN_SUPPORTED_SCHEMA_VERSION: i64 = 1;
/// Project-local directory that owns the database file.
const PROJECTATLAS_DIRECTORY_NAME: &str = ".projectatlas";
/// Minimum headroom reserved when estimating backup feasibility.
const BACKUP_HEADROOM_BYTES: u64 = 64 * 1024;

/// One accepted migration before its schema version is allocated by inventory order.
#[derive(Clone, Copy, Debug)]
struct MigrationDefinition {
    /// Immutable accepted migration identity.
    id: &'static str,
    /// Smallest runtime module responsible for the migration.
    owner: &'static str,
    /// State that must be proven before applying the migration.
    prerequisites: &'static str,
    /// Contract for ProjectAtlas-authored rows.
    authored_effects: &'static str,
    /// Contract for reproducible derived rows.
    derived_effects: &'static str,
    /// Atomicity boundary that owns every migration write.
    transaction_boundary: &'static str,
    /// Successful state transition performed by the migration.
    forward_behavior: &'static str,
    /// State retained when any migration step fails.
    rollback_behavior: &'static str,
    /// Focused executable proof for this migration.
    evidence: &'static str,
    /// Concrete schema operation for this closed migration inventory.
    apply: fn(&Connection) -> DbResult<()>,
    /// Concrete postcondition checked before the ledger row is written.
    verify: fn(&Connection) -> DbResult<()>,
}

/// Accepted migrations in immutable application order.
const MIGRATIONS: [MigrationDefinition; 6] = [
    MigrationDefinition {
        id: "install-runtime-migration-ledger",
        owner: "projectatlas-db::schema",
        prerequisites: "read-only-preflight=write-ready;source-schema=1..=9-or-fresh",
        authored_effects: "preserve=metadata,purposes,settings,telemetry",
        derived_effects: "preserve=nodes,summaries,symbols,relations,file-texts",
        transaction_boundary: "single-sqlite-transaction",
        forward_behavior: "create-ledger;record-accepted-history;advance-schema-version",
        rollback_behavior: "rollback-transaction;retain-source-schema-and-data",
        evidence: "cargo test --locked --workspace --all-features task_arri_ut_arri_4_7",
        apply: create_migration_ledger,
        verify: verify_migration_ledger_schema,
    },
    MigrationDefinition {
        id: "install-file-text-trigram-index",
        owner: "projectatlas-db::schema::file-text-search",
        prerequisites: "read-only-preflight=write-ready;sqlite-feature=fts5-trigram-or-exact-fallback",
        authored_effects: "preserve=metadata,purposes,settings,telemetry",
        derived_effects: "preserve=file-texts;when-supported=create-and-backfill-file-text-fts;otherwise=retain-exact-search",
        transaction_boundary: "single-sqlite-transaction",
        forward_behavior: "create-trigram-index;backfill;reconcile;install-sync-triggers;advance-schema-version",
        rollback_behavior: "rollback-index-backfill-triggers-and-ledger;retain-authoritative-file-texts",
        evidence: "cargo test --locked --workspace --all-features task_arri_ut_arri_4_8",
        apply: create_optional_file_text_fts,
        verify: verify_unbound_optional_file_text_fts,
    },
    MigrationDefinition {
        id: "install-typed-repository-graph",
        owner: "projectatlas-db::schema::repository-graph",
        prerequisites: "read-only-preflight=write-ready;graph-contract=typed-stable-keys",
        authored_effects: "preserve=metadata,purposes,settings,telemetry",
        derived_effects: "preserve=nodes,summaries,symbols,relations,source-parse-metadata,file-texts;create=typed-entities,typed-relations,typed-evidence,typed-resolution,typed-coverage",
        transaction_boundary: "single-sqlite-transaction",
        forward_behavior: "create-normalized-typed-graph-tables;reconcile-column-contracts;advance-schema-version",
        rollback_behavior: "rollback-typed-graph-tables-and-ledger;retain-authored-and-compatibility-data",
        evidence: "cargo test --locked --workspace --all-features task_arri_ut_arri_4_9",
        apply: create_typed_repository_graph_schema,
        verify: verify_unbound_typed_repository_graph_schema,
    },
    MigrationDefinition {
        id: "install-structural-publication-state",
        owner: "projectatlas-db::schema::structural-publication",
        prerequisites: "read-only-preflight=write-ready;typed-repository-graph=installed",
        authored_effects: "preserve=metadata,purposes,settings,telemetry",
        derived_effects: "preserve=legacy-and-typed-derived-data;create=exactly-two-structural-slots,singleton-publication-state",
        transaction_boundary: "single-sqlite-transaction",
        forward_behavior: "create-slot-identities-and-publication-state;initialize=active-slot-a,active-epoch-0;advance-schema-version",
        rollback_behavior: "rollback-slot-identities-publication-state-and-ledger;retain-authored-and-derived-data",
        evidence: "cargo test --locked --workspace --all-features task_arri_ut_arri_4_10",
        apply: create_structural_publication_schema,
        verify: verify_structural_publication_schema,
    },
    MigrationDefinition {
        id: "bind-structural-derived-rows",
        owner: "projectatlas-db::schema::structural-storage",
        prerequisites: "read-only-preflight=write-ready;structural-publication=installed",
        authored_effects: "preserve=metadata,purposes,settings,telemetry,health-resolutions",
        derived_effects: "slot-bind=typed-graph,coverage,file-texts,optional-file-text-fts;backfill=current-active-slot-and-epoch;exclude=compatibility-and-authored-tables,semantic-vector-storage;future-pack-vector-binding=structural-slot+structural-epoch+semantic-generation",
        transaction_boundary: "single-sqlite-transaction",
        forward_behavior: "rebuild-core-derived-keys-and-foreign-keys;permit-same-identity-in-both-slots;retain-last-changed-epoch-as-non-key-freshness;advance-schema-version",
        rollback_behavior: "rollback-rebuilt-tables-indexes-triggers-and-ledger;retain-source-schema-and-all-rows",
        evidence: "cargo test --locked --workspace --all-features task_arri_ut_arri_4_11",
        apply: bind_structural_derived_rows,
        verify: verify_bound_structural_derived_rows,
    },
    MigrationDefinition {
        id: "install-affected-reconciliation-contracts",
        owner: "projectatlas-db::schema::structural-reconciliation",
        prerequisites: "read-only-preflight=write-ready;structural-derived-rows=slot-bound",
        authored_effects: "preserve=metadata,purposes,settings,telemetry,health-resolutions",
        derived_effects: "preserve=all-authoritative-and-structural-rows;index=affected-graph-paths,affected-fts-paths;canonicalize=slot-aware-fts-triggers",
        transaction_boundary: "single-sqlite-transaction",
        forward_behavior: "install-path-range-indexes;rebuild-legacy-slot-aware-fts-lookup-when-needed;reconcile-current-schema-and-fts-content;advance-schema-version",
        rollback_behavior: "rollback-indexes-fts-rebuild-and-ledger;retain-prior-schema-and-all-rows",
        evidence: "cargo test --locked --workspace --all-features task_arri_ut_arri_4_16",
        apply: install_affected_reconciliation_contracts,
        verify: verify_affected_reconciliation_contracts,
    },
];

/// One migration with the schema version allocated from accepted inventory order.
#[derive(Clone, Debug)]
struct AllocatedMigration {
    /// Runtime schema version allocated from inventory order.
    schema_version: i64,
    /// Accepted behavior and evidence contract.
    definition: MigrationDefinition,
    /// Integrity digest over the allocated version and accepted contract.
    checksum: String,
}

impl AllocatedMigration {
    /// Encode the immutable contract fields covered by the ledger digest.
    fn canonical_payload(&self) -> String {
        let definition = self.definition;
        [
            self.schema_version.to_string(),
            definition.id.to_owned(),
            definition.owner.to_owned(),
            definition.prerequisites.to_owned(),
            definition.authored_effects.to_owned(),
            definition.derived_effects.to_owned(),
            definition.transaction_boundary.to_owned(),
            definition.forward_behavior.to_owned(),
            definition.rollback_behavior.to_owned(),
            definition.evidence.to_owned(),
        ]
        .join("\0")
    }
}

/// Preflight-derived migration work passed across the write-capable open boundary.
#[derive(Clone, Debug)]
pub(crate) struct SchemaMigrationPlan {
    /// Schema identifier observed by read-only preflight.
    source_version: i64,
    /// Current runtime schema identifier derived from accepted migrations.
    target_version: i64,
    /// Ordered migration work not represented by the source ledger.
    pending: Vec<AllocatedMigration>,
}

impl SchemaMigrationPlan {
    /// Plan all accepted migrations for an empty database.
    fn fresh() -> Self {
        Self {
            source_version: 0,
            target_version: SCHEMA_VERSION,
            pending: allocated_migrations(),
        }
    }
}

/// One durable migration row read during preflight or post-migration reconciliation.
#[derive(Clone, Debug, Eq, PartialEq)]
struct MigrationLedgerRecord {
    /// Schema version reached by the recorded migration.
    schema_version: i64,
    /// Immutable accepted migration identity.
    migration_id: String,
    /// Integrity digest from the runtime inventory.
    checksum: String,
}

/// Read-only compatibility result for the runtime-owned migration ledger.
#[derive(Debug, Eq, PartialEq)]
enum MigrationLedgerState {
    /// The source does not contain a migration ledger.
    Absent,
    /// Every persisted row agrees with the runtime inventory.
    Compatible(Vec<MigrationLedgerRecord>),
    /// Table shape or persisted history disagrees with the runtime inventory.
    Rejected(String),
}

/// Allocate schema versions and integrity digests from immutable inventory order.
fn allocated_migrations() -> Vec<AllocatedMigration> {
    MIGRATIONS
        .iter()
        .copied()
        .zip((MIGRATION_BASE_SCHEMA_VERSION + 1)..)
        .map(|(definition, schema_version)| {
            let mut migration = AllocatedMigration {
                schema_version,
                definition,
                checksum: String::new(),
            };
            migration.checksum = format!(
                "blake3:{}",
                blake3::hash(migration.canonical_payload().as_bytes()).to_hex()
            );
            migration
        })
        .collect()
}

/// Schema state found before a write-capable database open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SchemaSourceState {
    /// The database file does not exist yet.
    Fresh,
    /// The database file exists but contains no application objects.
    Empty,
    /// The database declares a supported metadata version.
    Supported {
        /// Declared metadata schema version.
        version: i64,
        /// Whether the source needs the current repair path.
        migration_required: bool,
    },
    /// The database declares a schema outside the supported source range.
    Unsupported {
        /// Declared unsupported metadata schema version.
        version: i64,
    },
}

impl SchemaSourceState {
    /// Whether the source requires a write migration or legacy repair.
    fn migration_required(self) -> bool {
        matches!(
            self,
            Self::Supported {
                migration_required: true,
                ..
            }
        )
    }

    /// Whether missing or legacy-partial current objects are expected.
    fn allows_incomplete_current_objects(self) -> bool {
        !matches!(
            self,
            Self::Supported {
                migration_required: false,
                ..
            }
        )
    }
}

/// Schema-owned object kinds inspected during preflight.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SchemaObjectKind {
    /// A persistent table.
    Table,
    /// A named index.
    Index,
}

impl SchemaObjectKind {
    /// Parse the object kinds selected from `sqlite_schema`.
    fn from_sql(value: &str) -> DbResult<Self> {
        match value {
            "table" => Ok(Self::Table),
            "index" => Ok(Self::Index),
            other => Err(preflight_error(format!(
                "unsupported schema object kind {other:?}"
            ))),
        }
    }

    /// Return the `sqlite_schema` spelling.
    fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Index => "index",
        }
    }
}

/// One required object derived from the authoritative current initializer.
#[derive(Debug, Eq, PartialEq)]
struct SchemaObjectContract {
    /// Required object kind.
    kind: SchemaObjectKind,
    /// Required object name.
    name: String,
    /// Owning table name.
    table_name: String,
    /// Ordered table or index columns.
    columns: Vec<String>,
}

/// Observed compatibility state for one current-schema object.
#[derive(Debug, Eq, PartialEq)]
enum RequiredSchemaObjectStatus {
    /// The object is absent.
    Missing,
    /// Kind, owner, and columns match the current contract.
    Matches,
    /// A supported legacy table is a compatible subset of the current columns.
    LegacyPartial {
        /// Current columns absent from the legacy table.
        missing_columns: Vec<String>,
    },
    /// The object exists with an incompatible kind, owner, or shape.
    Conflict {
        /// Actionable shape mismatch.
        detail: String,
    },
}

/// Required table/column or index observation retained in the report.
#[derive(Debug, Eq, PartialEq)]
struct RequiredSchemaObjectState {
    /// Required object kind.
    kind: SchemaObjectKind,
    /// Required object name.
    name: String,
    /// Required owning table.
    table_name: String,
    /// Required ordered columns.
    columns: Vec<String>,
    /// Observed compatibility status.
    status: RequiredSchemaObjectStatus,
}

/// `SQLite` runtime capabilities observed without target-schema writes.
#[derive(Debug, Eq, PartialEq)]
struct SqliteFeatureState {
    /// Runtime `SQLite` version string.
    version: String,
    /// Sorted compile-time option inventory.
    compile_options: Vec<String>,
    /// Whether the runtime was compiled with FTS5.
    fts5: bool,
    /// Whether the runtime includes the built-in JSON functions.
    json: bool,
}

/// Result of the read-only `SQLite` quick integrity check.
#[derive(Debug, Eq, PartialEq)]
enum IntegrityState {
    /// `SQLite` reported exactly `ok`.
    Passed,
    /// `SQLite` returned one or more integrity diagnostics.
    Failed(Vec<String>),
}

/// Filesystem state for one `SQLite` sidecar.
#[derive(Clone, Debug, Eq, PartialEq)]
struct SidecarFileState {
    /// Sidecar path.
    path: PathBuf,
    /// Whether the sidecar exists.
    exists: bool,
    /// Observed byte length, or zero when absent.
    bytes: u64,
}

/// Journal mode and sidecar state captured before migration writes.
#[derive(Clone, Debug, Eq, PartialEq)]
struct SqliteSidecarState {
    /// Journal mode reported by the inspected connection.
    journal_mode: String,
    /// Write-ahead log state.
    wal: SidecarFileState,
    /// Shared-memory sidecar state.
    shm: SidecarFileState,
    /// Rollback-journal sidecar state.
    rollback_journal: SidecarFileState,
}

/// Root-binding classification observed in metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RootBindingStatus {
    /// No root metadata exists yet.
    Unbound,
    /// The database is outside the standard project-local location.
    UnverifiedLocation,
    /// Stored and inferred project-local roots agree.
    Bound,
    /// Stored metadata points at a different project root.
    Mismatch,
}

/// Root and persistent-instance binding observed in metadata.
#[derive(Debug, Eq, PartialEq)]
struct RootBindingState {
    /// Binding classification.
    status: RootBindingStatus,
    /// Root recorded in metadata.
    stored_root: Option<String>,
    /// Root inferred from a project-local database path.
    expected_root: Option<String>,
    /// Existing project instance identity, if any.
    project_instance_id: Option<ProjectInstanceId>,
}

/// Filesystem capacity observed for migration backup planning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FreeSpaceState {
    /// Bytes available to the current user.
    available_bytes: u64,
    /// Estimated bytes required for a consistent backup plus headroom.
    required_bytes: u64,
    /// Whether the observed capacity meets the estimate.
    sufficient: bool,
}

/// Backup and rollback feasibility before a migration write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BackupReadiness {
    /// Whether the current source requires a pre-migration backup.
    required: bool,
    /// Whether parent permissions and free space permit backup creation.
    backup_ready: bool,
    /// Whether the same retained backup can support rollback.
    rollback_ready: bool,
}

/// Complete schema preflight record produced before migration writes.
#[derive(Debug, Eq, PartialEq)]
struct SchemaPreflightReport {
    /// Fresh, empty, current, supported-legacy, or unsupported source state.
    source: SchemaSourceState,
    /// Independent `SQLite` `PRAGMA user_version` value.
    user_version: i64,
    /// Required table/column and index observations.
    required_objects: Vec<RequiredSchemaObjectState>,
    /// Runtime `SQLite` capability observations.
    sqlite_features: SqliteFeatureState,
    /// Read-only quick-check outcome.
    integrity: IntegrityState,
    /// Journal mode and sidecar observations.
    sidecars: SqliteSidecarState,
    /// Stored root and project-instance binding.
    root_binding: RootBindingState,
    /// Inclusive supported metadata schema source range.
    supported_source_range: (i64, i64),
    /// Filesystem capacity for backup planning.
    free_space: FreeSpaceState,
    /// Backup and rollback feasibility.
    backup: BackupReadiness,
    /// Runtime migration-ledger shape, order, metadata, and digest state.
    migration_ledger: MigrationLedgerState,
    /// Conflicting or leftover partial-object diagnostics.
    conflicting_partial_objects: Vec<String>,
}

impl SchemaPreflightReport {
    /// Reject any state that cannot safely precede current initialization.
    fn ensure_write_ready(&self) -> DbResult<()> {
        if self.supported_source_range != (MIN_SUPPORTED_SCHEMA_VERSION, SCHEMA_VERSION) {
            return Err(preflight_error(
                "supported source range is internally inconsistent",
            ));
        }
        if let SchemaSourceState::Unsupported { version } = self.source {
            return Err(DbError::SchemaVersion {
                found: version,
                expected: SCHEMA_VERSION,
            });
        }
        if self.user_version < 0 {
            return Err(preflight_error("SQLite user_version cannot be negative"));
        }
        if self.sqlite_features.version.is_empty()
            || self.sqlite_features.compile_options.is_empty()
        {
            return Err(preflight_error(
                "SQLite runtime version or compile-option inventory is unavailable",
            ));
        }
        let fts5_declared = self
            .sqlite_features
            .compile_options
            .iter()
            .any(|option| option == "ENABLE_FTS5");
        let json_omitted = self
            .sqlite_features
            .compile_options
            .iter()
            .any(|option| option == "OMIT_JSON");
        if self.sqlite_features.fts5 != fts5_declared || self.sqlite_features.json == json_omitted {
            return Err(preflight_error(
                "SQLite feature probes disagree with the compile-option inventory",
            ));
        }
        if let IntegrityState::Failed(details) = &self.integrity {
            return Err(preflight_error(format!(
                "SQLite quick_check failed: {}",
                details.join("; ")
            )));
        }
        if self.sidecars.journal_mode.is_empty() {
            return Err(preflight_error("SQLite journal mode was not recorded"));
        }
        if self.sidecars.rollback_journal.exists && self.sidecars.rollback_journal.bytes > 0 {
            return Err(preflight_error(format!(
                "hot or unresolved rollback journal exists at {}",
                self.sidecars.rollback_journal.path.display()
            )));
        }
        if self.sidecars.wal.exists && !self.sidecars.shm.exists {
            return Err(preflight_error(format!(
                "WAL sidecar exists without shared-memory sidecar at {}",
                self.sidecars.wal.path.display()
            )));
        }
        for sidecar in [
            &self.sidecars.wal,
            &self.sidecars.shm,
            &self.sidecars.rollback_journal,
        ] {
            if !sidecar.exists && sidecar.bytes != 0 {
                return Err(preflight_error(format!(
                    "absent SQLite sidecar {} has a nonzero byte count",
                    sidecar.path.display()
                )));
            }
        }
        match self.root_binding.status {
            RootBindingStatus::Unbound => {
                if self.root_binding.stored_root.is_some() {
                    return Err(preflight_error(
                        "unbound database unexpectedly records a project root",
                    ));
                }
            }
            RootBindingStatus::UnverifiedLocation => {
                if self.root_binding.stored_root.is_none()
                    || self.root_binding.expected_root.is_some()
                {
                    return Err(preflight_error(
                        "unverified root binding has inconsistent stored or inferred roots",
                    ));
                }
            }
            RootBindingStatus::Bound => {
                if self.root_binding.stored_root != self.root_binding.expected_root
                    || (self.root_binding.project_instance_id.is_none()
                        && !self.source.migration_required())
                {
                    return Err(preflight_error(
                        "bound database has inconsistent root or project-instance metadata",
                    ));
                }
            }
            RootBindingStatus::Mismatch => {
                return Err(preflight_error(format!(
                    "database root binding {:?} does not match project-local root {:?} (project_instance_id={:?})",
                    self.root_binding.stored_root,
                    self.root_binding.expected_root,
                    self.root_binding.project_instance_id
                )));
            }
        }
        for object in &self.required_objects {
            if object.name.is_empty() || object.table_name.is_empty() || object.columns.is_empty() {
                return Err(preflight_error(format!(
                    "required {} contract is incomplete for {:?}",
                    object.kind.as_str(),
                    object.name
                )));
            }
            match &object.status {
                RequiredSchemaObjectStatus::Missing => {
                    if !self.source.allows_incomplete_current_objects() {
                        return Err(preflight_error(format!(
                            "current {} {} is missing",
                            object.kind.as_str(),
                            object.name
                        )));
                    }
                }
                RequiredSchemaObjectStatus::Matches => {}
                RequiredSchemaObjectStatus::LegacyPartial { missing_columns } => {
                    if !self.source.allows_incomplete_current_objects()
                        || missing_columns.is_empty()
                        || missing_columns
                            .iter()
                            .any(|column| !object.columns.iter().any(|required| required == column))
                    {
                        return Err(preflight_error(format!(
                            "legacy-partial {} contract is inconsistent for {:?}",
                            object.kind.as_str(),
                            object.name
                        )));
                    }
                }
                RequiredSchemaObjectStatus::Conflict { detail } => {
                    if !self
                        .conflicting_partial_objects
                        .iter()
                        .any(|conflict| conflict == detail)
                    {
                        return Err(preflight_error(format!(
                            "object conflict was not retained in the report: {detail}"
                        )));
                    }
                }
            }
        }
        if !self.conflicting_partial_objects.is_empty() {
            return Err(preflight_error(format!(
                "conflicting schema objects: {}",
                self.conflicting_partial_objects.join("; ")
            )));
        }
        if self.free_space.sufficient
            != (self.free_space.available_bytes >= self.free_space.required_bytes)
            || self.backup.required != self.source.migration_required()
        {
            return Err(preflight_error(
                "free-space or backup readiness does not match the observed source state",
            ));
        }
        if self.source.migration_required()
            && (!self.free_space.sufficient
                || !self.backup.required
                || !self.backup.backup_ready
                || !self.backup.rollback_ready)
        {
            return Err(preflight_error(format!(
                "migration backup is not ready: available={} required={} backup_ready={} rollback_ready={}",
                self.free_space.available_bytes,
                self.free_space.required_bytes,
                self.backup.backup_ready,
                self.backup.rollback_ready
            )));
        }
        match &self.migration_ledger {
            MigrationLedgerState::Absent if !self.source.migration_required() => {
                if matches!(self.source, SchemaSourceState::Supported { .. }) {
                    return Err(preflight_error(
                        "current schema is missing the runtime migration ledger",
                    ));
                }
            }
            MigrationLedgerState::Absent | MigrationLedgerState::Compatible(_) => {}
            MigrationLedgerState::Rejected(detail) => {
                return Err(preflight_error(format!(
                    "migration ledger is incompatible: {detail}"
                )));
            }
        }
        Ok(())
    }
}

/// Convert a write-ready report into the exact pending migration sequence.
fn migration_plan(report: &SchemaPreflightReport) -> DbResult<SchemaMigrationPlan> {
    report.ensure_write_ready()?;
    let source_version = match report.source {
        SchemaSourceState::Fresh | SchemaSourceState::Empty => 0,
        SchemaSourceState::Supported { version, .. } => version,
        SchemaSourceState::Unsupported { version } => {
            return Err(DbError::SchemaVersion {
                found: version,
                expected: SCHEMA_VERSION,
            });
        }
    };
    Ok(SchemaMigrationPlan {
        source_version,
        target_version: SCHEMA_VERSION,
        pending: allocated_migrations()
            .into_iter()
            .filter(|migration| migration.schema_version > source_version)
            .collect(),
    })
}

/// Minimal `SQLite` schema-object identity used during preflight.
#[derive(Debug)]
struct SqliteObjectState {
    /// `SQLite` object kind such as table, index, view, or trigger.
    object_type: String,
    /// Owning table reported by `SQLite`.
    table_name: String,
}

/// Run the file-backed read-only preflight before a write-capable open.
pub(crate) fn preflight(path: &Path) -> DbResult<SchemaMigrationPlan> {
    let report = inspect_schema_preflight(path)?;
    migration_plan(&report)
}

/// Inspect and retain the complete file-backed preflight report.
fn inspect_schema_preflight(path: &Path) -> DbResult<SchemaPreflightReport> {
    if path.exists() {
        inspect_existing_database(path)
    } else {
        inspect_fresh_database(path)
    }
}

/// Record the explicitly reduced preflight for a new in-memory database.
///
/// Filesystem capacity, sidecars, root binding, and backup readiness do not
/// apply because the database has no persistent target and cannot migrate one.
pub(crate) fn preflight_in_memory() -> DbResult<SchemaMigrationPlan> {
    let connection = Connection::open_in_memory()?;
    let features = sqlite_feature_state(&connection)?;
    if features.version.is_empty() || features.compile_options.is_empty() {
        return Err(preflight_error(
            "in-memory SQLite runtime capability inventory is unavailable",
        ));
    }
    match integrity_state(&connection)? {
        IntegrityState::Passed => Ok(SchemaMigrationPlan::fresh()),
        IntegrityState::Failed(details) => Err(preflight_error(format!(
            "in-memory SQLite quick_check failed: {}",
            details.join("; ")
        ))),
    }
}

/// Inspect a database path that does not exist yet without creating it.
fn inspect_fresh_database(path: &Path) -> DbResult<SchemaPreflightReport> {
    let parent = database_parent(path)?;
    let available_bytes = available_space(parent)?;
    let (reference, contract) = current_schema_reference()?;
    Ok(SchemaPreflightReport {
        source: SchemaSourceState::Fresh,
        user_version: 0,
        required_objects: contract
            .into_iter()
            .map(|object| RequiredSchemaObjectState {
                kind: object.kind,
                name: object.name,
                table_name: object.table_name,
                columns: object.columns,
                status: RequiredSchemaObjectStatus::Missing,
            })
            .collect(),
        sqlite_features: sqlite_feature_state(&reference)?,
        integrity: integrity_state(&reference)?,
        sidecars: sidecar_state(path, "none")?,
        root_binding: RootBindingState {
            status: RootBindingStatus::Unbound,
            stored_root: None,
            expected_root: inferred_project_root(path),
            project_instance_id: None,
        },
        supported_source_range: (MIN_SUPPORTED_SCHEMA_VERSION, SCHEMA_VERSION),
        free_space: FreeSpaceState {
            available_bytes,
            required_bytes: 0,
            sufficient: true,
        },
        backup: BackupReadiness {
            required: false,
            backup_ready: parent_is_writable(parent)?,
            rollback_ready: true,
        },
        migration_ledger: MigrationLedgerState::Absent,
        conflicting_partial_objects: Vec::new(),
    })
}

/// Inspect one existing database through a read-only `SQLite` connection.
fn inspect_existing_database(path: &Path) -> DbResult<SchemaPreflightReport> {
    let before_sidecars = sidecar_state(path, "unknown")?;
    if before_sidecars.rollback_journal.exists && before_sidecars.rollback_journal.bytes > 0 {
        return Err(preflight_error(format!(
            "hot or unresolved rollback journal exists at {}",
            before_sidecars.rollback_journal.path.display()
        )));
    }
    if before_sidecars.wal.exists && !before_sidecars.shm.exists {
        return Err(preflight_error(format!(
            "WAL sidecar exists without shared-memory sidecar at {}",
            before_sidecars.wal.path.display()
        )));
    }

    // Immutable inspection avoids creating lock sidecars for a main-file-only
    // database. An existing WAL uses SQLite's read-only WAL path so committed
    // frames remain part of the preflight.
    let uri = sqlite_read_uri(path, !before_sidecars.wal.exists);
    let connection = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;
    let inspection = (|| {
        let user_version = read_user_version(&connection)?;
        let objects = sqlite_objects(&connection)?;
        let source = schema_source_state(&connection, &objects)?;
        let (_, contract) = current_schema_reference()?;
        let (required_objects, mut conflicts) =
            required_schema_object_states(&connection, &objects, contract, source)?;
        conflicts.extend(leftover_partial_objects(&objects, &required_objects));
        conflicts.sort();
        conflicts.dedup();

        let root_binding = root_binding_state(&connection, path, &objects, source)?;
        let migration_ledger = migration_ledger_state(&connection, &objects, source)?;
        let database_bytes = fs::metadata(path)
            .map_err(|error| filesystem_error(path, "read database metadata", &error))?
            .len();
        let required_bytes = database_bytes
            .saturating_add(before_sidecars.wal.bytes)
            .saturating_add(BACKUP_HEADROOM_BYTES);
        let parent = database_parent(path)?;
        let available_bytes = available_space(parent)?;
        let backup_ready = parent_is_writable(parent)? && available_bytes >= required_bytes;
        let journal_mode =
            connection.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))?;

        Ok(SchemaPreflightReport {
            source,
            user_version,
            required_objects,
            sqlite_features: sqlite_feature_state(&connection)?,
            integrity: integrity_state(&connection)?,
            sidecars: SqliteSidecarState {
                journal_mode,
                ..before_sidecars.clone()
            },
            root_binding,
            supported_source_range: (MIN_SUPPORTED_SCHEMA_VERSION, SCHEMA_VERSION),
            free_space: FreeSpaceState {
                available_bytes,
                required_bytes,
                sufficient: available_bytes >= required_bytes,
            },
            backup: BackupReadiness {
                required: source.migration_required(),
                backup_ready,
                rollback_ready: backup_ready,
            },
            migration_ledger,
            conflicting_partial_objects: conflicts,
        })
    })();
    drop(connection);
    // Keep the sidecar inventory captured immediately before inspection. Another
    // process may append or checkpoint WAL frames while this read-only preflight
    // runs, so an after-the-fact filesystem comparison cannot attribute a change
    // to the inspector and would reject valid concurrent opens.
    inspection
}

/// Build the authoritative current object contract in a throwaway database.
fn current_schema_reference() -> DbResult<(Connection, Vec<SchemaObjectContract>)> {
    let mut connection = Connection::open_in_memory()?;
    apply_migration_plan(&mut connection, &SchemaMigrationPlan::fresh())?;
    let contracts = schema_object_contracts(&connection)?;
    Ok((connection, contracts))
}

/// Require every structural table/index/trigger DDL contract to match the initializer.
fn verify_exact_structural_schema_sql(connection: &Connection) -> DbResult<()> {
    let expected = STRUCTURAL_SCHEMA_SQL_REFERENCE.get_or_init(|| {
        current_schema_reference()
            .and_then(|(connection, _)| structural_schema_sql(&connection))
            .map_err(|error| error.to_string())
    });
    let expected = expected.as_ref().map_err(|message| {
        preflight_error(format!(
            "failed to build the authoritative structural schema SQL reference: {message}"
        ))
    })?;
    let observed = structural_schema_sql(connection)?;
    if &observed == expected {
        return Ok(());
    }
    let names = expected
        .keys()
        .chain(observed.keys())
        .collect::<std::collections::BTreeSet<_>>();
    let differences = names
        .into_iter()
        .filter_map(|name| {
            let expected_sql = expected.get(name);
            let observed_sql = observed.get(name);
            (expected_sql != observed_sql)
                .then(|| format!("{name:?}: expected={expected_sql:?}, observed={observed_sql:?}"))
        })
        .collect::<Vec<_>>();
    Err(preflight_error(format!(
        "structural schema object canonical SQL did not reconcile: {}",
        differences.join("; ")
    )))
}

/// Load normalized SQL for required structural objects and optional FTS objects.
fn structural_schema_sql(connection: &Connection) -> DbResult<BTreeMap<String, String>> {
    let mut contracts = BTreeMap::new();
    for name in STRUCTURAL_SCHEMA_SQL_OBJECTS {
        let sql = schema_sql(connection, name)?.ok_or_else(|| {
            preflight_error(format!(
                "required structural schema object {name:?} has no canonical SQL"
            ))
        })?;
        contracts.insert(name.to_owned(), normalize_schema_sql(&sql));
    }
    for name in OPTIONAL_STRUCTURAL_SCHEMA_SQL_OBJECTS {
        if let Some(sql) = schema_sql(connection, name)? {
            contracts.insert(name.to_owned(), normalize_schema_sql(&sql));
        }
    }
    Ok(contracts)
}

/// Read one named object's stored SQL, distinguishing absence from a null definition.
fn schema_sql(connection: &Connection, name: &str) -> DbResult<Option<String>> {
    connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name = ?1",
            [name],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map(Option::flatten)
        .map_err(DbError::from)
}

/// Read the independent `SQLite` application user-version integer.
fn read_user_version(connection: &Connection) -> DbResult<i64> {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(DbError::from)
}

/// Record `SQLite` runtime version, compile options, and required feature probes.
fn sqlite_feature_state(connection: &Connection) -> DbResult<SqliteFeatureState> {
    let version =
        connection.query_row("SELECT sqlite_version()", [], |row| row.get::<_, String>(0))?;
    let mut statement = connection.prepare("PRAGMA compile_options")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut compile_options = rows.collect::<Result<Vec<_>, _>>()?;
    compile_options.sort();
    let fts5 = connection.query_row(
        "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
        [],
        |row| row.get::<_, i64>(0),
    )? != 0;
    let json = connection.query_row(
        "SELECT NOT sqlite_compileoption_used('OMIT_JSON')",
        [],
        |row| row.get::<_, i64>(0),
    )? != 0;
    Ok(SqliteFeatureState {
        version,
        compile_options,
        fts5,
        json,
    })
}

/// Run and retain every `PRAGMA quick_check` result row.
fn integrity_state(connection: &Connection) -> DbResult<IntegrityState> {
    let mut statement = connection.prepare("PRAGMA quick_check")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let details = rows.collect::<Result<Vec<_>, _>>()?;
    if details == ["ok"] {
        Ok(IntegrityState::Passed)
    } else {
        Ok(IntegrityState::Failed(details))
    }
}

/// Load non-internal `SQLite` object names, kinds, and owning tables.
fn sqlite_objects(connection: &Connection) -> DbResult<BTreeMap<String, SqliteObjectState>> {
    let mut statement = connection.prepare(
        "SELECT name, type, tbl_name FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            SqliteObjectState {
                object_type: row.get(1)?,
                table_name: row.get(2)?,
            },
        ))
    })?;
    rows.collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(DbError::from)
}

/// Derive required table/column and named-index contracts from one schema.
fn schema_object_contracts(connection: &Connection) -> DbResult<Vec<SchemaObjectContract>> {
    let mut statement = connection.prepare(
        "SELECT type, name, tbl_name
         FROM sqlite_schema
         WHERE type IN ('table', 'index') AND name NOT LIKE 'sqlite_%'
         ORDER BY type, name",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut contracts = Vec::new();
    for row in rows {
        let (object_type, name, table_name) = row?;
        if is_optional_file_text_fts_object(&name, &table_name) {
            continue;
        }
        let kind = SchemaObjectKind::from_sql(&object_type)?;
        let columns = schema_object_columns(connection, kind, &name)?;
        if columns.is_empty() {
            return Err(preflight_error(format!(
                "current {} contract {name:?} has no columns",
                kind.as_str()
            )));
        }
        contracts.push(SchemaObjectContract {
            kind,
            name,
            table_name,
            columns,
        });
    }
    Ok(contracts)
}

/// Return whether one schema object belongs to the optional FTS capability.
fn is_optional_file_text_fts_object(name: &str, table_name: &str) -> bool {
    name == FILE_TEXT_FTS_TABLE
        || table_name == FILE_TEXT_FTS_TABLE
        || name
            .strip_prefix(FILE_TEXT_FTS_TABLE)
            .is_some_and(|suffix| suffix.starts_with('_'))
        || table_name
            .strip_prefix(FILE_TEXT_FTS_TABLE)
            .is_some_and(|suffix| suffix.starts_with('_'))
}

/// Return ordered columns for one trusted schema-owned table or index name.
fn schema_object_columns(
    connection: &Connection,
    kind: SchemaObjectKind,
    name: &str,
) -> DbResult<Vec<String>> {
    let pragma = match kind {
        SchemaObjectKind::Table => "table_info",
        SchemaObjectKind::Index => "index_info",
    };
    let column = match kind {
        SchemaObjectKind::Table => 1,
        SchemaObjectKind::Index => 2,
    };
    let mut statement = connection.prepare(&format!("PRAGMA {pragma}({name})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(column))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
}

/// Classify the metadata schema source without changing it.
fn schema_source_state(
    connection: &Connection,
    objects: &BTreeMap<String, SqliteObjectState>,
) -> DbResult<SchemaSourceState> {
    if objects.is_empty() {
        return Ok(SchemaSourceState::Empty);
    }
    let Some(metadata) = objects.get("metadata") else {
        return Err(preflight_error(
            "database contains ProjectAtlas-like objects without the metadata table",
        ));
    };
    if metadata.object_type != "table" {
        return Err(preflight_error(format!(
            "metadata is a {}, not a table",
            metadata.object_type
        )));
    }
    let columns = table_columns(connection, "metadata")?;
    if !["key", "value"]
        .iter()
        .all(|required| columns.iter().any(|column| column == required))
    {
        return Err(preflight_error(
            "metadata table does not contain the required key/value columns",
        ));
    }
    let value = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [SCHEMA_VERSION_METADATA_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| preflight_error("metadata schema_version row is missing"))?;
    let version = value.parse::<i64>().map_err(|error| {
        preflight_error(format!(
            "metadata schema_version {value:?} is not an integer: {error}"
        ))
    })?;
    if !(MIN_SUPPORTED_SCHEMA_VERSION..=SCHEMA_VERSION).contains(&version) {
        return Ok(SchemaSourceState::Unsupported { version });
    }
    Ok(SchemaSourceState::Supported {
        version,
        migration_required: version < SCHEMA_VERSION,
    })
}

/// Validate the append-only migration history against the runtime inventory.
fn migration_ledger_state(
    connection: &Connection,
    objects: &BTreeMap<String, SqliteObjectState>,
    source: SchemaSourceState,
) -> DbResult<MigrationLedgerState> {
    let Some(object) = objects.get(MIGRATION_LEDGER_TABLE) else {
        return Ok(MigrationLedgerState::Absent);
    };
    if object.object_type != "table" {
        return Ok(MigrationLedgerState::Rejected(format!(
            "{MIGRATION_LEDGER_TABLE} is a {}, not a table",
            object.object_type
        )));
    }
    if table_columns(connection, MIGRATION_LEDGER_TABLE)?
        != ["schema_version", "migration_id", "checksum"]
    {
        return Ok(MigrationLedgerState::Rejected(format!(
            "{MIGRATION_LEDGER_TABLE} has an unsupported column contract"
        )));
    }

    let query = format!(
        "SELECT schema_version, migration_id, checksum FROM {MIGRATION_LEDGER_TABLE} ORDER BY schema_version"
    );
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map([], |row| {
        Ok(MigrationLedgerRecord {
            schema_version: row.get(0)?,
            migration_id: row.get(1)?,
            checksum: row.get(2)?,
        })
    })?;
    let records = rows.collect::<Result<Vec<_>, _>>()?;
    let source_version = match source {
        SchemaSourceState::Fresh | SchemaSourceState::Empty => 0,
        SchemaSourceState::Supported { version, .. }
        | SchemaSourceState::Unsupported { version } => version,
    };
    let expected = allocated_migrations()
        .into_iter()
        .filter(|migration| migration.schema_version <= source_version)
        .collect::<Vec<_>>();
    if records.len() != expected.len() {
        return Ok(MigrationLedgerState::Rejected(format!(
            "declared schema version {source_version} has {} ledger rows; expected {}",
            records.len(),
            expected.len()
        )));
    }
    for (record, migration) in records.iter().zip(&expected) {
        if record.schema_version != migration.schema_version
            || record.migration_id != migration.definition.id
            || record.checksum != migration.checksum
        {
            return Ok(MigrationLedgerState::Rejected(format!(
                "schema version {} has migration {:?} digest {:?}; expected {:?} digest {:?}",
                record.schema_version,
                record.migration_id,
                record.checksum,
                migration.definition.id,
                migration.checksum
            )));
        }
    }
    Ok(MigrationLedgerState::Compatible(records))
}

/// Compare one target database with the authoritative current object contract.
fn required_schema_object_states(
    connection: &Connection,
    objects: &BTreeMap<String, SqliteObjectState>,
    contracts: Vec<SchemaObjectContract>,
    source: SchemaSourceState,
) -> DbResult<(Vec<RequiredSchemaObjectState>, Vec<String>)> {
    let mut states = Vec::with_capacity(contracts.len());
    let mut conflicts = Vec::new();
    for contract in contracts {
        let status = match objects.get(&contract.name) {
            None => RequiredSchemaObjectStatus::Missing,
            Some(actual) if actual.object_type != contract.kind.as_str() => {
                RequiredSchemaObjectStatus::Conflict {
                    detail: format!(
                        "required {} {} exists as {}",
                        contract.kind.as_str(),
                        contract.name,
                        actual.object_type
                    ),
                }
            }
            Some(actual) => {
                let actual_columns =
                    schema_object_columns(connection, contract.kind, &contract.name)?;
                if actual.table_name == contract.table_name
                    && schema_object_columns_match(
                        contract.kind,
                        &actual_columns,
                        &contract.columns,
                    )
                {
                    RequiredSchemaObjectStatus::Matches
                } else if source.migration_required()
                    && contract.kind == SchemaObjectKind::Table
                    && actual.table_name == contract.table_name
                    && actual_columns
                        .iter()
                        .all(|column| contract.columns.iter().any(|required| required == column))
                {
                    let missing_columns = contract
                        .columns
                        .iter()
                        .filter(|required| !actual_columns.iter().any(|column| column == *required))
                        .cloned()
                        .collect();
                    RequiredSchemaObjectStatus::LegacyPartial { missing_columns }
                } else {
                    RequiredSchemaObjectStatus::Conflict {
                        detail: format!(
                            "required {} {} expected table {:?} columns {:?}, found table {:?} columns {:?}",
                            contract.kind.as_str(),
                            contract.name,
                            contract.table_name,
                            contract.columns,
                            actual.table_name,
                            actual_columns
                        ),
                    }
                }
            }
        };
        if let RequiredSchemaObjectStatus::Conflict { detail } = &status {
            conflicts.push(detail.clone());
        } else if !source.allows_incomplete_current_objects()
            && !matches!(status, RequiredSchemaObjectStatus::Matches)
        {
            conflicts.push(format!(
                "current {} {} is missing or incomplete",
                contract.kind.as_str(),
                contract.name
            ));
        }
        states.push(RequiredSchemaObjectState {
            kind: contract.kind,
            name: contract.name,
            table_name: contract.table_name,
            columns: contract.columns,
            status,
        });
    }
    Ok((states, conflicts))
}

/// Compare table columns by membership while preserving index-column order.
fn schema_object_columns_match(
    kind: SchemaObjectKind,
    actual: &[String],
    required: &[String],
) -> bool {
    actual.len() == required.len()
        && match kind {
            SchemaObjectKind::Table => actual
                .iter()
                .all(|column| required.iter().any(|candidate| candidate == column)),
            SchemaObjectKind::Index => actual == required,
        }
}

/// Find reserved migration leftovers such as `nodes_new` or `idx_name_tmp`.
fn leftover_partial_objects(
    objects: &BTreeMap<String, SqliteObjectState>,
    required: &[RequiredSchemaObjectState],
) -> Vec<String> {
    let mut conflicts = Vec::new();
    for name in objects.keys() {
        for suffix in ["_new", "_old", "_backup", "_tmp"] {
            let Some(base) = name.strip_suffix(suffix) else {
                continue;
            };
            if required.iter().any(|object| object.name == base) {
                conflicts.push(format!("leftover partial schema object {name}"));
            }
        }
    }
    conflicts
}

/// Load project-root and persistent-instance metadata after shape validation.
fn root_binding_state(
    connection: &Connection,
    path: &Path,
    objects: &BTreeMap<String, SqliteObjectState>,
    source: SchemaSourceState,
) -> DbResult<RootBindingState> {
    if objects
        .get("metadata")
        .is_none_or(|object| object.object_type != "table")
    {
        return Ok(RootBindingState {
            status: RootBindingStatus::Unbound,
            stored_root: None,
            expected_root: inferred_project_root(path),
            project_instance_id: None,
        });
    }
    let stored_root = metadata_value(connection, "project_root")?;
    let stored_identity = metadata_value(connection, PROJECT_INSTANCE_ID_METADATA_KEY)?;
    let project_instance_id = stored_identity.map(parse_project_instance_id).transpose()?;
    if matches!(
        source,
        SchemaSourceState::Supported {
            migration_required: false,
            ..
        }
    ) && project_instance_id.is_none()
    {
        return Err(DbError::ProjectInstanceIdMissing);
    }
    let expected_root = inferred_project_root(path);
    let status = match (&stored_root, &expected_root) {
        (None, _) => RootBindingStatus::Unbound,
        (Some(_), None) => RootBindingStatus::UnverifiedLocation,
        (Some(stored), Some(expected)) if stored == expected => RootBindingStatus::Bound,
        (Some(_), Some(_)) => RootBindingStatus::Mismatch,
    };
    Ok(RootBindingState {
        status,
        stored_root,
        expected_root,
        project_instance_id,
    })
}

/// Read one optional metadata value from a validated metadata table.
fn metadata_value(connection: &Connection, key: &str) -> DbResult<Option<String>> {
    connection
        .query_row("SELECT value FROM metadata WHERE key = ?1", [key], |row| {
            row.get::<_, String>(0)
        })
        .optional()
        .map_err(DbError::from)
}

/// Infer the owning project root only for a standard project-local database path.
fn inferred_project_root(path: &Path) -> Option<String> {
    let atlas_directory = path.parent()?;
    if atlas_directory.file_name()? != PROJECTATLAS_DIRECTORY_NAME {
        return None;
    }
    let project_root = atlas_directory.parent()?;
    let project_root = if project_root.as_os_str().is_empty() {
        Path::new(".")
    } else {
        project_root
    };
    let project_root = std::path::absolute(project_root).ok()?;
    Some(normalize_native_path_display(project_root))
}

/// Record journal mode and all `SQLite` sidecar file sizes.
fn sidecar_state(path: &Path, journal_mode: &str) -> DbResult<SqliteSidecarState> {
    Ok(SqliteSidecarState {
        journal_mode: journal_mode.to_string(),
        wal: sidecar_file_state(path, "-wal")?,
        shm: sidecar_file_state(path, "-shm")?,
        rollback_journal: sidecar_file_state(path, "-journal")?,
    })
}

/// Read one filesystem sidecar state without creating it.
fn sidecar_file_state(path: &Path, suffix: &str) -> DbResult<SidecarFileState> {
    let sidecar_path = sqlite_sidecar_path(path, suffix);
    match fs::metadata(&sidecar_path) {
        Ok(metadata) => Ok(SidecarFileState {
            path: sidecar_path,
            exists: true,
            bytes: metadata.len(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(SidecarFileState {
            path: sidecar_path,
            exists: false,
            bytes: 0,
        }),
        Err(error) => Err(filesystem_error(
            &sidecar_path,
            "inspect SQLite sidecar",
            &error,
        )),
    }
}

/// Append a `SQLite` sidecar suffix without requiring a Unicode path conversion.
fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(suffix);
    PathBuf::from(value)
}

/// Return the existing parent directory used for file and space checks.
fn database_parent(path: &Path) -> DbResult<&Path> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(preflight_error(format!(
            "database parent directory does not exist: {}",
            parent.display()
        )));
    }
    Ok(parent)
}

/// Return available bytes through the safe cross-platform filesystem-stat crate.
#[cfg(any(unix, windows))]
fn available_space(path: &Path) -> DbResult<u64> {
    fs4::available_space(path)
        .map_err(|error| filesystem_error(path, "inspect available filesystem space", &error))
}

/// Reject unsupported targets rather than guessing migration capacity.
#[cfg(not(any(unix, windows)))]
fn available_space(path: &Path) -> DbResult<u64> {
    Err(preflight_error(format!(
        "available filesystem space is unsupported on this target for {}",
        path.display()
    )))
}

/// Check the parent directory's coarse write permission before backup planning.
fn parent_is_writable(path: &Path) -> DbResult<bool> {
    fs::metadata(path)
        .map(|metadata| !metadata.permissions().readonly())
        .map_err(|error| filesystem_error(path, "inspect parent permissions", &error))
}

/// Build one contextual filesystem preflight error.
fn filesystem_error(path: &Path, operation: &str, error: &std::io::Error) -> DbError {
    preflight_error(format!(
        "{operation} failed for {}: {error}",
        path.display()
    ))
}

/// Build one typed database preflight rejection.
fn preflight_error(message: impl Into<String>) -> DbError {
    DbError::SchemaPreflight {
        message: message.into(),
    }
}

/// Apply a preflight-derived migration plan through one `SQLite` transaction.
pub(crate) fn apply_migration_plan(
    connection: &mut Connection,
    plan: &SchemaMigrationPlan,
) -> DbResult<()> {
    validate_migration_plan(plan)?;
    if plan.pending.is_empty() {
        return verify_current_schema(connection);
    }

    let transaction = connection.transaction()?;
    let observed_source = observed_schema_version(&transaction)?;
    if observed_source != plan.source_version {
        return Err(preflight_error(format!(
            "schema changed after preflight: planned source {} but writable transaction observed {observed_source}",
            plan.source_version
        )));
    }

    initialize_schema_objects(&transaction)?;
    ensure_project_instance_id(&transaction, true)?;
    for migration in &plan.pending {
        (migration.definition.apply)(&transaction)?;
        (migration.definition.verify)(&transaction)?;
        record_applied_migration(&transaction, migration)?;
        write_schema_version(&transaction, migration.schema_version)?;
    }
    verify_schema_history(&transaction, plan.target_version)?;
    transaction.commit()?;
    Ok(())
}

/// Revalidate an already initialized store without issuing schema writes.
pub(crate) fn verify_current_schema(connection: &Connection) -> DbResult<()> {
    verify_current_schema_objects(connection)?;
    reconcile_full_file_text_fts(connection)?;
    reconcile_foreign_key_integrity(connection)
}

/// Reconcile schema-owned objects before inspecting publication content.
fn verify_current_schema_objects(connection: &Connection) -> DbResult<()> {
    verify_schema_history(connection, SCHEMA_VERSION)?;
    verify_structural_publication_schema(connection)?;
    verify_structural_derived_row_schema(connection)?;
    verify_exact_structural_schema_sql(connection)
}

/// Reconcile one complete structural publication inside its owning transaction.
pub(crate) fn reconcile_full_structural_publication(
    connection: &Connection,
    expected_root: &str,
    expected_publication: PublicationState,
) -> DbResult<()> {
    reconcile_quick_check(connection)?;
    verify_current_schema_objects(connection)?;
    reconcile_project_root(connection, expected_root)?;
    reconcile_publication_state(connection, expected_publication)?;
    reconcile_structural_slots_and_epochs(connection, expected_publication.active_epoch)?;
    reconcile_structural_text(connection)?;
    reconcile_resolution_candidate_uniqueness(connection)?;
    reconcile_relation_families(connection)?;
    reconcile_foreign_key_integrity(connection)?;
    reconcile_resolution_candidate_counts(connection)?;
    reconcile_coverage(connection)?;
    reconcile_full_file_text_fts(connection)
}

/// Reconcile only the active-slot closure changed by one incremental publication.
pub(crate) fn reconcile_incremental_structural_publication(
    connection: &Connection,
    expected_root: &str,
    expected_publication: PublicationState,
    affected_paths: &[String],
) -> DbResult<()> {
    verify_current_schema_objects(connection)?;
    reconcile_project_root(connection, expected_root)?;
    reconcile_publication_state(connection, expected_publication)?;
    require_foreign_keys_enabled(connection)?;
    let active_slot = structural_slot_text(expected_publication.active_slot);
    let active_epoch = sqlite_epoch(expected_publication.active_epoch)?;
    reconcile_affected_structural_invalidation(connection, active_slot, affected_paths)?;
    reconcile_affected_file_texts(connection, active_slot, active_epoch, affected_paths)?;
    reconcile_affected_file_text_fts(connection, active_slot, affected_paths)
}

/// Require native foreign-key enforcement for the affected-row proof.
fn require_foreign_keys_enabled(connection: &Connection) -> DbResult<()> {
    let enabled = connection.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))?;
    if enabled == 1 {
        Ok(())
    } else {
        Err(preflight_error(
            "incremental structural reconciliation requires SQLite foreign_keys=ON",
        ))
    }
}

/// Require the complete `SQLite` quick-check result to be exactly successful.
fn reconcile_quick_check(connection: &Connection) -> DbResult<()> {
    match integrity_state(connection)? {
        IntegrityState::Passed => Ok(()),
        IntegrityState::Failed(details) => Err(preflight_error(format!(
            "SQLite quick_check failed before structural publication: {}",
            details.join("; ")
        ))),
    }
}

/// Require the complete foreign-key diagnostic page to be empty.
fn reconcile_foreign_key_integrity(connection: &Connection) -> DbResult<()> {
    let mut statement = connection.prepare("PRAGMA foreign_key_check")?;
    let rows = statement.query_map([], |row| {
        Ok(format!(
            "table={:?}, row={:?}, parent={:?}, foreign-key={}",
            row.get::<_, String>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?
        ))
    })?;
    let violations = rows.collect::<Result<Vec<_>, _>>()?;
    if violations.is_empty() {
        Ok(())
    } else {
        Err(preflight_error(format!(
            "structural endpoint foreign-key reconciliation failed: {}",
            violations.join("; ")
        )))
    }
}

/// Require exact canonical root metadata at the publication boundary.
fn reconcile_project_root(connection: &Connection, expected_root: &str) -> DbResult<()> {
    let observed = metadata_value(connection, "project_root")?;
    if observed.as_deref() == Some(expected_root) {
        Ok(())
    } else {
        Err(preflight_error(format!(
            "structural publication project root did not reconcile: expected {expected_root:?}, found {observed:?}"
        )))
    }
}

/// Require the singleton to equal the state that the transaction will publish.
fn reconcile_publication_state(
    connection: &Connection,
    expected: PublicationState,
) -> DbResult<()> {
    let mut statement = connection.prepare(
        "SELECT singleton, active_slot, active_epoch
         FROM graph_publication_state ORDER BY singleton",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let observed = rows.collect::<Result<Vec<_>, _>>()?;
    let expected_row = (
        1,
        structural_slot_text(expected.active_slot).to_owned(),
        sqlite_epoch(expected.active_epoch)?,
    );
    if observed == [expected_row] {
        Ok(())
    } else {
        Err(preflight_error(format!(
            "structural publication state did not reconcile: expected {expected:?}, found {observed:?}"
        )))
    }
}

/// Reject invalid physical slots and rows newer than the published epoch.
fn reconcile_structural_slots_and_epochs(
    connection: &Connection,
    expected_epoch: IndexEpoch,
) -> DbResult<()> {
    let expected_epoch = sqlite_epoch(expected_epoch)?;
    for table in STRUCTURAL_DERIVED_TABLES {
        let mut statement = connection.prepare(&format!(
            "SELECT structural_slot, last_changed_epoch
             FROM {table}
             WHERE structural_slot NOT IN ('a', 'b')
                OR last_changed_epoch < 0
                OR last_changed_epoch > ?1"
        ))?;
        let rows = statement.query_map([expected_epoch], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let invalid = rows.collect::<Result<Vec<_>, _>>()?;
        if !invalid.is_empty() {
            return Err(preflight_error(format!(
                "structural slot or epoch did not reconcile for {table:?} at published epoch {expected_epoch}: {invalid:?}"
            )));
        }
    }
    Ok(())
}

/// Require every invalidated active-slot path row to be absent after mutation.
fn reconcile_affected_structural_invalidation(
    connection: &Connection,
    active_slot: &str,
    affected_paths: &[String],
) -> DbResult<()> {
    for (table, path_column) in [
        (GRAPH_ENTITIES_TABLE, "repository_path"),
        (GRAPH_EVIDENCE_OCCURRENCES_TABLE, "origin_repository_path"),
        (GRAPH_RESOLUTION_OCCURRENCES_TABLE, "origin_repository_path"),
        (GRAPH_COVERAGE_TABLE, "repository_path"),
    ] {
        for path in affected_paths {
            if path == "." {
                let survives = connection.query_row(
                    &format!(
                        "SELECT EXISTS(
                             SELECT 1 FROM {table} WHERE structural_slot = ?1
                         )"
                    ),
                    [active_slot],
                    |row| row.get::<_, i64>(0),
                )? != 0;
                if survives {
                    return Err(preflight_error(format!(
                        "incremental root invalidation left an active-slot row in {table:?}; dependent relations and candidates are protected by enforced foreign keys"
                    )));
                }
                continue;
            }
            let (descendant_start, descendant_end) = crate::sqlite_descendant_bounds(path);
            let survives = connection.query_row(
                &format!(
                    "SELECT EXISTS(
                         SELECT 1 FROM {table}
                         WHERE structural_slot = ?1
                           AND ({path_column} = ?2
                                OR ({path_column} >= ?3 AND {path_column} < ?4))
                     )"
                ),
                params![active_slot, path, descendant_start, descendant_end],
                |row| row.get::<_, i64>(0),
            )? != 0;
            if survives {
                return Err(preflight_error(format!(
                    "incremental structural invalidation left an active-slot row in {table:?} for affected path {path:?}; dependent relations and candidates are protected by enforced foreign keys"
                )));
            }
        }
    }
    Ok(())
}

/// Require affected authoritative text rows to belong to the new active epoch.
fn reconcile_affected_file_texts(
    connection: &Connection,
    active_slot: &str,
    active_epoch: i64,
    affected_paths: &[String],
) -> DbResult<()> {
    let root_scope = affected_paths.iter().any(|path| path == ".");
    let sql = if root_scope {
        "SELECT path, content_hash, content, updated_at, last_changed_epoch
         FROM file_texts WHERE structural_slot = ?1 ORDER BY path"
    } else {
        "SELECT path, content_hash, content, updated_at, last_changed_epoch
         FROM file_texts
         WHERE structural_slot = ?1
           AND (path = ?2 OR (path >= ?3 AND path < ?4))
         ORDER BY path"
    };
    let mut statement = connection.prepare(sql)?;
    for affected_path in affected_paths {
        let mut rows = if root_scope {
            statement.query([active_slot])?
        } else {
            let (descendant_start, descendant_end) = crate::sqlite_descendant_bounds(affected_path);
            statement.query(params![
                active_slot,
                affected_path,
                descendant_start,
                descendant_end
            ])?
        };
        while let Some(row) = rows.next()? {
            let path = row.get::<_, String>(0)?;
            let _content_hash = row.get::<_, Option<String>>(1)?;
            let _content = row.get::<_, String>(2)?;
            let _updated_at = row.get::<_, String>(3)?;
            let row_epoch = row.get::<_, i64>(4)?;
            if row_epoch != active_epoch {
                return Err(preflight_error(format!(
                    "incremental file-text slot or epoch did not reconcile for {path:?}: expected slot={active_slot:?}, epoch={active_epoch}, found epoch={row_epoch}"
                )));
            }
        }
        if root_scope {
            break;
        }
    }
    Ok(())
}

/// Reconcile only affected FTS candidates against authoritative file text.
fn reconcile_affected_file_text_fts(
    connection: &Connection,
    active_slot: &str,
    affected_paths: &[String],
) -> DbResult<()> {
    verify_optional_file_text_fts_schema(connection)?;
    if !sqlite_objects(connection)?.contains_key(FILE_TEXT_FTS_TABLE) {
        return Ok(());
    }
    if affected_paths.iter().any(|path| path == ".") {
        return reconcile_full_file_text_fts(connection);
    }
    let mut statement = connection.prepare(
        "SELECT
             (SELECT COUNT(*) FROM file_texts
              WHERE structural_slot = ?1
                AND (path = ?2 OR (path >= ?3 AND path < ?4))),
             (SELECT COUNT(*) FROM file_text_fts
              WHERE file_text_fts MATCH ?5
                AND structural_slot = ?1
                AND (path = ?2 OR (path >= ?3 AND path < ?4))),
             EXISTS(
                 SELECT structural_slot, last_changed_epoch, path, content
                 FROM file_texts
                 WHERE structural_slot = ?1
                   AND (path = ?2 OR (path >= ?3 AND path < ?4))
                 EXCEPT
                 SELECT structural_slot, last_changed_epoch, path, content
                 FROM file_text_fts
                 WHERE file_text_fts MATCH ?5
                   AND structural_slot = ?1
                   AND (path = ?2 OR (path >= ?3 AND path < ?4))
             ) OR EXISTS(
                 SELECT structural_slot, last_changed_epoch, path, content
                 FROM file_text_fts
                 WHERE file_text_fts MATCH ?5
                   AND structural_slot = ?1
                   AND (path = ?2 OR (path >= ?3 AND path < ?4))
                 EXCEPT
                 SELECT structural_slot, last_changed_epoch, path, content
                 FROM file_texts
                 WHERE structural_slot = ?1
                   AND (path = ?2 OR (path >= ?3 AND path < ?4))
             )",
    )?;
    for affected_path in affected_paths {
        let (descendant_start, descendant_end) = crate::sqlite_descendant_bounds(affected_path);
        let lookup = format!("{FILE_TEXT_FTS_PATH_LOOKUP_PREFIX}{affected_path}");
        let match_query = format!("path_lookup : \"{}\"", lookup.replace('"', "\"\""));
        let (source_count, candidate_count, mismatch) = statement.query_row(
            params![
                active_slot,
                affected_path,
                descendant_start,
                descendant_end,
                match_query
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        if source_count != candidate_count || mismatch != 0 {
            return Err(preflight_error(format!(
                "affected slot-aware file-text FTS content did not reconcile for {affected_path:?}: source={source_count}, candidate={candidate_count}"
            )));
        }
    }
    Ok(())
}

/// Reject ambiguous candidate targets that violate the typed set contract.
fn reconcile_resolution_candidate_uniqueness(connection: &Connection) -> DbResult<()> {
    let mut statement = connection.prepare(
        "SELECT structural_slot, hex(resolution_occurrence_digest), target_scope,
                coalesce(hex(target_entity_digest), ''),
                coalesce(external_target_namespace, ''),
                coalesce(external_target_value, ''), COUNT(*)
         FROM graph_resolution_candidates
         GROUP BY structural_slot, resolution_occurrence_digest, target_scope,
                  target_entity_digest, external_target_namespace, external_target_value
         HAVING COUNT(*) > 1",
    )?;
    let duplicates = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if duplicates.is_empty() {
        Ok(())
    } else {
        Err(preflight_error(format!(
            "resolution candidate target uniqueness failed: {duplicates:?}"
        )))
    }
}

/// Require every persisted relation spelling to belong to the typed inventory.
fn reconcile_relation_families(connection: &Connection) -> DbResult<()> {
    for table in [GRAPH_RELATIONS_TABLE, GRAPH_RESOLUTION_OCCURRENCES_TABLE] {
        let mut statement =
            connection.prepare(&format!("SELECT DISTINCT relation_kind FROM {table}"))?;
        let values = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for value in values {
            if parse_graph_relation_kind(&value).is_err() {
                return Err(preflight_error(format!(
                    "relation family {value:?} in {table:?} is outside the typed inventory"
                )));
            }
        }
    }
    Ok(())
}

/// Reconcile ambiguous candidate totals with the retained bounded rows.
fn reconcile_resolution_candidate_counts(connection: &Connection) -> DbResult<()> {
    let retained_limit = i64::try_from(
        DefaultCoreBudgetKind::ResolutionCandidates
            .default_budget()
            .value(),
    )
    .map_err(|error| {
        preflight_error(format!(
            "resolution-candidate hard budget exceeds SQLite INTEGER: {error}"
        ))
    })?;
    let mut statement = connection.prepare(
        "SELECT occurrence.structural_slot,
                hex(occurrence.stable_key_digest),
                occurrence.resolution_status,
                occurrence.candidate_total,
                occurrence.candidate_completeness,
                COUNT(candidate.candidate_ordinal),
                MIN(candidate.candidate_ordinal),
                MAX(candidate.candidate_ordinal)
         FROM graph_resolution_occurrences AS occurrence
         LEFT JOIN graph_resolution_candidates AS candidate
           ON candidate.structural_slot = occurrence.structural_slot
          AND candidate.resolution_occurrence_digest = occurrence.stable_key_digest
         GROUP BY occurrence.structural_slot, occurrence.stable_key_digest
         HAVING
            (occurrence.resolution_status = 'ambiguous' AND (
                occurrence.candidate_total IS NULL
                OR occurrence.candidate_total < 2
                OR occurrence.candidate_total > ?1
                OR COUNT(candidate.candidate_ordinal) = 0
                OR COUNT(candidate.candidate_ordinal) > ?2
                OR COUNT(candidate.candidate_ordinal) > occurrence.candidate_total
                OR MIN(candidate.candidate_ordinal) != 0
                OR MAX(candidate.candidate_ordinal) != COUNT(candidate.candidate_ordinal) - 1
                OR occurrence.candidate_completeness NOT IN ('complete', 'partial', 'truncated')
                OR (occurrence.candidate_completeness = 'complete'
                    AND COUNT(candidate.candidate_ordinal) != occurrence.candidate_total)
                OR (occurrence.candidate_completeness = 'truncated'
                    AND COUNT(candidate.candidate_ordinal) >= occurrence.candidate_total)
            ))
            OR (occurrence.resolution_status = 'unresolved' AND (
                occurrence.candidate_total IS NOT NULL
                OR occurrence.candidate_completeness IS NOT NULL
                OR COUNT(candidate.candidate_ordinal) != 0
            ))
            OR occurrence.resolution_status NOT IN ('ambiguous', 'unresolved')",
    )?;
    let mismatches = statement
        .query_map(params![i64::from(u32::MAX), retained_limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<i64>>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(preflight_error(format!(
            "resolution candidate counts did not reconcile: {mismatches:?}"
        )))
    }
}

/// One raw coverage row awaiting core-domain validation.
struct PersistedCoverageRow {
    /// Persisted coverage-scope discriminator.
    scope_kind: String,
    /// Repository-relative file key or the repository sentinel.
    repository_path: String,
    /// Optional structural-pass identity.
    pass_identity: String,
    /// Optional typed relation-family spelling.
    relation_kind: String,
    /// Persisted coverage-state spelling.
    coverage_state: String,
    /// Number of retained structural facts.
    produced_count: i64,
    /// Exact omitted count or unknown when absent.
    omitted_count: Option<i64>,
    /// Optional hard-budget contract identifier.
    reached_limit: Option<String>,
    /// Optional bounded non-complete explanation.
    reason: Option<String>,
    /// Physical structural slot containing the row.
    structural_slot: String,
    /// Structural epoch that last changed the row.
    last_changed_epoch: i64,
}

/// Reuse the typed coverage contract and reconcile relation-scope counts.
fn reconcile_coverage(connection: &Connection) -> DbResult<()> {
    let mut statement = connection.prepare(
        "SELECT scope_kind, repository_path, pass_identity, relation_kind,
                coverage_state, produced_count, omitted_count, reached_limit,
                reason, structural_slot, last_changed_epoch
         FROM graph_coverage
         ORDER BY structural_slot, scope_kind, repository_path, pass_identity, relation_kind",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        validate_persisted_coverage(PersistedCoverageRow {
            scope_kind: row.get(0)?,
            repository_path: row.get(1)?,
            pass_identity: row.get(2)?,
            relation_kind: row.get(3)?,
            coverage_state: row.get(4)?,
            produced_count: row.get(5)?,
            omitted_count: row.get(6)?,
            reached_limit: row.get(7)?,
            reason: row.get(8)?,
            structural_slot: row.get(9)?,
            last_changed_epoch: row.get(10)?,
        })?;
    }
    drop(rows);
    drop(statement);

    let mut statement = connection.prepare(
        "SELECT coverage.structural_slot, coverage.repository_path,
                coverage.relation_kind, coverage.produced_count,
                COUNT(relation.stable_key_digest)
         FROM graph_coverage AS coverage
         LEFT JOIN graph_entities AS source
           ON source.structural_slot = coverage.structural_slot
          AND source.repository_path = coverage.repository_path
         LEFT JOIN graph_relations AS relation
           ON relation.structural_slot = source.structural_slot
          AND relation.source_entity_digest = source.stable_key_digest
          AND relation.relation_kind = coverage.relation_kind
         WHERE coverage.scope_kind = 'relation'
         GROUP BY coverage.structural_slot, coverage.repository_path,
                  coverage.relation_kind, coverage.produced_count
         HAVING coverage.produced_count != COUNT(relation.stable_key_digest)",
    )?;
    let mismatches = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(preflight_error(format!(
            "relation coverage counts did not reconcile: {mismatches:?}"
        )))
    }
}

/// Convert a persisted coverage row back through the core invariant owner.
fn validate_persisted_coverage(row: PersistedCoverageRow) -> DbResult<()> {
    let scope = match row.scope_kind.as_str() {
        "repository"
            if row.repository_path.is_empty()
                && row.pass_identity.is_empty()
                && row.relation_kind.is_empty() =>
        {
            CoverageScope::Repository
        }
        "file" if row.pass_identity.is_empty() && row.relation_kind.is_empty() => {
            CoverageScope::File {
                path: parse_repository_file_path(row.repository_path)?,
            }
        }
        "pass" if row.relation_kind.is_empty() => CoverageScope::Pass {
            path: parse_repository_file_path(row.repository_path)?,
            pass: parse_identity_text(row.pass_identity, "coverage pass identity")?,
        },
        "relation" if row.pass_identity.is_empty() => CoverageScope::Relation {
            path: parse_repository_file_path(row.repository_path)?,
            relation: parse_graph_relation_kind(&row.relation_kind)?,
        },
        _ => {
            return Err(preflight_error(format!(
                "coverage scope fields did not reconcile for {:?}",
                row.scope_kind
            )));
        }
    };
    let state = CoverageState::ALL
        .into_iter()
        .find(|state| state.as_str() == row.coverage_state)
        .ok_or_else(|| {
            preflight_error(format!(
                "coverage state {:?} is outside the typed inventory",
                row.coverage_state
            ))
        })?;
    let produced = nonnegative_u64(row.produced_count, "coverage produced count")?;
    let omitted = match row.omitted_count {
        Some(value) => OmittedFactCount::Known(nonnegative_u64(value, "coverage omitted count")?),
        None => OmittedFactCount::Unknown,
    };
    let reached_limit = row
        .reached_limit
        .map(|value| {
            DefaultCoreBudgetKind::ALL
                .into_iter()
                .find(|kind| kind.contract_id() == value)
                .ok_or_else(|| {
                    preflight_error(format!(
                        "coverage reached-limit {value:?} is outside the typed inventory"
                    ))
                })
        })
        .transpose()?;
    let reason = row
        .reason
        .map(|value| parse_identity_text(value, "coverage reason"))
        .transpose()?;
    let slot = parse_structural_slot(&row.structural_slot)?;
    let last_changed_epoch = IndexEpoch::new(nonnegative_u64(
        row.last_changed_epoch,
        "coverage last-changed epoch",
    )?);
    CoverageRecord::new(
        scope,
        state,
        produced,
        omitted,
        reached_limit,
        reason,
        slot,
        last_changed_epoch,
    )
    .map(|_| ())
    .map_err(|error| preflight_error(format!("coverage row did not reconcile: {error}")))
}

/// Require every value in every declared structural TEXT column to be UTF-8.
fn reconcile_structural_text(connection: &Connection) -> DbResult<()> {
    for table in STRUCTURAL_DERIVED_TABLES {
        for column in table_text_columns(connection, table)? {
            let identifier = quote_identifier(&column);
            let mut statement = connection.prepare(&format!("SELECT {identifier} FROM {table}"))?;
            let mut rows = statement.query([])?;
            let mut row_number = 0_u64;
            while let Some(row) = rows.next()? {
                row_number += 1;
                match row.get_ref(0)? {
                    ValueRef::Null => {}
                    ValueRef::Text(bytes) if std::str::from_utf8(bytes).is_ok() => {}
                    ValueRef::Text(_) => {
                        return Err(preflight_error(format!(
                            "invalid UTF-8 in structural TEXT column {table}.{column} at row {row_number}"
                        )));
                    }
                    value => {
                        return Err(preflight_error(format!(
                            "non-TEXT value {} in structural TEXT column {table}.{column} at row {row_number}",
                            sqlite_value_kind(value)
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Parse one relation family through the closed core inventory.
fn parse_graph_relation_kind(value: &str) -> DbResult<GraphRelationKind> {
    GraphRelationKind::ALL
        .into_iter()
        .find(|kind| kind.as_str() == value)
        .ok_or_else(|| {
            preflight_error(format!(
                "relation family {value:?} is outside the typed inventory"
            ))
        })
}

/// Parse one persisted structural-slot spelling.
fn parse_structural_slot(value: &str) -> DbResult<StructuralSlot> {
    match value {
        "a" => Ok(StructuralSlot::A),
        "b" => Ok(StructuralSlot::B),
        _ => Err(preflight_error(format!(
            "structural slot {value:?} is invalid"
        ))),
    }
}

/// Return the persistent spelling for a typed structural slot.
const fn structural_slot_text(slot: StructuralSlot) -> &'static str {
    match slot {
        StructuralSlot::A => "a",
        StructuralSlot::B => "b",
    }
}

/// Convert a typed epoch into `SQLite`'s signed integer domain.
fn sqlite_epoch(epoch: IndexEpoch) -> DbResult<i64> {
    i64::try_from(epoch.get()).map_err(|error| {
        preflight_error(format!(
            "structural publication epoch exceeds SQLite INTEGER: {error}"
        ))
    })
}

/// Parse and retain a canonical repository file path.
fn parse_repository_file_path(value: String) -> DbResult<RepositoryFilePath> {
    RepositoryFilePath::try_from(value)
        .map_err(|error| preflight_error(format!("coverage repository path is invalid: {error}")))
}

/// Parse one bounded identity scalar with storage context.
fn parse_identity_text(value: String, field: &str) -> DbResult<IdentityText> {
    IdentityText::try_from(value)
        .map_err(|error| preflight_error(format!("{field} is invalid: {error}")))
}

/// Convert a nonnegative `SQLite` count into the core unsigned domain.
fn nonnegative_u64(value: i64, field: &str) -> DbResult<u64> {
    u64::try_from(value)
        .map_err(|error| preflight_error(format!("{field} is invalid ({value}): {error}")))
}

/// Quote one schema-derived `SQLite` identifier.
fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// Return a diagnostic name for one dynamically typed `SQLite` value.
const fn sqlite_value_kind(value: ValueRef<'_>) -> &'static str {
    match value {
        ValueRef::Null => "NULL",
        ValueRef::Integer(_) => "INTEGER",
        ValueRef::Real(_) => "REAL",
        ValueRef::Text(_) => "TEXT",
        ValueRef::Blob(_) => "BLOB",
    }
}

/// Create or repair the pre-ledger schema objects inside the migration transaction.
fn initialize_schema_objects(connection: &Connection) -> DbResult<()> {
    reset_legacy_summary_schema(connection)?;
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS nodes (
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

        CREATE TABLE IF NOT EXISTS purposes (
            node_id INTEGER PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
            purpose TEXT,
            source TEXT NOT NULL,
            status TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_by TEXT
        );

        CREATE TABLE IF NOT EXISTS summaries (
            id INTEGER PRIMARY KEY,
            node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
            summary_level TEXT NOT NULL DEFAULT 'node',
            subject TEXT NOT NULL DEFAULT '',
            summary TEXT,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(node_id, summary_level, subject)
        );

        CREATE TABLE IF NOT EXISTS usage_events (
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

        CREATE TABLE IF NOT EXISTS symbols (
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

        CREATE TABLE IF NOT EXISTS source_parse_metadata (
            path TEXT PRIMARY KEY,
            language TEXT,
            parser TEXT NOT NULL,
            symbol_count INTEGER NOT NULL,
            relation_count INTEGER NOT NULL,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS symbol_relations (
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

        CREATE TABLE IF NOT EXISTS health_resolutions (
            finding_id TEXT PRIMARY KEY,
            category TEXT NOT NULL,
            path TEXT NOT NULL,
            related_path TEXT,
            rationale TEXT NOT NULL,
            resolved_by TEXT NOT NULL DEFAULT 'agent',
            resolved_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS file_texts (
            path TEXT PRIMARY KEY,
            content_hash TEXT,
            byte_count INTEGER NOT NULL,
            line_count INTEGER NOT NULL,
            content TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);
        CREATE INDEX IF NOT EXISTS idx_nodes_parent ON nodes(parent_path);
        CREATE INDEX IF NOT EXISTS idx_purposes_status ON purposes(status);
        CREATE INDEX IF NOT EXISTS idx_summaries_level ON summaries(summary_level);
        CREATE INDEX IF NOT EXISTS idx_summaries_summary ON summaries(summary);
        CREATE INDEX IF NOT EXISTS idx_usage_session ON usage_events(session_id);
        CREATE INDEX IF NOT EXISTS idx_symbols_path ON symbols(path);
        CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
        CREATE INDEX IF NOT EXISTS idx_symbols_kind ON symbols(kind);
        CREATE INDEX IF NOT EXISTS idx_source_parse_metadata_parser ON source_parse_metadata(parser);
        CREATE INDEX IF NOT EXISTS idx_symbol_relations_path ON symbol_relations(path);
        CREATE INDEX IF NOT EXISTS idx_symbol_relations_target ON symbol_relations(target_name);
        CREATE INDEX IF NOT EXISTS idx_health_resolutions_category ON health_resolutions(category);
        CREATE INDEX IF NOT EXISTS idx_file_texts_hash ON file_texts(content_hash);
        ",
    )?;
    ensure_symbol_metadata_columns(connection)?;
    ensure_usage_event_metadata_columns(connection)?;
    connection.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_usage_created_at ON usage_events(created_at);
        CREATE INDEX IF NOT EXISTS idx_usage_session_created_at ON usage_events(session_id, created_at);
        ",
    )?;
    Ok(())
}

/// Reject a plan that no longer matches the compiled migration inventory.
fn validate_migration_plan(plan: &SchemaMigrationPlan) -> DbResult<()> {
    if !(0..=SCHEMA_VERSION).contains(&plan.source_version) || plan.target_version != SCHEMA_VERSION
    {
        return Err(preflight_error(format!(
            "migration plan range {}..={} is incompatible with runtime target {SCHEMA_VERSION}",
            plan.source_version, plan.target_version
        )));
    }
    let expected = allocated_migrations()
        .into_iter()
        .filter(|migration| migration.schema_version > plan.source_version)
        .collect::<Vec<_>>();
    if plan.pending.len() != expected.len()
        || plan
            .pending
            .iter()
            .zip(&expected)
            .any(|(actual, expected)| {
                actual.schema_version != expected.schema_version
                    || actual.definition.id != expected.definition.id
                    || actual.checksum != expected.checksum
            })
    {
        return Err(preflight_error(
            "migration plan does not match the compiled ordered inventory",
        ));
    }
    Ok(())
}

/// Read the schema identifier again at the transaction boundary.
fn observed_schema_version(connection: &Connection) -> DbResult<i64> {
    let objects = sqlite_objects(connection)?;
    match schema_source_state(connection, &objects)? {
        SchemaSourceState::Fresh | SchemaSourceState::Empty => Ok(0),
        SchemaSourceState::Supported { version, .. }
        | SchemaSourceState::Unsupported { version } => Ok(version),
    }
}

/// Create the runtime-owned append-only migration history table.
fn create_migration_ledger(connection: &Connection) -> DbResult<()> {
    connection.execute_batch(
        "CREATE TABLE schema_migrations (
            schema_version INTEGER PRIMARY KEY,
            migration_id TEXT NOT NULL UNIQUE,
            checksum TEXT NOT NULL
        );",
    )?;
    Ok(())
}

/// Verify the table shape before recording the migration that created it.
fn verify_migration_ledger_schema(connection: &Connection) -> DbResult<()> {
    if table_columns(connection, MIGRATION_LEDGER_TABLE)?
        != ["schema_version", "migration_id", "checksum"]
    {
        return Err(preflight_error(
            "runtime migration ledger column contract did not reconcile",
        ));
    }
    Ok(())
}

/// Create and synchronize the optional trigram candidate index when supported.
fn create_optional_file_text_fts(connection: &Connection) -> DbResult<()> {
    if !sqlite_feature_state(connection)?.fts5 {
        return Ok(());
    }

    connection.execute_batch("SAVEPOINT projectatlas_file_text_fts_install;")?;
    match connection.execute_batch(
        "CREATE VIRTUAL TABLE file_text_fts
         USING fts5(path UNINDEXED, content, tokenize='trigram');",
    ) {
        Ok(()) => {
            connection.execute_batch("RELEASE SAVEPOINT projectatlas_file_text_fts_install;")?;
        }
        Err(error) if is_optional_fts_unavailable(&error) => {
            connection.execute_batch(
                "ROLLBACK TO SAVEPOINT projectatlas_file_text_fts_install;
                 RELEASE SAVEPOINT projectatlas_file_text_fts_install;",
            )?;
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    }

    connection.execute_batch(
        "
         INSERT INTO file_text_fts(path, content)
         SELECT path, content FROM file_texts ORDER BY path;

         CREATE TRIGGER file_text_fts_insert
         AFTER INSERT ON file_texts
         BEGIN
             INSERT INTO file_text_fts(path, content) VALUES(new.path, new.content);
         END;

         CREATE TRIGGER file_text_fts_delete
         AFTER DELETE ON file_texts
         BEGIN
             DELETE FROM file_text_fts WHERE path = old.path;
         END;

         CREATE TRIGGER file_text_fts_update
         AFTER UPDATE OF path, content ON file_texts
         BEGIN
             DELETE FROM file_text_fts WHERE path = old.path;
             INSERT INTO file_text_fts(path, content) VALUES(new.path, new.content);
         END;",
    )?;
    Ok(())
}

/// Verify the pre-slot optional index or an unavailable exact-only runtime.
fn verify_unbound_optional_file_text_fts(connection: &Connection) -> DbResult<()> {
    let objects = sqlite_objects(connection)?;
    if !objects.contains_key(FILE_TEXT_FTS_TABLE) {
        return Ok(());
    }

    for trigger in [
        FILE_TEXT_FTS_INSERT_TRIGGER,
        FILE_TEXT_FTS_DELETE_TRIGGER,
        FILE_TEXT_FTS_UPDATE_TRIGGER,
    ] {
        if !matches!(
            objects.get(trigger),
            Some(object) if object.object_type == "trigger" && object.table_name == "file_texts"
        ) {
            return Err(preflight_error(format!(
                "optional file-text FTS trigger {trigger:?} did not reconcile"
            )));
        }
    }

    let source_count = connection.query_row("SELECT COUNT(*) FROM file_texts", [], |row| {
        row.get::<_, i64>(0)
    })?;
    let candidate_count =
        connection.query_row("SELECT COUNT(*) FROM file_text_fts", [], |row| {
            row.get::<_, i64>(0)
        })?;
    let content_mismatch = connection.query_row(
        "SELECT
             EXISTS(
                 SELECT path, content FROM file_texts
                 EXCEPT
                 SELECT path, content FROM file_text_fts
             )
             OR EXISTS(
                 SELECT path, content FROM file_text_fts
                 EXCEPT
                 SELECT path, content FROM file_texts
             )",
        [],
        |row| row.get::<_, i64>(0),
    )? != 0;
    if source_count != candidate_count || content_mismatch {
        return Err(preflight_error(format!(
            "optional file-text FTS content did not reconcile: source={source_count}, candidate={candidate_count}"
        )));
    }
    Ok(())
}

/// Create normalized typed graph storage without rewriting legacy compatibility rows.
fn create_typed_repository_graph_schema(connection: &Connection) -> DbResult<()> {
    connection.execute_batch(&format!(
        "
        CREATE TABLE graph_entities (
            stable_key_digest BLOB NOT NULL CHECK(length(stable_key_digest) = 32),
            stable_key_version INTEGER NOT NULL CHECK(stable_key_version > 0),
            stable_key_canonical BLOB NOT NULL,
            project_instance_id BLOB NOT NULL CHECK(length(project_instance_id) = 16),
            entity_kind TEXT NOT NULL CHECK(entity_kind IN (
                'repository', 'folder', 'file', 'package', 'module', 'declaration',
                'reference', 'endpoint', 'route', 'channel', 'configuration',
                'environment', 'infrastructure', 'test', 'external'
            )),
            repository_path TEXT,
            qualified_name TEXT,
            signature TEXT,
            discriminator TEXT,
            external_namespace TEXT,
            external_value TEXT,
            language TEXT,
            source_start_byte INTEGER,
            source_end_byte INTEGER,
            source_start_line INTEGER,
            source_end_line INTEGER,
            parser_kind TEXT NOT NULL CHECK(parser_kind IN ({GRAPH_PARSER_KIND_VALUES})),
            parser_identity TEXT NOT NULL,
            parser_version TEXT NOT NULL,
            PRIMARY KEY(stable_key_digest),
            UNIQUE(stable_key_canonical),
            CHECK(
                (source_start_byte IS NULL
                    AND source_end_byte IS NULL
                    AND source_start_line IS NULL
                    AND source_end_line IS NULL)
                OR
                (source_start_byte >= 0
                    AND source_end_byte >= source_start_byte
                    AND source_start_line > 0
                    AND source_end_line >= source_start_line)
            )
        );

        CREATE TABLE graph_relations (
            stable_key_digest BLOB NOT NULL CHECK(length(stable_key_digest) = 32),
            stable_key_version INTEGER NOT NULL CHECK(stable_key_version > 0),
            stable_key_canonical BLOB NOT NULL,
            source_entity_digest BLOB NOT NULL CHECK(length(source_entity_digest) = 32),
            relation_kind TEXT NOT NULL CHECK(relation_kind IN ({GRAPH_RELATION_KIND_VALUES})),
            resolution_status TEXT NOT NULL DEFAULT 'resolved'
                CHECK(resolution_status = 'resolved'),
            target_scope TEXT NOT NULL CHECK(target_scope IN ({GRAPH_TARGET_SCOPE_VALUES})),
            target_entity_digest BLOB,
            external_target_namespace TEXT,
            external_target_value TEXT,
            confidence TEXT NOT NULL CHECK(confidence IN ({GRAPH_CONFIDENCE_VALUES})),
            parser_kind TEXT NOT NULL CHECK(parser_kind IN ({GRAPH_PARSER_KIND_VALUES})),
            parser_identity TEXT NOT NULL,
            parser_version TEXT NOT NULL,
            PRIMARY KEY(stable_key_digest),
            UNIQUE(stable_key_canonical),
            FOREIGN KEY(source_entity_digest)
                REFERENCES graph_entities(stable_key_digest) ON DELETE CASCADE,
            FOREIGN KEY(target_entity_digest)
                REFERENCES graph_entities(stable_key_digest) ON DELETE CASCADE,
            CHECK(
                (target_scope = 'internal'
                    AND length(target_entity_digest) = 32
                    AND external_target_namespace IS NULL
                    AND external_target_value IS NULL)
                OR
                (target_scope = 'external'
                    AND target_entity_digest IS NULL
                    AND external_target_namespace IS NOT NULL
                    AND external_target_value IS NOT NULL)
            )
        );

        CREATE TABLE graph_evidence_occurrences (
            stable_key_digest BLOB NOT NULL CHECK(length(stable_key_digest) = 32),
            stable_key_version INTEGER NOT NULL CHECK(stable_key_version > 0),
            stable_key_canonical BLOB NOT NULL,
            relation_digest BLOB NOT NULL CHECK(length(relation_digest) = 32),
            origin_kind TEXT NOT NULL CHECK(origin_kind IN ({GRAPH_EVIDENCE_ORIGIN_VALUES})),
            origin_entity_digest BLOB,
            origin_project_instance_id BLOB,
            origin_repository_path TEXT,
            origin_external_namespace TEXT,
            origin_external_value TEXT,
            source_start_byte INTEGER,
            source_end_byte INTEGER,
            source_start_line INTEGER,
            source_end_line INTEGER,
            resolver_name TEXT NOT NULL,
            resolver_version TEXT NOT NULL,
            content_span_fingerprint BLOB NOT NULL CHECK(length(content_span_fingerprint) = 32),
            occurrence_discriminator INTEGER NOT NULL CHECK(occurrence_discriminator >= 0),
            evidence_class TEXT NOT NULL CHECK(evidence_class IN ({GRAPH_EVIDENCE_CLASS_VALUES})),
            confidence TEXT NOT NULL CHECK(confidence IN ({GRAPH_CONFIDENCE_VALUES})),
            completeness TEXT NOT NULL CHECK(completeness IN ({GRAPH_COMPLETENESS_VALUES})),
            explanation TEXT,
            PRIMARY KEY(stable_key_digest),
            UNIQUE(stable_key_canonical),
            FOREIGN KEY(relation_digest)
                REFERENCES graph_relations(stable_key_digest) ON DELETE CASCADE,
            FOREIGN KEY(origin_entity_digest)
                REFERENCES graph_entities(stable_key_digest) ON DELETE CASCADE,
            CHECK(
                (origin_kind = 'entity'
                    AND length(origin_entity_digest) = 32
                    AND origin_project_instance_id IS NULL
                    AND origin_repository_path IS NULL
                    AND origin_external_namespace IS NULL
                    AND origin_external_value IS NULL)
                OR
                (origin_kind = 'repository-path'
                    AND origin_entity_digest IS NULL
                    AND length(origin_project_instance_id) = 16
                    AND origin_repository_path IS NOT NULL
                    AND origin_external_namespace IS NULL
                    AND origin_external_value IS NULL)
                OR
                (origin_kind = 'external'
                    AND origin_entity_digest IS NULL
                    AND origin_project_instance_id IS NULL
                    AND origin_repository_path IS NULL
                    AND origin_external_namespace IS NOT NULL
                    AND origin_external_value IS NOT NULL)
            ),
            CHECK(
                (source_start_byte IS NULL
                    AND source_end_byte IS NULL
                    AND source_start_line IS NULL
                    AND source_end_line IS NULL)
                OR
                (source_start_byte >= 0
                    AND source_end_byte >= source_start_byte
                    AND source_start_line > 0
                    AND source_end_line >= source_start_line)
            )
        );

        CREATE TABLE graph_resolution_occurrences (
            stable_key_digest BLOB NOT NULL CHECK(length(stable_key_digest) = 32),
            stable_key_version INTEGER NOT NULL CHECK(stable_key_version > 0),
            stable_key_canonical BLOB NOT NULL,
            source_entity_digest BLOB NOT NULL CHECK(length(source_entity_digest) = 32),
            relation_kind TEXT NOT NULL CHECK(relation_kind IN ({GRAPH_RELATION_KIND_VALUES})),
            origin_kind TEXT NOT NULL CHECK(origin_kind IN ({GRAPH_EVIDENCE_ORIGIN_VALUES})),
            origin_entity_digest BLOB,
            origin_project_instance_id BLOB,
            origin_repository_path TEXT,
            origin_external_namespace TEXT,
            origin_external_value TEXT,
            source_start_byte INTEGER,
            source_end_byte INTEGER,
            source_start_line INTEGER,
            source_end_line INTEGER,
            resolver_name TEXT NOT NULL,
            resolver_version TEXT NOT NULL,
            content_span_fingerprint BLOB NOT NULL CHECK(length(content_span_fingerprint) = 32),
            occurrence_discriminator INTEGER NOT NULL CHECK(occurrence_discriminator >= 0),
            resolution_status TEXT NOT NULL
                CHECK(resolution_status IN ('ambiguous', 'unresolved')),
            candidate_total INTEGER,
            candidate_completeness TEXT
                CHECK(candidate_completeness IN ({GRAPH_COMPLETENESS_VALUES})),
            unresolved_reason TEXT,
            evidence_class TEXT NOT NULL CHECK(evidence_class IN ({GRAPH_EVIDENCE_CLASS_VALUES})),
            confidence TEXT NOT NULL CHECK(confidence IN ({GRAPH_CONFIDENCE_VALUES})),
            completeness TEXT NOT NULL CHECK(completeness IN ({GRAPH_COMPLETENESS_VALUES})),
            parser_kind TEXT NOT NULL CHECK(parser_kind IN ({GRAPH_PARSER_KIND_VALUES})),
            parser_identity TEXT NOT NULL,
            parser_version TEXT NOT NULL,
            PRIMARY KEY(stable_key_digest),
            UNIQUE(stable_key_canonical),
            FOREIGN KEY(source_entity_digest)
                REFERENCES graph_entities(stable_key_digest) ON DELETE CASCADE,
            FOREIGN KEY(origin_entity_digest)
                REFERENCES graph_entities(stable_key_digest) ON DELETE CASCADE,
            CHECK(
                (origin_kind = 'entity'
                    AND length(origin_entity_digest) = 32
                    AND origin_project_instance_id IS NULL
                    AND origin_repository_path IS NULL
                    AND origin_external_namespace IS NULL
                    AND origin_external_value IS NULL)
                OR
                (origin_kind = 'repository-path'
                    AND origin_entity_digest IS NULL
                    AND length(origin_project_instance_id) = 16
                    AND origin_repository_path IS NOT NULL
                    AND origin_external_namespace IS NULL
                    AND origin_external_value IS NULL)
                OR
                (origin_kind = 'external'
                    AND origin_entity_digest IS NULL
                    AND origin_project_instance_id IS NULL
                    AND origin_repository_path IS NULL
                    AND origin_external_namespace IS NOT NULL
                    AND origin_external_value IS NOT NULL)
            ),
            CHECK(
                (source_start_byte IS NULL
                    AND source_end_byte IS NULL
                    AND source_start_line IS NULL
                    AND source_end_line IS NULL)
                OR
                (source_start_byte >= 0
                    AND source_end_byte >= source_start_byte
                    AND source_start_line > 0
                    AND source_end_line >= source_start_line)
            ),
            CHECK(
                (resolution_status = 'ambiguous'
                    AND candidate_total >= 2
                    AND candidate_completeness IS NOT NULL
                    AND unresolved_reason IS NULL)
                OR
                (resolution_status = 'unresolved'
                    AND candidate_total IS NULL
                    AND candidate_completeness IS NULL)
            )
        );

        CREATE TABLE graph_resolution_candidates (
            resolution_occurrence_digest BLOB NOT NULL
                CHECK(length(resolution_occurrence_digest) = 32),
            candidate_ordinal INTEGER NOT NULL CHECK(candidate_ordinal >= 0),
            target_scope TEXT NOT NULL CHECK(target_scope IN ({GRAPH_TARGET_SCOPE_VALUES})),
            target_entity_digest BLOB,
            external_target_namespace TEXT,
            external_target_value TEXT,
            confidence TEXT NOT NULL CHECK(confidence IN ({GRAPH_CONFIDENCE_VALUES})),
            explanation TEXT,
            PRIMARY KEY(
                resolution_occurrence_digest,
                candidate_ordinal
            ),
            FOREIGN KEY(resolution_occurrence_digest)
                REFERENCES graph_resolution_occurrences(stable_key_digest) ON DELETE CASCADE,
            FOREIGN KEY(target_entity_digest)
                REFERENCES graph_entities(stable_key_digest) ON DELETE CASCADE,
            CHECK(
                (target_scope = 'internal'
                    AND length(target_entity_digest) = 32
                    AND external_target_namespace IS NULL
                    AND external_target_value IS NULL)
                OR
                (target_scope = 'external'
                    AND target_entity_digest IS NULL
                    AND external_target_namespace IS NOT NULL
                    AND external_target_value IS NOT NULL)
            )
        );

        CREATE TABLE graph_coverage (
            scope_kind TEXT NOT NULL
                CHECK(scope_kind IN ('repository', 'file', 'pass', 'relation')),
            repository_path TEXT NOT NULL DEFAULT '',
            pass_identity TEXT NOT NULL DEFAULT '',
            relation_kind TEXT NOT NULL DEFAULT '' CHECK(
                relation_kind = ''
                OR relation_kind IN ({GRAPH_RELATION_KIND_VALUES})
            ),
            coverage_state TEXT NOT NULL CHECK(coverage_state IN (
                'complete', 'partial', 'failed', 'ignored', 'oversized',
                'quarantined', 'stale'
            )),
            produced_count INTEGER NOT NULL CHECK(produced_count >= 0),
            omitted_count INTEGER CHECK(omitted_count >= 0),
            reached_limit TEXT CHECK(
                reached_limit IS NULL
                OR reached_limit IN (
                    'source_file_bytes', 'ast_depth', 'symbols_per_file',
                    'relations_per_file', 'resolution_candidates', 'worker_count',
                    'stage_time', 'working_memory', 'query_depth', 'visited_nodes',
                    'expanded_edges', 'returned_rows', 'response_bytes',
                    'cancellation_poll', 'cancellation_grace'
                )
            ),
            reason TEXT,
            structural_slot TEXT NOT NULL CHECK(structural_slot IN ({GRAPH_STRUCTURAL_SLOT_VALUES})),
            last_changed_epoch INTEGER NOT NULL CHECK(last_changed_epoch >= 0),
            PRIMARY KEY(
                scope_kind,
                repository_path,
                pass_identity,
                relation_kind,
                structural_slot
            ),
            CHECK(
                (scope_kind = 'repository'
                    AND repository_path = ''
                    AND pass_identity = ''
                    AND relation_kind = '')
                OR
                (scope_kind = 'file'
                    AND repository_path != ''
                    AND pass_identity = ''
                    AND relation_kind = '')
                OR
                (scope_kind = 'pass'
                    AND repository_path != ''
                    AND pass_identity != ''
                    AND relation_kind = '')
                OR
                (scope_kind = 'relation'
                    AND repository_path != ''
                    AND pass_identity = ''
                    AND relation_kind != '')
            )
        );
        ",
    ))?;
    Ok(())
}

/// Reconcile the typed graph shape created before structural slots exist.
fn verify_unbound_typed_repository_graph_schema(connection: &Connection) -> DbResult<()> {
    verify_typed_repository_graph_columns(connection, false)
}

/// Reconcile typed graph columns for either the immutable pre-slot or current shape.
fn verify_typed_repository_graph_columns(
    connection: &Connection,
    structural_binding: bool,
) -> DbResult<()> {
    for (table, expected) in [
        (
            GRAPH_ENTITIES_TABLE,
            &[
                "stable_key_digest",
                "stable_key_version",
                "stable_key_canonical",
                "project_instance_id",
                "entity_kind",
                "repository_path",
                "qualified_name",
                "signature",
                "discriminator",
                "external_namespace",
                "external_value",
                "language",
                "source_start_byte",
                "source_end_byte",
                "source_start_line",
                "source_end_line",
                "parser_kind",
                "parser_identity",
                "parser_version",
            ][..],
        ),
        (
            GRAPH_RELATIONS_TABLE,
            &[
                "stable_key_digest",
                "stable_key_version",
                "stable_key_canonical",
                "source_entity_digest",
                "relation_kind",
                "resolution_status",
                "target_scope",
                "target_entity_digest",
                "external_target_namespace",
                "external_target_value",
                "confidence",
                "parser_kind",
                "parser_identity",
                "parser_version",
            ][..],
        ),
        (
            GRAPH_EVIDENCE_OCCURRENCES_TABLE,
            &[
                "stable_key_digest",
                "stable_key_version",
                "stable_key_canonical",
                "relation_digest",
                "origin_kind",
                "origin_entity_digest",
                "origin_project_instance_id",
                "origin_repository_path",
                "origin_external_namespace",
                "origin_external_value",
                "source_start_byte",
                "source_end_byte",
                "source_start_line",
                "source_end_line",
                "resolver_name",
                "resolver_version",
                "content_span_fingerprint",
                "occurrence_discriminator",
                "evidence_class",
                "confidence",
                "completeness",
                "explanation",
            ][..],
        ),
        (
            GRAPH_RESOLUTION_OCCURRENCES_TABLE,
            &[
                "stable_key_digest",
                "stable_key_version",
                "stable_key_canonical",
                "source_entity_digest",
                "relation_kind",
                "origin_kind",
                "origin_entity_digest",
                "origin_project_instance_id",
                "origin_repository_path",
                "origin_external_namespace",
                "origin_external_value",
                "source_start_byte",
                "source_end_byte",
                "source_start_line",
                "source_end_line",
                "resolver_name",
                "resolver_version",
                "content_span_fingerprint",
                "occurrence_discriminator",
                "resolution_status",
                "candidate_total",
                "candidate_completeness",
                "unresolved_reason",
                "evidence_class",
                "confidence",
                "completeness",
                "parser_kind",
                "parser_identity",
                "parser_version",
            ][..],
        ),
        (
            GRAPH_RESOLUTION_CANDIDATES_TABLE,
            &[
                "resolution_occurrence_digest",
                "candidate_ordinal",
                "target_scope",
                "target_entity_digest",
                "external_target_namespace",
                "external_target_value",
                "confidence",
                "explanation",
            ][..],
        ),
        (
            GRAPH_COVERAGE_TABLE,
            &[
                "scope_kind",
                "repository_path",
                "pass_identity",
                "relation_kind",
                "coverage_state",
                "produced_count",
                "omitted_count",
                "reached_limit",
                "reason",
                "structural_slot",
                "last_changed_epoch",
            ][..],
        ),
    ] {
        let mut expected = expected.to_vec();
        if structural_binding && table != GRAPH_COVERAGE_TABLE {
            expected.extend(["structural_slot", "last_changed_epoch"]);
        }
        let actual = table_columns(connection, table)?;
        if actual != expected {
            return Err(preflight_error(format!(
                "typed repository graph table {table:?} did not reconcile: {actual:?}"
            )));
        }
    }
    Ok(())
}

/// Reconcile composite slot identities and same-slot graph references.
fn verify_typed_repository_graph_schema(connection: &Connection) -> DbResult<()> {
    verify_typed_repository_graph_columns(connection, true)?;

    for (table, expected) in [
        (
            GRAPH_ENTITIES_TABLE,
            &["structural_slot", "stable_key_digest"][..],
        ),
        (
            GRAPH_RELATIONS_TABLE,
            &["structural_slot", "stable_key_digest"][..],
        ),
        (
            GRAPH_EVIDENCE_OCCURRENCES_TABLE,
            &["structural_slot", "stable_key_digest"][..],
        ),
        (
            GRAPH_RESOLUTION_OCCURRENCES_TABLE,
            &["structural_slot", "stable_key_digest"][..],
        ),
        (
            GRAPH_RESOLUTION_CANDIDATES_TABLE,
            &[
                "structural_slot",
                "resolution_occurrence_digest",
                "candidate_ordinal",
            ][..],
        ),
        (
            GRAPH_COVERAGE_TABLE,
            &[
                "scope_kind",
                "repository_path",
                "pass_identity",
                "relation_kind",
                "structural_slot",
            ][..],
        ),
    ] {
        let actual = table_primary_key_columns(connection, table)?;
        if actual != expected {
            return Err(preflight_error(format!(
                "typed repository graph primary key for {table:?} did not reconcile: {actual:?}"
            )));
        }
        require_foreign_key(
            connection,
            table,
            GRAPH_STRUCTURAL_SLOTS_TABLE,
            &[("structural_slot", "slot")],
        )?;
    }

    for table in [
        GRAPH_ENTITIES_TABLE,
        GRAPH_RELATIONS_TABLE,
        GRAPH_EVIDENCE_OCCURRENCES_TABLE,
        GRAPH_RESOLUTION_OCCURRENCES_TABLE,
    ] {
        require_unique_constraint(
            connection,
            table,
            &["structural_slot", "stable_key_canonical"],
        )?;
    }

    for (index, table, columns) in [
        (
            GRAPH_ENTITIES_SLOT_PATH_INDEX,
            GRAPH_ENTITIES_TABLE,
            &["structural_slot", "repository_path"][..],
        ),
        (
            GRAPH_EVIDENCE_SLOT_PATH_INDEX,
            GRAPH_EVIDENCE_OCCURRENCES_TABLE,
            &["structural_slot", "origin_repository_path"][..],
        ),
        (
            GRAPH_RESOLUTION_SLOT_PATH_INDEX,
            GRAPH_RESOLUTION_OCCURRENCES_TABLE,
            &["structural_slot", "origin_repository_path"][..],
        ),
        (
            GRAPH_COVERAGE_SLOT_PATH_INDEX,
            GRAPH_COVERAGE_TABLE,
            &["structural_slot", "repository_path"][..],
        ),
    ] {
        require_index_contract(connection, index, table, columns)?;
    }

    for (table, target, columns) in [
        (
            GRAPH_RELATIONS_TABLE,
            GRAPH_ENTITIES_TABLE,
            &[
                ("structural_slot", "structural_slot"),
                ("source_entity_digest", "stable_key_digest"),
            ][..],
        ),
        (
            GRAPH_RELATIONS_TABLE,
            GRAPH_ENTITIES_TABLE,
            &[
                ("structural_slot", "structural_slot"),
                ("target_entity_digest", "stable_key_digest"),
            ][..],
        ),
        (
            GRAPH_EVIDENCE_OCCURRENCES_TABLE,
            GRAPH_RELATIONS_TABLE,
            &[
                ("structural_slot", "structural_slot"),
                ("relation_digest", "stable_key_digest"),
            ][..],
        ),
        (
            GRAPH_EVIDENCE_OCCURRENCES_TABLE,
            GRAPH_ENTITIES_TABLE,
            &[
                ("structural_slot", "structural_slot"),
                ("origin_entity_digest", "stable_key_digest"),
            ][..],
        ),
        (
            GRAPH_RESOLUTION_OCCURRENCES_TABLE,
            GRAPH_ENTITIES_TABLE,
            &[
                ("structural_slot", "structural_slot"),
                ("source_entity_digest", "stable_key_digest"),
            ][..],
        ),
        (
            GRAPH_RESOLUTION_OCCURRENCES_TABLE,
            GRAPH_ENTITIES_TABLE,
            &[
                ("structural_slot", "structural_slot"),
                ("origin_entity_digest", "stable_key_digest"),
            ][..],
        ),
        (
            GRAPH_RESOLUTION_CANDIDATES_TABLE,
            GRAPH_RESOLUTION_OCCURRENCES_TABLE,
            &[
                ("structural_slot", "structural_slot"),
                ("resolution_occurrence_digest", "stable_key_digest"),
            ][..],
        ),
        (
            GRAPH_RESOLUTION_CANDIDATES_TABLE,
            GRAPH_ENTITIES_TABLE,
            &[
                ("structural_slot", "structural_slot"),
                ("target_entity_digest", "stable_key_digest"),
            ][..],
        ),
    ] {
        require_foreign_key(connection, table, target, columns)?;
    }
    Ok(())
}

/// Create the immutable pair of structural slots and singleton publication state.
fn create_structural_publication_schema(connection: &Connection) -> DbResult<()> {
    connection.execute_batch(&format!(
        "
        CREATE TABLE graph_structural_slots (
            slot TEXT PRIMARY KEY CHECK(slot IN ({GRAPH_STRUCTURAL_SLOT_VALUES}))
        ) WITHOUT ROWID;

        INSERT INTO graph_structural_slots(slot) VALUES('a'), ('b');

        CREATE TABLE graph_publication_state (
            singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
            active_slot TEXT NOT NULL CHECK(active_slot IN ({GRAPH_STRUCTURAL_SLOT_VALUES})),
            active_epoch INTEGER NOT NULL CHECK(active_epoch >= 0),
            FOREIGN KEY(active_slot)
                REFERENCES graph_structural_slots(slot) ON UPDATE RESTRICT ON DELETE RESTRICT
        );

        INSERT INTO graph_publication_state(singleton, active_slot, active_epoch)
        VALUES(1, 'a', 0);
        "
    ))?;
    connection.execute_batch(GRAPH_STRUCTURAL_SLOTS_DELETE_GUARD_SQL)?;
    connection.execute_batch(GRAPH_STRUCTURAL_SLOTS_UPDATE_GUARD_SQL)?;
    connection.execute_batch(GRAPH_PUBLICATION_STATE_DELETE_GUARD_SQL)?;
    Ok(())
}

/// Reconcile the exact structural slot identities and singleton publication state.
fn verify_structural_publication_schema(connection: &Connection) -> DbResult<()> {
    if table_columns(connection, GRAPH_STRUCTURAL_SLOTS_TABLE)? != ["slot"] {
        return Err(preflight_error(
            "structural slot identity table did not reconcile",
        ));
    }
    if table_columns(connection, GRAPH_PUBLICATION_STATE_TABLE)?
        != ["singleton", "active_slot", "active_epoch"]
    {
        return Err(preflight_error(
            "structural publication state table did not reconcile",
        ));
    }

    let slots = connection
        .prepare("SELECT slot FROM graph_structural_slots ORDER BY slot")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if slots != ["a", "b"] {
        return Err(preflight_error(format!(
            "structural slot identities did not reconcile: {slots:?}"
        )));
    }

    let publication_rows =
        connection.query_row("SELECT COUNT(*) FROM graph_publication_state", [], |row| {
            row.get::<_, i64>(0)
        })?;
    if publication_rows != 1 {
        return Err(preflight_error(format!(
            "structural publication state must contain one row, found {publication_rows}"
        )));
    }
    let publication = connection.query_row(
        "SELECT singleton, active_slot, active_epoch FROM graph_publication_state",
        [],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        },
    )?;
    if publication.0 != 1 || !slots.iter().any(|slot| slot == &publication.1) || publication.2 < 0 {
        return Err(preflight_error(format!(
            "structural publication state did not reconcile: {publication:?}"
        )));
    }

    for (trigger, table, expected_sql) in [
        (
            GRAPH_STRUCTURAL_SLOTS_DELETE_GUARD,
            GRAPH_STRUCTURAL_SLOTS_TABLE,
            GRAPH_STRUCTURAL_SLOTS_DELETE_GUARD_SQL,
        ),
        (
            GRAPH_STRUCTURAL_SLOTS_UPDATE_GUARD,
            GRAPH_STRUCTURAL_SLOTS_TABLE,
            GRAPH_STRUCTURAL_SLOTS_UPDATE_GUARD_SQL,
        ),
        (
            GRAPH_PUBLICATION_STATE_DELETE_GUARD,
            GRAPH_PUBLICATION_STATE_TABLE,
            GRAPH_PUBLICATION_STATE_DELETE_GUARD_SQL,
        ),
    ] {
        require_trigger_contract(
            connection,
            trigger,
            table,
            expected_sql,
            "publication guard",
        )?;
    }
    Ok(())
}

/// Require one schema-owned trigger to retain its canonical owner and body.
fn require_trigger_contract(
    connection: &Connection,
    trigger: &str,
    table: &str,
    expected_sql: &str,
    role: &str,
) -> DbResult<()> {
    let observed = connection
        .query_row(
            "SELECT type, tbl_name, sql FROM sqlite_schema WHERE name = ?1",
            [trigger],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    let expected = normalize_schema_sql(expected_sql);
    if matches!(
        observed.as_ref(),
        Some((object_type, table_name, Some(sql)))
            if object_type == "trigger"
                && table_name == table
                && normalize_schema_sql(sql) == expected
    ) {
        Ok(())
    } else {
        Err(preflight_error(format!(
            "structural {role} {trigger:?} did not reconcile with its canonical SQL: {observed:?}"
        )))
    }
}

/// Return whether one installed trigger exactly matches its schema-owned contract.
fn trigger_contract_matches(
    connection: &Connection,
    trigger: &str,
    table: &str,
    expected_sql: &str,
) -> DbResult<bool> {
    let observed = connection
        .query_row(
            "SELECT type, tbl_name, sql FROM sqlite_schema WHERE name = ?1",
            [trigger],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    let expected = normalize_schema_sql(expected_sql);
    Ok(matches!(
        observed.as_ref(),
        Some((object_type, table_name, Some(sql)))
            if object_type == "trigger"
                && table_name == table
                && normalize_schema_sql(sql) == expected
    ))
}

/// Normalize only SQL whitespace outside quoted literals and a trailing delimiter.
fn normalize_schema_sql(sql: &str) -> String {
    let mut normalized = String::with_capacity(sql.len());
    let mut characters = sql
        .trim()
        .trim_end_matches(';')
        .trim_end()
        .chars()
        .peekable();
    let mut quote = None;
    let mut pending_space = false;
    while let Some(character) = characters.next() {
        if let Some(delimiter) = quote {
            normalized.push(character);
            if character == delimiter {
                if characters.next_if_eq(&delimiter).is_some() {
                    normalized.push(delimiter);
                } else {
                    quote = None;
                }
            }
            continue;
        }
        if matches!(character, '\'' | '"' | '`') {
            if pending_space && !normalized.is_empty() {
                normalized.push(' ');
            }
            pending_space = false;
            quote = Some(character);
            normalized.push(character);
        } else if character.is_whitespace() {
            pending_space = true;
        } else {
            if pending_space && !normalized.is_empty() {
                normalized.push(' ');
            }
            pending_space = false;
            normalized.push(character);
        }
    }
    normalized
}

/// Rebuild core typed graph and lexical rows around structural slot identity.
fn bind_structural_derived_rows(connection: &Connection) -> DbResult<()> {
    let had_file_text_fts = sqlite_objects(connection)?.contains_key(FILE_TEXT_FTS_TABLE);
    if had_file_text_fts {
        connection.execute_batch(
            "DROP TRIGGER file_text_fts_insert;
             DROP TRIGGER file_text_fts_delete;
             DROP TRIGGER file_text_fts_update;
             DROP TABLE file_text_fts;",
        )?;
    }

    connection.execute_batch(&format!(
        "
        CREATE TABLE graph_entities_with_slot (
            stable_key_digest BLOB NOT NULL CHECK(length(stable_key_digest) = 32),
            stable_key_version INTEGER NOT NULL CHECK(stable_key_version > 0),
            stable_key_canonical BLOB NOT NULL,
            project_instance_id BLOB NOT NULL CHECK(length(project_instance_id) = 16),
            entity_kind TEXT NOT NULL CHECK(entity_kind IN (
                'repository', 'folder', 'file', 'package', 'module', 'declaration',
                'reference', 'endpoint', 'route', 'channel', 'configuration',
                'environment', 'infrastructure', 'test', 'external'
            )),
            repository_path TEXT,
            qualified_name TEXT,
            signature TEXT,
            discriminator TEXT,
            external_namespace TEXT,
            external_value TEXT,
            language TEXT,
            source_start_byte INTEGER,
            source_end_byte INTEGER,
            source_start_line INTEGER,
            source_end_line INTEGER,
            parser_kind TEXT NOT NULL CHECK(parser_kind IN ({GRAPH_PARSER_KIND_VALUES})),
            parser_identity TEXT NOT NULL,
            parser_version TEXT NOT NULL,
            structural_slot TEXT NOT NULL DEFAULT 'a'
                CHECK(structural_slot IN ({GRAPH_STRUCTURAL_SLOT_VALUES})),
            last_changed_epoch INTEGER NOT NULL DEFAULT 0 CHECK(last_changed_epoch >= 0),
            PRIMARY KEY(structural_slot, stable_key_digest),
            UNIQUE(structural_slot, stable_key_canonical),
            FOREIGN KEY(structural_slot)
                REFERENCES graph_structural_slots(slot) ON UPDATE RESTRICT ON DELETE RESTRICT,
            CHECK(
                (source_start_byte IS NULL
                    AND source_end_byte IS NULL
                    AND source_start_line IS NULL
                    AND source_end_line IS NULL)
                OR
                (source_start_byte >= 0
                    AND source_end_byte >= source_start_byte
                    AND source_start_line > 0
                    AND source_end_line >= source_start_line)
            )
        );

        CREATE TABLE graph_relations_with_slot (
            stable_key_digest BLOB NOT NULL CHECK(length(stable_key_digest) = 32),
            stable_key_version INTEGER NOT NULL CHECK(stable_key_version > 0),
            stable_key_canonical BLOB NOT NULL,
            source_entity_digest BLOB NOT NULL CHECK(length(source_entity_digest) = 32),
            relation_kind TEXT NOT NULL CHECK(relation_kind IN ({GRAPH_RELATION_KIND_VALUES})),
            resolution_status TEXT NOT NULL DEFAULT 'resolved'
                CHECK(resolution_status = 'resolved'),
            target_scope TEXT NOT NULL CHECK(target_scope IN ({GRAPH_TARGET_SCOPE_VALUES})),
            target_entity_digest BLOB,
            external_target_namespace TEXT,
            external_target_value TEXT,
            confidence TEXT NOT NULL CHECK(confidence IN ({GRAPH_CONFIDENCE_VALUES})),
            parser_kind TEXT NOT NULL CHECK(parser_kind IN ({GRAPH_PARSER_KIND_VALUES})),
            parser_identity TEXT NOT NULL,
            parser_version TEXT NOT NULL,
            structural_slot TEXT NOT NULL DEFAULT 'a'
                CHECK(structural_slot IN ({GRAPH_STRUCTURAL_SLOT_VALUES})),
            last_changed_epoch INTEGER NOT NULL DEFAULT 0 CHECK(last_changed_epoch >= 0),
            PRIMARY KEY(structural_slot, stable_key_digest),
            UNIQUE(structural_slot, stable_key_canonical),
            FOREIGN KEY(structural_slot)
                REFERENCES graph_structural_slots(slot) ON UPDATE RESTRICT ON DELETE RESTRICT,
            FOREIGN KEY(structural_slot, source_entity_digest)
                REFERENCES graph_entities_with_slot(structural_slot, stable_key_digest)
                ON DELETE CASCADE,
            FOREIGN KEY(structural_slot, target_entity_digest)
                REFERENCES graph_entities_with_slot(structural_slot, stable_key_digest)
                ON DELETE CASCADE,
            CHECK(
                (target_scope = 'internal'
                    AND length(target_entity_digest) = 32
                    AND external_target_namespace IS NULL
                    AND external_target_value IS NULL)
                OR
                (target_scope = 'external'
                    AND target_entity_digest IS NULL
                    AND external_target_namespace IS NOT NULL
                    AND external_target_value IS NOT NULL)
            )
        );

        CREATE TABLE graph_evidence_occurrences_with_slot (
            stable_key_digest BLOB NOT NULL CHECK(length(stable_key_digest) = 32),
            stable_key_version INTEGER NOT NULL CHECK(stable_key_version > 0),
            stable_key_canonical BLOB NOT NULL,
            relation_digest BLOB NOT NULL CHECK(length(relation_digest) = 32),
            origin_kind TEXT NOT NULL CHECK(origin_kind IN ({GRAPH_EVIDENCE_ORIGIN_VALUES})),
            origin_entity_digest BLOB,
            origin_project_instance_id BLOB,
            origin_repository_path TEXT,
            origin_external_namespace TEXT,
            origin_external_value TEXT,
            source_start_byte INTEGER,
            source_end_byte INTEGER,
            source_start_line INTEGER,
            source_end_line INTEGER,
            resolver_name TEXT NOT NULL,
            resolver_version TEXT NOT NULL,
            content_span_fingerprint BLOB NOT NULL CHECK(length(content_span_fingerprint) = 32),
            occurrence_discriminator INTEGER NOT NULL CHECK(occurrence_discriminator >= 0),
            evidence_class TEXT NOT NULL CHECK(evidence_class IN ({GRAPH_EVIDENCE_CLASS_VALUES})),
            confidence TEXT NOT NULL CHECK(confidence IN ({GRAPH_CONFIDENCE_VALUES})),
            completeness TEXT NOT NULL CHECK(completeness IN ({GRAPH_COMPLETENESS_VALUES})),
            explanation TEXT,
            structural_slot TEXT NOT NULL DEFAULT 'a'
                CHECK(structural_slot IN ({GRAPH_STRUCTURAL_SLOT_VALUES})),
            last_changed_epoch INTEGER NOT NULL DEFAULT 0 CHECK(last_changed_epoch >= 0),
            PRIMARY KEY(structural_slot, stable_key_digest),
            UNIQUE(structural_slot, stable_key_canonical),
            FOREIGN KEY(structural_slot)
                REFERENCES graph_structural_slots(slot) ON UPDATE RESTRICT ON DELETE RESTRICT,
            FOREIGN KEY(structural_slot, relation_digest)
                REFERENCES graph_relations_with_slot(structural_slot, stable_key_digest)
                ON DELETE CASCADE,
            FOREIGN KEY(structural_slot, origin_entity_digest)
                REFERENCES graph_entities_with_slot(structural_slot, stable_key_digest)
                ON DELETE CASCADE,
            CHECK(
                (origin_kind = 'entity'
                    AND length(origin_entity_digest) = 32
                    AND origin_project_instance_id IS NULL
                    AND origin_repository_path IS NULL
                    AND origin_external_namespace IS NULL
                    AND origin_external_value IS NULL)
                OR
                (origin_kind = 'repository-path'
                    AND origin_entity_digest IS NULL
                    AND length(origin_project_instance_id) = 16
                    AND origin_repository_path IS NOT NULL
                    AND origin_external_namespace IS NULL
                    AND origin_external_value IS NULL)
                OR
                (origin_kind = 'external'
                    AND origin_entity_digest IS NULL
                    AND origin_project_instance_id IS NULL
                    AND origin_repository_path IS NULL
                    AND origin_external_namespace IS NOT NULL
                    AND origin_external_value IS NOT NULL)
            ),
            CHECK(
                (source_start_byte IS NULL
                    AND source_end_byte IS NULL
                    AND source_start_line IS NULL
                    AND source_end_line IS NULL)
                OR
                (source_start_byte >= 0
                    AND source_end_byte >= source_start_byte
                    AND source_start_line > 0
                    AND source_end_line >= source_start_line)
            )
        );

        CREATE TABLE graph_resolution_occurrences_with_slot (
            stable_key_digest BLOB NOT NULL CHECK(length(stable_key_digest) = 32),
            stable_key_version INTEGER NOT NULL CHECK(stable_key_version > 0),
            stable_key_canonical BLOB NOT NULL,
            source_entity_digest BLOB NOT NULL CHECK(length(source_entity_digest) = 32),
            relation_kind TEXT NOT NULL CHECK(relation_kind IN ({GRAPH_RELATION_KIND_VALUES})),
            origin_kind TEXT NOT NULL CHECK(origin_kind IN ({GRAPH_EVIDENCE_ORIGIN_VALUES})),
            origin_entity_digest BLOB,
            origin_project_instance_id BLOB,
            origin_repository_path TEXT,
            origin_external_namespace TEXT,
            origin_external_value TEXT,
            source_start_byte INTEGER,
            source_end_byte INTEGER,
            source_start_line INTEGER,
            source_end_line INTEGER,
            resolver_name TEXT NOT NULL,
            resolver_version TEXT NOT NULL,
            content_span_fingerprint BLOB NOT NULL CHECK(length(content_span_fingerprint) = 32),
            occurrence_discriminator INTEGER NOT NULL CHECK(occurrence_discriminator >= 0),
            resolution_status TEXT NOT NULL
                CHECK(resolution_status IN ('ambiguous', 'unresolved')),
            candidate_total INTEGER,
            candidate_completeness TEXT
                CHECK(candidate_completeness IN ({GRAPH_COMPLETENESS_VALUES})),
            unresolved_reason TEXT,
            evidence_class TEXT NOT NULL CHECK(evidence_class IN ({GRAPH_EVIDENCE_CLASS_VALUES})),
            confidence TEXT NOT NULL CHECK(confidence IN ({GRAPH_CONFIDENCE_VALUES})),
            completeness TEXT NOT NULL CHECK(completeness IN ({GRAPH_COMPLETENESS_VALUES})),
            parser_kind TEXT NOT NULL CHECK(parser_kind IN ({GRAPH_PARSER_KIND_VALUES})),
            parser_identity TEXT NOT NULL,
            parser_version TEXT NOT NULL,
            structural_slot TEXT NOT NULL DEFAULT 'a'
                CHECK(structural_slot IN ({GRAPH_STRUCTURAL_SLOT_VALUES})),
            last_changed_epoch INTEGER NOT NULL DEFAULT 0 CHECK(last_changed_epoch >= 0),
            PRIMARY KEY(structural_slot, stable_key_digest),
            UNIQUE(structural_slot, stable_key_canonical),
            FOREIGN KEY(structural_slot)
                REFERENCES graph_structural_slots(slot) ON UPDATE RESTRICT ON DELETE RESTRICT,
            FOREIGN KEY(structural_slot, source_entity_digest)
                REFERENCES graph_entities_with_slot(structural_slot, stable_key_digest)
                ON DELETE CASCADE,
            FOREIGN KEY(structural_slot, origin_entity_digest)
                REFERENCES graph_entities_with_slot(structural_slot, stable_key_digest)
                ON DELETE CASCADE,
            CHECK(
                (origin_kind = 'entity'
                    AND length(origin_entity_digest) = 32
                    AND origin_project_instance_id IS NULL
                    AND origin_repository_path IS NULL
                    AND origin_external_namespace IS NULL
                    AND origin_external_value IS NULL)
                OR
                (origin_kind = 'repository-path'
                    AND origin_entity_digest IS NULL
                    AND length(origin_project_instance_id) = 16
                    AND origin_repository_path IS NOT NULL
                    AND origin_external_namespace IS NULL
                    AND origin_external_value IS NULL)
                OR
                (origin_kind = 'external'
                    AND origin_entity_digest IS NULL
                    AND origin_project_instance_id IS NULL
                    AND origin_repository_path IS NULL
                    AND origin_external_namespace IS NOT NULL
                    AND origin_external_value IS NOT NULL)
            ),
            CHECK(
                (source_start_byte IS NULL
                    AND source_end_byte IS NULL
                    AND source_start_line IS NULL
                    AND source_end_line IS NULL)
                OR
                (source_start_byte >= 0
                    AND source_end_byte >= source_start_byte
                    AND source_start_line > 0
                    AND source_end_line >= source_start_line)
            ),
            CHECK(
                (resolution_status = 'ambiguous'
                    AND candidate_total >= 2
                    AND candidate_completeness IS NOT NULL
                    AND unresolved_reason IS NULL)
                OR
                (resolution_status = 'unresolved'
                    AND candidate_total IS NULL
                    AND candidate_completeness IS NULL)
            )
        );

        CREATE TABLE graph_resolution_candidates_with_slot (
            resolution_occurrence_digest BLOB NOT NULL
                CHECK(length(resolution_occurrence_digest) = 32),
            candidate_ordinal INTEGER NOT NULL CHECK(candidate_ordinal >= 0),
            target_scope TEXT NOT NULL CHECK(target_scope IN ({GRAPH_TARGET_SCOPE_VALUES})),
            target_entity_digest BLOB,
            external_target_namespace TEXT,
            external_target_value TEXT,
            confidence TEXT NOT NULL CHECK(confidence IN ({GRAPH_CONFIDENCE_VALUES})),
            explanation TEXT,
            structural_slot TEXT NOT NULL DEFAULT 'a'
                CHECK(structural_slot IN ({GRAPH_STRUCTURAL_SLOT_VALUES})),
            last_changed_epoch INTEGER NOT NULL DEFAULT 0 CHECK(last_changed_epoch >= 0),
            PRIMARY KEY(structural_slot, resolution_occurrence_digest, candidate_ordinal),
            FOREIGN KEY(structural_slot)
                REFERENCES graph_structural_slots(slot) ON UPDATE RESTRICT ON DELETE RESTRICT,
            FOREIGN KEY(structural_slot, resolution_occurrence_digest)
                REFERENCES graph_resolution_occurrences_with_slot(
                    structural_slot, stable_key_digest
                ) ON DELETE CASCADE,
            FOREIGN KEY(structural_slot, target_entity_digest)
                REFERENCES graph_entities_with_slot(structural_slot, stable_key_digest)
                ON DELETE CASCADE,
            CHECK(
                (target_scope = 'internal'
                    AND length(target_entity_digest) = 32
                    AND external_target_namespace IS NULL
                    AND external_target_value IS NULL)
                OR
                (target_scope = 'external'
                    AND target_entity_digest IS NULL
                    AND external_target_namespace IS NOT NULL
                    AND external_target_value IS NOT NULL)
            )
        );

        CREATE TABLE graph_coverage_with_slot (
            scope_kind TEXT NOT NULL
                CHECK(scope_kind IN ('repository', 'file', 'pass', 'relation')),
            repository_path TEXT NOT NULL DEFAULT '',
            pass_identity TEXT NOT NULL DEFAULT '',
            relation_kind TEXT NOT NULL DEFAULT '' CHECK(
                relation_kind = ''
                OR relation_kind IN ({GRAPH_RELATION_KIND_VALUES})
            ),
            coverage_state TEXT NOT NULL CHECK(coverage_state IN (
                'complete', 'partial', 'failed', 'ignored', 'oversized',
                'quarantined', 'stale'
            )),
            produced_count INTEGER NOT NULL CHECK(produced_count >= 0),
            omitted_count INTEGER CHECK(omitted_count >= 0),
            reached_limit TEXT CHECK(
                reached_limit IS NULL
                OR reached_limit IN (
                    'source_file_bytes', 'ast_depth', 'symbols_per_file',
                    'relations_per_file', 'resolution_candidates', 'worker_count',
                    'stage_time', 'working_memory', 'query_depth', 'visited_nodes',
                    'expanded_edges', 'returned_rows', 'response_bytes',
                    'cancellation_poll', 'cancellation_grace'
                )
            ),
            reason TEXT,
            structural_slot TEXT NOT NULL CHECK(structural_slot IN ({GRAPH_STRUCTURAL_SLOT_VALUES})),
            last_changed_epoch INTEGER NOT NULL CHECK(last_changed_epoch >= 0),
            PRIMARY KEY(
                scope_kind,
                repository_path,
                pass_identity,
                relation_kind,
                structural_slot
            ),
            FOREIGN KEY(structural_slot)
                REFERENCES graph_structural_slots(slot) ON UPDATE RESTRICT ON DELETE RESTRICT,
            CHECK(
                (scope_kind = 'repository'
                    AND repository_path = ''
                    AND pass_identity = ''
                    AND relation_kind = '')
                OR
                (scope_kind = 'file'
                    AND repository_path != ''
                    AND pass_identity = ''
                    AND relation_kind = '')
                OR
                (scope_kind = 'pass'
                    AND repository_path != ''
                    AND pass_identity != ''
                    AND relation_kind = '')
                OR
                (scope_kind = 'relation'
                    AND repository_path != ''
                    AND pass_identity = ''
                    AND relation_kind != '')
            )
        );

        CREATE TABLE file_texts_with_slot (
            path TEXT NOT NULL,
            content_hash TEXT,
            byte_count INTEGER NOT NULL,
            line_count INTEGER NOT NULL,
            content TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            structural_slot TEXT NOT NULL DEFAULT 'a'
                CHECK(structural_slot IN ({GRAPH_STRUCTURAL_SLOT_VALUES})),
            last_changed_epoch INTEGER NOT NULL DEFAULT 0 CHECK(last_changed_epoch >= 0),
            PRIMARY KEY(structural_slot, path),
            FOREIGN KEY(structural_slot)
                REFERENCES graph_structural_slots(slot) ON UPDATE RESTRICT ON DELETE RESTRICT
        );

        INSERT INTO graph_entities_with_slot(
            stable_key_digest, stable_key_version, stable_key_canonical,
            project_instance_id, entity_kind, repository_path, qualified_name,
            signature, discriminator, external_namespace, external_value, language,
            source_start_byte, source_end_byte, source_start_line, source_end_line,
            parser_kind, parser_identity, parser_version,
            structural_slot, last_changed_epoch
        )
        SELECT
            entity.stable_key_digest, entity.stable_key_version,
            entity.stable_key_canonical, entity.project_instance_id,
            entity.entity_kind, entity.repository_path, entity.qualified_name,
            entity.signature, entity.discriminator, entity.external_namespace,
            entity.external_value, entity.language, entity.source_start_byte,
            entity.source_end_byte, entity.source_start_line, entity.source_end_line,
            entity.parser_kind, entity.parser_identity, entity.parser_version,
            publication.active_slot, publication.active_epoch
        FROM graph_entities AS entity
        CROSS JOIN graph_publication_state AS publication
        WHERE publication.singleton = 1;

        INSERT INTO graph_relations_with_slot(
            stable_key_digest, stable_key_version, stable_key_canonical,
            source_entity_digest, relation_kind, resolution_status, target_scope,
            target_entity_digest, external_target_namespace, external_target_value,
            confidence, parser_kind, parser_identity, parser_version,
            structural_slot, last_changed_epoch
        )
        SELECT
            relation.stable_key_digest, relation.stable_key_version,
            relation.stable_key_canonical, relation.source_entity_digest,
            relation.relation_kind, relation.resolution_status, relation.target_scope,
            relation.target_entity_digest, relation.external_target_namespace,
            relation.external_target_value, relation.confidence, relation.parser_kind,
            relation.parser_identity, relation.parser_version,
            publication.active_slot, publication.active_epoch
        FROM graph_relations AS relation
        CROSS JOIN graph_publication_state AS publication
        WHERE publication.singleton = 1;

        INSERT INTO graph_evidence_occurrences_with_slot(
            stable_key_digest, stable_key_version, stable_key_canonical,
            relation_digest, origin_kind, origin_entity_digest,
            origin_project_instance_id, origin_repository_path,
            origin_external_namespace, origin_external_value, source_start_byte,
            source_end_byte, source_start_line, source_end_line, resolver_name,
            resolver_version, content_span_fingerprint, occurrence_discriminator,
            evidence_class, confidence, completeness, explanation,
            structural_slot, last_changed_epoch
        )
        SELECT
            evidence.stable_key_digest, evidence.stable_key_version,
            evidence.stable_key_canonical, evidence.relation_digest,
            evidence.origin_kind, evidence.origin_entity_digest,
            evidence.origin_project_instance_id, evidence.origin_repository_path,
            evidence.origin_external_namespace, evidence.origin_external_value,
            evidence.source_start_byte, evidence.source_end_byte,
            evidence.source_start_line, evidence.source_end_line,
            evidence.resolver_name, evidence.resolver_version,
            evidence.content_span_fingerprint, evidence.occurrence_discriminator,
            evidence.evidence_class, evidence.confidence, evidence.completeness,
            evidence.explanation, publication.active_slot, publication.active_epoch
        FROM graph_evidence_occurrences AS evidence
        CROSS JOIN graph_publication_state AS publication
        WHERE publication.singleton = 1;

        INSERT INTO graph_resolution_occurrences_with_slot(
            stable_key_digest, stable_key_version, stable_key_canonical,
            source_entity_digest, relation_kind, origin_kind, origin_entity_digest,
            origin_project_instance_id, origin_repository_path,
            origin_external_namespace, origin_external_value, source_start_byte,
            source_end_byte, source_start_line, source_end_line, resolver_name,
            resolver_version, content_span_fingerprint, occurrence_discriminator,
            resolution_status, candidate_total, candidate_completeness,
            unresolved_reason, evidence_class, confidence, completeness,
            parser_kind, parser_identity, parser_version,
            structural_slot, last_changed_epoch
        )
        SELECT
            occurrence.stable_key_digest, occurrence.stable_key_version,
            occurrence.stable_key_canonical, occurrence.source_entity_digest,
            occurrence.relation_kind, occurrence.origin_kind,
            occurrence.origin_entity_digest, occurrence.origin_project_instance_id,
            occurrence.origin_repository_path, occurrence.origin_external_namespace,
            occurrence.origin_external_value, occurrence.source_start_byte,
            occurrence.source_end_byte, occurrence.source_start_line,
            occurrence.source_end_line, occurrence.resolver_name,
            occurrence.resolver_version, occurrence.content_span_fingerprint,
            occurrence.occurrence_discriminator, occurrence.resolution_status,
            occurrence.candidate_total, occurrence.candidate_completeness,
            occurrence.unresolved_reason, occurrence.evidence_class,
            occurrence.confidence, occurrence.completeness, occurrence.parser_kind,
            occurrence.parser_identity, occurrence.parser_version,
            publication.active_slot, publication.active_epoch
        FROM graph_resolution_occurrences AS occurrence
        CROSS JOIN graph_publication_state AS publication
        WHERE publication.singleton = 1;

        INSERT INTO graph_resolution_candidates_with_slot(
            resolution_occurrence_digest, candidate_ordinal, target_scope,
            target_entity_digest, external_target_namespace, external_target_value,
            confidence, explanation, structural_slot, last_changed_epoch
        )
        SELECT
            candidate.resolution_occurrence_digest, candidate.candidate_ordinal,
            candidate.target_scope, candidate.target_entity_digest,
            candidate.external_target_namespace, candidate.external_target_value,
            candidate.confidence, candidate.explanation,
            publication.active_slot, publication.active_epoch
        FROM graph_resolution_candidates AS candidate
        CROSS JOIN graph_publication_state AS publication
        WHERE publication.singleton = 1;

        INSERT INTO graph_coverage_with_slot(
            scope_kind, repository_path, pass_identity, relation_kind,
            coverage_state, produced_count, omitted_count, reached_limit,
            reason, structural_slot, last_changed_epoch
        )
        SELECT
            scope_kind, repository_path, pass_identity, relation_kind,
            coverage_state, produced_count, omitted_count, reached_limit,
            reason, structural_slot, last_changed_epoch
        FROM graph_coverage;

        INSERT INTO file_texts_with_slot(
            path, content_hash, byte_count, line_count, content, updated_at,
            structural_slot, last_changed_epoch
        )
        SELECT
            text.path, text.content_hash, text.byte_count, text.line_count,
            text.content, text.updated_at,
            publication.active_slot, publication.active_epoch
        FROM file_texts AS text
        CROSS JOIN graph_publication_state AS publication
        WHERE publication.singleton = 1;

        DROP TABLE graph_resolution_candidates;
        DROP TABLE graph_evidence_occurrences;
        DROP TABLE graph_resolution_occurrences;
        DROP TABLE graph_relations;
        DROP TABLE graph_coverage;
        DROP TABLE graph_entities;
        DROP TABLE file_texts;

        ALTER TABLE graph_entities_with_slot RENAME TO graph_entities;
        ALTER TABLE graph_relations_with_slot RENAME TO graph_relations;
        ALTER TABLE graph_evidence_occurrences_with_slot
            RENAME TO graph_evidence_occurrences;
        ALTER TABLE graph_resolution_occurrences_with_slot
            RENAME TO graph_resolution_occurrences;
        ALTER TABLE graph_resolution_candidates_with_slot
            RENAME TO graph_resolution_candidates;
        ALTER TABLE graph_coverage_with_slot RENAME TO graph_coverage;
        ALTER TABLE file_texts_with_slot RENAME TO file_texts;

        CREATE INDEX idx_file_texts_hash ON file_texts(content_hash);
        "
    ))?;

    install_affected_reconciliation_indexes(connection)?;
    if had_file_text_fts {
        create_slot_aware_file_text_fts(connection)?;
    }
    Ok(())
}

/// Recreate optional FTS candidate rows with the same structural identity as source text.
fn create_slot_aware_file_text_fts(connection: &Connection) -> DbResult<()> {
    connection.execute_batch(&format!(
        "CREATE VIRTUAL TABLE file_text_fts
         USING fts5(
             structural_slot UNINDEXED,
             last_changed_epoch UNINDEXED,
             path UNINDEXED,
             path_lookup,
             content,
             tokenize='trigram'
         );

         INSERT INTO file_text_fts(
             structural_slot, last_changed_epoch, path, path_lookup, content
         )
         SELECT structural_slot, last_changed_epoch, path,
                '{FILE_TEXT_FTS_PATH_LOOKUP_PREFIX}' || path, content
         FROM file_texts
         ORDER BY structural_slot, path;",
    ))?;
    connection.execute_batch(&file_text_fts_insert_trigger_sql())?;
    connection.execute_batch(FILE_TEXT_FTS_DELETE_TRIGGER_SQL)?;
    connection.execute_batch(&file_text_fts_update_trigger_sql())?;
    Ok(())
}

/// Install the bounded path-range indexes shared by mutation and reconciliation.
fn install_affected_reconciliation_indexes(connection: &Connection) -> DbResult<()> {
    connection.execute_batch(AFFECTED_RECONCILIATION_INDEX_SQL)?;
    Ok(())
}

/// Upgrade an existing slot-bound schema to indexed affected-row reconciliation.
fn install_affected_reconciliation_contracts(connection: &Connection) -> DbResult<()> {
    install_affected_reconciliation_indexes(connection)?;
    let objects = sqlite_objects(connection)?;
    if !objects.contains_key(FILE_TEXT_FTS_TABLE) {
        return Ok(());
    }
    let current_columns = table_columns(connection, FILE_TEXT_FTS_TABLE)?
        == [
            "structural_slot",
            "last_changed_epoch",
            "path",
            "path_lookup",
            "content",
        ];
    let current_triggers = trigger_contract_matches(
        connection,
        FILE_TEXT_FTS_INSERT_TRIGGER,
        FILE_TEXTS_TABLE,
        &file_text_fts_insert_trigger_sql(),
    )? && trigger_contract_matches(
        connection,
        FILE_TEXT_FTS_DELETE_TRIGGER,
        FILE_TEXTS_TABLE,
        FILE_TEXT_FTS_DELETE_TRIGGER_SQL,
    )? && trigger_contract_matches(
        connection,
        FILE_TEXT_FTS_UPDATE_TRIGGER,
        FILE_TEXTS_TABLE,
        &file_text_fts_update_trigger_sql(),
    )?;
    if !current_columns || !current_triggers {
        connection.execute_batch(
            "DROP TRIGGER IF EXISTS file_text_fts_insert;
             DROP TRIGGER IF EXISTS file_text_fts_delete;
             DROP TRIGGER IF EXISTS file_text_fts_update;
             DROP TABLE file_text_fts;",
        )?;
        create_slot_aware_file_text_fts(connection)?;
    }
    Ok(())
}

/// Verify the post-upgrade schema and complete FTS preservation before sealing migration.
fn verify_affected_reconciliation_contracts(connection: &Connection) -> DbResult<()> {
    verify_structural_derived_row_schema(connection)?;
    reconcile_full_file_text_fts(connection)
}

/// Verify structural identity for every current core graph and lexical row family.
fn verify_structural_derived_row_schema(connection: &Connection) -> DbResult<()> {
    verify_typed_repository_graph_schema(connection)?;
    if table_columns(connection, FILE_TEXTS_TABLE)?
        != [
            "path",
            "content_hash",
            "byte_count",
            "line_count",
            "content",
            "updated_at",
            "structural_slot",
            "last_changed_epoch",
        ]
    {
        return Err(preflight_error(
            "authoritative file-text structural columns did not reconcile",
        ));
    }
    let file_text_key = table_primary_key_columns(connection, FILE_TEXTS_TABLE)?;
    if file_text_key != ["structural_slot", "path"] {
        return Err(preflight_error(format!(
            "authoritative file-text primary key did not reconcile: {file_text_key:?}"
        )));
    }
    require_foreign_key(
        connection,
        FILE_TEXTS_TABLE,
        GRAPH_STRUCTURAL_SLOTS_TABLE,
        &[("structural_slot", "slot")],
    )?;
    verify_optional_file_text_fts_schema(connection)?;
    Ok(())
}

/// Verify the bound schema and its complete optional FTS backfill before sealing migration.
fn verify_bound_structural_derived_rows(connection: &Connection) -> DbResult<()> {
    verify_structural_derived_row_schema(connection)?;
    reconcile_full_file_text_fts(connection)
}

/// Verify slot-aware FTS schema or an unavailable exact-only runtime.
fn verify_optional_file_text_fts_schema(connection: &Connection) -> DbResult<()> {
    let objects = sqlite_objects(connection)?;
    if !objects.contains_key(FILE_TEXT_FTS_TABLE) {
        return Ok(());
    }
    if table_columns(connection, FILE_TEXT_FTS_TABLE)?
        != [
            "structural_slot",
            "last_changed_epoch",
            "path",
            "path_lookup",
            "content",
        ]
    {
        return Err(preflight_error(
            "optional file-text FTS structural columns did not reconcile",
        ));
    }

    for (trigger, expected_sql) in [
        (
            FILE_TEXT_FTS_INSERT_TRIGGER,
            file_text_fts_insert_trigger_sql(),
        ),
        (
            FILE_TEXT_FTS_DELETE_TRIGGER,
            FILE_TEXT_FTS_DELETE_TRIGGER_SQL.to_owned(),
        ),
        (
            FILE_TEXT_FTS_UPDATE_TRIGGER,
            file_text_fts_update_trigger_sql(),
        ),
    ] {
        require_trigger_contract(
            connection,
            trigger,
            FILE_TEXTS_TABLE,
            &expected_sql,
            "file-text FTS trigger",
        )?;
    }

    Ok(())
}

/// Verify all slot-aware FTS candidates against authoritative file text.
fn reconcile_full_file_text_fts(connection: &Connection) -> DbResult<()> {
    verify_optional_file_text_fts_schema(connection)?;
    if !sqlite_objects(connection)?.contains_key(FILE_TEXT_FTS_TABLE) {
        return Ok(());
    }

    let source_count = connection.query_row("SELECT COUNT(*) FROM file_texts", [], |row| {
        row.get::<_, i64>(0)
    })?;
    let candidate_count =
        connection.query_row("SELECT COUNT(*) FROM file_text_fts", [], |row| {
            row.get::<_, i64>(0)
        })?;
    let content_mismatch = connection.query_row(
        "SELECT
             EXISTS(
                 SELECT structural_slot, last_changed_epoch, path,
                        ?1 || path, content
                 FROM file_texts
                 EXCEPT
                 SELECT structural_slot, last_changed_epoch, path,
                        path_lookup, content
                 FROM file_text_fts
             )
             OR EXISTS(
                 SELECT structural_slot, last_changed_epoch, path,
                        path_lookup, content
                 FROM file_text_fts
                 EXCEPT
                 SELECT structural_slot, last_changed_epoch, path,
                        ?1 || path, content
                 FROM file_texts
             )",
        [FILE_TEXT_FTS_PATH_LOOKUP_PREFIX],
        |row| row.get::<_, i64>(0),
    )? != 0;
    if source_count != candidate_count || content_mismatch {
        return Err(preflight_error(format!(
            "optional slot-aware file-text FTS content did not reconcile: source={source_count}, candidate={candidate_count}"
        )));
    }
    Ok(())
}

/// Preserve the migration/test proof that schema and complete content agree.
#[cfg(test)]
fn verify_optional_file_text_fts(connection: &Connection) -> DbResult<()> {
    reconcile_full_file_text_fts(connection)
}

/// Classify only the `SQLite` errors that mean the optional FTS capability is absent.
fn is_optional_fts_unavailable(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(_, Some(message))
            if message.contains("no such module: fts5")
                || message.contains("no such tokenizer: trigram")
    )
}

/// Record one accepted migration after its behavior reconciles successfully.
fn record_applied_migration(
    transaction: &Transaction<'_>,
    migration: &AllocatedMigration,
) -> DbResult<()> {
    let statement = format!(
        "INSERT INTO {MIGRATION_LEDGER_TABLE}(schema_version, migration_id, checksum) VALUES(?1, ?2, ?3)"
    );
    transaction.execute(
        &statement,
        params![
            migration.schema_version,
            migration.definition.id,
            migration.checksum
        ],
    )?;
    Ok(())
}

/// Advance metadata only after the migration behavior and ledger row succeed.
fn write_schema_version(connection: &Connection, schema_version: i64) -> DbResult<()> {
    connection.execute(
        "INSERT INTO metadata(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        (SCHEMA_VERSION_METADATA_KEY, schema_version.to_string()),
    )?;
    Ok(())
}

/// Reconcile the declared version with the exact accepted ledger history.
fn verify_schema_history(connection: &Connection, expected_version: i64) -> DbResult<()> {
    let objects = sqlite_objects(connection)?;
    let source = schema_source_state(connection, &objects)?;
    if source
        != (SchemaSourceState::Supported {
            version: expected_version,
            migration_required: false,
        })
    {
        return Err(preflight_error(format!(
            "schema history reconciled to {source:?}, expected version {expected_version}"
        )));
    }
    match migration_ledger_state(connection, &objects, source)? {
        MigrationLedgerState::Compatible(records)
            if records.len() == allocated_migrations().len() => {}
        state => {
            return Err(preflight_error(format!(
                "schema history did not reconcile with the runtime ledger: {state:?}"
            )));
        }
    }
    Ok(())
}

/// Load and validate the persistent identity of one initialized database.
pub(crate) fn project_instance_id(connection: &Connection) -> DbResult<ProjectInstanceId> {
    load_project_instance_id(connection)?.ok_or(DbError::ProjectInstanceIdMissing)
}

/// Preserve an existing identity or initialize one for a fresh or supported legacy database.
fn ensure_project_instance_id(
    connection: &Connection,
    initialize_when_missing: bool,
) -> DbResult<ProjectInstanceId> {
    if let Some(identity) = load_project_instance_id(connection)? {
        return Ok(identity);
    }
    if !initialize_when_missing {
        return Err(DbError::ProjectInstanceIdMissing);
    }

    let value = connection.query_row("SELECT lower(hex(randomblob(16)))", [], |row| {
        row.get::<_, String>(0)
    })?;
    let identity = parse_project_instance_id(value.clone())?;
    connection.execute(
        "INSERT INTO metadata(key, value) VALUES(?1, ?2)",
        (PROJECT_INSTANCE_ID_METADATA_KEY, &value),
    )?;
    Ok(identity)
}

/// Return validated identity metadata when the row exists.
fn load_project_instance_id(connection: &Connection) -> DbResult<Option<ProjectInstanceId>> {
    connection
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [PROJECT_INSTANCE_ID_METADATA_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(parse_project_instance_id)
        .transpose()
}

/// Convert stored identity text through the core graph-domain validator.
fn parse_project_instance_id(value: String) -> DbResult<ProjectInstanceId> {
    ProjectInstanceId::try_from(value.as_str()).map_err(|source| {
        DbError::InvalidProjectInstanceId {
            value,
            source: Box::new(source),
        }
    })
}

/// Add usage telemetry metadata columns to older databases.
fn ensure_usage_event_metadata_columns(connection: &Connection) -> DbResult<()> {
    let columns = table_columns(connection, "usage_events")?;
    for (name, definition) in [
        (
            "token_savings_bucket",
            "TEXT NOT NULL DEFAULT 'navigation_avoidance'",
        ),
        ("provider", "TEXT NOT NULL DEFAULT 'heuristic'"),
        ("model", "TEXT NOT NULL DEFAULT 'unknown'"),
        ("tokenizer_backend", "TEXT NOT NULL DEFAULT 'chars_div_4'"),
        ("accuracy", "TEXT NOT NULL DEFAULT 'heuristic_estimate'"),
        (
            "baseline_kind",
            "TEXT NOT NULL DEFAULT 'selected_candidates'",
        ),
        ("confidence", "TEXT NOT NULL DEFAULT 'inferred'"),
        (
            "calculation_trace",
            "TEXT NOT NULL DEFAULT 'heuristic=ceil(chars_or_bytes/4)'",
        ),
        (
            "accounting_layer",
            "TEXT NOT NULL DEFAULT 'modeled_avoidance'",
        ),
        (
            "estimate_method",
            "TEXT NOT NULL DEFAULT 'heuristic_chars_or_bytes_div_ceil_4'",
        ),
        (
            "denominator_kind",
            "TEXT NOT NULL DEFAULT 'selected_candidates'",
        ),
        ("baseline_identity", "TEXT NOT NULL DEFAULT ''"),
        ("baseline_fingerprint", "TEXT NOT NULL DEFAULT ''"),
        ("dedupe_scope", "TEXT NOT NULL DEFAULT 'session'"),
        ("created_at", "TEXT"),
    ] {
        ensure_usage_event_column(connection, &columns, name, definition)?;
    }
    connection.execute(
        "
        UPDATE usage_events
        SET accounting_layer = 'observed_delta',
            denominator_kind = 'full_file',
            dedupe_scope = 'event'
        WHERE token_savings_bucket = 'full_file_compression'
          AND (
            accounting_layer != 'observed_delta'
            OR denominator_kind != 'full_file'
            OR dedupe_scope != 'event'
          )
        ",
        [],
    )?;
    connection.execute(
        "UPDATE usage_events SET created_at = CURRENT_TIMESTAMP WHERE created_at IS NULL OR created_at = ''",
        [],
    )?;
    Ok(())
}

/// Add one usage-event column when it is absent.
fn ensure_usage_event_column(
    connection: &Connection,
    columns: &[String],
    name: &str,
    definition: &str,
) -> DbResult<()> {
    if columns.iter().any(|column| column == name) {
        return Ok(());
    }
    connection.execute(
        &format!("ALTER TABLE usage_events ADD COLUMN {name} {definition}"),
        [],
    )?;
    Ok(())
}

/// Add optional symbol metadata columns to older databases.
fn ensure_symbol_metadata_columns(connection: &Connection) -> DbResult<()> {
    let columns = table_columns(connection, "symbols")?;
    if !columns.iter().any(|column| column == "exported") {
        connection.execute(
            "ALTER TABLE symbols ADD COLUMN exported INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !columns.iter().any(|column| column == "documentation") {
        connection.execute("ALTER TABLE symbols ADD COLUMN documentation TEXT", [])?;
    }
    Ok(())
}

/// Return the declared columns for one trusted static table name.
fn table_columns(connection: &Connection, table: &str) -> DbResult<Vec<String>> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
}

/// Return declared TEXT columns for one trusted static table name.
fn table_text_columns(connection: &Connection, table: &str) -> DbResult<Vec<String>> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
    })?;
    rows.filter_map(|row| match row {
        Ok((name, declared_type)) if declared_type.eq_ignore_ascii_case("TEXT") => Some(Ok(name)),
        Ok(_) => None,
        Err(error) => Some(Err(error)),
    })
    .collect::<Result<Vec<_>, _>>()
    .map_err(DbError::from)
}

/// Return primary-key columns in their declared composite-key order.
fn table_primary_key_columns(connection: &Connection, table: &str) -> DbResult<Vec<String>> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, i64>(5)?, row.get::<_, String>(1)?))
    })?;
    let mut columns = rows
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|(position, _)| *position > 0)
        .collect::<Vec<_>>();
    columns.sort_by_key(|(position, _)| *position);
    Ok(columns.into_iter().map(|(_, name)| name).collect())
}

/// Require one exact table-level `UNIQUE` contract by ordered columns.
fn require_unique_constraint(
    connection: &Connection,
    table: &str,
    expected_columns: &[&str],
) -> DbResult<()> {
    let mut statement =
        connection.prepare(&format!("PRAGMA index_list({})", quote_identifier(table)))?;
    let indexes = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (index, unique, origin, partial) in indexes {
        if unique != 1 || origin != "u" || partial != 0 {
            continue;
        }
        let mut index_statement =
            connection.prepare(&format!("PRAGMA index_info({})", quote_identifier(&index)))?;
        let columns = index_statement
            .query_map([], |row| row.get::<_, String>(2))?
            .collect::<Result<Vec<_>, _>>()?;
        if columns
            .iter()
            .map(String::as_str)
            .eq(expected_columns.iter().copied())
        {
            return Ok(());
        }
    }
    Err(preflight_error(format!(
        "typed repository graph UNIQUE constraint for {table:?} did not reconcile: expected {expected_columns:?}"
    )))
}

/// Require one exact schema-owned index used by affected-row reconciliation.
fn require_index_contract(
    connection: &Connection,
    index: &str,
    table: &str,
    expected_columns: &[&str],
) -> DbResult<()> {
    let objects = sqlite_objects(connection)?;
    if !matches!(
        objects.get(index),
        Some(object) if object.object_type == "index" && object.table_name == table
    ) {
        return Err(preflight_error(format!(
            "affected-row index {index:?} for {table:?} did not reconcile"
        )));
    }
    let columns = schema_object_columns(connection, SchemaObjectKind::Index, index)?;
    if columns
        .iter()
        .map(String::as_str)
        .eq(expected_columns.iter().copied())
    {
        Ok(())
    } else {
        Err(preflight_error(format!(
            "affected-row index {index:?} columns did not reconcile: expected {expected_columns:?}, found {columns:?}"
        )))
    }
}

/// Require one complete same-parent foreign-key column group.
fn require_foreign_key(
    connection: &Connection,
    table: &str,
    target_table: &str,
    expected_columns: &[(&str, &str)],
) -> DbResult<()> {
    let mut statement = connection.prepare(&format!("PRAGMA foreign_key_list({table})"))?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    let mut groups = BTreeMap::<i64, (String, Vec<(i64, String, String)>)>::new();
    for row in rows {
        let (id, sequence, target, source_column, target_column) = row?;
        let group = groups.entry(id).or_insert_with(|| (target, Vec::new()));
        group.1.push((sequence, source_column, target_column));
    }
    let expected = expected_columns
        .iter()
        .map(|(source, target)| ((*source).to_owned(), (*target).to_owned()))
        .collect::<Vec<_>>();
    let found = groups.into_values().any(|(target, mut columns)| {
        columns.sort_by_key(|(sequence, _, _)| *sequence);
        target == target_table
            && columns
                .into_iter()
                .map(|(_, source, target)| (source, target))
                .collect::<Vec<_>>()
                == expected
    });
    if !found {
        return Err(preflight_error(format!(
            "foreign key from {table:?} to {target_table:?} did not reconcile: {expected:?}"
        )));
    }
    Ok(())
}

/// Drop an in-progress generated summary table that lacks multi-level keys.
fn reset_legacy_summary_schema(connection: &Connection) -> DbResult<()> {
    let exists = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'summaries'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .is_some();
    if !exists {
        return Ok(());
    }
    let columns = table_columns(connection, "summaries")?;
    if !columns.iter().any(|column| column == "subject") {
        connection.execute("DROP TABLE summaries", [])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::io;

    #[test]
    fn store_facade_preserves_schema_and_legacy_repairs() -> Result<(), Box<dyn Error>> {
        let store = crate::AtlasStore::in_memory()?;
        store.initialize_schema()?;
        let connection = &store.connection;

        let version = connection.query_row(
            "SELECT value FROM metadata WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        if version != SCHEMA_VERSION.to_string() {
            return Err(io::Error::other("schema version changed during extraction").into());
        }
        if object_names(connection, "table")?
            != [
                "file_texts",
                "graph_coverage",
                "graph_entities",
                "graph_evidence_occurrences",
                "graph_publication_state",
                "graph_relations",
                "graph_resolution_candidates",
                "graph_resolution_occurrences",
                "graph_structural_slots",
                "health_resolutions",
                "metadata",
                "nodes",
                "purposes",
                "schema_migrations",
                "source_parse_metadata",
                "summaries",
                "symbol_relations",
                "symbols",
                "usage_events",
            ]
        {
            return Err(io::Error::other("table inventory changed during extraction").into());
        }
        if object_names(connection, "index")?
            != [
                "idx_file_texts_hash",
                "idx_graph_coverage_slot_repository_path",
                "idx_graph_entities_slot_repository_path",
                "idx_graph_evidence_slot_origin_repository_path",
                "idx_graph_resolution_slot_origin_repository_path",
                "idx_health_resolutions_category",
                "idx_nodes_kind",
                "idx_nodes_parent",
                "idx_purposes_status",
                "idx_source_parse_metadata_parser",
                "idx_summaries_level",
                "idx_summaries_summary",
                "idx_symbol_relations_path",
                "idx_symbol_relations_target",
                "idx_symbols_kind",
                "idx_symbols_name",
                "idx_symbols_path",
                "idx_usage_created_at",
                "idx_usage_session",
                "idx_usage_session_created_at",
            ]
        {
            return Err(io::Error::other("index inventory changed during extraction").into());
        }
        if object_names(connection, "trigger")?
            != [
                GRAPH_PUBLICATION_STATE_DELETE_GUARD,
                GRAPH_STRUCTURAL_SLOTS_DELETE_GUARD,
                GRAPH_STRUCTURAL_SLOTS_UPDATE_GUARD,
            ]
        {
            return Err(io::Error::other("trigger inventory changed during extraction").into());
        }
        require_column_contract(
            connection,
            "summaries",
            "subject",
            ("TEXT", true, Some("''"), false),
        )?;
        require_column_contract(
            connection,
            "symbols",
            "exported",
            ("INTEGER", true, Some("0"), false),
        )?;
        require_column_contract(
            connection,
            "usage_events",
            "dedupe_scope",
            ("TEXT", true, Some("'session'"), false),
        )?;
        if index_columns(connection, "idx_usage_session_created_at")?
            != ["session_id", "created_at"]
        {
            return Err(io::Error::other("compound usage index changed during extraction").into());
        }

        let legacy = tempfile::tempdir()?;
        let legacy_path = legacy.path().join("legacy.db");
        seed_legacy_schema(&legacy_path, 7)?;
        let legacy_store = crate::AtlasStore::open(&legacy_path)?;
        let legacy_connection = &legacy_store.connection;
        let version = legacy_connection.query_row(
            "SELECT value FROM metadata WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        if version != SCHEMA_VERSION.to_string() {
            return Err(io::Error::other("supported schema version was not reconciled").into());
        }
        require_column_contract(
            legacy_connection,
            "summaries",
            "subject",
            ("TEXT", true, Some("''"), false),
        )?;
        require_column_contract(
            legacy_connection,
            "symbols",
            "exported",
            ("INTEGER", true, Some("0"), false),
        )?;
        require_column_contract(
            legacy_connection,
            "symbols",
            "documentation",
            ("TEXT", false, None, false),
        )?;
        require_column_contract(
            legacy_connection,
            "usage_events",
            "created_at",
            ("TEXT", false, None, false),
        )?;
        let repaired_usage = legacy_connection.query_row(
            "SELECT accounting_layer, denominator_kind, dedupe_scope, created_at FROM usage_events WHERE session_id = 'legacy-session'",
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
        if repaired_usage.0 != "observed_delta"
            || repaired_usage.1 != "full_file"
            || repaired_usage.2 != "event"
            || repaired_usage.3.is_empty()
        {
            return Err(io::Error::other("legacy usage metadata was not repaired").into());
        }

        let future = tempfile::tempdir()?;
        let future_path = future.path().join("future.db");
        seed_legacy_schema(&future_path, SCHEMA_VERSION + 1)?;
        match crate::AtlasStore::open(&future_path) {
            Err(DbError::SchemaVersion { found, expected })
                if found == SCHEMA_VERSION + 1 && expected == SCHEMA_VERSION => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "future schema returned the wrong error: {error}"
                ))
                .into());
            }
            Ok(_) => return Err(io::Error::other("future schema version was accepted").into()),
        }
        Ok(())
    }

    #[test]
    fn task_arri_ut_arri_4_2() -> Result<(), Box<dyn Error>> {
        let initialized = tempfile::tempdir()?;
        let initialized_path = initialized.path().join("initialized.db");
        let first_identity = {
            let store = crate::AtlasStore::open(&initialized_path)?;
            let identity = store.project_instance_id()?;
            store.initialize_schema()?;
            if store.project_instance_id()? != identity {
                return Err(io::Error::other(
                    "repeated schema initialization replaced the project identity",
                )
                .into());
            }
            identity
        };
        let reopened = crate::AtlasStore::open(&initialized_path)?;
        if reopened.project_instance_id()? != first_identity {
            return Err(io::Error::other("database reopen replaced the project identity").into());
        }

        let independent_path = initialized.path().join("independent.db");
        let independent = crate::AtlasStore::open(&independent_path)?.project_instance_id()?;
        if independent == first_identity {
            return Err(io::Error::other(
                "independent database initialization reused a project identity",
            )
            .into());
        }

        let legacy = tempfile::tempdir()?;
        let legacy_path = legacy.path().join("legacy.db");
        seed_legacy_schema(&legacy_path, MIGRATION_BASE_SCHEMA_VERSION)?;
        {
            let connection = Connection::open(&legacy_path)?;
            connection.execute(
                "INSERT INTO metadata(key, value) VALUES('project_root', ?1)",
                ["C:/workspace/authored"],
            )?;
            connection.execute(
                "INSERT INTO metadata(key, value) VALUES('authored_note', ?1)",
                ["preserve-me"],
            )?;
        }
        let legacy_store = crate::AtlasStore::open(&legacy_path)?;
        let legacy_identity = legacy_store.project_instance_id()?;
        if legacy_store.project_root()?.as_deref() != Some("C:/workspace/authored") {
            return Err(io::Error::other("legacy upgrade changed project-root metadata").into());
        }
        let authored_note = legacy_store.connection.query_row(
            "SELECT value FROM metadata WHERE key = 'authored_note'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        if authored_note != "preserve-me" {
            return Err(io::Error::other("legacy upgrade changed authored metadata").into());
        }
        legacy_store.initialize_schema()?;
        if legacy_store.project_instance_id()? != legacy_identity {
            return Err(io::Error::other("legacy identity was not stable after upgrade").into());
        }

        let preserved = tempfile::tempdir()?;
        let preserved_path = preserved.path().join("preserved.db");
        seed_legacy_schema(&preserved_path, MIGRATION_BASE_SCHEMA_VERSION)?;
        let expected = ProjectInstanceId::try_from("00112233445566778899aabbccddeeff")?;
        Connection::open(&preserved_path)?.execute(
            "INSERT INTO metadata(key, value) VALUES(?1, ?2)",
            (PROJECT_INSTANCE_ID_METADATA_KEY, expected.to_string()),
        )?;
        let preserved_store = crate::AtlasStore::open(&preserved_path)?;
        if preserved_store.project_instance_id()? != expected {
            return Err(io::Error::other("legacy upgrade replaced a valid identity").into());
        }

        for (label, invalid) in [
            ("malformed", "not-an-instance-id"),
            ("all-zero", "00000000000000000000000000000000"),
        ] {
            let corrupt = tempfile::tempdir()?;
            let corrupt_path = corrupt.path().join(format!("{label}.db"));
            seed_legacy_schema(&corrupt_path, MIGRATION_BASE_SCHEMA_VERSION)?;
            Connection::open(&corrupt_path)?.execute(
                "INSERT INTO metadata(key, value) VALUES(?1, ?2)",
                (PROJECT_INSTANCE_ID_METADATA_KEY, invalid),
            )?;
            match crate::AtlasStore::open(&corrupt_path) {
                Err(DbError::InvalidProjectInstanceId { value, .. }) if value == invalid => {}
                Err(error) => {
                    return Err(io::Error::other(format!(
                        "{label} identity returned the wrong error: {error}"
                    ))
                    .into());
                }
                Ok(_) => {
                    return Err(io::Error::other(format!(
                        "{label} identity was silently accepted or replaced"
                    ))
                    .into());
                }
            }
            let stored_version = Connection::open(&corrupt_path)?.query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                [SCHEMA_VERSION_METADATA_KEY],
                |row| row.get::<_, String>(0),
            )?;
            if stored_version != MIGRATION_BASE_SCHEMA_VERSION.to_string() {
                return Err(io::Error::other(format!(
                    "{label} identity advanced the schema version before failing"
                ))
                .into());
            }
        }

        let missing = tempfile::tempdir()?;
        let missing_path = missing.path().join("missing.db");
        {
            let store = crate::AtlasStore::open(&missing_path)?;
            store.connection.execute(
                "DELETE FROM metadata WHERE key = ?1",
                [PROJECT_INSTANCE_ID_METADATA_KEY],
            )?;
        }
        match crate::AtlasStore::open(&missing_path) {
            Err(DbError::ProjectInstanceIdMissing) => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "current schema without identity returned the wrong error: {error}"
                ))
                .into());
            }
            Ok(_) => {
                return Err(io::Error::other(
                    "current schema silently replaced missing identity metadata",
                )
                .into());
            }
        }
        Ok(())
    }

    #[test]
    fn task_arri_ut_arri_4_6() -> Result<(), Box<dyn Error>> {
        let historical_table_columns = ["id", "created_at", "session_id"]
            .map(str::to_owned)
            .to_vec();
        let current_table_columns = ["id", "session_id", "created_at"]
            .map(str::to_owned)
            .to_vec();
        if !schema_object_columns_match(
            SchemaObjectKind::Table,
            &historical_table_columns,
            &current_table_columns,
        ) || schema_object_columns_match(
            SchemaObjectKind::Index,
            &historical_table_columns,
            &current_table_columns,
        ) {
            return Err(io::Error::other(
                "schema preflight did not distinguish table membership from index order",
            )
            .into());
        }

        let expected_relative_root = normalize_native_path_display(std::env::current_dir()?);
        let inferred_relative_root =
            inferred_project_root(Path::new(".projectatlas/projectatlas.db"));
        if inferred_relative_root.as_deref() != Some(expected_relative_root.as_str()) {
            return Err(io::Error::other(format!(
                "relative project-local database inferred {inferred_relative_root:?} instead of {expected_relative_root:?}"
            ))
            .into());
        }

        let fresh = tempfile::tempdir()?;
        let fresh_path = fresh.path().join("fresh.db");
        let fresh_report = inspect_schema_preflight(&fresh_path)?;
        fresh_report.ensure_write_ready()?;
        if fresh_report.source != SchemaSourceState::Fresh
            || fresh_report.user_version != 0
            || fresh_report.required_objects.is_empty()
            || !fresh_report
                .required_objects
                .iter()
                .any(|object| object.kind == SchemaObjectKind::Table)
            || !fresh_report
                .required_objects
                .iter()
                .any(|object| object.kind == SchemaObjectKind::Index)
            || fresh_report
                .required_objects
                .iter()
                .any(|object| object.status != RequiredSchemaObjectStatus::Missing)
            || fresh_report.integrity != IntegrityState::Passed
            || fresh_report.sidecars.wal.exists
            || fresh_report.sidecars.shm.exists
            || fresh_report.sidecars.rollback_journal.exists
            || fresh_report.root_binding.status != RootBindingStatus::Unbound
            || fresh_report.supported_source_range != (MIN_SUPPORTED_SCHEMA_VERSION, SCHEMA_VERSION)
            || !fresh_report.free_space.sufficient
            || fresh_report.backup.required
            || !fresh_report.backup.backup_ready
            || !fresh_report.backup.rollback_ready
            || fresh_report.migration_ledger != MigrationLedgerState::Absent
            || !fresh_report.conflicting_partial_objects.is_empty()
        {
            return Err(io::Error::other(format!(
                "fresh database preflight omitted or misclassified required state: {fresh_report:?}"
            ))
            .into());
        }

        let current = tempfile::tempdir()?;
        let project_root = current.path().join("repository");
        let atlas_directory = project_root.join(PROJECTATLAS_DIRECTORY_NAME);
        fs::create_dir_all(&atlas_directory)?;
        let current_path = atlas_directory.join("projectatlas.db");
        {
            let store = crate::AtlasStore::open(&current_path)?;
            store.set_project_root(&project_root)?;
            store.connection.pragma_update(None, "user_version", 41)?;
            let wal_report = inspect_schema_preflight(&current_path)?;
            wal_report.ensure_write_ready()?;
            if wal_report.sidecars.journal_mode != "wal"
                || !wal_report.sidecars.wal.exists
                || !wal_report.sidecars.shm.exists
                || wal_report.sidecars.rollback_journal.exists
            {
                return Err(io::Error::other(format!(
                    "WAL database preflight omitted or changed required sidecar state: {wal_report:?}"
                ))
                .into());
            }
            store
                .connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        }
        remove_sqlite_sidecars(&current_path)?;
        let current_report = inspect_schema_preflight(&current_path)?;
        current_report.ensure_write_ready()?;
        if current_report.source
            != (SchemaSourceState::Supported {
                version: SCHEMA_VERSION,
                migration_required: false,
            })
            || current_report.user_version != 41
            || current_report.required_objects.is_empty()
            || current_report
                .required_objects
                .iter()
                .any(|object| object.status != RequiredSchemaObjectStatus::Matches)
            || current_report.sqlite_features.version.is_empty()
            || current_report.sqlite_features.compile_options.is_empty()
            || current_report.integrity != IntegrityState::Passed
            || current_report.sidecars.wal.exists
            || current_report.sidecars.shm.exists
            || current_report.sidecars.rollback_journal.exists
            || current_report.root_binding.status != RootBindingStatus::Bound
            || current_report.root_binding.project_instance_id.is_none()
            || current_report.backup.required
            || !current_report.backup.backup_ready
            || !current_report.backup.rollback_ready
            || !matches!(
                current_report.migration_ledger,
                MigrationLedgerState::Compatible(ref records)
                    if records.len() == allocated_migrations().len()
            )
            || !current_report.conflicting_partial_objects.is_empty()
        {
            return Err(io::Error::other(format!(
                "current database preflight omitted or misclassified required state: {current_report:?}"
            ))
            .into());
        }

        let moved = tempfile::tempdir()?;
        let moved_root = moved.path().join("other-repository");
        let moved_atlas_directory = moved_root.join(PROJECTATLAS_DIRECTORY_NAME);
        fs::create_dir_all(&moved_atlas_directory)?;
        let moved_path = moved_atlas_directory.join("projectatlas.db");
        fs::copy(&current_path, &moved_path)?;
        let moved_bytes = fs::read(&moved_path)?;
        match crate::AtlasStore::open(&moved_path) {
            Err(DbError::SchemaPreflight { message }) if message.contains("root binding") => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "root mismatch returned the wrong preflight error: {error}"
                ))
                .into());
            }
            Ok(_) => return Err(io::Error::other("root mismatch passed schema preflight").into()),
        }
        require_database_unchanged(&moved_path, &moved_bytes)?;

        {
            let connection = Connection::open(&current_path)?;
            connection.execute_batch(
                "DROP INDEX idx_nodes_kind;
                 PRAGMA wal_checkpoint(TRUNCATE);",
            )?;
        }
        remove_sqlite_sidecars(&current_path)?;
        let missing_object_bytes = fs::read(&current_path)?;
        let missing_object_report = inspect_schema_preflight(&current_path)?;
        if !missing_object_report.required_objects.iter().any(|object| {
            object.name == "idx_nodes_kind" && object.status == RequiredSchemaObjectStatus::Missing
        }) || missing_object_report.conflicting_partial_objects.is_empty()
        {
            return Err(io::Error::other(format!(
                "current schema with a missing index was not retained as a conflict: {missing_object_report:?}"
            ))
            .into());
        }
        match preflight(&current_path) {
            Err(DbError::SchemaPreflight { message })
                if message.contains("missing") || message.contains("conflicting") => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "missing current object returned the wrong preflight error: {error}"
                ))
                .into());
            }
            Ok(_) => {
                return Err(
                    io::Error::other("missing current object passed schema preflight").into(),
                );
            }
        }
        require_database_unchanged(&current_path, &missing_object_bytes)?;

        let supported = tempfile::tempdir()?;
        let supported_path = supported.path().join("supported.db");
        seed_legacy_schema(&supported_path, SCHEMA_VERSION - 1)?;
        let supported_report = inspect_schema_preflight(&supported_path)?;
        supported_report.ensure_write_ready()?;
        if supported_report.source
            != (SchemaSourceState::Supported {
                version: SCHEMA_VERSION - 1,
                migration_required: true,
            })
            || !supported_report.required_objects.iter().any(|object| {
                matches!(
                    object.status,
                    RequiredSchemaObjectStatus::Missing
                        | RequiredSchemaObjectStatus::LegacyPartial { .. }
                )
            })
            || supported_report.required_objects.iter().any(|object| {
                matches!(
                    &object.status,
                    RequiredSchemaObjectStatus::LegacyPartial { missing_columns }
                        if missing_columns.is_empty()
                )
            })
            || !supported_report.backup.required
            || !supported_report.backup.backup_ready
            || !supported_report.backup.rollback_ready
            || supported_report.free_space.required_bytes == 0
            || !supported_report.free_space.sufficient
            || supported_report.integrity != IntegrityState::Passed
            || supported_report.migration_ledger != MigrationLedgerState::Absent
        {
            return Err(io::Error::other(format!(
                "supported source preflight did not record migration readiness: {supported_report:?}"
            ))
            .into());
        }

        let bound_legacy = tempfile::tempdir()?;
        let bound_legacy_root = bound_legacy.path().join("repository");
        let bound_legacy_atlas = bound_legacy_root.join(PROJECTATLAS_DIRECTORY_NAME);
        fs::create_dir_all(&bound_legacy_atlas)?;
        let bound_legacy_path = bound_legacy_atlas.join("projectatlas.db");
        seed_legacy_schema(&bound_legacy_path, MIGRATION_BASE_SCHEMA_VERSION - 1)?;
        Connection::open(&bound_legacy_path)?.execute(
            "INSERT INTO metadata(key, value) VALUES('project_root', ?1)",
            [normalize_native_path_display(&bound_legacy_root)],
        )?;
        remove_sqlite_sidecars(&bound_legacy_path)?;
        let bound_legacy_bytes = fs::read(&bound_legacy_path)?;
        let bound_legacy_report = inspect_schema_preflight(&bound_legacy_path)?;
        bound_legacy_report.ensure_write_ready()?;
        if bound_legacy_report.root_binding.status != RootBindingStatus::Bound
            || bound_legacy_report
                .root_binding
                .project_instance_id
                .is_some()
            || !bound_legacy_report.source.migration_required()
        {
            return Err(io::Error::other(format!(
                "bound legacy database preflight did not preserve migration eligibility: {bound_legacy_report:?}"
            ))
            .into());
        }
        require_database_unchanged(&bound_legacy_path, &bound_legacy_bytes)?;

        Connection::open(&supported_path)?
            .execute_batch("CREATE TABLE summaries_new(id INTEGER PRIMARY KEY);")?;
        let partial_object_bytes = fs::read(&supported_path)?;
        let partial_object_report = inspect_schema_preflight(&supported_path)?;
        if !partial_object_report
            .conflicting_partial_objects
            .iter()
            .any(|conflict| conflict.contains("summaries_new"))
        {
            return Err(io::Error::other(format!(
                "leftover partial object was not retained in the report: {partial_object_report:?}"
            ))
            .into());
        }
        match preflight(&supported_path) {
            Err(DbError::SchemaPreflight { message }) if message.contains("summaries_new") => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "leftover partial object returned the wrong preflight error: {error}"
                ))
                .into());
            }
            Ok(_) => {
                return Err(
                    io::Error::other("leftover partial object passed schema preflight").into(),
                );
            }
        }
        require_database_unchanged(&supported_path, &partial_object_bytes)?;

        let future = tempfile::tempdir()?;
        let future_path = future.path().join("future.db");
        seed_legacy_schema(&future_path, SCHEMA_VERSION + 1)?;
        Connection::open(&future_path)?.pragma_update(None, "user_version", 73)?;
        let future_bytes = fs::read(&future_path)?;
        match crate::AtlasStore::open(&future_path) {
            Err(DbError::SchemaVersion { found, expected })
                if found == SCHEMA_VERSION + 1 && expected == SCHEMA_VERSION => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "future source returned the wrong preflight error: {error}"
                ))
                .into());
            }
            Ok(_) => return Err(io::Error::other("future source passed schema preflight").into()),
        }
        require_database_unchanged(&future_path, &future_bytes)?;

        let conflict = tempfile::tempdir()?;
        let conflict_path = conflict.path().join("conflict.db");
        Connection::open(&conflict_path)?.execute_batch(
            "CREATE VIEW metadata AS SELECT 'schema_version' AS key, '9' AS value;",
        )?;
        let conflict_bytes = fs::read(&conflict_path)?;
        match crate::AtlasStore::open(&conflict_path) {
            Err(DbError::SchemaPreflight { message }) if message.contains("metadata") => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "conflicting partial object returned the wrong preflight error: {error}"
                ))
                .into());
            }
            Ok(_) => {
                return Err(
                    io::Error::other("conflicting partial object passed schema preflight").into(),
                );
            }
        }
        require_database_unchanged(&conflict_path, &conflict_bytes)?;
        Ok(())
    }

    #[test]
    fn task_arri_ut_arri_4_7() -> Result<(), Box<dyn Error>> {
        fn reject_applied_migration(_: &Connection) -> DbResult<()> {
            Err(preflight_error("injected migration verification failure"))
        }

        let migrations = allocated_migrations();
        let Some(migration) = migrations.first() else {
            return Err(io::Error::other(
                "ARRI 4.7 must remain the first accepted ledger migration",
            )
            .into());
        };
        let migration_count = i64::try_from(migrations.len())?;
        let definition = migration.definition;
        if SCHEMA_VERSION != MIGRATION_BASE_SCHEMA_VERSION + migration_count
            || migration.schema_version != MIGRATION_BASE_SCHEMA_VERSION + 1
            || definition.id != "install-runtime-migration-ledger"
            || definition.owner.is_empty()
            || definition.prerequisites.is_empty()
            || definition.authored_effects.is_empty()
            || definition.derived_effects.is_empty()
            || definition.transaction_boundary != "single-sqlite-transaction"
            || definition.forward_behavior.is_empty()
            || definition.rollback_behavior.is_empty()
            || !definition.evidence.contains("task_arri_ut_arri_4_7")
            || migration.checksum
                != format!(
                    "blake3:{}",
                    blake3::hash(migration.canonical_payload().as_bytes()).to_hex()
                )
        {
            return Err(io::Error::other(format!(
                "accepted migration contract is incomplete or not inventory allocated: {migration:?}"
            ))
            .into());
        }

        let fresh = tempfile::tempdir()?;
        let fresh_path = fresh.path().join("fresh.db");
        let fresh_plan = preflight(&fresh_path)?;
        if fresh_plan.source_version != 0
            || fresh_plan.target_version != SCHEMA_VERSION
            || fresh_plan.pending.len() != migrations.len()
        {
            return Err(io::Error::other(format!(
                "fresh preflight did not allocate the accepted migration inventory: {fresh_plan:?}"
            ))
            .into());
        }
        let fresh_store = crate::AtlasStore::open(&fresh_path)?;
        verify_current_schema(&fresh_store.connection)?;
        if table_columns(&fresh_store.connection, MIGRATION_LEDGER_TABLE)?
            != ["schema_version", "migration_id", "checksum"]
        {
            return Err(io::Error::other("fresh ledger persisted the wrong contract").into());
        }
        let fresh_record = fresh_store.connection.query_row(
            "SELECT schema_version, migration_id, checksum FROM schema_migrations",
            [],
            |row| {
                Ok(MigrationLedgerRecord {
                    schema_version: row.get(0)?,
                    migration_id: row.get(1)?,
                    checksum: row.get(2)?,
                })
            },
        )?;
        if fresh_record
            != (MigrationLedgerRecord {
                schema_version: migration.schema_version,
                migration_id: definition.id.to_owned(),
                checksum: migration.checksum.clone(),
            })
        {
            return Err(io::Error::other(format!(
                "fresh ledger row does not bind accepted history: {fresh_record:?}"
            ))
            .into());
        }
        drop(fresh_store);
        let reopened = crate::AtlasStore::open(&fresh_path)?;
        let reopened_rows =
            reopened
                .connection
                .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                    row.get::<_, i64>(0)
                })?;
        if reopened_rows != migration_count || !preflight(&fresh_path)?.pending.is_empty() {
            return Err(io::Error::other(
                "reopening a current database reran or replanned accepted migrations",
            )
            .into());
        }

        let legacy = tempfile::tempdir()?;
        let legacy_path = legacy.path().join("legacy.db");
        seed_legacy_schema(&legacy_path, MIGRATION_BASE_SCHEMA_VERSION)?;
        {
            let connection = Connection::open(&legacy_path)?;
            connection.execute(
                "INSERT INTO metadata(key, value) VALUES('authored_note', 'preserve-me')",
                [],
            )?;
            connection.execute(
                "INSERT INTO symbols(path, language, name, kind, signature, line_start, line_end, parser)
                 VALUES('src/lib.rs', 'rust', 'preserved_symbol', 'function', 'fn preserved_symbol()', 1, 1, 'tree_sitter')",
                [],
            )?;
        }
        let legacy_plan = preflight(&legacy_path)?;
        if legacy_plan.source_version != MIGRATION_BASE_SCHEMA_VERSION
            || legacy_plan.pending.len() != migrations.len()
        {
            return Err(io::Error::other(format!(
                "legacy preflight did not allocate pending history: {legacy_plan:?}"
            ))
            .into());
        }
        let legacy_store = crate::AtlasStore::open(&legacy_path)?;
        let preserved = legacy_store.connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'authored_note'),
                (SELECT COUNT(*) FROM usage_events WHERE session_id = 'legacy-session'),
                (SELECT COUNT(*) FROM symbols WHERE name = 'preserved_symbol')",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        if preserved != ("preserve-me".to_owned(), 1, 1) {
            return Err(io::Error::other(format!(
                "ledger migration changed authored or derived source rows: {preserved:?}"
            ))
            .into());
        }
        verify_current_schema(&legacy_store.connection)?;

        let raced = tempfile::tempdir()?;
        let raced_path = raced.path().join("raced.db");
        seed_legacy_schema(&raced_path, MIGRATION_BASE_SCHEMA_VERSION)?;
        let raced_plan = preflight(&raced_path)?;
        Connection::open(&raced_path)?.execute(
            "UPDATE metadata SET value = ?1 WHERE key = ?2",
            (
                (MIGRATION_BASE_SCHEMA_VERSION - 1).to_string(),
                SCHEMA_VERSION_METADATA_KEY,
            ),
        )?;
        let mut raced_connection = Connection::open(&raced_path)?;
        match apply_migration_plan(&mut raced_connection, &raced_plan) {
            Err(DbError::SchemaPreflight { message })
                if message.contains("changed after preflight") => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "source-version race returned the wrong error: {error}"
                ))
                .into());
            }
            Ok(()) => {
                return Err(io::Error::other("source-version race applied a migration").into());
            }
        }
        if sqlite_objects(&raced_connection)?.contains_key(MIGRATION_LEDGER_TABLE)
            || observed_schema_version(&raced_connection)? != MIGRATION_BASE_SCHEMA_VERSION - 1
        {
            return Err(io::Error::other("source-version race wrote migration state").into());
        }

        let mut rollback_connection = Connection::open_in_memory()?;
        let mut rollback_plan = SchemaMigrationPlan::fresh();
        rollback_plan.pending[0].definition.verify = reject_applied_migration;
        match apply_migration_plan(&mut rollback_connection, &rollback_plan) {
            Err(DbError::SchemaPreflight { message })
                if message.contains("injected migration verification failure") => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "failed migration returned the wrong error: {error}"
                ))
                .into());
            }
            Ok(()) => return Err(io::Error::other("failed migration committed").into()),
        }
        if !sqlite_objects(&rollback_connection)?.is_empty() {
            return Err(io::Error::other(
                "failed migration did not roll back schema and ledger writes",
            )
            .into());
        }

        let tampered = tempfile::tempdir()?;
        let tampered_path = tampered.path().join("tampered.db");
        {
            let store = crate::AtlasStore::open(&tampered_path)?;
            if !store.connection.is_autocommit() {
                return Err(io::Error::other(
                    "current schema verification left an open transaction",
                )
                .into());
            }
        }
        {
            let checkpoint = Connection::open(&tampered_path)?;
            checkpoint
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .map_err(|error| {
                    io::Error::other(format!("tampered checkpoint failed: {error}"))
                })?;
        }
        remove_sqlite_sidecars(&tampered_path)?;
        Connection::open(&tampered_path)?.execute(
            "UPDATE schema_migrations SET checksum = 'blake3:tampered'",
            [],
        )?;
        let tampered_bytes = fs::read(&tampered_path)?;
        match crate::AtlasStore::open(&tampered_path) {
            Err(DbError::SchemaPreflight { message })
                if message.contains("migration ledger is incompatible") => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "tampered ledger returned the wrong error: {error}"
                ))
                .into());
            }
            Ok(_) => return Err(io::Error::other("tampered ledger was accepted").into()),
        }
        require_database_unchanged(&tampered_path, &tampered_bytes)?;
        Ok(())
    }

    #[test]
    fn task_arri_ut_arri_4_8() -> Result<(), Box<dyn Error>> {
        fn reject_file_text_fts(_: &Connection) -> DbResult<()> {
            Err(preflight_error(
                "injected file-text FTS verification failure",
            ))
        }

        let migrations = allocated_migrations();
        let Some((ledger_migration, fts_migration)) = migrations.first().zip(migrations.get(1))
        else {
            return Err(io::Error::other(
                "ARRI 4.8 requires the ledger and file-text FTS migrations",
            )
            .into());
        };
        if fts_migration.schema_version != ledger_migration.schema_version + 1
            || fts_migration.definition.id != "install-file-text-trigram-index"
            || fts_migration.definition.transaction_boundary != "single-sqlite-transaction"
            || !fts_migration
                .definition
                .forward_behavior
                .contains("backfill")
            || !fts_migration
                .definition
                .rollback_behavior
                .contains("retain-authoritative-file-texts")
            || !fts_migration
                .definition
                .evidence
                .contains("task_arri_ut_arri_4_8")
        {
            return Err(io::Error::other(format!(
                "file-text FTS migration contract is incomplete: {fts_migration:?}"
            ))
            .into());
        }

        let prefix = tempfile::tempdir()?;
        let prefix_root = prefix.path().join("repository");
        let prefix_atlas = prefix_root.join(PROJECTATLAS_DIRECTORY_NAME);
        fs::create_dir_all(&prefix_atlas)?;
        let prefix_path = prefix_atlas.join("projectatlas.db");
        {
            let mut prefix_connection = Connection::open(&prefix_path)?;
            initialize_ledger_schema(&mut prefix_connection, ledger_migration)?;
            prefix_connection.execute(
                "INSERT INTO metadata(key, value) VALUES('project_root', ?1)",
                [normalize_native_path_display(&prefix_root)],
            )?;
        }
        let prefix_plan = preflight(&prefix_path)?;
        if prefix_plan.source_version != ledger_migration.schema_version
            || prefix_plan.pending.len() != migrations.len() - 1
            || prefix_plan.pending[0].definition.id != fts_migration.definition.id
        {
            return Err(io::Error::other(format!(
                "valid migration-ledger prefix did not plan the FTS migration: {prefix_plan:?}"
            ))
            .into());
        }
        let prefix_store = crate::AtlasStore::open(&prefix_path)?;
        verify_current_schema(&prefix_store.connection)?;

        let mut connection = Connection::open_in_memory()?;
        initialize_ledger_schema(&mut connection, ledger_migration)?;
        connection.execute(
            "INSERT INTO file_texts(path, content_hash, byte_count, line_count, content)
             VALUES('src/lib.rs', 'hash-before', 17, 1, 'pub fn before() {}')",
            [],
        )?;

        let plan = SchemaMigrationPlan {
            source_version: ledger_migration.schema_version,
            target_version: SCHEMA_VERSION,
            pending: migrations[1..].to_vec(),
        };
        apply_migration_plan(&mut connection, &plan)?;
        verify_optional_file_text_fts(&connection)?;
        let fts_active = sqlite_objects(&connection)?.contains_key(FILE_TEXT_FTS_TABLE);
        if fts_active {
            let backfilled = connection.query_row(
                "SELECT content FROM file_text_fts WHERE path = 'src/lib.rs'",
                [],
                |row| row.get::<_, String>(0),
            )?;
            if backfilled != "pub fn before() {}" {
                return Err(
                    io::Error::other("file-text FTS backfill changed source content").into(),
                );
            }

            connection.execute(
                "UPDATE file_texts
                 SET path = 'src/main.rs', content_hash = 'hash-after', byte_count = 16,
                     content = 'pub fn after() {}'
                 WHERE path = 'src/lib.rs'",
                [],
            )?;
            let synchronized =
                connection.query_row("SELECT path, content FROM file_text_fts", [], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;
            if synchronized != ("src/main.rs".to_owned(), "pub fn after() {}".to_owned()) {
                return Err(io::Error::other(format!(
                    "file-text FTS update trigger drifted: {synchronized:?}"
                ))
                .into());
            }
            connection.execute("DELETE FROM file_texts WHERE path = 'src/main.rs'", [])?;
            verify_optional_file_text_fts(&connection)?;
        } else {
            connection.execute("DELETE FROM file_texts WHERE path = 'src/lib.rs'", [])?;
        }

        connection.execute(
            "INSERT INTO file_texts(path, content_hash, byte_count, line_count, content)
             VALUES('src/fallback.rs', 'hash-fallback', 24, 1, 'punctuation::still_exact')",
            [],
        )?;
        if fts_active {
            let mirrored = connection.query_row(
                "SELECT COUNT(*) FROM file_text_fts WHERE path = 'src/fallback.rs'",
                [],
                |row| row.get::<_, i64>(0),
            )?;
            if mirrored != 1 {
                return Err(io::Error::other("file-text FTS insert trigger drifted").into());
            }
            connection.execute("DELETE FROM file_text_fts", [])?;
        }
        let fallback_store = crate::AtlasStore { connection };
        let exact_fallback = fallback_store.load_file_texts_for_search(Some("::"), true)?;
        if exact_fallback.len() != 1
            || exact_fallback[0].path != "src/fallback.rs"
            || exact_fallback[0].content != "punctuation::still_exact"
        {
            return Err(io::Error::other("exact search depended on FTS candidates").into());
        }

        let mut rollback_connection = Connection::open_in_memory()?;
        initialize_ledger_schema(&mut rollback_connection, ledger_migration)?;
        rollback_connection.execute(
            "INSERT INTO file_texts(path, content_hash, byte_count, line_count, content)
             VALUES('src/rollback.rs', 'hash-rollback', 18, 1, 'rollback survives')",
            [],
        )?;
        let mut rollback_plan = plan;
        rollback_plan.pending[0].definition.verify = reject_file_text_fts;
        match apply_migration_plan(&mut rollback_connection, &rollback_plan) {
            Err(DbError::SchemaPreflight { message })
                if message.contains("injected file-text FTS verification failure") => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "failed FTS migration returned the wrong error: {error}"
                ))
                .into());
            }
            Ok(()) => return Err(io::Error::other("failed FTS migration committed").into()),
        }
        let rollback_state = rollback_connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'schema_version'),
                (SELECT COUNT(*) FROM schema_migrations),
                (SELECT content FROM file_texts WHERE path = 'src/rollback.rs')",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )?;
        if rollback_state
            != (
                ledger_migration.schema_version.to_string(),
                1,
                "rollback survives".to_owned(),
            )
            || sqlite_objects(&rollback_connection)?.contains_key(FILE_TEXT_FTS_TABLE)
        {
            return Err(io::Error::other(format!(
                "failed FTS migration changed the source schema or data: {rollback_state:?}"
            ))
            .into());
        }
        Ok(())
    }

    #[test]
    fn task_arri_ut_arri_4_9() -> Result<(), Box<dyn Error>> {
        type CompatibilitySnapshot = (
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
        );

        fn reject_typed_graph(_: &Connection) -> DbResult<()> {
            Err(preflight_error(
                "injected typed repository graph verification failure",
            ))
        }

        fn seed_compatibility_rows(connection: &Connection) -> DbResult<()> {
            connection.execute_batch(
                "
                INSERT INTO metadata(key, value)
                VALUES('authored_note', 'preserve-metadata');

                INSERT INTO nodes(
                    id, path, kind, parent_path, extension, language, size_bytes,
                    mtime_ns, content_hash, exists_now
                ) VALUES(
                    41, 'src/lib.rs', 'file', 'src', 'rs', 'rust', 17,
                    99, 'node-hash', 1
                );

                INSERT INTO purposes(node_id, purpose, source, status, updated_by)
                VALUES(41, 'Preserve authored purpose', 'agent', 'approved', 'codex');

                INSERT INTO summaries(id, node_id, summary_level, subject, summary)
                VALUES(42, 41, 'file', 'src/lib.rs', 'Preserve summary');

                INSERT INTO usage_events(
                    id, session_id, command, path, query,
                    estimated_tokens_without_projectatlas,
                    estimated_tokens_with_projectatlas,
                    estimated_tokens_saved
                ) VALUES(
                    43, 'preserve-session', 'summary', 'src/lib.rs', 'needle',
                    100, 20, 80
                );

                INSERT INTO symbols(
                    id, path, language, name, kind, signature, exported,
                    documentation, line_start, line_end, parent, parser, detail
                ) VALUES(
                    44, 'src/lib.rs', 'rust', 'preserved_symbol', 'function',
                    'fn preserved_symbol()', 1, 'Preserve docs', 1, 1,
                    'crate', 'tree_sitter', 'Preserve detail'
                );

                INSERT INTO source_parse_metadata(
                    path, language, parser, symbol_count, relation_count
                ) VALUES('src/lib.rs', 'rust', 'tree_sitter', 1, 1);

                INSERT INTO symbol_relations(
                    id, path, source_name, target_name, kind, line, context, parser
                ) VALUES(
                    45, 'src/lib.rs', 'preserved_symbol', 'target_symbol',
                    'calls', 1, 'preserve context', 'tree_sitter'
                );

                INSERT INTO health_resolutions(
                    finding_id, category, path, related_path, rationale, resolved_by
                ) VALUES(
                    'preserve-finding', 'duplicate-purpose', 'src/lib.rs',
                    'src/main.rs', 'Preserve resolution', 'agent'
                );

                INSERT INTO file_texts(
                    path, content_hash, byte_count, line_count, content
                ) VALUES(
                    'src/lib.rs', 'text-hash', 17, 1, 'pub fn before() {}'
                );
                ",
            )?;
            Ok(())
        }

        fn compatibility_snapshot(connection: &Connection) -> DbResult<CompatibilitySnapshot> {
            connection
                .query_row(
                    "
                    SELECT
                        (SELECT key || '=' || value
                         FROM metadata WHERE key = 'authored_note'),
                        (SELECT path || '|' || kind || '|' || content_hash
                         FROM nodes WHERE id = 41),
                        (SELECT purpose || '|' || source || '|' || status || '|' || updated_by
                         FROM purposes WHERE node_id = 41),
                        (SELECT summary_level || '|' || subject || '|' || summary
                         FROM summaries WHERE id = 42),
                        (SELECT session_id || '|' || command || '|' || path || '|' || query
                         FROM usage_events WHERE id = 43),
                        (SELECT path || '|' || name || '|' || signature || '|' || parser
                         FROM symbols WHERE id = 44),
                        (SELECT path || '|' || parser || '|' || symbol_count || '|' || relation_count
                         FROM source_parse_metadata WHERE path = 'src/lib.rs'),
                        (SELECT source_name || '|' || target_name || '|' || kind || '|' || context
                         FROM symbol_relations WHERE id = 45),
                        (SELECT finding_id || '|' || rationale || '|' || resolved_by
                         FROM health_resolutions WHERE finding_id = 'preserve-finding'),
                        (SELECT path || '|' || content_hash || '|' || content
                         FROM file_texts WHERE path = 'src/lib.rs')
                    ",
                    [],
                    |row| {
                        Ok((
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
                        ))
                    },
                )
                .map_err(DbError::from)
        }

        let migrations = allocated_migrations();
        let [ledger_migration, fts_migration, typed_graph_migration, ..] = migrations.as_slice()
        else {
            return Err(io::Error::other(
                "ARRI 4.9 requires the ledger, FTS, and typed graph migrations",
            )
            .into());
        };
        if typed_graph_migration.schema_version != fts_migration.schema_version + 1
            || typed_graph_migration.definition.id != "install-typed-repository-graph"
            || typed_graph_migration.definition.owner != "projectatlas-db::schema::repository-graph"
            || typed_graph_migration.definition.transaction_boundary != "single-sqlite-transaction"
            || !typed_graph_migration
                .definition
                .authored_effects
                .contains("preserve")
            || !typed_graph_migration
                .definition
                .derived_effects
                .contains("source-parse-metadata")
            || !typed_graph_migration
                .definition
                .rollback_behavior
                .contains("retain-authored-and-compatibility-data")
            || !typed_graph_migration
                .definition
                .evidence
                .contains("task_arri_ut_arri_4_9")
        {
            return Err(io::Error::other(format!(
                "typed repository graph migration contract is incomplete: {typed_graph_migration:?}"
            ))
            .into());
        }

        let prefix = tempfile::tempdir()?;
        let prefix_root = prefix.path().join("repository");
        let prefix_atlas = prefix_root.join(PROJECTATLAS_DIRECTORY_NAME);
        fs::create_dir_all(&prefix_atlas)?;
        let prefix_path = prefix_atlas.join("projectatlas.db");
        let before = {
            let mut connection = Connection::open(&prefix_path)?;
            initialize_migration_prefix(&mut connection, &migrations[..2])?;
            connection.execute(
                "INSERT INTO metadata(key, value) VALUES('project_root', ?1)",
                [normalize_native_path_display(&prefix_root)],
            )?;
            seed_compatibility_rows(&connection)?;
            compatibility_snapshot(&connection)?
        };

        let prefix_plan = preflight(&prefix_path)?;
        if prefix_plan.source_version != fts_migration.schema_version
            || prefix_plan.pending.len() != migrations.len() - 2
            || prefix_plan.pending[0].definition.id != typed_graph_migration.definition.id
        {
            return Err(io::Error::other(format!(
                "valid FTS migration prefix did not plan the typed graph migration: {prefix_plan:?}"
            ))
            .into());
        }

        let migrated = crate::AtlasStore::open(&prefix_path)?;
        let connection = &migrated.connection;
        verify_typed_repository_graph_schema(connection)?;
        verify_current_schema(connection)?;
        if compatibility_snapshot(connection)? != before {
            return Err(io::Error::other(
                "typed graph migration changed authored or compatibility rows",
            )
            .into());
        }
        let migrated_state = connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'schema_version'),
                (SELECT COUNT(*) FROM schema_migrations),
                (SELECT COUNT(*) FROM graph_entities)
                    + (SELECT COUNT(*) FROM graph_relations)
                    + (SELECT COUNT(*) FROM graph_evidence_occurrences)
                    + (SELECT COUNT(*) FROM graph_resolution_occurrences)
                    + (SELECT COUNT(*) FROM graph_resolution_candidates)
                    + (SELECT COUNT(*) FROM graph_coverage)",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        if migrated_state
            != (
                SCHEMA_VERSION.to_string(),
                i64::try_from(migrations.len())?,
                0,
            )
        {
            return Err(io::Error::other(format!(
                "typed graph migration fabricated rows or failed to advance history: {migrated_state:?}"
            ))
            .into());
        }
        require_column_contract(
            connection,
            GRAPH_ENTITIES_TABLE,
            "stable_key_digest",
            ("BLOB", true, None, true),
        )?;
        require_column_contract(
            connection,
            GRAPH_RELATIONS_TABLE,
            "resolution_status",
            ("TEXT", true, Some("'resolved'"), false),
        )?;
        require_column_contract(
            connection,
            GRAPH_EVIDENCE_OCCURRENCES_TABLE,
            "content_span_fingerprint",
            ("BLOB", true, None, false),
        )?;
        require_column_contract(
            connection,
            GRAPH_RESOLUTION_OCCURRENCES_TABLE,
            "candidate_total",
            ("INTEGER", false, None, false),
        )?;
        require_column_contract(
            connection,
            GRAPH_RESOLUTION_CANDIDATES_TABLE,
            "candidate_ordinal",
            ("INTEGER", true, None, true),
        )?;
        require_column_contract(
            connection,
            GRAPH_COVERAGE_TABLE,
            "structural_slot",
            ("TEXT", true, None, true),
        )?;

        let mut rollback_connection = Connection::open_in_memory()?;
        initialize_migration_prefix(&mut rollback_connection, &migrations[..2])?;
        seed_compatibility_rows(&rollback_connection)?;
        let rollback_before = compatibility_snapshot(&rollback_connection)?;
        let mut rollback_plan = SchemaMigrationPlan {
            source_version: fts_migration.schema_version,
            target_version: SCHEMA_VERSION,
            pending: migrations[2..].to_vec(),
        };
        rollback_plan.pending[0].definition.verify = reject_typed_graph;
        match apply_migration_plan(&mut rollback_connection, &rollback_plan) {
            Err(DbError::SchemaPreflight { message })
                if message.contains("injected typed repository graph verification failure") => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "failed typed graph migration returned the wrong error: {error}"
                ))
                .into());
            }
            Ok(()) => return Err(io::Error::other("failed typed graph migration committed").into()),
        }
        if compatibility_snapshot(&rollback_connection)? != rollback_before {
            return Err(io::Error::other(
                "failed typed graph migration changed authored or compatibility rows",
            )
            .into());
        }
        let rollback_state = rollback_connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'schema_version'),
                (SELECT COUNT(*) FROM schema_migrations)",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?;
        let rollback_objects = sqlite_objects(&rollback_connection)?;
        if rollback_state != (fts_migration.schema_version.to_string(), 2)
            || [
                GRAPH_ENTITIES_TABLE,
                GRAPH_RELATIONS_TABLE,
                GRAPH_EVIDENCE_OCCURRENCES_TABLE,
                GRAPH_RESOLUTION_OCCURRENCES_TABLE,
                GRAPH_RESOLUTION_CANDIDATES_TABLE,
                GRAPH_COVERAGE_TABLE,
            ]
            .iter()
            .any(|table| rollback_objects.contains_key(*table))
        {
            return Err(io::Error::other(format!(
                "failed typed graph migration changed the source schema: {rollback_state:?}"
            ))
            .into());
        }
        if ledger_migration.schema_version + 1 != fts_migration.schema_version {
            return Err(io::Error::other("migration prefix order changed").into());
        }
        Ok(())
    }

    #[test]
    fn task_arri_ut_arri_4_10() -> Result<(), Box<dyn Error>> {
        type PreservationSnapshot = (String, String, String, String, String);

        fn reject_structural_publication(_: &Connection) -> DbResult<()> {
            Err(preflight_error(
                "injected structural publication verification failure",
            ))
        }

        fn seed_preserved_rows(connection: &Connection) -> DbResult<()> {
            connection.execute_batch(
                "
                INSERT INTO metadata(key, value)
                VALUES('authored_note', 'preserve-publication-migration');

                INSERT INTO nodes(
                    id, path, kind, extension, language, size_bytes,
                    mtime_ns, content_hash, exists_now
                ) VALUES(
                    51, 'src/publication.rs', 'file', 'rs', 'rust', 19,
                    101, 'publication-node-hash', 1
                );

                INSERT INTO purposes(node_id, purpose, source, status, updated_by)
                VALUES(
                    51, 'Preserve publication migration purpose',
                    'agent', 'approved', 'codex'
                );

                INSERT INTO file_texts(
                    path, content_hash, byte_count, line_count, content
                ) VALUES(
                    'src/publication.rs', 'publication-text-hash', 19, 1,
                    'pub fn retained() {}'
                );

                INSERT INTO graph_coverage(
                    scope_kind, coverage_state, produced_count, omitted_count,
                    structural_slot, last_changed_epoch
                ) VALUES('repository', 'complete', 0, 0, 'b', 7);
                ",
            )?;
            Ok(())
        }

        fn preservation_snapshot(connection: &Connection) -> DbResult<PreservationSnapshot> {
            connection
                .query_row(
                    "
                    SELECT
                        (SELECT key || '=' || value
                         FROM metadata WHERE key = 'authored_note'),
                        (SELECT path || '|' || kind || '|' || content_hash
                         FROM nodes WHERE id = 51),
                        (SELECT purpose || '|' || source || '|' || status || '|' || updated_by
                         FROM purposes WHERE node_id = 51),
                        (SELECT path || '|' || content_hash || '|' || content
                         FROM file_texts WHERE path = 'src/publication.rs'),
                        (SELECT scope_kind || '|' || coverage_state || '|' || structural_slot
                                || '|' || last_changed_epoch
                         FROM graph_coverage WHERE scope_kind = 'repository')
                    ",
                    [],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    },
                )
                .map_err(DbError::from)
        }

        let migrations = allocated_migrations();
        let [_, _, typed_graph_migration, publication_migration, ..] = migrations.as_slice() else {
            return Err(io::Error::other(
                "ARRI 4.10 requires the typed graph and structural publication migrations",
            )
            .into());
        };
        if publication_migration.schema_version != typed_graph_migration.schema_version + 1
            || publication_migration.definition.id != "install-structural-publication-state"
            || publication_migration.definition.owner
                != "projectatlas-db::schema::structural-publication"
            || publication_migration.definition.transaction_boundary != "single-sqlite-transaction"
            || !publication_migration
                .definition
                .derived_effects
                .contains("exactly-two-structural-slots")
            || !publication_migration
                .definition
                .forward_behavior
                .contains("active-epoch-0")
            || !publication_migration
                .definition
                .rollback_behavior
                .contains("retain-authored-and-derived-data")
            || !publication_migration
                .definition
                .evidence
                .contains("task_arri_ut_arri_4_10")
        {
            return Err(io::Error::other(format!(
                "structural publication migration contract is incomplete: {publication_migration:?}"
            ))
            .into());
        }

        let prefix = tempfile::tempdir()?;
        let prefix_root = prefix.path().join("repository");
        let prefix_atlas = prefix_root.join(PROJECTATLAS_DIRECTORY_NAME);
        fs::create_dir_all(&prefix_atlas)?;
        let prefix_path = prefix_atlas.join("projectatlas.db");
        let before = {
            let mut connection = Connection::open(&prefix_path)?;
            initialize_migration_prefix(&mut connection, &migrations[..3])?;
            connection.execute(
                "INSERT INTO metadata(key, value) VALUES('project_root', ?1)",
                [normalize_native_path_display(&prefix_root)],
            )?;
            seed_preserved_rows(&connection)?;
            preservation_snapshot(&connection)?
        };

        let prefix_plan = preflight(&prefix_path)?;
        if prefix_plan.source_version != typed_graph_migration.schema_version
            || prefix_plan.pending.len() != migrations.len() - 3
            || prefix_plan.pending[0].definition.id != publication_migration.definition.id
        {
            return Err(io::Error::other(format!(
                "valid typed graph prefix did not plan structural publication: {prefix_plan:?}"
            ))
            .into());
        }

        let migrated = crate::AtlasStore::open(&prefix_path)?;
        let connection = &migrated.connection;
        verify_structural_publication_schema(connection)?;
        verify_current_schema(connection)?;
        if preservation_snapshot(connection)? != before {
            return Err(io::Error::other(
                "structural publication migration changed authored or derived rows",
            )
            .into());
        }
        let slots = connection
            .prepare("SELECT slot FROM graph_structural_slots ORDER BY slot")?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let publication = connection.query_row(
            "SELECT singleton, active_slot, active_epoch FROM graph_publication_state",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        let history = connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'schema_version'),
                (SELECT COUNT(*) FROM schema_migrations)",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?;
        if slots != ["a", "b"]
            || publication != (1, "a".to_owned(), 0)
            || history != (SCHEMA_VERSION.to_string(), i64::try_from(migrations.len())?)
        {
            return Err(io::Error::other(format!(
                "structural publication initialization drifted: slots={slots:?}, publication={publication:?}, history={history:?}"
            ))
            .into());
        }

        for (statement, description) in [
            (
                "INSERT INTO graph_structural_slots(slot) VALUES('c')",
                "a third structural slot",
            ),
            (
                "UPDATE graph_structural_slots SET slot = 'c' WHERE slot = 'b'",
                "a rewritten structural slot identity",
            ),
            (
                "DELETE FROM graph_structural_slots WHERE slot = 'b'",
                "a deleted structural slot identity",
            ),
            (
                "INSERT INTO graph_publication_state(singleton, active_slot, active_epoch) VALUES(2, 'a', 0)",
                "a second publication row",
            ),
            (
                "DELETE FROM graph_publication_state WHERE singleton = 1",
                "a deleted publication singleton",
            ),
            (
                "UPDATE graph_publication_state SET active_slot = 'c' WHERE singleton = 1",
                "an invalid active structural slot",
            ),
            (
                "UPDATE graph_publication_state SET active_epoch = -1 WHERE singleton = 1",
                "a negative publication epoch",
            ),
        ] {
            if connection.execute(statement, []).is_ok() {
                return Err(io::Error::other(format!(
                    "structural publication schema accepted {description}"
                ))
                .into());
            }
        }
        connection.execute(
            "UPDATE graph_publication_state
             SET active_slot = 'b', active_epoch = 1
             WHERE singleton = 1",
            [],
        )?;
        verify_structural_publication_schema(connection)?;
        verify_current_schema(connection)?;

        let mut rollback_connection = Connection::open_in_memory()?;
        initialize_migration_prefix(&mut rollback_connection, &migrations[..3])?;
        seed_preserved_rows(&rollback_connection)?;
        let rollback_before = preservation_snapshot(&rollback_connection)?;
        let mut rollback_plan = SchemaMigrationPlan {
            source_version: typed_graph_migration.schema_version,
            target_version: SCHEMA_VERSION,
            pending: migrations[3..].to_vec(),
        };
        rollback_plan.pending[0].definition.verify = reject_structural_publication;
        match apply_migration_plan(&mut rollback_connection, &rollback_plan) {
            Err(DbError::SchemaPreflight { message })
                if message.contains("injected structural publication verification failure") => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "failed structural publication migration returned the wrong error: {error}"
                ))
                .into());
            }
            Ok(()) => {
                return Err(
                    io::Error::other("failed structural publication migration committed").into(),
                );
            }
        }
        if preservation_snapshot(&rollback_connection)? != rollback_before {
            return Err(io::Error::other(
                "failed structural publication migration changed authored or derived rows",
            )
            .into());
        }
        let rollback_history = rollback_connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'schema_version'),
                (SELECT COUNT(*) FROM schema_migrations)",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?;
        let rollback_objects = sqlite_objects(&rollback_connection)?;
        if rollback_history
            != (
                typed_graph_migration.schema_version.to_string(),
                i64::try_from(migrations.len() - rollback_plan.pending.len())?,
            )
            || [
                GRAPH_STRUCTURAL_SLOTS_TABLE,
                GRAPH_PUBLICATION_STATE_TABLE,
                GRAPH_STRUCTURAL_SLOTS_DELETE_GUARD,
                GRAPH_STRUCTURAL_SLOTS_UPDATE_GUARD,
                GRAPH_PUBLICATION_STATE_DELETE_GUARD,
            ]
            .iter()
            .any(|object| rollback_objects.contains_key(*object))
        {
            return Err(io::Error::other(format!(
                "failed structural publication migration changed the source schema: {rollback_history:?}"
            ))
            .into());
        }
        Ok(())
    }

    #[test]
    fn task_arri_ut_arri_4_11() -> Result<(), Box<dyn Error>> {
        fn reject_structural_binding(_: &Connection) -> DbResult<()> {
            Err(preflight_error(
                "injected structural derived-row verification failure",
            ))
        }

        fn seed_slotless_graph(connection: &Connection) -> DbResult<()> {
            let project = vec![9_u8; 16];
            let source = vec![1_u8; 32];
            let target = vec![2_u8; 32];
            let relation = vec![3_u8; 32];
            let evidence = vec![4_u8; 32];
            let resolution = vec![5_u8; 32];
            let fingerprint = vec![6_u8; 32];

            connection.execute(
                "INSERT INTO graph_entities(
                    stable_key_digest, stable_key_version, stable_key_canonical,
                    project_instance_id, entity_kind, repository_path, qualified_name,
                    parser_kind, parser_identity, parser_version
                 ) VALUES(?1, 1, ?2, ?3, 'declaration', 'src/lib.rs', 'crate::source',
                          'tree-sitter', 'rust', '1')",
                params![&source, vec![11_u8], &project],
            )?;
            connection.execute(
                "INSERT INTO graph_entities(
                    stable_key_digest, stable_key_version, stable_key_canonical,
                    project_instance_id, entity_kind, repository_path, qualified_name,
                    parser_kind, parser_identity, parser_version
                 ) VALUES(?1, 1, ?2, ?3, 'declaration', 'src/lib.rs', 'crate::target',
                          'tree-sitter', 'rust', '1')",
                params![&target, vec![12_u8], &project],
            )?;
            connection.execute(
                "INSERT INTO graph_relations(
                    stable_key_digest, stable_key_version, stable_key_canonical,
                    source_entity_digest, relation_kind, target_scope,
                    target_entity_digest, confidence, parser_kind,
                    parser_identity, parser_version
                 ) VALUES(?1, 1, ?2, ?3, 'calls', 'internal', ?4, 'exact',
                          'tree-sitter', 'rust', '1')",
                params![&relation, vec![13_u8], &source, &target],
            )?;
            connection.execute(
                "INSERT INTO graph_evidence_occurrences(
                    stable_key_digest, stable_key_version, stable_key_canonical,
                    relation_digest, origin_kind, origin_entity_digest,
                    resolver_name, resolver_version, content_span_fingerprint,
                    occurrence_discriminator, evidence_class, confidence, completeness
                 ) VALUES(?1, 1, ?2, ?3, 'entity', ?4, 'rust-resolver', '1', ?5,
                          0, 'direct', 'exact', 'complete')",
                params![&evidence, vec![14_u8], &relation, &source, &fingerprint],
            )?;
            connection.execute(
                "INSERT INTO graph_resolution_occurrences(
                    stable_key_digest, stable_key_version, stable_key_canonical,
                    source_entity_digest, relation_kind, origin_kind,
                    origin_entity_digest, resolver_name, resolver_version,
                    content_span_fingerprint, occurrence_discriminator,
                    resolution_status, candidate_total, candidate_completeness,
                    evidence_class, confidence, completeness,
                    parser_kind, parser_identity, parser_version
                 ) VALUES(?1, 1, ?2, ?3, 'calls', 'entity', ?3, 'rust-resolver', '1',
                          ?4, 1, 'ambiguous', 2, 'complete', 'direct', 'medium',
                          'complete', 'tree-sitter', 'rust', '1')",
                params![&resolution, vec![15_u8], &source, &fingerprint],
            )?;
            connection.execute(
                "INSERT INTO graph_resolution_candidates(
                    resolution_occurrence_digest, candidate_ordinal, target_scope,
                    target_entity_digest, confidence, explanation
                 ) VALUES(?1, 0, 'internal', ?2, 'high', 'visible target')",
                params![&resolution, &target],
            )?;
            connection.execute(
                "INSERT INTO graph_resolution_candidates(
                    resolution_occurrence_digest, candidate_ordinal, target_scope,
                    external_target_namespace, external_target_value,
                    confidence, explanation
                 ) VALUES(?1, 1, 'external', 'crate', 'outside', 'low',
                          'external alternative')",
                params![&resolution],
            )?;
            connection.execute_batch(
                "INSERT INTO graph_coverage(
                    scope_kind, coverage_state, produced_count, omitted_count,
                    structural_slot, last_changed_epoch
                 ) VALUES('repository', 'complete', 2, 0, 'b', 7);

                 INSERT INTO file_texts(
                    path, content_hash, byte_count, line_count, content
                 ) VALUES(
                    'src/lib.rs', 'slotless-text', 18, 1, 'pub fn source() {}'
                 );",
            )?;
            Ok(())
        }

        let migrations = allocated_migrations();
        let [
            _,
            _,
            _,
            publication_migration,
            structural_binding_migration,
            affected_reconciliation_migration,
        ] = migrations.as_slice()
        else {
            return Err(io::Error::other(
                "ARRI 4.11 requires publication and structural row-binding migrations",
            )
            .into());
        };
        if structural_binding_migration.schema_version != publication_migration.schema_version + 1
            || structural_binding_migration.definition.id != "bind-structural-derived-rows"
            || structural_binding_migration.definition.owner
                != "projectatlas-db::schema::structural-storage"
            || !structural_binding_migration
                .definition
                .derived_effects
                .contains("slot-bind=typed-graph,coverage,file-texts,optional-file-text-fts")
            || !structural_binding_migration
                .definition
                .derived_effects
                .contains("future-pack-vector-binding=structural-slot+structural-epoch+semantic-generation")
            || !structural_binding_migration
                .definition
                .derived_effects
                .contains("exclude=compatibility-and-authored-tables,semantic-vector-storage")
            || !structural_binding_migration
                .definition
                .forward_behavior
                .contains("retain-last-changed-epoch-as-non-key-freshness")
            || !structural_binding_migration
                .definition
                .evidence
                .contains("task_arri_ut_arri_4_11")
        {
            return Err(io::Error::other(format!(
                "structural row-binding migration contract is incomplete: {structural_binding_migration:?}"
            ))
            .into());
        }

        let prefix = tempfile::tempdir()?;
        let root = prefix.path().join("repository");
        let atlas = root.join(PROJECTATLAS_DIRECTORY_NAME);
        fs::create_dir_all(&atlas)?;
        let path = atlas.join("projectatlas.db");
        {
            let mut connection = Connection::open(&path)?;
            initialize_migration_prefix(&mut connection, &migrations[..4])?;
            connection.execute(
                "INSERT INTO metadata(key, value) VALUES('project_root', ?1)",
                [normalize_native_path_display(&root)],
            )?;
            connection.execute_batch(
                "INSERT INTO metadata(key, value)
                 VALUES('authored_note', 'preserve-structural-binding');

                 INSERT INTO nodes(
                    id, path, kind, extension, language, size_bytes,
                    mtime_ns, content_hash, exists_now
                 ) VALUES(
                    61, 'src/lib.rs', 'file', 'rs', 'rust', 18,
                    201, 'compatibility-node', 1
                 );

                 UPDATE graph_publication_state
                 SET active_slot = 'b', active_epoch = 7
                 WHERE singleton = 1;",
            )?;
            seed_slotless_graph(&connection)?;
        }

        let plan = preflight(&path)?;
        if plan.source_version != publication_migration.schema_version
            || plan.pending.len() != 2
            || plan.pending[0].definition.id != structural_binding_migration.definition.id
            || plan.pending[1].definition.id != affected_reconciliation_migration.definition.id
        {
            return Err(io::Error::other(format!(
                "valid publication prefix did not plan structural row binding: {plan:?}"
            ))
            .into());
        }

        let migrated = crate::AtlasStore::open(&path)?;
        let connection = &migrated.connection;
        verify_current_schema(connection)?;
        let preserved = connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'authored_note'),
                (SELECT path || '|' || kind || '|' || content_hash FROM nodes WHERE id = 61),
                (SELECT COUNT(*) FROM graph_entities),
                (SELECT COUNT(*) FROM graph_relations),
                (SELECT COUNT(*) FROM graph_evidence_occurrences),
                (SELECT COUNT(*) FROM graph_resolution_occurrences),
                (SELECT COUNT(*) FROM graph_resolution_candidates),
                (SELECT COUNT(*) FROM graph_coverage),
                (SELECT COUNT(*) FROM file_texts)",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            },
        )?;
        if preserved
            != (
                "preserve-structural-binding".to_owned(),
                "src/lib.rs|file|compatibility-node".to_owned(),
                2,
                1,
                1,
                1,
                2,
                1,
                1,
            )
        {
            return Err(io::Error::other(format!(
                "structural row binding did not preserve source rows: {preserved:?}"
            ))
            .into());
        }

        for table in [
            GRAPH_ENTITIES_TABLE,
            GRAPH_RELATIONS_TABLE,
            GRAPH_EVIDENCE_OCCURRENCES_TABLE,
            GRAPH_RESOLUTION_OCCURRENCES_TABLE,
            GRAPH_RESOLUTION_CANDIDATES_TABLE,
            GRAPH_COVERAGE_TABLE,
            FILE_TEXTS_TABLE,
        ] {
            let binding = connection.query_row(
                &format!("SELECT structural_slot, last_changed_epoch FROM {table} LIMIT 1"),
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )?;
            if binding != ("b".to_owned(), 7) {
                return Err(io::Error::other(format!(
                    "{table} did not bind existing rows to the active structural tuple: {binding:?}"
                ))
                .into());
            }
        }
        verify_optional_file_text_fts(connection)?;

        connection.execute_batch(
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
                parser_kind, parser_identity, parser_version, 'a', 99
             FROM graph_entities
             WHERE structural_slot = 'b' AND qualified_name = 'crate::source';

             INSERT INTO file_texts(
                path, content_hash, byte_count, line_count, content, updated_at,
                structural_slot, last_changed_epoch
             )
             SELECT
                path, content_hash, byte_count, line_count, content, updated_at, 'a', 99
             FROM file_texts
             WHERE structural_slot = 'b' AND path = 'src/lib.rs';

             INSERT INTO graph_coverage(
                scope_kind, repository_path, pass_identity, relation_kind,
                coverage_state, produced_count, omitted_count, reached_limit,
                reason, structural_slot, last_changed_epoch
             )
             SELECT
                scope_kind, repository_path, pass_identity, relation_kind,
                coverage_state, produced_count, omitted_count, reached_limit,
                reason, 'a', 99
             FROM graph_coverage
             WHERE structural_slot = 'b' AND scope_kind = 'repository';",
        )?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let cross_slot_relation = connection.execute(
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
                confidence, parser_kind, parser_identity, parser_version, 'a', 99
             FROM graph_relations
             WHERE structural_slot = 'b'",
            [],
        );
        if cross_slot_relation.is_ok() {
            return Err(io::Error::other(
                "same-slot relation foreign keys accepted a target from another slot",
            )
            .into());
        }
        connection.execute_batch(
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
                parser_kind, parser_identity, parser_version, 'a', 99
             FROM graph_entities
             WHERE structural_slot = 'b' AND qualified_name = 'crate::target';

             INSERT INTO graph_relations(
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
                confidence, parser_kind, parser_identity, parser_version, 'a', 99
             FROM graph_relations
             WHERE structural_slot = 'b';

             UPDATE graph_entities
             SET last_changed_epoch = 100
             WHERE structural_slot = 'a' AND qualified_name = 'crate::source';",
        )?;
        let visibility_identity = connection.query_row(
            "SELECT
                (SELECT COUNT(*) FROM graph_entities
                 WHERE qualified_name = 'crate::source'),
                (SELECT COUNT(DISTINCT structural_slot) FROM graph_entities
                 WHERE qualified_name = 'crate::source'),
                (SELECT COUNT(*) FROM file_texts WHERE path = 'src/lib.rs'),
                (SELECT active_slot FROM graph_publication_state WHERE singleton = 1),
                (SELECT active_epoch FROM graph_publication_state WHERE singleton = 1)",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )?;
        if visibility_identity != (2, 2, 2, "b".to_owned(), 7) {
            return Err(io::Error::other(format!(
                "slot identity or freshness-only epoch behavior drifted: {visibility_identity:?}"
            ))
            .into());
        }
        verify_structural_derived_row_schema(connection)?;

        let table_names = object_names(connection, "table")?;
        if table_names
            .iter()
            .any(|table| table.contains("vector") || table.contains("semantic"))
            || table_columns(connection, GRAPH_PUBLICATION_STATE_TABLE)?
                != ["singleton", "active_slot", "active_epoch"]
        {
            return Err(io::Error::other(
                "core structural migration created or published pack-owned semantic storage",
            )
            .into());
        }

        let mut rollback_connection = Connection::open_in_memory()?;
        initialize_migration_prefix(&mut rollback_connection, &migrations[..4])?;
        rollback_connection.execute_batch(
            "INSERT INTO metadata(key, value)
             VALUES('authored_note', 'rollback-structural-binding');
             INSERT INTO file_texts(
                path, content_hash, byte_count, line_count, content
             ) VALUES('src/rollback.rs', 'rollback-text', 19, 1, 'rollback survives');",
        )?;
        let rollback_before = rollback_connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'authored_note'),
                (SELECT content FROM file_texts WHERE path = 'src/rollback.rs')",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?;
        let mut rollback_plan = SchemaMigrationPlan {
            source_version: publication_migration.schema_version,
            target_version: SCHEMA_VERSION,
            pending: vec![
                structural_binding_migration.clone(),
                affected_reconciliation_migration.clone(),
            ],
        };
        rollback_plan.pending[0].definition.verify = reject_structural_binding;
        match apply_migration_plan(&mut rollback_connection, &rollback_plan) {
            Err(DbError::SchemaPreflight { message })
                if message.contains("injected structural derived-row verification failure") => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "failed structural row binding returned the wrong error: {error}"
                ))
                .into());
            }
            Ok(()) => {
                return Err(io::Error::other("failed structural row binding committed").into());
            }
        }
        let rollback_after = rollback_connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'authored_note'),
                (SELECT content FROM file_texts WHERE path = 'src/rollback.rs')",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?;
        let rollback_history = rollback_connection.query_row(
            "SELECT
                (SELECT value FROM metadata WHERE key = 'schema_version'),
                (SELECT COUNT(*) FROM schema_migrations)",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?;
        if rollback_after != rollback_before
            || rollback_history
                != (
                    publication_migration.schema_version.to_string(),
                    i64::try_from(migrations.len() - 2)?,
                )
            || table_columns(&rollback_connection, FILE_TEXTS_TABLE)?
                != [
                    "path",
                    "content_hash",
                    "byte_count",
                    "line_count",
                    "content",
                    "updated_at",
                ]
        {
            return Err(io::Error::other(format!(
                "failed structural row binding changed the source schema or rows: {rollback_history:?}"
            ))
            .into());
        }
        verify_unbound_typed_repository_graph_schema(&rollback_connection)?;
        verify_unbound_optional_file_text_fts(&rollback_connection)?;
        verify_structural_publication_schema(&rollback_connection)?;
        Ok(())
    }

    fn initialize_ledger_schema(
        connection: &mut Connection,
        migration: &AllocatedMigration,
    ) -> DbResult<()> {
        initialize_migration_prefix(connection, std::slice::from_ref(migration))
    }

    fn initialize_migration_prefix(
        connection: &mut Connection,
        migrations: &[AllocatedMigration],
    ) -> DbResult<()> {
        let transaction = connection.transaction()?;
        initialize_schema_objects(&transaction)?;
        ensure_project_instance_id(&transaction, true)?;
        for migration in migrations {
            (migration.definition.apply)(&transaction)?;
            (migration.definition.verify)(&transaction)?;
            record_applied_migration(&transaction, migration)?;
            write_schema_version(&transaction, migration.schema_version)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Require a rejected database and its sidecar inventory to remain untouched.
    fn require_database_unchanged(path: &Path, expected: &[u8]) -> Result<(), Box<dyn Error>> {
        if fs::read(path)? != expected {
            return Err(io::Error::other(format!(
                "schema preflight modified rejected database {}",
                path.display()
            ))
            .into());
        }
        for suffix in ["-wal", "-shm", "-journal"] {
            let sidecar = sqlite_sidecar_path(path, suffix);
            if sidecar.exists() {
                return Err(io::Error::other(format!(
                    "schema preflight created rejected-database sidecar {}",
                    sidecar.display()
                ))
                .into());
            }
        }
        Ok(())
    }

    /// Remove closed-test sidecars so read-only preflight starts from a stable file set.
    fn remove_sqlite_sidecars(path: &Path) -> Result<(), Box<dyn Error>> {
        for suffix in ["-wal", "-shm", "-journal"] {
            let sidecar = sqlite_sidecar_path(path, suffix);
            match fs::remove_file(&sidecar) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn seed_legacy_schema(path: &std::path::Path, version: i64) -> DbResult<()> {
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "
            CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE summaries(id INTEGER PRIMARY KEY, node_id INTEGER NOT NULL, summary TEXT);
            CREATE TABLE symbols(
                id INTEGER PRIMARY KEY,
                path TEXT NOT NULL,
                language TEXT,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                signature TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL,
                parent TEXT,
                parser TEXT NOT NULL,
                detail TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE usage_events(
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
                estimated_tokens_saved,
                token_savings_bucket
            ) VALUES('legacy-session', 'legacy', 100, 20, 80, 'full_file_compression');
            ",
        )?;
        connection.execute(
            "INSERT INTO metadata(key, value) VALUES('schema_version', ?1)",
            [version.to_string()],
        )?;
        Ok(())
    }

    fn require_column_contract(
        connection: &Connection,
        table: &str,
        column: &str,
        expected: (&str, bool, Option<&str>, bool),
    ) -> Result<(), Box<dyn Error>> {
        let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)? != 0,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)? != 0,
            ))
        })?;
        let mut actual = None;
        for row in rows {
            let (name, declared_type, not_null, default_value, primary_key) = row?;
            if name == column {
                actual = Some((declared_type, not_null, default_value, primary_key));
                break;
            }
        }
        let actual = actual
            .ok_or_else(|| io::Error::other(format!("missing {table}.{column} schema contract")))?;
        let expected = (
            expected.0.to_string(),
            expected.1,
            expected.2.map(str::to_string),
            expected.3,
        );
        if actual != expected {
            return Err(io::Error::other(format!(
                "{table}.{column} schema contract changed: {actual:?}"
            ))
            .into());
        }
        Ok(())
    }

    fn index_columns(connection: &Connection, index: &str) -> DbResult<Vec<String>> {
        let mut statement = connection.prepare(&format!("PRAGMA index_info({index})"))?;
        let rows = statement.query_map([], |row| row.get::<_, String>(2))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    fn object_names(connection: &Connection, kind: &str) -> DbResult<Vec<String>> {
        let mut statement = connection.prepare(
            "SELECT name, tbl_name
             FROM sqlite_master
             WHERE type = ?1 AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )?;
        let rows = statement.query_map([kind], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut names = Vec::new();
        for row in rows {
            let (name, table_name) = row?;
            if !is_optional_file_text_fts_object(&name, &table_name) {
                names.push(name);
            }
        }
        Ok(names)
    }
}
