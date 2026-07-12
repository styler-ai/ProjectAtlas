//! `SQLite` schema initialization and legacy repair behind the store facade.

use crate::{DbError, DbResult, sqlite_read_uri};
use projectatlas_core::graph::ProjectInstanceId;
use projectatlas_core::normalize_native_path_display;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

/// Current `SQLite` schema version supported by this crate.
const SCHEMA_VERSION: i64 = 9;
/// Metadata key for the schema contract version.
const SCHEMA_VERSION_METADATA_KEY: &str = "schema_version";
/// Metadata key for one independently initialized database identity.
const PROJECT_INSTANCE_ID_METADATA_KEY: &str = "project_instance_id";

/// Earliest metadata schema version that the current repair path accepts.
const MIN_SUPPORTED_SCHEMA_VERSION: i64 = 1;
/// Project-local directory that owns the database file.
const PROJECTATLAS_DIRECTORY_NAME: &str = ".projectatlas";
/// Minimum headroom reserved when estimating backup feasibility.
const BACKUP_HEADROOM_BYTES: u64 = 64 * 1024;

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
                    || self.root_binding.project_instance_id.is_none()
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
        Ok(())
    }
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
pub(crate) fn preflight(path: &Path) -> DbResult<()> {
    let report = inspect_schema_preflight(path)?;
    report.ensure_write_ready()
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
pub(crate) fn preflight_in_memory() -> DbResult<()> {
    let connection = Connection::open_in_memory()?;
    let features = sqlite_feature_state(&connection)?;
    if features.version.is_empty() || features.compile_options.is_empty() {
        return Err(preflight_error(
            "in-memory SQLite runtime capability inventory is unavailable",
        ));
    }
    match integrity_state(&connection)? {
        IntegrityState::Passed => Ok(()),
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
            conflicting_partial_objects: conflicts,
        })
    })();
    drop(connection);
    let journal_mode = inspection
        .as_ref()
        .map_or("unknown", |report| report.sidecars.journal_mode.as_str());
    let after_sidecars = sidecar_state(path, journal_mode)?;
    if before_sidecars.wal != after_sidecars.wal
        || before_sidecars.shm != after_sidecars.shm
        || before_sidecars.rollback_journal != after_sidecars.rollback_journal
    {
        return Err(preflight_error(
            "read-only SQLite inspection changed WAL or journal sidecar state",
        ));
    }
    inspection.map(|mut report| {
        report.sidecars = after_sidecars;
        report
    })
}

/// Build the authoritative current object contract in a throwaway database.
fn current_schema_reference() -> DbResult<(Connection, Vec<SchemaObjectContract>)> {
    let connection = Connection::open_in_memory()?;
    initialize(&connection)?;
    let contracts = schema_object_contracts(&connection)?;
    Ok((connection, contracts))
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
                if actual.table_name == contract.table_name && actual_columns == contract.columns {
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

/// Initialize the current schema and repair supported legacy columns.
pub(crate) fn initialize(connection: &Connection) -> DbResult<()> {
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
    reconcile_schema_version(connection)
}

/// Reconcile the supported metadata schema version without changing its contract.
fn reconcile_schema_version(connection: &Connection) -> DbResult<()> {
    let stored = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [SCHEMA_VERSION_METADATA_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(value) = stored {
        let found = value.parse::<i64>().map_or(-1, |parsed| parsed);
        if (1..SCHEMA_VERSION).contains(&found) {
            ensure_project_instance_id(connection, true)?;
            connection.execute(
                "UPDATE metadata SET value = ?1 WHERE key = ?2",
                (SCHEMA_VERSION.to_string(), SCHEMA_VERSION_METADATA_KEY),
            )?;
        } else if found == SCHEMA_VERSION {
            ensure_project_instance_id(connection, false)?;
        } else {
            return Err(DbError::SchemaVersion {
                found,
                expected: SCHEMA_VERSION,
            });
        }
    } else {
        ensure_project_instance_id(connection, true)?;
        connection.execute(
            "INSERT INTO metadata(key, value) VALUES(?1, ?2)",
            (SCHEMA_VERSION_METADATA_KEY, SCHEMA_VERSION.to_string()),
        )?;
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
                "health_resolutions",
                "metadata",
                "nodes",
                "purposes",
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
        seed_legacy_schema(&legacy_path, SCHEMA_VERSION - 1)?;
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
        seed_legacy_schema(&preserved_path, SCHEMA_VERSION - 1)?;
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
            seed_legacy_schema(&corrupt_path, SCHEMA_VERSION - 1)?;
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
            if stored_version != (SCHEMA_VERSION - 1).to_string() {
                return Err(io::Error::other(format!(
                    "{label} identity advanced the schema version before failing"
                ))
                .into());
            }
        }

        let missing = tempfile::tempdir()?;
        let missing_path = missing.path().join("missing.db");
        seed_legacy_schema(&missing_path, SCHEMA_VERSION)?;
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
            Ok(()) => {
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
        {
            return Err(io::Error::other(format!(
                "supported source preflight did not record migration readiness: {supported_report:?}"
            ))
            .into());
        }

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
            Ok(()) => {
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
            "SELECT name FROM sqlite_master WHERE type = ?1 AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        let rows = statement.query_map([kind], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }
}
