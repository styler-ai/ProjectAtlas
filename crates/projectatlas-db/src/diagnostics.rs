//! Build bounded content-free database settings diagnostics.

use crate::schema::{self, SCHEMA_VERSION, SchemaState};
use crate::sqlite_profile::{
    REQUIRED_JOURNAL_MODE, REQUIRED_SYNCHRONOUS_NAME, SQLITE_BUSY_TIMEOUT,
};
use crate::telemetry::{
    PlannerStatisticsPolicy, PlannerStatisticsState, TelemetryCheckpointState,
    TelemetryRetentionPolicy,
};
use crate::{AtlasStore, DbError, DbResult, IndexPublication, IndexPublicationState};
use projectatlas_core::IndexGeneration;
use projectatlas_core::graph::{CoverageRecord, GraphIdentityText, RepositoryNodePath};
use projectatlas_core::graph::{CoverageScope, CoverageState, GraphLimitKind, GraphRelationKind};
use rusqlite::{Connection, ErrorCode};
use serde::Serialize;
use std::path::Path;

/// Maximum project-scope coverage rows returned by one settings report.
const COVERAGE_SAMPLE_LIMIT: u32 = 8;
/// Maximum distinct compile options accepted from one linked `SQLite` runtime.
const MAX_COMPILE_OPTIONS: u32 = 1_024;
/// Maximum bytes accepted for one linked `SQLite` compile-option name.
const MAX_COMPILE_OPTION_BYTES: usize = 1_024;
/// Domain separator for deterministic `SQLite` compile-option identities.
const COMPILE_OPTIONS_DIGEST_DOMAIN: &[u8] = b"projectatlas:sqlite-compile-options:v1\0";
/// Exact lowercase hexadecimal bytes in a runtime-owned publication fingerprint.
const PUBLICATION_FINGERPRINT_BYTES: usize = 64;

/// Compatibility of the selected database with the current runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseSchemaCompatibility {
    /// The database file does not exist yet.
    Missing,
    /// The durable schema matches the current runtime.
    Current,
    /// The durable schema is an admitted predecessor with a transactional migration path.
    SupportedPredecessor,
    /// The durable schema or project binding is not admitted by this runtime.
    Incompatible,
    /// `SQLite` or its integrity check identified corrupt database state.
    Corrupt,
    /// Filesystem support could not admit a safe database inspection.
    NotInspected,
}

/// Content-free schema and migration state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DatabaseSchemaReport {
    /// Schema version supported by this runtime.
    pub runtime_version: i64,
    /// Durable schema version when a validated admitted schema exposed one.
    pub stored_version: Option<i64>,
    /// Closed compatibility and migration state.
    pub compatibility: DatabaseSchemaCompatibility,
    /// Whether the selected state is known to require migration.
    pub migration_required: Option<bool>,
    /// Whether this runtime owns a complete migration path for the selected state.
    pub migration_supported: bool,
    /// Remaining append-only migration steps when the state is admitted.
    pub migration_steps_remaining: Option<u32>,
}

/// WAL suitability of the selected database filesystem.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseFilesystemSupport {
    /// The location resolved to an admitted local filesystem.
    SupportedLocal,
    /// The location resolved to a known unsupported network or distributed filesystem.
    Unsupported,
    /// The runtime could not prove an admitted local filesystem.
    Uncertain,
}

/// Deterministic fixed-size identity of the active `SQLite` compile options.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SqliteCompileOptionsIdentity {
    /// Number of distinct compile options represented by the digest.
    pub count: u32,
    /// Lowercase BLAKE3 digest of sorted length-delimited option names.
    pub digest: String,
}

/// Content-free identity of the linked `SQLite` runtime.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SqliteRuntimeReport {
    /// Version reported by the linked `SQLite` library.
    pub version: String,
    /// Numeric version reported by the linked `SQLite` library.
    pub version_number: i32,
    /// Fixed-size identity of all active compile options.
    pub compile_options: SqliteCompileOptionsIdentity,
}

/// Required and observed database connection and maintenance policy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DatabaseOperatingProfileReport {
    /// Journal mode required for live project databases.
    pub required_journal_mode: String,
    /// Journal mode observed on the reporting connection, when available.
    pub observed_journal_mode: Option<String>,
    /// Synchronous mode required for authored and derived state.
    pub required_synchronous_mode: String,
    /// Synchronous mode observed on the reporting connection, when available.
    pub observed_synchronous_mode: Option<String>,
    /// Busy timeout required for ordinary connections in milliseconds.
    pub required_busy_timeout_ms: u64,
    /// Busy timeout observed on the reporting connection in milliseconds.
    pub observed_busy_timeout_ms: Option<u64>,
    /// Writes between `ProjectAtlas` passive-checkpoint attempts.
    pub checkpoint_write_interval: usize,
    /// Most recent passive-checkpoint state, when durable state is available.
    pub checkpoint_state: Option<TelemetryCheckpointState>,
    /// Connection-local automatic checkpoint threshold, when available.
    pub wal_autocheckpoint_pages: Option<usize>,
    /// `ProjectAtlas` planner-statistics maintenance policy.
    pub statistics_policy: PlannerStatisticsPolicy,
    /// Observed planner-statistics state, when available.
    pub statistics_state: Option<PlannerStatisticsState>,
}

/// Validation state of persisted publication-contract identity metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabasePublicationContractState {
    /// No contract fingerprint is present for this publication state.
    Missing,
    /// The fingerprint is a validated lowercase 256-bit hexadecimal identity.
    Valid,
    /// Persisted metadata is not a safe runtime-owned fingerprint and was omitted.
    Invalid,
}

/// Content-free active publication identity for settings diagnostics.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DatabasePublicationReport {
    /// Current publication lifecycle state.
    pub state: IndexPublicationState,
    /// Validated contract identity, never arbitrary persisted metadata.
    pub contract_fingerprint: Option<String>,
    /// Validation state for the persisted contract identity.
    pub contract_fingerprint_state: DatabasePublicationContractState,
    /// Monotonic generation of the last complete derived index.
    pub generation: IndexGeneration,
}

/// Whether a bounded coverage total is exact or a lower bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseCoverageTotalState {
    /// Every matching row was returned by the bounded query.
    Exact,
    /// At least one additional row exists beyond the returned sample.
    AtLeast,
}

/// One content-free project-scope graph coverage row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DatabaseCoverageSample {
    /// Typed project or repository-relative path scope.
    pub scope: CoverageScope,
    /// Optional relation family covered by this row.
    pub relation: Option<GraphRelationKind>,
    /// Coverage lifecycle state.
    pub state: CoverageState,
    /// Supported items in this coverage scope.
    pub total: u64,
    /// Successfully covered items.
    pub covered: u64,
    /// Omitted, failed, or untrusted items.
    pub omitted: u64,
    /// Bounded actionable non-complete explanation.
    pub actionable_reason: Option<String>,
    /// Product limit reached, when applicable.
    pub reached_limit: Option<GraphLimitKind>,
}

/// One bounded project-scope graph coverage sample with honest total state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DatabaseCoverageSummary {
    /// Complete graph generation represented by the coverage rows.
    pub generation: IndexGeneration,
    /// Fixed maximum number of returned rows.
    pub sample_limit: u32,
    /// Rows validated including the one truncation sentinel, when present.
    pub inspected: usize,
    /// Rows returned to the caller.
    pub returned: usize,
    /// Validated rows in deterministic indexed order.
    pub sample: Vec<DatabaseCoverageSample>,
    /// Exact total when complete, otherwise a proved lower bound.
    pub known_total: usize,
    /// Interpretation of `known_total`.
    pub total_state: DatabaseCoverageTotalState,
    /// Whether additional matching rows exist.
    pub truncated: bool,
    /// Existing agent route for inspecting one returned path in context.
    pub next_call: &'static str,
}

/// Bounded content-free database state for CLI and MCP settings projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DatabaseSettingsReport {
    /// Schema compatibility and migration state.
    pub schema: DatabaseSchemaReport,
    /// Linked `SQLite` runtime identity.
    pub sqlite: SqliteRuntimeReport,
    /// WAL suitability of the selected database location.
    pub filesystem: DatabaseFilesystemSupport,
    /// Required and observed operating profile.
    pub operating_profile: DatabaseOperatingProfileReport,
    /// Current derived publication state, when the schema is current.
    pub publication: Option<DatabasePublicationReport>,
    /// Bounded project-scope graph coverage, when a complete graph is available.
    pub coverage: Option<DatabaseCoverageSummary>,
}

/// Build bounded content-free database settings without creating, migrating, or repairing state.
///
/// # Errors
///
/// Returns an error when runtime introspection or an otherwise admitted database read fails.
pub fn database_settings_report(path: &Path) -> DbResult<DatabaseSettingsReport> {
    let sqlite = sqlite_runtime_report()?;
    let operating_profile = required_operating_profile()?;
    let filesystem = match crate::sqlite_profile::inspect_database_location(path) {
        Ok(_) => DatabaseFilesystemSupport::SupportedLocal,
        Err(DbError::DatabaseFilesystemUnsupported { .. }) => {
            return Ok(uninspected_report(
                sqlite,
                operating_profile,
                DatabaseFilesystemSupport::Unsupported,
            ));
        }
        Err(DbError::DatabaseFilesystemUncertain { .. }) => {
            return Ok(uninspected_report(
                sqlite,
                operating_profile,
                DatabaseFilesystemSupport::Uncertain,
            ));
        }
        Err(error) => return Err(error),
    };

    let preflight = match schema::inspect_compatibility(path, None) {
        Ok((preflight, _)) => preflight,
        Err(error) => {
            let stored_version = stored_version_from_error(&error);
            let compatibility = classify_schema_error(error)?;
            return Ok(DatabaseSettingsReport {
                schema: schema_report(stored_version, compatibility),
                sqlite,
                filesystem,
                operating_profile,
                publication: None,
                coverage: None,
            });
        }
    };
    let compatibility = match preflight.state {
        SchemaState::Fresh => DatabaseSchemaCompatibility::Missing,
        SchemaState::Current => DatabaseSchemaCompatibility::Current,
        SchemaState::UpgradeRequired => DatabaseSchemaCompatibility::SupportedPredecessor,
    };
    if preflight.state != SchemaState::Current {
        return Ok(DatabaseSettingsReport {
            schema: schema_report(preflight.schema_version, compatibility),
            sqlite,
            filesystem,
            operating_profile,
            publication: None,
            coverage: None,
        });
    }

    let store = match AtlasStore::open_read_only(path) {
        Ok(store) => store,
        Err(error) => {
            let compatibility = classify_schema_error(error)?;
            return Ok(DatabaseSettingsReport {
                schema: schema_report(preflight.schema_version, compatibility),
                sqlite,
                filesystem,
                operating_profile,
                publication: None,
                coverage: None,
            });
        }
    };
    current_report(
        &store,
        preflight.schema_version,
        sqlite,
        filesystem,
        operating_profile,
    )
}

/// Build the constant required operating profile before observed state is available.
fn required_operating_profile() -> DbResult<DatabaseOperatingProfileReport> {
    let required_busy_timeout_ms =
        u64::try_from(SQLITE_BUSY_TIMEOUT.as_millis()).map_err(|_source| {
            DbError::GraphCountOverflow {
                field: "database_settings.required_busy_timeout_ms",
                value: u64::MAX,
            }
        })?;
    Ok(DatabaseOperatingProfileReport {
        required_journal_mode: REQUIRED_JOURNAL_MODE.to_string(),
        observed_journal_mode: None,
        required_synchronous_mode: REQUIRED_SYNCHRONOUS_NAME.to_string(),
        observed_synchronous_mode: None,
        required_busy_timeout_ms,
        observed_busy_timeout_ms: None,
        checkpoint_write_interval: TelemetryRetentionPolicy::default().checkpoint_write_interval,
        checkpoint_state: None,
        wal_autocheckpoint_pages: None,
        statistics_policy: PlannerStatisticsPolicy::NotConfigured,
        statistics_state: None,
    })
}

/// Build a report when filesystem policy intentionally prevents database inspection.
fn uninspected_report(
    sqlite: SqliteRuntimeReport,
    operating_profile: DatabaseOperatingProfileReport,
    filesystem: DatabaseFilesystemSupport,
) -> DatabaseSettingsReport {
    DatabaseSettingsReport {
        schema: schema_report(None, DatabaseSchemaCompatibility::NotInspected),
        sqlite,
        filesystem,
        operating_profile,
        publication: None,
        coverage: None,
    }
}

/// Complete current-schema diagnostics from one validated read-only snapshot.
fn current_report(
    store: &AtlasStore,
    stored_version: Option<i64>,
    sqlite: SqliteRuntimeReport,
    filesystem: DatabaseFilesystemSupport,
    mut operating_profile: DatabaseOperatingProfileReport,
) -> DbResult<DatabaseSettingsReport> {
    let publication = store.index_publication()?.map(database_publication_report);
    let coverage = if store.validated_project_instance_id.is_some() {
        let telemetry = store.telemetry_retention_state()?;
        operating_profile.observed_journal_mode = Some(telemetry.journal_mode);
        operating_profile.observed_synchronous_mode = Some(telemetry.synchronous_mode);
        operating_profile.observed_busy_timeout_ms = Some(telemetry.connection_busy_timeout_ms);
        operating_profile.checkpoint_write_interval = telemetry.checkpoint_write_interval;
        operating_profile.checkpoint_state = Some(telemetry.checkpoint_state);
        operating_profile.wal_autocheckpoint_pages = Some(telemetry.wal_autocheckpoint_pages);
        operating_profile.statistics_policy = telemetry.statistics_policy;
        operating_profile.statistics_state = Some(telemetry.statistics_state);
        if publication
            .as_ref()
            .is_some_and(is_trusted_complete_publication)
        {
            coverage_summary(store, publication.as_ref())?
        } else {
            None
        }
    } else {
        None
    };
    Ok(DatabaseSettingsReport {
        schema: schema_report(stored_version, DatabaseSchemaCompatibility::Current),
        sqlite,
        filesystem,
        operating_profile,
        publication,
        coverage,
    })
}

/// Return whether derived diagnostic rows belong to one validated complete publication.
fn is_trusted_complete_publication(publication: &DatabasePublicationReport) -> bool {
    publication.state == IndexPublicationState::Complete
        && publication.generation != IndexGeneration::ZERO
        && publication.contract_fingerprint_state == DatabasePublicationContractState::Valid
}

/// Omit arbitrary persisted publication text while retaining validated identity truth.
fn database_publication_report(publication: IndexPublication) -> DatabasePublicationReport {
    let (contract_fingerprint, contract_fingerprint_state) = match publication.contract_fingerprint
    {
        None => (None, DatabasePublicationContractState::Missing),
        Some(fingerprint) if is_valid_publication_fingerprint(&fingerprint) => {
            (Some(fingerprint), DatabasePublicationContractState::Valid)
        }
        Some(_) => (None, DatabasePublicationContractState::Invalid),
    };
    DatabasePublicationReport {
        state: publication.state,
        contract_fingerprint,
        contract_fingerprint_state,
        generation: publication.generation,
    }
}

/// Accept only the runtime's lowercase BLAKE3 publication identity shape.
fn is_valid_publication_fingerprint(value: &str) -> bool {
    value.len() == PUBLICATION_FINGERPRINT_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Project one existing bounded non-complete coverage page.
fn coverage_summary(
    store: &AtlasStore,
    publication: Option<&DatabasePublicationReport>,
) -> DbResult<Option<DatabaseCoverageSummary>> {
    let Some(publication) = publication else {
        return Ok(None);
    };
    if publication.state != crate::IndexPublicationState::Complete
        || publication.generation == IndexGeneration::ZERO
    {
        return Ok(None);
    }
    let Some(graph_generation) = crate::project_identity::load_graph_generation(&store.connection)?
    else {
        return Ok(None);
    };
    if graph_generation == IndexGeneration::ZERO || graph_generation != publication.generation {
        return Err(DbError::GraphRowShape {
            table: "project_identity",
            reason: "diagnostic coverage generation does not match complete publication",
        });
    }
    let limit_plus_one = i64::from(COVERAGE_SAMPLE_LIMIT) + 1;
    let mut statement = store.connection.prepare_cached(
        "SELECT scope_kind, scope_path, state, total, covered, omitted, reason, reached_limit
           FROM graph_coverage INDEXED BY idx_graph_coverage_relation_state
          WHERE relation_scope IS NULL
            AND relation_kind IS NULL
            AND state IN ('partial', 'failed', 'ignored', 'oversized',
                          'quarantined', 'stale')
          ORDER BY state, id
          LIMIT ?1",
    )?;
    let raw = statement
        .query_map([limit_plus_one], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let inspected = raw.len();
    let truncated = inspected > COVERAGE_SAMPLE_LIMIT as usize;
    let sample = raw
        .into_iter()
        .take(COVERAGE_SAMPLE_LIMIT as usize)
        .map(
            |(scope_kind, scope_path, state, total, covered, omitted, reason, reached_limit)| {
                let scope = match (scope_kind.as_str(), scope_path) {
                    ("project", None) => CoverageScope::Project,
                    ("path", Some(path)) => CoverageScope::Path {
                        path: RepositoryNodePath::new(Path::new(&path))?,
                    },
                    ("project" | "path", _) => {
                        return Err(DbError::GraphRowShape {
                            table: "graph_coverage",
                            reason: "diagnostic coverage scope columns contradict scope kind",
                        });
                    }
                    (value, _) => {
                        return Err(DbError::InvalidEnum {
                            field: "graph_coverage.scope_kind",
                            value: value.to_string(),
                        });
                    }
                };
                let reason = reason.map(GraphIdentityText::new).transpose()?;
                let record = CoverageRecord::new(
                    scope,
                    None,
                    crate::repository_graph::parse_coverage_state(&state)?,
                    nonnegative_coverage_count("graph_coverage.covered", covered)?,
                    nonnegative_coverage_count("graph_coverage.omitted", omitted)?,
                    graph_generation,
                    reason,
                    reached_limit
                        .as_deref()
                        .map(crate::repository_graph::parse_limit_kind)
                        .transpose()?,
                )?;
                if record.total() != nonnegative_coverage_count("graph_coverage.total", total)? {
                    return Err(DbError::GraphRowShape {
                        table: "graph_coverage",
                        reason: "diagnostic coverage total does not equal covered plus omitted",
                    });
                }
                Ok(DatabaseCoverageSample {
                    scope: record.scope().clone(),
                    relation: None,
                    state: record.state(),
                    total: record.total(),
                    covered: record.covered(),
                    omitted: record.omitted(),
                    actionable_reason: record.reason().map(|value| value.as_str().to_string()),
                    reached_limit: record.reached_limit(),
                })
            },
        )
        .collect::<DbResult<Vec<_>>>()?;
    let known_total = sample.len() + usize::from(truncated);
    Ok(Some(DatabaseCoverageSummary {
        generation: graph_generation,
        sample_limit: COVERAGE_SAMPLE_LIMIT,
        inspected,
        returned: sample.len(),
        sample,
        known_total,
        total_state: if truncated {
            DatabaseCoverageTotalState::AtLeast
        } else {
            DatabaseCoverageTotalState::Exact
        },
        truncated,
        next_call: "atlas_file_summary",
    }))
}

/// Convert one nonnegative persisted coverage count without losing its source error.
fn nonnegative_coverage_count(field: &'static str, value: i64) -> DbResult<u64> {
    u64::try_from(value).map_err(|source| DbError::InvalidCount {
        field,
        value,
        source,
    })
}

/// Derive explicit migration flags from the closed compatibility state.
fn schema_report(
    stored_version: Option<i64>,
    compatibility: DatabaseSchemaCompatibility,
) -> DatabaseSchemaReport {
    let migration_steps_remaining = stored_version.and_then(schema::migration_steps_remaining);
    let (migration_required, migration_supported, migration_steps_remaining) = match compatibility {
        DatabaseSchemaCompatibility::Missing | DatabaseSchemaCompatibility::Current => {
            (Some(false), true, Some(0))
        }
        DatabaseSchemaCompatibility::SupportedPredecessor => {
            (Some(true), true, migration_steps_remaining)
        }
        DatabaseSchemaCompatibility::Incompatible => (
            stored_version.map(|version| version != SCHEMA_VERSION),
            false,
            None,
        ),
        DatabaseSchemaCompatibility::Corrupt | DatabaseSchemaCompatibility::NotInspected => {
            (None, false, None)
        }
    };
    DatabaseSchemaReport {
        runtime_version: SCHEMA_VERSION,
        stored_version,
        compatibility,
        migration_required,
        migration_supported,
        migration_steps_remaining,
    }
}

/// Return a deterministic runtime version and compile-option identity.
fn sqlite_runtime_report() -> DbResult<SqliteRuntimeReport> {
    let connection = Connection::open_in_memory()?;
    let mut statement = connection.prepare(
        "SELECT DISTINCT compile_options FROM pragma_compile_options ORDER BY compile_options",
    )?;
    let mut rows = statement.query([])?;
    let mut count = 0_u32;
    let mut hasher = blake3::Hasher::new();
    hasher.update(COMPILE_OPTIONS_DIGEST_DOMAIN);
    while let Some(row) = rows.next()? {
        let option = row.get::<_, String>(0)?;
        if option.len() > MAX_COMPILE_OPTION_BYTES {
            return Err(DbError::DatabaseOperatingProfile {
                setting: "sqlite_compile_options.option_bytes",
                expected: format!("at most {MAX_COMPILE_OPTION_BYTES}"),
                found: option.len().to_string(),
            });
        }
        count = count.checked_add(1).ok_or(DbError::GraphCountOverflow {
            field: "sqlite_compile_options.count",
            value: u64::from(u32::MAX) + 1,
        })?;
        if count > MAX_COMPILE_OPTIONS {
            return Err(DbError::DatabaseOperatingProfile {
                setting: "sqlite_compile_options.count",
                expected: format!("at most {MAX_COMPILE_OPTIONS}"),
                found: count.to_string(),
            });
        }
        let length =
            u64::try_from(option.len()).map_err(|_source| DbError::GraphCountOverflow {
                field: "sqlite_compile_options.option_bytes",
                value: u64::MAX,
            })?;
        hasher.update(&length.to_le_bytes());
        hasher.update(option.as_bytes());
    }
    Ok(SqliteRuntimeReport {
        version: rusqlite::version().to_string(),
        version_number: rusqlite::version_number(),
        compile_options: SqliteCompileOptionsIdentity {
            count,
            digest: hasher.finalize().to_hex().to_string(),
        },
    })
}

/// Convert known schema validation failures into a closed unhealthy state.
fn classify_schema_error(error: DbError) -> DbResult<DatabaseSchemaCompatibility> {
    match &error {
        DbError::IntegrityCheck { .. } => Ok(DatabaseSchemaCompatibility::Corrupt),
        DbError::Sqlite(error)
            if matches!(
                error.sqlite_error_code(),
                Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase)
            ) =>
        {
            Ok(DatabaseSchemaCompatibility::Corrupt)
        }
        DbError::SchemaVersion { .. }
        | DbError::SchemaVersionMissing
        | DbError::SchemaShape { .. }
        | DbError::ProjectRootMissing
        | DbError::ProjectInstanceIdentityMissing
        | DbError::ProjectRootTransitionChanged { .. }
        | DbError::InvalidInteger { .. } => Ok(DatabaseSchemaCompatibility::Incompatible),
        _ => Err(error),
    }
}

/// Retain a stored unsupported version without opening an incompatible schema further.
const fn stored_version_from_error(error: &DbError) -> Option<i64> {
    match error {
        DbError::SchemaVersion { found, .. } => Some(*found),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{PREVIOUS_SCHEMA_VERSION, SCHEMA_VERSION_KEY};
    use crate::{INDEX_PUBLICATION_FINGERPRINT_KEY, IndexPublicationState, set_metadata};
    use projectatlas_core::IndexGeneration;
    use projectatlas_core::graph::{
        CoverageRecord, EntitySelector, GraphEntity, GraphIdentityText,
    };
    use std::error::Error;
    use std::fmt::Debug;
    use std::fs;
    use std::io;
    use tempfile::tempdir;

    #[test]
    fn reports_missing_current_predecessor_and_unhealthy_schema_without_mutation()
    -> Result<(), Box<dyn Error>> {
        let temp = tempdir()?;
        let missing = temp.path().join("missing.db");
        let first = database_settings_report(&missing)?;
        let second = database_settings_report(&missing)?;
        require_eq(
            &first.schema.compatibility,
            &DatabaseSchemaCompatibility::Missing,
            "missing database compatibility",
        )?;
        require(
            !missing.exists(),
            "settings report created a missing database",
        )?;
        require_eq(
            &first.sqlite,
            &second.sqlite,
            "deterministic SQLite identity",
        )?;
        require(
            first.sqlite.compile_options.count > 0,
            "SQLite compile option count is empty",
        )?;
        require_eq(
            &first.sqlite.compile_options.digest.len(),
            &64,
            "SQLite compile option digest length",
        )?;

        let root = temp.path().join("project");
        fs::create_dir(&root)?;
        let current_path = temp.path().join("current.db");
        drop(AtlasStore::open_for_project(&current_path, &root)?);
        let current_before = fs::read(&current_path)?;
        let current = database_settings_report(&current_path)?;
        require_eq(
            &current.schema,
            &DatabaseSchemaReport {
                runtime_version: SCHEMA_VERSION,
                stored_version: Some(SCHEMA_VERSION),
                compatibility: DatabaseSchemaCompatibility::Current,
                migration_required: Some(false),
                migration_supported: true,
                migration_steps_remaining: Some(0),
            },
            "current schema report",
        )?;
        require_eq(
            &fs::read(&current_path)?,
            &current_before,
            "current database bytes",
        )?;

        let predecessor_path = temp.path().join("predecessor.db");
        let predecessor = Connection::open(&predecessor_path)?;
        crate::schema::create_released_schema_eight(&predecessor)?;
        set_metadata(
            &predecessor,
            SCHEMA_VERSION_KEY,
            &PREVIOUS_SCHEMA_VERSION.to_string(),
        )?;
        drop(predecessor);
        let predecessor_before = fs::read(&predecessor_path)?;
        let predecessor = database_settings_report(&predecessor_path)?;
        require_eq(
            &predecessor.schema.compatibility,
            &DatabaseSchemaCompatibility::SupportedPredecessor,
            "predecessor schema compatibility",
        )?;
        require_eq(
            &predecessor.schema.stored_version,
            &Some(PREVIOUS_SCHEMA_VERSION),
            "predecessor stored schema version",
        )?;
        require_eq(
            &predecessor.schema.migration_required,
            &Some(true),
            "predecessor migration requirement",
        )?;
        require(
            predecessor.schema.migration_supported,
            "migration not supported",
        )?;
        require_eq(
            &predecessor.schema.migration_steps_remaining,
            &Some(8),
            "predecessor migration steps",
        )?;
        require_eq(
            &fs::read(&predecessor_path)?,
            &predecessor_before,
            "predecessor database bytes",
        )?;

        let resolution_path = temp.path().join("resolution-predecessor.db");
        let resolution_store = AtlasStore::open_for_project(&resolution_path, &root)?;
        resolution_store.connection.execute_batch(
            "ALTER TABLE source_parse_metadata RENAME TO source_parse_metadata_current;
             CREATE TABLE source_parse_metadata (
                 path TEXT PRIMARY KEY,
                 language TEXT,
                 parser TEXT NOT NULL,
                 symbol_count INTEGER NOT NULL,
                 relation_count INTEGER NOT NULL,
                 updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             INSERT INTO source_parse_metadata(
                 path, language, parser, symbol_count, relation_count, updated_at
             )
             SELECT path, language, fact_parser, symbol_count, relation_count, updated_at
               FROM source_parse_metadata_current;
             DROP TABLE source_parse_metadata_current;
             CREATE INDEX idx_source_parse_metadata_parser
                 ON source_parse_metadata(parser);
             DROP TABLE file_text_fts;",
        )?;
        set_metadata(&resolution_store.connection, SCHEMA_VERSION_KEY, "12")?;
        drop(resolution_store);
        let resolution_before = fs::read(&resolution_path)?;
        let resolution = database_settings_report(&resolution_path)?;
        require_eq(
            &resolution.schema.compatibility,
            &DatabaseSchemaCompatibility::SupportedPredecessor,
            "schema-12 compatibility",
        )?;
        require_eq(
            &resolution.schema.migration_steps_remaining,
            &Some(4),
            "schema-12 migration steps",
        )?;
        require_eq(
            &fs::read(&resolution_path)?,
            &resolution_before,
            "schema-12 database bytes",
        )?;

        let incompatible_path = temp.path().join("incompatible.db");
        let incompatible = Connection::open(&incompatible_path)?;
        incompatible.execute_batch("CREATE TABLE unrelated(value TEXT NOT NULL)")?;
        drop(incompatible);
        let incompatible = database_settings_report(&incompatible_path)?;
        require_eq(
            &incompatible.schema.compatibility,
            &DatabaseSchemaCompatibility::Incompatible,
            "incompatible schema state",
        )?;

        let corrupt_path = temp.path().join("corrupt.db");
        fs::write(&corrupt_path, b"not a sqlite database")?;
        let corrupt = database_settings_report(&corrupt_path)?;
        require_eq(
            &corrupt.schema.compatibility,
            &DatabaseSchemaCompatibility::Corrupt,
            "corrupt schema state",
        )?;
        Ok(())
    }

    #[test]
    fn reports_publication_and_bounded_content_free_coverage() -> Result<(), Box<dyn Error>> {
        const VALID_FINGERPRINT: &str =
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        const PRIVATE_FINGERPRINT_SENTINEL: &str = "private-publication-fingerprint-sentinel";
        let temp = tempdir()?;
        let root = temp.path().join("sensitive-project-name");
        fs::create_dir(&root)?;
        let database = temp.path().join("atlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let project = store.captured_project_binding()?.project_instance_id;
        let generation = IndexGeneration::new(1);
        let entity = GraphEntity::new(project, EntitySelector::Project, generation)?;
        let coverage = (0..=COVERAGE_SAMPLE_LIMIT)
            .enumerate()
            .map(|(index, _)| {
                CoverageRecord::new(
                    CoverageScope::Path {
                        path: RepositoryNodePath::new(Path::new(&format!(
                            "src/incomplete-{index}.rs"
                        )))?,
                    },
                    None,
                    CoverageState::Partial,
                    1,
                    1,
                    generation,
                    Some(GraphIdentityText::new(
                        "coverage incomplete after row limit",
                    )?),
                    Some(GraphLimitKind::Rows),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut publication = store.begin_index_publication(VALID_FINGERPRINT)?;
        publication.replace_repository_graph(project, &[entity], &[], &[], &coverage)?;
        publication.complete()?;
        drop(store);

        let before = fs::read(&database)?;
        let report = database_settings_report(&database)?;
        require_eq(&fs::read(&database)?, &before, "published database bytes")?;
        let publication = report.publication.as_ref().ok_or("publication missing")?;
        require_eq(
            &publication.state,
            &IndexPublicationState::Complete,
            "publication state",
        )?;
        require_eq(
            &publication.generation,
            &generation,
            "publication generation",
        )?;
        require_eq(
            &publication.contract_fingerprint.as_deref(),
            &Some(VALID_FINGERPRINT),
            "publication contract fingerprint",
        )?;
        require_eq(
            &publication.contract_fingerprint_state,
            &DatabasePublicationContractState::Valid,
            "publication contract fingerprint state",
        )?;
        let coverage = report.coverage.as_ref().ok_or("coverage missing")?;
        require_eq(
            &coverage.sample.len(),
            &(COVERAGE_SAMPLE_LIMIT as usize),
            "coverage sample length",
        )?;
        require(coverage.truncated, "coverage sample was not truncated")?;
        require_eq(
            &coverage.known_total,
            &(COVERAGE_SAMPLE_LIMIT as usize + 1),
            "coverage lower bound",
        )?;
        require_eq(
            &coverage.total_state,
            &DatabaseCoverageTotalState::AtLeast,
            "coverage total state",
        )?;

        let encoded = serde_json::to_string(&report)?;
        require(
            !encoded.contains("sensitive-project-name"),
            "serialized report leaked project path",
        )?;
        require(
            encoded.contains("coverage incomplete after row limit"),
            "serialized report omitted the bounded actionable coverage reason",
        )?;
        require(
            !encoded.contains(database.to_string_lossy().as_ref()),
            "serialized report leaked database path",
        )?;
        let value = serde_json::from_str::<serde_json::Value>(&encoded)?;
        let compile_options = value["sqlite"]["compile_options"]
            .as_object()
            .ok_or("compile-options identity is not an object")?;
        require_eq(&compile_options.len(), &2, "compile option identity fields")?;
        require(
            compile_options.contains_key("count"),
            "compile option count is missing",
        )?;
        require(
            compile_options.contains_key("digest"),
            "compile option digest is missing",
        )?;

        let store = AtlasStore::open_for_project(&database, &root)?;
        set_metadata(
            &store.connection,
            INDEX_PUBLICATION_FINGERPRINT_KEY,
            PRIVATE_FINGERPRINT_SENTINEL,
        )?;
        drop(store);
        let invalid = database_settings_report(&database)?;
        let invalid_publication = invalid
            .publication
            .as_ref()
            .ok_or("invalid publication state missing")?;
        require_eq(
            &invalid_publication.contract_fingerprint_state,
            &DatabasePublicationContractState::Invalid,
            "invalid publication contract fingerprint state",
        )?;
        require_eq(
            &invalid_publication.contract_fingerprint,
            &None,
            "invalid publication contract fingerprint",
        )?;
        require_eq(
            &invalid.coverage,
            &None,
            "coverage from invalid publication metadata",
        )?;
        require(
            !serde_json::to_string(&invalid)?.contains(PRIVATE_FINGERPRINT_SENTINEL),
            "serialized report leaked invalid publication metadata",
        )?;
        Ok(())
    }

    #[test]
    fn coverage_query_uses_the_relation_state_index() -> Result<(), Box<dyn Error>> {
        let store = AtlasStore::in_memory()?;
        let mut statement = store.connection.prepare(
            "EXPLAIN QUERY PLAN
             SELECT scope_kind, scope_path, state, total, covered, omitted, reason, reached_limit
               FROM graph_coverage INDEXED BY idx_graph_coverage_relation_state
              WHERE relation_scope IS NULL
                AND relation_kind IS NULL
                AND state IN ('partial', 'failed', 'ignored', 'oversized',
                              'quarantined', 'stale')
              ORDER BY state, id
              LIMIT ?1",
        )?;
        let plan = statement
            .query_map([i64::from(COVERAGE_SAMPLE_LIMIT) + 1], |row| {
                row.get::<_, String>(3)
            })?
            .collect::<Result<Vec<_>, _>>()?
            .join("; ");
        require(
            plan.contains("idx_graph_coverage_relation_state"),
            &format!("coverage plan did not use relation-state index: {plan}"),
        )?;
        require(
            !plan.contains("TEMP B-TREE"),
            &format!("coverage plan required a temporary sort: {plan}"),
        )?;
        Ok(())
    }

    /// Return a test error instead of panicking in result-returning tests.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }

    /// Compare one test value without panicking in result-returning tests.
    fn require_eq<T: Debug + PartialEq>(
        actual: &T,
        expected: &T,
        label: &str,
    ) -> Result<(), Box<dyn Error>> {
        if actual == expected {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "{label} mismatch: expected {expected:?}, found {actual:?}"
            ))
            .into())
        }
    }
}
