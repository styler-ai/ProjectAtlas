//! Purpose: Persist `ProjectAtlas` 3 indexes in `SQLite`.

mod content_classification;
mod derived_snapshot;
mod diagnostics;
mod hydration;
mod project_identity;
mod repository_graph;
mod schema;
mod sqlite_profile;
mod telemetry;
mod worktree_registry;

pub use content_classification::{
    FileContentClassification, FileContentClassificationPage,
    MAX_FILE_CONTENT_CLASSIFICATION_PAGE_ROWS, MAX_FILE_CONTENT_CLASSIFICATION_PATHS,
};
pub use derived_snapshot::{
    DerivedGraphSnapshot, DerivedGraphSnapshotImport, DerivedGraphSnapshotMetadata,
    DerivedSnapshotContent, MAX_DERIVED_SNAPSHOT_JSON_BYTES,
};
pub use diagnostics::{
    DatabaseCoverageSample, DatabaseCoverageSummary, DatabaseCoverageTotalState,
    DatabaseFilesystemSupport, DatabaseOperatingProfileReport, DatabasePublicationContractState,
    DatabasePublicationReport, DatabaseSchemaCompatibility, DatabaseSchemaReport,
    DatabaseSettingsReport, SqliteCompileOptionsIdentity, SqliteRuntimeReport,
    database_settings_report,
};
pub use hydration::{
    PreparedWorktreeHydrationCandidate, WorktreeHydrationActivation, WorktreeHydrationCandidate,
};
pub use project_identity::{ProjectRootTransition, ProjectRootTransitionResult};
pub use repository_graph::{
    MAX_REPOSITORY_GRAPH_FRONTIER, RepositoryAffectedSourceFootprint, RepositoryCoverageQuery,
    RepositoryCoverageRow, RepositoryGraphAdjacencyContinuation, RepositoryGraphAdjacencyPage,
    RepositoryGraphAdjacencyReadPage, RepositoryGraphAdjacencyRow,
    RepositoryGraphClassifiedRelationRow, RepositoryGraphDirection, RepositoryGraphPage,
    RepositoryGraphReadBatch, RepositoryGraphReadBudget, RepositoryGraphReadPage,
    RepositoryGraphReadPages, RepositoryGraphReadWork, RepositoryGraphRelationQuery,
    RepositoryGraphRelationRow, RepositoryGraphStagingGuard, RepositoryNavigationConnections,
    RepositoryNavigationNode, RepositoryResolutionCandidate,
};
pub use sqlite_profile::validate_database_location;
pub use telemetry::{
    PlannerStatisticsPolicy, PlannerStatisticsState, SpillCleanupState, TelemetryCheckpointState,
    TelemetryRetentionPolicy, TelemetryRetentionState, WorktreeUsageSnapshot,
    WorktreeUsageSyncState,
};
pub use worktree_registry::{
    ActiveWorktreeRegistrationGuard, MAIN_WORKTREE_ALIAS, MAX_WORKTREE_ALIAS_BYTES, WorktreeAlias,
    WorktreeRegistration, WorktreeRegistrationState,
};

use blake3::Hasher;
use projectatlas_core::graph::{GraphContractError, ProjectInstanceId};
use projectatlas_core::health::{
    CATEGORY_DUPLICATE_PURPOSE, CATEGORY_MISSING_PURPOSE, CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED,
    CATEGORY_REPEATED_TEMPORARY_FOLDER, CATEGORY_STALE_PURPOSE, CATEGORY_SUGGESTED_PURPOSE_REVIEW,
    HealthFinding, MESSAGE_MISSING_PURPOSE, MESSAGE_PURPOSE_AGENT_REVIEW_REQUIRED,
    MESSAGE_STALE_PURPOSE, MESSAGE_SUGGESTED_PURPOSE_REVIEW, RECOMMENDATION_DUPLICATE_PURPOSE,
    RECOMMENDATION_MISSING_PURPOSE_QUEUE, RECOMMENDATION_PURPOSE_AGENT_REVIEW_REQUIRED,
    RECOMMENDATION_REPEATED_TEMPORARY_FOLDER, RECOMMENDATION_STALE_PURPOSE,
    RECOMMENDATION_SUGGESTED_PURPOSE_REVIEW_QUEUE, STRUCTURAL_HEALTH_CATEGORIES, Severity,
    TEMP_FOLDER_BUCKETS, finding_id,
};
use projectatlas_core::language::{ContentClassification, ContentSelection};
use projectatlas_core::symbols::{
    CodeSymbol, ParserKind, RelationKind, SourceParseMetadata, SymbolGraph, SymbolKind,
    SymbolRelation, SymbolSourceSelector,
};
use projectatlas_core::telemetry::{
    TelemetryContractError, TokenOverview, TokenTrendReport, TokenTrendWindow, UsageEvent,
    UsageInstanceId, UsageInstanceOwner,
};
use projectatlas_core::{
    AGENT_REVIEWED_SOURCE_VALUES, CanonicalProjectRoot, CoreError, HIGH_IMPACT_FILE_NAMES,
    HIGH_IMPACT_PATH_PREFIXES, HIGH_IMPACT_PATH_SEGMENTS, IndexGeneration, IndexWorkControl,
    IndexWorkFailure, IndexWorkStage, IndexedNode, LEGACY_HUMAN_PURPOSE_SOURCE, Node, NodeKind,
    Overview, Purpose, PurposeSource, PurposeStatus, normalize_native_path_display,
    normalize_repo_path_prefix,
};
use rusqlite::types::Value;
use rusqlite::{
    Connection, ErrorCode, OpenFlags, OptionalExtension, TransactionBehavior, params,
    params_from_iter,
};
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::num::{ParseIntError, TryFromIntError};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

use schema::{
    FILE_TEXT_FTS_PROJECTION_REVISION_KEY, FILE_TEXT_FTS_SOURCE_REVISION_KEY,
    INDEX_PUBLICATION_FINGERPRINT_KEY, INDEX_PUBLICATION_GENERATION_KEY,
    INDEX_PUBLICATION_STATE_KEY, PROJECT_ROOT_KEY, SchemaState,
};
#[cfg(test)]
use schema::{PREVIOUS_SCHEMA_VERSION, SCHEMA_VERSION, SCHEMA_VERSION_KEY, sqlite_sidecar_path};
use sqlite_profile::{
    DatabaseLocation, JournalModePolicy, SQLITE_BUSY_TIMEOUT, open_writable_connection,
};

/// Maximum persisted text for denormalized symbol-name search summaries.
const MAX_SYMBOL_SEARCH_SUMMARY_CHARS: usize = 16_000;
/// Publication acquisition fails fast so callers must restage after contention.
const SQLITE_PUBLICATION_ACQUIRE_TIMEOUT: Duration = Duration::ZERO;
/// Ancillary telemetry must not delay a valid navigation result under contention.
const SQLITE_TELEMETRY_BUSY_TIMEOUT: Duration = Duration::from_millis(25);
/// Maximum paths admitted to one purpose-curation hydration statement.
pub const MAX_PURPOSE_CURATION_BATCH_ROWS: usize = 200;
/// `SQLite` schema version supported by this runtime.
pub const CURRENT_SCHEMA_VERSION: i64 = schema::SCHEMA_VERSION;
/// Maximum FTS candidates decoded for one exact-verification request.
pub const MAX_FILE_TEXT_FTS_CANDIDATES: usize = 4_096;
/// Path-indexed metadata cursor for one exact path-or-descendant fallback scope.
const FILE_TEXT_FALLBACK_SCOPED_METADATA_SQL: &str = "
    SELECT text.path, text.content_hash, text.byte_count, text.line_count,
           classification.classification
    FROM file_texts AS text
    LEFT JOIN file_content_classifications AS classification
      ON classification.path = text.path
    WHERE text.path = ?1 OR (text.path >= ?2 AND text.path < ?3)
    ORDER BY text.path
";
/// Metadata cursor used when a fallback glob has no safe fixed prefix.
const FILE_TEXT_FALLBACK_ALL_METADATA_SQL: &str = "
    SELECT text.path, text.content_hash, text.byte_count, text.line_count,
           classification.classification
    FROM file_texts AS text
    LEFT JOIN file_content_classifications AS classification
      ON classification.path = text.path
    ORDER BY text.path
";
/// Exact authoritative content hydration after service-owned admission.
const FILE_TEXT_FALLBACK_CONTENT_SQL: &str = "SELECT content FROM file_texts WHERE path = ?1";
/// Maximum UTF-8 bytes retained in one host-owned purpose task label.
const MAX_PURPOSE_CURATION_TASK_BYTES: usize = 256;
/// Domain separator for deterministic purpose-curation item work keys.
const PURPOSE_CURATION_ITEM_KEY_DOMAIN: &str = "projectatlas:purpose-curation:item:v1";
/// Domain separator for deterministic purpose-curation row-state tokens.
const PURPOSE_CURATION_STATE_TOKEN_DOMAIN: &str = "projectatlas:purpose-curation:state:v1";
/// Domain separator for deterministic purpose-curation batch work keys.
const PURPOSE_CURATION_BATCH_KEY_DOMAIN: &str = "projectatlas:purpose-curation:batch:v1";
/// Monotonic metadata revision for accepted authored-purpose mutations.
const AUTHORED_PURPOSE_REVISION_KEY: &str = "purpose.authored_revision";
/// Select create capability only for a path proven absent by preflight.
fn writable_open_flags(state: SchemaState, database_exists: bool) -> OpenFlags {
    match (state, database_exists) {
        (SchemaState::Fresh, false) => {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
        }
        (SchemaState::Fresh | SchemaState::Current | SchemaState::UpgradeRequired, true)
        | (SchemaState::Current | SchemaState::UpgradeRequired, false) => {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        }
    }
}

/// Establish WAL only while creating or upgrading an admitted database.
const fn writable_journal_policy(state: SchemaState) -> JournalModePolicy {
    match state {
        SchemaState::Current => JournalModePolicy::RequireWal,
        SchemaState::Fresh | SchemaState::UpgradeRequired => JournalModePolicy::EnsureWal,
    }
}

/// Database-layer error type.
#[derive(Debug, Error)]
pub enum DbError {
    /// `SQLite` operation failed.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// A live project database is located on a known unsupported filesystem.
    #[error(
        "database path {path:?} is on an unsupported filesystem (mount: {mount_point:?}, type: {filesystem_type:?}); live SQLite WAL requires supported local storage"
    )]
    DatabaseFilesystemUnsupported {
        /// Database path rejected before writable access.
        path: PathBuf,
        /// Resolved mount point when one was safely available.
        mount_point: Option<PathBuf>,
        /// Normalized filesystem type when one was safely available.
        filesystem_type: Option<String>,
    },
    /// Local WAL-safe filesystem placement could not be proved.
    #[error(
        "database path {path:?} has uncertain filesystem placement (mount: {mount_point:?}, type: {filesystem_type:?}): {reason}"
    )]
    DatabaseFilesystemUncertain {
        /// Database path rejected before writable access.
        path: PathBuf,
        /// Resolved mount point when one was safely available.
        mount_point: Option<PathBuf>,
        /// Normalized filesystem type when one was safely available.
        filesystem_type: Option<String>,
        /// Bounded reason the local profile could not be established.
        reason: String,
    },
    /// `SQLite` did not retain a required connection operating-profile value.
    #[error("SQLite operating profile mismatch for {setting}: expected {expected}, found {found}")]
    DatabaseOperatingProfile {
        /// Connection or durable setting that did not match.
        setting: &'static str,
        /// Required value.
        expected: String,
        /// Observed value.
        found: String,
    },
    /// A persisted or requested graph value violates the typed domain contract.
    #[error("repository graph contract error: {0}")]
    GraphContract(#[from] GraphContractError),
    /// A telemetry identity violates its typed domain contract.
    #[error("telemetry contract error: {0}")]
    TelemetryContract(#[from] TelemetryContractError),
    /// A requested worktree alias violates the public registry contract.
    #[error("invalid worktree alias {alias:?}: {reason}")]
    InvalidWorktreeAlias {
        /// Rejected caller value.
        alias: String,
        /// Stable validation reason.
        reason: &'static str,
    },
    /// No active worktree registration has the requested alias.
    #[error("active worktree registration {alias:?} was not found")]
    WorktreeRegistrationNotFound {
        /// Requested normalized alias.
        alias: String,
    },
    /// A registration identity is already owned by another active worktree.
    #[error("worktree registration {field} conflicts with active value {value:?}")]
    WorktreeRegistrationConflict {
        /// Conflicting identity field.
        field: &'static str,
        /// Bounded caller-visible value.
        value: String,
    },
    /// The bounded worktree-registration catalog is full.
    #[error("worktree registration capacity {limit} is exhausted")]
    WorktreeRegistrationCapacity {
        /// Maximum active and retired registrations retained by one control atlas.
        limit: usize,
    },
    /// A registration path is not an absolute caller-validated structural identity.
    #[error("invalid worktree registration {field} path {path:?}")]
    InvalidWorktreeRegistrationPath {
        /// Path responsibility being validated.
        field: &'static str,
        /// Rejected normalized path.
        path: String,
    },
    /// A persisted worktree registration row violates its typed contract.
    #[error("invalid worktree registration row: {reason}")]
    WorktreeRegistrationRow {
        /// Stable validation failure.
        reason: &'static str,
    },
    /// A worktree aggregate snapshot belongs to a different initialized atlas.
    #[error("worktree telemetry project identity does not match registration {registration_id}")]
    WorktreeTelemetryProjectMismatch {
        /// Stable control-database registration identity.
        registration_id: i64,
    },
    /// A bounded worktree telemetry snapshot resource ceiling was exceeded.
    #[error("worktree telemetry snapshot {resource} exceeded limit {limit}: observed {observed}")]
    WorktreeTelemetrySnapshotLimit {
        /// Resource whose fixed bound was exceeded.
        resource: &'static str,
        /// Maximum admitted value.
        limit: usize,
        /// Observed rejected value.
        observed: usize,
    },
    /// One telemetry runtime instance was reused across incompatible origins.
    #[error("telemetry runtime instance has a conflicting worktree origin")]
    WorktreeTelemetryOriginConflict,
    /// A worktree hydration request violates its target-local safety contract.
    #[error("invalid worktree hydration request: {reason}")]
    WorktreeHydrationInvalid {
        /// Stable rejection reason.
        reason: &'static str,
    },
    /// A hydration destination already contains a database and cannot be replaced.
    #[error("worktree hydration destination already exists: {path:?}")]
    WorktreeHydrationDestinationExists {
        /// Existing destination preserved without mutation.
        path: PathBuf,
    },
    /// Worktree hydration could not create, clean, or activate its private candidate.
    #[error("worktree hydration I/O failed for {path:?}: {source}")]
    WorktreeHydrationIo {
        /// Candidate or destination path involved in the failure.
        path: PathBuf,
        /// Underlying filesystem failure.
        #[source]
        source: std::io::Error,
    },
    /// Online backup could not make progress within its fixed busy retry budget.
    #[error("worktree hydration backup remained busy after {attempts} attempts")]
    WorktreeHydrationBackupBusy {
        /// Consecutive busy or locked backup steps.
        attempts: usize,
    },
    /// The candidate has not completed a post-hydration source reconciliation.
    #[error(
        "worktree hydration candidate is not reconciled: baseline generation {baseline}, found {found}"
    )]
    WorktreeHydrationNotReconciled {
        /// Generation published by baseline rebinding.
        baseline: IndexGeneration,
        /// Current complete generation, or zero when unavailable.
        found: IndexGeneration,
    },
    /// Persisted graph project identity differs from the selected identity.
    #[error(
        "repository graph project identity {found} does not match selected identity {expected}"
    )]
    GraphProjectIdentityMismatch {
        /// Project identity selected by the caller.
        expected: String,
        /// Project identity stored in the database.
        found: String,
    },
    /// A graph query requires one complete nonzero publication generation.
    #[error("repository graph is unavailable without a complete published generation")]
    GraphPublicationUnavailable,
    /// A normalized graph row has an impossible column shape.
    #[error("invalid {table} row: {reason}")]
    GraphRowShape {
        /// Owning normalized graph table.
        table: &'static str,
        /// Stable shape diagnostic.
        reason: &'static str,
    },
    /// A derived graph snapshot violates its bounded portable contract.
    #[error("invalid derived graph snapshot: {reason}")]
    DerivedSnapshotInvalid {
        /// Stable validation failure.
        reason: &'static str,
    },
    /// A derived graph snapshot exceeded one declared resource ceiling.
    #[error(
        "derived graph snapshot {resource} exceeds the limit: found {found}, maximum {maximum}"
    )]
    DerivedSnapshotLimit {
        /// Bounded resource that was exceeded.
        resource: &'static str,
        /// Observed amount.
        found: u64,
        /// Maximum admitted amount.
        maximum: u64,
    },
    /// A private snapshot capture could not be created or cleaned up.
    #[error("derived graph snapshot I/O failed for {path:?}: {source}")]
    DerivedSnapshotIo {
        /// Private temporary path involved in the operation.
        path: PathBuf,
        /// Underlying filesystem failure.
        #[source]
        source: std::io::Error,
    },
    /// A portable snapshot payload was not valid JSON.
    #[error("derived graph snapshot JSON is invalid: {0}")]
    DerivedSnapshotJson(#[from] serde_json::Error),
    /// Persisted per-file symbol rows do not match their owning parser metadata.
    #[error("invalid persisted symbol graph for {path:?}: {reason}")]
    SymbolGraphRowShape {
        /// Repository path whose persisted graph is inconsistent.
        path: String,
        /// Stable shape diagnostic.
        reason: &'static str,
    },
    /// Equal scoped resolution digests retained different canonical witnesses.
    #[error("resolution-key collision in {domain} for digest {digest:?}")]
    ResolutionKeyCollision {
        /// Closed resolution domain containing the conflict.
        domain: &'static str,
        /// Fixed digest shared by conflicting witnesses.
        digest: [u8; 32],
    },
    /// A normalized binary graph field has the wrong width.
    #[error("invalid {field} blob length {found}; expected {expected}")]
    InvalidBlobLength {
        /// Owning database field.
        field: &'static str,
        /// Required fixed byte width.
        expected: usize,
        /// Observed byte width.
        found: usize,
    },
    /// An unsigned graph count cannot be represented by `SQLite`.
    #[error("graph count for {field} exceeds SQLite integer range: {value}")]
    GraphCountOverflow {
        /// Owning database field.
        field: &'static str,
        /// Unsigned domain value that exceeded the database range.
        value: u64,
    },
    /// Schema version is not supported.
    #[error("unsupported schema version {found}, expected {expected}")]
    SchemaVersion {
        /// Version found in database.
        found: i64,
        /// Expected version.
        expected: i64,
    },
    /// An existing database has no durable schema version.
    #[error("existing database is missing schema_version metadata")]
    SchemaVersionMissing,
    /// A durable `SQLite` object does not match the supported schema contract.
    #[error("incompatible schema object {object:?}: expected {expected}, found {found}")]
    SchemaShape {
        /// Table, index, or column whose shape is incompatible.
        object: String,
        /// Required `SQLite` object kind.
        expected: String,
        /// Observed `SQLite` object kind.
        found: String,
    },
    /// `SQLite` integrity validation failed before migration.
    #[error("database integrity check failed: {message}")]
    IntegrityCheck {
        /// Bounded `SQLite` integrity diagnostic.
        message: String,
    },
    /// A migration did not reach the supported current schema.
    #[error("schema migration did not reach expected version {expected}")]
    SchemaPostcondition {
        /// Version required after migration.
        expected: i64,
    },
    /// A source-owned database has no durable project identity.
    #[error("existing database is missing project_root metadata")]
    ProjectRootMissing,
    /// A source-owned database belongs to another project root.
    #[error("database project root {found:?} does not match selected root {expected:?}")]
    ProjectRootMismatch {
        /// Canonical root selected by the caller.
        expected: String,
        /// Durable root recorded in `SQLite`.
        found: String,
    },
    /// A native project-root identity or its lossless codec is invalid.
    #[error("project-root identity error: {0}")]
    ProjectRootIdentity(#[from] CoreError),
    /// A current bound database has no lossless native project-root identity.
    #[error("bound project database is missing canonical project-root identity")]
    ProjectRootIdentityMissing,
    /// A root transition destination is not an absolute existing directory.
    #[error("invalid project root transition destination {root:?}: {source}")]
    ProjectRootDestinationInvalid {
        /// Destination rejected before database preflight or mutation.
        root: String,
        /// Filesystem or input failure that made the destination invalid.
        #[source]
        source: std::io::Error,
    },
    /// A move or detach requires a previously bound project root.
    #[error("project root transition requires an existing bound root")]
    ProjectRootTransitionRequiresExistingRoot,
    /// A move must select a destination different from the old root.
    #[error("project root move destination {root:?} matches the existing root")]
    ProjectRootTransitionRequiresDifferentRoot {
        /// Root that was selected as both source and destination.
        root: String,
    },
    /// A verified move cannot preserve identity while the old root still exists.
    #[error("project root {root:?} still exists; use detach for an independent copy")]
    ProjectRootStillPresent {
        /// Recorded old root that remains accessible.
        root: String,
    },
    /// Filesystem state could not prove that a move's old root is absent.
    #[error("cannot prove project root {root:?} is absent: {source}")]
    ProjectRootAbsenceUncertain {
        /// Recorded old root whose state is uncertain.
        root: String,
        /// Filesystem failure that prevents an absence proof.
        #[source]
        source: std::io::Error,
    },
    /// Root or identity state changed after transition preflight.
    #[error(
        "project root transition state changed: root {expected_root:?} -> {found_root:?}, identity {expected_identity:?} -> {found_identity:?}"
    )]
    ProjectRootTransitionChanged {
        /// Root captured by read-only preflight.
        expected_root: Option<String>,
        /// Root observed inside the write transaction.
        found_root: Option<String>,
        /// Identity captured by read-only preflight.
        expected_identity: Option<String>,
        /// Identity observed inside the write transaction.
        found_identity: Option<String>,
    },
    /// A bound project database has no durable instance identity.
    #[error("bound project database is missing project instance identity")]
    ProjectInstanceIdentityMissing,
    /// `SQLite` did not yield a usable nonzero project identity.
    #[error("failed to generate a distinct nonzero project instance identity")]
    ProjectInstanceIdentityGenerationFailed,
    /// A transaction failed and the explicit rollback also failed.
    #[error("{operation}; rollback also failed: {rollback}")]
    TransactionRollback {
        /// Primary operation failure that caused rollback.
        #[source]
        operation: Box<DbError>,
        /// Secondary rollback failure retained for diagnosis.
        rollback: rusqlite::Error,
    },
    /// Publication acquisition failed and its standard busy policy was not restored.
    #[error(
        "publication writer acquisition failed: {operation}; restoring the standard busy policy also failed: {restore}"
    )]
    PublicationAcquirePolicyRestore {
        /// Writer-acquisition failure observed with fail-fast busy handling.
        #[source]
        operation: Box<rusqlite::Error>,
        /// Failure restoring the ordinary connection busy policy.
        restore: Box<rusqlite::Error>,
    },
    /// Invalid enum value read from the database.
    #[error("invalid {field} value in database: {value}")]
    InvalidEnum {
        /// Field name.
        field: &'static str,
        /// Invalid value.
        value: String,
    },
    /// A literal cannot safely use the complete-candidate FTS path.
    #[error("literal token cannot use FTS acceleration: {reason}")]
    FileTextFtsTokenUnsafe {
        /// Stable rejection reason for service fallback selection.
        reason: &'static str,
    },
    /// One FTS request exceeded the storage-owned candidate bound.
    #[error("FTS candidate request {requested} exceeds the maximum {maximum}")]
    FileTextFtsCandidateLimit {
        /// Requested exact-verification candidates.
        requested: usize,
        /// Storage-owned hard maximum.
        maximum: usize,
    },
    /// `SQLite` returned a non-finite lexical ranking score.
    #[error("invalid FTS BM25 score for {path:?}")]
    FileTextFtsScoreInvalid {
        /// Candidate path whose score was invalid.
        path: String,
    },
    /// Durable FTS synchronization metadata is absent or contradictory.
    #[error("invalid FTS synchronization state: {reason}")]
    FileTextFtsStateInvalid {
        /// Stable reason that makes the acceleration state untrustworthy.
        reason: &'static str,
    },
    /// Persisted text metadata disagrees with its authoritative UTF-8 content.
    #[error("file text {path:?} has invalid {field}: recorded {recorded}, actual {actual}")]
    FileTextMetadataMismatch {
        /// Repository-relative path owning the invalid text row.
        path: String,
        /// Metadata field that disagrees with content.
        field: &'static str,
        /// Persisted or caller-supplied value.
        recorded: usize,
        /// Value derived from authoritative content.
        actual: usize,
    },
    /// A bounded database read observed cancellation or its deadline.
    #[error("{0}")]
    IndexWork(#[from] IndexWorkFailure),
    /// Count value from `SQLite` could not fit its owning unsigned domain type.
    #[error("invalid count for {field}: {value}")]
    InvalidCount {
        /// Count field name.
        field: &'static str,
        /// Invalid database count.
        value: i64,
        /// Source conversion error.
        source: TryFromIntError,
    },
    /// Integer metadata could not be parsed without losing its source error.
    #[error("invalid integer metadata for {field}: {value:?}: {source}")]
    InvalidInteger {
        /// Metadata field name.
        field: &'static str,
        /// Invalid persisted value.
        value: String,
        /// Source parse failure.
        source: ParseIntError,
    },
    /// Unsigned integer metadata could not advance without overflow.
    #[error("integer metadata for {field} overflowed at {value}")]
    IntegerMetadataOverflow {
        /// Metadata key owning the monotonic value.
        field: &'static str,
        /// Largest persisted value that could not advance.
        value: u64,
    },
    /// A caller supplied a path that is not in the current index.
    #[error("path {path:?} is not indexed; run scan, fix the path, or choose an indexed path")]
    PathNotIndexed {
        /// Repository-relative path.
        path: String,
    },
    /// One classification write/read batch exceeded its fixed path ceiling.
    #[error("file classification batch requested {requested} paths; maximum is {maximum}")]
    FileContentClassificationBatchTooLarge {
        /// Unique paths requested.
        requested: usize,
        /// Storage-owned hard maximum.
        maximum: usize,
    },
    /// One classification batch repeated an exact path.
    #[error("file classification batch repeats path {path:?}")]
    FileContentClassificationDuplicatePath {
        /// Repeated repository-relative path.
        path: String,
    },
    /// A current admitted file has no classification in the active transaction.
    #[error("current file {path:?} has no content classification")]
    FileContentClassificationMissing {
        /// Current repository-relative file path.
        path: String,
    },
    /// A classification row outlived current file ownership.
    #[error("content classification for {path:?} does not belong to a current file")]
    FileContentClassificationNotCurrent {
        /// Stale or non-file repository-relative path.
        path: String,
    },
    /// One classification/path page requested an invalid row limit.
    #[error("file classification page requested {requested} rows; maximum is {maximum}")]
    FileContentClassificationLimit {
        /// Requested page rows.
        requested: u32,
        /// Storage-owned hard maximum.
        maximum: u32,
    },
    /// A purpose-curation task label is blank, unsafe, or too large.
    #[error("invalid purpose-curation task label: {reason}")]
    PurposeCurationTaskInvalid {
        /// Stable validation reason.
        reason: &'static str,
    },
    /// A purpose-curation hydration request exceeds the bounded statement size.
    #[error("purpose-curation batch requested {requested} paths; maximum is {maximum}")]
    PurposeCurationBatchTooLarge {
        /// Number of unique paths requested.
        requested: usize,
        /// Maximum paths admitted to one prepared set query.
        maximum: usize,
    },
    /// A caller attempted to resolve a health finding that is not currently active.
    #[error(
        "health finding {finding_id:?} with category {category:?} and path {path:?} is not active; run health-check and use an exact finding id/path/category"
    )]
    HealthFindingNotActive {
        /// Requested finding id.
        finding_id: String,
        /// Requested category.
        category: String,
        /// Requested primary path.
        path: String,
    },
    /// A projection-only refresh no longer matches the established index contract.
    #[error("index publication contract changed during projection refresh")]
    PublicationContractChanged,
    /// Prepared publication work was based on a generation that is no longer current.
    #[error("index publication base generation changed: expected {expected}, found {found}")]
    PublicationBaseGenerationChanged {
        /// Complete generation used to prepare the publication batch.
        expected: IndexGeneration,
        /// Complete generation observed after reserving the writer transaction.
        found: IndexGeneration,
    },
    /// A complete index generation cannot advance any further.
    #[error("index publication generation overflowed")]
    PublicationGenerationOverflow,
    /// A full scan replacement has not removed its remaining absent projections.
    #[error("index publication cannot complete before scan replacement finishes")]
    ScanReplacementIncomplete,
    /// The store already has an active read snapshot.
    #[error("index read snapshot is already active on this store")]
    IndexReadSnapshotActive,
    /// A read-only store cannot locate its database for a separate telemetry write.
    #[error("read-only store has no database path for telemetry persistence")]
    TelemetryPathUnavailable,
    /// One telemetry field exceeds its declared UTF-8 byte budget.
    #[error("telemetry field {field} uses {bytes} UTF-8 bytes; limit is {limit}")]
    TelemetryFieldTooLarge {
        /// Stable field owner.
        field: &'static str,
        /// Observed UTF-8 byte count.
        bytes: usize,
        /// Maximum admitted UTF-8 byte count.
        limit: usize,
    },
    /// A telemetry retention policy bound cannot make forward progress.
    #[error("invalid telemetry retention limit for {field}: {value}")]
    TelemetryLimitInvalid {
        /// Stable retention-policy field.
        field: &'static str,
        /// Rejected policy value.
        value: usize,
    },
    /// A telemetry counter cannot be represented exactly in `SQLite`.
    #[error("telemetry integer overflow for {field}")]
    TelemetryIntegerOverflow {
        /// Stable counter owner.
        field: &'static str,
    },
    /// A runtime attempted to reuse a sealed or expired instance.
    #[error("telemetry usage instance is sealed or expired")]
    TelemetryInstanceInactive,
    /// Operating-system randomness was unavailable for optional telemetry identity creation.
    #[error("telemetry runtime identity is unavailable")]
    TelemetryIdentityUnavailable,
    /// A runtime identity was reused with incompatible durable instance metadata.
    #[error("telemetry runtime identity does not match its retained owner or caller label")]
    TelemetryInstanceMismatch,
    /// The bounded active-instance capacity is exhausted.
    #[error("telemetry active-instance capacity is exhausted")]
    TelemetryInstanceCapacity,
    /// The bounded active-baseline capacity is exhausted.
    #[error("telemetry modeled-baseline capacity is exhausted")]
    TelemetryBaselineCapacity,
    /// A compact modeled-baseline key collided with different witness material.
    #[error("telemetry modeled-baseline key collided with different witness material")]
    TelemetryBaselineCollision,
}

impl DbError {
    /// Return a schema version with a complete migration route owned by this runtime.
    #[must_use]
    pub fn supported_schema_migration(&self) -> Option<(i64, i64, u32)> {
        match self {
            Self::SchemaVersion { found, expected } => schema::migration_steps_remaining(*found)
                .filter(|steps| *steps > 0)
                .map(|steps| (*found, *expected, steps)),
            _ => None,
        }
    }

    /// Return a schema version rejected by this runtime's migration inventory.
    #[must_use]
    pub fn unsupported_schema_version(&self) -> Option<(i64, i64)> {
        match self {
            Self::SchemaVersion { found, expected }
                if schema::migration_steps_remaining(*found).is_none() =>
            {
                Some((*found, *expected))
            }
            _ => None,
        }
    }

    /// Return whether a derived-index write could not proceed without implying
    /// corruption, schema drift, or an identity-contract failure.
    #[must_use]
    pub fn is_write_unavailable(&self) -> bool {
        match self {
            Self::Sqlite(error) => matches!(
                error.sqlite_error_code(),
                Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked | ErrorCode::ReadOnly)
            ),
            _ => false,
        }
    }
}

/// Convenient result alias for database operations.
pub type DbResult<T> = Result<T, DbError>;

/// Maximum exact paths admitted to one bounded symbol hydration request.
pub const MAX_SYMBOL_BATCH_PATHS: u32 = 64;
/// Maximum symbol rows admitted to one bounded hydration request.
pub const MAX_SYMBOL_BATCH_ROWS: u32 = 4_096;
/// Maximum decoded symbol bytes admitted to one bounded hydration request.
pub const MAX_SYMBOL_BATCH_DECODED_BYTES: u64 = 4 * 1_024 * 1_024;
/// Paths bound per statement, below supported `SQLite` variable ceilings.
const SYMBOL_BATCH_BIND_PATHS: usize = 48;

/// One persisted symbol with the classification of its owning file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassifiedSymbol {
    /// Persisted symbol detail.
    pub symbol: CodeSymbol,
    /// Registry-owned content role for the symbol's file.
    pub classification: ContentClassification,
}

/// Typed database envelope for one exact-path symbol batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SymbolBatchReadBudget {
    /// Maximum unique exact repository paths.
    paths: u32,
    /// Maximum decoded symbol rows.
    rows: u32,
    /// Maximum retained Rust row and owned string allocation bytes.
    decoded_bytes: u64,
}

impl SymbolBatchReadBudget {
    /// Construct one bounded exact-path symbol read envelope.
    ///
    /// # Errors
    ///
    /// Returns an error when a limit is zero or above its product ceiling.
    pub fn new(paths: u32, rows: u32, decoded_bytes: u64) -> DbResult<Self> {
        if paths == 0
            || paths > MAX_SYMBOL_BATCH_PATHS
            || rows == 0
            || rows > MAX_SYMBOL_BATCH_ROWS
            || decoded_bytes == 0
            || decoded_bytes > MAX_SYMBOL_BATCH_DECODED_BYTES
        {
            return Err(GraphContractError::InvalidLimits {
                reason: "symbol batch limit is zero or above the product ceiling",
            }
            .into());
        }
        Ok(Self {
            paths,
            rows,
            decoded_bytes,
        })
    }

    /// Return the exact-path ceiling.
    #[must_use]
    pub const fn paths(self) -> u32 {
        self.paths
    }

    /// Return the decoded-row ceiling.
    #[must_use]
    pub const fn rows(self) -> u32 {
        self.rows
    }

    /// Return the decoded-byte ceiling.
    #[must_use]
    pub const fn decoded_bytes(self) -> u64 {
        self.decoded_bytes
    }
}

/// Exact work retained by one bounded symbol batch read.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SymbolBatchReadWork {
    /// Unique exact paths supplied to `SQLite`.
    pub requested_paths: u32,
    /// Symbol rows retained after all bounds.
    pub returned_rows: u32,
    /// Retained Rust row and owned string allocation bytes after all bounds.
    pub decoded_bytes: u64,
}

/// Bounded symbols and truncation state returned for exact paths.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SymbolBatchRead {
    /// Deterministically ordered persisted symbols.
    pub rows: Vec<CodeSymbol>,
    /// Whether a path, row, or decoded-byte bound omitted rows.
    pub truncated: bool,
    /// First deterministic database limit that omitted symbol rows.
    pub reached_limit: Option<SymbolBatchReadLimit>,
    /// Exact work retained by this database read.
    pub work: SymbolBatchReadWork,
}

/// Closed database limits that can truncate an exact-path symbol batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolBatchReadLimit {
    /// More unique paths were requested than the batch admits.
    Paths,
    /// More symbol rows matched than the batch admits.
    Rows,
    /// The next symbol row crossed the decoded-byte envelope.
    DecodedBytes,
}

/// Durable state of the multi-projection derived index publication.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexPublicationState {
    /// A publisher may have changed only part of the derived index.
    Updating,
    /// Every projection completed under the recorded contract fingerprint.
    Complete,
}

impl IndexPublicationState {
    /// Return the stable `SQLite` representation.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Updating => "updating",
            Self::Complete => "complete",
        }
    }

    /// Parse the stable `SQLite` representation.
    fn from_db(value: String) -> DbResult<Self> {
        match value.as_str() {
            "updating" => Ok(Self::Updating),
            "complete" => Ok(Self::Complete),
            _ => Err(DbError::InvalidEnum {
                field: INDEX_PUBLICATION_STATE_KEY,
                value,
            }),
        }
    }
}

/// Persisted state needed to reject mixed or incompatible derived projections.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IndexPublication {
    /// Current publication state.
    pub state: IndexPublicationState,
    /// Contract fingerprint recorded by the last complete publication.
    pub contract_fingerprint: Option<String>,
    /// Monotonic generation of the last complete derived index.
    pub generation: IndexGeneration,
}

/// Publication contract applied when one atomic writer commits.
enum PublicationContract {
    /// Establish or replace the complete derived-index contract.
    Full(String),
    /// Preserve the already established complete contract.
    Projection(String),
}

/// Exclusive parent-owned atomic publication over all derived projections.
pub struct IndexPublicationGuard<'store> {
    /// Store whose connection owns the active `SQLite` write transaction.
    store: &'store mut AtlasStore,
    /// Contract behavior selected when the publication began.
    contract: PublicationContract,
    /// Generation visible before this publication began.
    previous_generation: IndexGeneration,
    /// Whether a full scan replacement still needs absent-projection cleanup.
    scan_replacement_pending: bool,
    /// Whether drop must roll the transaction back.
    active: bool,
}

/// `SQLite`-backed `ProjectAtlas` index store.
pub struct AtlasStore {
    /// Active database connection for index reads and writes.
    connection: Connection,
    /// Whether normal reads currently share one explicit `SQLite` snapshot.
    read_snapshot_active: Cell<bool>,
    /// Durable database path when the store is file-backed.
    database_path: Option<PathBuf>,
    /// WAL-safe filesystem identity retained for later ancillary connections.
    database_location: Option<DatabaseLocation>,
    /// Whether this connection is restricted to non-mutating queries.
    read_only: bool,
    /// Project root validated for this store, when the database records one.
    validated_project_root: Option<String>,
    /// Native root identity captured with the validated root binding.
    validated_project_root_identity: Option<CanonicalProjectRoot>,
    /// Project identity captured with the validated root binding.
    validated_project_instance_id: Option<ProjectInstanceId>,
    /// Bounded per-label instances used by direct library callers for this handle lifetime.
    library_usage_instances: RefCell<HashMap<String, Option<UsageInstanceId>>>,
}

/// Caller-owned purpose mutation transaction with an explicit commit boundary.
///
/// Purpose adapters keep this guard alive while they revalidate saved-source
/// admission. Dropping it before [`Self::commit`] rolls the mutation back.
pub struct PurposeMutationTransaction<'connection> {
    /// Immediate transaction rolled back on drop until the caller accepts the mutation.
    transaction: rusqlite::Transaction<'connection>,
}

impl PurposeMutationTransaction<'_> {
    /// Commit the admitted purpose mutation.
    ///
    /// # Errors
    ///
    /// Returns an error when `SQLite` cannot commit the transaction.
    pub fn commit(self) -> DbResult<()> {
        self.transaction.commit().map_err(Into::into)
    }

    /// Roll back a rejected purpose mutation and report any storage failure.
    ///
    /// # Errors
    ///
    /// Returns an error when `SQLite` cannot roll the transaction back.
    pub fn rollback(self) -> DbResult<()> {
        self.transaction.rollback().map_err(Into::into)
    }
}

/// Root and project identity captured when one store binding was validated.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CapturedProjectBinding {
    /// Stable project identity captured at open or explicit root transition.
    pub project_instance_id: ProjectInstanceId,
    /// Normalized local source root captured with the project identity.
    pub project_root: String,
    /// Lossless native root identity captured with the project identity.
    #[serde(skip)]
    pub project_root_identity: CanonicalProjectRoot,
}

/// Lightweight persisted import fact used by alias resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredImportRelation {
    /// Repository-relative source path that owns the import.
    pub path: String,
    /// Parser-selected source declaration or module.
    pub source_name: String,
    /// Original persisted import text.
    pub target_name: String,
    /// One-based source line.
    pub line: usize,
}

/// One unapproved purpose row bound to deterministic curator work.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PurposeCurationCandidate {
    /// Current indexed node, purpose, and summary state.
    pub node: IndexedNode,
    /// Project/generation/task/path work identity used to coalesce duplicate work.
    pub work_key: String,
    /// Opaque token binding conditional apply to this exact unapproved purpose row.
    pub state_token: String,
}

/// Bounded purpose-curation work selected from one project generation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PurposeCurationBatch {
    /// Stable selected-project identity.
    pub project_instance_id: ProjectInstanceId,
    /// Active graph/index generation observed with the queue rows.
    pub active_generation: IndexGeneration,
    /// Host-supplied task label used to coalesce task-local work.
    pub task: String,
    /// Deterministic identity of the complete returned batch.
    pub work_key: String,
    /// Current missing or suggested purpose rows, ordered by path.
    pub items: Vec<PurposeCurationCandidate>,
}

/// Result of applying a purpose through the stale-safe curator path.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PurposeConditionalApplyState {
    /// The current unapproved row matched and was approved atomically.
    Applied,
    /// Project, generation, task, path, or purpose row state changed after selection.
    Stale,
    /// The path now has accepted authored intent and was not overwritten.
    Accepted,
    /// The selected path is no longer active in the current index.
    PathUnavailable,
}

/// One stale-safe purpose approval copied from a queue item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PurposeConditionalApplyRequest {
    /// Queue task label.
    pub task: String,
    /// Exact repository-relative path.
    pub path: String,
    /// Queue item work key.
    pub work_key: String,
    /// Queue item current-row state token.
    pub state_token: String,
    /// Agent-reviewed purpose to approve on an exact state match.
    pub purpose: String,
}

/// Per-item outcome from one atomic conditional purpose-review batch.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PurposeConditionalApplyResult {
    /// Exact requested path.
    pub path: String,
    /// Applied or non-mutating stale/accepted/unavailable outcome.
    pub state: PurposeConditionalApplyState,
    /// Current purpose state observed or written inside the owning transaction.
    pub current_purpose: Option<Purpose>,
}

/// Identity depth required while opening one current project binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProjectIdentityRequirement {
    /// Ordinary stores require a complete root and identity binding.
    Required,
    /// An explicit root transition owns identity creation or repair.
    TransitionOwned,
}

impl ProjectIdentityRequirement {
    /// Whether a missing identity must fail the open.
    const fn is_required(self) -> bool {
        matches!(self, Self::Required)
    }
}

impl Deref for IndexPublicationGuard<'_> {
    type Target = AtlasStore;

    fn deref(&self) -> &Self::Target {
        self.store
    }
}

impl DerefMut for IndexPublicationGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.store
    }
}

impl IndexPublicationGuard<'_> {
    /// Mark the current scan projection absent before bounded replacement batches.
    ///
    /// The caller must finish with [`Self::finish_scan_replacement`] before
    /// completing this publication. Dropping the guard rolls every partial
    /// replacement batch back with the parent transaction.
    ///
    /// # Errors
    ///
    /// Returns an error if the scan projection cannot be updated.
    pub fn begin_scan_replacement(&mut self) -> DbResult<()> {
        mark_all_scan_nodes_absent(&self.store.connection)?;
        self.scan_replacement_pending = true;
        Ok(())
    }

    /// Upsert one bounded scan-node batch inside the parent publication.
    ///
    /// # Errors
    ///
    /// Returns an error if any node in the batch cannot be persisted.
    pub fn upsert_scan_node_batch(&mut self, nodes: &[Node]) -> DbResult<()> {
        upsert_nodes(&self.store.connection, nodes)
    }

    /// Remove derived projections for nodes left absent after replacement.
    ///
    /// # Errors
    ///
    /// Returns an error if stale projections cannot be removed.
    pub fn finish_scan_replacement(&mut self) -> DbResult<()> {
        delete_absent_scan_projections(&self.store.connection)?;
        content_classification::validate_complete_file_content_classifications(
            &self.store.connection,
        )?;
        self.scan_replacement_pending = false;
        Ok(())
    }

    /// Commit every derived projection and advance the complete generation
    /// exactly once.
    ///
    /// # Errors
    ///
    /// Returns an error if generation metadata is invalid, a projection-only
    /// refresh no longer matches its established contract, or commit fails.
    pub fn complete(mut self) -> DbResult<()> {
        if self.scan_replacement_pending {
            return Err(DbError::ScanReplacementIncomplete);
        }
        content_classification::validate_complete_file_content_classifications(
            &self.store.connection,
        )?;
        repository_graph::validate_complete_document_unresolved_reasons(&self.store.connection)?;
        let next_generation = self
            .previous_generation
            .checked_next()
            .ok_or(DbError::PublicationGenerationOverflow)?;
        match &self.contract {
            PublicationContract::Full(contract_fingerprint) => {
                set_metadata(
                    &self.store.connection,
                    INDEX_PUBLICATION_FINGERPRINT_KEY,
                    contract_fingerprint,
                )?;
            }
            PublicationContract::Projection(contract_fingerprint) => {
                let matches =
                    load_index_publication(&self.store.connection)?.is_some_and(|publication| {
                        publication.state == IndexPublicationState::Updating
                            && publication.contract_fingerprint.as_deref()
                                == Some(contract_fingerprint.as_str())
                            && publication.generation == self.previous_generation
                    });
                if !matches {
                    return Err(DbError::PublicationContractChanged);
                }
            }
        }
        set_metadata(
            &self.store.connection,
            INDEX_PUBLICATION_GENERATION_KEY,
            &next_generation.to_string(),
        )?;
        set_metadata(
            &self.store.connection,
            INDEX_PUBLICATION_STATE_KEY,
            IndexPublicationState::Complete.as_str(),
        )?;
        self.store.connection.execute_batch("COMMIT")?;
        self.active = false;
        Ok(())
    }
}

impl Drop for IndexPublicationGuard<'_> {
    fn drop(&mut self) {
        if self.active {
            let _rollback_result = self.store.connection.execute_batch("ROLLBACK");
        }
    }
}

/// UTF-8 source text persisted for indexed search.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IndexedFileText {
    /// Repository-relative file path using forward slashes.
    pub path: String,
    /// BLAKE3 content hash from the scanned file node.
    pub content_hash: Option<String>,
    /// UTF-8 byte count stored for telemetry.
    pub byte_count: usize,
    /// Number of text lines stored for context extraction.
    pub line_count: usize,
    /// Full UTF-8 source text used by indexed search.
    pub content: String,
}

/// Bounded safe-token request for FTS candidate acceleration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileTextFtsQuery<'query> {
    /// One ASCII-alphanumeric token that exact lexical matching will verify.
    pub literal_token: &'query str,
    /// Optional exact path-or-descendant scope.
    pub path_prefix: Option<&'query str>,
    /// Maximum candidates returned before reporting overflow.
    pub limit: usize,
}

/// One FTS-ranked candidate carrying only authoritative persisted metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct FileTextFtsCandidate {
    /// Repository-relative file path used for budgeted content hydration.
    pub path: String,
    /// BLAKE3 content hash from the authoritative persisted text row.
    pub content_hash: Option<String>,
    /// Persisted UTF-8 source byte count used for pre-hydration budgets.
    pub byte_count: usize,
    /// Persisted line count.
    pub line_count: usize,
    /// `SQLite` FTS5 BM25 score; lower values rank first.
    pub bm25: f64,
}

/// Bounded FTS candidate page for service-owned exact verification.
#[derive(Clone, Debug, PartialEq)]
pub struct FileTextFtsPage {
    /// Candidates ordered by BM25 and repository path.
    pub candidates: Vec<FileTextFtsCandidate>,
    /// Whether at least one more candidate matched the bound query.
    pub overflow: bool,
}

/// Persisted file metadata available before source-content decoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileTextMetadata {
    /// Repository-relative file path using forward slashes.
    pub path: String,
    /// BLAKE3 content hash from the scanned file node.
    pub content_hash: Option<String>,
    /// Persisted UTF-8 source byte count.
    pub byte_count: usize,
    /// Persisted line count.
    pub line_count: usize,
    /// Closed content role available before source decoding.
    pub classification: ContentClassification,
}

/// Service decision made before one fallback row decodes source content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileTextAdmission {
    /// Decode and visit this row's source content.
    Read,
    /// Continue without decoding this row's source content.
    Skip,
    /// Stop fallback iteration without decoding this row's source content.
    Stop,
}

/// Content-free synchronization state for the rebuildable FTS projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileTextFtsState {
    /// Authoritative persisted-text rows.
    pub source_rows: usize,
    /// Document rows currently represented by the FTS projection.
    pub indexed_rows: usize,
    /// Whether authoritative and projected document identities agree exactly.
    pub synchronized: bool,
}

/// Agent-approved resolution for a deterministic health finding.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HealthResolution {
    /// Stable health finding id.
    pub finding_id: String,
    /// Finding category.
    pub category: String,
    /// Primary path.
    pub path: String,
    /// Related path, when any.
    pub related_path: Option<String>,
    /// Agent rationale for suppressing future repeats.
    pub rationale: String,
}

/// Bounded health query used by agent-facing adapters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HealthQuery {
    /// Pagination start index after filters are applied.
    pub start_index: usize,
    /// Maximum findings to return.
    pub limit: usize,
    /// Optional finding category filter.
    pub category: Option<String>,
    /// Optional severity filter.
    pub severity: Option<Severity>,
    /// Optional repository-relative path prefix filter.
    pub path_prefix: Option<String>,
    /// Return counts without finding rows.
    pub summary_only: bool,
    /// Health and purpose-curation scope.
    pub scope: HealthScope,
}

/// Resolution ownership used by bounded health queries.
#[derive(Clone, Copy)]
enum HealthResolutionFilter<'a> {
    /// Caller-owned compatibility filter.
    Explicit(&'a [String]),
    /// Durable resolutions owned by the current project database.
    Stored,
}

/// Scope controls for bounded health and purpose-curation queries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HealthScope {
    /// Include all indexed paths.
    All,
    /// Include only source files and folders with source descendants.
    SourceOnly,
    /// Include all folders plus high-impact files.
    PurposeDefault,
    /// Include all folders, high-impact files, and non-source files.
    PurposeWithAssets,
    /// Include all folders, high-impact files, and all source files.
    PurposeWithSourceFiles,
    /// Include every indexed file and folder.
    PurposeStrict,
}

impl HealthScope {
    /// Scope matching unfiltered health output.
    pub fn all() -> Self {
        Self::All
    }

    /// Scope restricted to source-relevant paths.
    pub fn source_only() -> Self {
        Self::SourceOnly
    }

    /// Default agent purpose curation scope: folders plus high-impact files.
    pub fn purpose_default() -> Self {
        Self::PurposeDefault
    }

    /// Purpose curation scope including non-source asset files.
    pub fn purpose_with_assets() -> Self {
        Self::PurposeWithAssets
    }

    /// Purpose curation scope including all source files.
    pub fn purpose_with_source_files() -> Self {
        Self::PurposeWithSourceFiles
    }

    /// Strict purpose curation scope including every indexed path.
    pub fn purpose_strict() -> Self {
        Self::PurposeStrict
    }

    /// Whether this scope should be reported as source-focused in agent payloads.
    pub fn is_source_focused(self) -> bool {
        self.source_only_filter()
    }

    /// Whether this scope uses the folder-first high-impact purpose queue.
    pub fn is_purpose_queue(self) -> bool {
        self.high_impact_queue()
    }

    /// Whether source relevance should be applied before queue-specific filters.
    fn source_only_filter(self) -> bool {
        matches!(
            self,
            Self::SourceOnly | Self::PurposeDefault | Self::PurposeWithSourceFiles
        )
    }

    /// Whether the scope should use folder-first purpose queue selection.
    fn high_impact_queue(self) -> bool {
        matches!(
            self,
            Self::PurposeDefault
                | Self::PurposeWithAssets
                | Self::PurposeWithSourceFiles
                | Self::PurposeStrict
        )
    }

    /// Whether non-source asset files should be included in queue selection.
    fn include_assets(self) -> bool {
        matches!(self, Self::PurposeWithAssets)
    }

    /// Whether all source files should be included in queue selection.
    fn include_source_files(self) -> bool {
        matches!(self, Self::PurposeWithSourceFiles | Self::PurposeStrict)
    }

    /// Whether all files should be included in queue selection.
    fn include_all_files(self) -> bool {
        matches!(self, Self::PurposeStrict)
    }
}

/// Bounded health findings page returned by the database layer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HealthFindingsPage {
    /// Findings after filters are applied.
    pub total: usize,
    /// Findings before filters are applied, after resolved findings are removed.
    pub unfiltered_total: usize,
    /// Findings returned in this page.
    pub returned: usize,
    /// Pagination start index used for this page.
    pub start_index: usize,
    /// Maximum findings requested for this page.
    pub limit: usize,
    /// Returned health finding rows.
    pub findings: Vec<HealthFinding>,
}

/// Static metadata for one purpose lifecycle health category.
#[derive(Clone, Copy, Debug)]
struct PurposeHealthSpec {
    /// Stored purpose status that emits this health category.
    status: &'static str,
    /// Health finding category for the lifecycle status.
    category: &'static str,
    /// Health finding message for every row in this lifecycle category.
    message: &'static str,
    /// Agent recommendation for resolving this lifecycle category.
    recommendation: &'static str,
}

/// Purpose lifecycle health categories that can be paged directly in `SQLite`.
const PURPOSE_HEALTH_SPECS: [PurposeHealthSpec; 3] = [
    PurposeHealthSpec {
        status: PurposeStatus::Missing.as_str(),
        category: CATEGORY_MISSING_PURPOSE,
        message: MESSAGE_MISSING_PURPOSE,
        recommendation: RECOMMENDATION_MISSING_PURPOSE_QUEUE,
    },
    PurposeHealthSpec {
        status: PurposeStatus::Suggested.as_str(),
        category: CATEGORY_SUGGESTED_PURPOSE_REVIEW,
        message: MESSAGE_SUGGESTED_PURPOSE_REVIEW,
        recommendation: RECOMMENDATION_SUGGESTED_PURPOSE_REVIEW_QUEUE,
    },
    PurposeHealthSpec {
        status: PurposeStatus::Stale.as_str(),
        category: CATEGORY_STALE_PURPOSE,
        message: MESSAGE_STALE_PURPOSE,
        recommendation: RECOMMENDATION_STALE_PURPOSE,
    },
];

/// Run one standalone write only while the captured schema and project binding still match.
#[cfg(test)]
fn with_validated_write_transaction<T>(
    connection: &Connection,
    expected_root: Option<&str>,
    expected_identity: Option<ProjectInstanceId>,
    operation: impl FnOnce(&Connection) -> DbResult<T>,
) -> DbResult<T> {
    let transaction =
        rusqlite::Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    let result = schema::validate_active_binding(&transaction, expected_root, expected_identity)
        .and_then(|()| operation(&transaction));
    match result {
        Ok(value) => {
            transaction.commit()?;
            Ok(value)
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

/// Run one standalone write while the captured native root and project binding still match.
fn with_validated_native_write_transaction<T>(
    connection: &Connection,
    expected_root: Option<&CanonicalProjectRoot>,
    expected_identity: Option<ProjectInstanceId>,
    operation: impl FnOnce(&Connection) -> DbResult<T>,
) -> DbResult<T> {
    let transaction =
        rusqlite::Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    let result =
        schema::validate_active_native_binding(&transaction, expected_root, expected_identity)
            .and_then(|()| operation(&transaction));
    match result {
        Ok(value) => {
            transaction.commit()?;
            Ok(value)
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

impl AtlasStore {
    /// Reject mutations while this connection owns an ordinary read snapshot.
    fn require_mutation_scope(&self) -> DbResult<()> {
        if self.read_snapshot_active.get() {
            Err(DbError::IndexReadSnapshotActive)
        } else {
            Ok(())
        }
    }

    /// Open a nested-capable write scope after validating the active binding.
    fn validated_savepoint(&mut self) -> DbResult<rusqlite::Savepoint<'_>> {
        self.require_mutation_scope()?;
        let validate_binding = self.connection.is_autocommit();
        let expected_root_identity = self.validated_project_root_identity.clone();
        let expected_identity = self.validated_project_instance_id;
        let savepoint = self.connection.savepoint()?;
        if validate_binding {
            schema::validate_active_native_binding(
                &savepoint,
                expected_root_identity.as_ref(),
                expected_identity,
            )?;
        }
        Ok(savepoint)
    }

    /// Run one atomic standalone write, reusing an already validated parent transaction.
    fn with_validated_write<T>(
        &self,
        operation: impl FnOnce(&Connection) -> DbResult<T>,
    ) -> DbResult<T> {
        self.require_mutation_scope()?;
        if !self.connection.is_autocommit() {
            return operation(&self.connection);
        }
        with_validated_native_write_transaction(
            &self.connection,
            self.validated_project_root_identity.as_ref(),
            self.validated_project_instance_id,
            operation,
        )
    }

    /// Begin one purpose mutation whose caller owns final source revalidation.
    ///
    /// Purpose writes performed through this store join the returned immediate
    /// transaction. The caller must revalidate its saved-source witness before
    /// committing; an early return drops the guard and rolls the complete batch
    /// back.
    ///
    /// # Errors
    ///
    /// Returns an error when mutation is unavailable, the binding changed, or
    /// `SQLite` cannot start the transaction.
    pub fn begin_purpose_mutation(&self) -> DbResult<PurposeMutationTransaction<'_>> {
        self.require_mutation_scope()?;
        let transaction =
            rusqlite::Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        schema::validate_active_native_binding(
            &transaction,
            self.validated_project_root_identity.as_ref(),
            self.validated_project_instance_id,
        )?;
        Ok(PurposeMutationTransaction { transaction })
    }

    /// Run telemetry against this store's exact captured database binding.
    ///
    /// File-backed stores use a separate short-lived writable connection with
    /// an ancillary fail-fast busy budget. Read-only navigation stores release
    /// their read snapshot first. The connection is never discovered, attached,
    /// substituted, or shared with another project.
    fn with_telemetry_connection<T>(
        &self,
        operation: impl FnOnce(&Connection) -> DbResult<T>,
    ) -> DbResult<T> {
        if self.read_only {
            self.finish_index_read_snapshot()?;
        } else {
            self.require_mutation_scope()?;
        }
        let (Some(path), Some(location)) =
            (self.database_path.as_ref(), self.database_location.as_ref())
        else {
            if self.database_path.is_none() && self.database_location.is_none() {
                return operation(&self.connection);
            }
            return Err(DbError::TelemetryPathUnavailable);
        };
        let _ = schema::preflight(path, None)?;
        let connection = open_writable_connection(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE,
            location,
            SQLITE_TELEMETRY_BUSY_TIMEOUT,
            JournalModePolicy::RequireWal,
        )?;
        operation(&connection)
    }

    /// Open or create an index store.
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` setup or schema validation fails.
    pub fn open(path: &Path) -> DbResult<Self> {
        Self::open_with_project_root(path, None)
    }

    /// Open or create an index store owned by one canonical project root.
    ///
    /// Existing databases bound to another root are rejected before writable
    /// access. A genuinely fresh database records the supplied root in the
    /// same transaction that creates its schema.
    ///
    /// # Errors
    ///
    /// Returns an error if read-only compatibility preflight, root validation,
    /// transactional migration, or `SQLite` setup fails.
    pub fn open_for_project(path: &Path, root: &Path) -> DbResult<Self> {
        let expected_identity = CanonicalProjectRoot::from_path(root)?;
        let expected_root = expected_identity.display_string();
        Self::open_with_binding_requirement(
            path,
            Some(&expected_root),
            Some(&expected_identity),
            ProjectIdentityRequirement::Required,
        )
    }

    /// Open with an optional source-owned project identity.
    fn open_with_project_root(path: &Path, expected_root: Option<&str>) -> DbResult<Self> {
        Self::open_with_binding_requirement(
            path,
            expected_root,
            None,
            ProjectIdentityRequirement::Required,
        )
    }

    /// Open for an explicit root transition that owns identity repair.
    fn open_for_root_transition(path: &Path) -> DbResult<Self> {
        Self::open_with_binding_requirement(
            path,
            None,
            None,
            ProjectIdentityRequirement::TransitionOwned,
        )
    }

    /// Open with the root and identity validation required by the caller.
    fn open_with_binding_requirement(
        path: &Path,
        expected_root: Option<&str>,
        expected_identity: Option<&CanonicalProjectRoot>,
        identity_requirement: ProjectIdentityRequirement,
    ) -> DbResult<Self> {
        let (preflight, location) = if let Some(expected_identity) = expected_identity {
            schema::preflight_for_project(path, expected_identity)?
        } else {
            schema::preflight(path, None)?
        };
        if expected_identity.is_none()
            && identity_requirement.is_required()
            && preflight.state == SchemaState::UpgradeRequired
            && preflight
                .project_root
                .as_deref()
                .is_some_and(|root| root.contains('\u{fffd}'))
        {
            // A predecessor's replacement character may be a lossy raw path
            // byte. Rootless migration has no native caller authority with
            // which to disambiguate that candidate, so refuse before opening
            // a writable connection or creating WAL state.
            return Err(DbError::ProjectRootIdentityMissing);
        }
        let validated_project_root = expected_identity
            .map(CanonicalProjectRoot::display_string)
            .or_else(|| expected_root.map(str::to_owned))
            .or(preflight.project_root.clone());
        let connection = open_writable_connection(
            path,
            writable_open_flags(preflight.state, location.database_exists),
            &location,
            SQLITE_BUSY_TIMEOUT,
            writable_journal_policy(preflight.state),
        )?;
        if preflight.state == SchemaState::Current
            && let Some(identity) = expected_identity
        {
            project_identity::ensure_project_root_identity(&connection, identity)?;
        }
        let validated_project_instance_id = if preflight.state == SchemaState::Current {
            if let Some(expected_identity) = expected_identity {
                schema::revalidate_current_native_binding(
                    &connection,
                    expected_identity,
                    identity_requirement.is_required(),
                )?
            } else if let Some(stored_identity) =
                project_identity::load_project_root_identity(&connection)?
            {
                schema::revalidate_current_native_binding(
                    &connection,
                    &stored_identity,
                    identity_requirement.is_required(),
                )?
            } else {
                let stored_instance_id = project_identity::load_project_identity(&connection)?;
                schema::validate_binding_completeness(
                    validated_project_root.as_deref(),
                    stored_instance_id,
                    identity_requirement.is_required(),
                )?;
                if stored_instance_id.is_some() {
                    return Err(DbError::ProjectRootIdentityMissing);
                }
                None
            }
        } else {
            if let Some(expected_identity) = expected_identity {
                schema::initialize_with_project_root(
                    &connection,
                    Some(&expected_identity.display_string()),
                    Some(expected_identity),
                )?;
            } else {
                let initialization_root = expected_root.or(preflight.project_root.as_deref());
                schema::initialize_with_project_root(&connection, initialization_root, None)?;
            }
            project_identity::load_project_identity(&connection)?
        };
        let validated_project_root_identity = if validated_project_root.is_some() {
            Some(
                project_identity::load_project_root_identity(&connection)?
                    .ok_or(DbError::ProjectRootIdentityMissing)?,
            )
        } else {
            None
        };
        if identity_requirement.is_required()
            && validated_project_root.is_some()
            && validated_project_instance_id.is_none()
        {
            return Err(DbError::ProjectInstanceIdentityMissing);
        }
        let database_location = if location.database_exists {
            location
        } else {
            sqlite_profile::inspect_database_location(path)?
        };
        Ok(Self {
            connection,
            read_snapshot_active: Cell::new(false),
            database_path: Some(path.to_path_buf()),
            database_location: Some(database_location),
            read_only: false,
            validated_project_root,
            validated_project_root_identity,
            validated_project_instance_id,
            library_usage_instances: RefCell::new(HashMap::new()),
        })
    }

    /// Open an existing index without creating, migrating, or backfilling it.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be opened read-only or its
    /// schema version is not exactly supported by this runtime.
    pub fn open_read_only(path: &Path) -> DbResult<Self> {
        Self::open_read_only_with_project_root(path, None)
    }

    /// Open one current read snapshot owned by a canonical project root.
    ///
    /// # Errors
    ///
    /// Returns an error if the database is incompatible, belongs to another
    /// root, or cannot be opened without database mutation.
    pub fn open_read_only_for_project(path: &Path, root: &Path) -> DbResult<Self> {
        let expected_identity = CanonicalProjectRoot::from_path(root)?;
        Self::open_read_only_with_project_root(path, Some(&expected_identity))
    }

    /// Return whether this store is restricted to non-mutating queries.
    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Return whether this store currently owns an open read snapshot.
    #[must_use]
    pub fn has_active_read_snapshot(&self) -> bool {
        self.read_snapshot_active.get()
    }

    /// Open a current read snapshot with optional project identity validation.
    fn open_read_only_with_project_root(
        path: &Path,
        expected_identity: Option<&CanonicalProjectRoot>,
    ) -> DbResult<Self> {
        let (connection, preflight) = schema::open_current_read_only(path, None)?;
        let validated_project_instance_id = project_identity::load_project_identity(&connection)?;
        let validated_project_root_identity =
            project_identity::load_project_root_identity(&connection)?;
        if expected_identity.is_some() && validated_project_root_identity.is_none() {
            return Err(DbError::ProjectRootIdentityMissing);
        }
        if let (Some(expected), Some(found)) =
            (expected_identity, validated_project_root_identity.as_ref())
            && expected != found
        {
            return Err(DbError::ProjectRootMismatch {
                expected: expected.display_string(),
                found: found.display_string(),
            });
        }
        schema::validate_binding_completeness(
            preflight.project_root.as_deref(),
            validated_project_instance_id,
            true,
        )?;
        let database_location = sqlite_profile::inspect_database_location(path)?;
        Ok(Self {
            connection,
            read_snapshot_active: Cell::new(true),
            database_path: Some(path.to_path_buf()),
            database_location: Some(database_location),
            read_only: true,
            validated_project_root: validated_project_root_identity
                .as_ref()
                .map(CanonicalProjectRoot::display_string)
                .or(preflight.project_root),
            validated_project_root_identity,
            validated_project_instance_id,
            library_usage_instances: RefCell::new(HashMap::new()),
        })
    }

    /// Open an in-memory store for tests.
    ///
    /// # Errors
    ///
    /// Returns an error if schema setup fails.
    pub fn in_memory() -> DbResult<Self> {
        let store = Self {
            connection: Connection::open_in_memory()?,
            read_snapshot_active: Cell::new(false),
            database_path: None,
            database_location: None,
            read_only: false,
            validated_project_root: None,
            validated_project_root_identity: None,
            validated_project_instance_id: None,
            library_usage_instances: RefCell::new(HashMap::new()),
        };
        schema::initialize(&store.connection, None)?;
        Ok(store)
    }

    /// Validate, initialize, or migrate the schema through the storage owner.
    ///
    /// # Errors
    ///
    /// Returns an error when schema compatibility, integrity, or migration fails.
    pub fn initialize_schema(&self) -> DbResult<()> {
        schema::initialize(&self.connection, None)
    }

    /// Upsert a full scan result and mark previously seen missing paths absent.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn replace_scan(&mut self, nodes: &[Node]) -> DbResult<()> {
        let savepoint = self.validated_savepoint()?;
        mark_all_scan_nodes_absent(&savepoint)?;
        upsert_nodes(&savepoint, nodes)?;
        delete_absent_scan_projections(&savepoint)?;
        savepoint.commit()?;
        Ok(())
    }

    /// Upsert a partial scan result without marking unrelated paths absent.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn upsert_scan_nodes(&mut self, nodes: &[Node]) -> DbResult<()> {
        let savepoint = self.validated_savepoint()?;
        upsert_nodes(&savepoint, nodes)?;
        savepoint.commit()?;
        Ok(())
    }

    /// Mark paths and their descendants absent after filesystem delete events.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn mark_paths_absent(&mut self, paths: &[String]) -> DbResult<()> {
        let savepoint = self.validated_savepoint()?;
        let fts_revision = begin_file_text_fts_update(&savepoint)?;
        {
            let mut mark_nodes = savepoint.prepare_cached(
                "UPDATE nodes SET exists_now = 0 WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
            )?;
            let mut delete_classifications = savepoint.prepare_cached(
                "DELETE FROM file_content_classifications
                  WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
            )?;
            let mut delete_relations = savepoint.prepare_cached(
                "DELETE FROM symbol_relations WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
            )?;
            let mut delete_symbols = savepoint.prepare_cached(
                "DELETE FROM symbols WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
            )?;
            let mut delete_parse_metadata = savepoint.prepare_cached(
                "DELETE FROM source_parse_metadata WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
            )?;
            let mut delete_text_fts = savepoint.prepare_cached(
                "INSERT INTO file_text_fts(file_text_fts, rowid, content) \
                 SELECT 'delete', rowid, content FROM file_texts \
                 WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
            )?;
            let mut delete_text = savepoint.prepare_cached(
                "DELETE FROM file_texts WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
            )?;
            for path in paths {
                if path == "." || path.is_empty() {
                    continue;
                }
                let descendant_pattern = sqlite_descendant_pattern(path);
                delete_classifications.execute(params![path, descendant_pattern])?;
                mark_nodes.execute(params![path, descendant_pattern])?;
                delete_relations.execute(params![path, descendant_pattern])?;
                delete_symbols.execute(params![path, descendant_pattern])?;
                delete_parse_metadata.execute(params![path, descendant_pattern])?;
                delete_text_fts.execute(params![path, descendant_pattern])?;
                delete_text.execute(params![path, descendant_pattern])?;
            }
        }
        complete_file_text_fts_update(&savepoint, fts_revision)?;
        savepoint.commit()?;
        Ok(())
    }

    /// Replace indexed text for scanned file paths.
    ///
    /// `paths` should contain every file path considered by the scan batch.
    /// Existing indexed text for those paths is cleared first so binary,
    /// deleted, or no-longer-UTF-8 files cannot leave stale searchable content.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn replace_file_texts_for_paths<'text>(
        &mut self,
        paths: &[String],
        texts: impl IntoIterator<Item = &'text IndexedFileText>,
    ) -> DbResult<()> {
        let texts = texts.into_iter().collect::<Vec<_>>();
        for text in &texts {
            validate_indexed_file_text(text)?;
        }
        let savepoint = self.validated_savepoint()?;
        let fts_revision = begin_file_text_fts_update(&savepoint)?;
        {
            let mut delete_fts = savepoint.prepare_cached(
                "INSERT INTO file_text_fts(file_text_fts, rowid, content) \
                 SELECT 'delete', rowid, content FROM file_texts WHERE path = ?1",
            )?;
            let mut delete = savepoint.prepare_cached("DELETE FROM file_texts WHERE path = ?1")?;
            for path in paths {
                delete_fts.execute([path])?;
                delete.execute([path])?;
            }
        }
        {
            let mut delete_current_fts = savepoint.prepare_cached(
                "INSERT INTO file_text_fts(file_text_fts, rowid, content) \
                 SELECT 'delete', rowid, content FROM file_texts WHERE path = ?1",
            )?;
            let mut upsert = savepoint.prepare_cached(
                "
                INSERT INTO file_texts(path, content_hash, byte_count, line_count, content, updated_at)
                VALUES(?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP)
                ON CONFLICT(path) DO UPDATE SET
                    content_hash = excluded.content_hash,
                    byte_count = excluded.byte_count,
                    line_count = excluded.line_count,
                    content = excluded.content,
                    updated_at = CURRENT_TIMESTAMP
                ",
            )?;
            let mut index = savepoint.prepare_cached(
                "INSERT INTO file_text_fts(rowid, content) \
                 SELECT rowid, content FROM file_texts WHERE path = ?1",
            )?;
            for text in texts {
                delete_current_fts.execute([&text.path])?;
                upsert.execute(params![
                    text.path,
                    text.content_hash.as_deref(),
                    usize_to_i64(text.byte_count),
                    usize_to_i64(text.line_count),
                    text.content
                ])?;
                index.execute([&text.path])?;
            }
        }
        complete_file_text_fts_update(&savepoint, fts_revision)?;
        savepoint.commit()?;
        Ok(())
    }

    /// Load one indexed text row by repository path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or stored counts are invalid.
    pub fn load_file_text(&self, path: &str) -> DbResult<Option<IndexedFileText>> {
        let mut statement = self.connection.prepare(
            "
            SELECT path, content_hash, byte_count, line_count, content
            FROM file_texts
            WHERE path = ?1
            ",
        )?;
        let mut rows = statement.query([path])?;
        rows.next()?.map(file_text_from_row).transpose()
    }

    /// Load a bounded FTS candidate superset for service-owned exact verification.
    ///
    /// The request accepts only one safe ASCII-alphanumeric token. The MATCH
    /// expression is bound as data. Only authoritative metadata is returned;
    /// callers hydrate selected paths with [`Self::load_file_text`] after
    /// applying their file, byte, and time budgets.
    ///
    /// # Errors
    ///
    /// Returns an error when the token shape or limit is unsafe, bounded work
    /// is canceled or reaches its deadline, or persisted rows are invalid.
    pub fn query_file_text_fts_candidates(
        &self,
        query: &FileTextFtsQuery<'_>,
        control: Option<&IndexWorkControl>,
    ) -> DbResult<FileTextFtsPage> {
        validate_file_text_fts_token(query.literal_token)?;
        if query.limit > MAX_FILE_TEXT_FTS_CANDIDATES {
            return Err(DbError::FileTextFtsCandidateLimit {
                requested: query.limit,
                maximum: MAX_FILE_TEXT_FTS_CANDIDATES,
            });
        }
        let match_expression = format!("\"{}\"", query.literal_token);
        let row_limit = usize_to_i64(query.limit.saturating_add(1));
        with_file_text_progress(&self.connection, control, || {
            let mut candidates = Vec::with_capacity(query.limit.saturating_add(1));
            if let Some(path_prefix) = normalized_file_text_path_prefix(query.path_prefix) {
                let descendant_pattern = sqlite_descendant_pattern(path_prefix);
                let mut statement = self.connection.prepare_cached(
                    "
                    SELECT
                        f.path,
                        f.content_hash,
                        f.byte_count,
                        f.line_count,
                        bm25(file_text_fts)
                    FROM file_text_fts
                    JOIN file_texts AS f ON f.rowid = file_text_fts.rowid
                    WHERE file_text_fts MATCH ?1
                      AND (f.path = ?2 OR f.path LIKE ?3 ESCAPE '\\')
                    ORDER BY bm25(file_text_fts), f.path
                    LIMIT ?4
                    ",
                )?;
                let mut rows = statement.query(params![
                    match_expression,
                    path_prefix,
                    descendant_pattern,
                    row_limit,
                ])?;
                collect_file_text_fts_candidates(&mut rows, control, &mut candidates)?;
            } else {
                let mut statement = self.connection.prepare_cached(
                    "
                    SELECT
                        f.path,
                        f.content_hash,
                        f.byte_count,
                        f.line_count,
                        bm25(file_text_fts)
                    FROM file_text_fts
                    JOIN file_texts AS f ON f.rowid = file_text_fts.rowid
                    WHERE file_text_fts MATCH ?1
                    ORDER BY bm25(file_text_fts), f.path
                    LIMIT ?2
                    ",
                )?;
                let mut rows = statement.query(params![match_expression, row_limit])?;
                collect_file_text_fts_candidates(&mut rows, control, &mut candidates)?;
            }
            let overflow = candidates.len() > query.limit;
            candidates.truncate(query.limit);
            Ok(FileTextFtsPage {
                candidates,
                overflow,
            })
        })
    }

    /// Visit correctness-authoritative persisted text with predecode admission.
    ///
    /// `admit` receives path and byte metadata before the source `String` is
    /// decoded from `SQLite`. Returning [`FileTextAdmission::Skip`] or
    /// [`FileTextAdmission::Stop`] therefore avoids allocating excluded source
    /// content. Returning `false` from `visitor` stops after an admitted row.
    ///
    /// # Errors
    ///
    /// Returns an error when bounded work is canceled or reaches its deadline,
    /// persisted counts are invalid, or either callback returns an error.
    pub fn visit_file_texts_for_fallback<A, V>(
        &self,
        path_prefix: Option<&str>,
        control: Option<&IndexWorkControl>,
        mut admit: A,
        mut visitor: V,
    ) -> DbResult<()>
    where
        A: FnMut(&FileTextMetadata) -> DbResult<FileTextAdmission>,
        V: FnMut(IndexedFileText) -> DbResult<bool>,
    {
        with_file_text_progress(&self.connection, control, || {
            let mut content_statement = self
                .connection
                .prepare_cached(FILE_TEXT_FALLBACK_CONTENT_SQL)?;
            if let Some(path_prefix) = normalized_file_text_path_prefix(path_prefix) {
                let (descendant_start, descendant_end) = file_text_descendant_range(path_prefix);
                let mut statement = self
                    .connection
                    .prepare_cached(FILE_TEXT_FALLBACK_SCOPED_METADATA_SQL)?;
                let mut rows =
                    statement.query(params![path_prefix, descendant_start, descendant_end])?;
                visit_file_text_fallback_rows(
                    &mut rows,
                    &mut content_statement,
                    control,
                    &mut admit,
                    &mut visitor,
                )
            } else {
                let mut statement = self
                    .connection
                    .prepare_cached(FILE_TEXT_FALLBACK_ALL_METADATA_SQL)?;
                let mut rows = statement.query([])?;
                visit_file_text_fallback_rows(
                    &mut rows,
                    &mut content_statement,
                    control,
                    &mut admit,
                    &mut visitor,
                )
            }
        })
    }

    /// Return whether the transactional FTS projection matches authoritative text.
    ///
    /// This constant-work readiness check is suitable for the search hot path.
    /// Explicit settings diagnostics may additionally count projection rows.
    ///
    /// # Errors
    ///
    /// Returns an error when durable revision metadata is malformed or partial.
    pub fn file_text_fts_ready(&self) -> DbResult<bool> {
        Ok(load_file_text_fts_revisions(&self.connection)?
            .is_some_and(|(source, projection)| source == projection))
    }

    /// Report content-free FTS synchronization state for settings/readiness.
    ///
    /// # Errors
    ///
    /// Returns an error when authoritative or projection row state cannot be
    /// inspected exactly.
    pub fn file_text_fts_state(&self) -> DbResult<FileTextFtsState> {
        let revision_synchronized = self.file_text_fts_ready()?;
        let (source_rows, indexed_rows, synchronized) = self.connection.query_row(
            "
            SELECT
                (SELECT COUNT(*) FROM file_texts),
                (SELECT COUNT(*) FROM file_text_fts_docsize),
                NOT EXISTS(
                    SELECT 1
                    FROM file_texts AS f
                    LEFT JOIN file_text_fts_docsize AS d ON d.id = f.rowid
                    WHERE d.id IS NULL
                )
                AND NOT EXISTS(
                    SELECT 1
                    FROM file_text_fts_docsize AS d
                    LEFT JOIN file_texts AS f ON f.rowid = d.id
                    WHERE f.rowid IS NULL
                )
            ",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)? != 0,
                ))
            },
        )?;
        Ok(FileTextFtsState {
            source_rows: count_to_usize("file_texts", source_rows)?,
            indexed_rows: count_to_usize("file_text_fts", indexed_rows)?,
            synchronized: revision_synchronized && synchronized,
        })
    }

    /// Load indexed text rows for search.
    ///
    /// When `literal_pattern` is supplied, `SQLite` prefilters candidate files
    /// with a substring search before the service performs line-level matching.
    /// Regex and fuzzy searches pass `None` and still use the persisted text
    /// index instead of reopening source files from disk.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or stored counts are invalid.
    pub fn load_file_texts_for_search(
        &self,
        literal_pattern: Option<&str>,
        case_sensitive: bool,
    ) -> DbResult<Vec<IndexedFileText>> {
        let mut texts = Vec::new();
        self.visit_file_texts_for_search(literal_pattern, case_sensitive, |text| {
            texts.push(text);
            Ok(true)
        })?;
        Ok(texts)
    }

    /// Visit indexed text rows for search without materializing all rows.
    ///
    /// When `literal_pattern` is supplied, `SQLite` prefilters candidate files
    /// with a substring search before the service performs line-level matching.
    /// Returning `false` from `visitor` stops iteration early.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails, stored counts are invalid, or the
    /// visitor returns an error.
    pub fn visit_file_texts_for_search<F>(
        &self,
        literal_pattern: Option<&str>,
        case_sensitive: bool,
        mut visitor: F,
    ) -> DbResult<()>
    where
        F: FnMut(IndexedFileText) -> DbResult<bool>,
    {
        if let Some(pattern) = literal_pattern.filter(|pattern| !pattern.is_empty()) {
            if case_sensitive {
                let mut statement = self.connection.prepare(
                    "
                    SELECT path, content_hash, byte_count, line_count, content
                    FROM file_texts
                    WHERE instr(content, ?1) > 0
                    ORDER BY path
                    ",
                )?;
                let mut rows = statement.query([pattern])?;
                while let Some(row) = rows.next()? {
                    if !visitor(file_text_from_row(row)?)? {
                        return Ok(());
                    }
                }
            } else {
                let pattern = pattern.to_ascii_lowercase();
                let mut statement = self.connection.prepare(
                    "
                    SELECT path, content_hash, byte_count, line_count, content
                    FROM file_texts
                    WHERE instr(lower(content), ?1) > 0
                    ORDER BY path
                    ",
                )?;
                let mut rows = statement.query([pattern])?;
                while let Some(row) = rows.next()? {
                    if !visitor(file_text_from_row(row)?)? {
                        return Ok(());
                    }
                }
            }
        } else {
            let mut statement = self.connection.prepare(
                "
                SELECT path, content_hash, byte_count, line_count, content
                FROM file_texts
                ORDER BY path
                ",
            )?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                if !visitor(file_text_from_row(row)?)? {
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// Count files with persisted UTF-8 text for indexed search.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn file_text_count(&self) -> DbResult<usize> {
        let count = self
            .connection
            .query_row("SELECT COUNT(*) FROM file_texts", [], |row| {
                row.get::<_, i64>(0)
            })?;
        count_to_usize("file_texts", count)
    }

    /// Sum persisted UTF-8 source bytes used by indexed search.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn file_text_byte_count(&self) -> DbResult<usize> {
        let count = self.connection.query_row(
            "SELECT COALESCE(SUM(byte_count), 0) FROM file_texts",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        count_to_usize("file_text_bytes", count)
    }

    /// Persist the canonical filesystem root for indexed repository files.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn set_project_root(&mut self, root: &Path) -> DbResult<()> {
        let identity = CanonicalProjectRoot::from_path(root)?;
        let value = identity.display_string();
        let previous_identity = self.validated_project_instance_id;
        let savepoint = self.validated_savepoint()?;
        let found = savepoint
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                [PROJECT_ROOT_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if found.is_none() {
            set_metadata(&savepoint, PROJECT_ROOT_KEY, &value)?;
            project_identity::set_project_root_identity(&savepoint, &identity)?;
        } else {
            project_identity::ensure_project_root_identity_in_transaction(&savepoint, &identity)?;
        }
        let (project_identity, _) = project_identity::ensure_project_identity(&savepoint)?;
        let identity_changed = previous_identity != Some(project_identity);
        savepoint.commit()?;
        self.validated_project_root = Some(value);
        self.validated_project_root_identity = Some(identity);
        self.validated_project_instance_id = Some(project_identity);
        if identity_changed {
            self.library_usage_instances.get_mut().clear();
        }
        Ok(())
    }

    /// Load the canonical filesystem root for indexed repository files.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn project_root(&self) -> DbResult<Option<String>> {
        self.connection
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                [PROJECT_ROOT_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(DbError::from)
    }

    /// Return the project binding captured when this store was validated.
    ///
    /// This accessor performs no database read. It is intended for services
    /// that must bind a result to the exact root and identity selected at open.
    ///
    /// # Errors
    ///
    /// Returns an error when the store has no complete project binding.
    pub fn captured_project_binding(&self) -> DbResult<CapturedProjectBinding> {
        Ok(CapturedProjectBinding {
            project_instance_id: self
                .validated_project_instance_id
                .ok_or(DbError::ProjectInstanceIdentityMissing)?,
            project_root: self
                .validated_project_root
                .clone()
                .ok_or(DbError::ProjectRootMissing)?,
            project_root_identity: self
                .validated_project_root_identity
                .clone()
                .ok_or(DbError::ProjectRootIdentityMissing)?,
        })
    }

    /// Load the authoritative native project-root identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the identity row cannot be read or its versioned
    /// codec payload is invalid.
    pub fn project_root_identity(&self) -> DbResult<Option<CanonicalProjectRoot>> {
        project_identity::load_project_root_identity(&self.connection)
    }

    /// Revalidate the captured binding against a fresh database snapshot.
    ///
    /// File-backed stores open an independent read-only snapshot so a report
    /// connection pinned to an older WAL end mark cannot hide a concurrent
    /// same-root identity rotation. In-memory stores validate their only
    /// connection because no external database binding exists.
    ///
    /// # Errors
    ///
    /// Returns an error when the database, root, identity, schema, or
    /// filesystem binding no longer matches the state captured at open.
    pub fn revalidate_captured_project_binding(&self) -> DbResult<()> {
        let binding = self.captured_project_binding()?;
        let Some(path) = self.database_path.as_deref() else {
            return schema::validate_active_native_binding(
                &self.connection,
                Some(&binding.project_root_identity),
                Some(binding.project_instance_id),
            );
        };
        let (connection, _) = schema::open_current_read_only(path, None)?;
        schema::validate_active_native_binding(
            &connection,
            Some(&binding.project_root_identity),
            Some(binding.project_instance_id),
        )
    }

    /// Read the selected project identity and current purpose-work generation.
    fn purpose_curation_context(
        &self,
        connection: &Connection,
    ) -> DbResult<(ProjectInstanceId, IndexGeneration)> {
        let selected = self
            .validated_project_instance_id
            .ok_or(DbError::ProjectInstanceIdentityMissing)?;
        let current = project_identity::load_project_identity(connection)?
            .ok_or(DbError::ProjectInstanceIdentityMissing)?;
        if current != selected {
            return Err(DbError::GraphProjectIdentityMismatch {
                expected: selected.to_string(),
                found: current.to_string(),
            });
        }
        let generation =
            project_identity::load_graph_generation(connection)?.unwrap_or(IndexGeneration::ZERO);
        Ok((current, generation))
    }

    /// Begin one exclusive full derived-index publication.
    ///
    /// Every nested projection write remains inside the returned guard's
    /// `SQLite` transaction. Other connections keep the prior complete
    /// generation queryable until [`IndexPublicationGuard::complete`] commits.
    ///
    /// # Errors
    ///
    /// Returns an error if the exclusive write transaction cannot begin.
    pub fn begin_index_publication(
        &mut self,
        contract_fingerprint: &str,
    ) -> DbResult<IndexPublicationGuard<'_>> {
        let base_generation = self
            .index_publication()?
            .map_or(IndexGeneration::ZERO, |publication| publication.generation);
        self.begin_index_publication_from(contract_fingerprint, base_generation)
    }

    /// Begin one exclusive full derived-index publication only when its
    /// prepared base generation is still current.
    ///
    /// [`IndexGeneration::ZERO`] matches only an uninitialized store. The
    /// generation comparison occurs after `BEGIN IMMEDIATE` and before any
    /// publication metadata or projection row is changed.
    ///
    /// # Errors
    ///
    /// Returns an error if the exclusive write transaction cannot begin or
    /// another publisher completed a newer generation after work was prepared.
    pub fn begin_index_publication_from(
        &mut self,
        contract_fingerprint: &str,
        expected_base_generation: IndexGeneration,
    ) -> DbResult<IndexPublicationGuard<'_>> {
        self.begin_publication(
            PublicationContract::Full(contract_fingerprint.to_string()),
            Some(expected_base_generation),
        )
    }

    /// Begin one exclusive symbol/projection refresh without replacing the
    /// established full-index contract.
    ///
    /// # Errors
    ///
    /// Returns an error if the index is incomplete, the established contract
    /// differs, or the exclusive write transaction cannot begin.
    pub fn begin_index_projection_refresh(
        &mut self,
        contract_fingerprint: &str,
    ) -> DbResult<IndexPublicationGuard<'_>> {
        let base_generation = self
            .index_publication()?
            .map_or(IndexGeneration::ZERO, |publication| publication.generation);
        self.begin_index_projection_refresh_from(contract_fingerprint, base_generation)
    }

    /// Begin one exclusive symbol/projection refresh only when its prepared
    /// base generation is still current.
    ///
    /// # Errors
    ///
    /// Returns an error if the index is incomplete, the established contract
    /// differs, the base generation changed, or the exclusive write
    /// transaction cannot begin.
    pub fn begin_index_projection_refresh_from(
        &mut self,
        contract_fingerprint: &str,
        expected_base_generation: IndexGeneration,
    ) -> DbResult<IndexPublicationGuard<'_>> {
        self.begin_publication(
            PublicationContract::Projection(contract_fingerprint.to_string()),
            Some(expected_base_generation),
        )
    }

    /// Begin one parent-owned atomic publication transaction.
    fn begin_publication(
        &mut self,
        contract: PublicationContract,
        expected_base_generation: Option<IndexGeneration>,
    ) -> DbResult<IndexPublicationGuard<'_>> {
        begin_immediate_publication(&self.connection)?;
        let setup = (|| {
            schema::validate_active_native_binding(
                &self.connection,
                self.validated_project_root_identity.as_ref(),
                self.validated_project_instance_id,
            )?;
            let previous = load_index_publication(&self.connection)?;
            if let Some(expected) = expected_base_generation {
                let base_matches = match previous.as_ref() {
                    None => expected == IndexGeneration::ZERO,
                    Some(publication) => {
                        publication.state == IndexPublicationState::Complete
                            && publication.generation != IndexGeneration::ZERO
                            && publication.generation == expected
                    }
                };
                if !base_matches {
                    return Err(DbError::PublicationBaseGenerationChanged {
                        expected,
                        found: previous
                            .as_ref()
                            .map_or(IndexGeneration::ZERO, |publication| publication.generation),
                    });
                }
            }
            if let PublicationContract::Projection(expected) = &contract {
                let matches = previous.as_ref().is_some_and(|publication| {
                    publication.state == IndexPublicationState::Complete
                        && publication.contract_fingerprint.as_deref() == Some(expected.as_str())
                });
                if !matches {
                    return Err(DbError::PublicationContractChanged);
                }
            }
            set_metadata(
                &self.connection,
                INDEX_PUBLICATION_STATE_KEY,
                IndexPublicationState::Updating.as_str(),
            )?;
            Ok(previous.map_or(IndexGeneration::ZERO, |publication| publication.generation))
        })();
        let previous_generation = match setup {
            Ok(generation) => generation,
            Err(error) => {
                self.connection.execute_batch("ROLLBACK")?;
                return Err(error);
            }
        };
        Ok(IndexPublicationGuard {
            store: self,
            contract,
            previous_generation,
            scan_replacement_pending: false,
            active: true,
        })
    }

    /// Start one stable read snapshot for freshness verification and every
    /// subsequent query used to construct the response.
    ///
    /// # Errors
    ///
    /// Returns an error if a snapshot is already active or `SQLite` cannot
    /// begin the transaction.
    pub fn begin_index_read_snapshot(&self) -> DbResult<()> {
        if self.read_snapshot_active.replace(true) {
            return Err(DbError::IndexReadSnapshotActive);
        }
        if let Err(error) = self.connection.execute_batch("BEGIN DEFERRED") {
            self.read_snapshot_active.set(false);
            return Err(error.into());
        }
        Ok(())
    }

    /// Finish an active read snapshot before an optional telemetry write.
    ///
    /// This method is a no-op for stores that were not opened for a normal
    /// freshness-verified read.
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLite` cannot finish the read transaction.
    pub fn finish_index_read_snapshot(&self) -> DbResult<()> {
        if !self.read_snapshot_active.get() {
            return Ok(());
        }
        self.connection.execute_batch("COMMIT")?;
        self.read_snapshot_active.set(false);
        Ok(())
    }

    /// Load the current derived-index publication state when initialized.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata is invalid or cannot be read.
    pub fn index_publication(&self) -> DbResult<Option<IndexPublication>> {
        load_index_publication(&self.connection)
    }

    /// Replace the symbol graph for a file path.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn replace_symbol_graph(&mut self, graph: &SymbolGraph) -> DbResult<()> {
        let metadata = SourceParseMetadata::from_graph(graph);
        self.replace_symbol_graph_with_metadata(graph, &metadata)
    }

    /// Replace one file's symbol graph while preserving independent source parse metadata.
    ///
    /// This permits a grammar-backed source parse to coexist with conservative fallback
    /// symbol and relationship facts without relabeling those facts as grammar-native.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata identity/counts differ from the graph or persistence fails.
    pub fn replace_symbol_graph_with_metadata(
        &mut self,
        graph: &SymbolGraph,
        metadata: &SourceParseMetadata,
    ) -> DbResult<()> {
        if metadata.path != graph.path
            || metadata.language != graph.language
            || metadata.symbol_count != graph.symbols.len()
            || metadata.relation_count != graph.relations.len()
        {
            return Err(DbError::SymbolGraphRowShape {
                path: graph.path.clone(),
                reason: "source parse metadata identity or fact counts differ from the graph",
            });
        }
        let savepoint = self.validated_savepoint()?;
        let node_id = {
            let mut delete_symbols =
                savepoint.prepare_cached("DELETE FROM symbols WHERE path = ?1")?;
            let mut delete_relations =
                savepoint.prepare_cached("DELETE FROM symbol_relations WHERE path = ?1")?;
            delete_symbols.execute([&graph.path])?;
            delete_relations.execute([&graph.path])?;

            let mut upsert_metadata = savepoint.prepare_cached(
                "
                INSERT INTO source_parse_metadata(
                    path,
                    language,
                    source_parser,
                    fact_parser,
                    symbol_count,
                    relation_count,
                    updated_at
                )
                VALUES(?1, ?2, ?3, ?4, ?5, ?6, CURRENT_TIMESTAMP)
                ON CONFLICT(path) DO UPDATE SET
                    language = excluded.language,
                    source_parser = excluded.source_parser,
                    fact_parser = excluded.fact_parser,
                    symbol_count = excluded.symbol_count,
                    relation_count = excluded.relation_count,
                    updated_at = CURRENT_TIMESTAMP
                ",
            )?;
            upsert_metadata.execute(params![
                metadata.path,
                metadata.language.as_deref(),
                metadata.parser.to_string(),
                graph.parser.to_string(),
                usize_to_i64(metadata.symbol_count),
                usize_to_i64(metadata.relation_count),
            ])?;
            let mut select_node = savepoint
                .prepare_cached("SELECT id FROM nodes WHERE path = ?1 AND exists_now = 1")?;
            let node_id = select_node
                .query_row([&graph.path], |row| row.get::<_, i64>(0))
                .optional()?;

            let mut insert_symbol = savepoint.prepare_cached(
                "
                INSERT INTO symbols(
                    path,
                    language,
                    name,
                    kind,
                    signature,
                    exported,
                    documentation,
                    line_start,
                    line_end,
                    source_byte_start,
                    source_byte_end,
                    source_column_start,
                    source_column_end,
                    parent,
                    parser,
                    detail
                )
                VALUES(
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                    ?13, ?14, ?15, ?16
                )
                ",
            )?;
            for symbol in &graph.symbols {
                let selector = symbol_source_selector_values(symbol)?;
                insert_symbol.execute(params![
                    symbol.path,
                    symbol.language.as_deref(),
                    symbol.name,
                    symbol.kind.to_string(),
                    symbol.signature,
                    symbol.exported,
                    symbol.documentation.as_deref(),
                    usize_to_i64(symbol.line_start),
                    usize_to_i64(symbol.line_end),
                    selector[0],
                    selector[1],
                    selector[2],
                    selector[3],
                    symbol.parent.as_deref(),
                    symbol.parser.to_string(),
                    symbol.detail.as_deref(),
                ])?;
            }

            let mut insert_relation = savepoint.prepare_cached(
                "
                INSERT INTO symbol_relations(
                    path,
                    source_name,
                    target_name,
                    kind,
                    line,
                    context,
                    parser
                )
                VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ",
            )?;
            for relation in &graph.relations {
                insert_relation.execute(params![
                    relation.path,
                    relation.source_name,
                    relation.target_name,
                    relation.kind.to_string(),
                    usize_to_i64(relation.line),
                    relation.context,
                    relation.parser.to_string(),
                ])?;
            }
            node_id
        };
        if let Some(node_id) = node_id {
            replace_symbol_search_summary(
                &savepoint,
                node_id,
                symbol_search_summary(graph).as_deref(),
            )?;
        }
        savepoint.commit()?;
        Ok(())
    }

    /// Clear source-derived intelligence for one live file path.
    ///
    /// This removes symbols, relations, and the node-level content summary so
    /// skipped or failed parser work cannot leave stale source facts visible.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist or persistence fails.
    pub fn clear_source_index_for_path(&self, path: &str) -> DbResult<()> {
        self.with_validated_write(|connection| {
            let node_id = self.node_id_for_path(path)?;
            connection
                .prepare_cached("DELETE FROM symbols WHERE path = ?1")?
                .execute([path])?;
            connection
                .prepare_cached("DELETE FROM symbol_relations WHERE path = ?1")?
                .execute([path])?;
            connection
                .prepare_cached("DELETE FROM source_parse_metadata WHERE path = ?1")?
                .execute([path])?;
            connection
                .prepare_cached(
                    "
            DELETE FROM summaries
            WHERE node_id = ?1
              AND (
                    (summary_level = 'node' AND subject = '')
                    OR (summary_level = 'search' AND subject = 'symbols')
                  )
            ",
                )?
                .execute([node_id])?;
            Ok(())
        })
    }

    /// Clear symbols and relations for one live file path while preserving node summaries.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn clear_symbol_graph_for_path(&self, path: &str) -> DbResult<()> {
        self.with_validated_write(|connection| {
            let node_id = self.node_id_for_path(path)?;
            connection
                .prepare_cached("DELETE FROM symbols WHERE path = ?1")?
                .execute([path])?;
            connection
                .prepare_cached("DELETE FROM symbol_relations WHERE path = ?1")?
                .execute([path])?;
            connection
                .prepare_cached("DELETE FROM source_parse_metadata WHERE path = ?1")?
                .execute([path])?;
            connection
                .prepare_cached(
                    "
            DELETE FROM summaries
            WHERE node_id = ?1
              AND summary_level = 'search'
              AND subject = 'symbols'
            ",
                )?
                .execute([node_id])?;
            Ok(())
        })
    }

    /// Persist an observed one-line summary for an indexed node.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist or persistence fails.
    pub fn set_node_summary(&self, path: &str, summary: &str) -> DbResult<()> {
        self.with_validated_write(|connection| {
            let node_id = self.node_id_for_path(path)?;
            connection
                .prepare_cached(
                    "
            INSERT INTO summaries(node_id, summary_level, subject, summary, updated_at)
            VALUES(?1, 'node', '', ?2, CURRENT_TIMESTAMP)
            ON CONFLICT(node_id, summary_level, subject) DO UPDATE SET
                summary_level = 'node',
                subject = '',
                summary = excluded.summary,
                updated_at = CURRENT_TIMESTAMP
            ",
                )?
                .execute(params![node_id, summary])?;
            Ok(())
        })
    }

    /// Remove the observed node-level summary for an indexed node.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist or persistence fails.
    pub fn clear_node_summary(&self, path: &str) -> DbResult<()> {
        self.with_validated_write(|connection| {
            let node_id = self.node_id_for_path(path)?;
            connection
                .prepare_cached(
                    "
            DELETE FROM summaries
            WHERE node_id = ?1
              AND summary_level = 'node'
              AND subject = ''
            ",
                )?
                .execute([node_id])?;
            Ok(())
        })
    }

    /// Load symbols filtered by optional file path and query.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbols(
        &self,
        file: Option<&str>,
        query: Option<&str>,
        limit: usize,
    ) -> DbResult<Vec<CodeSymbol>> {
        let max_rows = usize_to_i64(limit.max(1));
        match (file, query) {
            (Some(file), Some(query)) => self.query_symbols(
                "
                SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation,
                       source_byte_start, source_byte_end, source_column_start, source_column_end
                FROM symbols
                WHERE path = ?1 AND (name LIKE ?2 OR signature LIKE ?2 OR documentation LIKE ?2)
                ORDER BY path, line_start, name
                LIMIT ?3
                ",
                params![file, like_query(query), max_rows],
            ),
            (Some(file), None) => self.query_symbols(
                "
                SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation,
                       source_byte_start, source_byte_end, source_column_start, source_column_end
                FROM symbols
                WHERE path = ?1
                ORDER BY path, line_start, name
                LIMIT ?2
                ",
                params![file, max_rows],
            ),
            (None, Some(query)) => self.query_symbols(
                "
                SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation,
                       source_byte_start, source_byte_end, source_column_start, source_column_end
                FROM symbols
                WHERE name LIKE ?1 OR signature LIKE ?1 OR documentation LIKE ?1 OR path LIKE ?1
                ORDER BY path, line_start, name
                LIMIT ?2
                ",
                params![like_query(query), max_rows],
            ),
            (None, None) => self.query_symbols(
                "
                SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation,
                       source_byte_start, source_byte_end, source_column_start, source_column_end
                FROM symbols
                ORDER BY path, line_start, name
                LIMIT ?1
                ",
                params![max_rows],
            ),
        }
    }

    /// Load symbols with their owning file classification and optional content selection.
    ///
    /// Omitted selection retains the legacy symbol candidate universe and order. Explicit
    /// selection is applied by `SQLite` before the row limit.
    ///
    /// # Errors
    ///
    /// Returns an error if a classification is missing or corrupt, or reading fails.
    pub fn load_classified_symbols(
        &self,
        file: Option<&str>,
        query: Option<&str>,
        selection: ContentSelection,
        limit: usize,
    ) -> DbResult<Vec<ClassifiedSymbol>> {
        let (sql, bindings) = classified_symbols_sql(file, query, selection, limit);
        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(bindings.iter()))?;
        let mut symbols = Vec::new();
        while let Some(row) = rows.next()? {
            symbols.push(classified_symbol_from_row(row)?);
        }
        Ok(symbols)
    }

    /// Load one bounded deterministic symbol set for exact repository paths.
    ///
    /// The request uses indexed `symbols.path` predicates, chunks bindings
    /// below the `SQLite` variable ceiling, and preserves the store's active
    /// read snapshot. The service remains responsible for matching exact
    /// declaration identities within these admitted owning paths.
    ///
    /// # Errors
    ///
    /// Returns an error when a path/budget is invalid, cancellation or the
    /// request deadline fires, a row is corrupt, or `SQLite` iteration fails.
    pub fn load_symbols_for_paths_bounded(
        &self,
        paths: &[String],
        budget: SymbolBatchReadBudget,
        control: Option<&IndexWorkControl>,
    ) -> DbResult<SymbolBatchRead> {
        let path_limit =
            usize::try_from(budget.paths()).map_err(|source| DbError::InvalidCount {
                field: "symbol batch paths",
                value: i64::from(budget.paths()),
                source,
            })?;
        let mut exact_paths = BTreeSet::new();
        let mut reached_limit = None;
        for path in paths {
            if let Some(control) = control {
                control.check(IndexWorkStage::RepositoryTraversal)?;
            }
            exact_paths.insert(path.clone());
            if exact_paths.len() > path_limit {
                exact_paths.pop_last();
                reached_limit.get_or_insert(SymbolBatchReadLimit::Paths);
            }
        }
        if exact_paths.is_empty() {
            return Ok(SymbolBatchRead::default());
        }
        let exact_paths = exact_paths.into_iter().collect::<Vec<_>>();
        let row_limit = usize::try_from(budget.rows()).map_err(|source| DbError::InvalidCount {
            field: "symbol batch rows",
            value: i64::from(budget.rows()),
            source,
        })?;
        let mut symbols = Vec::new();
        let mut decoded_bytes = 0_u64;
        'chunks: for chunk in exact_paths.chunks(SYMBOL_BATCH_BIND_PATHS) {
            if let Some(control) = control {
                control.check(IndexWorkStage::RepositoryTraversal)?;
            }
            let remaining_rows = row_limit.saturating_sub(symbols.len());
            if remaining_rows == 0 {
                reached_limit.get_or_insert(SymbolBatchReadLimit::Rows);
                break;
            }
            let placeholders = numbered_placeholders(1, chunk.len());
            let limit_parameter = chunk.len().saturating_add(1);
            let sql = format!(
                "
                SELECT
                    path,
                    language,
                    name,
                    kind,
                    signature,
                    line_start,
                    line_end,
                    parent,
                    parser,
                    detail,
                    exported,
                    documentation,
                    source_byte_start,
                    source_byte_end,
                    source_column_start,
                    source_column_end,
                    length(CAST(path AS BLOB))
                        + COALESCE(length(CAST(language AS BLOB)), 0)
                        + length(CAST(name AS BLOB))
                        + length(CAST(signature AS BLOB))
                        + COALESCE(length(CAST(parent AS BLOB)), 0)
                        + COALESCE(length(CAST(detail AS BLOB)), 0)
                        + COALESCE(length(CAST(documentation AS BLOB)), 0)
                FROM symbols
                WHERE path IN ({placeholders})
                ORDER BY path, line_start, name
                LIMIT ?{limit_parameter}
                "
            );
            let mut bindings = chunk.iter().cloned().map(Value::Text).collect::<Vec<_>>();
            bindings.push(Value::Integer(usize_to_i64(
                remaining_rows.saturating_add(1),
            )));
            with_sqlite_read_progress(
                &self.connection,
                control,
                IndexWorkStage::RepositoryTraversal,
                || {
                    let mut statement = self.connection.prepare(&sql)?;
                    let mut rows = statement.query(params_from_iter(bindings.iter()))?;
                    while let Some(row) = rows.next()? {
                        if let Some(control) = control {
                            control.check(IndexWorkStage::RepositoryTraversal)?;
                        }
                        if symbols.len() >= row_limit {
                            reached_limit.get_or_insert(SymbolBatchReadLimit::Rows);
                            break;
                        }
                        let preflight_bytes = code_symbol_preflight_bytes(row)?;
                        if decoded_bytes.saturating_add(preflight_bytes) > budget.decoded_bytes() {
                            reached_limit.get_or_insert(SymbolBatchReadLimit::DecodedBytes);
                            break;
                        }
                        let symbol = code_symbol_from_row(row)?;
                        let row_bytes = code_symbol_decoded_bytes(&symbol)?;
                        if decoded_bytes.saturating_add(row_bytes) > budget.decoded_bytes() {
                            reached_limit.get_or_insert(SymbolBatchReadLimit::DecodedBytes);
                            break;
                        }
                        decoded_bytes = decoded_bytes.saturating_add(row_bytes);
                        symbols.push(symbol);
                    }
                    Ok(())
                },
            )?;
            if matches!(
                reached_limit,
                Some(SymbolBatchReadLimit::Rows | SymbolBatchReadLimit::DecodedBytes)
            ) {
                break 'chunks;
            }
        }
        symbols.sort_by(|left, right| {
            (&left.path, left.line_start, &left.name, &left.signature).cmp(&(
                &right.path,
                right.line_start,
                &right.name,
                &right.signature,
            ))
        });
        let symbols = symbols.into_boxed_slice().into_vec();
        Ok(SymbolBatchRead {
            work: SymbolBatchReadWork {
                requested_paths: u32::try_from(exact_paths.len()).unwrap_or(u32::MAX),
                returned_rows: u32::try_from(symbols.len()).unwrap_or(u32::MAX),
                decoded_bytes,
            },
            rows: symbols,
            truncated: reached_limit.is_some(),
            reached_limit,
        })
    }

    /// Load symbols for a file and one or more exact kinds.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbols_by_kinds(
        &self,
        file: &str,
        kinds: &[SymbolKind],
        limit: usize,
    ) -> DbResult<Vec<CodeSymbol>> {
        if kinds.is_empty() {
            return Ok(Vec::new());
        }
        let max_rows = usize_to_i64(limit.max(1));
        let placeholders = numbered_placeholders(2, kinds.len());
        let sql = format!(
            "
            SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation,
                   source_byte_start, source_byte_end, source_column_start, source_column_end
            FROM symbols INDEXED BY idx_symbols_path
            WHERE path = ?1 AND kind IN ({placeholders})
            ORDER BY path, line_start, name
            LIMIT {max_rows}
            "
        );
        let mut values = Vec::with_capacity(kinds.len() + 1);
        values.push(file.to_string());
        values.extend(kinds.iter().map(ToString::to_string));
        self.query_symbols(&sql, params_from_iter(values.iter()))
    }

    /// Count symbols for a file and one or more exact kinds.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn count_symbols_by_kinds(&self, file: &str, kinds: &[SymbolKind]) -> DbResult<usize> {
        if kinds.is_empty() {
            return Ok(0);
        }
        let placeholders = numbered_placeholders(2, kinds.len());
        let sql = format!(
            "SELECT COUNT(*) FROM symbols INDEXED BY idx_symbols_path
             WHERE path = ?1 AND kind IN ({placeholders})"
        );
        let mut values = Vec::with_capacity(kinds.len() + 1);
        values.push(file.to_string());
        values.extend(kinds.iter().map(ToString::to_string));
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values.iter()), |row| {
                row.get::<_, i64>(0)
            })?;
        Ok(i64_to_usize(count))
    }

    /// Count indexed symbols grouped by exact name.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_name_counts(&self, names: &[String]) -> DbResult<HashMap<String, usize>> {
        if names.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders = numbered_placeholders(1, names.len());
        let sql = format!(
            "SELECT name, COUNT(*) FROM symbols WHERE name IN ({placeholders}) GROUP BY name"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(names.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut counts = HashMap::new();
        for row in rows {
            let (name, count) = row?;
            counts.insert(name, i64_to_usize(count));
        }
        Ok(counts)
    }

    /// Load symbols with exact names.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbols_by_names(&self, names: &[String]) -> DbResult<Vec<CodeSymbol>> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = numbered_placeholders(1, names.len());
        let sql = format!(
            "
            SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation,
                   source_byte_start, source_byte_end, source_column_start, source_column_end
            FROM symbols
            WHERE name IN ({placeholders})
            ORDER BY path, line_start, name
            "
        );
        self.query_symbols(&sql, params_from_iter(names.iter()))
    }

    /// Load exported symbol names for one file.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_exported_symbol_names_for_path(
        &self,
        file: &str,
        limit: usize,
    ) -> DbResult<Vec<String>> {
        let max_rows = usize_to_i64(limit.max(1));
        let mut statement = self.connection.prepare(
            "
            SELECT DISTINCT name
            FROM symbols
            WHERE path = ?1 AND exported = 1
            ORDER BY name
            LIMIT ?2
            ",
        )?;
        let rows = statement.query_map(params![file, max_rows], |row| row.get::<_, String>(0))?;
        let mut names = Vec::new();
        for row in rows {
            names.push(row?);
        }
        Ok(names)
    }

    /// Count exported symbol names for one file.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn exported_symbol_count_for_path(&self, file: &str) -> DbResult<usize> {
        let count = self.connection.query_row(
            "SELECT COUNT(DISTINCT name) FROM symbols WHERE path = ?1 AND exported = 1",
            [file],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(i64_to_usize(count))
    }

    /// Load one symbol by exact file and name.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbol_by_name(&self, file: &str, name: &str) -> DbResult<Option<CodeSymbol>> {
        let mut symbols = self.load_symbols(Some(file), Some(name), 100)?;
        symbols.retain(|symbol| symbol.name == name);
        Ok(symbols.into_iter().next())
    }

    /// Load all symbols with an exact file and name.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbols_by_exact_file_and_name(
        &self,
        file: &str,
        name: &str,
    ) -> DbResult<Vec<CodeSymbol>> {
        self.query_symbols(
            "
            SELECT path, language, name, kind, signature, line_start, line_end, parent, parser, detail, exported, documentation,
                   source_byte_start, source_byte_end, source_column_start, source_column_end
            FROM symbols
            WHERE path = ?1 AND name = ?2
            ORDER BY line_start, line_end, kind, parent
            ",
            params![file, name],
        )
    }

    /// Load one existing node with purpose state by repository path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading or enum conversion fails.
    pub fn load_node_by_path(&self, path: &str) -> DbResult<Option<IndexedNode>> {
        let mut statement = self.connection.prepare(
            "
            SELECT
                n.path,
                n.kind,
                n.parent_path,
                n.extension,
                n.language,
                n.size_bytes,
                n.mtime_ns,
                n.content_hash,
                p.purpose,
                p.source,
                p.status,
                s.summary
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            LEFT JOIN summaries s ON s.node_id = n.id
                AND s.summary_level = 'node'
                AND s.subject = ''
            WHERE n.exists_now = 1 AND n.path = ?1
            ",
        )?;
        let row = statement
            .query_row([path], |row| {
                let kind_value: String = row.get(1)?;
                let source_value: String = row.get(9)?;
                let status_value: String = row.get(10)?;
                Ok((
                    row.get::<_, String>(0)?,
                    kind_value,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    option_u64_from_sql(row, 5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    source_value,
                    status_value,
                    row.get::<_, Option<String>>(11)?,
                ))
            })
            .optional()?;
        row.map(indexed_node_from_parts).transpose()
    }

    /// Load existing nodes for exact repository paths.
    ///
    /// # Errors
    ///
    /// Returns an error if reading or enum conversion fails.
    pub fn load_nodes_by_paths(&self, paths: &[String]) -> DbResult<Vec<IndexedNode>> {
        self.load_nodes_by_paths_controlled(paths, None)
    }

    /// Load existing nodes for exact repository paths in bounded cancellable batches.
    ///
    /// # Errors
    ///
    /// Returns an error if cancellation fires or reading or enum conversion fails.
    pub fn load_nodes_by_paths_controlled(
        &self,
        paths: &[String],
        control: Option<&IndexWorkControl>,
    ) -> DbResult<Vec<IndexedNode>> {
        let mut unique_paths = paths.to_vec();
        unique_paths.sort();
        unique_paths.dedup();
        if unique_paths.is_empty() {
            return Ok(Vec::new());
        }
        let mut nodes = Vec::new();
        for chunk in unique_paths.chunks(MAX_PURPOSE_CURATION_BATCH_ROWS) {
            let hydrated = with_sqlite_read_progress(
                &self.connection,
                control,
                IndexWorkStage::RepositoryTraversal,
                || {
                    let sql = load_nodes_by_paths_sql(chunk.len());
                    let mut statement = self.connection.prepare(&sql)?;
                    let rows = statement.query_map(params_from_iter(chunk), |row| {
                        let kind_value: String = row.get(1)?;
                        let source_value: String = row.get(9)?;
                        let status_value: String = row.get(10)?;
                        Ok((
                            row.get::<_, String>(0)?,
                            kind_value,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            option_u64_from_sql(row, 5)?,
                            row.get::<_, Option<i64>>(6)?,
                            row.get::<_, Option<String>>(7)?,
                            row.get::<_, Option<String>>(8)?,
                            source_value,
                            status_value,
                            row.get::<_, Option<String>>(11)?,
                        ))
                    })?;
                    let mut selected = Vec::new();
                    for row in rows {
                        selected.push(indexed_node_from_parts(row?)?);
                    }
                    Ok(selected)
                },
            )?;
            nodes.extend(hydrated);
        }
        Ok(nodes)
    }

    /// Hydrate every exact purpose-owner path under one graph read envelope.
    ///
    /// Rows are returned in caller order. Unlike the compatibility path loader,
    /// this graph-facing boundary evaluates every unique requested path in the
    /// selected project and generation. Existing rows retain caller order;
    /// absent ancestor candidates are represented by omission rather than a
    /// partial-query failure.
    ///
    /// # Errors
    ///
    /// Returns an error for duplicate paths, a stale project or generation,
    /// cancellation, corrupt node or purpose state, `SQLite`
    /// failure, or any returned-row, decoded-byte, or hydrated-path budget
    /// overrun. No partial batch is returned.
    pub fn load_purpose_owner_nodes_by_paths_controlled(
        &self,
        project: ProjectInstanceId,
        generation: IndexGeneration,
        paths: &[String],
        budget: RepositoryGraphReadBudget,
        control: Option<&IndexWorkControl>,
    ) -> DbResult<RepositoryGraphReadBatch<IndexedNode>> {
        if paths
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>()
            .len()
            != paths.len()
        {
            return Err(GraphContractError::InvalidLimits {
                reason: "purpose-owner hydration paths must be unique",
            }
            .into());
        }
        self.require_repository_graph_snapshot(project, generation)?;
        let mut meter = repository_graph::RepositoryGraphReadMeter::new(budget, paths.len())?;
        let path_count =
            u32::try_from(paths.len()).map_err(|_source| GraphContractError::InvalidLimits {
                reason: "purpose-owner hydration path count overflowed",
            })?;
        if path_count > budget.returned_rows() {
            return Err(GraphContractError::InvalidLimits {
                reason: "purpose-owner paths exceed the return budget",
            }
            .into());
        }
        if path_count > budget.hydrated_paths() {
            return Err(GraphContractError::InvalidLimits {
                reason: "purpose-owner paths exceed the hydration budget",
            }
            .into());
        }
        if paths.is_empty() {
            return Ok(RepositoryGraphReadBatch {
                rows: Vec::new(),
                work: meter.finish(0)?,
            });
        }

        let mut selected = HashMap::with_capacity(paths.len());
        for chunk in paths.chunks(MAX_PURPOSE_CURATION_BATCH_ROWS) {
            let hydrated = with_sqlite_read_progress(
                &self.connection,
                control,
                IndexWorkStage::RepositoryTraversal,
                || {
                    let sql = load_nodes_by_paths_sql(chunk.len());
                    let mut statement = self.connection.prepare(&sql)?;
                    let mut rows = statement.query(params_from_iter(chunk))?;
                    let mut nodes = Vec::new();
                    while let Some(row) = rows.next()? {
                        let parts = indexed_node_parts_from_sql_row(row)?;
                        meter.record_decoded_bytes(indexed_node_parts_decoded_bytes(&parts)?)?;
                        let node = indexed_node_from_parts(parts)?;
                        meter.record_hydrated_path(&node.node.path)?;
                        nodes.push(node);
                    }
                    Ok(nodes)
                },
            )?;
            for node in hydrated {
                let path = node.node.path.clone();
                if selected.insert(path, node).is_some() {
                    return Err(DbError::GraphRowShape {
                        table: "nodes",
                        reason: "purpose-owner hydration returned a duplicate path",
                    });
                }
            }
        }

        let mut ordered = Vec::with_capacity(paths.len());
        for path in paths {
            if let Some(node) = selected.remove(path) {
                ordered.push(node);
            }
        }
        let work = meter.finish(ordered.len())?;
        Ok(RepositoryGraphReadBatch {
            rows: ordered,
            work,
        })
    }

    /// Load one bounded stale-safe purpose-curation batch for exact paths.
    ///
    /// The node hydration is one prepared set query regardless of row count.
    /// Accepted purposes are intentionally omitted from curator work.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid task, oversized batch, changed project
    /// binding, invalid persisted state, or failed `SQLite` read.
    pub fn load_purpose_curation_batch(
        &self,
        task: &str,
        paths: &[String],
    ) -> DbResult<PurposeCurationBatch> {
        let task = normalize_purpose_curation_task(task)?;
        let mut unique_paths = paths.to_vec();
        unique_paths.sort();
        unique_paths.dedup();
        if unique_paths.len() > MAX_PURPOSE_CURATION_BATCH_ROWS {
            return Err(DbError::PurposeCurationBatchTooLarge {
                requested: unique_paths.len(),
                maximum: MAX_PURPOSE_CURATION_BATCH_ROWS,
            });
        }
        let (project_instance_id, active_generation) =
            self.purpose_curation_context(&self.connection)?;
        let items = self
            .load_nodes_by_paths(&unique_paths)?
            .into_iter()
            .filter(|node| {
                matches!(
                    node.purpose.status,
                    PurposeStatus::Missing | PurposeStatus::Suggested
                )
            })
            .map(|node| {
                purpose_curation_candidate(project_instance_id, active_generation, &task, node)
            })
            .collect::<Vec<_>>();
        let work_key =
            purpose_curation_batch_work_key(project_instance_id, active_generation, &task, &items);
        Ok(PurposeCurationBatch {
            project_instance_id,
            active_generation,
            task,
            work_key,
            items,
        })
    }

    /// Load symbol relations filtered by optional file path and query.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbol_relations(
        &self,
        file: Option<&str>,
        query: Option<&str>,
        limit: usize,
    ) -> DbResult<Vec<SymbolRelation>> {
        let max_rows = usize_to_i64(limit.max(1));
        match (file, query) {
            (Some(file), Some(query)) => self.query_relations(
                "
                SELECT path, source_name, target_name, kind, line, context, parser
                FROM symbol_relations
                WHERE path = ?1 AND (source_name LIKE ?2 OR target_name LIKE ?2 OR context LIKE ?2)
                ORDER BY path, line, source_name, target_name
                LIMIT ?3
                ",
                params![file, like_query(query), max_rows],
            ),
            (Some(file), None) => self.query_relations(
                "
                SELECT path, source_name, target_name, kind, line, context, parser
                FROM symbol_relations
                WHERE path = ?1
                ORDER BY path, line, source_name, target_name
                LIMIT ?2
                ",
                params![file, max_rows],
            ),
            (None, Some(query)) => self.query_relations(
                "
                SELECT path, source_name, target_name, kind, line, context, parser
                FROM symbol_relations
                WHERE source_name LIKE ?1 OR target_name LIKE ?1 OR context LIKE ?1 OR path LIKE ?1
                ORDER BY path, line, source_name, target_name
                LIMIT ?2
                ",
                params![like_query(query), max_rows],
            ),
            (None, None) => self.query_relations(
                "
                SELECT path, source_name, target_name, kind, line, context, parser
                FROM symbol_relations
                ORDER BY path, line, source_name, target_name
                LIMIT ?1
                ",
                params![max_rows],
            ),
        }
    }

    /// Load symbol relations for a file and exact relation kind.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_symbol_relations_by_kind(
        &self,
        file: &str,
        kind: RelationKind,
        limit: usize,
    ) -> DbResult<Vec<SymbolRelation>> {
        let max_rows = usize_to_i64(limit.max(1));
        self.query_relations(
            "
            SELECT path, source_name, target_name, kind, line, context, parser
            FROM symbol_relations
            WHERE path = ?1 AND kind = ?2
            ORDER BY path, line, source_name, target_name
            LIMIT ?3
            ",
            params![file, kind.to_string(), max_rows],
        )
    }

    /// Count symbol relations for a file and exact relation kind.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn count_symbol_relations_by_kind(
        &self,
        file: &str,
        kind: RelationKind,
    ) -> DbResult<usize> {
        let count = self.connection.query_row(
            "SELECT COUNT(*) FROM symbol_relations WHERE path = ?1 AND kind = ?2",
            params![file, kind.to_string()],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(i64_to_usize(count))
    }

    /// Load distinct relation targets for a file and exact relation kind.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_distinct_relation_targets_by_kind(
        &self,
        file: &str,
        kind: RelationKind,
        limit: usize,
    ) -> DbResult<Vec<String>> {
        let max_rows = usize_to_i64(limit.max(1));
        let mut statement = self.connection.prepare(
            "
            SELECT DISTINCT target_name
            FROM symbol_relations
            WHERE path = ?1 AND kind = ?2
            ORDER BY target_name
            LIMIT ?3
            ",
        )?;
        let rows = statement.query_map(params![file, kind.to_string(), max_rows], |row| {
            row.get::<_, String>(0)
        })?;
        let mut targets = Vec::new();
        for row in rows {
            targets.push(row?);
        }
        Ok(targets)
    }

    /// Count distinct relation targets for a file and exact relation kind.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn count_distinct_relation_targets_by_kind(
        &self,
        file: &str,
        kind: RelationKind,
    ) -> DbResult<usize> {
        let count = self.connection.query_row(
            "SELECT COUNT(DISTINCT target_name) FROM symbol_relations WHERE path = ?1 AND kind = ?2",
            params![file, kind.to_string()],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(i64_to_usize(count))
    }

    /// Load call relations targeting any of the requested symbol names.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_call_relations_to_targets(
        &self,
        target_names: &[String],
        limit_per_target: usize,
    ) -> DbResult<Vec<SymbolRelation>> {
        if target_names.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = numbered_placeholders(1, target_names.len());
        let limit_placeholder = target_names.len() + 1;
        let sql = format!(
            "
            SELECT path, source_name, target_name, kind, line, context, parser
            FROM (
                SELECT
                    path,
                    source_name,
                    target_name,
                    kind,
                    line,
                    context,
                    parser,
                    ROW_NUMBER() OVER (
                        PARTITION BY target_name
                        ORDER BY path, line, source_name, target_name
                    ) AS target_row
                FROM symbol_relations INDEXED BY idx_symbol_relations_target
                WHERE kind = 'calls' AND target_name IN ({placeholders})
            )
            WHERE target_row <= ?{limit_placeholder}
            ORDER BY path, line, source_name, target_name
            "
        );
        let mut values = target_names
            .iter()
            .map(|target| Value::Text(target.clone()))
            .collect::<Vec<_>>();
        values.push(Value::Integer(usize_to_i64(limit_per_target.max(1))));
        let mut relations = self.query_relations(&sql, params_from_iter(values.iter()))?;
        relations.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.source_name.cmp(&right.source_name))
                .then_with(|| left.target_name.cmp(&right.target_name))
        });
        relations.dedup_by(|left, right| {
            left.path == right.path
                && left.source_name == right.source_name
                && left.target_name == right.target_name
                && left.kind == right.kind
                && left.line == right.line
        });
        Ok(relations)
    }

    /// Load import relations whose persisted target text mentions any term.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn load_import_relations_matching_targets(
        &self,
        terms: &[String],
        limit_per_term: usize,
    ) -> DbResult<Vec<StoredImportRelation>> {
        let mut unique_terms = terms.to_vec();
        unique_terms.sort();
        unique_terms.dedup();
        let mut relations = Vec::new();
        for term in unique_terms.iter().filter(|term| !term.trim().is_empty()) {
            let mut statement = self.connection.prepare_cached(
                "
                SELECT path, source_name, target_name, line
                FROM symbol_relations INDEXED BY idx_symbol_import_alias_lookup
                WHERE kind = 'imports' AND target_name LIKE ?1 ESCAPE '\\'
                ORDER BY path, line, source_name, target_name
                LIMIT ?2
                ",
            )?;
            let rows = statement.query_map(
                params![
                    sqlite_like_pattern(term),
                    usize_to_i64(limit_per_term.max(1))
                ],
                stored_import_relation_from_row,
            )?;
            for row in rows {
                relations.push(row?);
            }
        }
        relations.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.source_name.cmp(&right.source_name))
                .then_with(|| left.target_name.cmp(&right.target_name))
        });
        relations.dedup_by(|left, right| {
            left.path == right.path
                && left.source_name == right.source_name
                && left.target_name == right.target_name
                && left.line == right.line
        });
        Ok(relations)
    }

    /// Load bounded import facts for one exact caller path.
    ///
    /// # Errors
    ///
    /// Returns an error if the covering indexed read fails.
    pub fn load_import_relations_for_path(
        &self,
        path: &str,
        limit: usize,
    ) -> DbResult<Vec<StoredImportRelation>> {
        let mut statement = self.connection.prepare_cached(
            "
            SELECT path, source_name, target_name, line
            FROM symbol_relations INDEXED BY idx_symbol_import_alias_lookup
            WHERE kind = 'imports' AND path = ?1
            ORDER BY path, line, source_name, target_name
            LIMIT ?2
            ",
        )?;
        let rows = statement.query_map(
            params![path, usize_to_i64(limit.max(1))],
            stored_import_relation_from_row,
        )?;
        let mut relations = Vec::new();
        for row in rows {
            relations.push(row?);
        }
        Ok(relations)
    }

    /// Count persisted symbols.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_count(&self) -> DbResult<usize> {
        let count = self
            .connection
            .query_row("SELECT COUNT(*) FROM symbols", [], |row| {
                row.get::<_, i64>(0)
            })?;
        Ok(i64_to_usize(count))
    }

    /// Count persisted symbol relations.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_relation_count(&self) -> DbResult<usize> {
        let count =
            self.connection
                .query_row("SELECT COUNT(*) FROM symbol_relations", [], |row| {
                    row.get::<_, i64>(0)
                })?;
        count_to_usize("symbol_relations", count)
    }

    /// Count persisted symbols for one file path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_count_for_path(&self, path: &str) -> DbResult<usize> {
        let count = self.connection.query_row(
            "SELECT COUNT(*) FROM symbols WHERE path = ?1",
            [path],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(i64_to_usize(count))
    }

    /// Count persisted symbols for a batch of file paths.
    ///
    /// Paths without symbols are omitted from the returned map.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_counts_for_paths(&self, paths: &[String]) -> DbResult<HashMap<String, usize>> {
        let mut counts = HashMap::new();
        for chunk in paths.chunks(900) {
            if chunk.is_empty() {
                continue;
            }
            let placeholders = vec!["?"; chunk.len()].join(",");
            let sql = format!(
                "SELECT path, COUNT(*) FROM symbols WHERE path IN ({placeholders}) GROUP BY path"
            );
            let mut statement = self.connection.prepare(&sql)?;
            let rows = statement.query_map(params_from_iter(chunk.iter()), |row| {
                let path = row.get::<_, String>(0)?;
                let count = row.get::<_, i64>(1)?;
                Ok((path, i64_to_usize(count)))
            })?;
            for row in rows {
                let (path, count) = row?;
                counts.insert(path, count);
            }
        }
        Ok(counts)
    }

    /// Return exact paths with persisted parser metadata from one bounded path set.
    ///
    /// # Errors
    ///
    /// Returns an error if reading or row decoding fails.
    pub fn source_parse_metadata_paths_for_paths(
        &self,
        paths: &[String],
    ) -> DbResult<HashSet<String>> {
        let mut indexed = HashSet::new();
        for chunk in paths.chunks(900) {
            if chunk.is_empty() {
                continue;
            }
            let placeholders = numbered_placeholders(1, chunk.len());
            let sql = format!(
                "SELECT path FROM source_parse_metadata
                 WHERE path IN ({placeholders})
                 ORDER BY path"
            );
            let mut statement = self.connection.prepare(&sql)?;
            let rows = statement.query_map(params_from_iter(chunk.iter()), |row| {
                row.get::<_, String>(0)
            })?;
            for row in rows {
                indexed.insert(row?);
            }
        }
        Ok(indexed)
    }

    /// Return distinct parser strategies that produced symbols for one path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn symbol_parser_kinds_for_path(&self, path: &str) -> DbResult<Vec<ParserKind>> {
        let mut statement = self.connection.prepare(
            "
            SELECT DISTINCT parser
            FROM symbols
            WHERE path = ?1
            ORDER BY parser
            ",
        )?;
        let rows = statement.query_map([path], |row| {
            Ok(ParserKind::from_db(&row.get::<_, String>(0)?))
        })?;
        let mut parsers = Vec::new();
        for row in rows {
            parsers.push(row?);
        }
        Ok(parsers)
    }

    /// Reconstruct persisted symbol graphs for exact repository paths in bounded batches.
    ///
    /// Paths without parser metadata are omitted. Any selected symbol or relation
    /// without matching metadata, or any metadata count mismatch, fails the whole
    /// operation instead of returning a partial graph set.
    ///
    /// # Errors
    ///
    /// Returns an error for `SQLite` failures, invalid persisted counts or enums,
    /// and inconsistent symbol-graph rows.
    pub fn load_symbol_graphs_for_paths(&self, paths: &[String]) -> DbResult<Vec<SymbolGraph>> {
        const PATHS_PER_QUERY: usize = 900;

        let mut paths = paths.to_vec();
        paths.sort();
        paths.dedup();
        let mut graphs = Vec::with_capacity(paths.len());
        for chunk in paths.chunks(PATHS_PER_QUERY) {
            let placeholders = numbered_placeholders(1, chunk.len());
            let metadata_sql = format!(
                "SELECT path, language, source_parser, fact_parser, symbol_count, relation_count
                   FROM source_parse_metadata
                  WHERE path IN ({placeholders})
                  ORDER BY path"
            );
            let mut metadata_statement = self.connection.prepare(&metadata_sql)?;
            let metadata_rows =
                metadata_statement.query_map(params_from_iter(chunk.iter()), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                })?;
            let mut staged = BTreeMap::new();
            for row in metadata_rows {
                let (path, language, source_parser, fact_parser, symbol_count, relation_count) =
                    row?;
                staged.insert(
                    path.clone(),
                    (
                        SourceParseMetadata {
                            path,
                            language,
                            parser: ParserKind::from_db(&source_parser),
                            symbol_count: count_to_usize(
                                "source_parse_metadata.symbol_count",
                                symbol_count,
                            )?,
                            relation_count: count_to_usize(
                                "source_parse_metadata.relation_count",
                                relation_count,
                            )?,
                        },
                        ParserKind::from_db(&fact_parser),
                        Vec::new(),
                        Vec::new(),
                    ),
                );
            }

            let symbol_sql = format!(
                "SELECT path, language, name, kind, signature, line_start, line_end,
                        parent, parser, detail, exported, documentation,
                        source_byte_start, source_byte_end,
                        source_column_start, source_column_end
                   FROM symbols
                  WHERE path IN ({placeholders})
                  ORDER BY path, line_start, line_end, name, kind"
            );
            for symbol in self.query_symbols(&symbol_sql, params_from_iter(chunk.iter()))? {
                let path = symbol.path.clone();
                let Some((_, _, symbols, _)) = staged.get_mut(&path) else {
                    return Err(DbError::SymbolGraphRowShape {
                        path,
                        reason: "symbol rows require matching parser metadata",
                    });
                };
                symbols.push(symbol);
            }

            let relation_sql = format!(
                "SELECT path, source_name, target_name, kind, line, context, parser
                   FROM symbol_relations
                  WHERE path IN ({placeholders})
                  ORDER BY path, line, source_name, target_name, kind"
            );
            for relation in self.query_relations(&relation_sql, params_from_iter(chunk.iter()))? {
                let path = relation.path.clone();
                let Some((_, _, _, relations)) = staged.get_mut(&path) else {
                    return Err(DbError::SymbolGraphRowShape {
                        path,
                        reason: "relation rows require matching parser metadata",
                    });
                };
                relations.push(relation);
            }

            for (path, (metadata, fact_parser, symbols, relations)) in staged {
                if metadata.symbol_count != symbols.len()
                    || metadata.relation_count != relations.len()
                {
                    return Err(DbError::SymbolGraphRowShape {
                        path,
                        reason: "parser metadata counts do not match persisted rows",
                    });
                }
                graphs.push(SymbolGraph {
                    path: metadata.path,
                    language: metadata.language,
                    parser: fact_parser,
                    symbols,
                    relations,
                });
            }
        }
        Ok(graphs)
    }

    /// Load file-level parser metadata for one path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or stored counts are invalid.
    pub fn load_source_parse_metadata(&self, path: &str) -> DbResult<Option<SourceParseMetadata>> {
        self.connection
            .query_row(
                "
                SELECT path, language, source_parser, symbol_count, relation_count
                FROM source_parse_metadata
                WHERE path = ?1
                ",
                [path],
                |row| {
                    let symbol_count = row.get::<_, i64>(3)?;
                    let relation_count = row.get::<_, i64>(4)?;
                    Ok(SourceParseMetadata {
                        path: row.get(0)?,
                        language: row.get(1)?,
                        parser: ParserKind::from_db(&row.get::<_, String>(2)?),
                        symbol_count: i64_to_usize(symbol_count),
                        relation_count: i64_to_usize(relation_count),
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Load the maximum indexed symbol end line for one file path.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn max_symbol_end_line_for_path(&self, path: &str) -> DbResult<usize> {
        let line = self.connection.query_row(
            "SELECT COALESCE(MAX(line_end), 0) FROM symbols WHERE path = ?1",
            [path],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(i64_to_usize(line))
    }

    /// Query symbols with a caller-provided statement and parameters.
    fn query_symbols<P>(&self, sql: &str, params: P) -> DbResult<Vec<CodeSymbol>>
    where
        P: rusqlite::Params,
    {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map(params, code_symbol_from_row)?;
        let mut symbols = Vec::new();
        for row in rows {
            symbols.push(row?);
        }
        Ok(symbols)
    }

    /// Query relations with a caller-provided statement and parameters.
    fn query_relations<P>(&self, sql: &str, params: P) -> DbResult<Vec<SymbolRelation>>
    where
        P: rusqlite::Params,
    {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map(params, |row| {
            let kind_value: String = row.get(3)?;
            let relation_kind = RelationKind::from_db(&kind_value).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::other(format!(
                        "invalid relation kind {kind_value}"
                    ))),
                )
            })?;
            Ok(SymbolRelation {
                path: row.get(0)?,
                source_name: row.get(1)?,
                target_name: row.get(2)?,
                kind: relation_kind,
                line: i64_to_usize(row.get::<_, i64>(4)?),
                context: row.get(5)?,
                parser: ParserKind::from_db(&row.get::<_, String>(6)?),
            })
        })?;
        let mut relations = Vec::new();
        for row in rows {
            relations.push(row?);
        }
        Ok(relations)
    }

    /// Return whether any accepted agent-authored purpose can affect navigation.
    ///
    /// # Errors
    ///
    /// Returns an error when the bounded indexed lookup fails.
    pub fn has_agent_approved_purpose(&self) -> DbResult<bool> {
        self.connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1
                    FROM purposes INDEXED BY idx_purposes_status
                    WHERE status = 'approved' AND source = 'agent'
                    LIMIT 1
                )",
                [],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// Persist a purpose for a path.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist or persistence fails.
    pub fn set_purpose(&self, path: &str, purpose: &str, source: PurposeSource) -> DbResult<()> {
        self.with_validated_write(|connection| {
            let node_id = self.node_id_for_path(path)?;
            let changed = connection
                .prepare_cached(
                    "
            INSERT INTO purposes(node_id, purpose, source, status, updated_at)
            VALUES(?1, ?2, ?3, 'approved', CURRENT_TIMESTAMP)
            ON CONFLICT(node_id) DO UPDATE SET
                purpose = excluded.purpose,
                source = excluded.source,
                status = 'approved',
                updated_at = CURRENT_TIMESTAMP
            WHERE purposes.purpose <> excluded.purpose
               OR purposes.source <> excluded.source
               OR purposes.status <> 'approved'
            ",
                )?
                .execute(params![node_id, purpose, source.to_string()])?;
            if changed > 0 {
                advance_authored_purpose_revision(connection)?;
            }
            Ok(())
        })
    }

    /// Return the accepted authored-purpose revision captured by this connection.
    ///
    /// Databases created before the revision contract have revision zero until
    /// their first accepted purpose mutation.
    ///
    /// # Errors
    ///
    /// Returns an error when the metadata value is corrupt.
    pub fn authored_purpose_revision(&self) -> DbResult<u64> {
        load_authored_purpose_revision(&self.connection)
    }

    /// Approve one purpose only while its curator work and unapproved row are current.
    ///
    /// This path never changes an accepted purpose. Call [`Self::set_purpose`]
    /// for a deliberate correction after an agent or user identifies one.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid task input, binding/schema failure, or a
    /// failed atomic `SQLite` transaction. Stale and accepted rows are returned
    /// as typed non-error states.
    pub fn conditionally_set_purpose(
        &self,
        task: &str,
        path: &str,
        work_key: &str,
        state_token: &str,
        purpose: &str,
    ) -> DbResult<PurposeConditionalApplyState> {
        let request = PurposeConditionalApplyRequest {
            task: task.to_string(),
            path: path.to_string(),
            work_key: work_key.to_string(),
            state_token: state_token.to_string(),
            purpose: purpose.to_string(),
        };
        let mut results = self.conditionally_set_purposes(&[request])?;
        Ok(results
            .pop()
            .map_or(PurposeConditionalApplyState::PathUnavailable, |result| {
                result.state
            }))
    }

    /// Apply a bounded stale-safe purpose-review batch in one writer transaction.
    ///
    /// Current-row lookup and conditional-update statements are cached and
    /// reused for every item. A stale item leaves independent matching rows
    /// eligible within the same host-owned batch.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid task input, oversized input, binding/schema
    /// failure, or any failed statement/commit. A database error rolls back the
    /// complete batch.
    pub fn conditionally_set_purposes(
        &self,
        requests: &[PurposeConditionalApplyRequest],
    ) -> DbResult<Vec<PurposeConditionalApplyResult>> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }
        if requests.len() > MAX_PURPOSE_CURATION_BATCH_ROWS {
            return Err(DbError::PurposeCurationBatchTooLarge {
                requested: requests.len(),
                maximum: MAX_PURPOSE_CURATION_BATCH_ROWS,
            });
        }
        let normalized = requests
            .iter()
            .map(|request| {
                normalize_purpose_curation_task(&request.task).map(|task| {
                    PurposeConditionalApplyRequest {
                        task,
                        path: request.path.clone(),
                        work_key: request.work_key.clone(),
                        state_token: request.state_token.clone(),
                        purpose: request.purpose.clone(),
                    }
                })
            })
            .collect::<DbResult<Vec<_>>>()?;
        self.with_validated_write(|connection| {
            let (project_instance_id, active_generation) =
                self.purpose_curation_context(connection)?;
            let results = normalized
                .iter()
                .map(|request| {
                    apply_conditional_purpose(
                        connection,
                        project_instance_id,
                        active_generation,
                        request,
                    )
                })
                .collect::<DbResult<Vec<_>>>()?;
            if results
                .iter()
                .any(|result| matches!(result.state, PurposeConditionalApplyState::Applied))
            {
                advance_authored_purpose_revision(connection)?;
            }
            Ok(results)
        })
    }

    /// Persist a non-approved purpose suggestion for a path.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist or persistence fails.
    pub fn set_suggested_purpose(&self, path: &str, purpose: &str) -> DbResult<()> {
        self.with_validated_write(|connection| {
            let node_id = self.node_id_for_path(path)?;
            connection
                .prepare_cached(
                    "
            INSERT INTO purposes(node_id, purpose, source, status, updated_at)
            VALUES(?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)
            ON CONFLICT(node_id) DO UPDATE SET
                purpose = excluded.purpose,
                source = excluded.source,
                status = excluded.status,
                updated_at = CURRENT_TIMESTAMP
            WHERE purposes.status IN (?5, ?6)
            ",
                )?
                .execute(params![
                    node_id,
                    purpose,
                    PurposeSource::Generated.to_string(),
                    PurposeStatus::Suggested.as_str(),
                    PurposeStatus::Missing.as_str(),
                    PurposeStatus::Suggested.as_str(),
                ])?;
            Ok(())
        })
    }

    /// Load a node id for a repository path.
    fn node_id_for_path(&self, path: &str) -> DbResult<i64> {
        self.connection
            .prepare_cached("SELECT id FROM nodes WHERE path = ?1 AND exists_now = 1")?
            .query_row([path], |row| row.get::<_, i64>(0))
            .optional()?
            .ok_or_else(|| DbError::PathNotIndexed {
                path: path.to_string(),
            })
    }

    /// Load existing nodes with purpose state.
    ///
    /// # Errors
    ///
    /// Returns an error if reading or enum conversion fails.
    pub fn load_nodes(&self) -> DbResult<Vec<IndexedNode>> {
        let mut statement = self.connection.prepare(
            "
            SELECT
                n.path,
                n.kind,
                n.parent_path,
                n.extension,
                n.language,
                n.size_bytes,
                n.mtime_ns,
                n.content_hash,
                p.purpose,
                p.source,
                p.status,
                s.summary
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            LEFT JOIN summaries s ON s.node_id = n.id
                AND s.summary_level = 'node'
                AND s.subject = ''
            WHERE n.exists_now = 1
            ORDER BY n.path
            ",
        )?;
        let rows = statement.query_map([], |row| {
            let kind_value: String = row.get(1)?;
            let source_value: String = row.get(9)?;
            let status_value: String = row.get(10)?;
            Ok((
                row.get::<_, String>(0)?,
                kind_value,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                option_u64_from_sql(row, 5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                source_value,
                status_value,
                row.get::<_, Option<String>>(11)?,
            ))
        })?;
        let mut nodes = Vec::new();
        for row in rows {
            let (
                path,
                kind_value,
                parent_path,
                extension,
                language,
                size_bytes,
                mtime_ns,
                content_hash,
                purpose,
                source_value,
                status_value,
                summary,
            ) = row?;
            let kind = NodeKind::from_db(&kind_value).ok_or_else(|| DbError::InvalidEnum {
                field: "kind",
                value: kind_value,
            })?;
            let source = parse_source(&source_value)?;
            let status =
                PurposeStatus::from_db(&status_value).ok_or_else(|| DbError::InvalidEnum {
                    field: "status",
                    value: status_value,
                })?;
            nodes.push(IndexedNode {
                node: Node {
                    path: path.clone(),
                    kind,
                    parent_path,
                    extension,
                    language,
                    size_bytes,
                    mtime_ns,
                    content_hash,
                },
                purpose: Purpose {
                    path,
                    purpose,
                    source,
                    status,
                },
                summary,
            });
        }
        Ok(nodes)
    }

    /// Load a bounded ranked node list directly from `SQLite`.
    ///
    /// This is the hot path for agent orientation commands. It keeps large
    /// repositories from materializing every indexed path just to answer a
    /// top-N folder or file query.
    ///
    /// # Errors
    ///
    /// Returns an error if reading or enum conversion fails.
    pub fn load_ranked_nodes(
        &self,
        query: &str,
        kind: NodeKind,
        folder: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> DbResult<Vec<IndexedNode>> {
        let terms = normalize_query_terms(query);
        let exact_query = normalize_exact_ranked_query(query);
        let exact_name_enabled = !exact_query.is_empty() && !exact_query.contains('/');
        let exact_name_pattern = format!("%/{}", sqlite_like_escape(&exact_query));
        let reviewed_match_expression = reviewed_purpose_match_expression(terms.len());
        let score_expression = ranked_score_expression(terms.len());
        let mut sql = format!(
            "
            SELECT path, kind, parent_path, extension, language, size_bytes, mtime_ns,
                   content_hash, purpose, source, status, summary
            FROM (
                SELECT
                    n.path,
                    n.kind,
                    n.parent_path,
                    n.extension,
                    n.language,
                    n.size_bytes,
                    n.mtime_ns,
                    n.content_hash,
                    p.purpose,
                    p.source,
                    p.status,
                    s.summary,
                    CASE WHEN lower(n.path) = ? THEN 1 ELSE 0 END AS exact_path,
                    CASE WHEN ? = 1
                              AND (lower(n.path) = ? OR lower(n.path) LIKE ? ESCAPE '\\')
                         THEN 1 ELSE 0 END AS exact_name,
                    {reviewed_match_expression} AS reviewed_purpose,
                    {score_expression} AS score
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                LEFT JOIN summaries s ON s.node_id = n.id
                    AND s.summary_level = 'node'
                    AND s.subject = ''
                LEFT JOIN summaries symbol_summaries ON symbol_summaries.node_id = n.id
                    AND symbol_summaries.summary_level = 'search'
                    AND symbol_summaries.subject = 'symbols'
                WHERE n.exists_now = 1
                  AND n.kind = ?
            "
        );
        let mut values = vec![
            Value::from(exact_query.clone()),
            Value::from(i64::from(exact_name_enabled)),
            Value::from(exact_query),
            Value::from(exact_name_pattern),
        ];
        for term in &terms {
            values.push(Value::from(sqlite_like_pattern(term)));
        }
        for term in &terms {
            let pattern = sqlite_like_pattern(term);
            values.push(Value::from(pattern.clone()));
            values.push(Value::from(pattern.clone()));
            values.push(Value::from(pattern.clone()));
            values.push(Value::from(pattern.clone()));
            values.push(Value::from(pattern));
        }
        values.push(Value::from(kind.to_string()));
        if kind == NodeKind::File
            && let Some(folder) = folder.filter(|folder| !folder.is_empty() && *folder != ".")
        {
            sql.push_str(" AND (n.parent_path = ? OR n.parent_path LIKE ? ESCAPE '\\')");
            values.push(Value::from(folder.to_string()));
            values.push(Value::from(sqlite_descendant_pattern(folder)));
        }
        sql.push_str(
            "
            )
            WHERE score > 0 OR exact_path > 0 OR exact_name > 0 OR reviewed_purpose > 0
            ORDER BY exact_path DESC, exact_name DESC, reviewed_purpose DESC, score DESC, path
            LIMIT ?
            OFFSET ?
            ",
        );
        values.push(Value::from(usize_to_i64(limit.max(1))));
        values.push(Value::from(usize_to_i64(offset)));

        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values))?;
        let mut nodes = Vec::new();
        while let Some(row) = rows.next()? {
            nodes.push(indexed_node_from_sql_row(row)?);
        }
        Ok(nodes)
    }

    /// Sum indexed source bytes represented by file nodes.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or the aggregate cannot fit in `usize`.
    pub fn source_file_byte_count(&self, folder: Option<&str>) -> DbResult<usize> {
        let mut sql = String::from(
            "
            SELECT COALESCE(SUM(COALESCE(size_bytes, 0)), 0)
            FROM nodes
            WHERE exists_now = 1
              AND kind = 'file'
            ",
        );
        let mut values = Vec::new();
        if let Some(folder) = folder.filter(|folder| !folder.is_empty() && *folder != ".") {
            sql.push_str(" AND (parent_path = ? OR parent_path LIKE ? ESCAPE '\\')");
            values.push(Value::from(folder.to_string()));
            values.push(Value::from(sqlite_descendant_pattern(folder)));
        }
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("source_file_bytes", count)
    }

    /// Visit indexed file paths and source sizes for exact token baselines.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails, stored counts are invalid, or the
    /// visitor returns an error.
    pub fn visit_file_token_estimates<F>(
        &self,
        folder: Option<&str>,
        mut visitor: F,
    ) -> DbResult<()>
    where
        F: FnMut(String, Option<u64>) -> DbResult<bool>,
    {
        let mut sql = String::from(
            "
            SELECT path, size_bytes
            FROM nodes
            WHERE exists_now = 1
              AND kind = 'file'
            ",
        );
        let mut values = Vec::new();
        if let Some(folder) = folder.filter(|folder| !folder.is_empty() && *folder != ".") {
            sql.push_str(" AND (parent_path = ? OR parent_path LIKE ? ESCAPE '\\')");
            values.push(Value::from(folder.to_string()));
            values.push(Value::from(sqlite_descendant_pattern(folder)));
        }
        sql.push_str(" ORDER BY path");
        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values))?;
        while let Some(row) = rows.next()? {
            if !visitor(row.get::<_, String>(0)?, option_u64_from_sql(row, 1)?)? {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Build unresolved health findings without loading the full node table.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or stored enum values are invalid.
    pub fn unresolved_health_findings(
        &self,
        resolved_ids: &[String],
    ) -> DbResult<Vec<HealthFinding>> {
        let mut findings = Vec::new();
        self.visit_unresolved_health_findings(resolved_ids, |finding| {
            findings.push(finding);
            Ok(true)
        })?;
        Ok(findings)
    }

    /// Build a bounded unresolved health findings page.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or stored enum values are invalid.
    pub fn unresolved_health_findings_page(
        &self,
        resolved_ids: &[String],
        query: &HealthQuery,
    ) -> DbResult<HealthFindingsPage> {
        self.unresolved_health_findings_page_with_filter(
            HealthResolutionFilter::Explicit(resolved_ids),
            query,
        )
    }

    /// Build a bounded page filtered by this store's durable resolutions.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or stored enum values are invalid.
    pub fn unresolved_health_findings_page_current(
        &self,
        query: &HealthQuery,
    ) -> DbResult<HealthFindingsPage> {
        self.unresolved_health_findings_page_with_filter(HealthResolutionFilter::Stored, query)
    }

    /// Build the actionable missing/suggested purpose queue for one low-scope request.
    ///
    /// Accepted and legacy-stale purposes are excluded; deliberate accepted-purpose
    /// correction remains owned by the explicit purpose-set/review path.
    ///
    /// # Errors
    ///
    /// Returns an error if counting, paging, or stored-state conversion fails.
    pub fn purpose_curation_findings_page_current(
        &self,
        query: &HealthQuery,
    ) -> DbResult<HealthFindingsPage> {
        let specs = &PURPOSE_HEALTH_SPECS[..2];
        let unfiltered_total = specs.iter().try_fold(0_usize, |total, spec| {
            self.count_purpose_status_findings(
                *spec,
                None,
                HealthResolutionFilter::Stored,
                HealthScope::all(),
            )
            .map(|count| total + count)
        })?;
        if query
            .severity
            .is_some_and(|severity| severity != Severity::Warning)
        {
            return Ok(HealthFindingsPage {
                total: 0,
                unfiltered_total,
                returned: 0,
                start_index: query.start_index,
                limit: query.limit,
                findings: Vec::new(),
            });
        }

        let matching_specs = query.category.as_deref().map_or(specs, |category| {
            specs
                .iter()
                .find(|spec| spec.category == category)
                .map_or(&[][..], std::slice::from_ref)
        });
        let total = self.count_purpose_lifecycle_findings(
            matching_specs,
            query.path_prefix.as_deref(),
            HealthResolutionFilter::Stored,
            query.scope,
        )?;
        let findings = if query.summary_only {
            Vec::new()
        } else {
            self.load_purpose_lifecycle_findings_page(
                matching_specs,
                query.path_prefix.as_deref(),
                HealthResolutionFilter::Stored,
                query.scope,
                query.start_index,
                query.limit,
            )?
        };
        Ok(HealthFindingsPage {
            total,
            unfiltered_total,
            returned: findings.len(),
            start_index: query.start_index,
            limit: query.limit,
            findings,
        })
    }

    /// Count all unresolved findings without materializing finding rows or resolution ids.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or stored enum values are invalid.
    pub fn unresolved_health_finding_count_current(&self) -> DbResult<usize> {
        self.unresolved_health_findings_page_current(&HealthQuery {
            start_index: 0,
            limit: 0,
            category: None,
            severity: None,
            path_prefix: None,
            summary_only: true,
            scope: HealthScope::all(),
        })
        .map(|page| page.total)
    }

    /// Build a bounded unresolved page with caller-owned or store-owned filtering.
    fn unresolved_health_findings_page_with_filter(
        &self,
        resolution_filter: HealthResolutionFilter<'_>,
        query: &HealthQuery,
    ) -> DbResult<HealthFindingsPage> {
        let mut unfiltered_total = 0_usize;
        let mut total = 0_usize;
        let mut findings = Vec::new();

        for spec in PURPOSE_HEALTH_SPECS {
            unfiltered_total += self.count_purpose_status_findings(
                spec,
                None,
                resolution_filter,
                HealthScope::all(),
            )?;
        }

        let scope = query.scope;
        if scope.high_impact_queue() && query.category.is_none() {
            let matching_count = if query
                .severity
                .is_none_or(|severity| severity == Severity::Warning)
            {
                self.count_purpose_lifecycle_findings(
                    &PURPOSE_HEALTH_SPECS,
                    query.path_prefix.as_deref(),
                    resolution_filter,
                    scope,
                )?
            } else {
                0
            };
            if !query.summary_only
                && findings.len() < query.limit
                && total + matching_count > query.start_index
            {
                let local_start = query.start_index.saturating_sub(total);
                let local_limit = query.limit - findings.len();
                findings.extend(self.load_purpose_lifecycle_findings_page(
                    &PURPOSE_HEALTH_SPECS,
                    query.path_prefix.as_deref(),
                    resolution_filter,
                    scope,
                    local_start,
                    local_limit,
                )?);
            }
            total += matching_count;
        } else {
            for spec in PURPOSE_HEALTH_SPECS {
                if !purpose_health_spec_matches_query(spec, query) {
                    continue;
                }

                let matching_count = self.count_purpose_status_findings(
                    spec,
                    query.path_prefix.as_deref(),
                    resolution_filter,
                    scope,
                )?;
                if !query.summary_only
                    && findings.len() < query.limit
                    && total + matching_count > query.start_index
                {
                    let local_start = query.start_index.saturating_sub(total);
                    let local_limit = query.limit - findings.len();
                    findings.extend(self.load_purpose_status_findings_page(
                        spec,
                        query.path_prefix.as_deref(),
                        resolution_filter,
                        scope,
                        local_start,
                        local_limit,
                    )?);
                }
                total += matching_count;
            }
        }

        for category in STRUCTURAL_HEALTH_CATEGORIES {
            let unfiltered_scope = if category == CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED {
                HealthScope::purpose_strict()
            } else {
                HealthScope::all()
            };
            let unfiltered_count = self.count_structural_health_findings(
                category,
                None,
                resolution_filter,
                unfiltered_scope,
            )?;
            unfiltered_total += unfiltered_count;
            if !health_category_matches_query(category, Severity::Warning, query) {
                continue;
            }
            let matching_count = self.count_structural_health_findings(
                category,
                query.path_prefix.as_deref(),
                resolution_filter,
                scope,
            )?;
            if !query.summary_only
                && findings.len() < query.limit
                && total + matching_count > query.start_index
            {
                let local_start = query.start_index.saturating_sub(total);
                let local_limit = query.limit - findings.len();
                findings.extend(self.load_structural_health_findings_page(
                    category,
                    query.path_prefix.as_deref(),
                    resolution_filter,
                    scope,
                    local_start,
                    local_limit,
                )?);
            }
            total += matching_count;
        }
        Ok(HealthFindingsPage {
            total,
            unfiltered_total,
            returned: findings.len(),
            start_index: query.start_index,
            limit: query.limit,
            findings,
        })
    }

    /// Visit unresolved health findings without materializing the full table.
    fn visit_unresolved_health_findings<F>(
        &self,
        resolved_ids: &[String],
        mut visitor: F,
    ) -> DbResult<()>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        let resolved = resolved_ids.iter().cloned().collect::<HashSet<_>>();
        if !self.visit_purpose_status_findings(PURPOSE_HEALTH_SPECS[0], &resolved, &mut visitor)? {
            return Ok(());
        }
        if !self.visit_purpose_status_findings(PURPOSE_HEALTH_SPECS[1], &resolved, &mut visitor)? {
            return Ok(());
        }
        if !self.visit_purpose_status_findings(PURPOSE_HEALTH_SPECS[2], &resolved, &mut visitor)? {
            return Ok(());
        }
        if !self.visit_agent_review_required_findings(&resolved, &mut visitor)? {
            return Ok(());
        }
        self.visit_structural_health_findings(&resolved, &mut visitor)
    }

    /// Visit structural health findings that are not simple purpose statuses.
    fn visit_structural_health_findings<F>(
        &self,
        resolved_ids: &HashSet<String>,
        mut visitor: F,
    ) -> DbResult<()>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        if !self.visit_duplicate_purpose_findings(resolved_ids, &mut visitor)? {
            return Ok(());
        }
        if !self.visit_repeated_temp_folder_findings(resolved_ids, &mut visitor)? {
            return Ok(());
        }
        Ok(())
    }

    /// Build findings for one purpose lifecycle status.
    fn visit_purpose_status_findings<F>(
        &self,
        spec: PurposeHealthSpec,
        resolved_ids: &HashSet<String>,
        visitor: &mut F,
    ) -> DbResult<bool>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        let mut statement = self.connection.prepare(
            "
            SELECT n.path
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE n.exists_now = 1
              AND p.status = ?1
            ORDER BY n.path
            ",
        )?;
        let rows = statement.query_map([spec.status], |row| row.get::<_, String>(0))?;
        for row in rows {
            let path = row?;
            let finding = HealthFinding {
                id: finding_id(spec.category, &path, None),
                severity: Severity::Warning,
                category: spec.category.to_string(),
                path,
                related_path: None,
                message: spec.message.to_string(),
                recommendation: spec.recommendation.to_string(),
            };
            if !emit_unresolved_finding(finding, resolved_ids, visitor)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Count unresolved purpose lifecycle findings directly in `SQLite`.
    fn count_purpose_lifecycle_findings(
        &self,
        specs: &[PurposeHealthSpec],
        path_prefix: Option<&str>,
        resolution_filter: HealthResolutionFilter<'_>,
        scope: HealthScope,
    ) -> DbResult<usize> {
        if specs.is_empty() {
            return Ok(0);
        }
        let (where_clause, values) =
            purpose_lifecycle_where_clause(specs, path_prefix, resolution_filter, scope);
        let sql = format!(
            "
            SELECT COUNT(*)
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE {where_clause}
            "
        );
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("health_purpose_lifecycle_count", count)
    }

    /// Load one globally ordered purpose lifecycle page directly from `SQLite`.
    fn load_purpose_lifecycle_findings_page(
        &self,
        specs: &[PurposeHealthSpec],
        path_prefix: Option<&str>,
        resolution_filter: HealthResolutionFilter<'_>,
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 || specs.is_empty() {
            return Ok(Vec::new());
        }
        let (where_clause, mut values) =
            purpose_lifecycle_where_clause(specs, path_prefix, resolution_filter, scope);
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let order_by = purpose_default_queue_order_expression("n", "p");
        let sql = format!(
            "
            SELECT n.path, p.status
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE {where_clause}
            ORDER BY {order_by}
            LIMIT ?{limit_placeholder} OFFSET ?{offset_placeholder}
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut findings = Vec::new();
        for row in rows {
            let (path, status) = row?;
            let spec = purpose_health_spec_for_status(&status)?;
            findings.push(HealthFinding {
                id: finding_id(spec.category, &path, None),
                severity: Severity::Warning,
                category: spec.category.to_string(),
                path,
                related_path: None,
                message: spec.message.to_string(),
                recommendation: spec.recommendation.to_string(),
            });
        }
        Ok(findings)
    }

    /// Count unresolved purpose lifecycle findings directly in `SQLite`.
    fn count_purpose_status_findings(
        &self,
        spec: PurposeHealthSpec,
        path_prefix: Option<&str>,
        resolution_filter: HealthResolutionFilter<'_>,
        scope: HealthScope,
    ) -> DbResult<usize> {
        let (where_clause, values) =
            purpose_status_where_clause(spec, path_prefix, resolution_filter, scope);
        let sql = format!(
            "
            SELECT COUNT(*)
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE {where_clause}
            "
        );
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("health_purpose_status_count", count)
    }

    /// Load one bounded unresolved purpose lifecycle page directly from `SQLite`.
    fn load_purpose_status_findings_page(
        &self,
        spec: PurposeHealthSpec,
        path_prefix: Option<&str>,
        resolution_filter: HealthResolutionFilter<'_>,
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let (where_clause, mut values) =
            purpose_status_where_clause(spec, path_prefix, resolution_filter, scope);
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let order_by = if scope.high_impact_queue() {
            purpose_default_queue_order_expression("n", "p")
        } else {
            "n.path".to_string()
        };
        let sql = format!(
            "
            SELECT n.path
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE {where_clause}
            ORDER BY {order_by}
            LIMIT ?{limit_placeholder} OFFSET ?{offset_placeholder}
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| row.get::<_, String>(0))?;
        let mut findings = Vec::new();
        for row in rows {
            let path = row?;
            findings.push(HealthFinding {
                id: finding_id(spec.category, &path, None),
                severity: Severity::Warning,
                category: spec.category.to_string(),
                path,
                related_path: None,
                message: spec.message.to_string(),
                recommendation: spec.recommendation.to_string(),
            });
        }
        Ok(findings)
    }

    /// Count unresolved structural health findings directly in `SQLite`.
    fn count_structural_health_findings(
        &self,
        category: &str,
        path_prefix: Option<&str>,
        resolution_filter: HealthResolutionFilter<'_>,
        scope: HealthScope,
    ) -> DbResult<usize> {
        match category {
            CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED => {
                self.count_agent_review_required_findings(path_prefix, resolution_filter, scope)
            }
            CATEGORY_DUPLICATE_PURPOSE => {
                self.count_duplicate_purpose_findings(path_prefix, resolution_filter, scope)
            }
            CATEGORY_REPEATED_TEMPORARY_FOLDER => {
                self.count_repeated_temp_folder_findings(path_prefix, resolution_filter, scope)
            }
            _ => Ok(0),
        }
    }

    /// Load a bounded unresolved structural health page directly from `SQLite`.
    fn load_structural_health_findings_page(
        &self,
        category: &str,
        path_prefix: Option<&str>,
        resolution_filter: HealthResolutionFilter<'_>,
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        match category {
            CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED => self
                .load_agent_review_required_findings_page(
                    path_prefix,
                    resolution_filter,
                    scope,
                    start_index,
                    limit,
                ),
            CATEGORY_DUPLICATE_PURPOSE => self.load_duplicate_purpose_findings_page(
                path_prefix,
                resolution_filter,
                scope,
                start_index,
                limit,
            ),
            CATEGORY_REPEATED_TEMPORARY_FOLDER => self.load_repeated_temp_folder_findings_page(
                path_prefix,
                resolution_filter,
                scope,
                start_index,
                limit,
            ),
            _ => Ok(Vec::new()),
        }
    }

    /// Visit approved navigation-critical purposes that still need agent review.
    fn visit_agent_review_required_findings<F>(
        &self,
        resolved_ids: &HashSet<String>,
        visitor: &mut F,
    ) -> DbResult<bool>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        let reviewed_sources = sql_string_literals(AGENT_REVIEWED_SOURCE_VALUES);
        let high_impact = high_impact_file_path_expression("lower(n.path)");
        let approved_status = PurposeStatus::Approved.as_str();
        let sql = format!(
            "
            SELECT n.path
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE n.exists_now = 1
              AND p.status = '{approved_status}'
              AND p.source NOT IN ({reviewed_sources})
              AND (n.kind = 'folder' OR (n.kind = 'file' AND {high_impact}))
            ORDER BY CASE WHEN n.kind = 'folder' THEN 0 ELSE 1 END, n.path
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            let finding = agent_review_required_finding(row?);
            if !emit_unresolved_finding(finding, resolved_ids, visitor)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Count approved navigation-critical purposes that still need agent review.
    fn count_agent_review_required_findings(
        &self,
        path_prefix: Option<&str>,
        resolution_filter: HealthResolutionFilter<'_>,
        scope: HealthScope,
    ) -> DbResult<usize> {
        let (where_clause, values) = structural_finding_where_clause(
            CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED,
            path_prefix,
            resolution_filter,
            scope,
            1,
        );
        let source_relevant = source_relevant_node_expression("n");
        let reviewed_sources = sql_string_literals(AGENT_REVIEWED_SOURCE_VALUES);
        let review_candidate = purpose_review_candidate_expression("n", scope);
        let approved_status = PurposeStatus::Approved.as_str();
        let sql = format!(
            "
            WITH findings AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       '' AS related_path,
                       {source_relevant} AS source_relevant
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                WHERE n.exists_now = 1
                  AND p.status = '{approved_status}'
                  AND p.source NOT IN ({reviewed_sources})
                  AND {review_candidate}
            )
            SELECT COUNT(*)
            FROM findings
            {where_clause}
            "
        );
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("health_agent_review_required_count", count)
    }

    /// Load approved navigation-critical purposes that still need agent review.
    fn load_agent_review_required_findings_page(
        &self,
        path_prefix: Option<&str>,
        resolution_filter: HealthResolutionFilter<'_>,
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let (where_clause, mut values) = structural_finding_where_clause(
            CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED,
            path_prefix,
            resolution_filter,
            scope,
            1,
        );
        let source_relevant = source_relevant_node_expression("n");
        let reviewed_sources = sql_string_literals(AGENT_REVIEWED_SOURCE_VALUES);
        let review_candidate = purpose_review_candidate_expression("n", scope);
        let approved_status = PurposeStatus::Approved.as_str();
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let sql = format!(
            "
            WITH findings AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       '' AS related_path,
                       {source_relevant} AS source_relevant
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                WHERE n.exists_now = 1
                  AND p.status = '{approved_status}'
                  AND p.source NOT IN ({reviewed_sources})
                  AND {review_candidate}
            )
            SELECT path
            FROM findings
            {where_clause}
            ORDER BY CASE WHEN kind = 'folder' THEN 0 ELSE 1 END, path
            LIMIT ?{limit_placeholder} OFFSET ?{offset_placeholder}
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| row.get::<_, String>(0))?;
        let mut findings = Vec::new();
        for row in rows {
            findings.push(agent_review_required_finding(row?));
        }
        Ok(findings)
    }

    /// Count duplicate-purpose findings directly in `SQLite`.
    fn count_duplicate_purpose_findings(
        &self,
        path_prefix: Option<&str>,
        resolution_filter: HealthResolutionFilter<'_>,
        scope: HealthScope,
    ) -> DbResult<usize> {
        let (where_clause, values) = structural_finding_where_clause(
            CATEGORY_DUPLICATE_PURPOSE,
            path_prefix,
            resolution_filter,
            scope,
            1,
        );
        let source_relevant = source_relevant_node_expression("n");
        let duplicate_scope =
            "CASE WHEN n.kind = 'folder' THEN COALESCE(n.parent_path, '') ELSE '' END";
        let approved_status = PurposeStatus::Approved.as_str();
        let sql = format!(
            "
            WITH duplicate_rows AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       p.purpose,
                       {source_relevant} AS source_relevant,
                       FIRST_VALUE(n.path) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS related_path,
                       ROW_NUMBER() OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS duplicate_rank,
                       COUNT(*) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                       ) AS duplicate_count
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                WHERE n.exists_now = 1
                  AND p.status = '{approved_status}'
                  AND p.purpose IS NOT NULL
            ),
            findings AS (
                SELECT path, kind, language, purpose, related_path, source_relevant
                FROM duplicate_rows
                WHERE duplicate_count > 1
                  AND duplicate_rank > 1
            )
            SELECT COUNT(*)
            FROM findings
            {where_clause}
            "
        );
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("health_duplicate_purpose_count", count)
    }

    /// Load a bounded duplicate-purpose findings page directly in `SQLite`.
    fn load_duplicate_purpose_findings_page(
        &self,
        path_prefix: Option<&str>,
        resolution_filter: HealthResolutionFilter<'_>,
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let (where_clause, mut values) = structural_finding_where_clause(
            CATEGORY_DUPLICATE_PURPOSE,
            path_prefix,
            resolution_filter,
            scope,
            1,
        );
        let source_relevant = source_relevant_node_expression("n");
        let duplicate_scope =
            "CASE WHEN n.kind = 'folder' THEN COALESCE(n.parent_path, '') ELSE '' END";
        let approved_status = PurposeStatus::Approved.as_str();
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let sql = format!(
            "
            WITH duplicate_rows AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       p.purpose,
                       {source_relevant} AS source_relevant,
                       FIRST_VALUE(n.path) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS related_path,
                       ROW_NUMBER() OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS duplicate_rank,
                       COUNT(*) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                       ) AS duplicate_count
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                WHERE n.exists_now = 1
                  AND p.status = '{approved_status}'
                  AND p.purpose IS NOT NULL
            ),
            findings AS (
                SELECT path, kind, language, purpose, related_path, source_relevant
                FROM duplicate_rows
                WHERE duplicate_count > 1
                  AND duplicate_rank > 1
            )
            SELECT path, kind, related_path
            FROM findings
            {where_clause}
            ORDER BY kind, lower(purpose), path
            LIMIT ?{limit_placeholder} OFFSET ?{offset_placeholder}
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut findings = Vec::new();
        for row in rows {
            let (path, kind_value, related_path) = row?;
            let kind = NodeKind::from_db(&kind_value).ok_or_else(|| DbError::InvalidEnum {
                field: "kind",
                value: kind_value,
            })?;
            findings.push(HealthFinding {
                id: finding_id(CATEGORY_DUPLICATE_PURPOSE, &path, Some(&related_path)),
                severity: Severity::Warning,
                category: CATEGORY_DUPLICATE_PURPOSE.to_string(),
                path,
                related_path: Some(related_path),
                message: format!("Multiple {kind} nodes share the same purpose."),
                recommendation: RECOMMENDATION_DUPLICATE_PURPOSE.to_string(),
            });
        }
        Ok(findings)
    }

    /// Count repeated temporary-folder findings directly in `SQLite`.
    fn count_repeated_temp_folder_findings(
        &self,
        path_prefix: Option<&str>,
        resolution_filter: HealthResolutionFilter<'_>,
        scope: HealthScope,
    ) -> DbResult<usize> {
        let mut total = 0_usize;
        for bucket in TEMP_FOLDER_BUCKETS {
            total += self.count_repeated_temp_folder_bucket_findings(
                bucket,
                path_prefix,
                resolution_filter,
                scope,
            )?;
        }
        Ok(total)
    }

    /// Count one repeated temporary-folder bucket directly in `SQLite`.
    fn count_repeated_temp_folder_bucket_findings(
        &self,
        bucket: &str,
        path_prefix: Option<&str>,
        resolution_filter: HealthResolutionFilter<'_>,
        scope: HealthScope,
    ) -> DbResult<usize> {
        let exact = bucket.to_string();
        let suffix = format!("%/{bucket}");
        let (where_clause, mut filter_values) = structural_finding_where_clause(
            CATEGORY_REPEATED_TEMPORARY_FOLDER,
            path_prefix,
            resolution_filter,
            scope,
            3,
        );
        let mut values = vec![Value::from(exact), Value::from(suffix)];
        values.append(&mut filter_values);
        let source_relevant = source_relevant_node_expression("n");
        let sql = format!(
            "
            WITH bucket_rows AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       {source_relevant} AS source_relevant,
                       FIRST_VALUE(n.path) OVER (ORDER BY n.path) AS related_path,
                       ROW_NUMBER() OVER (ORDER BY path) AS duplicate_rank,
                       COUNT(*) OVER () AS duplicate_count
                FROM nodes n
                WHERE n.exists_now = 1
                  AND n.kind = 'folder'
                  AND (lower(n.path) = ?1 OR lower(n.path) LIKE ?2)
            ),
            findings AS (
                SELECT path, kind, language, related_path, source_relevant
                FROM bucket_rows
                WHERE duplicate_count > 1
                  AND duplicate_rank > 1
            )
            SELECT COUNT(*)
            FROM findings
            {where_clause}
            "
        );
        let count = self
            .connection
            .query_row(&sql, params_from_iter(values), |row| row.get::<_, i64>(0))?;
        count_to_usize("health_repeated_temp_count", count)
    }

    /// Load a bounded repeated temporary-folder findings page directly in `SQLite`.
    fn load_repeated_temp_folder_findings_page(
        &self,
        path_prefix: Option<&str>,
        resolution_filter: HealthResolutionFilter<'_>,
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut total = 0_usize;
        let mut findings = Vec::new();
        for bucket in TEMP_FOLDER_BUCKETS {
            let matching_count = self.count_repeated_temp_folder_bucket_findings(
                bucket,
                path_prefix,
                resolution_filter,
                scope,
            )?;
            if findings.len() < limit && total + matching_count > start_index {
                let local_start = start_index.saturating_sub(total);
                let local_limit = limit - findings.len();
                findings.extend(self.load_repeated_temp_folder_bucket_findings_page(
                    bucket,
                    path_prefix,
                    resolution_filter,
                    scope,
                    local_start,
                    local_limit,
                )?);
            }
            total += matching_count;
            if findings.len() >= limit {
                break;
            }
        }
        Ok(findings)
    }

    /// Load one repeated temporary-folder bucket directly in `SQLite`.
    fn load_repeated_temp_folder_bucket_findings_page(
        &self,
        bucket: &str,
        path_prefix: Option<&str>,
        resolution_filter: HealthResolutionFilter<'_>,
        scope: HealthScope,
        start_index: usize,
        limit: usize,
    ) -> DbResult<Vec<HealthFinding>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let exact = bucket.to_string();
        let suffix = format!("%/{bucket}");
        let (where_clause, mut filter_values) = structural_finding_where_clause(
            CATEGORY_REPEATED_TEMPORARY_FOLDER,
            path_prefix,
            resolution_filter,
            scope,
            3,
        );
        let mut values = vec![Value::from(exact), Value::from(suffix)];
        values.append(&mut filter_values);
        let source_relevant = source_relevant_node_expression("n");
        let limit_placeholder = values.len() + 1;
        let offset_placeholder = values.len() + 2;
        values.push(Value::from(usize_to_i64(limit)));
        values.push(Value::from(usize_to_i64(start_index)));
        let sql = format!(
            "
            WITH bucket_rows AS (
                SELECT n.path,
                       n.kind,
                       n.language,
                       {source_relevant} AS source_relevant,
                       FIRST_VALUE(n.path) OVER (ORDER BY n.path) AS related_path,
                       ROW_NUMBER() OVER (ORDER BY n.path) AS duplicate_rank,
                       COUNT(*) OVER () AS duplicate_count
                FROM nodes n
                WHERE n.exists_now = 1
                  AND n.kind = 'folder'
                  AND (lower(n.path) = ?1 OR lower(n.path) LIKE ?2)
            ),
            findings AS (
                SELECT path, kind, language, related_path, source_relevant
                FROM bucket_rows
                WHERE duplicate_count > 1
                  AND duplicate_rank > 1
            )
            SELECT path, related_path
            FROM findings
            {where_clause}
            ORDER BY path
            LIMIT ?{limit_placeholder} OFFSET ?{offset_placeholder}
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut findings = Vec::new();
        for row in rows {
            let (path, related_path) = row?;
            findings.push(HealthFinding {
                id: finding_id(
                    CATEGORY_REPEATED_TEMPORARY_FOLDER,
                    &path,
                    Some(&related_path),
                ),
                severity: Severity::Warning,
                category: CATEGORY_REPEATED_TEMPORARY_FOLDER.to_string(),
                path,
                related_path: Some(related_path),
                message: format!("Repeated temporary/generated folder name `{bucket}` found."),
                recommendation: RECOMMENDATION_REPEATED_TEMPORARY_FOLDER.to_string(),
            });
        }
        Ok(findings)
    }

    /// Visit duplicate-purpose health findings through grouped SQL candidates.
    fn visit_duplicate_purpose_findings<F>(
        &self,
        resolved_ids: &HashSet<String>,
        visitor: &mut F,
    ) -> DbResult<bool>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        let duplicate_scope =
            "CASE WHEN n.kind = 'folder' THEN COALESCE(n.parent_path, '') ELSE '' END";
        let approved_status = PurposeStatus::Approved.as_str();
        let sql = format!(
            "
            WITH duplicate_rows AS (
                SELECT n.path,
                       n.kind,
                       p.purpose,
                       FIRST_VALUE(n.path) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS related_path,
                       ROW_NUMBER() OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                           ORDER BY n.path
                       ) AS duplicate_rank,
                       COUNT(*) OVER (
                           PARTITION BY n.kind, lower(p.purpose), {duplicate_scope}
                       ) AS duplicate_count
                FROM nodes n
                JOIN purposes p ON p.node_id = n.id
                WHERE n.exists_now = 1
                  AND p.status = '{approved_status}'
                  AND p.purpose IS NOT NULL
            )
            SELECT path, kind, purpose, related_path
            FROM duplicate_rows
            WHERE duplicate_count > 1
              AND duplicate_rank > 1
            ORDER BY kind, lower(purpose), path
            "
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (path, kind_value, _purpose, related_path) = row?;
            let kind = NodeKind::from_db(&kind_value).ok_or_else(|| DbError::InvalidEnum {
                field: "kind",
                value: kind_value.clone(),
            })?;
            let finding = HealthFinding {
                id: finding_id(CATEGORY_DUPLICATE_PURPOSE, &path, Some(&related_path)),
                severity: Severity::Warning,
                category: CATEGORY_DUPLICATE_PURPOSE.to_string(),
                path,
                related_path: Some(related_path),
                message: format!("Multiple {kind} nodes share the same purpose."),
                recommendation: RECOMMENDATION_DUPLICATE_PURPOSE.to_string(),
            };
            if !emit_unresolved_finding(finding, resolved_ids, visitor)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Visit repeated temporary/generated folder findings.
    fn visit_repeated_temp_folder_findings<F>(
        &self,
        resolved_ids: &HashSet<String>,
        visitor: &mut F,
    ) -> DbResult<bool>
    where
        F: FnMut(HealthFinding) -> DbResult<bool>,
    {
        for bucket in TEMP_FOLDER_BUCKETS {
            let exact = bucket.to_string();
            let suffix = format!("%/{bucket}");
            let mut statement = self.connection.prepare(
                "
                SELECT path
                FROM nodes
                WHERE exists_now = 1
                  AND kind = 'folder'
                  AND (lower(path) = ?1 OR lower(path) LIKE ?2)
                ORDER BY path
                ",
            )?;
            let rows =
                statement.query_map(params![exact, suffix], |row| row.get::<_, String>(0))?;
            let mut first_path = None;
            for row in rows {
                let path = row?;
                let Some(first_path) = first_path.as_ref() else {
                    first_path = Some(path);
                    continue;
                };
                let finding = HealthFinding {
                    id: finding_id(
                        CATEGORY_REPEATED_TEMPORARY_FOLDER,
                        &path,
                        Some(first_path.as_str()),
                    ),
                    severity: Severity::Warning,
                    category: CATEGORY_REPEATED_TEMPORARY_FOLDER.to_string(),
                    path,
                    related_path: Some(first_path.clone()),
                    message: format!("Repeated temporary/generated folder name `{bucket}` found."),
                    recommendation: RECOMMENDATION_REPEATED_TEMPORARY_FOLDER.to_string(),
                };
                if !emit_unresolved_finding(finding, resolved_ids, visitor)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// Compute an overview from the current index.
    ///
    /// # Errors
    ///
    /// Returns an error if the aggregate query fails or a count is invalid.
    pub fn overview(&self) -> DbResult<Overview> {
        let missing_status = PurposeStatus::Missing.as_str();
        let stale_status = PurposeStatus::Stale.as_str();
        let approved_status = PurposeStatus::Approved.as_str();
        let suggested_status = PurposeStatus::Suggested.as_str();
        let sql = format!(
            "
            SELECT
                COALESCE(SUM(CASE WHEN n.kind = 'file' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN n.kind = 'folder' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN p.status = '{missing_status}' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN p.status = '{stale_status}' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN p.status = '{approved_status}' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN p.status = '{suggested_status}' THEN 1 ELSE 0 END), 0)
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE n.exists_now = 1
            "
        );
        let counts = self.connection.query_row(&sql, [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        Ok(Overview {
            files: count_to_usize("files", counts.0)?,
            folders: count_to_usize("folders", counts.1)?,
            missing_purposes: count_to_usize("missing_purposes", counts.2)?,
            stale_purposes: count_to_usize("stale_purposes", counts.3)?,
            approved_purposes: count_to_usize("approved_purposes", counts.4)?,
            suggested_purposes: count_to_usize("suggested_purposes", counts.5)?,
        })
    }

    /// Record a usage event.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence fails.
    pub fn record_usage(&self, event: &UsageEvent) -> DbResult<()> {
        telemetry::validate_event(event, TelemetryRetentionPolicy::default())?;
        let instance_id = self.library_usage_instance(&event.session_id, false)?;
        match self.record_usage_for_instance(
            instance_id,
            UsageInstanceOwner::LibraryHandle,
            event,
            false,
        ) {
            Err(DbError::TelemetryInstanceInactive) => {
                let replacement = self.library_usage_instance(&event.session_id, true)?;
                self.record_usage_for_instance(
                    replacement,
                    UsageInstanceOwner::LibraryHandle,
                    event,
                    false,
                )
            }
            Err(DbError::TelemetryBaselineCapacity) => {
                let replacement = telemetry::generate_usage_instance_id()?;
                self.seal_usage_instance(instance_id)?;
                self.library_usage_instances
                    .borrow_mut()
                    .insert(event.session_id.clone(), Some(replacement));
                self.record_usage_for_instance(
                    replacement,
                    UsageInstanceOwner::LibraryHandle,
                    event,
                    false,
                )
            }
            result => result,
        }
    }

    /// Return or rotate one bounded direct-library runtime instance per caller label.
    fn library_usage_instance(
        &self,
        caller_label: &str,
        rotate: bool,
    ) -> DbResult<UsageInstanceId> {
        let mut instances = self.library_usage_instances.borrow_mut();
        if !rotate && let Some(instance) = instances.get(caller_label) {
            return instance.ok_or(DbError::TelemetryIdentityUnavailable);
        }
        if !instances.contains_key(caller_label)
            && instances.len() >= TelemetryRetentionPolicy::default().max_retained_labels
        {
            return Err(DbError::TelemetryInstanceCapacity);
        }
        let instance = telemetry::generate_usage_instance_id().ok();
        instances.insert(caller_label.to_string(), instance);
        instance.ok_or(DbError::TelemetryIdentityUnavailable)
    }

    /// Record a usage event for one bounded runtime or invocation instance.
    ///
    /// The internal instance is deliberately separate from the optional
    /// caller-visible label carried by [`UsageEvent::session_id`].
    ///
    /// # Errors
    ///
    /// Returns an error when the selected binding changes, the instance is
    /// inactive, a retention bound rejects the event, or `SQLite` cannot commit
    /// the complete telemetry transaction.
    pub fn record_usage_for_instance(
        &self,
        instance_id: UsageInstanceId,
        owner: UsageInstanceOwner,
        event: &UsageEvent,
        seal_after_record: bool,
    ) -> DbResult<()> {
        self.record_usage_for_instance_origin(instance_id, owner, None, event, seal_after_record)
    }

    /// Record one centrally routed event for a captured worktree registration.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected control binding changes, the
    /// registration or runtime origin conflicts, a retention bound rejects the
    /// event, or `SQLite` cannot commit the complete telemetry transaction.
    pub fn record_usage_for_worktree_instance(
        &self,
        instance_id: UsageInstanceId,
        owner: UsageInstanceOwner,
        registration_id: i64,
        event: &UsageEvent,
        seal_after_record: bool,
    ) -> DbResult<()> {
        self.record_usage_for_instance_origin(
            instance_id,
            owner,
            Some(registration_id),
            event,
            seal_after_record,
        )
    }

    /// Record one event with an optional captured worktree origin.
    fn record_usage_for_instance_origin(
        &self,
        instance_id: UsageInstanceId,
        owner: UsageInstanceOwner,
        registration_id: Option<i64>,
        event: &UsageEvent,
        seal_after_record: bool,
    ) -> DbResult<()> {
        let policy = TelemetryRetentionPolicy::default();
        let project_instance_id = self
            .validated_project_instance_id
            .ok_or(DbError::ProjectInstanceIdentityMissing)?;
        self.with_telemetry_connection(|connection| {
            with_validated_native_write_transaction(
                connection,
                self.validated_project_root_identity.as_ref(),
                Some(project_instance_id),
                |transaction| {
                    telemetry::record_usage_for_project(
                        transaction,
                        project_instance_id,
                        instance_id,
                        owner,
                        registration_id,
                        event,
                        policy,
                        seal_after_record,
                    )
                },
            )?;
            // The event is already committed. Passive maintenance remains
            // observable through retention state and must never make callers
            // retry a successfully persisted event.
            drop(telemetry::maintain_after_commit_for_native_project(
                connection,
                self.validated_project_root_identity.as_ref(),
                project_instance_id,
                policy,
            ));
            Ok(())
        })
    }

    /// Seal one cleanly completed runtime or invocation instance.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected binding changed, the instance is
    /// already inactive, or `SQLite` cannot commit the state transition.
    pub fn seal_usage_instance(&self, instance_id: UsageInstanceId) -> DbResult<()> {
        self.with_telemetry_connection(|connection| {
            with_validated_native_write_transaction(
                connection,
                self.validated_project_root_identity.as_ref(),
                self.validated_project_instance_id,
                |transaction| telemetry::seal_usage_instance(transaction, instance_id),
            )
        })
    }

    /// Return content-free bounded telemetry retention and maintenance state.
    ///
    /// # Errors
    ///
    /// Returns an error when persisted state is missing, corrupt, or cannot be read.
    pub fn telemetry_retention_state(&self) -> DbResult<TelemetryRetentionState> {
        telemetry::retention_state(&self.connection)
    }

    /// Load usage events.
    ///
    /// # Errors
    ///
    /// Returns an error if loading fails.
    pub fn usage_events(&self, session_id: Option<&str>) -> DbResult<Vec<UsageEvent>> {
        telemetry::usage_events(&self.connection, session_id)
    }

    /// Build a token overview.
    ///
    /// # Errors
    ///
    /// Returns an error if loading events fails.
    pub fn token_overview(&self, session_id: Option<&str>) -> DbResult<TokenOverview> {
        telemetry::token_overview(&self.connection, session_id)
    }

    /// Build the control atlas's combined native-main and synchronized-worktree overview.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected binding or aggregate state is invalid,
    /// arithmetic overflows, or `SQLite` cannot complete the bounded read.
    pub fn repository_token_overview(&self) -> DbResult<TokenOverview> {
        telemetry::repository_token_overview(&self.connection)
    }

    /// Build exact retained routed plus synchronized totals for one active alias.
    ///
    /// # Errors
    ///
    /// Returns an error when the alias is absent, aggregate state is invalid,
    /// arithmetic overflows, or `SQLite` cannot complete the bounded read.
    pub fn registered_worktree_token_overview(
        &self,
        alias: &WorktreeAlias,
    ) -> DbResult<TokenOverview> {
        let registration = self.worktree_registration(alias)?;
        telemetry::worktree_token_overview(&self.connection, registration.registration_id)
    }

    /// Build token trend aggregates grouped by day, week, month, or year.
    ///
    /// # Errors
    ///
    /// Returns an error if the window is unsupported or loading events fails.
    pub fn token_trends(
        &self,
        session_id: Option<&str>,
        window: TokenTrendWindow,
    ) -> DbResult<TokenTrendReport> {
        telemetry::token_trends(&self.connection, session_id, window)
    }

    /// Build combined native-main and synchronized-worktree token trends.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported window, invalid aggregate state,
    /// arithmetic overflow, or a bounded `SQLite` read failure.
    pub fn repository_token_trends(&self, window: TokenTrendWindow) -> DbResult<TokenTrendReport> {
        telemetry::repository_token_trends(&self.connection, window)
    }

    /// Build exact retained routed plus synchronized trends for one active alias.
    ///
    /// # Errors
    ///
    /// Returns an error when the alias is absent, the window or aggregate state
    /// is invalid, arithmetic overflows, or `SQLite` cannot complete the read.
    pub fn registered_worktree_token_trends(
        &self,
        alias: &WorktreeAlias,
        window: TokenTrendWindow,
    ) -> DbResult<TokenTrendReport> {
        let registration = self.worktree_registration(alias)?;
        telemetry::worktree_token_trends(&self.connection, registration.registration_id, window)
    }

    /// Mark a deterministic health finding as agent-resolved.
    ///
    /// # Errors
    ///
    /// Returns an error if the finding is not active or persistence fails.
    pub fn resolve_health_finding(&self, resolution: &HealthResolution) -> DbResult<()> {
        self.with_validated_write(|connection| {
            if !self.active_health_finding_matches(resolution)? {
                return Err(DbError::HealthFindingNotActive {
                    finding_id: resolution.finding_id.clone(),
                    category: resolution.category.clone(),
                    path: resolution.path.clone(),
                });
            }
            connection.execute(
                "
            INSERT INTO health_resolutions(
                finding_id,
                category,
                path,
                related_path,
                rationale,
                resolved_by,
                resolved_at
            )
            VALUES(?1, ?2, ?3, ?4, ?5, 'agent', CURRENT_TIMESTAMP)
            ON CONFLICT(finding_id) DO UPDATE SET
                category = excluded.category,
                path = excluded.path,
                related_path = excluded.related_path,
                rationale = excluded.rationale,
                resolved_by = 'agent',
                resolved_at = CURRENT_TIMESTAMP
            ",
                params![
                    resolution.finding_id,
                    resolution.category,
                    resolution.path,
                    resolution.related_path,
                    resolution.rationale,
                ],
            )?;
            Ok(())
        })
    }

    /// Return whether the visible SQL health surface contains the exact finding.
    fn active_health_finding_matches(&self, resolution: &HealthResolution) -> DbResult<bool> {
        const PAGE_SIZE: usize = 256;
        let mut start_index = 0_usize;
        loop {
            let page = self.unresolved_health_findings_page_current(&HealthQuery {
                start_index,
                limit: PAGE_SIZE,
                category: Some(resolution.category.clone()),
                severity: Some(Severity::Warning),
                path_prefix: Some(resolution.path.clone()),
                summary_only: false,
                scope: HealthScope::all(),
            })?;
            if page.findings.iter().any(|finding| {
                finding.id == resolution.finding_id
                    && finding.category == resolution.category
                    && finding.path == resolution.path
                    && finding.related_path == resolution.related_path
            }) {
                return Ok(true);
            }
            if page.returned == 0 || start_index + page.returned >= page.total {
                return Ok(false);
            }
            start_index += page.returned;
        }
    }

    /// Load resolved health finding ids.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails.
    pub fn resolved_health_ids(&self) -> DbResult<Vec<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT finding_id FROM health_resolutions ORDER BY finding_id")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row?);
        }
        Ok(ids)
    }
}

/// Acquire the publication writer without waiting after exact input validation.
fn begin_immediate_publication(connection: &Connection) -> DbResult<()> {
    connection.busy_timeout(SQLITE_PUBLICATION_ACQUIRE_TIMEOUT)?;
    let begin_result = connection.execute_batch("BEGIN IMMEDIATE");
    let restore_result = connection.busy_timeout(SQLITE_BUSY_TIMEOUT);
    match (begin_result, restore_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error.into()),
        (Ok(()), Err(error)) => Err(schema::rollback_after_error(connection, error.into())),
        (Err(operation), Err(restore)) => Err(DbError::PublicationAcquirePolicyRestore {
            operation: Box::new(operation),
            restore: Box::new(restore),
        }),
    }
}

/// Read the recorded project root without creating or migrating a database.
///
/// # Errors
///
/// Returns an error if `SQLite` cannot open or query the database read-only.
pub fn read_project_root_read_only(path: &Path) -> DbResult<Option<String>> {
    schema::read_project_root(path)
}

/// Read an untrusted predecessor root candidate without creating, migrating,
/// or repairing a database.
///
/// This is recovery evidence for selecting a filesystem candidate only. It is
/// never an authoritative project identity and callers must canonicalize the
/// candidate through the live filesystem before opening a project database.
/// Current databases intentionally return no candidate because their typed
/// native identity is the only authority.
///
/// # Errors
///
/// Returns an error when the database cannot be inspected read-only or its
/// predecessor schema is incompatible or malformed.
pub fn read_legacy_project_root_candidate_read_only(path: &Path) -> DbResult<Option<String>> {
    let (preflight, _) = schema::inspect_compatibility(path, None)?;
    Ok((preflight.state == SchemaState::UpgradeRequired)
        .then_some(preflight.project_root)
        .flatten())
}

/// Read the authoritative native project-root identity without mutation.
///
/// # Errors
///
/// Returns an error if the database cannot be opened read-only or the identity
/// row contains an invalid versioned codec payload.
pub fn read_project_root_identity_read_only(path: &Path) -> DbResult<Option<CanonicalProjectRoot>> {
    let location = sqlite_profile::inspect_database_location(path)?;
    if !location.database_exists {
        return Ok(None);
    }
    let (preflight, _) = schema::inspect_compatibility(path, None)?;
    if preflight.state != schema::SchemaState::Current {
        return Ok(None);
    }
    let connection = schema::open_current_read_only(path, None)?.0;
    let identity = project_identity::load_project_root_identity(&connection)?;
    connection.execute_batch("ROLLBACK")?;
    Ok(identity)
}

/// Verify the current project database schema, identity, and full integrity read-only.
///
/// # Errors
///
/// Returns an error when the database is missing, incompatible, corrupt, or belongs to another
/// project root.
pub fn verify_project_database(path: &Path, project_root: &Path) -> DbResult<()> {
    let identity = CanonicalProjectRoot::from_path(project_root)?;
    schema::verify_current_integrity_for_project(path, &identity)
}

/// Normalize a filesystem path stored in `SQLite` metadata.
fn normalize_metadata_path(path: &Path) -> String {
    normalize_native_path_display(path)
}

/// Upsert one metadata value through the caller's active connection/transaction.
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

/// Load the accepted authored-purpose revision without scanning purpose rows.
fn load_authored_purpose_revision(connection: &Connection) -> DbResult<u64> {
    let value = connection
        .prepare_cached("SELECT value FROM metadata WHERE key = ?1")?
        .query_row([AUTHORED_PURPOSE_REVISION_KEY], |row| {
            row.get::<_, String>(0)
        })
        .optional()?;
    value.map_or(Ok(0), |value| {
        value
            .parse::<u64>()
            .map_err(|source| DbError::InvalidInteger {
                field: AUTHORED_PURPOSE_REVISION_KEY,
                value,
                source,
            })
    })
}

/// Advance the accepted authored-purpose revision in the caller's transaction.
fn advance_authored_purpose_revision(connection: &Connection) -> DbResult<u64> {
    let current = load_authored_purpose_revision(connection)?;
    let revision = current
        .checked_add(1)
        .ok_or(DbError::IntegerMetadataOverflow {
            field: AUTHORED_PURPOSE_REVISION_KEY,
            value: current,
        })?;
    set_metadata(
        connection,
        AUTHORED_PURPOSE_REVISION_KEY,
        &revision.to_string(),
    )?;
    Ok(revision)
}

/// Load the authoritative and projection revisions without scanning text rows.
fn load_file_text_fts_revisions(connection: &Connection) -> DbResult<Option<(u64, u64)>> {
    let (source, projection) = connection.query_row(
        "
        SELECT
            (SELECT value FROM metadata WHERE key = ?1),
            (SELECT value FROM metadata WHERE key = ?2)
        ",
        params![
            FILE_TEXT_FTS_SOURCE_REVISION_KEY,
            FILE_TEXT_FTS_PROJECTION_REVISION_KEY,
        ],
        |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
            ))
        },
    )?;
    if source.is_none() && projection.is_none() {
        return Ok(None);
    }
    let (Some(source), Some(projection)) = (source, projection) else {
        return Err(DbError::FileTextFtsStateInvalid {
            reason: "only one FTS revision is present",
        });
    };
    let source_revision =
        source
            .parse::<u64>()
            .map_err(|source_error| DbError::InvalidInteger {
                field: FILE_TEXT_FTS_SOURCE_REVISION_KEY,
                value: source,
                source: source_error,
            })?;
    let projection_revision =
        projection
            .parse::<u64>()
            .map_err(|source_error| DbError::InvalidInteger {
                field: FILE_TEXT_FTS_PROJECTION_REVISION_KEY,
                value: projection,
                source: source_error,
            })?;
    Ok(Some((source_revision, projection_revision)))
}

/// Mark an authoritative text mutation before changing its FTS projection.
fn begin_file_text_fts_update(connection: &Connection) -> DbResult<u64> {
    let (source, projection) =
        load_file_text_fts_revisions(connection)?.ok_or(DbError::FileTextFtsStateInvalid {
            reason: "FTS revisions are missing",
        })?;
    if source != projection {
        return Err(DbError::FileTextFtsStateInvalid {
            reason: "an earlier FTS revision is incomplete",
        });
    }
    let revision = source
        .checked_add(1)
        .ok_or(DbError::FileTextFtsStateInvalid {
            reason: "FTS revision overflowed",
        })?;
    set_metadata(
        connection,
        FILE_TEXT_FTS_SOURCE_REVISION_KEY,
        &revision.to_string(),
    )?;
    Ok(revision)
}

/// Publish the matching projection revision after every FTS mutation succeeds.
fn complete_file_text_fts_update(connection: &Connection, revision: u64) -> DbResult<()> {
    let (source, projection) =
        load_file_text_fts_revisions(connection)?.ok_or(DbError::FileTextFtsStateInvalid {
            reason: "FTS revisions disappeared during an update",
        })?;
    if source != revision || projection.checked_add(1) != Some(revision) {
        return Err(DbError::FileTextFtsStateInvalid {
            reason: "FTS revisions changed during an update",
        });
    }
    set_metadata(
        connection,
        FILE_TEXT_FTS_PROJECTION_REVISION_KEY,
        &revision.to_string(),
    )
}

/// Persist one usage event in the immutable released schema-8 fixture shape.
#[cfg(test)]
fn record_released_schema_eight_usage_event(
    connection: &Connection,
    event: &UsageEvent,
) -> DbResult<()> {
    connection.execute(
        "
        INSERT INTO usage_events(
            session_id,
            command,
            path,
            query,
            estimated_tokens_without_projectatlas,
            estimated_tokens_with_projectatlas,
            estimated_tokens_saved,
            token_savings_bucket,
            provider,
            model,
            tokenizer_backend,
            accuracy,
            baseline_kind,
            confidence,
            calculation_trace,
            accounting_layer,
            estimate_method,
            denominator_kind,
            baseline_identity,
            baseline_fingerprint,
            dedupe_scope,
            created_at
        )
        VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, CURRENT_TIMESTAMP)
        ",
        params![
            event.session_id,
            event.command,
            event.path,
            event.query,
            option_usize_to_i64(
                "estimated_tokens_without_projectatlas",
                event.estimated_tokens_without_projectatlas,
            )?,
            option_usize_to_i64(
                "estimated_tokens_with_projectatlas",
                event.estimated_tokens_with_projectatlas,
            )?,
            event.estimated_tokens_saved,
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
            event.dedupe_scope
        ],
    )?;
    Ok(())
}

/// Load durable publication metadata from one connection snapshot.
fn load_index_publication(connection: &Connection) -> DbResult<Option<IndexPublication>> {
    let row = connection
        .query_row(
            "
            SELECT state.value, fingerprint.value, generation.value
            FROM metadata AS state
            LEFT JOIN metadata AS fingerprint ON fingerprint.key = ?2
            LEFT JOIN metadata AS generation ON generation.key = ?3
            WHERE state.key = ?1
            ",
            params![
                INDEX_PUBLICATION_STATE_KEY,
                INDEX_PUBLICATION_FINGERPRINT_KEY,
                INDEX_PUBLICATION_GENERATION_KEY,
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((state, contract_fingerprint, generation)) = row else {
        return Ok(None);
    };
    let generation = generation.map_or(Ok(IndexGeneration::ZERO), |value| {
        value
            .parse::<u64>()
            .map(IndexGeneration::new)
            .map_err(|source| DbError::InvalidInteger {
                field: INDEX_PUBLICATION_GENERATION_KEY,
                value,
                source,
            })
    })?;
    Ok(Some(IndexPublication {
        state: IndexPublicationState::from_db(state)?,
        contract_fingerprint,
        generation,
    }))
}

/// Mark every indexed node absent before a complete scan replacement.
fn mark_all_scan_nodes_absent(connection: &Connection) -> DbResult<()> {
    connection.execute("UPDATE nodes SET exists_now = 0", [])?;
    Ok(())
}

/// Delete derived rows whose owning scan node remained absent.
fn delete_absent_scan_projections(connection: &Connection) -> DbResult<()> {
    content_classification::delete_absent_file_content_classifications(connection)?;
    let fts_revision = begin_file_text_fts_update(connection)?;
    connection.execute(
        "DELETE FROM symbol_relations WHERE path IN (SELECT path FROM nodes WHERE exists_now = 0)",
        [],
    )?;
    connection.execute(
        "DELETE FROM symbols WHERE path IN (SELECT path FROM nodes WHERE exists_now = 0)",
        [],
    )?;
    connection.execute(
        "DELETE FROM source_parse_metadata WHERE path IN (SELECT path FROM nodes WHERE exists_now = 0)",
        [],
    )?;
    connection.execute(
        "INSERT INTO file_text_fts(file_text_fts, rowid, content) \
         SELECT 'delete', rowid, content FROM file_texts \
         WHERE path IN (SELECT path FROM nodes WHERE exists_now = 0)",
        [],
    )?;
    connection.execute(
        "DELETE FROM file_texts WHERE path IN (SELECT path FROM nodes WHERE exists_now = 0)",
        [],
    )?;
    complete_file_text_fts_update(connection, fts_revision)?;
    Ok(())
}

/// Upsert scanned nodes through transaction-owned prepared statements.
fn upsert_nodes(connection: &Connection, nodes: &[Node]) -> DbResult<()> {
    content_classification::delete_non_file_content_classifications(connection, nodes)?;
    let mut select_existing = connection.prepare_cached(
        "
        SELECT content_hash
        FROM nodes
        WHERE path = ?1
        ",
    )?;
    let mut upsert_node = connection.prepare_cached(
        "
        INSERT INTO nodes(path, kind, parent_path, extension, language, size_bytes, mtime_ns, content_hash, exists_now)
        VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)
        ON CONFLICT(path) DO UPDATE SET
            kind = excluded.kind,
            parent_path = excluded.parent_path,
            extension = excluded.extension,
            language = excluded.language,
            size_bytes = excluded.size_bytes,
            mtime_ns = excluded.mtime_ns,
            content_hash = excluded.content_hash,
            exists_now = 1,
            last_seen_at = CURRENT_TIMESTAMP,
            last_indexed_at = CURRENT_TIMESTAMP
        ",
    )?;
    let mut ensure_file_classification = connection.prepare_cached(
        "INSERT INTO file_content_classifications(path, classification)
         SELECT ?1, 'opaque'
          WHERE ?2 = 'file'
         ON CONFLICT(path) DO NOTHING",
    )?;
    let mut select_node_id = connection.prepare_cached("SELECT id FROM nodes WHERE path = ?1")?;
    let mut ensure_purpose = connection.prepare_cached(
        "
        INSERT INTO purposes(node_id, purpose, source, status)
        VALUES(?1, NULL, 'missing', 'missing')
        ON CONFLICT(node_id) DO NOTHING
        ",
    )?;
    let mut upsert_summary = connection.prepare_cached(
        "
        INSERT INTO summaries(node_id, summary_level, subject, summary, updated_at)
        VALUES(?1, 'node', '', ?2, CURRENT_TIMESTAMP)
        ON CONFLICT(node_id, summary_level, subject) DO UPDATE SET
            summary = CASE WHEN ?3 THEN excluded.summary ELSE summaries.summary END,
            updated_at = CURRENT_TIMESTAMP
        ",
    )?;
    for node in nodes {
        let size_bytes = node
            .size_bytes
            .map(|value| {
                i64::try_from(value).map_err(|_source| DbError::GraphCountOverflow {
                    field: "nodes.size_bytes",
                    value,
                })
            })
            .transpose()?;
        let existing = select_existing
            .query_row([&node.path], |row| row.get::<_, Option<String>>(0))
            .optional()?;
        let content_changed = existing.as_ref().is_some_and(|old_hash| {
            node.kind == NodeKind::File
                && old_hash.is_some()
                && node.content_hash.is_some()
                && old_hash != &node.content_hash
        });
        upsert_node.execute(params![
            node.path,
            node.kind.to_string(),
            node.parent_path,
            node.extension,
            node.language,
            size_bytes,
            node.mtime_ns,
            node.content_hash
        ])?;
        ensure_file_classification.execute(params![node.path, node.kind.to_string()])?;
        let node_id = select_node_id.query_row([&node.path], |row| row.get::<_, i64>(0))?;
        ensure_purpose.execute([node_id])?;
        upsert_summary.execute(params![
            node_id,
            generate_node_summary(node),
            content_changed
        ])?;
    }
    Ok(())
}

/// Read one persisted indexed text row.
fn file_text_from_row(row: &rusqlite::Row<'_>) -> DbResult<IndexedFileText> {
    let byte_count = count_to_usize("file_texts.byte_count", row.get::<_, i64>(2)?)?;
    let line_count = count_to_usize("file_texts.line_count", row.get::<_, i64>(3)?)?;
    let text = IndexedFileText {
        path: row.get(0)?,
        content_hash: row.get(1)?,
        byte_count,
        line_count,
        content: row.get(4)?,
    };
    validate_indexed_file_text(&text)?;
    Ok(text)
}

/// Verify that persisted byte/line accounting matches authoritative content.
fn validate_indexed_file_text(text: &IndexedFileText) -> DbResult<()> {
    let actual_bytes = text.content.len();
    if text.byte_count != actual_bytes {
        return Err(DbError::FileTextMetadataMismatch {
            path: text.path.clone(),
            field: "byte_count",
            recorded: text.byte_count,
            actual: actual_bytes,
        });
    }
    let actual_lines = text.content.lines().count();
    if text.line_count != actual_lines {
        return Err(DbError::FileTextMetadataMismatch {
            path: text.path.clone(),
            field: "line_count",
            recorded: text.line_count,
            actual: actual_lines,
        });
    }
    Ok(())
}

/// Virtual-machine operations between bounded database-read progress checks.
const SQLITE_READ_PROGRESS_OPS: i32 = 1_000;

/// Deterministic observation of production `SQLite` progress boundaries for downstream tests.
#[cfg(feature = "sqlite-progress-test-observer")]
pub mod sqlite_progress_test_observer {
    use projectatlas_core::IndexWorkStage;
    use std::cell::RefCell;

    /// One observable event emitted by the production `SQLite` read-progress boundary.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum SqliteReadProgressEvent {
        /// A repository relation-family query entered after its live-control precheck.
        RepositoryRelationFamilyQueryEntered,
        /// A repository relation-family query returned from `SQLite`.
        RepositoryRelationFamilyQueryExited,
        /// `SQLite` invoked the production progress callback.
        CallbackEntered {
            /// Typed work stage owned by the guarded query.
            stage: IndexWorkStage,
        },
        /// The production progress callback evaluated its control.
        CallbackEvaluated {
            /// Typed work stage owned by the guarded query.
            stage: IndexWorkStage,
            /// Whether the callback requested `SQLite` interruption.
            interrupted: bool,
        },
    }

    /// Thread-local callback used only while a downstream test owns observation.
    type Observer = Box<dyn FnMut(SqliteReadProgressEvent)>;

    thread_local! {
        /// Observer scoped to the synchronous test thread running the SQLite connection.
        static OBSERVER: RefCell<Option<Observer>> = RefCell::new(None);
    }

    /// Restores a prior nested observer on every return or unwind path.
    struct ObserverGuard {
        /// Observer replaced for the current scope.
        previous: Option<Observer>,
    }

    impl Drop for ObserverGuard {
        fn drop(&mut self) {
            OBSERVER.with(|slot| {
                drop(slot.replace(self.previous.take()));
            });
        }
    }

    /// Run one operation while observing its synchronous `SQLite` progress events.
    pub fn observe_sqlite_read_progress<T>(
        observer: impl FnMut(SqliteReadProgressEvent) + 'static,
        operation: impl FnOnce() -> T,
    ) -> T {
        let previous = OBSERVER.with(|slot| slot.replace(Some(Box::new(observer))));
        let _guard = ObserverGuard { previous };
        operation()
    }

    /// Notify the current thread-local observer when one is installed.
    pub(crate) fn notify(event: SqliteReadProgressEvent) {
        OBSERVER.with(|slot| {
            if let Some(observer) = slot.borrow_mut().as_mut() {
                observer(event);
            }
        });
    }
}

/// Clears a connection-local `SQLite` progress handler on every exit path.
pub(crate) struct SqliteReadProgressGuard<'connection> {
    /// Connection whose temporary progress callback is armed.
    connection: &'connection Connection,
    /// Whether this guard installed a callback that must be removed.
    armed: bool,
}

impl<'connection> SqliteReadProgressGuard<'connection> {
    /// Install one cooperative progress callback for the owning work stage.
    pub(crate) fn new(
        connection: &'connection Connection,
        control: Option<&IndexWorkControl>,
        stage: IndexWorkStage,
    ) -> DbResult<Self> {
        let armed = if let Some(control) = control {
            control.check(stage)?;
            let progress_control = control.clone();
            connection.progress_handler(
                SQLITE_READ_PROGRESS_OPS,
                Some(move || {
                    #[cfg(feature = "sqlite-progress-test-observer")]
                    sqlite_progress_test_observer::notify(
                        sqlite_progress_test_observer::SqliteReadProgressEvent::CallbackEntered {
                            stage,
                        },
                    );
                    let interrupted = progress_control.check(stage).is_err();
                    #[cfg(feature = "sqlite-progress-test-observer")]
                    sqlite_progress_test_observer::notify(
                        sqlite_progress_test_observer::SqliteReadProgressEvent::CallbackEvaluated {
                            stage,
                            interrupted,
                        },
                    );
                    interrupted
                }),
            )?;
            true
        } else {
            false
        };
        Ok(Self { connection, armed })
    }
}

impl Drop for SqliteReadProgressGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            drop(self.connection.progress_handler(0, None::<fn() -> bool>));
        }
    }
}

/// Run one read with a temporary cooperative progress handler.
pub(crate) fn with_sqlite_read_progress<T>(
    connection: &Connection,
    control: Option<&IndexWorkControl>,
    stage: IndexWorkStage,
    operation: impl FnOnce() -> DbResult<T>,
) -> DbResult<T> {
    let guard = SqliteReadProgressGuard::new(connection, control, stage)?;
    let result = operation();
    drop(guard);
    if result.as_ref().is_err_and(|error| {
        matches!(
            error,
            DbError::Sqlite(sqlite)
                if sqlite.sqlite_error_code() == Some(ErrorCode::OperationInterrupted)
        )
    }) && let Some(control) = control
    {
        control.check(stage)?;
    }
    result
}

/// Run one text-index read through the shared bounded database-read guard.
fn with_file_text_progress<T>(
    connection: &Connection,
    control: Option<&IndexWorkControl>,
    operation: impl FnOnce() -> DbResult<T>,
) -> DbResult<T> {
    with_sqlite_read_progress(connection, control, IndexWorkStage::TextIndex, operation)
}

/// Enforce the single safe-token contract used by FTS candidate lookup.
fn validate_file_text_fts_token(token: &str) -> DbResult<()> {
    if token.len() < 3 {
        return Err(DbError::FileTextFtsTokenUnsafe {
            reason: "token is shorter than three ASCII bytes",
        });
    }
    if !token.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(DbError::FileTextFtsTokenUnsafe {
            reason: "token is not exclusively ASCII alphanumeric",
        });
    }
    Ok(())
}

/// Normalize only the root/trailing-separator cases of an already-relative scope.
fn normalized_file_text_path_prefix(path_prefix: Option<&str>) -> Option<&str> {
    path_prefix
        .map(|prefix| prefix.trim_end_matches('/'))
        .filter(|prefix| !prefix.is_empty() && *prefix != ".")
}

/// Build one binary-collation range containing exactly a path's descendants.
fn file_text_descendant_range(path_prefix: &str) -> (String, String) {
    (format!("{path_prefix}/"), format!("{path_prefix}0"))
}

/// Decode one bounded page of FTS metadata candidates without source text.
fn collect_file_text_fts_candidates(
    rows: &mut rusqlite::Rows<'_>,
    control: Option<&IndexWorkControl>,
    candidates: &mut Vec<FileTextFtsCandidate>,
) -> DbResult<()> {
    while let Some(row) = rows.next()? {
        if let Some(control) = control {
            control.check(IndexWorkStage::TextIndex)?;
        }
        let path = row.get::<_, String>(0)?;
        let bm25 = row.get::<_, f64>(4)?;
        if !bm25.is_finite() {
            return Err(DbError::FileTextFtsScoreInvalid { path });
        }
        candidates.push(FileTextFtsCandidate {
            path,
            content_hash: row.get(1)?,
            byte_count: count_to_usize("file_texts.byte_count", row.get::<_, i64>(2)?)?,
            line_count: count_to_usize("file_texts.line_count", row.get::<_, i64>(3)?)?,
            bm25,
        });
    }
    Ok(())
}

/// Decode persisted file metadata before the potentially large source column.
fn file_text_metadata_from_row(row: &rusqlite::Row<'_>) -> DbResult<FileTextMetadata> {
    let path = row.get::<_, String>(0)?;
    let classification = row
        .get::<_, Option<String>>(4)?
        .ok_or_else(|| DbError::FileContentClassificationMissing { path: path.clone() })?;
    Ok(FileTextMetadata {
        path,
        content_hash: row.get(1)?,
        byte_count: count_to_usize("file_texts.byte_count", row.get::<_, i64>(2)?)?,
        line_count: count_to_usize("file_texts.line_count", row.get::<_, i64>(3)?)?,
        classification: ContentClassification::from_db(&classification).ok_or(
            DbError::InvalidEnum {
                field: "file_content_classifications.classification",
                value: classification,
            },
        )?,
    })
}

/// Visit fallback rows with metadata-first admission and cooperative stops.
fn visit_file_text_fallback_rows<A, V>(
    rows: &mut rusqlite::Rows<'_>,
    content_statement: &mut rusqlite::CachedStatement<'_>,
    control: Option<&IndexWorkControl>,
    admit: &mut A,
    visitor: &mut V,
) -> DbResult<()>
where
    A: FnMut(&FileTextMetadata) -> DbResult<FileTextAdmission>,
    V: FnMut(IndexedFileText) -> DbResult<bool>,
{
    while let Some(row) = rows.next()? {
        if let Some(control) = control {
            control.check(IndexWorkStage::TextIndex)?;
        }
        let metadata = file_text_metadata_from_row(row)?;
        match admit(&metadata)? {
            FileTextAdmission::Skip => continue,
            FileTextAdmission::Stop => return Ok(()),
            FileTextAdmission::Read => {}
        }
        let content =
            content_statement.query_row([&metadata.path], |content_row| content_row.get(0))?;
        let text = IndexedFileText {
            path: metadata.path,
            content_hash: metadata.content_hash,
            byte_count: metadata.byte_count,
            line_count: metadata.line_count,
            content,
        };
        validate_indexed_file_text(&text)?;
        if !visitor(text)? {
            return Ok(());
        }
    }
    Ok(())
}

/// Raw standard node columns retained until typed enum validation succeeds.
type IndexedNodeParts = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<u64>,
    Option<i64>,
    Option<String>,
    Option<String>,
    String,
    String,
    Option<String>,
);

/// Decode the standard node select column order without interpreting enums.
fn indexed_node_parts_from_sql_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<IndexedNodeParts> {
    let kind_value: String = row.get(1)?;
    let source_value: String = row.get(9)?;
    let status_value: String = row.get(10)?;
    Ok((
        row.get::<_, String>(0)?,
        kind_value,
        row.get::<_, Option<String>>(2)?,
        row.get::<_, Option<String>>(3)?,
        row.get::<_, Option<String>>(4)?,
        option_u64_from_sql(row, 5)?,
        row.get::<_, Option<i64>>(6)?,
        row.get::<_, Option<String>>(7)?,
        row.get::<_, Option<String>>(8)?,
        source_value,
        status_value,
        row.get::<_, Option<String>>(11)?,
    ))
}

/// Build an indexed node from the standard node select column order.
fn indexed_node_from_sql_row(row: &rusqlite::Row<'_>) -> DbResult<IndexedNode> {
    indexed_node_from_parts(indexed_node_parts_from_sql_row(row)?)
}

/// Count exact variable-width payload and fixed scalar slots for one node row.
fn indexed_node_parts_decoded_bytes(row: &IndexedNodeParts) -> DbResult<u64> {
    let lengths = [
        row.0.len(),
        row.1.len(),
        row.2.as_ref().map_or(0, String::len),
        row.3.as_ref().map_or(0, String::len),
        row.4.as_ref().map_or(0, String::len),
        row.7.as_ref().map_or(0, String::len),
        row.8.as_ref().map_or(0, String::len),
        row.9.len(),
        row.10.len(),
        row.11.as_ref().map_or(0, String::len),
    ];
    let mut bytes = 16_u64;
    for length in lengths {
        let length =
            u64::try_from(length).map_err(|_source| GraphContractError::InvalidLimits {
                reason: "purpose-owner decoded field length overflowed",
            })?;
        bytes = bytes
            .checked_add(length)
            .ok_or(GraphContractError::InvalidLimits {
                reason: "purpose-owner decoded row size overflowed",
            })?;
    }
    Ok(bytes)
}

/// Build an indexed node from database row parts.
fn indexed_node_from_parts(row: IndexedNodeParts) -> DbResult<IndexedNode> {
    let (
        path,
        kind_value,
        parent_path,
        extension,
        language,
        size_bytes,
        mtime_ns,
        content_hash,
        purpose,
        source_value,
        status_value,
        summary,
    ) = row;
    let kind = NodeKind::from_db(&kind_value).ok_or_else(|| DbError::InvalidEnum {
        field: "kind",
        value: kind_value,
    })?;
    let source = parse_source(&source_value)?;
    let status = PurposeStatus::from_db(&status_value).ok_or_else(|| DbError::InvalidEnum {
        field: "status",
        value: status_value,
    })?;
    Ok(IndexedNode {
        node: Node {
            path: path.clone(),
            kind,
            parent_path,
            extension,
            language,
            size_bytes,
            mtime_ns,
            content_hash,
        },
        purpose: Purpose {
            path,
            purpose,
            source,
            status,
        },
        summary,
    })
}

/// Split a user query into lowercase terms for SQL ranking.
fn normalize_query_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Normalize the complete query used by exact path and basename admission tiers.
fn normalize_exact_ranked_query(query: &str) -> String {
    query.trim().replace('\\', "/").to_lowercase()
}

/// Build the reviewed-purpose admission predicate for normalized query terms.
fn reviewed_purpose_match_expression(term_count: usize) -> String {
    if term_count == 0 {
        return "0".to_string();
    }
    let matches = std::iter::repeat_n(
        "lower(COALESCE(p.purpose, '')) LIKE ? ESCAPE '\\'",
        term_count,
    )
    .collect::<Vec<_>>()
    .join(" OR ");
    format!(
        "CASE WHEN p.status = 'approved' AND p.source IN ('agent', 'human') \
                    AND ({matches}) THEN 1 ELSE 0 END"
    )
}

/// Build the SQL score expression for ranked node lookup.
fn ranked_score_expression(term_count: usize) -> String {
    if term_count == 0 {
        return "1".to_string();
    }
    (0..term_count)
        .map(|_| {
            "(CASE WHEN lower(n.path) LIKE ? ESCAPE '\\' THEN 20 ELSE 0 END \
             + CASE WHEN p.status = 'approved' AND p.source IN ('agent', 'human') \
                          AND lower(COALESCE(p.purpose, '')) LIKE ? ESCAPE '\\' THEN 30 ELSE 0 END \
             + CASE WHEN NOT (p.status = 'approved' AND p.source IN ('agent', 'human')) \
                          AND lower(COALESCE(p.purpose, '')) LIKE ? ESCAPE '\\' THEN 2 ELSE 0 END \
             + CASE WHEN lower(COALESCE(s.summary, '')) LIKE ? ESCAPE '\\' THEN 10 ELSE 0 END \
             + CASE WHEN lower(COALESCE(symbol_summaries.summary, '')) LIKE ? ESCAPE '\\' THEN 25 ELSE 0 END)"
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

/// Convert a normalized term into a `SQLite` LIKE pattern.
fn sqlite_like_pattern(term: &str) -> String {
    format!("%{}%", sqlite_like_escape(term))
}

/// Build a `SQLite` LIKE descendant pattern for a repository path prefix.
fn sqlite_descendant_pattern(path: &str) -> String {
    format!("{}/%", sqlite_like_escape(path))
}

/// Escape user or path text for `SQLite` LIKE patterns with backslash escaping.
fn sqlite_like_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Replace the denormalized symbol-name search summary for one file node.
fn replace_symbol_search_summary(
    connection: &Connection,
    node_id: i64,
    summary: Option<&str>,
) -> DbResult<()> {
    if let Some(summary) = summary {
        connection
            .prepare_cached(
                "
                INSERT INTO summaries(node_id, summary_level, subject, summary, updated_at)
                VALUES(?1, 'search', 'symbols', ?2, CURRENT_TIMESTAMP)
                ON CONFLICT(node_id, summary_level, subject) DO UPDATE SET
                    summary = excluded.summary,
                    updated_at = CURRENT_TIMESTAMP
                ",
            )?
            .execute(params![node_id, summary])?;
    } else {
        connection
            .prepare_cached(
                "
                DELETE FROM summaries
                WHERE node_id = ?1
                  AND summary_level = 'search'
                  AND subject = 'symbols'
                ",
            )?
            .execute([node_id])?;
    }
    Ok(())
}

/// Build a bounded search-only summary from symbol names.
fn symbol_search_summary(graph: &SymbolGraph) -> Option<String> {
    let mut names = graph
        .symbols
        .iter()
        .filter(|symbol| !matches!(symbol.kind, SymbolKind::Import | SymbolKind::Unknown))
        .map(|symbol| symbol.name.trim())
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    if names.is_empty() {
        return None;
    }
    let summary = format!("symbols {}", names.join(" "));
    Some(truncate_summary_chars(
        &summary,
        MAX_SYMBOL_SEARCH_SUMMARY_CHARS,
    ))
}

/// Truncate a summary at a valid UTF-8 boundary.
fn truncate_summary_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

/// Parse a stored purpose source value into the domain enum.
fn parse_source(value: &str) -> DbResult<PurposeSource> {
    let source = match value {
        value if value == PurposeSource::Missing.as_str() => PurposeSource::Missing,
        value if value == PurposeSource::Imported.as_str() => PurposeSource::Imported,
        value if value == PurposeSource::Generated.as_str() => PurposeSource::Generated,
        // Older databases could contain `human`; ProjectAtlas now treats
        // explicit approval as agent-owned and serializes new writes as `agent`.
        value if value == PurposeSource::Agent.as_str() || value == LEGACY_HUMAN_PURPOSE_SOURCE => {
            PurposeSource::Agent
        }
        _ => {
            return Err(DbError::InvalidEnum {
                field: "source",
                value: value.to_string(),
            });
        }
    };
    Ok(source)
}

/// Normalize a bounded host-owned task label used only for curator work identity.
fn normalize_purpose_curation_task(task: &str) -> DbResult<String> {
    let normalized = task.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err(DbError::PurposeCurationTaskInvalid {
            reason: "task must not be blank",
        });
    }
    if normalized.len() > MAX_PURPOSE_CURATION_TASK_BYTES {
        return Err(DbError::PurposeCurationTaskInvalid {
            reason: "task exceeds the UTF-8 byte limit",
        });
    }
    if normalized.chars().any(char::is_control) {
        return Err(DbError::PurposeCurationTaskInvalid {
            reason: "task contains control characters",
        });
    }
    Ok(normalized)
}

/// Load one current purpose row inside the caller's read or write snapshot.
fn load_current_purpose_state(
    connection: &Connection,
    path: &str,
) -> DbResult<Option<(i64, Purpose)>> {
    let row = connection
        .prepare_cached(
            "
            SELECT n.id, p.purpose, p.source, p.status
            FROM nodes n
            JOIN purposes p ON p.node_id = n.id
            WHERE n.exists_now = 1 AND n.path = ?1
            ",
        )?
        .query_row([path], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .optional()?;
    row.map(|(node_id, purpose, source, status)| {
        let source = parse_source(&source)?;
        let status = PurposeStatus::from_db(&status).ok_or_else(|| DbError::InvalidEnum {
            field: "purpose_status",
            value: status,
        })?;
        Ok((
            node_id,
            Purpose {
                path: path.to_string(),
                purpose,
                source,
                status,
            },
        ))
    })
    .transpose()
}

/// Apply one queue item inside the caller's validated writer transaction.
fn apply_conditional_purpose(
    connection: &Connection,
    project: ProjectInstanceId,
    generation: IndexGeneration,
    request: &PurposeConditionalApplyRequest,
) -> DbResult<PurposeConditionalApplyResult> {
    let current = load_current_purpose_state(connection, &request.path)?;
    let Some((node_id, current_purpose)) = current else {
        return Ok(PurposeConditionalApplyResult {
            path: request.path.clone(),
            state: PurposeConditionalApplyState::PathUnavailable,
            current_purpose: None,
        });
    };
    if !matches!(
        current_purpose.status,
        PurposeStatus::Missing | PurposeStatus::Suggested
    ) {
        return Ok(PurposeConditionalApplyResult {
            path: request.path.clone(),
            state: PurposeConditionalApplyState::Accepted,
            current_purpose: Some(current_purpose),
        });
    }
    let current_work_key =
        purpose_curation_item_work_key(project, generation, &request.task, &request.path);
    let current_state_token = purpose_curation_state_token(&current_work_key, &current_purpose);
    if current_work_key != request.work_key || current_state_token != request.state_token {
        return Ok(PurposeConditionalApplyResult {
            path: request.path.clone(),
            state: PurposeConditionalApplyState::Stale,
            current_purpose: Some(current_purpose),
        });
    }
    let generation_sql =
        i64::try_from(generation.get()).map_err(|_source| DbError::GraphCountOverflow {
            field: "project_identity.active_generation",
            value: generation.get(),
        })?;
    let changed = connection
        .prepare_cached(
            "
            UPDATE purposes
            SET purpose = ?2,
                source = ?3,
                status = ?4,
                updated_at = CURRENT_TIMESTAMP
            WHERE node_id = ?1
              AND status = ?5
              AND source = ?6
              AND purpose IS ?7
              AND EXISTS (
                  SELECT 1
                  FROM nodes n
                  JOIN project_identity pi ON pi.singleton = 1
                  WHERE n.id = ?1
                    AND n.exists_now = 1
                    AND n.path = ?8
                    AND pi.project_instance_id = ?9
                    AND pi.active_generation = ?10
              )
            ",
        )?
        .execute(params![
            node_id,
            request.purpose,
            PurposeSource::Agent.as_str(),
            PurposeStatus::Approved.as_str(),
            current_purpose.status.as_str(),
            current_purpose.source.as_str(),
            current_purpose.purpose,
            request.path,
            &project.as_bytes()[..],
            generation_sql,
        ])?;
    if changed == 1 {
        Ok(PurposeConditionalApplyResult {
            path: request.path.clone(),
            state: PurposeConditionalApplyState::Applied,
            current_purpose: Some(Purpose {
                path: request.path.clone(),
                purpose: Some(request.purpose.clone()),
                source: PurposeSource::Agent,
                status: PurposeStatus::Approved,
            }),
        })
    } else {
        Ok(PurposeConditionalApplyResult {
            path: request.path.clone(),
            state: PurposeConditionalApplyState::Stale,
            current_purpose: Some(current_purpose),
        })
    }
}

/// Construct one candidate with deterministic work and stale-state identities.
fn purpose_curation_candidate(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    task: &str,
    node: IndexedNode,
) -> PurposeCurationCandidate {
    let work_key = purpose_curation_item_work_key(project, generation, task, &node.node.path);
    let state_token = purpose_curation_state_token(&work_key, &node.purpose);
    PurposeCurationCandidate {
        node,
        work_key,
        state_token,
    }
}

/// Derive one stable project/generation/task/path work identity.
fn purpose_curation_item_work_key(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    task: &str,
    path: &str,
) -> String {
    digest_fields(
        PURPOSE_CURATION_ITEM_KEY_DOMAIN,
        &[
            &project.as_bytes(),
            &generation.get().to_le_bytes(),
            task.as_bytes(),
            path.as_bytes(),
        ],
    )
}

/// Bind conditional apply to the exact unapproved purpose row selected by the queue.
fn purpose_curation_state_token(work_key: &str, purpose: &Purpose) -> String {
    let purpose_presence = [u8::from(purpose.purpose.is_some())];
    digest_fields(
        PURPOSE_CURATION_STATE_TOKEN_DOMAIN,
        &[
            work_key.as_bytes(),
            &purpose_presence,
            purpose.purpose.as_deref().unwrap_or_default().as_bytes(),
            purpose.source.as_str().as_bytes(),
            purpose.status.as_str().as_bytes(),
        ],
    )
}

/// Derive a deterministic identity for one complete returned candidate set.
fn purpose_curation_batch_work_key(
    project: ProjectInstanceId,
    generation: IndexGeneration,
    task: &str,
    items: &[PurposeCurationCandidate],
) -> String {
    let mut hasher = Hasher::new();
    digest_field(&mut hasher, PURPOSE_CURATION_BATCH_KEY_DOMAIN.as_bytes());
    digest_field(&mut hasher, &project.as_bytes());
    digest_field(&mut hasher, &generation.get().to_le_bytes());
    digest_field(&mut hasher, task.as_bytes());
    for item in items {
        digest_field(&mut hasher, item.work_key.as_bytes());
        digest_field(&mut hasher, item.state_token.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

/// Hash length-delimited fields under one stable domain separator.
fn digest_fields(domain: &str, fields: &[&[u8]]) -> String {
    let mut hasher = Hasher::new();
    digest_field(&mut hasher, domain.as_bytes());
    for field in fields {
        digest_field(&mut hasher, field);
    }
    hasher.finalize().to_hex().to_string()
}

/// Append one unambiguous field to a deterministic digest.
fn digest_field(hasher: &mut Hasher, value: &[u8]) {
    let length = u64::try_from(value.len()).unwrap_or(u64::MAX);
    hasher.update(&length.to_le_bytes());
    hasher.update(value);
}

/// Convert an aggregate database count into a platform `usize`.
fn count_to_usize(field: &'static str, value: i64) -> DbResult<usize> {
    usize::try_from(value).map_err(|source| DbError::InvalidCount {
        field,
        value,
        source,
    })
}

/// Convert a usize to i64 with saturation for database storage.
fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Convert a non-negative i64 to usize for database reads.
fn i64_to_usize(value: i64) -> usize {
    usize::try_from(value.max(0)).unwrap_or(usize::MAX)
}

/// Decode an optional nonnegative `SQLite` integer into the public unsigned size type.
fn option_u64_from_sql(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(index)?
        .map(|value| {
            u64::try_from(value).map_err(|source| {
                rusqlite::Error::FromSqlConversionFailure(
                    index,
                    rusqlite::types::Type::Integer,
                    Box::new(source),
                )
            })
        })
        .transpose()
}

/// Convert an optional token count into the exact `SQLite` integer range.
#[cfg(test)]
fn option_usize_to_i64(field: &'static str, value: Option<usize>) -> DbResult<Option<i64>> {
    value
        .map(|value| {
            i64::try_from(value).map_err(|_source| DbError::TelemetryIntegerOverflow { field })
        })
        .transpose()
}

/// Build the classified symbol query from static clauses and bound caller values.
fn classified_symbols_sql(
    file: Option<&str>,
    query: Option<&str>,
    selection: ContentSelection,
    limit: usize,
) -> (String, Vec<Value>) {
    let mut predicates = Vec::new();
    let mut bindings = Vec::new();
    if let Some(file) = file {
        bindings.push(Value::Text(file.to_string()));
        predicates.push(format!("symbol.path = ?{}", bindings.len()));
    }
    if let Some(query) = query {
        bindings.push(Value::Text(like_query(query)));
        let placeholder = bindings.len();
        let path_predicate = if file.is_some() {
            String::new()
        } else {
            format!(" OR symbol.path LIKE ?{placeholder}")
        };
        predicates.push(format!(
            "(symbol.name LIKE ?{placeholder} OR symbol.signature LIKE ?{placeholder} \
             OR symbol.documentation LIKE ?{placeholder}{path_predicate})"
        ));
    }
    match selection {
        ContentSelection::UnspecifiedLegacy => {}
        ContentSelection::Source => {
            bindings.push(Value::Text(
                ContentClassification::Source.as_str().to_string(),
            ));
            predicates.push(format!(
                "(classification.classification = ?{} OR classification.path IS NULL)",
                bindings.len()
            ));
        }
        ContentSelection::Documentation => {
            bindings.push(Value::Text(
                ContentClassification::Documentation.as_str().to_string(),
            ));
            predicates.push(format!(
                "(classification.classification = ?{} OR classification.path IS NULL)",
                bindings.len()
            ));
        }
        ContentSelection::Both => {
            bindings.push(Value::Text(
                ContentClassification::Source.as_str().to_string(),
            ));
            let source = bindings.len();
            bindings.push(Value::Text(
                ContentClassification::Documentation.as_str().to_string(),
            ));
            let documentation = bindings.len();
            predicates.push(format!(
                "(classification.classification IN (?{source}, ?{documentation}) \
                 OR classification.path IS NULL)"
            ));
        }
    }
    bindings.push(Value::Integer(usize_to_i64(limit.max(1))));
    let limit = bindings.len();
    let where_clause = if predicates.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", predicates.join(" AND "))
    };
    let sql = format!(
        "SELECT symbol.path, symbol.language, symbol.name, symbol.kind,
                symbol.signature, symbol.line_start, symbol.line_end, symbol.parent,
                symbol.parser, symbol.detail, symbol.exported, symbol.documentation,
                symbol.source_byte_start, symbol.source_byte_end,
                symbol.source_column_start, symbol.source_column_end,
                classification.classification
           FROM symbols AS symbol INDEXED BY idx_symbols_path
           LEFT JOIN file_content_classifications AS classification
             ON classification.path = symbol.path
           {where_clause}
          ORDER BY symbol.path, symbol.line_start, symbol.name
          LIMIT ?{limit}"
    );
    (sql, bindings)
}

/// Validate and narrow one parser-supplied source selector for `SQLite` storage.
fn symbol_source_selector_values(symbol: &CodeSymbol) -> DbResult<[Option<i64>; 4]> {
    let Some(selector) = symbol.source_selector else {
        return Ok([None; 4]);
    };
    if symbol.line_start == 0
        || symbol.line_end < symbol.line_start
        || selector.byte_end < selector.byte_start
        || (symbol.line_start == symbol.line_end && selector.column_end < selector.column_start)
    {
        return Err(DbError::SymbolGraphRowShape {
            path: symbol.path.clone(),
            reason: "symbol source selector range is invalid",
        });
    }
    let narrow = |value| match i64::try_from(value) {
        Ok(value) => Ok(Some(value)),
        Err(_) => Err(DbError::SymbolGraphRowShape {
            path: symbol.path.clone(),
            reason: "symbol source selector exceeds the SQLite integer range",
        }),
    };
    Ok([
        narrow(selector.byte_start)?,
        narrow(selector.byte_end)?,
        narrow(selector.column_start)?,
        narrow(selector.column_end)?,
    ])
}

/// Decode one all-or-none persisted source selector without coercing corrupt ranges.
fn symbol_source_selector_from_row(
    row: &rusqlite::Row<'_>,
    line_start: usize,
    line_end: usize,
) -> rusqlite::Result<Option<SymbolSourceSelector>> {
    let raw = [
        row.get::<_, Option<i64>>(12)?,
        row.get::<_, Option<i64>>(13)?,
        row.get::<_, Option<i64>>(14)?,
        row.get::<_, Option<i64>>(15)?,
    ];
    let [
        Some(byte_start),
        Some(byte_end),
        Some(column_start),
        Some(column_end),
    ] = raw
    else {
        if raw.iter().all(Option::is_none) {
            return Ok(None);
        }
        return Err(invalid_symbol_source_selector(
            "symbol source selector columns are only partially populated",
        ));
    };
    if byte_start < 0
        || byte_end < byte_start
        || column_start < 0
        || column_end < 0
        || line_start == 0
        || line_end < line_start
        || (line_start == line_end && column_end < column_start)
    {
        return Err(invalid_symbol_source_selector(
            "symbol source selector range is invalid",
        ));
    }
    Ok(Some(SymbolSourceSelector {
        byte_start: symbol_source_selector_usize(byte_start)?,
        byte_end: symbol_source_selector_usize(byte_end)?,
        column_start: symbol_source_selector_usize(column_start)?,
        column_end: symbol_source_selector_usize(column_end)?,
    }))
}

/// Convert one validated nonnegative selector integer without losing overflow detail.
fn symbol_source_selector_usize(value: i64) -> rusqlite::Result<usize> {
    usize::try_from(value).map_err(|source| {
        rusqlite::Error::FromSqlConversionFailure(
            12,
            rusqlite::types::Type::Integer,
            Box::new(source),
        )
    })
}

/// Construct one fail-closed `SQLite` conversion error for a malformed selector row.
fn invalid_symbol_source_selector(message: &'static str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        12,
        rusqlite::types::Type::Integer,
        Box::new(std::io::Error::other(message)),
    )
}

/// Decode one persisted symbol row through the shared column contract.
fn code_symbol_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CodeSymbol> {
    let line_start = i64_to_usize(row.get::<_, i64>(5)?);
    let line_end = i64_to_usize(row.get::<_, i64>(6)?);
    Ok(CodeSymbol {
        path: row.get(0)?,
        language: row.get(1)?,
        name: row.get(2)?,
        kind: SymbolKind::from_db(row.get_ref(3)?.as_str()?),
        signature: row.get(4)?,
        line_start,
        line_end,
        source_selector: symbol_source_selector_from_row(row, line_start, line_end)?,
        parent: row.get(7)?,
        parser: ParserKind::from_db(row.get_ref(8)?.as_str()?),
        detail: row.get(9)?,
        exported: row.get::<_, i64>(10)? != 0,
        documentation: row.get(11)?,
    })
}

/// Decode one joined symbol/classification row through both owning contracts.
fn classified_symbol_from_row(row: &rusqlite::Row<'_>) -> DbResult<ClassifiedSymbol> {
    let symbol = code_symbol_from_row(row)?;
    let classification = row.get::<_, Option<String>>(16)?.ok_or_else(|| {
        DbError::FileContentClassificationMissing {
            path: symbol.path.clone(),
        }
    })?;
    Ok(ClassifiedSymbol {
        symbol,
        classification: content_classification::parse_classification(classification)?,
    })
}

/// Read the exact owned-string byte floor before hydrating one symbol row.
fn code_symbol_preflight_bytes(row: &rusqlite::Row<'_>) -> DbResult<u64> {
    let string_bytes = row.get::<_, i64>(16)?;
    let string_bytes = u64::try_from(string_bytes).map_err(|source| DbError::InvalidCount {
        field: "symbol persisted field bytes",
        value: string_bytes,
        source,
    })?;
    let row_bytes = u64::try_from(std::mem::size_of::<CodeSymbol>()).map_err(|source| {
        DbError::InvalidCount {
            field: "symbol persisted field bytes",
            value: i64::MAX,
            source,
        }
    })?;
    row_bytes.checked_add(string_bytes).ok_or_else(|| {
        GraphContractError::InvalidLimits {
            reason: "symbol persisted row bytes overflowed",
        }
        .into()
    })
}

/// Decode the covering persisted import projection.
fn stored_import_relation_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<StoredImportRelation> {
    Ok(StoredImportRelation {
        path: row.get(0)?,
        source_name: row.get(1)?,
        target_name: row.get(2)?,
        line: i64_to_usize(row.get::<_, i64>(3)?),
    })
}

/// Count the retained Rust row plus its owned string allocation capacities.
fn code_symbol_decoded_bytes(symbol: &CodeSymbol) -> DbResult<u64> {
    let capacities = [
        symbol.path.capacity(),
        symbol.language.as_ref().map_or(0, String::capacity),
        symbol.name.capacity(),
        symbol.signature.capacity(),
        symbol.documentation.as_ref().map_or(0, String::capacity),
        symbol.parent.as_ref().map_or(0, String::capacity),
        symbol.detail.as_ref().map_or(0, String::capacity),
    ];
    let mut bytes = u64::try_from(std::mem::size_of::<CodeSymbol>()).map_err(|source| {
        DbError::InvalidCount {
            field: "symbol decoded field bytes",
            value: i64::MAX,
            source,
        }
    })?;
    for capacity in capacities {
        let capacity = u64::try_from(capacity).map_err(|source| DbError::InvalidCount {
            field: "symbol decoded field bytes",
            value: i64::MAX,
            source,
        })?;
        bytes = bytes
            .checked_add(capacity)
            .ok_or(GraphContractError::InvalidLimits {
                reason: "symbol decoded row bytes overflowed",
            })?;
    }
    Ok(bytes)
}

/// Wrap a query string for a SQL LIKE expression.
fn like_query(query: &str) -> String {
    format!("%{query}%")
}

/// Build the single set-oriented query used to hydrate exact purpose-work paths.
fn load_nodes_by_paths_sql(path_count: usize) -> String {
    let placeholders = numbered_placeholders(1, path_count);
    format!(
        "
        SELECT
            n.path,
            n.kind,
            n.parent_path,
            n.extension,
            n.language,
            n.size_bytes,
            n.mtime_ns,
            n.content_hash,
            p.purpose,
            p.source,
            p.status,
            s.summary
        FROM nodes n
        JOIN purposes p ON p.node_id = n.id
        LEFT JOIN summaries s ON s.node_id = n.id
            AND s.summary_level = 'node'
            AND s.subject = ''
        WHERE n.exists_now = 1 AND n.path IN ({placeholders})
        ORDER BY n.path
        "
    )
}

/// Build numbered SQL placeholders starting at a caller-selected index.
fn numbered_placeholders(start: usize, count: usize) -> String {
    (start..start + count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Generate the durable node-level content summary.
fn generate_node_summary(node: &Node) -> String {
    match node.kind {
        NodeKind::Folder => format!("Folder for {}", path_label(&node.path)),
        NodeKind::File => file_summary(node),
    }
}

/// Generate a one-line observed file summary from scan metadata.
fn file_summary(node: &Node) -> String {
    let language = node
        .language
        .as_deref()
        .or(node.extension.as_deref())
        .unwrap_or("unknown");
    let size = node.size_bytes.map_or_else(
        || "unknown size".to_string(),
        |bytes| format!("{bytes} bytes"),
    );
    format!("{language} file, {size}")
}

/// Return a readable label for a repository-relative path.
fn path_label(path: &str) -> String {
    if path == "." {
        return "repository root".to_string();
    }
    path.rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(path)
        .replace(['-', '_'], " ")
}

/// Emit a health finding when it has not already been resolved.
fn emit_unresolved_finding<F>(
    finding: HealthFinding,
    resolved_ids: &HashSet<String>,
    visitor: &mut F,
) -> DbResult<bool>
where
    F: FnMut(HealthFinding) -> DbResult<bool>,
{
    if resolved_ids.contains(&finding.id) {
        return Ok(true);
    }
    visitor(finding)
}

/// Return whether a purpose-status source can match a bounded health query.
fn purpose_health_spec_matches_query(spec: PurposeHealthSpec, query: &HealthQuery) -> bool {
    health_category_matches_query(spec.category, Severity::Warning, query)
}

/// Return whether a health category/severity can match a bounded query.
fn health_category_matches_query(category: &str, severity: Severity, query: &HealthQuery) -> bool {
    query
        .category
        .as_deref()
        .is_none_or(|requested| category.eq_ignore_ascii_case(requested))
        && query.severity.is_none_or(|requested| severity == requested)
}

/// Return purpose health metadata for a stored purpose status.
fn purpose_health_spec_for_status(status: &str) -> DbResult<PurposeHealthSpec> {
    PURPOSE_HEALTH_SPECS
        .iter()
        .copied()
        .find(|spec| spec.status == status)
        .ok_or_else(|| DbError::InvalidEnum {
            field: "status",
            value: status.to_string(),
        })
}

/// Build the health finding for an approved purpose that still needs agent review.
fn agent_review_required_finding(path: String) -> HealthFinding {
    HealthFinding {
        id: finding_id(CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED, &path, None),
        severity: Severity::Warning,
        category: CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED.to_string(),
        path,
        related_path: None,
        message: MESSAGE_PURPOSE_AGENT_REVIEW_REQUIRED.to_string(),
        recommendation: RECOMMENDATION_PURPOSE_AGENT_REVIEW_REQUIRED.to_string(),
    }
}

/// Build the shared SQL filter for globally ordered purpose lifecycle findings.
fn purpose_lifecycle_where_clause(
    specs: &[PurposeHealthSpec],
    path_prefix: Option<&str>,
    resolution_filter: HealthResolutionFilter<'_>,
    scope: HealthScope,
) -> (String, Vec<Value>) {
    let statuses = specs
        .iter()
        .map(|spec| format!("'{}'", spec.status))
        .collect::<Vec<_>>()
        .join(", ");
    let mut clauses = vec![
        "n.exists_now = 1".to_string(),
        format!("p.status IN ({statuses})"),
    ];
    let mut values = Vec::new();

    if source_filter_applies_before_queue(scope) {
        clauses.push(source_relevant_node_expression("n"));
    }
    if scope.high_impact_queue() {
        clauses.push(purpose_default_queue_node_expression("n", "p", scope));
    }

    let normalized_prefix = path_prefix
        .map(normalize_repo_path_prefix)
        .filter(|prefix| prefix != ".");
    if let Some(prefix) = normalized_prefix {
        clauses.push(format!(
            "(n.path = ?{} OR n.path LIKE ?{} ESCAPE '\\')",
            values.len() + 1,
            values.len() + 2
        ));
        values.push(Value::from(prefix.clone()));
        values.push(Value::from(sqlite_descendant_pattern(&prefix)));
    }

    match resolution_filter {
        HealthResolutionFilter::Explicit(resolved_ids) => {
            for spec in specs {
                let resolved_paths = resolved_purpose_paths(resolved_ids, spec.category);
                if !resolved_paths.is_empty() {
                    clauses.push(format!(
                        "NOT (p.status = '{}' AND n.path IN ({}))",
                        spec.status,
                        numbered_placeholders(values.len() + 1, resolved_paths.len())
                    ));
                    values.extend(resolved_paths.into_iter().map(Value::from));
                }
            }
        }
        HealthResolutionFilter::Stored => clauses.push(stored_resolution_filter_clause(
            &purpose_lifecycle_finding_id_expression(specs, "n", "p"),
        )),
    }

    (clauses.join(" AND "), values)
}

/// Build the shared SQL filter for purpose lifecycle health findings.
fn purpose_status_where_clause(
    spec: PurposeHealthSpec,
    path_prefix: Option<&str>,
    resolution_filter: HealthResolutionFilter<'_>,
    scope: HealthScope,
) -> (String, Vec<Value>) {
    let mut clauses = vec!["n.exists_now = 1".to_string(), "p.status = ?1".to_string()];
    let mut values = vec![Value::from(spec.status.to_string())];

    if source_filter_applies_before_queue(scope) {
        clauses.push(source_relevant_node_expression("n"));
    }
    if scope.high_impact_queue() {
        clauses.push(purpose_default_queue_node_expression("n", "p", scope));
    }

    let normalized_prefix = path_prefix
        .map(normalize_repo_path_prefix)
        .filter(|prefix| prefix != ".");
    if let Some(prefix) = normalized_prefix {
        clauses.push(format!(
            "(n.path = ?{} OR n.path LIKE ?{} ESCAPE '\\')",
            values.len() + 1,
            values.len() + 2
        ));
        values.push(Value::from(prefix.clone()));
        values.push(Value::from(sqlite_descendant_pattern(&prefix)));
    }

    match resolution_filter {
        HealthResolutionFilter::Explicit(resolved_ids) => {
            let resolved_paths = resolved_purpose_paths(resolved_ids, spec.category);
            if !resolved_paths.is_empty() {
                clauses.push(format!(
                    "n.path NOT IN ({})",
                    numbered_placeholders(values.len() + 1, resolved_paths.len())
                ));
                values.extend(resolved_paths.into_iter().map(Value::from));
            }
        }
        HealthResolutionFilter::Stored => clauses.push(stored_resolution_filter_clause(&format!(
            "'{}:' || n.path || ':'",
            spec.category
        ))),
    }

    (clauses.join(" AND "), values)
}

/// Build a structural-health SQL filter over `findings` CTE columns.
fn structural_finding_where_clause(
    category: &str,
    path_prefix: Option<&str>,
    resolution_filter: HealthResolutionFilter<'_>,
    scope: HealthScope,
    first_placeholder: usize,
) -> (String, Vec<Value>) {
    let mut placeholder = first_placeholder;
    let mut clauses = Vec::new();
    let mut values = Vec::new();

    if source_filter_applies_before_queue(scope) {
        clauses.push("source_relevant = 1".to_string());
    }
    if scope.high_impact_queue() {
        clauses.push(purpose_default_queue_finding_expression(scope));
    }

    let normalized_prefix = path_prefix
        .map(normalize_repo_path_prefix)
        .filter(|prefix| prefix != ".");
    if let Some(prefix) = normalized_prefix {
        clauses.push(format!(
            "((path = ?{path_exact} OR path LIKE ?{path_descendant} ESCAPE '\\') \
              OR (related_path = ?{related_exact} OR related_path LIKE ?{related_descendant} ESCAPE '\\'))",
            path_exact = placeholder,
            path_descendant = placeholder + 1,
            related_exact = placeholder + 2,
            related_descendant = placeholder + 3
        ));
        values.push(Value::from(prefix.clone()));
        values.push(Value::from(sqlite_descendant_pattern(&prefix)));
        values.push(Value::from(prefix.clone()));
        values.push(Value::from(sqlite_descendant_pattern(&prefix)));
        placeholder += 4;
    }

    match resolution_filter {
        HealthResolutionFilter::Explicit(resolved_ids) => {
            let resolved_ids = resolved_ids_for_category(resolved_ids, category);
            if !resolved_ids.is_empty() {
                clauses.push(format!(
                    "('{category}:' || path || ':' || related_path) NOT IN ({})",
                    numbered_placeholders(placeholder, resolved_ids.len())
                ));
                values.extend(resolved_ids.into_iter().map(Value::from));
            }
        }
        HealthResolutionFilter::Stored => clauses.push(stored_resolution_filter_clause(&format!(
            "'{category}:' || path || ':' || related_path"
        ))),
    }

    if clauses.is_empty() {
        (String::new(), values)
    } else {
        (format!("WHERE {}", clauses.join(" AND ")), values)
    }
}

/// Build the exact stored finding-id expression for mixed purpose lifecycle rows.
fn purpose_lifecycle_finding_id_expression(
    specs: &[PurposeHealthSpec],
    node_alias: &str,
    purpose_alias: &str,
) -> String {
    let category_cases = specs
        .iter()
        .map(|spec| format!("WHEN '{}' THEN '{}:'", spec.status, spec.category))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "(CASE {purpose_alias}.status {category_cases} ELSE '' END || {node_alias}.path || ':')"
    )
}

/// Build an indexed anti-lookup against durable health resolutions.
fn stored_resolution_filter_clause(finding_id_expression: &str) -> String {
    format!(
        "NOT EXISTS (SELECT 1 FROM health_resolutions hr WHERE hr.finding_id = {finding_id_expression})"
    )
}

/// SQL expression for approved purposes that need agent review at the requested scope.
fn purpose_review_candidate_expression(node_alias: &str, scope: HealthScope) -> String {
    let scope = match scope {
        HealthScope::All => HealthScope::PurposeStrict,
        other => other,
    };
    purpose_default_queue_node_expression(node_alias, "p", scope)
}

/// SQL expression for paths that belong in the default purpose queue.
fn purpose_default_queue_node_expression(
    node_alias: &str,
    purpose_alias: &str,
    scope: HealthScope,
) -> String {
    let asset_clause = if scope.include_assets() {
        format!(
            " OR ({node_alias}.kind = 'file' AND NOT ({}))",
            source_relevant_node_expression(node_alias)
        )
    } else {
        String::new()
    };
    let source_file_clause = if scope.include_source_files() {
        format!(" OR ({node_alias}.kind = 'file' AND COALESCE({node_alias}.language, '') <> '')")
    } else {
        String::new()
    };
    let all_file_clause = if scope.include_all_files() {
        format!(" OR {node_alias}.kind = 'file'")
    } else {
        String::new()
    };
    let stale_queue_sources = sql_string_literals(STALE_FILE_PURPOSE_QUEUE_SOURCE_VALUES);
    format!(
        "({node_alias}.kind = 'folder' \
          OR ({node_alias}.kind = 'file' \
              AND {purpose_alias}.status = 'stale' \
              AND {purpose_alias}.source IN ({stale_queue_sources})) \
          OR ({node_alias}.kind = 'file' AND {}){source_file_clause}{all_file_clause}{asset_clause})",
        high_impact_file_path_expression(&format!("lower({node_alias}.path)")),
    )
}

/// SQL expression for finding CTE columns that belong in the default purpose queue.
fn purpose_default_queue_finding_expression(scope: HealthScope) -> String {
    let asset_clause = if scope.include_assets() {
        " OR (kind = 'file' AND COALESCE(language, '') = '')"
    } else {
        ""
    };
    let source_file_clause = if scope.include_source_files() {
        " OR (kind = 'file' AND COALESCE(language, '') <> '')"
    } else {
        ""
    };
    let all_file_clause = if scope.include_all_files() {
        " OR kind = 'file'"
    } else {
        ""
    };
    format!(
        "(kind = 'folder' OR (kind = 'file' AND {}){source_file_clause}{all_file_clause}{asset_clause})",
        high_impact_file_path_expression("lower(path)")
    )
}

/// SQL ORDER BY expression that keeps folder-purpose work ahead of file cleanup.
fn purpose_default_queue_order_expression(node_alias: &str, purpose_alias: &str) -> String {
    let stale_queue_sources = sql_string_literals(STALE_FILE_PURPOSE_QUEUE_SOURCE_VALUES);
    format!(
        "CASE \
            WHEN {node_alias}.kind = 'folder' THEN 0 \
            WHEN {node_alias}.kind = 'file' \
                AND {purpose_alias}.status = 'stale' \
                AND ({purpose_alias}.source IN ({stale_queue_sources}) OR {}) THEN 1 \
            WHEN {node_alias}.kind = 'file' AND {} THEN 2 \
            ELSE 3 \
        END, {node_alias}.path",
        high_impact_file_path_expression(&format!("lower({node_alias}.path)")),
        high_impact_file_path_expression(&format!("lower({node_alias}.path)"))
    )
}

/// Purpose sources whose stale file purposes stay in the default queue regardless of path impact.
const STALE_FILE_PURPOSE_QUEUE_SOURCE_VALUES: &[&str] = &["human", "imported"];

/// Return whether `source_only` should run before queue-specific folder/file selection.
fn source_filter_applies_before_queue(scope: HealthScope) -> bool {
    scope.source_only_filter() && !scope.high_impact_queue()
}

/// Render trusted static strings as SQL string literals.
fn sql_string_literals(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| format!("'{}'", value.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// SQL expression mirroring the path-based high-impact file heuristic.
fn high_impact_file_path_expression(lower_path: &str) -> String {
    let name_matches = HIGH_IMPACT_FILE_NAMES
        .iter()
        .map(|name| format!("{lower_path} = '{name}' OR {lower_path} LIKE '%/{name}'"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let prefix_matches = HIGH_IMPACT_PATH_PREFIXES
        .iter()
        .map(|prefix| format!("{lower_path} LIKE '{prefix}%'"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let segment_matches = HIGH_IMPACT_PATH_SEGMENTS
        .iter()
        .map(|segment| format!("{lower_path} LIKE '%{segment}%'"))
        .collect::<Vec<_>>()
        .join(" OR ");
    format!("({name_matches} OR {prefix_matches} OR {segment_matches})")
}

/// Return a SQL expression that treats source files and folders with source descendants as source-relevant.
fn source_relevant_node_expression(alias: &str) -> String {
    format!(
        "(({alias}.kind = 'file' AND COALESCE({alias}.language, '') <> '') \
          OR ({alias}.kind = 'folder' AND EXISTS (\
              SELECT 1 FROM nodes source_child \
              WHERE source_child.exists_now = 1 \
                AND source_child.kind = 'file' \
                AND COALESCE(source_child.language, '') <> '' \
                AND (\
                    {alias}.path = '.' \
                    OR source_child.parent_path = {alias}.path \
                    OR substr(source_child.parent_path, 1, length({alias}.path) + 1) = {alias}.path || '/'\
                )\
          )))"
    )
}

/// Extract resolved primary paths for lifecycle categories without related paths.
fn resolved_purpose_paths(resolved_ids: &[String], category: &str) -> Vec<String> {
    let prefix = format!("{category}:");
    resolved_ids
        .iter()
        .filter_map(|id| {
            id.strip_prefix(&prefix)
                .and_then(|rest| rest.strip_suffix(':'))
                .filter(|path| !path.is_empty())
                .map(ToOwned::to_owned)
        })
        .collect()
}

/// Extract resolved full ids for categories that include related paths.
fn resolved_ids_for_category(resolved_ids: &[String], category: &str) -> Vec<String> {
    let prefix = format!("{category}:");
    resolved_ids
        .iter()
        .filter(|id| id.starts_with(&prefix))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectatlas_core::telemetry::{
        READ_AVOIDANCE_CONFIDENCE_MODELED, READ_AVOIDANCE_CONFIDENCE_NOT_RECORDED,
        READ_AVOIDANCE_CONFIDENCE_OBSERVED, TOKEN_ACCOUNTING_MODELED_AVOIDANCE,
        TOKEN_ACCURACY_HEURISTIC, TOKEN_BASELINE_SELECTED_CANDIDATES,
        TOKEN_BUCKET_FULL_FILE_COMPRESSION, TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
        TOKEN_COMMAND_SEARCH, TOKEN_CONFIDENCE_INFERRED, TOKEN_DEDUPE_SCOPE_EVENT,
        usage_from_estimates, usage_from_estimates_with_accounting,
        usage_from_estimates_with_context, usage_from_text,
    };
    use projectatlas_core::{NodeKind, normalized_parent};
    use std::error::Error;
    use std::fmt::Debug;
    use std::fs;
    use std::io;
    use std::time::Instant;

    #[test]
    fn unsupported_schema_versions_follow_the_migration_inventory() -> Result<(), Box<dyn Error>> {
        for found in schema::PREVIOUS_SCHEMA_VERSION..=schema::SCHEMA_VERSION {
            let error = DbError::SchemaVersion {
                found,
                expected: schema::SCHEMA_VERSION,
            };
            let expected_migration = if found < schema::SCHEMA_VERSION {
                Some((
                    found,
                    schema::SCHEMA_VERSION,
                    u32::try_from(schema::SCHEMA_VERSION - found)?,
                ))
            } else {
                None
            };
            require_eq(
                &error.supported_schema_migration(),
                &expected_migration,
                "admitted schema migration",
            )?;
            require_eq(
                &error.unsupported_schema_version(),
                &None,
                "admitted schema version",
            )?;
        }
        for found in [
            schema::PREVIOUS_SCHEMA_VERSION - 1,
            schema::SCHEMA_VERSION + 1,
        ] {
            let error = DbError::SchemaVersion {
                found,
                expected: schema::SCHEMA_VERSION,
            };
            require_eq(
                &error.unsupported_schema_version(),
                &Some((found, schema::SCHEMA_VERSION)),
                "unsupported schema version",
            )?;
            require_eq(
                &error.supported_schema_migration(),
                &None,
                "unsupported schema migration",
            )?;
        }
        require_eq(
            &DbError::SchemaVersionMissing.unsupported_schema_version(),
            &None,
            "unrelated database error",
        )?;
        require_eq(
            &DbError::SchemaVersionMissing.supported_schema_migration(),
            &None,
            "unrelated migration error",
        )?;
        Ok(())
    }

    #[test]
    fn stores_nodes_and_overview() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let node = Node {
            path: "src/main.rs".to_string(),
            kind: NodeKind::File,
            parent_path: normalized_parent("src/main.rs"),
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(12),
            mtime_ns: Some(10),
            content_hash: Some("abc".to_string()),
        };
        store.replace_scan(&[node])?;
        let overview = store.overview()?;
        require_eq(&overview.files, &1, "file count")?;
        require_eq(&overview.missing_purposes, &1, "missing purpose count")?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.purpose,
            &None,
            "purpose remains separate from summary",
        )?;
        require_eq(
            &nodes[0].summary,
            &Some("rust file, 12 bytes".to_string()),
            "node-level summary",
        )?;
        let loaded = store
            .load_node_by_path("src/main.rs")?
            .ok_or_else(|| io::Error::other("indexed node was not found by path"))?;
        require_eq(
            &loaded.node.path,
            &"src/main.rs".to_string(),
            "targeted path lookup",
        )?;
        require_eq(
            &store.load_node_by_path("src/missing.rs")?.is_none(),
            &true,
            "missing targeted path lookup",
        )?;
        Ok(())
    }

    #[test]
    fn load_nodes_rejects_negative_size_bytes_without_partial_page() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.connection.execute(
            "UPDATE nodes SET size_bytes = -1 WHERE path = 'src/b.rs'",
            [],
        )?;

        let Err(error) = store.load_nodes() else {
            return Err(io::Error::other("negative node size returned a partial page").into());
        };
        require(
            matches!(
                error,
                DbError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Integer,
                    _
                ))
            ),
            "negative node size returned the wrong conversion error",
        )?;
        Ok(())
    }

    #[test]
    fn validated_existing_database_paths_are_never_recreated() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;

        let current_path = temp.path().join("current.db");
        drop(AtlasStore::open(&current_path)?);
        let (current, current_location) = schema::preflight(&current_path, None)?;
        require_eq(
            &current.state,
            &SchemaState::Current,
            "current preflight state",
        )?;
        fs::remove_file(&current_path)?;
        if Connection::open_with_flags(
            &current_path,
            writable_open_flags(current.state, current_location.database_exists),
        )
        .is_ok()
        {
            return Err(io::Error::other("current database path was recreated").into());
        }
        require_eq(&current_path.exists(), &false, "current path stays absent")?;

        let released_path = temp.path().join("released.db");
        let released_root = temp.path().join("released-root");
        write_released_schema_eight_compatibility_fixture(&released_path, &released_root)?;
        let (released, released_location) = schema::preflight(&released_path, None)?;
        require_eq(
            &released.state,
            &SchemaState::UpgradeRequired,
            "released preflight state",
        )?;
        fs::remove_file(&released_path)?;
        if Connection::open_with_flags(
            &released_path,
            writable_open_flags(released.state, released_location.database_exists),
        )
        .is_ok()
        {
            return Err(io::Error::other("released database path was recreated").into());
        }
        require_eq(
            &released_path.exists(),
            &false,
            "released path stays absent",
        )?;

        let fresh_path = temp.path().join("fresh.db");
        let (fresh, _) = schema::preflight(&fresh_path, None)?;
        require_eq(&fresh.state, &SchemaState::Fresh, "fresh preflight state")?;
        drop(AtlasStore::open(&fresh_path)?);
        require_eq(&fresh_path.is_file(), &true, "fresh path is created")?;
        drop(AtlasStore::open_read_only(&fresh_path)?);
        Ok(())
    }

    #[test]
    fn released_schema_layouts_upgrade_without_losing_local_state() -> Result<(), Box<dyn Error>> {
        let layouts: [(&str, fn(&Path, &Path) -> Result<(), Box<dyn Error>>); 2] = [
            (
                "fresh-v0.3.26",
                write_released_schema_eight_compatibility_fixture,
            ),
            (
                "evolved-v0.3.11-to-v0.3.26",
                write_evolved_released_schema_eight_compatibility_fixture,
            ),
        ];
        let mut previous_binding = None;
        for (label, write_fixture) in layouts {
            let binding =
                assert_released_schema_upgrade_preserves_local_state(label, write_fixture)?;
            if previous_binding
                .as_ref()
                .is_some_and(|previous| previous == &binding)
            {
                return Err(io::Error::other(
                    "independent released-schema databases shared one project binding",
                )
                .into());
            }
            previous_binding = Some(binding);
        }
        Ok(())
    }

    /// Verify one released schema layout through migration, reopen, and publication.
    fn assert_released_schema_upgrade_preserves_local_state(
        label: &str,
        write_fixture: fn(&Path, &Path) -> Result<(), Box<dyn Error>>,
    ) -> Result<CapturedProjectBinding, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let db_path = temp.path().join("projectatlas.db");
        let root = temp.path().join("repository");
        write_fixture(&db_path, &root)?;
        fs::create_dir_all(&root)?;
        let database_before_read = fs::read(&db_path)?;

        let Err(read_error) = AtlasStore::open_read_only(&db_path) else {
            return Err(io::Error::other("schema-8 read-only open unexpectedly succeeded").into());
        };
        require_eq(
            &matches!(
                read_error,
                DbError::SchemaVersion {
                    found: PREVIOUS_SCHEMA_VERSION,
                    expected: SCHEMA_VERSION,
                }
            ),
            &true,
            "schema-8 read-only rejection",
        )?;
        require_eq(
            &fs::read(&db_path)?,
            &database_before_read,
            "read-only rejection leaves database unchanged",
        )?;

        let store = AtlasStore::open(&db_path)?;
        let stored_schema = store.connection.query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [SCHEMA_VERSION_KEY],
            |row| row.get::<_, String>(0),
        )?;
        require_eq(
            &stored_schema,
            &SCHEMA_VERSION.to_string(),
            "upgraded schema version",
        )?;
        let migrated_binding = store.captured_project_binding()?;
        drop(store);
        schema::verify_current_integrity(&db_path, Some(&normalize_native_path_display(&root)))?;
        let mut store = AtlasStore::open_for_project(&db_path, &root)?;
        require_eq(
            &store.captured_project_binding()?,
            &migrated_binding,
            &format!("{label} identity survives reopen"),
        )?;
        require_eq(
            &store.project_root()?,
            &Some(normalize_native_path_display(&root)),
            "upgraded project root",
        )?;
        let node = store
            .load_node_by_path("src/lib.rs")?
            .ok_or_else(|| io::Error::other("upgraded source node missing"))?;
        require_eq(
            &node.node.content_hash,
            &Some("hash-legacy".to_string()),
            "upgraded source row",
        )?;
        require_eq(
            &node.purpose.purpose,
            &Some("Schema compatibility source".to_string()),
            "upgraded purpose text",
        )?;
        require_eq(
            &node.purpose.source,
            &PurposeSource::Agent,
            "upgraded purpose source",
        )?;
        require_eq(
            &node.purpose.status,
            &PurposeStatus::Approved,
            "upgraded purpose review state",
        )?;
        require_eq(
            &store.resolved_health_ids()?,
            &vec!["schema-review".to_string()],
            "upgraded authored review",
        )?;
        let custom_setting = store.connection.query_row(
            "SELECT value FROM metadata WHERE key = 'custom_setting'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        require_eq(
            &custom_setting,
            &"preserved".to_string(),
            "upgraded compatible metadata",
        )?;
        let telemetry = store.token_overview(Some("schema-session"))?;
        require_eq(&telemetry.calls, &1, "upgraded telemetry call count")?;
        require_eq(
            &telemetry.estimated_saved,
            &80,
            "upgraded telemetry savings",
        )?;
        require_eq(
            &store.index_publication()?,
            &None,
            "untrusted publication metadata invalidated",
        )?;
        let remaining_publication_keys = store.connection.query_row(
            "SELECT COUNT(*) FROM metadata WHERE key IN (?1, ?2, ?3)",
            params![
                INDEX_PUBLICATION_STATE_KEY,
                INDEX_PUBLICATION_FINGERPRINT_KEY,
                INDEX_PUBLICATION_GENERATION_KEY,
            ],
            |row| row.get::<_, i64>(0),
        )?;
        require_eq(
            &remaining_publication_keys,
            &0,
            "all untrusted publication keys removed",
        )?;

        {
            let mut publication = store.begin_index_publication("schema-9-contract")?;
            write_test_projection(&mut publication, "fresh")?;
            publication.complete()?;
        }
        require_test_projection(&store, 1, "fresh")?;
        require_eq(
            &store
                .index_publication()?
                .and_then(|publication| publication.contract_fingerprint),
            &Some("schema-9-contract".to_string()),
            &format!("{label} fresh publication contract"),
        )?;
        Ok(migrated_binding)
    }

    #[test]
    fn future_schema_rejection_preserves_source_and_authored_rows() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let db_path = temp.path().join("projectatlas.db");
        let root = temp.path().join("repository");
        fs::create_dir(&root)?;
        let future_schema = SCHEMA_VERSION + 1;
        write_schema_compatibility_fixture(&db_path, &root, future_schema, "future")?;
        let database_before = fs::read(&db_path)?;
        let Err(open_error) = AtlasStore::open(&db_path) else {
            return Err(
                io::Error::other("future schema writable open unexpectedly succeeded").into(),
            );
        };
        require_eq(
            &matches!(
                open_error,
                DbError::SchemaVersion {
                    found,
                    expected: SCHEMA_VERSION,
                } if found == future_schema
            ),
            &true,
            "future schema rejection",
        )?;
        require_eq(
            &fs::read(&db_path)?,
            &database_before,
            "future schema bytes remain unchanged",
        )?;
        let connection = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let stored_schema = connection.query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [SCHEMA_VERSION_KEY],
            |row| row.get::<_, String>(0),
        )?;
        require_eq(
            &stored_schema,
            &future_schema.to_string(),
            "future schema remains unchanged",
        )?;
        let stored_root = connection.query_row(
            "SELECT value FROM metadata WHERE key = 'project_root'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        require_eq(
            &stored_root,
            &normalize_native_path_display(&root),
            "future project root remains unchanged",
        )?;
        let source_state = connection.query_row(
            "
            SELECT n.content_hash, p.purpose, p.source, p.status
            FROM nodes AS n
            JOIN purposes AS p ON p.node_id = n.id
            WHERE n.path = 'src/lib.rs'
            ",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?;
        require_eq(
            &source_state,
            &(
                Some("hash-future".to_string()),
                Some("Schema compatibility source".to_string()),
                PurposeSource::Agent.to_string(),
                PurposeStatus::Approved.as_str().to_string(),
            ),
            "future source and purpose rows remain unchanged",
        )?;
        let review_rationale = connection.query_row(
            "SELECT rationale FROM health_resolutions WHERE finding_id = 'schema-review'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        require_eq(
            &review_rationale,
            &"Reviewed schema fixture".to_string(),
            "future authored review remains unchanged",
        )?;
        let telemetry_state = connection.query_row(
            "
            SELECT COUNT(*), SUM(e.estimated_tokens_saved)
            FROM usage_events AS e
            JOIN usage_instances AS i USING(instance_row_id)
            WHERE i.caller_label = 'schema-session'
            ",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
        )?;
        require_eq(
            &telemetry_state,
            &(1, Some(80)),
            "future telemetry remains unchanged",
        )?;
        Ok(())
    }

    #[test]
    fn projection_refresh_cannot_replace_publication_contract() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.begin_index_publication("contract-a")?.complete()?;

        store
            .begin_index_projection_refresh("contract-a")?
            .complete()?;
        require_eq(
            &store.index_publication()?,
            &Some(IndexPublication {
                state: IndexPublicationState::Complete,
                contract_fingerprint: Some("contract-a".to_string()),
                generation: IndexGeneration::new(2),
            }),
            "matching projection refresh",
        )?;

        let publication = store.begin_index_projection_refresh("contract-a")?;
        set_metadata(
            &publication.connection,
            INDEX_PUBLICATION_FINGERPRINT_KEY,
            "contract-b",
        )?;
        let Err(mismatch) = publication.complete() else {
            return Err(io::Error::other(
                "projection refresh replaced the global publication contract",
            )
            .into());
        };
        if !matches!(mismatch, DbError::PublicationContractChanged) {
            return Err(io::Error::other(format!(
                "unexpected projection refresh mismatch: {mismatch}"
            ))
            .into());
        }
        require_eq(
            &store.index_publication()?,
            &Some(IndexPublication {
                state: IndexPublicationState::Complete,
                contract_fingerprint: Some("contract-a".to_string()),
                generation: IndexGeneration::new(2),
            }),
            "mismatched projection refresh rolls back",
        )?;
        Ok(())
    }

    #[test]
    fn read_snapshots_expose_only_complete_publications() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let db_path = temp.path().join("projectatlas.db");
        let independent_db_path = temp.path().join("independent.db");
        let mut writer_a = AtlasStore::open(&db_path)?;
        require_writable_connection_profile(&writer_a.connection)?;
        {
            let mut publication = writer_a.begin_index_publication("contract")?;
            write_test_projection(&mut publication, "old")?;
            publication.complete()?;
        }
        let mut writer_b = AtlasStore::open(&db_path)?;
        let mut independent_writer = AtlasStore::open(&independent_db_path)?;
        let old_reader = AtlasStore::open_read_only(&db_path)?;
        require_read_connection_profile(&old_reader.connection)?;
        if old_reader
            .connection
            .execute("DELETE FROM metadata", [])
            .is_ok()
        {
            return Err(io::Error::other("read-only connection accepted a mutation").into());
        }
        require_test_projection(&old_reader, 1, "old")?;

        {
            let mut publication = writer_a.begin_index_publication("contract")?;
            write_test_projection(&mut publication, "new")?;
            require_test_projection(&old_reader, 1, "old")?;

            let started = std::time::Instant::now();
            let Err(contention) = writer_b.begin_index_publication("contract") else {
                return Err(io::Error::other(
                    "second writer entered an active publication transaction",
                )
                .into());
            };
            require_eq(
                &matches!(
                    contention,
                    DbError::Sqlite(ref error)
                        if matches!(
                            error.sqlite_error_code(),
                            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
                        )
                ),
                &true,
                "same-database writer contention",
            )?;
            require_eq(
                &(started.elapsed() < Duration::from_secs(2)),
                &true,
                "fail-fast same-database writer acquisition",
            )?;
            let restored_busy_timeout =
                writer_b
                    .connection
                    .query_row("PRAGMA busy_timeout", [], |row| row.get::<_, i64>(0))?;
            require_eq(
                &u128::try_from(restored_busy_timeout)?,
                &SQLITE_BUSY_TIMEOUT.as_millis(),
                "ordinary busy timeout after failed publication acquisition",
            )?;

            independent_writer
                .begin_index_publication("independent-contract")?
                .complete()?;
            require_eq(
                &independent_writer
                    .index_publication()?
                    .ok_or_else(|| io::Error::other("independent publication missing"))?
                    .generation,
                &IndexGeneration::new(1),
                "independent database generation",
            )?;

            drop(publication);
        }
        let rolled_back_reader = AtlasStore::open_read_only(&db_path)?;
        require_test_projection(&rolled_back_reader, 1, "old")?;
        rolled_back_reader.finish_index_read_snapshot()?;

        {
            let mut publication = writer_a.begin_index_publication("contract")?;
            write_test_projection(&mut publication, "new")?;
            publication.complete()?;
        }
        let restored_busy_timeout =
            writer_a
                .connection
                .query_row("PRAGMA busy_timeout", [], |row| row.get::<_, i64>(0))?;
        require_eq(
            &u128::try_from(restored_busy_timeout)?,
            &SQLITE_BUSY_TIMEOUT.as_millis(),
            "ordinary busy timeout after successful publication acquisition",
        )?;
        require_test_projection(&old_reader, 1, "old")?;
        let new_reader = AtlasStore::open_read_only(&db_path)?;
        require_test_projection(&new_reader, 2, "new")?;
        new_reader.finish_index_read_snapshot()?;
        old_reader.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn authored_write_waits_for_the_existing_writer_before_validation() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let db_path = temp.path().join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&db_path, &root)?;
        store.replace_scan(&[test_file_node("src/lib.rs", "initial")])?;

        let blocking_writer = Connection::open(&db_path)?;
        blocking_writer.busy_timeout(SQLITE_BUSY_TIMEOUT)?;
        blocking_writer.execute_batch("BEGIN IMMEDIATE")?;
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1_250));
            blocking_writer.execute_batch("ROLLBACK")
        });

        let started = Instant::now();
        store.set_purpose(
            "src/lib.rs",
            "Own the library source.",
            PurposeSource::Agent,
        )?;
        let elapsed = started.elapsed();
        release
            .join()
            .map_err(|_panic| io::Error::other("blocking writer thread panicked"))??;

        require_eq(
            &(elapsed >= Duration::from_secs(1)),
            &true,
            "authored write waited for the existing writer",
        )?;
        require_eq(
            &(elapsed < SQLITE_BUSY_TIMEOUT),
            &true,
            "authored write stayed within the ordinary busy timeout",
        )?;
        require_eq(
            &store
                .load_node_by_path("src/lib.rs")?
                .ok_or_else(|| io::Error::other("purpose node missing"))?
                .purpose
                .purpose,
            &Some("Own the library source.".to_string()),
            "purpose after bounded writer contention",
        )
    }

    #[test]
    fn writable_mutators_reject_an_active_read_snapshot() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let db_path = temp.path().join("projectatlas.db");
        let mut store = AtlasStore::open(&db_path)?;
        store.replace_scan(&[test_file_node("src/lib.rs", "initial")])?;
        store.begin_index_read_snapshot()?;

        let Err(purpose_error) = store.set_purpose(
            "src/lib.rs",
            "Must not be written through a read snapshot.",
            PurposeSource::Agent,
        ) else {
            return Err(io::Error::other("purpose wrote through a read snapshot").into());
        };
        require_eq(
            &matches!(purpose_error, DbError::IndexReadSnapshotActive),
            &true,
            "read-snapshot purpose rejection",
        )?;

        let Err(scan_error) = store.replace_scan(&[]) else {
            return Err(io::Error::other("scan wrote through a read snapshot").into());
        };
        require_eq(
            &matches!(scan_error, DbError::IndexReadSnapshotActive),
            &true,
            "read-snapshot scan rejection",
        )?;

        store.finish_index_read_snapshot()?;
        require_eq(
            &store.load_node_by_path("src/lib.rs")?.is_some(),
            &true,
            "read-snapshot rejected writes preserved indexed state",
        )?;
        Ok(())
    }

    #[test]
    fn stale_publication_base_is_rejected_before_batch_mutation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let db_path = temp.path().join("projectatlas.db");
        let mut writer_a = AtlasStore::open(&db_path)?;
        {
            let mut publication =
                writer_a.begin_index_publication_from("contract", IndexGeneration::ZERO)?;
            write_test_projection(&mut publication, "base")?;
            publication.complete()?;
        }
        let prepared_base = writer_a
            .index_publication()?
            .ok_or_else(|| io::Error::other("prepared base publication missing"))?
            .generation;

        let incomplete_result = {
            let mut publication =
                writer_a.begin_index_publication_from("contract", prepared_base)?;
            publication.begin_scan_replacement()?;
            publication.complete()
        };
        if !matches!(incomplete_result, Err(DbError::ScanReplacementIncomplete)) {
            return Err(io::Error::other(
                "unfinished scan replacement was allowed to complete publication",
            )
            .into());
        }
        require_test_projection(&writer_a, 1, "base")?;

        let mut writer_b = AtlasStore::open(&db_path)?;
        {
            let mut publication = writer_b.begin_index_publication("contract")?;
            write_test_projection(&mut publication, "winner")?;
            publication.complete()?;
        }

        let Err(conflict) = writer_a.begin_index_publication_from("contract", prepared_base) else {
            return Err(io::Error::other("stale prepared publication was accepted").into());
        };
        require_eq(
            &matches!(
                conflict,
                DbError::PublicationBaseGenerationChanged { expected, found }
                    if expected == IndexGeneration::new(1)
                        && found == IndexGeneration::new(2)
            ),
            &true,
            "stale publication base conflict",
        )?;
        require_test_projection(&writer_a, 2, "winner")?;
        require_eq(
            &writer_a.load_symbols(Some("src/lib.rs"), None, 10)?.len(),
            &1,
            "rejected batch symbol row count",
        )?;
        require_eq(
            &writer_a
                .load_symbol_relations(Some("src/lib.rs"), None, 10)?
                .len(),
            &1,
            "rejected batch relation row count",
        )?;

        let Err(zero_conflict) =
            writer_a.begin_index_publication_from("contract", IndexGeneration::ZERO)
        else {
            return Err(io::Error::other("zero base matched an initialized store").into());
        };
        require_eq(
            &matches!(
                zero_conflict,
                DbError::PublicationBaseGenerationChanged { expected, found }
                    if expected == IndexGeneration::ZERO
                        && found == IndexGeneration::new(2)
            ),
            &true,
            "zero publication base conflict",
        )?;
        require_test_projection(&writer_a, 2, "winner")?;
        Ok(())
    }

    #[test]
    fn records_token_overview() -> Result<(), Box<dyn Error>> {
        let project = tempfile::tempdir()?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(project.path())?;
        let mut session_event = usage_from_estimates(
            "session",
            "outline",
            Some("src/main.rs".to_string()),
            None,
            100,
            20,
        );
        session_event.estimated_tokens_saved = Some(1);
        store.record_usage(&session_event)?;
        let mut unknown_event = usage_from_estimates("session", "unknown", None, None, 0, 0);
        unknown_event.estimated_tokens_without_projectatlas = None;
        unknown_event.estimated_tokens_with_projectatlas = None;
        unknown_event.estimated_tokens_saved = None;
        store.record_usage(&unknown_event)?;
        store.record_usage(&usage_from_estimates(
            "other-session",
            "outline",
            Some("src/lib.rs".to_string()),
            None,
            200,
            50,
        ))?;
        let overview = store.token_overview(Some("session"))?;
        require_eq(&overview.calls, &1, "usage call count")?;
        require_eq(&overview.estimated_saved, &80, "saved token count")?;
        require_eq(&overview.buckets.len(), &1, "usage bucket count")?;
        require_eq(
            &overview.buckets[0].accuracy,
            &TOKEN_ACCURACY_HEURISTIC.to_string(),
            "usage bucket accuracy",
        )?;
        let all_sessions = store.token_overview(None)?;
        require_eq(&all_sessions.calls, &2, "all-session usage call count")?;
        require_eq(
            &all_sessions.estimated_without_projectatlas,
            &300,
            "all-session baseline tokens",
        )?;
        require_eq(
            &all_sessions.estimated_with_projectatlas,
            &70,
            "all-session atlas tokens",
        )?;
        require_eq(
            &all_sessions.estimated_saved,
            &230,
            "all-session saved tokens",
        )?;
        require_eq(
            &all_sessions.likely_file_reads_avoided,
            &0,
            "non-search estimate events do not count as avoided file reads",
        )?;
        require_eq(
            &all_sessions.read_avoidance_confidence,
            &READ_AVOIDANCE_CONFIDENCE_NOT_RECORDED.to_string(),
            "non-search estimate read avoidance confidence",
        )?;

        store.record_usage(&usage_from_text(
            "bucketed",
            "summary",
            Some("src/main.rs".to_string()),
            None,
            "abcdefghijkl",
            "abcd",
        ))?;
        store.record_usage(&usage_from_estimates(
            "bucketed", "folders", None, None, 100, 20,
        ))?;
        let bucketed = store.token_overview(Some("bucketed"))?;
        require_eq(&bucketed.buckets.len(), &2, "bucketed overview count")?;
        require_eq(
            &bucketed.buckets[0].token_savings_bucket,
            &TOKEN_BUCKET_FULL_FILE_COMPRESSION.to_string(),
            "source compression bucket",
        )?;
        require_eq(
            &bucketed.buckets[1].token_savings_bucket,
            &TOKEN_BUCKET_NAVIGATION_AVOIDANCE.to_string(),
            "navigation bucket",
        )?;
        require_eq(
            &bucketed.observed_file_read_replacements,
            &1,
            "observed read replacement count",
        )?;
        require_eq(
            &bucketed.modeled_file_reads_avoided,
            &0,
            "folder navigation does not count as modeled file-read avoidance",
        )?;
        require_eq(
            &bucketed.likely_file_reads_avoided,
            &1,
            "observed-only likely file reads avoided",
        )?;
        require_eq(
            &bucketed.read_avoidance_confidence,
            &READ_AVOIDANCE_CONFIDENCE_OBSERVED.to_string(),
            "observed-only read avoidance confidence",
        )?;

        store.record_usage(&usage_from_text(
            "deduped",
            "summary",
            Some("src/lib.rs".to_string()),
            None,
            "abcdabcd",
            "ab",
        ))?;
        store.record_usage(&usage_from_estimates(
            "deduped",
            TOKEN_COMMAND_SEARCH,
            None,
            Some("token".to_string()),
            400,
            40,
        ))?;
        store.record_usage(&usage_from_estimates(
            "deduped",
            TOKEN_COMMAND_SEARCH,
            None,
            Some("token".to_string()),
            400,
            30,
        ))?;
        let deduped = store.token_overview(Some("deduped"))?;
        require_eq(
            &deduped.legacy_gross_estimated_saved,
            &731,
            "legacy gross saved tokens remains available",
        )?;
        require_eq(
            &deduped.measured_tokens_saved,
            &1,
            "measured saved tokens remain separate",
        )?;
        require_eq(
            &deduped.gross_modeled_tokens_avoided,
            &730,
            "gross modeled avoided tokens remains available",
        )?;
        require_eq(
            &deduped.deduped_modeled_tokens_avoided,
            &330,
            "modeled avoided tokens are deduped by baseline",
        )?;
        require_eq(
            &deduped.tokens_avoided,
            &331,
            "headline avoided tokens use measured plus deduped modeled",
        )?;
        require_eq(
            &deduped.observed_file_read_replacements,
            &1,
            "deduped observed read replacements",
        )?;
        require_eq(
            &deduped.modeled_file_reads_avoided,
            &2,
            "deduped raw search events remain likely file reads avoided",
        )?;
        require_eq(
            &deduped.likely_file_reads_avoided,
            &3,
            "deduped likely file reads avoided",
        )?;
        require_eq(
            &deduped.read_avoidance_confidence,
            &READ_AVOIDANCE_CONFIDENCE_MODELED.to_string(),
            "deduped read avoidance confidence",
        )?;

        store.record_usage(&usage_from_estimates_with_accounting(
            "event-scoped",
            "folders",
            None,
            Some("token".to_string()),
            400,
            40,
            TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
            TOKEN_BASELINE_SELECTED_CANDIDATES,
            TOKEN_CONFIDENCE_INFERRED,
            TOKEN_ACCOUNTING_MODELED_AVOIDANCE,
            TOKEN_BASELINE_SELECTED_CANDIDATES,
            TOKEN_DEDUPE_SCOPE_EVENT,
        ))?;
        store.record_usage(&usage_from_estimates_with_accounting(
            "event-scoped",
            "folders",
            None,
            Some("token".to_string()),
            400,
            30,
            TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
            TOKEN_BASELINE_SELECTED_CANDIDATES,
            TOKEN_CONFIDENCE_INFERRED,
            TOKEN_ACCOUNTING_MODELED_AVOIDANCE,
            TOKEN_BASELINE_SELECTED_CANDIDATES,
            TOKEN_DEDUPE_SCOPE_EVENT,
        ))?;
        let event_scoped = store.token_overview(Some("event-scoped"))?;
        require_eq(
            &event_scoped.gross_modeled_tokens_avoided,
            &730,
            "event-scoped gross modeled avoided tokens",
        )?;
        require_eq(
            &event_scoped.deduped_modeled_tokens_avoided,
            &730,
            "event-scoped modeled events are not collapsed",
        )?;
        require_eq(
            &event_scoped.repeated_baselines_deduped,
            &0,
            "event-scoped modeled events do not count as deduped repeats",
        )?;
        require_eq(
            &event_scoped.likely_file_reads_avoided,
            &0,
            "folder navigation events do not count as likely file reads avoided",
        )?;
        require_eq(
            &event_scoped.read_avoidance_confidence,
            &READ_AVOIDANCE_CONFIDENCE_NOT_RECORDED.to_string(),
            "folder navigation read avoidance confidence",
        )?;

        let mut negative_event = usage_from_estimates("negative", "outline", None, None, 20, 50);
        negative_event.estimated_tokens_saved = Some(999);
        store.record_usage(&negative_event)?;
        let negative = store.token_overview(Some("negative"))?;
        require_eq(&negative.calls, &1, "negative session call count")?;
        require_eq(
            &negative.estimated_saved,
            &-30,
            "negative session recomputed delta",
        )?;
        require_eq(
            &negative.savings_rate,
            &Some(-1.5),
            "negative session savings rate",
        )?;

        let mut zero_event = usage_from_estimates("zero-baseline", "outline", None, None, 0, 12);
        zero_event.estimated_tokens_saved = Some(999);
        store.record_usage(&zero_event)?;
        let zero_baseline = store.token_overview(Some("zero-baseline"))?;
        require_eq(&zero_baseline.calls, &1, "zero baseline call count")?;
        require_eq(
            &zero_baseline.estimated_saved,
            &-12,
            "zero baseline recomputed delta",
        )?;
        require_eq(
            &zero_baseline.savings_rate,
            &None,
            "zero baseline savings rate",
        )?;

        let large_project = tempfile::tempdir()?;
        let mut large_store = AtlasStore::in_memory()?;
        large_store.set_project_root(large_project.path())?;
        let maximum_estimate = usize::try_from(i64::MAX)?;
        large_store.record_usage(&usage_from_estimates(
            "large-primary",
            "large",
            None,
            None,
            maximum_estimate,
            0,
        ))?;
        let Err(overflow) = large_store.record_usage(&usage_from_estimates(
            "large-rejected",
            "large",
            None,
            None,
            maximum_estimate,
            0,
        )) else {
            return Err(io::Error::other("overflowing telemetry aggregate was committed").into());
        };
        require_eq(
            &matches!(overflow, DbError::TelemetryIntegerOverflow { .. }),
            &true,
            "overflowing telemetry aggregate error",
        )?;
        let large = large_store.token_overview(Some("large-primary"))?;
        require_eq(
            &large.estimated_saved,
            &isize::MAX,
            "largest accepted aggregate narrows at the public boundary",
        )?;
        require_eq(
            &large_store.token_overview(Some("large-rejected"))?.calls,
            &0,
            "rejected aggregate left no partial report",
        )?;
        Ok(())
    }

    #[test]
    fn direct_library_usage_rotates_at_baseline_capacity_without_reopening_the_old_scope()
    -> Result<(), Box<dyn Error>> {
        let project = tempfile::tempdir()?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(project.path())?;
        let policy = TelemetryRetentionPolicy::default();
        let label = "bounded-library-label";

        for index in 0..policy.max_baselines_per_instance {
            store.record_usage(&usage_from_estimates(
                label,
                "search",
                None,
                Some(format!("query-{index}")),
                100,
                20,
            ))?;
        }
        let old_instance = store
            .library_usage_instances
            .borrow()
            .get(label)
            .copied()
            .flatten()
            .ok_or_else(|| io::Error::other("library instance was not retained"))?;

        let boundary_event = usage_from_estimates(
            label,
            "search",
            None,
            Some("capacity-boundary".to_string()),
            100,
            20,
        );
        store.record_usage(&boundary_event)?;
        let replacement = store
            .library_usage_instances
            .borrow()
            .get(label)
            .copied()
            .flatten()
            .ok_or_else(|| io::Error::other("replacement library instance was not retained"))?;
        require_eq(
            &(replacement != old_instance),
            &true,
            "capacity rotation created a new internal instance",
        )?;
        require_eq(
            &store.connection.query_row(
                "SELECT COUNT(*) FROM usage_instances WHERE state = 'sealed'",
                [],
                |row| row.get::<_, i64>(0),
            )?,
            &1,
            "sealed predecessor instances",
        )?;
        require_eq(
            &store.connection.query_row(
                "SELECT COUNT(*) FROM usage_instances WHERE state = 'active'",
                [],
                |row| row.get::<_, i64>(0),
            )?,
            &1,
            "active replacement instances",
        )?;
        require_eq(
            &store.connection.query_row(
                "SELECT COUNT(*) FROM usage_instance_baselines",
                [],
                |row| row.get::<_, i64>(0),
            )?,
            &1,
            "only replacement baseline witnesses remain",
        )?;
        require_eq(
            &store.token_overview(Some(label))?.calls,
            &(policy.max_baselines_per_instance + 1),
            "caller-label report spans both bounded instances",
        )?;
        require_eq(
            &store.token_overview(None)?.estimated_saved,
            &(isize::try_from(policy.max_baselines_per_instance + 1)? * 80),
            "global totals remain exact across rotation",
        )?;
        require_eq(
            &matches!(
                store.record_usage_for_instance(
                    old_instance,
                    UsageInstanceOwner::LibraryHandle,
                    &boundary_event,
                    false,
                ),
                Err(DbError::TelemetryInstanceInactive)
            ),
            &true,
            "sealed predecessor cannot be reopened",
        )?;
        Ok(())
    }

    #[test]
    fn rejected_library_labels_do_not_consume_the_bounded_identity_map()
    -> Result<(), Box<dyn Error>> {
        let project = tempfile::tempdir()?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(project.path())?;
        let maximum = TelemetryRetentionPolicy::default().max_label_bytes;

        for extra in 1..=4 {
            let event =
                usage_from_estimates(&"x".repeat(maximum + extra), "summary", None, None, 100, 20);
            require_eq(
                &matches!(
                    store.record_usage(&event),
                    Err(DbError::TelemetryFieldTooLarge {
                        field: "session_id",
                        ..
                    })
                ),
                &true,
                "oversized caller label rejection",
            )?;
        }
        require_eq(
            &store.library_usage_instances.borrow().len(),
            &0,
            "rejected labels retained no identity-map entries",
        )?;
        Ok(())
    }

    #[test]
    fn token_trends_group_usage_by_period_and_bucket() -> Result<(), Box<dyn Error>> {
        let project = tempfile::tempdir()?;
        let mut store = AtlasStore::in_memory()?;
        store.set_project_root(project.path())?;
        for (session, bucket, baseline_kind, confidence, without, with) in [
            (
                "session",
                TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
                "selected_candidates",
                "inferred",
                100_usize,
                25_usize,
            ),
            (
                "session",
                TOKEN_BUCKET_FULL_FILE_COMPRESSION,
                "full_file",
                "observed",
                50_usize,
                10_usize,
            ),
            (
                "session",
                TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
                "selected_candidates",
                "inferred",
                80_usize,
                20_usize,
            ),
            (
                "other",
                TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
                "selected_candidates",
                "inferred",
                999_usize,
                1_usize,
            ),
        ] {
            store.record_usage(&usage_from_estimates_with_context(
                session,
                "trend",
                None,
                None,
                without,
                with,
                bucket,
                baseline_kind,
                confidence,
            ))?;
        }

        let trends = store.token_trends(Some("session"), TokenTrendWindow::Day)?;
        require_eq(&trends.periods.len(), &1, "current daily period")?;
        require_eq(&trends.periods[0].calls, &3, "session trend call count")?;
        require_eq(
            &trends.periods[0].estimated_saved,
            &175,
            "session trend saved tokens",
        )?;
        require_eq(
            &trends.periods[0].buckets.len(),
            &2,
            "trend preserves evidence buckets",
        )?;
        require_eq(
            &trends.periods[0].buckets[0].token_savings_bucket,
            &TOKEN_BUCKET_FULL_FILE_COMPRESSION.to_string(),
            "full-file bucket remains visible",
        )?;
        require_eq(
            &trends.periods[0].buckets[0].confidence,
            &"observed".to_string(),
            "bucket confidence remains visible",
        )?;
        let all_labels = store.token_trends(None, TokenTrendWindow::Day)?;
        require_eq(&all_labels.periods.len(), &1, "all-label daily period")?;
        require_eq(&all_labels.periods[0].calls, &4, "all-label trend calls")?;
        Ok(())
    }

    #[test]
    fn unsupported_legacy_schema_is_refused_without_mutation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let db_path = temp.path().join("legacy.db");
        {
            let connection = Connection::open(&db_path)?;
            connection.execute_batch(
                "
                CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
                INSERT INTO metadata(key, value) VALUES('schema_version', '7');
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
                    calculation_trace TEXT NOT NULL DEFAULT 'heuristic=ceil(chars_or_bytes/4)'
                );
                INSERT INTO usage_events(
                    session_id,
                    command,
                    estimated_tokens_without_projectatlas,
                    estimated_tokens_with_projectatlas,
                    estimated_tokens_saved
                )
                VALUES('legacy-session', 'legacy', 100, 20, 80);
                ",
            )?;
        }

        let database_before = fs::read(&db_path)?;
        let Err(open_error) = AtlasStore::open(&db_path) else {
            return Err(io::Error::other("unsupported schema unexpectedly opened").into());
        };
        require_eq(
            &matches!(
                open_error,
                DbError::SchemaVersion {
                    found: 7,
                    expected: SCHEMA_VERSION,
                }
            ),
            &true,
            "unsupported schema rejection",
        )?;
        require_eq(
            &fs::read(&db_path)?,
            &database_before,
            "unsupported schema bytes remain unchanged",
        )?;
        Ok(())
    }

    #[test]
    fn set_project_root_requires_existing_root_and_preserves_binding() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let mut store = AtlasStore::in_memory()?;
        let missing = temp.path().join("missing-root");
        let Err(missing_error) = store.set_project_root(&missing) else {
            return Err(io::Error::other("missing project root was accepted").into());
        };
        require(
            matches!(missing_error, DbError::ProjectRootIdentity(_)),
            "missing project root returned the wrong error",
        )?;
        require_eq(
            &store.project_root()?,
            &None,
            "missing-root rejection changed metadata",
        )?;
        require_eq(
            &store.project_root_identity()?,
            &None,
            "missing-root rejection changed native identity",
        )?;
        require_eq(
            &store.project_instance_id()?,
            &None,
            "missing-root rejection changed project identity",
        )?;

        let root = temp.path().join("workspace").join("example");
        fs::create_dir_all(&root)?;
        store.set_project_root(&root)?;
        require_eq(
            &store.project_root()?,
            &Some(normalize_native_path_display(&root)),
            "project root metadata",
        )?;

        #[cfg(windows)]
        let extended = PathBuf::from(format!(r"\\?\{}", root.display()));
        #[cfg(windows)]
        store.set_project_root(&extended)?;
        #[cfg(windows)]
        require_eq(
            &store.project_root()?,
            &Some(normalize_native_path_display(&root)),
            "windows extended project root metadata",
        )?;
        let other = temp.path().join("other");
        fs::create_dir(&other)?;
        let Err(rebind_error) = store.set_project_root(&other) else {
            return Err(io::Error::other("project identity was rebound implicitly").into());
        };
        require_eq(
            &matches!(rebind_error, DbError::ProjectRootMismatch { .. }),
            &true,
            "project identity rebind rejection",
        )?;
        Ok(())
    }

    #[test]
    fn ordinary_root_admission_rejects_missing_and_regular_paths_without_mutation()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("root");
        let missing = temp.path().join("missing-root");
        let regular = temp.path().join("regular-root");
        fs::create_dir(&root)?;
        fs::write(&regular, b"not a directory")?;
        let database = temp.path().join("projectatlas.db");
        drop(AtlasStore::open_for_project(&database, &root)?);

        for (label, candidate) in [
            ("missing", missing.as_path()),
            ("regular", regular.as_path()),
        ] {
            let before = fs::read(&database)?;
            if AtlasStore::open_for_project(&database, candidate).is_ok() {
                return Err(
                    io::Error::other(format!("ordinary {label} root admission succeeded")).into(),
                );
            }
            if fs::read(&database)? != before {
                return Err(io::Error::other(format!(
                    "ordinary {label} root admission mutated the database"
                ))
                .into());
            }
            if verify_project_database(&database, candidate).is_ok() {
                return Err(
                    io::Error::other(format!("verify admitted ordinary {label} root")).into(),
                );
            }
            if fs::read(&database)? != before {
                return Err(io::Error::other(format!(
                    "verify of ordinary {label} root mutated the database"
                ))
                .into());
            }
        }
        Ok(())
    }

    #[test]
    fn malformed_project_root_identity_is_refused_without_mutation() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("root");
        fs::create_dir(&root)?;
        let database = temp.path().join("projectatlas.db");
        drop(AtlasStore::open_for_project(&database, &root)?);
        let connection = Connection::open(&database)?;
        connection.execute(
            "UPDATE project_root_identity SET root = ?1 WHERE singleton = 1",
            [vec![1_u8, 1_u8, b'x']],
        )?;
        drop(connection);
        let before = fs::read(&database)?;

        if AtlasStore::open(&database).is_ok()
            || AtlasStore::open_for_project(&database, &root).is_ok()
            || verify_project_database(&database, &root).is_ok()
        {
            return Err(io::Error::other("malformed project-root identity was admitted").into());
        }
        if fs::read(&database)? != before {
            return Err(io::Error::other(
                "malformed project-root identity refusal mutated the database",
            )
            .into());
        }
        Ok(())
    }

    #[test]
    fn read_project_root_read_only_does_not_change_database_bytes() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo with spaces");
        let atlas_dir = root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let db_path = atlas_dir.join("projectatlas.db");
        {
            let mut store = AtlasStore::open(&db_path)?;
            store.set_project_root(&root)?;
            store
                .connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        }
        let wal_path = db_path.with_extension("db-wal");
        let shm_path = db_path.with_extension("db-shm");
        for sidecar_path in [&wal_path, &shm_path] {
            match fs::remove_file(sidecar_path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        let database_before = fs::read(&db_path)?;

        require_eq(
            &read_project_root_read_only(&db_path)?,
            &Some(normalize_native_path_display(&root)),
            "read-only project root",
        )?;
        require_eq(
            &read_legacy_project_root_candidate_read_only(&db_path)?,
            &None,
            "current schema has no legacy recovery candidate",
        )?;
        require_eq(
            &fs::read(&db_path)?,
            &database_before,
            "read-only project-root database bytes",
        )?;
        Ok(())
    }

    #[test]
    fn read_project_root_read_only_observes_active_wal_state() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo-Δ");
        let atlas_dir = root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let db_path = atlas_dir.join("projectatlas.db");
        let mut store = AtlasStore::open(&db_path)?;
        store
            .connection
            .execute_batch("PRAGMA wal_autocheckpoint = 0")?;
        store.set_project_root(&root)?;

        require_eq(
            &sqlite_sidecar_path(&db_path, "-wal").exists(),
            &true,
            "active WAL exists",
        )?;
        require_eq(
            &read_project_root_read_only(&db_path)?,
            &Some(normalize_native_path_display(&root)),
            "WAL-aware read-only project root",
        )?;
        Ok(())
    }

    #[test]
    fn validated_write_transaction_revalidates_before_operation_and_rolls_back()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let db_path = temp.path().join("projectatlas.db");
        let store = AtlasStore::open_for_project(&db_path, &root)?;
        let expected_identity = store
            .validated_project_instance_id
            .ok_or(DbError::ProjectInstanceIdentityMissing)?;
        let sentinel_key = "validated_write_transaction_test";

        with_validated_write_transaction(
            &store.connection,
            store.validated_project_root.as_deref(),
            Some(expected_identity),
            |connection| set_metadata(connection, sentinel_key, "committed"),
        )?;
        require_eq(
            &store.connection.query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                [sentinel_key],
                |row| row.get::<_, String>(0),
            )?,
            &"committed".to_string(),
            "validated write positive control",
        )?;

        let Err(operation_error) = with_validated_write_transaction(
            &store.connection,
            store.validated_project_root.as_deref(),
            Some(expected_identity),
            |connection| -> DbResult<()> {
                set_metadata(connection, sentinel_key, "rolled-back")?;
                Err(DbError::TelemetryIdentityUnavailable)
            },
        ) else {
            return Err(io::Error::other("failing validated write unexpectedly committed").into());
        };
        require_eq(
            &matches!(operation_error, DbError::TelemetryIdentityUnavailable),
            &true,
            "validated write operation error",
        )?;
        require_eq(
            &store.connection.query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                [sentinel_key],
                |row| row.get::<_, String>(0),
            )?,
            &"committed".to_string(),
            "validated write rollback",
        )?;

        let future_schema = schema::SCHEMA_VERSION + 1;
        let newer_owner = Connection::open(&db_path)?;
        newer_owner.execute(
            "UPDATE metadata SET value = ?2 WHERE key = ?1",
            params![schema::SCHEMA_VERSION_KEY, future_schema.to_string()],
        )?;
        let operation_invoked = Cell::new(false);
        let Err(binding_error) = with_validated_write_transaction(
            &store.connection,
            store.validated_project_root.as_deref(),
            Some(expected_identity),
            |connection| {
                operation_invoked.set(true);
                set_metadata(connection, sentinel_key, "wrong-schema")
            },
        ) else {
            return Err(io::Error::other("newer schema accepted a validated write").into());
        };
        require_eq(
            &matches!(
                binding_error,
                DbError::SchemaVersion { found, expected }
                    if found == future_schema && expected == schema::SCHEMA_VERSION
            ),
            &true,
            "validated write schema recheck",
        )?;
        require_eq(
            &operation_invoked.get(),
            &false,
            "validated write closure invocation",
        )?;
        require_eq(
            &newer_owner.query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                [sentinel_key],
                |row| row.get::<_, String>(0),
            )?,
            &"committed".to_string(),
            "newer-schema sentinel value",
        )?;
        Ok(())
    }

    #[test]
    fn read_only_telemetry_revalidates_project_identity_before_write() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let db_path = temp.path().join("projectatlas.db");
        drop(AtlasStore::open_for_project(&db_path, &root)?);

        let reader = AtlasStore::open_read_only_for_project(&db_path, &root)?;
        reader.finish_index_read_snapshot()?;
        AtlasStore::transition_project_root(&db_path, &root, ProjectRootTransition::Detach)?;

        let event = usage_from_estimates(
            "identity-race",
            "summary",
            Some("src/lib.rs".to_string()),
            None,
            100,
            20,
        );
        let Err(error) = reader.record_usage(&event) else {
            return Err(io::Error::other("telemetry wrote through replaced identity").into());
        };
        require_eq(
            &matches!(error, DbError::ProjectRootTransitionChanged { .. }),
            &true,
            "telemetry project identity recheck",
        )?;
        let current = AtlasStore::open_read_only_for_project(&db_path, &root)?;
        require_eq(
            &current.token_overview(Some("identity-race"))?.calls,
            &0,
            "telemetry refused after identity replacement",
        )?;
        Ok(())
    }

    #[test]
    fn telemetry_contention_uses_ancillary_busy_budget() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let db_path = temp.path().join("projectatlas.db");
        drop(AtlasStore::open_for_project(&db_path, &root)?);

        let reader = AtlasStore::open_read_only_for_project(&db_path, &root)?;
        let writer = Connection::open(&db_path)?;
        writer.execute_batch("BEGIN IMMEDIATE")?;
        let started = Instant::now();
        let result = reader.record_usage(&usage_from_estimates(
            "contended",
            "overview",
            None,
            None,
            100,
            20,
        ));
        let elapsed = started.elapsed();
        writer.execute_batch("ROLLBACK")?;

        let Err(error) = result else {
            return Err(
                io::Error::other("contended telemetry write unexpectedly succeeded").into(),
            );
        };
        require_eq(
            &error.is_write_unavailable(),
            &true,
            "contended telemetry error kind",
        )?;
        require_eq(
            &(elapsed < Duration::from_millis(500)),
            &true,
            "ancillary telemetry busy latency",
        )?;
        require_eq(
            &reader.token_overview(Some("contended"))?.calls,
            &0,
            "contended telemetry rollback",
        )?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn read_only_telemetry_does_not_recreate_a_removed_database() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(&root)?;
        let db_path = temp.path().join("projectatlas.db");
        drop(AtlasStore::open_for_project(&db_path, &root)?);

        let reader = AtlasStore::open_read_only_for_project(&db_path, &root)?;
        fs::remove_file(&db_path)?;
        let event = usage_from_estimates(
            "removed-database",
            "summary",
            Some("src/lib.rs".to_string()),
            None,
            100,
            20,
        );
        if reader.record_usage(&event).is_ok() {
            return Err(io::Error::other("telemetry reopened a removed database").into());
        }
        require_eq(
            &db_path.exists(),
            &false,
            "telemetry must not recreate a removed database",
        )?;
        Ok(())
    }

    #[test]
    fn read_only_store_opens_checkpointed_wal_without_existing_sidecars()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repo with spaces");
        let atlas_dir = root.join(".projectatlas");
        fs::create_dir_all(&atlas_dir)?;
        let db_path = atlas_dir.join("projectatlas.db");
        {
            let mut store = AtlasStore::open(&db_path)?;
            store.set_project_root(&root)?;
            store
                .connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        }
        for sidecar_path in [
            sqlite_sidecar_path(&db_path, "-wal"),
            sqlite_sidecar_path(&db_path, "-shm"),
        ] {
            match fs::remove_file(sidecar_path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }

        let store = AtlasStore::open_read_only(&db_path)?;
        require_eq(
            &store.project_root()?,
            &Some(normalize_native_path_display(&root)),
            "read-only checkpointed project root",
        )?;
        store.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn partial_scan_updates_and_absents_paths() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.upsert_scan_nodes(&[test_file_node("src/a.rs", "hash-a2")])?;
        let updated = store
            .load_node_by_path("src/a.rs")?
            .ok_or_else(|| io::Error::other("updated node missing"))?;
        require_eq(
            &updated.node.content_hash,
            &Some("hash-a2".to_string()),
            "partial content hash",
        )?;
        require_eq(
            &store.load_node_by_path("src/b.rs")?.is_some(),
            &true,
            "unrelated node remains indexed",
        )?;
        store.mark_paths_absent(&["src/b.rs".to_string()])?;
        require_eq(
            &store.load_node_by_path("src/b.rs")?.is_none(),
            &true,
            "absent path is no longer returned",
        )?;
        Ok(())
    }

    #[test]
    fn approved_purpose_survives_incremental_file_hash_changes() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash-a")])?;
        store.set_purpose(
            "src/main.rs",
            "Application entry point",
            PurposeSource::Agent,
        )?;
        store.upsert_scan_nodes(&[test_file_node("src/main.rs", "hash-b")])?;

        let node = store
            .load_node_by_path("src/main.rs")?
            .ok_or_else(|| io::Error::other("changed node missing"))?;
        require_eq(
            &node.purpose.status,
            &PurposeStatus::Approved,
            "changed approved file purpose status",
        )?;
        require_eq(
            &node.purpose.agent_reviewed(),
            &true,
            "changed approved file remains agent reviewed",
        )?;
        Ok(())
    }

    #[test]
    fn ranked_nodes_are_loaded_bounded_from_sql() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let mut gradle_task_node = test_file_node("build.gradle.kts", "hash-gradle");
        gradle_task_node.extension = Some(".kts".to_string());
        gradle_task_node.language = Some("kotlin".to_string());
        store.replace_scan(&[
            test_folder_node("src/auth"),
            test_folder_node("src/ui"),
            test_file_node("src/auth/login.rs", "hash-login"),
            test_file_node("src/ui/button.rs", "hash-button"),
            gradle_task_node,
        ])?;
        store.set_purpose(
            "src/auth",
            "Authentication workflow folder",
            PurposeSource::Agent,
        )?;
        store.set_purpose("src/ui", "User interface folder", PurposeSource::Agent)?;
        store.set_node_summary("src/auth/login.rs", "rust source defining login flow")?;

        let folders = store.load_ranked_nodes("authentication", NodeKind::Folder, None, 1, 0)?;
        require_eq(&folders.len(), &1, "bounded folder ranking")?;
        require_eq(
            &folders[0].node.path,
            &"src/auth".to_string(),
            "semantic folder ranking",
        )?;

        let files = store.load_ranked_nodes("login", NodeKind::File, Some("src/auth"), 10, 0)?;
        require_eq(&files.len(), &1, "folder-constrained file ranking")?;
        require_eq(
            &files[0].node.path,
            &"src/auth/login.rs".to_string(),
            "ranked file path",
        )?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "build.gradle.kts".to_string(),
            language: Some("kotlin".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![CodeSymbol {
                path: "build.gradle.kts".to_string(),
                language: Some("kotlin".to_string()),
                name: "bootRunE2E".to_string(),
                kind: SymbolKind::Function,
                signature: "tasks.register<BootRun>(\"bootRunE2E\")".to_string(),
                exported: false,
                documentation: None,
                line_start: 1,
                line_end: 1,
                source_selector: None,
                parent: None,
                parser: ParserKind::TreeSitter,
                detail: Some("gradle-kotlin-dsl-task".to_string()),
            }],
            relations: Vec::new(),
        })?;
        let gradle_files = store.load_ranked_nodes("bootRunE2E", NodeKind::File, None, 10, 0)?;
        require_eq(&gradle_files.len(), &1, "symbol-ranked file count")?;
        require_eq(
            &gradle_files[0].node.path,
            &"build.gradle.kts".to_string(),
            "symbol-ranked file path",
        )?;
        store.clear_symbol_graph_for_path("build.gradle.kts")?;
        let cleared_gradle_files =
            store.load_ranked_nodes("bootRunE2E", NodeKind::File, None, 10, 0)?;
        require_eq(
            &cleared_gradle_files.len(),
            &0,
            "cleared symbol-ranked file count",
        )?;
        Ok(())
    }

    #[test]
    fn ranked_node_admission_preserves_dominant_tiers_before_candidate_cap()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let mut nodes = vec![
            test_file_node("needle", "hash-exact"),
            test_file_node("deep/needle", "hash-name"),
            test_file_node("reviewed.rs", "hash-reviewed"),
        ];
        nodes.extend(
            (0..130).map(|index| test_file_node(&format!("weak/needle-{index:03}.rs"), "hash")),
        );
        store.replace_scan(&nodes)?;
        store.set_purpose(
            "reviewed.rs",
            "Own needle responsibility",
            PurposeSource::Agent,
        )?;
        for index in 0..130 {
            let path = format!("weak/needle-{index:03}.rs");
            store.set_suggested_purpose(&path, "Generated needle suggestion")?;
            store.set_node_summary(&path, "Observed needle summary")?;
        }

        let selected = store.load_ranked_nodes("needle", NodeKind::File, None, 100, 0)?;
        require_eq(&selected.len(), &100, "bounded adversarial candidate count")?;
        require_eq(
            &selected[..3]
                .iter()
                .map(|node| node.node.path.as_str())
                .collect::<Vec<_>>(),
            &vec!["needle", "deep/needle", "reviewed.rs"],
            "pre-cap exact path basename and reviewed-purpose admission",
        )?;
        Ok(())
    }

    #[test]
    fn folder_like_filters_treat_wildcards_as_literal_path_text() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("src/a%b"),
            test_folder_node("src/axb"),
            test_folder_node("src/a_b"),
            test_folder_node("src/acb"),
            test_file_node("src/a%b/target.rs", "hash-percent-target"),
            test_file_node("src/axb/false.rs", "hash-percent-false"),
            test_file_node("src/a_b/target.rs", "hash-underscore-target"),
            test_file_node("src/acb/false.rs", "hash-underscore-false"),
        ])?;
        for path in [
            "src/a%b/target.rs",
            "src/axb/false.rs",
            "src/a_b/target.rs",
            "src/acb/false.rs",
        ] {
            store.set_node_summary(path, "needle indexed summary")?;
        }

        let percent_files =
            store.load_ranked_nodes("needle", NodeKind::File, Some("src/a%b"), 10, 0)?;
        require_eq(&percent_files.len(), &1, "percent folder ranked count")?;
        require_eq(
            &percent_files[0].node.path,
            &"src/a%b/target.rs".to_string(),
            "percent folder ranked path",
        )?;
        require_eq(
            &store.source_file_byte_count(Some("src/a%b"))?,
            &12,
            "percent folder byte count",
        )?;

        let mut visited = Vec::new();
        store.visit_file_token_estimates(Some("src/a_b"), |path, _size| {
            visited.push(path);
            Ok(true)
        })?;
        require_eq(
            &visited,
            &vec!["src/a_b/target.rs".to_string()],
            "underscore folder token paths",
        )?;

        store.mark_paths_absent(&["src/a%b".to_string(), "src/a_b".to_string()])?;
        require_eq(
            &store.load_node_by_path("src/axb/false.rs")?.is_some(),
            &true,
            "percent-like sibling remains indexed",
        )?;
        require_eq(
            &store.load_node_by_path("src/acb/false.rs")?.is_some(),
            &true,
            "underscore-like sibling remains indexed",
        )?;
        require_eq(
            &store.load_node_by_path("src/a%b/target.rs")?.is_none(),
            &true,
            "percent folder target removed",
        )?;
        require_eq(
            &store.load_node_by_path("src/a_b/target.rs")?.is_none(),
            &true,
            "underscore folder target removed",
        )?;
        Ok(())
    }

    #[test]
    fn sql_health_findings_match_resolution_ids() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.set_purpose("src/a.rs", "Shared purpose", PurposeSource::Agent)?;
        store.set_purpose("src/b.rs", "Shared purpose", PurposeSource::Agent)?;

        let findings = store.unresolved_health_findings(&[])?;
        let duplicate = findings
            .iter()
            .find(|finding| finding.category == "duplicate-purpose")
            .ok_or_else(|| io::Error::other("duplicate-purpose finding missing"))?;
        store.resolve_health_finding(&HealthResolution {
            finding_id: duplicate.id.clone(),
            category: duplicate.category.clone(),
            path: duplicate.path.clone(),
            related_path: duplicate.related_path.clone(),
            rationale: "Intentional mirror for test.".to_string(),
        })?;
        let remaining = store.unresolved_health_findings(&store.resolved_health_ids()?)?;
        require_eq(&remaining.is_empty(), &true, "resolved SQL health finding")?;
        Ok(())
    }

    #[test]
    fn unresolved_health_findings_page_filters_and_bounds_rows() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("docs/a.rs", "hash-doc"),
        ])?;
        let query = HealthQuery {
            start_index: 1,
            limit: 1,
            category: Some("missing-purpose".to_string()),
            severity: Some(Severity::Warning),
            path_prefix: Some("src".to_string()),
            summary_only: false,
            scope: HealthScope::all(),
        };

        let page = store.unresolved_health_findings_page(&[], &query)?;
        require_eq(&page.unfiltered_total, &4, "unfiltered health total")?;
        require_eq(&page.total, &2, "filtered health total")?;
        require_eq(&page.returned, &1, "returned health rows")?;
        require_eq(
            &page.findings[0].path,
            &"src/a.rs".to_string(),
            "paged path",
        )?;

        let summary_page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                summary_only: true,
                ..query
            },
        )?;
        require_eq(&summary_page.total, &2, "summary-only total")?;
        require_eq(
            &summary_page.findings.is_empty(),
            &true,
            "summary-only rows",
        )?;
        Ok(())
    }

    #[test]
    fn unresolved_health_findings_page_skips_resolved_lifecycle_rows_before_paging()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
            test_file_node("src/c.rs", "hash-c"),
            test_file_node("src/d.rs", "hash-d"),
        ])?;
        store.resolve_health_finding(&HealthResolution {
            finding_id: finding_id("missing-purpose", "src/b.rs", None),
            category: "missing-purpose".to_string(),
            path: "src/b.rs".to_string(),
            related_path: None,
            rationale: "Resolved for pagination regression.".to_string(),
        })?;

        let page = store.unresolved_health_findings_page_current(&HealthQuery {
            start_index: 0,
            limit: 2,
            category: Some("missing-purpose".to_string()),
            severity: Some(Severity::Warning),
            path_prefix: Some("src".to_string()),
            summary_only: false,
            scope: HealthScope::all(),
        })?;

        require_eq(&page.total, &3, "filtered unresolved missing total")?;
        require_eq(&page.returned, &2, "returned unresolved missing rows")?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec!["src/a.rs", "src/c.rs"],
            "resolved row skipped before limit",
        )?;
        Ok(())
    }

    #[test]
    fn stored_health_resolution_filter_stays_indexed_and_bind_bounded() -> Result<(), Box<dyn Error>>
    {
        const HISTORICAL_RESOLUTIONS: usize = 1_500;

        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir_all(root.join("src"))?;
        let database = temp.path().join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        store.replace_scan(&[test_file_node("src/current.rs", "hash-current")])?;
        {
            let transaction = store.connection.transaction()?;
            {
                let mut statement = transaction.prepare(
                    "
                    INSERT INTO health_resolutions(
                        finding_id, category, path, related_path, rationale, resolved_by
                    )
                    VALUES(?1, 'missing-purpose', ?2, NULL, 'historical', 'agent')
                    ",
                )?;
                for index in 0..HISTORICAL_RESOLUTIONS {
                    let path = format!("removed/{index}.rs");
                    statement.execute(params![finding_id("missing-purpose", &path, None), path])?;
                }
            }
            transaction.commit()?;
        }

        drop(store);
        let store = AtlasStore::open_read_only_for_project(&database, &root)?;
        let page = store.unresolved_health_findings_page_current(&HealthQuery {
            start_index: 0,
            limit: 2,
            category: Some("missing-purpose".to_string()),
            severity: Some(Severity::Warning),
            path_prefix: Some("src".to_string()),
            summary_only: false,
            scope: HealthScope::all(),
        })?;
        require_eq(&page.total, &1, "current unresolved total")?;
        require_eq(
            &page.findings[0].path,
            &"src/current.rs".to_string(),
            "current unresolved path",
        )?;

        let (where_clause, values) = purpose_status_where_clause(
            PURPOSE_HEALTH_SPECS[0],
            None,
            HealthResolutionFilter::Stored,
            HealthScope::all(),
        );
        require_eq(&values.len(), &1, "stored filter bind count")?;
        let plan_sql = format!(
            "EXPLAIN QUERY PLAN
             SELECT COUNT(*)
             FROM nodes n
             JOIN purposes p ON p.node_id = n.id
             WHERE {where_clause}"
        );
        let mut statement = store.connection.prepare(&plan_sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| row.get::<_, String>(3))?;
        let mut plan = Vec::new();
        for row in rows {
            plan.push(row?);
        }
        if !plan.iter().any(|detail| {
            detail.contains("health_resolutions")
                && detail.contains("finding_id")
                && detail.contains("SEARCH")
        }) {
            return Err(io::Error::other(format!(
                "stored resolution anti-lookup did not use the finding-id index: {plan:?}"
            ))
            .into());
        }
        store.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn unresolved_health_findings_page_streams_duplicate_and_temp_rows()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("tmp"),
            test_folder_node("src"),
            test_folder_node("src/tmp"),
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.set_purpose(".", "Repository root", PurposeSource::Agent)?;
        store.set_purpose("src", "Source folder", PurposeSource::Agent)?;
        store.set_purpose("tmp", "Temporary output", PurposeSource::Agent)?;
        store.set_purpose("src/tmp", "Source temporary output", PurposeSource::Agent)?;
        store.set_purpose("src/a.rs", "Shared implementation", PurposeSource::Agent)?;
        store.set_purpose("src/b.rs", "Shared implementation", PurposeSource::Agent)?;

        let duplicate_page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 1,
                category: Some("duplicate-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some("src".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        require_eq(&duplicate_page.total, &1, "duplicate total")?;
        require_eq(&duplicate_page.returned, &1, "duplicate returned")?;
        require_eq(
            &duplicate_page.findings[0].category,
            &"duplicate-purpose".to_string(),
            "duplicate category",
        )?;

        let temp_page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 1,
                category: Some("repeated-temporary-folder".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        require_eq(&temp_page.total, &1, "temp total")?;
        require_eq(&temp_page.returned, &1, "temp returned")?;
        require_eq(
            &temp_page.findings[0].category,
            &"repeated-temporary-folder".to_string(),
            "temp category",
        )?;
        Ok(())
    }

    #[test]
    fn unresolved_health_findings_page_source_only_filters_asset_noise()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let asset_file = Node {
            path: "assets/logo.png".to_string(),
            kind: NodeKind::File,
            parent_path: Some("assets".to_string()),
            extension: Some(".png".to_string()),
            language: None,
            size_bytes: Some(42),
            mtime_ns: Some(10),
            content_hash: Some("hash-logo".to_string()),
        };
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/main.rs", "hash-main"),
            test_folder_node("assets"),
            asset_file,
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::source_only(),
            },
        )?;

        require_eq(&page.unfiltered_total, &5, "all unresolved rows")?;
        require_eq(&page.total, &3, "source-only missing total")?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec![".", "src", "src/main.rs"],
            "source-only paths",
        )?;
        Ok(())
    }

    #[test]
    fn high_impact_purpose_queue_filters_low_priority_files() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/main.rs", "hash-main"),
            test_file_node("src/helper.rs", "hash-helper"),
            test_file_node("build.gradle.kts", "hash-gradle"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(&page.unfiltered_total, &5, "all missing rows")?;
        require_eq(&page.total, &4, "default actionable rows")?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec![".", "src", "build.gradle.kts", "src/main.rs"],
            "folder-first high-impact queue paths",
        )?;

        let broad_page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        require_eq(&broad_page.total, &5, "explicit broad queue rows")?;
        Ok(())
    }

    #[test]
    fn high_impact_purpose_queue_keeps_asset_only_folders_without_asset_files()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let asset_file = Node {
            path: "assets/logo.svg".to_string(),
            kind: NodeKind::File,
            parent_path: Some("assets".to_string()),
            extension: Some(".svg".to_string()),
            language: None,
            size_bytes: Some(42),
            mtime_ns: Some(10),
            content_hash: Some("hash-logo".to_string()),
        };
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("assets"),
            test_folder_node("src"),
            asset_file,
            test_file_node("src/helper.rs", "hash-helper"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(&page.unfiltered_total, &5, "all missing rows")?;
        require_eq(&page.total, &3, "all folders without asset files")?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec![".", "assets", "src"],
            "folder-first default queue keeps asset-only folders",
        )?;
        Ok(())
    }

    #[test]
    fn high_impact_purpose_queue_pages_folders_before_files() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("Cargo.toml", "hash-cargo"),
            test_file_node("package.json", "hash-package"),
            test_file_node("pyproject.toml", "hash-python"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 2,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(&page.total, &5, "default actionable total")?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec![".", "src"],
            "small page keeps folders first",
        )?;
        Ok(())
    }

    #[test]
    fn high_impact_purpose_queue_omits_low_value_stale_files() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/helper.rs", "hash-a"),
            test_file_node("Cargo.toml", "hash-cargo"),
            test_file_node("package.json", "hash-package"),
        ])?;
        store.set_purpose(
            "src/helper.rs",
            "Reviewed helper implementation.",
            PurposeSource::Agent,
        )?;
        store.set_purpose(
            "Cargo.toml",
            "Reviewed Cargo manifest.",
            PurposeSource::Agent,
        )?;
        store.connection.execute(
            "UPDATE purposes
                SET status = 'stale'
              WHERE node_id IN (
                  SELECT id FROM nodes WHERE path IN ('src/helper.rs', 'Cargo.toml')
              )",
            [],
        )?;
        store.replace_scan(&[
            test_file_node("src/helper.rs", "hash-b"),
            test_file_node("Cargo.toml", "hash-cargo-new"),
            test_file_node("package.json", "hash-package"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 1,
                category: None,
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(&page.total, &2, "default actionable total")?;
        require_eq(&page.returned, &1, "small page returned")?;
        require_eq(
            &page.findings[0].category,
            &"stale-purpose".to_string(),
            "stale high-impact file is still prioritized",
        )?;
        require_eq(
            &page.findings[0].path,
            &"Cargo.toml".to_string(),
            "stale high-impact file path",
        )?;

        let broad_page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some(CATEGORY_STALE_PURPOSE.to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_with_source_files(),
            },
        )?;
        require_eq(&broad_page.total, &2, "broad source stale rows")?;
        require_eq(
            &health_paths(&broad_page),
            &vec!["Cargo.toml", "src/helper.rs"],
            "broad scope includes low-value stale source files",
        )?;
        Ok(())
    }

    #[test]
    fn include_assets_queue_includes_asset_files_not_low_priority_source()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let asset_file = Node {
            path: "assets/logo.svg".to_string(),
            kind: NodeKind::File,
            parent_path: Some("assets".to_string()),
            extension: Some(".svg".to_string()),
            language: None,
            size_bytes: Some(42),
            mtime_ns: Some(10),
            content_hash: Some("hash-logo".to_string()),
        };
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("assets"),
            test_folder_node("src"),
            asset_file,
            test_file_node("src/helper.rs", "hash-helper"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("missing-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_with_assets(),
            },
        )?;

        require_eq(&page.unfiltered_total, &5, "all missing rows")?;
        require_eq(
            &page.total,
            &4,
            "assets included without broad source cleanup",
        )?;
        require_eq(
            &page
                .findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            &vec![".", "assets", "src", "assets/logo.svg"],
            "asset files included and low-priority source omitted",
        )?;
        Ok(())
    }

    #[test]
    fn legacy_human_stale_files_remain_in_default_queue() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/helper.rs", "hash-a")])?;
        store.set_purpose(
            "src/helper.rs",
            "Legacy reviewed helper implementation.",
            PurposeSource::Agent,
        )?;
        store.connection.execute(
            "
            UPDATE purposes
            SET source = 'human', status = 'stale'
            WHERE node_id = (SELECT id FROM nodes WHERE path = 'src/helper.rs')
            ",
            [],
        )?;
        store.replace_scan(&[test_file_node("src/helper.rs", "hash-b")])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("stale-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(&page.total, &1, "legacy reviewed stale row total")?;
        require_eq(
            &page.findings[0].path,
            &"src/helper.rs".to_string(),
            "legacy reviewed stale file",
        )?;
        Ok(())
    }

    #[test]
    fn stale_imported_files_remain_in_default_queue() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/imported.rs", "hash-a"),
            test_file_node("Cargo.toml", "hash-cargo"),
        ])?;
        store.set_purpose(
            "src/imported.rs",
            "Imported helper implementation.",
            PurposeSource::Imported,
        )?;
        store.connection.execute(
            "UPDATE purposes
                SET status = 'stale'
              WHERE node_id = (SELECT id FROM nodes WHERE path = 'src/imported.rs')",
            [],
        )?;
        store.replace_scan(&[
            test_file_node("src/imported.rs", "hash-b"),
            test_file_node("Cargo.toml", "hash-cargo"),
        ])?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: None,
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;

        require_eq(
            &page.total,
            &2,
            "default queue includes stale imported file",
        )?;
        require_eq(
            &health_paths(&page),
            &vec!["src/imported.rs", "Cargo.toml"],
            "stale imported file is queued before high-impact files",
        )?;
        require_eq(
            &page.findings[0].category,
            &"stale-purpose".to_string(),
            "stale imported finding category",
        )?;
        Ok(())
    }

    #[test]
    fn duplicate_purpose_health_is_contextual_for_folders() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("customers"),
            test_folder_node("customers/service"),
            test_folder_node("settings"),
            test_folder_node("settings/service"),
        ])?;
        store.set_purpose("customers/service", "Service layer", PurposeSource::Agent)?;
        store.set_purpose("settings/service", "Service layer", PurposeSource::Agent)?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("duplicate-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;

        require_eq(&page.total, &0, "folder duplicates scoped by parent")?;
        Ok(())
    }

    #[test]
    fn contextual_folder_duplicate_identity_matches_unpaged_health_and_resolution()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("customers"),
            test_folder_node("customers/service"),
            test_folder_node("settings"),
            test_folder_node("settings/service"),
            test_folder_node("settings/worker"),
        ])?;
        store.set_purpose("customers/service", "Service layer", PurposeSource::Agent)?;
        store.set_purpose("settings/service", "Service layer", PurposeSource::Agent)?;
        store.set_purpose("settings/worker", "Service layer", PurposeSource::Agent)?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 10,
                category: Some("duplicate-purpose".to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        require_eq(&page.total, &1, "contextual duplicate total")?;
        let paged_finding = page
            .findings
            .first()
            .ok_or_else(|| io::Error::other("paged duplicate missing"))?;
        require_eq(
            &paged_finding.path,
            &"settings/worker".to_string(),
            "paged duplicate path",
        )?;
        require_eq(
            &paged_finding.related_path,
            &Some("settings/service".to_string()),
            "paged related path",
        )?;

        let unpaged_duplicates = store
            .unresolved_health_findings(&[])?
            .into_iter()
            .filter(|finding| finding.category == "duplicate-purpose")
            .collect::<Vec<_>>();
        require_eq(
            &unpaged_duplicates,
            &page.findings,
            "unpaged duplicate identity",
        )?;

        store.resolve_health_finding(&HealthResolution {
            finding_id: paged_finding.id.clone(),
            category: paged_finding.category.clone(),
            path: paged_finding.path.clone(),
            related_path: paged_finding.related_path.clone(),
            rationale: "Settings service and worker intentionally share a layer purpose."
                .to_string(),
        })?;
        let has_remaining_duplicate = store
            .unresolved_health_findings(&store.resolved_health_ids()?)?
            .into_iter()
            .any(|finding| finding.category == "duplicate-purpose");
        require_eq(
            &has_remaining_duplicate,
            &false,
            "resolved contextual duplicate",
        )?;
        Ok(())
    }

    #[test]
    fn agent_review_required_scope_expands_from_low_to_strict() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let asset_file = Node {
            path: "assets/logo.svg".to_string(),
            kind: NodeKind::File,
            parent_path: Some("assets".to_string()),
            extension: Some(".svg".to_string()),
            language: None,
            size_bytes: Some(42),
            mtime_ns: Some(10),
            content_hash: Some("hash-logo".to_string()),
        };
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("assets"),
            test_folder_node("src"),
            test_file_node("Cargo.toml", "hash-cargo"),
            test_file_node("src/detail.rs", "hash-detail"),
            asset_file,
        ])?;
        for (path, purpose) in [
            (".", "Imported repository root"),
            ("assets", "Imported asset folder"),
            ("src", "Imported Rust source folder"),
            ("Cargo.toml", "Imported Rust manifest"),
            ("src/detail.rs", "Imported implementation detail"),
            ("assets/logo.svg", "Imported SVG brand asset"),
        ] {
            store.set_purpose(path, purpose, PurposeSource::Imported)?;
        }

        let low = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 20,
                category: Some(CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED.to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::purpose_default(),
            },
        )?;
        require_eq(
            &health_paths(&low),
            &vec![".", "assets", "src", "Cargo.toml"],
            "low purpose review scope",
        )?;
        require_eq(
            &low.unfiltered_total,
            &6,
            "agent-review findings are counted once in unfiltered total",
        )?;

        let asset_scope = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                scope: HealthScope::purpose_with_assets(),
                ..low_query()
            },
        )?;
        require_eq(
            &health_paths(&asset_scope),
            &vec![".", "assets", "src", "Cargo.toml", "assets/logo.svg"],
            "asset purpose review scope",
        )?;

        let medium = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                scope: HealthScope::purpose_with_source_files(),
                ..low_query()
            },
        )?;
        require_eq(
            &health_paths(&medium),
            &vec![".", "assets", "src", "Cargo.toml", "src/detail.rs"],
            "medium purpose review scope",
        )?;

        let strict = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                scope: HealthScope::purpose_strict(),
                ..low_query()
            },
        )?;
        require_eq(
            &health_paths(&strict),
            &vec![
                ".",
                "assets",
                "src",
                "Cargo.toml",
                "assets/logo.svg",
                "src/detail.rs",
            ],
            "strict purpose review scope",
        )?;

        let all = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                scope: HealthScope::all(),
                ..low_query()
            },
        )?;
        require_eq(
            &health_paths(&all),
            &health_paths(&strict),
            "all health scope should include every purpose review candidate",
        )?;
        Ok(())
    }

    #[test]
    fn replace_scan_preserves_curated_purposes_and_reconciles_changed_paths()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/main.rs", "hash-a"),
        ])?;
        store.set_purpose(".", "Agent-reviewed repository root", PurposeSource::Agent)?;
        store.set_purpose(
            "src",
            "Agent-reviewed Rust source folder",
            PurposeSource::Agent,
        )?;
        store.set_purpose(
            "src/main.rs",
            "Agent-reviewed Rust entry point",
            PurposeSource::Agent,
        )?;

        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/main.rs", "hash-a"),
            test_file_node("src/new.rs", "hash-new"),
        ])?;
        let nodes = store.load_nodes_by_paths(&[
            ".".to_string(),
            "src".to_string(),
            "src/main.rs".to_string(),
            "src/new.rs".to_string(),
        ])?;
        let by_path = nodes
            .iter()
            .map(|node| (node.node.path.as_str(), node))
            .collect::<HashMap<_, _>>();
        require_eq(
            &by_path["src/main.rs"].purpose.purpose,
            &Some("Agent-reviewed Rust entry point".to_string()),
            "unchanged file purpose preserved",
        )?;
        require_eq(
            &by_path["src/main.rs"].purpose.status,
            &PurposeStatus::Approved,
            "unchanged file purpose stays approved",
        )?;
        require_eq(
            &by_path["src/new.rs"].purpose.status,
            &PurposeStatus::Missing,
            "new file starts missing",
        )?;

        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/main.rs", "hash-b"),
            test_file_node("src/new.rs", "hash-new"),
        ])?;
        let changed = store
            .load_nodes_by_paths(&["src/main.rs".to_string()])?
            .pop()
            .ok_or_else(|| io::Error::other("changed node missing"))?;
        require_eq(
            &changed.purpose.purpose,
            &Some("Agent-reviewed Rust entry point".to_string()),
            "changed file purpose text preserved",
        )?;
        require_eq(
            &changed.purpose.status,
            &PurposeStatus::Approved,
            "changed file purpose stays approved",
        )?;

        store.replace_scan(&[test_folder_node("."), test_folder_node("src")])?;
        let removed = store.load_nodes_by_paths(&["src/main.rs".to_string()])?;
        require_eq(&removed.is_empty(), &true, "removed file is inactive")?;

        let dormant = store.connection.query_row(
            "SELECT n.exists_now, p.purpose, p.status
               FROM nodes AS n
               JOIN purposes AS p ON p.node_id = n.id
              WHERE n.path = 'src/main.rs'",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )?;
        require_eq(
            &dormant,
            &(
                0,
                Some("Agent-reviewed Rust entry point".to_string()),
                PurposeStatus::Approved.as_str().to_string(),
            ),
            "removed file keeps a dormant approved purpose",
        )?;

        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/renamed.rs", "hash-b"),
        ])?;
        let renamed = store
            .load_node_by_path("src/renamed.rs")?
            .ok_or_else(|| io::Error::other("renamed path missing"))?;
        require_eq(
            &renamed.purpose.status,
            &PurposeStatus::Missing,
            "rename does not transfer approval",
        )?;

        store.replace_scan(&[
            test_folder_node("."),
            test_folder_node("src"),
            test_file_node("src/main.rs", "hash-c"),
        ])?;
        let reactivated = store
            .load_node_by_path("src/main.rs")?
            .ok_or_else(|| io::Error::other("reactivated path missing"))?;
        require_eq(
            &reactivated.purpose.purpose,
            &Some("Agent-reviewed Rust entry point".to_string()),
            "exact-path reactivation restores the dormant purpose",
        )?;
        require_eq(
            &reactivated.purpose.status,
            &PurposeStatus::Approved,
            "exact-path reactivation restores approval",
        )?;
        Ok(())
    }

    #[test]
    fn file_token_estimates_are_visited_without_loading_nodes() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("tests/b.rs", "hash-b"),
        ])?;

        let mut visited = Vec::new();
        store.visit_file_token_estimates(Some("src"), |path, size_bytes| {
            visited.push((path, size_bytes));
            Ok(true)
        })?;
        require_eq(
            &visited,
            &vec![("src/a.rs".to_string(), Some(12))],
            "folder-scoped token estimate rows",
        )?;
        Ok(())
    }

    #[test]
    fn indexed_file_text_replaces_and_clears_stale_rows() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash-a")])?;
        store.replace_file_texts_for_paths(
            &["src/main.rs".to_string()],
            &[IndexedFileText {
                path: "src/main.rs".to_string(),
                content_hash: Some("hash-a".to_string()),
                byte_count: "needle old\n".len(),
                line_count: 1,
                content: "needle old\n".to_string(),
            }],
        )?;
        let texts = store.load_file_texts_for_search(Some("needle"), true)?;
        require_eq(&texts.len(), &1, "indexed text row count")?;

        store.replace_file_texts_for_paths(&["src/main.rs".to_string()], &[])?;
        let missing = store.load_file_text("src/main.rs")?;
        require_eq(&missing.is_none(), &true, "cleared stale indexed text")?;
        Ok(())
    }

    #[test]
    fn indexed_file_text_search_can_stop_without_collecting_all_rows() -> Result<(), Box<dyn Error>>
    {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.replace_file_texts_for_paths(
            &["src/a.rs".to_string(), "src/b.rs".to_string()],
            &[
                IndexedFileText {
                    path: "src/a.rs".to_string(),
                    content_hash: Some("hash-a".to_string()),
                    byte_count: "needle first\n".len(),
                    line_count: 1,
                    content: "needle first\n".to_string(),
                },
                IndexedFileText {
                    path: "src/b.rs".to_string(),
                    content_hash: Some("hash-b".to_string()),
                    byte_count: "needle second\n".len(),
                    line_count: 1,
                    content: "needle second\n".to_string(),
                },
            ],
        )?;

        let mut visited = Vec::new();
        store.visit_file_texts_for_search(Some("needle"), true, |text| {
            visited.push(text.path);
            Ok(false)
        })?;
        require_eq(&visited, &vec!["src/a.rs".to_string()], "early stop rows")?;
        Ok(())
    }

    #[test]
    fn file_text_fts_candidates_are_bounded_scoped_safe_and_metadata_only()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
            test_file_node("tests/c.rs", "hash-c"),
        ])?;
        store.replace_file_texts_for_paths(
            &[
                "src/a.rs".to_string(),
                "src/b.rs".to_string(),
                "tests/c.rs".to_string(),
            ],
            &[
                IndexedFileText {
                    path: "src/a.rs".to_string(),
                    content_hash: Some("hash-a".to_string()),
                    byte_count: 13,
                    line_count: 1,
                    content: "needle alpha\n".to_string(),
                },
                IndexedFileText {
                    path: "src/b.rs".to_string(),
                    content_hash: Some("hash-b".to_string()),
                    byte_count: 19,
                    line_count: 1,
                    content: "prefixneedlesuffix\n".to_string(),
                },
                IndexedFileText {
                    path: "tests/c.rs".to_string(),
                    content_hash: Some("hash-c".to_string()),
                    byte_count: 32,
                    line_count: 1,
                    content: "needle gamma AND NOT NEAR terms\n".to_string(),
                },
            ],
        )?;

        let bounded = store.query_file_text_fts_candidates(
            &FileTextFtsQuery {
                literal_token: "needle",
                path_prefix: None,
                limit: 1,
            },
            None,
        )?;
        require_eq(&bounded.candidates.len(), &1, "bounded FTS row count")?;
        require_eq(&bounded.overflow, &true, "bounded FTS overflow")?;
        require(
            bounded.candidates[0].bm25.is_finite(),
            "FTS rank was non-finite",
        )?;
        let zero_limit = store.query_file_text_fts_candidates(
            &FileTextFtsQuery {
                literal_token: "needle",
                path_prefix: None,
                limit: 0,
            },
            None,
        )?;
        require_eq(
            &zero_limit,
            &FileTextFtsPage {
                candidates: Vec::new(),
                overflow: true,
            },
            "zero-limit FTS overflow",
        )?;

        let scoped = store.query_file_text_fts_candidates(
            &FileTextFtsQuery {
                literal_token: "needle",
                path_prefix: Some("src/"),
                limit: 10,
            },
            None,
        )?;
        require_eq(
            &scoped
                .candidates
                .iter()
                .map(|candidate| candidate.path.as_str())
                .collect::<Vec<_>>(),
            &vec!["src/a.rs", "src/b.rs"],
            "path-scoped FTS rank order",
        )?;
        let exact_path = store.query_file_text_fts_candidates(
            &FileTextFtsQuery {
                literal_token: "needle",
                path_prefix: Some("src/a.rs"),
                limit: 10,
            },
            None,
        )?;
        require_eq(
            &exact_path.candidates[0].path,
            &"src/a.rs".to_string(),
            "exact-path FTS scope",
        )?;
        for reserved_token in ["AND", "NOT", "NEAR"] {
            let reserved = store.query_file_text_fts_candidates(
                &FileTextFtsQuery {
                    literal_token: reserved_token,
                    path_prefix: Some("tests/c.rs"),
                    limit: 10,
                },
                None,
            )?;
            require_eq(
                &reserved.candidates.len(),
                &1,
                "quoted FTS reserved-token candidate",
            )?;
        }
        let mut plan_statement = store.connection.prepare(
            "EXPLAIN QUERY PLAN
             SELECT f.path, f.content_hash, f.byte_count, f.line_count, bm25(file_text_fts)
             FROM file_text_fts
             JOIN file_texts AS f ON f.rowid = file_text_fts.rowid
             WHERE file_text_fts MATCH ?1
             ORDER BY bm25(file_text_fts), f.path
             LIMIT ?2",
        )?;
        let plan_details = plan_statement
            .query_map(params!["needle", 11], |row| row.get::<_, String>(3))?
            .collect::<Result<Vec<_>, _>>()?;
        require(
            plan_details
                .iter()
                .any(|detail| detail.contains("VIRTUAL TABLE INDEX")),
            "FTS candidate query did not use the virtual-table index",
        )?;
        require(
            plan_details
                .iter()
                .any(|detail| detail.contains("INTEGER PRIMARY KEY")),
            "FTS candidate query did not hydrate metadata by file_texts rowid",
        )?;

        for unsafe_token in ["ab", "foo_bar", "needle\" OR content:*", "néedle"] {
            let result = store.query_file_text_fts_candidates(
                &FileTextFtsQuery {
                    literal_token: unsafe_token,
                    path_prefix: None,
                    limit: 10,
                },
                None,
            );
            require(
                matches!(result, Err(DbError::FileTextFtsTokenUnsafe { .. })),
                "unsafe FTS token was accepted",
            )?;
        }
        let over_cap = store.query_file_text_fts_candidates(
            &FileTextFtsQuery {
                literal_token: "needle",
                path_prefix: None,
                limit: MAX_FILE_TEXT_FTS_CANDIDATES + 1,
            },
            None,
        );
        require(
            matches!(over_cap, Err(DbError::FileTextFtsCandidateLimit { .. })),
            "over-cap FTS request was accepted",
        )?;

        store.connection.execute(
            "UPDATE file_texts SET content = x'80' WHERE path = 'src/a.rs'",
            [],
        )?;
        let metadata_only = store.query_file_text_fts_candidates(
            &FileTextFtsQuery {
                literal_token: "needle",
                path_prefix: Some("src/a.rs"),
                limit: 1,
            },
            None,
        )?;
        require_eq(
            &metadata_only.candidates[0].byte_count,
            &13,
            "metadata-only candidate byte count",
        )?;
        require(
            store.load_file_text("src/a.rs").is_err(),
            "content corruption fixture did not prove candidate pre-hydration",
        )?;
        Ok(())
    }

    #[test]
    fn file_text_fts_mutations_are_atomic_and_state_reports_identity_drift()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash-a")])?;
        let invalid_metadata = IndexedFileText {
            path: "src/main.rs".to_string(),
            content_hash: Some("hash-invalid".to_string()),
            byte_count: 0,
            line_count: 1,
            content: "not empty\n".to_string(),
        };
        require(
            matches!(
                store.replace_file_texts_for_paths(
                    std::slice::from_ref(&invalid_metadata.path),
                    std::slice::from_ref(&invalid_metadata),
                ),
                Err(DbError::FileTextMetadataMismatch {
                    field: "byte_count",
                    ..
                })
            ),
            "invalid file-text byte metadata was accepted",
        )?;
        require_eq(
            &store.load_file_text(&invalid_metadata.path)?,
            &None,
            "invalid metadata write rollback",
        )?;
        let invalid_line_count = IndexedFileText {
            byte_count: invalid_metadata.content.len(),
            line_count: 0,
            ..invalid_metadata.clone()
        };
        require(
            matches!(
                store.replace_file_texts_for_paths(
                    std::slice::from_ref(&invalid_line_count.path),
                    std::slice::from_ref(&invalid_line_count),
                ),
                Err(DbError::FileTextMetadataMismatch {
                    field: "line_count",
                    ..
                })
            ),
            "invalid file-text line metadata was accepted",
        )?;
        let original = IndexedFileText {
            path: "src/main.rs".to_string(),
            content_hash: Some("hash-a".to_string()),
            byte_count: "needle old\n".len(),
            line_count: 1,
            content: "needle old\n".to_string(),
        };
        store.replace_file_texts_for_paths(
            std::slice::from_ref(&original.path),
            std::slice::from_ref(&original),
        )?;
        require_eq(
            &store.file_text_fts_state()?,
            &FileTextFtsState {
                source_rows: 1,
                indexed_rows: 1,
                synchronized: true,
            },
            "initial FTS synchronization state",
        )?;

        store.connection.execute_batch(
            "CREATE TRIGGER fail_file_text_insert
             BEFORE INSERT ON file_texts
             BEGIN
                 SELECT RAISE(ABORT, 'injected file-text failure');
             END;",
        )?;
        let replacement = IndexedFileText {
            path: original.path.clone(),
            content_hash: Some("hash-b".to_string()),
            byte_count: 11,
            line_count: 1,
            content: "beacon new\n".to_string(),
        };
        require(
            store
                .replace_file_texts_for_paths(
                    std::slice::from_ref(&replacement.path),
                    std::slice::from_ref(&replacement),
                )
                .is_err(),
            "fault-injected replacement unexpectedly committed",
        )?;
        store
            .connection
            .execute_batch("DROP TRIGGER fail_file_text_insert")?;
        require_eq(
            &store.load_file_text(&original.path)?,
            &Some(original.clone()),
            "rolled-back authoritative text",
        )?;
        require_eq(
            &store
                .query_file_text_fts_candidates(
                    &FileTextFtsQuery {
                        literal_token: "needle",
                        path_prefix: None,
                        limit: 10,
                    },
                    None,
                )?
                .candidates
                .len(),
            &1,
            "rolled-back FTS document",
        )?;
        require_eq(
            &store.file_text_fts_state()?.synchronized,
            &true,
            "rollback FTS synchronization",
        )?;

        store.replace_file_texts_for_paths(
            std::slice::from_ref(&replacement.path),
            std::slice::from_ref(&replacement),
        )?;
        require_eq(
            &store
                .query_file_text_fts_candidates(
                    &FileTextFtsQuery {
                        literal_token: "needle",
                        path_prefix: None,
                        limit: 10,
                    },
                    None,
                )?
                .candidates
                .len(),
            &0,
            "replaced token removed from FTS",
        )?;
        require_eq(
            &store
                .query_file_text_fts_candidates(
                    &FileTextFtsQuery {
                        literal_token: "beacon",
                        path_prefix: None,
                        limit: 10,
                    },
                    None,
                )?
                .candidates
                .len(),
            &1,
            "replacement token added to FTS",
        )?;

        store.connection.execute(
            "INSERT INTO file_text_fts(file_text_fts, rowid, content)
             SELECT 'delete', rowid, content FROM file_texts WHERE path = ?1",
            [&replacement.path],
        )?;
        require_eq(
            &store.file_text_fts_state()?,
            &FileTextFtsState {
                source_rows: 1,
                indexed_rows: 0,
                synchronized: false,
            },
            "desynchronized FTS identity state",
        )?;
        store.connection.execute(
            "INSERT INTO file_text_fts(file_text_fts) VALUES('rebuild')",
            [],
        )?;
        let (source_revision, projection_revision) =
            load_file_text_fts_revisions(&store.connection)?.ok_or_else(|| {
                io::Error::other("FTS revisions disappeared after synchronized writes")
            })?;
        require_eq(
            &source_revision,
            &projection_revision,
            "synchronized FTS revisions",
        )?;
        set_metadata(
            &store.connection,
            FILE_TEXT_FTS_PROJECTION_REVISION_KEY,
            &projection_revision.saturating_sub(1).to_string(),
        )?;
        require_eq(
            &store.file_text_fts_ready()?,
            &false,
            "incomplete FTS revision readiness",
        )?;
        set_metadata(
            &store.connection,
            FILE_TEXT_FTS_PROJECTION_REVISION_KEY,
            &source_revision.to_string(),
        )?;
        require_eq(
            &store.file_text_fts_ready()?,
            &true,
            "restored FTS revision readiness",
        )?;
        store.mark_paths_absent(&["src".to_string()])?;
        require_eq(
            &store.file_text_fts_state()?,
            &FileTextFtsState {
                source_rows: 0,
                indexed_rows: 0,
                synchronized: true,
            },
            "absent-path FTS synchronization",
        )?;

        store.replace_scan(&[test_file_node("src/main.rs", "hash-c")])?;
        let rescanned = IndexedFileText {
            path: "src/main.rs".to_string(),
            content_hash: Some("hash-c".to_string()),
            byte_count: 13,
            line_count: 1,
            content: "rescan token\n".to_string(),
        };
        store.replace_file_texts_for_paths(
            std::slice::from_ref(&rescanned.path),
            std::slice::from_ref(&rescanned),
        )?;
        store.replace_scan(&[])?;
        require_eq(
            &store.file_text_fts_state()?,
            &FileTextFtsState {
                source_rows: 0,
                indexed_rows: 0,
                synchronized: true,
            },
            "full-scan deletion FTS synchronization",
        )?;
        Ok(())
    }

    #[test]
    fn file_text_fts_migration_backfills_existing_text_and_survives_reopen()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("project");
        fs::create_dir(&root)?;
        let database = temp.path().join("atlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash-a")])?;
        store.replace_file_texts_for_paths(
            &["src/main.rs".to_string()],
            &[IndexedFileText {
                path: "src/main.rs".to_string(),
                content_hash: Some("hash-a".to_string()),
                byte_count: 17,
                line_count: 1,
                content: "migration needle\n".to_string(),
            }],
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
        crate::schema::recreate_disposable_graph_projection(&store.connection, false)?;
        crate::schema::recreate_pre_selector_symbol_storage_for_test(&store.connection)?;
        store.connection.execute_batch(
            "DROP INDEX idx_symbol_import_alias_lookup;
             DROP TABLE file_text_fts;",
        )?;
        store.connection.execute(
            "DELETE FROM metadata WHERE key IN (?1, ?2)",
            params![
                FILE_TEXT_FTS_SOURCE_REVISION_KEY,
                FILE_TEXT_FTS_PROJECTION_REVISION_KEY,
            ],
        )?;
        set_metadata(
            &store.connection,
            SCHEMA_VERSION_KEY,
            &crate::schema::COVERAGE_DISCOVERY_SCHEMA_VERSION.to_string(),
        )?;
        drop(store);

        let migrated = AtlasStore::open_for_project(&database, &root)?;
        require_eq(
            &migrated.file_text_fts_state()?,
            &FileTextFtsState {
                source_rows: 1,
                indexed_rows: 1,
                synchronized: true,
            },
            "migrated FTS synchronization state",
        )?;
        require_eq(
            &migrated
                .query_file_text_fts_candidates(
                    &FileTextFtsQuery {
                        literal_token: "needle",
                        path_prefix: None,
                        limit: 10,
                    },
                    None,
                )?
                .candidates[0]
                .path,
            &"src/main.rs".to_string(),
            "migrated FTS backfill candidate",
        )?;
        drop(migrated);

        let reopened = AtlasStore::open_read_only_for_project(&database, &root)?;
        require_eq(
            &reopened.file_text_fts_state()?.synchronized,
            &true,
            "reopened FTS synchronization",
        )?;
        require_eq(
            &reopened
                .query_file_text_fts_candidates(
                    &FileTextFtsQuery {
                        literal_token: "migration",
                        path_prefix: Some("src"),
                        limit: 1,
                    },
                    None,
                )?
                .candidates
                .len(),
            &1,
            "reopened FTS candidate count",
        )?;
        Ok(())
    }

    #[test]
    fn sqlite_read_progress_interrupts_active_repository_traversal_and_clears_handler()
    -> Result<(), Box<dyn Error>> {
        let store = AtlasStore::in_memory()?;
        let control = IndexWorkControl::with_deadline(
            projectatlas_core::IndexCancellation::new(),
            Instant::now() + std::time::Duration::from_millis(50),
        );
        let interrupted = with_sqlite_read_progress(
            &store.connection,
            Some(&control),
            IndexWorkStage::RepositoryTraversal,
            || {
                store
                    .connection
                    .query_row(
                        "WITH RECURSIVE numbers(value) AS (
                             VALUES(1)
                             UNION ALL
                             SELECT value + 1 FROM numbers WHERE value < 100000000
                         )
                         SELECT SUM(value) FROM numbers",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(DbError::from)
            },
        );
        require(
            matches!(
                interrupted,
                Err(DbError::IndexWork(IndexWorkFailure::DeadlineExceeded {
                    stage: IndexWorkStage::RepositoryTraversal
                }))
            ),
            "active SQLite traversal was not interrupted with its typed deadline",
        )?;
        require_eq(
            &store
                .connection
                .query_row("SELECT 1", [], |row| row.get::<_, i64>(0))?,
            &1,
            "cleared repository traversal progress handler",
        )?;
        store
            .connection
            .execute("CREATE TEMP TABLE follow_up(value INTEGER NOT NULL)", [])?;
        store
            .connection
            .execute("INSERT INTO follow_up(value) VALUES(1)", [])?;
        Ok(())
    }

    #[cfg(feature = "sqlite-progress-test-observer")]
    #[test]
    fn sqlite_read_progress_propagates_cancellation_during_an_active_batch()
    -> Result<(), Box<dyn Error>> {
        use crate::sqlite_progress_test_observer::{
            SqliteReadProgressEvent, observe_sqlite_read_progress,
        };
        use std::cell::Cell;
        use std::rc::Rc;

        let store = AtlasStore::in_memory()?;
        let cancellation = projectatlas_core::IndexCancellation::new();
        let control = IndexWorkControl::new(cancellation.clone(), None);
        let callback_seen = Rc::new(Cell::new(false));
        let cancelled = observe_sqlite_read_progress(
            {
                let callback_seen = Rc::clone(&callback_seen);
                move |event| {
                    if matches!(
                        event,
                        SqliteReadProgressEvent::CallbackEntered {
                            stage: IndexWorkStage::RepositoryTraversal
                        }
                    ) {
                        callback_seen.set(true);
                        cancellation.cancel();
                    }
                }
            },
            || {
                with_sqlite_read_progress(
                    &store.connection,
                    Some(&control),
                    IndexWorkStage::RepositoryTraversal,
                    || {
                        store
                            .connection
                            .query_row(
                                "WITH RECURSIVE numbers(value) AS (
                                     VALUES(1)
                                     UNION ALL
                                     SELECT value + 1 FROM numbers WHERE value < 100000000
                                 )
                                 SELECT SUM(value) FROM numbers",
                                [],
                                |row| row.get::<_, i64>(0),
                            )
                            .map_err(DbError::from)
                    },
                )
            },
        );
        require(
            callback_seen.get()
                && matches!(
                    cancelled,
                    Err(DbError::IndexWork(IndexWorkFailure::Cancelled {
                        stage: IndexWorkStage::RepositoryTraversal
                    }))
                ),
            "active SQLite batch did not propagate typed cancellation",
        )?;
        store.connection.execute(
            "CREATE TEMP TABLE cancellation_follow_up(value INTEGER)",
            [],
        )?;
        Ok(())
    }

    #[test]
    fn fallback_admission_precedes_content_decode_and_clears_progress_handlers()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/good.rs", "hash-good")])?;
        store.replace_file_texts_for_paths(
            &["src/good.rs".to_string()],
            &[IndexedFileText {
                path: "src/good.rs".to_string(),
                content_hash: Some("hash-good".to_string()),
                byte_count: 12,
                line_count: 1,
                content: "needle good\n".to_string(),
            }],
        )?;
        store.connection.execute_batch(
            "INSERT INTO nodes(path, kind, exists_now) VALUES('src/bad.rs', 'file', 1);
             INSERT INTO file_content_classifications(path, classification)
                VALUES('src/bad.rs', 'source');
             INSERT INTO file_texts(path, content_hash, byte_count, line_count, content)
                VALUES('src/bad.rs', 'hash-bad', 1, 1, x'80');",
        )?;

        let mut admitted = Vec::new();
        let mut visited = Vec::new();
        store.visit_file_texts_for_fallback(
            Some("src"),
            None,
            |metadata| {
                admitted.push((metadata.path.clone(), metadata.classification));
                Ok(if metadata.path == "src/bad.rs" {
                    FileTextAdmission::Skip
                } else {
                    FileTextAdmission::Read
                })
            },
            |text| {
                visited.push(text.path);
                Ok(true)
            },
        )?;
        require_eq(
            &admitted,
            &vec![
                ("src/bad.rs".to_string(), ContentClassification::Source),
                ("src/good.rs".to_string(), ContentClassification::Opaque),
            ],
            "fallback metadata admission rows",
        )?;
        require_eq(
            &visited,
            &vec!["src/good.rs".to_string()],
            "fallback decoded rows",
        )?;
        let mut scoped_plan = store.connection.prepare(&format!(
            "EXPLAIN QUERY PLAN {FILE_TEXT_FALLBACK_SCOPED_METADATA_SQL}"
        ))?;
        let scoped_plan_details = scoped_plan
            .query_map(params!["src", "src/", "src0"], |row| {
                row.get::<_, String>(3)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        require(
            scoped_plan_details
                .iter()
                .any(|detail| detail.contains("sqlite_autoindex_file_texts_1"))
                && scoped_plan_details.iter().any(|detail| {
                    detail.contains("sqlite_autoindex_file_content_classifications_1")
                })
                && scoped_plan_details.iter().all(|detail| {
                    !detail.contains("SCAN text") && !detail.contains("SCAN classification")
                }),
            "path-scoped fallback did not use bounded binary index ranges",
        )?;
        let mut scoped_opcodes = store
            .connection
            .prepare(&format!("EXPLAIN {FILE_TEXT_FALLBACK_SCOPED_METADATA_SQL}"))?;
        let scoped_opcode_rows = scoped_opcodes
            .query_map(params!["src", "src/", "src0"], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let file_text_table_cursors = scoped_opcode_rows
            .iter()
            .filter(|(opcode, _, _, shape)| opcode == "DeferredSeek" && shape.is_some())
            .map(|(_, _, table_cursor, _)| *table_cursor)
            .collect::<Vec<_>>();
        require(
            !file_text_table_cursors.is_empty()
                && scoped_opcode_rows
                    .iter()
                    .all(|(opcode, cursor, column, _)| {
                        opcode != "Column"
                            || !file_text_table_cursors.contains(cursor)
                            || *column != 4
                    }),
            &format!(
                "fallback metadata cursor read source content before admission: {scoped_opcode_rows:?}"
            ),
        )?;
        require(
            store
                .visit_file_texts_for_fallback(
                    Some("src/bad.rs"),
                    None,
                    |_| Ok(FileTextAdmission::Read),
                    |_| Ok(true),
                )
                .is_err(),
            "invalid source content decoded without error",
        )?;

        let success_cancellation = projectatlas_core::IndexCancellation::new();
        let success_control = IndexWorkControl::new(success_cancellation.clone(), None);
        store.query_file_text_fts_candidates(
            &FileTextFtsQuery {
                literal_token: "needle",
                path_prefix: None,
                limit: 10,
            },
            Some(&success_control),
        )?;
        success_cancellation.cancel();
        let recursive_sum = store.connection.query_row(
            "WITH RECURSIVE numbers(value) AS (
                 VALUES(1) UNION ALL SELECT value + 1 FROM numbers WHERE value < 5000
             ) SELECT SUM(value) FROM numbers",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        require_eq(
            &recursive_sum,
            &12_502_500,
            "cleared success progress handler",
        )?;

        let cancellation = projectatlas_core::IndexCancellation::new();
        let control = IndexWorkControl::new(cancellation.clone(), None);
        let mut rows_seen = 0;
        let cancelled = store.visit_file_texts_for_fallback(
            Some("src"),
            Some(&control),
            |_| {
                rows_seen += 1;
                cancellation.cancel();
                Ok(FileTextAdmission::Skip)
            },
            |_| Ok(true),
        );
        require_eq(&rows_seen, &1, "rows before fallback cancellation")?;
        require(
            matches!(
                cancelled,
                Err(DbError::IndexWork(IndexWorkFailure::Cancelled {
                    stage: IndexWorkStage::TextIndex
                }))
            ),
            "fallback cancellation was not typed",
        )?;
        let post_error_sum = store.connection.query_row(
            "WITH RECURSIVE numbers(value) AS (
                 VALUES(1) UNION ALL SELECT value + 1 FROM numbers WHERE value < 5000
             ) SELECT SUM(value) FROM numbers",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        require_eq(
            &post_error_sum,
            &12_502_500,
            "cleared error progress handler",
        )?;

        let expired = IndexWorkControl::with_deadline(
            projectatlas_core::IndexCancellation::new(),
            Instant::now(),
        );
        let deadline = store.query_file_text_fts_candidates(
            &FileTextFtsQuery {
                literal_token: "needle",
                path_prefix: None,
                limit: 10,
            },
            Some(&expired),
        );
        require(
            matches!(
                deadline,
                Err(DbError::IndexWork(IndexWorkFailure::DeadlineExceeded {
                    stage: IndexWorkStage::TextIndex
                }))
            ),
            "FTS deadline was not typed",
        )?;
        store.connection.execute(
            "DELETE FROM file_content_classifications WHERE path = 'src/good.rs'",
            [],
        )?;
        let missing_classification = store.visit_file_texts_for_fallback(
            Some("src/good.rs"),
            None,
            |_| Ok(FileTextAdmission::Read),
            |_| Ok(true),
        );
        require(
            matches!(
                missing_classification,
                Err(DbError::FileContentClassificationMissing { .. })
            ),
            "fallback omitted a text row whose predecode classification was missing",
        )?;
        Ok(())
    }

    #[test]
    fn suggested_purpose_is_not_approved() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let node = Node {
            path: "src/main.rs".to_string(),
            kind: NodeKind::File,
            parent_path: normalized_parent("src/main.rs"),
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(12),
            mtime_ns: Some(10),
            content_hash: Some("abc".to_string()),
        };
        store.replace_scan(&[node])?;
        store.set_suggested_purpose("src/main.rs", "Maybe application entry point")?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.source,
            &PurposeSource::Generated,
            "suggested source",
        )?;
        require_eq(
            &nodes[0].purpose.status,
            &PurposeStatus::Suggested,
            "suggested status",
        )?;
        store.set_purpose(
            "src/main.rs",
            "Application entry point",
            PurposeSource::Agent,
        )?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.status,
            &PurposeStatus::Approved,
            "agent-approved status",
        )?;
        store.set_suggested_purpose("src/main.rs", "Late generated suggestion")?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.purpose,
            &Some("Application entry point".to_string()),
            "approved purpose survives a late suggestion",
        )?;
        require_eq(
            &nodes[0].purpose.source,
            &PurposeSource::Agent,
            "approved purpose source survives a late suggestion",
        )?;
        require_eq(
            &nodes[0].purpose.status,
            &PurposeStatus::Approved,
            "approved purpose status survives a late suggestion",
        )?;

        store.connection.execute(
            "
            UPDATE purposes
            SET status = ?1
            WHERE node_id = (SELECT id FROM nodes WHERE path = ?2)
            ",
            params![PurposeStatus::Stale.as_str(), "src/main.rs"],
        )?;
        store.set_suggested_purpose("src/main.rs", "Later generated suggestion")?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.purpose,
            &Some("Application entry point".to_string()),
            "stale reviewed purpose survives a late suggestion",
        )?;
        require_eq(
            &nodes[0].purpose.source,
            &PurposeSource::Agent,
            "stale reviewed source survives a late suggestion",
        )?;
        require_eq(
            &nodes[0].purpose.status,
            &PurposeStatus::Stale,
            "stale reviewed status survives a late suggestion",
        )?;
        Ok(())
    }

    #[test]
    fn agent_reviewed_marker_depends_on_agent_approved_source() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash-a")])?;

        store.set_suggested_purpose("src/main.rs", "Maybe application entry point")?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.agent_reviewed(),
            &false,
            "generated suggestion is not agent reviewed",
        )?;

        store.set_purpose(
            "src/main.rs",
            "Imported application entry point",
            PurposeSource::Imported,
        )?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.agent_reviewed(),
            &false,
            "imported purpose is not agent reviewed",
        )?;

        store.set_purpose(
            "src/main.rs",
            "Agent-reviewed application entry point",
            PurposeSource::Agent,
        )?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.agent_reviewed(),
            &true,
            "agent-approved purpose is agent reviewed",
        )?;

        store.connection.execute(
            "
            UPDATE purposes
            SET source = 'human'
            WHERE node_id = (SELECT id FROM nodes WHERE path = 'src/main.rs')
            ",
            [],
        )?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.source,
            &PurposeSource::Agent,
            "legacy human source normalizes to agent",
        )?;
        require_eq(
            &nodes[0].purpose.agent_reviewed(),
            &true,
            "legacy approved human row remains reviewed",
        )?;

        store.replace_scan(&[test_file_node("src/main.rs", "hash-b")])?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].purpose.status,
            &PurposeStatus::Approved,
            "changed reviewed purpose stays approved",
        )?;
        require_eq(
            &nodes[0].purpose.agent_reviewed(),
            &true,
            "changed approved purpose remains agent reviewed",
        )?;
        Ok(())
    }

    #[test]
    fn purpose_curation_batch_coalesces_current_unapproved_rows() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("project");
        fs::create_dir(&root)?;
        let database = temp.path().join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
            test_file_node("src/accepted.rs", "hash-accepted"),
        ])?;
        store.set_suggested_purpose("src/b.rs", "Generated B suggestion")?;
        store.set_purpose("src/accepted.rs", "Accepted purpose", PurposeSource::Agent)?;

        let selected = vec![
            "src/b.rs".to_string(),
            "src/a.rs".to_string(),
            "src/a.rs".to_string(),
            "src/accepted.rs".to_string(),
        ];
        let first = store.load_purpose_curation_batch(" issue   308 ", &selected)?;
        let second = store.load_purpose_curation_batch(
            "issue 308",
            &selected.iter().rev().cloned().collect::<Vec<_>>(),
        )?;
        require_eq(&first.task, &"issue 308".to_string(), "normalized task")?;
        require_eq(&first.work_key, &second.work_key, "deterministic batch key")?;
        require_eq(&first.items, &second.items, "deterministic candidate rows")?;
        require_eq(&first.items.len(), &2, "coalesced unapproved row count")?;
        require_eq(
            &first
                .items
                .iter()
                .map(|item| item.node.node.path.as_str())
                .collect::<Vec<_>>(),
            &vec!["src/a.rs", "src/b.rs"],
            "sorted actionable paths",
        )?;
        require_eq(
            &first
                .items
                .iter()
                .all(|item| item.work_key.len() == 64 && item.state_token.len() == 64),
            &true,
            "bounded opaque item identities",
        )?;

        let page = store.purpose_curation_findings_page_current(&HealthQuery {
            start_index: 0,
            limit: 20,
            category: None,
            severity: Some(Severity::Warning),
            path_prefix: None,
            summary_only: false,
            scope: HealthScope::all(),
        })?;
        require_eq(&page.total, &2, "actionable queue count")?;
        require_eq(
            &page
                .findings
                .iter()
                .any(|finding| finding.path == "src/accepted.rs"),
            &false,
            "accepted purpose omitted from automatic curation",
        )?;

        store.set_purpose(
            "src/accepted.rs",
            "Deliberately corrected purpose",
            PurposeSource::Agent,
        )?;
        let corrected = store
            .load_node_by_path("src/accepted.rs")?
            .ok_or_else(|| io::Error::other("corrected purpose path disappeared"))?;
        require_eq(
            &corrected.purpose.purpose,
            &Some("Deliberately corrected purpose".to_string()),
            "explicit accepted-purpose correction",
        )?;
        Ok(())
    }

    #[test]
    fn explicit_purpose_rollback_reports_storage_failure() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("project");
        fs::create_dir(&root)?;
        let database = temp.path().join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        store.replace_scan(&[test_file_node("source.rs", "hash")])?;
        let before_revision = store.authored_purpose_revision()?;
        let transaction = store.begin_purpose_mutation()?;
        store.set_purpose("source.rs", "Rejected purpose", PurposeSource::Agent)?;
        store.connection.execute_batch("ROLLBACK")?;

        require(
            matches!(transaction.rollback(), Err(DbError::Sqlite(_))),
            "explicit purpose rollback suppressed its SQLite failure",
        )?;
        require_eq(
            &store.authored_purpose_revision()?,
            &before_revision,
            "manually rolled-back purpose revision",
        )?;
        require(
            store
                .load_node_by_path("source.rs")?
                .is_none_or(|node| node.purpose.purpose.as_deref() != Some("Rejected purpose")),
            "manual rollback retained the rejected purpose",
        )
    }

    #[test]
    fn conditional_purpose_batch_is_atomic_and_rejects_changed_work() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("project");
        fs::create_dir(&root)?;
        let database = temp.path().join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        let nodes = vec![
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
            test_file_node("src/changed.rs", "hash-changed"),
            test_file_node("src/accepted.rs", "hash-accepted"),
            test_file_node("src/generation.rs", "hash-generation"),
            test_file_node("src/rollback-a.rs", "hash-rollback-a"),
            test_file_node("src/rollback-b.rs", "hash-rollback-b"),
        ];
        store.replace_scan(&nodes)?;
        store.set_suggested_purpose("src/b.rs", "Generated B suggestion")?;
        store.set_suggested_purpose("src/changed.rs", "Original suggestion")?;
        require_eq(
            &store.authored_purpose_revision()?,
            &0,
            "suggestions do not advance authored-purpose revision",
        )?;
        let task = "issue-308-purpose-curation";

        let apply_batch = store
            .load_purpose_curation_batch(task, &["src/a.rs".to_string(), "src/b.rs".to_string()])?;
        let requests = apply_batch
            .items
            .iter()
            .map(|candidate| {
                conditional_purpose_request(
                    task,
                    candidate,
                    &format!("Reviewed {}", candidate.node.node.path),
                )
            })
            .collect::<Vec<_>>();
        let applied = store.conditionally_set_purposes(&requests)?;
        require_eq(
            &applied
                .iter()
                .map(|result| result.state)
                .collect::<Vec<_>>(),
            &vec![
                PurposeConditionalApplyState::Applied,
                PurposeConditionalApplyState::Applied,
            ],
            "one-transaction batch outcomes",
        )?;
        require_eq(
            &applied.iter().all(|result| {
                result.current_purpose.as_ref().is_some_and(|purpose| {
                    purpose.status == PurposeStatus::Approved
                        && purpose.source == PurposeSource::Agent
                })
            }),
            &true,
            "applied transaction-owned purpose metadata",
        )?;
        require_eq(
            &store.authored_purpose_revision()?,
            &1,
            "one accepted batch advances one authored-purpose revision",
        )?;

        let changed = store
            .load_purpose_curation_batch(task, &["src/changed.rs".to_string()])?
            .items
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("changed candidate missing"))?;
        store.set_suggested_purpose("src/changed.rs", "Concurrent suggestion")?;
        let changed_result = store
            .conditionally_set_purposes(&[conditional_purpose_request(
                task,
                &changed,
                "Stale curator answer",
            )])?
            .pop()
            .ok_or_else(|| io::Error::other("changed conditional result missing"))?;
        require_eq(
            &changed_result.state,
            &PurposeConditionalApplyState::Stale,
            "changed suggestion state",
        )?;
        require_eq(
            &changed_result
                .current_purpose
                .as_ref()
                .map(|purpose| (purpose.status, purpose.source, purpose.purpose.as_deref())),
            &Some((
                PurposeStatus::Suggested,
                PurposeSource::Generated,
                Some("Concurrent suggestion"),
            )),
            "stale transaction-owned purpose metadata",
        )?;
        require_eq(
            &store.authored_purpose_revision()?,
            &1,
            "stale conditional purpose leaves revision unchanged",
        )?;

        let accepted = store
            .load_purpose_curation_batch(task, &["src/accepted.rs".to_string()])?
            .items
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("accepted candidate missing"))?;
        store.set_purpose(
            "src/accepted.rs",
            "Concurrent accepted purpose",
            PurposeSource::Agent,
        )?;
        require_eq(
            &store.authored_purpose_revision()?,
            &2,
            "explicit accepted correction advances the authored-purpose revision",
        )?;
        store.set_purpose(
            "src/accepted.rs",
            "Concurrent accepted purpose",
            PurposeSource::Agent,
        )?;
        require_eq(
            &store.authored_purpose_revision()?,
            &2,
            "identical accepted purpose leaves the authored-purpose revision unchanged",
        )?;
        let accepted_result = store
            .conditionally_set_purposes(&[conditional_purpose_request(
                task,
                &accepted,
                "Stale curator overwrite",
            )])?
            .pop()
            .ok_or_else(|| io::Error::other("accepted conditional result missing"))?;
        require_eq(
            &accepted_result.state,
            &PurposeConditionalApplyState::Accepted,
            "accepted concurrent state",
        )?;
        require_eq(
            &accepted_result
                .current_purpose
                .as_ref()
                .map(|purpose| (purpose.status, purpose.source, purpose.purpose.as_deref())),
            &Some((
                PurposeStatus::Approved,
                PurposeSource::Agent,
                Some("Concurrent accepted purpose"),
            )),
            "accepted transaction-owned purpose metadata",
        )?;
        require_eq(
            &store.authored_purpose_revision()?,
            &2,
            "accepted conditional no-op leaves revision unchanged",
        )?;

        let unavailable_result = store
            .conditionally_set_purposes(&[PurposeConditionalApplyRequest {
                task: task.to_string(),
                path: "src/unavailable.rs".to_string(),
                work_key: "unavailable-work".to_string(),
                state_token: "unavailable-state".to_string(),
                purpose: "Unavailable purpose".to_string(),
            }])?
            .pop()
            .ok_or_else(|| io::Error::other("unavailable conditional result missing"))?;
        require_eq(
            &unavailable_result.state,
            &PurposeConditionalApplyState::PathUnavailable,
            "unavailable path state",
        )?;
        require_eq(
            &unavailable_result.current_purpose,
            &None,
            "unavailable transaction-owned purpose metadata",
        )?;

        let generation = store
            .load_purpose_curation_batch(task, &["src/generation.rs".to_string()])?
            .items
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("generation candidate missing"))?;
        let project = store
            .project_instance_id()?
            .ok_or_else(|| io::Error::other("project identity missing"))?;
        {
            let mut publication = store.begin_index_publication("purpose-curation-test")?;
            publication.begin_scan_replacement()?;
            publication.upsert_scan_node_batch(&nodes)?;
            publication.finish_scan_replacement()?;
            publication.replace_repository_graph(project, &[], &[], &[], &[])?;
            publication.complete()?;
        }
        let generation_state = store.conditionally_set_purpose(
            task,
            "src/generation.rs",
            &generation.work_key,
            &generation.state_token,
            "Prior-generation answer",
        )?;
        require_eq(
            &generation_state,
            &PurposeConditionalApplyState::Stale,
            "generation-bound state",
        )?;

        let current = store.load_nodes_by_paths(&[
            "src/changed.rs".to_string(),
            "src/accepted.rs".to_string(),
            "src/generation.rs".to_string(),
        ])?;
        let purposes = current
            .iter()
            .map(|node| {
                (
                    node.node.path.as_str(),
                    node.purpose.purpose.as_deref(),
                    node.purpose.status,
                )
            })
            .collect::<Vec<_>>();
        require_eq(
            &purposes,
            &vec![
                (
                    "src/accepted.rs",
                    Some("Concurrent accepted purpose"),
                    PurposeStatus::Approved,
                ),
                (
                    "src/changed.rs",
                    Some("Concurrent suggestion"),
                    PurposeStatus::Suggested,
                ),
                ("src/generation.rs", None, PurposeStatus::Missing),
            ],
            "conflicting rows remain untouched",
        )?;

        let rollback_batch = store.load_purpose_curation_batch(
            task,
            &[
                "src/rollback-a.rs".to_string(),
                "src/rollback-b.rs".to_string(),
            ],
        )?;
        let rollback_requests = rollback_batch
            .items
            .iter()
            .map(|candidate| {
                conditional_purpose_request(
                    task,
                    candidate,
                    &format!("Reviewed {}", candidate.node.node.path),
                )
            })
            .collect::<Vec<_>>();
        store.connection.execute_batch(
            "
            CREATE TEMP TRIGGER abort_second_conditional_purpose_update
            BEFORE UPDATE ON purposes
            WHEN OLD.node_id = (
                SELECT id FROM nodes WHERE path = 'src/rollback-b.rs'
            )
            BEGIN
                SELECT RAISE(ABORT, 'forced conditional purpose rollback');
            END;
            ",
        )?;
        if store.conditionally_set_purposes(&rollback_requests).is_ok() {
            return Err(io::Error::other("injected conditional batch failure succeeded").into());
        }
        drop(store);
        let reopened = AtlasStore::open_for_project(&database, &root)?;
        let rolled_back = reopened.load_nodes_by_paths(&[
            "src/rollback-a.rs".to_string(),
            "src/rollback-b.rs".to_string(),
        ])?;
        require_eq(
            &rolled_back
                .iter()
                .map(|node| node.purpose.status)
                .collect::<Vec<_>>(),
            &vec![PurposeStatus::Missing, PurposeStatus::Missing],
            "failed conditional batch rolled back after reopen",
        )?;
        Ok(())
    }

    #[test]
    fn concurrent_purpose_curators_cannot_overwrite_the_winner() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("project");
        fs::create_dir(&root)?;
        let database = temp.path().join("projectatlas.db");
        let mut store = AtlasStore::open_for_project(&database, &root)?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash-main")])?;
        let candidate = store
            .load_purpose_curation_batch("concurrent-curators", &["src/main.rs".to_string()])?
            .items
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("concurrent candidate missing"))?;
        drop(store);

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let mut handles = Vec::new();
        for purpose in ["First reviewed purpose", "Second reviewed purpose"] {
            let database = database.clone();
            let root = root.clone();
            let barrier = std::sync::Arc::clone(&barrier);
            let candidate = candidate.clone();
            handles.push(std::thread::spawn(move || {
                let store = AtlasStore::open_for_project(&database, &root)?;
                barrier.wait();
                store.conditionally_set_purpose(
                    "concurrent-curators",
                    "src/main.rs",
                    &candidate.work_key,
                    &candidate.state_token,
                    purpose,
                )
            }));
        }
        let outcomes = handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_panic| io::Error::other("curator thread panicked"))?
                    .map_err(io::Error::other)
            })
            .collect::<Result<Vec<_>, io::Error>>()?;
        require_eq(
            &outcomes
                .iter()
                .filter(|state| **state == PurposeConditionalApplyState::Applied)
                .count(),
            &1,
            "single concurrent winner",
        )?;
        require_eq(
            &outcomes
                .iter()
                .filter(|state| **state == PurposeConditionalApplyState::Accepted)
                .count(),
            &1,
            "accepted concurrent loser",
        )?;

        let reopened = AtlasStore::open_for_project(&database, &root)?;
        let node = reopened
            .load_node_by_path("src/main.rs")?
            .ok_or_else(|| io::Error::other("concurrent path disappeared"))?;
        require_eq(
            &node.purpose.status,
            &PurposeStatus::Approved,
            "persisted concurrent winner status",
        )?;
        require_eq(
            &matches!(
                node.purpose.purpose.as_deref(),
                Some("First reviewed purpose" | "Second reviewed purpose")
            ),
            &true,
            "persisted concurrent winner value",
        )?;
        Ok(())
    }

    #[test]
    fn purpose_curation_hydration_uses_existing_unique_indexes() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        let sql = format!("EXPLAIN QUERY PLAN {}", load_nodes_by_paths_sql(2));
        let mut statement = store.connection.prepare(&sql)?;
        let details = statement
            .query_map(params!["src/a.rs", "src/b.rs"], |row| {
                row.get::<_, String>(3)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let plan = details.join("\n");
        require_eq(
            &plan.contains("SEARCH n USING INDEX sqlite_autoindex_nodes_"),
            &true,
            "unique path index query plan",
        )?;
        require_eq(
            &plan.contains("SEARCH p USING INTEGER PRIMARY KEY"),
            &true,
            "purpose primary-key query plan",
        )?;
        require_eq(&plan.contains("SCAN n"), &false, "no node-table scan")?;
        Ok(())
    }

    #[test]
    fn updates_content_summary_without_approving_purpose() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let node = Node {
            path: "src/lib.rs".to_string(),
            kind: NodeKind::File,
            parent_path: normalized_parent("src/lib.rs"),
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(24),
            mtime_ns: Some(10),
            content_hash: Some("def".to_string()),
        };
        store.replace_scan(&[node])?;
        store.set_node_summary(
            "src/lib.rs",
            "rust source defining library entry functions.",
        )?;
        let nodes = store.load_nodes()?;
        require_eq(
            &nodes[0].summary,
            &Some("rust source defining library entry functions.".to_string()),
            "updated content summary",
        )?;
        require_eq(
            &nodes[0].purpose.status,
            &PurposeStatus::Missing,
            "summary update does not approve purpose",
        )?;
        require_eq(
            &store.has_agent_approved_purpose()?,
            &false,
            "missing purpose does not activate graph hydration",
        )?;
        store.set_purpose(
            "src/lib.rs",
            "Owns the library entry points.",
            PurposeSource::Agent,
        )?;
        require_eq(
            &store.has_agent_approved_purpose()?,
            &true,
            "agent-approved purpose activates graph hydration",
        )?;
        Ok(())
    }

    #[test]
    fn replaces_symbol_graph_idempotently() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let graph = SymbolGraph {
            path: "src/main.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![CodeSymbol {
                path: "src/main.rs".to_string(),
                language: Some("rust".to_string()),
                name: "main".to_string(),
                kind: SymbolKind::Function,
                signature: "fn main()".to_string(),
                exported: true,
                documentation: Some("Run the application.".to_string()),
                line_start: 1,
                line_end: 3,
                source_selector: None,
                parent: None,
                parser: ParserKind::TreeSitter,
                detail: Some("function_item".to_string()),
            }],
            relations: vec![SymbolRelation {
                path: "src/main.rs".to_string(),
                source_name: "main".to_string(),
                target_name: "println!".to_string(),
                kind: RelationKind::Calls,
                line: 2,
                context: "println!(\"hello\")".to_string(),
                parser: ParserKind::TreeSitter,
            }],
        };

        store.replace_symbol_graph(&graph)?;
        store.replace_symbol_graph(&graph)?;
        let symbols = store.load_symbols(Some("src/main.rs"), Some("main"), 10)?;
        let relations = store.load_symbol_relations(Some("src/main.rs"), Some("println"), 10)?;
        let metadata = store
            .load_source_parse_metadata("src/main.rs")?
            .ok_or_else(|| io::Error::other("missing source parse metadata"))?;
        require_eq(&symbols.len(), &1, "symbol count after replace")?;
        require_eq(&relations.len(), &1, "relation count after replace")?;
        require_eq(&metadata.parser, &ParserKind::TreeSitter, "metadata parser")?;
        require_eq(&metadata.symbol_count, &1, "metadata symbol count")?;
        require_eq(&metadata.relation_count, &1, "metadata relation count")?;
        require_eq(&symbols[0].exported, &true, "exported metadata")?;
        require_eq(
            &symbols[0].documentation,
            &Some("Run the application.".to_string()),
            "documentation metadata",
        )?;
        Ok(())
    }

    #[test]
    fn symbol_source_selectors_round_trip_all_read_paths_and_reopen() -> Result<(), Box<dyn Error>>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        fs::create_dir(&root)?;
        let database = temp.path().join("projectatlas.db");
        let path = "docs/guide.md";
        let expected = SymbolSourceSelector {
            byte_start: 17,
            byte_end: 31,
            column_start: 4,
            column_end: 2,
        };
        let graph = SymbolGraph {
            path: path.to_string(),
            language: Some("markdown".to_string()),
            parser: ParserKind::Structural,
            symbols: vec![
                CodeSymbol {
                    path: path.to_string(),
                    language: Some("markdown".to_string()),
                    name: "Overview".to_string(),
                    kind: SymbolKind::Heading,
                    signature: "overview#1".to_string(),
                    exported: false,
                    documentation: None,
                    line_start: 2,
                    line_end: 3,
                    source_selector: Some(expected),
                    parent: None,
                    parser: ParserKind::Structural,
                    detail: Some("level=2".to_string()),
                },
                CodeSymbol {
                    path: path.to_string(),
                    language: Some("markdown".to_string()),
                    name: "Compatibility".to_string(),
                    kind: SymbolKind::Heading,
                    signature: "compatibility#1".to_string(),
                    exported: false,
                    documentation: None,
                    line_start: 6,
                    line_end: 6,
                    source_selector: None,
                    parent: None,
                    parser: ParserKind::Structural,
                    detail: Some("level=2".to_string()),
                },
            ],
            relations: Vec::new(),
        };

        let mut store = AtlasStore::open_for_project(&database, &root)?;
        store.replace_scan(&[test_file_node(path, "guide-hash")])?;
        store.replace_symbol_graph(&graph)?;
        let mut publication = store.begin_index_publication("symbol-source-selectors")?;
        publication.upsert_file_content_classification_batch(&[FileContentClassification {
            path: path.to_string(),
            classification: ContentClassification::Documentation,
        }])?;
        publication.complete()?;
        verify_symbol_source_selector_read_paths(&store, path, expected)?;
        drop(store);

        let reopened = AtlasStore::open_for_project(&database, &root)?;
        verify_symbol_source_selector_read_paths(&reopened, path, expected)?;
        drop(reopened);

        let read_only = AtlasStore::open_read_only_for_project(&database, &root)?;
        verify_symbol_source_selector_read_paths(&read_only, path, expected)?;
        read_only.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn symbol_source_selector_constraints_and_corrupt_reads_fail_closed()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let path = "docs/guide.md";
        store.replace_symbol_graph(&SymbolGraph {
            path: path.to_string(),
            language: Some("markdown".to_string()),
            parser: ParserKind::Structural,
            symbols: vec![CodeSymbol {
                path: path.to_string(),
                language: Some("markdown".to_string()),
                name: "Overview".to_string(),
                kind: SymbolKind::Heading,
                signature: "overview#1".to_string(),
                exported: false,
                documentation: None,
                line_start: 2,
                line_end: 2,
                source_selector: None,
                parent: None,
                parser: ParserKind::Structural,
                detail: None,
            }],
            relations: Vec::new(),
        })?;

        for invalid_update in [
            "UPDATE symbols SET source_byte_start = 0 WHERE path = 'docs/guide.md'",
            "UPDATE symbols SET source_byte_start = -1, source_byte_end = 4,
                                source_column_start = 0, source_column_end = 4
              WHERE path = 'docs/guide.md'",
            "UPDATE symbols SET source_byte_start = 5, source_byte_end = 4,
                                source_column_start = 0, source_column_end = 4
              WHERE path = 'docs/guide.md'",
            "UPDATE symbols SET source_byte_start = 0, source_byte_end = 4,
                                source_column_start = 5, source_column_end = 4
              WHERE path = 'docs/guide.md'",
        ] {
            require(
                store.connection.execute(invalid_update, []).is_err(),
                "invalid source selector bypassed its table constraint",
            )?;
        }
        store.connection.execute(
            "UPDATE symbols
                SET source_byte_start = 0, source_byte_end = 4,
                    source_column_start = 0, source_column_end = 4
              WHERE path = ?1",
            [path],
        )?;
        require_symbol_source_selectors(
            &store.load_symbols(Some(path), None, 10)?,
            Some(SymbolSourceSelector {
                byte_start: 0,
                byte_end: 4,
                column_start: 0,
                column_end: 4,
            }),
            "valid constrained selector",
        )?;

        store.connection.execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             UPDATE symbols
                SET source_byte_start = 0, source_byte_end = NULL,
                    source_column_start = 0, source_column_end = 4
              WHERE path = 'docs/guide.md';
             PRAGMA ignore_check_constraints = OFF;",
        )?;
        let corrupt = store.load_symbols(Some(path), None, 10);
        require(
            matches!(corrupt, Err(DbError::Sqlite(_))),
            "partially populated source selector did not fail closed",
        )?;
        Ok(())
    }

    #[test]
    fn symbol_source_selector_write_failure_rolls_back_graph() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let path = "docs/guide.md";
        let previous = SymbolGraph {
            path: path.to_string(),
            language: Some("markdown".to_string()),
            parser: ParserKind::Structural,
            symbols: vec![batch_test_symbol(path, "previous", 1, 0)],
            relations: Vec::new(),
        };
        store.replace_symbol_graph(&previous)?;

        let mut valid = batch_test_symbol(path, "valid", 2, 0);
        valid.source_selector = Some(SymbolSourceSelector {
            byte_start: 4,
            byte_end: 9,
            column_start: 0,
            column_end: 5,
        });
        let mut invalid = batch_test_symbol(path, "invalid", 3, 0);
        invalid.source_selector = Some(SymbolSourceSelector {
            byte_start: 12,
            byte_end: 11,
            column_start: 0,
            column_end: 5,
        });
        let replacement = SymbolGraph {
            path: path.to_string(),
            language: Some("markdown".to_string()),
            parser: ParserKind::Structural,
            symbols: vec![valid, invalid],
            relations: Vec::new(),
        };
        let Err(error) = store.replace_symbol_graph(&replacement) else {
            return Err(io::Error::other("invalid selector replacement committed").into());
        };
        require(
            matches!(error, DbError::SymbolGraphRowShape { .. }),
            "invalid selector replacement returned the wrong error",
        )?;
        require_eq(
            &store.load_symbol_graphs_for_paths(&[path.to_string()])?,
            &vec![previous],
            "selector failure transaction rollback",
        )?;
        Ok(())
    }

    #[test]
    fn classified_symbol_list_filters_before_limit_and_preserves_legacy_order()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let classified_paths = [
            (
                "config/first.toml",
                ContentClassification::ConfigurationData,
            ),
            ("docs/guide.md", ContentClassification::Documentation),
            ("other/notes.txt", ContentClassification::OtherText),
            ("src/lib.rs", ContentClassification::Source),
            ("src/second.rs", ContentClassification::Source),
        ];
        let nodes = classified_paths
            .iter()
            .enumerate()
            .map(|(index, (path, _))| test_file_node(path, &format!("hash-{index}")))
            .collect::<Vec<_>>();
        store.replace_scan(&nodes)?;
        for (index, (path, _)) in classified_paths.iter().enumerate() {
            store.replace_symbol_graph(&SymbolGraph {
                path: (*path).to_string(),
                language: Some("rust".to_string()),
                parser: ParserKind::TreeSitter,
                symbols: vec![batch_test_symbol(path, &format!("needle_{index}"), 1, 0)],
                relations: Vec::new(),
            })?;
        }
        let classifications = classified_paths
            .iter()
            .map(|(path, classification)| FileContentClassification {
                path: (*path).to_string(),
                classification: *classification,
            })
            .collect::<Vec<_>>();
        let mut publication = store.begin_index_publication("classified-symbol-list")?;
        publication.upsert_file_content_classification_batch(&classifications)?;
        publication.complete()?;

        for (file, query, limit) in [
            (None, None, 3),
            (None, Some("needle"), 3),
            (Some("src/second.rs"), None, 10),
            (Some("src/second.rs"), Some("needle_4"), 10),
            (Some("src/second.rs"), Some("second"), 10),
            (None, None, 0),
        ] {
            let legacy = store.load_symbols(file, query, limit)?;
            let classified = store.load_classified_symbols(
                file,
                query,
                ContentSelection::UnspecifiedLegacy,
                limit,
            )?;
            require_eq(
                &classified
                    .into_iter()
                    .map(|row| row.symbol)
                    .collect::<Vec<_>>(),
                &legacy,
                "omitted classified symbol candidates and order",
            )?;
        }
        let omitted = store.load_classified_symbols(
            None,
            Some("needle"),
            ContentSelection::UnspecifiedLegacy,
            3,
        )?;
        require_eq(
            &omitted
                .iter()
                .map(|row| row.classification)
                .collect::<Vec<_>>(),
            &vec![
                ContentClassification::ConfigurationData,
                ContentClassification::Documentation,
                ContentClassification::OtherText,
            ],
            "additive legacy classifications",
        )?;

        let source =
            store.load_classified_symbols(None, Some("needle"), ContentSelection::Source, 1)?;
        require_eq(
            &source.first().map(|row| row.symbol.path.as_str()),
            &Some("src/lib.rs"),
            "source selection before limit",
        )?;
        let documentation = store.load_classified_symbols(
            None,
            Some("needle"),
            ContentSelection::Documentation,
            1,
        )?;
        require_eq(
            &documentation.first().map(|row| row.symbol.path.as_str()),
            &Some("docs/guide.md"),
            "documentation selection before limit",
        )?;
        let both =
            store.load_classified_symbols(None, Some("needle"), ContentSelection::Both, 2)?;
        require_eq(
            &both
                .iter()
                .map(|row| row.symbol.path.as_str())
                .collect::<Vec<_>>(),
            &vec!["docs/guide.md", "src/lib.rs"],
            "combined selection order",
        )?;
        let file_query = store.load_classified_symbols(
            Some("src/second.rs"),
            Some("needle_4"),
            ContentSelection::Source,
            10,
        )?;
        require_eq(&file_query.len(), &1, "file/query classified symbol filter")?;
        Ok(())
    }

    #[test]
    fn classified_symbol_list_rejects_missing_owner_and_uses_existing_indexes()
    -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/lib.rs", "hash")])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/lib.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![batch_test_symbol("src/lib.rs", "needle", 1, 0)],
            relations: Vec::new(),
        })?;

        for selection in [
            ContentSelection::UnspecifiedLegacy,
            ContentSelection::Source,
            ContentSelection::Both,
        ] {
            let (sql, bindings) = classified_symbols_sql(None, Some("needle"), selection, 1);
            let mut statement = store
                .connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
            let plan = statement
                .query_map(params_from_iter(bindings.iter()), |row| {
                    row.get::<_, String>(3)
                })?
                .collect::<Result<Vec<_>, _>>()?
                .join("\n");
            require(
                plan.contains("idx_symbols_path")
                    && plan.contains("sqlite_autoindex_file_content_classifications_1")
                    && !plan.contains("SCAN classification"),
                &format!("classified symbol query missed existing indexes: {plan}"),
            )?;
        }

        store.connection.execute(
            "DELETE FROM file_content_classifications WHERE path = 'src/lib.rs'",
            [],
        )?;
        for selection in [
            ContentSelection::UnspecifiedLegacy,
            ContentSelection::Source,
            ContentSelection::Both,
        ] {
            let Err(error) = store.load_classified_symbols(Some("src/lib.rs"), None, selection, 10)
            else {
                return Err(
                    io::Error::other("symbol without a file classification was accepted").into(),
                );
            };
            require(
                matches!(error, DbError::FileContentClassificationMissing { .. }),
                "missing symbol classification returned the wrong error",
            )?;
        }
        Ok(())
    }

    #[test]
    fn import_alias_lookup_uses_kind_first_covering_index() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::new(),
            relations: vec![
                SymbolRelation {
                    path: "src/main.rs".to_string(),
                    source_name: "main".to_string(),
                    target_name: "use crate::service::run".to_string(),
                    kind: RelationKind::Imports,
                    line: 1,
                    context: "use crate::service::run;".to_string(),
                    parser: ParserKind::TreeSitter,
                },
                SymbolRelation {
                    path: "src/main.rs".to_string(),
                    source_name: "main".to_string(),
                    target_name: "use crate::other::Thing".to_string(),
                    kind: RelationKind::Imports,
                    line: 2,
                    context: "use crate::other::Thing;".to_string(),
                    parser: ParserKind::TreeSitter,
                },
            ],
        })?;

        let relations =
            store.load_import_relations_matching_targets(&["service".to_string()], 10)?;
        require_eq(&relations.len(), &1, "matched import relation count")?;
        require_eq(
            &store
                .load_import_relations_for_path("src/main.rs", 10)?
                .len(),
            &2,
            "exact-path import relation count",
        )?;
        let mut statement = store.connection.prepare(
            "EXPLAIN QUERY PLAN
             SELECT path, source_name, target_name, line
             FROM symbol_relations INDEXED BY idx_symbol_import_alias_lookup
             WHERE kind = 'imports' AND target_name LIKE '%service%' ESCAPE '\\'
             ORDER BY path, line, source_name, target_name
             LIMIT 10",
        )?;
        let plan = statement
            .query_map([], |row| row.get::<_, String>(3))?
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        require(
            plan.contains(
                "SEARCH symbol_relations USING COVERING INDEX idx_symbol_import_alias_lookup (kind=?)",
            ),
            &format!("import alias lookup missed kind-first covering index: {plan}"),
        )?;
        Ok(())
    }

    #[test]
    fn file_scoped_symbol_kind_lookup_uses_path_index() -> Result<(), Box<dyn Error>> {
        let store = AtlasStore::in_memory()?;
        for sql in [
            "EXPLAIN QUERY PLAN
             SELECT path, language, name, kind, signature, line_start, line_end,
                    parent, parser, detail, exported, documentation,
                    source_byte_start, source_byte_end,
                    source_column_start, source_column_end
             FROM symbols INDEXED BY idx_symbols_path
             WHERE path = 'src/lib.rs' AND kind IN ('function')
             ORDER BY path, line_start, name
             LIMIT 25",
            "EXPLAIN QUERY PLAN
             SELECT COUNT(*) FROM symbols INDEXED BY idx_symbols_path
             WHERE path = 'src/lib.rs' AND kind IN ('function')",
        ] {
            let mut statement = store.connection.prepare(sql)?;
            let plan = statement
                .query_map([], |row| row.get::<_, String>(3))?
                .collect::<Result<Vec<_>, _>>()?
                .join("\n");
            require(
                plan.contains("SEARCH symbols USING INDEX idx_symbols_path (path=?)"),
                &format!("file-scoped symbol lookup missed exact-path index: {plan}"),
            )?;
        }
        Ok(())
    }

    #[test]
    fn preserves_source_parse_and_fact_parser_provenance_independently()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let database = temp.path().join("projectatlas.db");
        let mut store = AtlasStore::open(&database)?;
        let graph = SymbolGraph {
            path: "src/optional.lang".to_string(),
            language: Some("optional-language".to_string()),
            parser: ParserKind::Fallback,
            symbols: vec![CodeSymbol {
                path: "src/optional.lang".to_string(),
                language: Some("optional-language".to_string()),
                name: "entry".to_string(),
                kind: SymbolKind::Function,
                signature: "entry()".to_string(),
                exported: false,
                documentation: None,
                line_start: 1,
                line_end: 1,
                source_selector: None,
                parent: None,
                parser: ParserKind::Fallback,
                detail: None,
            }],
            relations: vec![SymbolRelation {
                path: "src/optional.lang".to_string(),
                source_name: "entry".to_string(),
                target_name: "helper".to_string(),
                kind: RelationKind::Calls,
                line: 1,
                context: "entry()".to_string(),
                parser: ParserKind::Fallback,
            }],
        };
        let metadata = SourceParseMetadata {
            path: graph.path.clone(),
            language: graph.language.clone(),
            parser: ParserKind::TreeSitter,
            symbol_count: graph.symbols.len(),
            relation_count: graph.relations.len(),
        };

        store.replace_symbol_graph_with_metadata(&graph, &metadata)?;

        let stored_metadata = store
            .load_source_parse_metadata(&graph.path)?
            .ok_or_else(|| io::Error::other("missing independent source parse metadata"))?;
        let symbols = store.load_symbols(Some(&graph.path), Some("entry"), 10)?;
        let relations = store.load_symbol_relations(Some(&graph.path), Some("helper"), 10)?;
        require_eq(
            &stored_metadata.parser,
            &ParserKind::TreeSitter,
            "grammar-backed source parser",
        )?;
        require_eq(
            &symbols[0].parser,
            &ParserKind::Fallback,
            "fallback symbol provenance",
        )?;
        require_eq(
            &relations[0].parser,
            &ParserKind::Fallback,
            "fallback relation provenance",
        )?;

        let invalid_metadata = SourceParseMetadata {
            symbol_count: 0,
            ..metadata
        };
        let Err(error) = store.replace_symbol_graph_with_metadata(&graph, &invalid_metadata) else {
            return Err(io::Error::other("mismatched explicit metadata was accepted").into());
        };
        if !matches!(error, DbError::SymbolGraphRowShape { .. }) {
            return Err(io::Error::other(format!(
                "mismatched explicit metadata returned the wrong error: {error}"
            ))
            .into());
        }

        let empty_graph = SymbolGraph {
            path: "src/empty.optional".to_string(),
            language: Some("optional-language".to_string()),
            parser: ParserKind::Fallback,
            symbols: Vec::new(),
            relations: Vec::new(),
        };
        let empty_metadata = SourceParseMetadata {
            path: empty_graph.path.clone(),
            language: empty_graph.language.clone(),
            parser: ParserKind::TreeSitter,
            symbol_count: 0,
            relation_count: 0,
        };
        store.replace_symbol_graph_with_metadata(&empty_graph, &empty_metadata)?;
        drop(store);

        let reader = AtlasStore::open_read_only(&database)?;
        let reopened_metadata = reader
            .load_source_parse_metadata(&graph.path)?
            .ok_or_else(|| io::Error::other("missing reopened source parse metadata"))?;
        let metadata_paths = reader.source_parse_metadata_paths_for_paths(&[
            "src/missing.optional".to_string(),
            empty_graph.path.clone(),
            graph.path.clone(),
            graph.path.clone(),
        ])?;
        let reopened_graphs =
            reader.load_symbol_graphs_for_paths(&[graph.path.clone(), empty_graph.path.clone()])?;
        require_eq(
            &reopened_metadata.parser,
            &ParserKind::TreeSitter,
            "reopened source parser provenance",
        )?;
        require_eq(
            &metadata_paths,
            &HashSet::from([graph.path.clone(), empty_graph.path.clone()]),
            "batched source parse metadata paths",
        )?;
        require_eq(
            &reopened_graphs,
            &vec![empty_graph, graph],
            "reopened fact graph provenance",
        )?;
        reader.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn reconstructs_exact_symbol_graph_batches_from_disk_and_fails_closed()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let db_path = temp.path().join("projectatlas.db");
        let mut store = AtlasStore::open(&db_path)?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        let graph = SymbolGraph {
            path: "src/a.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![CodeSymbol {
                path: "src/a.rs".to_string(),
                language: Some("rust".to_string()),
                name: "owner".to_string(),
                kind: SymbolKind::Function,
                signature: "fn owner()".to_string(),
                exported: true,
                documentation: None,
                line_start: 1,
                line_end: 2,
                source_selector: None,
                parent: None,
                parser: ParserKind::TreeSitter,
                detail: Some("function_item".to_string()),
            }],
            relations: vec![SymbolRelation {
                path: "src/a.rs".to_string(),
                source_name: "owner".to_string(),
                target_name: "dependency".to_string(),
                kind: RelationKind::Calls,
                line: 2,
                context: "dependency()".to_string(),
                parser: ParserKind::TreeSitter,
            }],
        };
        store.replace_symbol_graph(&graph)?;
        let empty_graph = SymbolGraph {
            path: "src/b.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::new(),
            relations: Vec::new(),
        };
        store.replace_symbol_graph(&empty_graph)?;
        drop(store);

        let reader = AtlasStore::open_read_only(&db_path)?;
        let loaded = reader.load_symbol_graphs_for_paths(&[
            "src/b.rs".to_string(),
            "src/a.rs".to_string(),
            "src/a.rs".to_string(),
            "src/missing.rs".to_string(),
        ])?;
        require_eq(
            &loaded,
            &vec![graph, empty_graph],
            "batched symbol graph round trip",
        )?;
        for (table, expected_index) in [
            (
                "source_parse_metadata",
                "sqlite_autoindex_source_parse_metadata_1",
            ),
            ("symbols", "idx_symbols_path"),
            ("symbol_relations", "idx_symbol_relations_path"),
        ] {
            let sql = format!(
                "EXPLAIN QUERY PLAN SELECT path FROM {table}
                  WHERE path IN ('src/a.rs', 'src/b.rs')"
            );
            let mut statement = reader.connection.prepare(&sql)?;
            let plan = statement
                .query_map([], |row| row.get::<_, String>(3))?
                .collect::<Result<Vec<_>, _>>()?
                .join("\n");
            if !plan.contains(expected_index) {
                return Err(io::Error::other(format!(
                    "{table} batch lookup missed {expected_index}: {plan}"
                ))
                .into());
            }
        }
        reader.finish_index_read_snapshot()?;
        drop(reader);

        let writer = AtlasStore::open(&db_path)?;
        writer.connection.execute(
            "UPDATE source_parse_metadata SET symbol_count = 2 WHERE path = 'src/a.rs'",
            [],
        )?;
        let Err(error) = writer.load_symbol_graphs_for_paths(&["src/a.rs".to_string()]) else {
            return Err(io::Error::other("metadata count corruption was accepted").into());
        };
        if !matches!(error, DbError::SymbolGraphRowShape { .. }) {
            return Err(io::Error::other(format!(
                "metadata count corruption returned the wrong error: {error}"
            ))
            .into());
        }
        Ok(())
    }

    #[test]
    fn bounded_symbol_batch_streams_exact_paths_and_uses_path_index() -> Result<(), Box<dyn Error>>
    {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/a.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![
                batch_test_symbol("src/a.rs", "alpha", 1, 32),
                batch_test_symbol("src/a.rs", "beta", 2, 32),
            ],
            relations: Vec::new(),
        })?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/b.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![batch_test_symbol("src/b.rs", "gamma", 1, 32)],
            relations: Vec::new(),
        })?;

        let complete = store.load_symbols_for_paths_bounded(
            &[
                "src/b.rs".to_string(),
                "src/a.rs".to_string(),
                "src/a.rs".to_string(),
            ],
            SymbolBatchReadBudget::new(2, 3, MAX_SYMBOL_BATCH_DECODED_BYTES)?,
            None,
        )?;
        require_eq(
            &complete
                .rows
                .iter()
                .map(|symbol| symbol.name.as_str())
                .collect::<Vec<_>>(),
            &vec!["alpha", "beta", "gamma"],
            "deterministic exact-path symbol order",
        )?;
        require_eq(&complete.truncated, &false, "complete symbol batch")?;
        require_eq(&complete.reached_limit, &None, "complete batch limit")?;
        require_eq(&complete.work.requested_paths, &2, "unique admitted paths")?;
        require_eq(&complete.work.returned_rows, &3, "complete returned rows")?;

        let row_limited = store.load_symbols_for_paths_bounded(
            &["src/b.rs".to_string(), "src/a.rs".to_string()],
            SymbolBatchReadBudget::new(2, 1, MAX_SYMBOL_BATCH_DECODED_BYTES)?,
            None,
        )?;
        require_eq(
            &row_limited.reached_limit,
            &Some(SymbolBatchReadLimit::Rows),
            "row-bound classification",
        )?;
        require_eq(&row_limited.rows.len(), &1, "row-bound retained rows")?;

        let byte_limited = store.load_symbols_for_paths_bounded(
            &["src/a.rs".to_string()],
            SymbolBatchReadBudget::new(1, 3, 1)?,
            None,
        )?;
        require_eq(
            &byte_limited.reached_limit,
            &Some(SymbolBatchReadLimit::DecodedBytes),
            "decoded-byte-bound classification",
        )?;
        require_eq(
            &byte_limited.rows.len(),
            &0,
            "tiny byte budget retained no partial row",
        )?;
        require_eq(
            &byte_limited.work.decoded_bytes,
            &0,
            "tiny byte budget retained no row payload",
        )?;

        let gamma_bytes = code_symbol_decoded_bytes(&complete.rows[2])?;
        let exact_byte_boundary = store.load_symbols_for_paths_bounded(
            &["src/b.rs".to_string()],
            SymbolBatchReadBudget::new(1, 1, gamma_bytes)?,
            None,
        )?;
        require_eq(
            &exact_byte_boundary.rows.len(),
            &1,
            "exact decoded-byte boundary retained the complete row",
        )?;
        require_eq(
            &exact_byte_boundary.work.decoded_bytes,
            &gamma_bytes,
            "exact decoded-byte boundary accounting",
        )?;

        let oversized_signature =
            "x".repeat(usize::try_from(MAX_SYMBOL_BATCH_DECODED_BYTES)?.saturating_add(1));
        store.connection.execute(
            "UPDATE symbols SET signature = ?1, line_start = 'invalid' WHERE path = 'src/b.rs'",
            [&oversized_signature],
        )?;
        let oversized_row = store.load_symbols_for_paths_bounded(
            &["src/b.rs".to_string()],
            SymbolBatchReadBudget::new(1, 1, MAX_SYMBOL_BATCH_DECODED_BYTES)?,
            None,
        )?;
        require_eq(
            &oversized_row.reached_limit,
            &Some(SymbolBatchReadLimit::DecodedBytes),
            "oversized persisted row was rejected before hydration",
        )?;
        require_eq(
            &oversized_row.rows.len(),
            &0,
            "oversized persisted row retained no allocation",
        )?;
        store.connection.execute(
            "UPDATE symbols SET signature = 'fn gamma()', line_start = 1 WHERE path = 'src/b.rs'",
            [],
        )?;

        let many_paths = (0..65)
            .rev()
            .map(|index| format!("generated/{index:04}.rs"))
            .collect::<Vec<_>>();
        for index in 0..65 {
            let path = format!("generated/{index:04}.rs");
            store.replace_symbol_graph(&SymbolGraph {
                path: path.clone(),
                language: Some("rust".to_string()),
                parser: ParserKind::TreeSitter,
                symbols: vec![batch_test_symbol(
                    &path,
                    &format!("generated_{index:04}"),
                    1,
                    0,
                )],
                relations: Vec::new(),
            })?;
        }
        let path_limited = store.load_symbols_for_paths_bounded(
            &many_paths,
            SymbolBatchReadBudget::new(
                MAX_SYMBOL_BATCH_PATHS,
                MAX_SYMBOL_BATCH_ROWS,
                MAX_SYMBOL_BATCH_DECODED_BYTES,
            )?,
            None,
        )?;
        require_eq(
            &path_limited.reached_limit,
            &Some(SymbolBatchReadLimit::Paths),
            "path-bound classification",
        )?;
        require_eq(
            &path_limited.work.requested_paths,
            &MAX_SYMBOL_BATCH_PATHS,
            "bounded path admission",
        )?;
        require_eq(
            &path_limited.rows.len(),
            &usize::try_from(MAX_SYMBOL_BATCH_PATHS)?,
            "all admitted paths loaded across binding chunks",
        )?;
        require(
            path_limited
                .rows
                .iter()
                .any(|symbol| symbol.name == "generated_0063"),
            "last deterministically admitted path was not loaded",
        )?;
        require(
            path_limited
                .rows
                .iter()
                .all(|symbol| symbol.name != "generated_0064"),
            "path outside the deterministic admission set was loaded",
        )?;

        let expired = IndexWorkControl::with_deadline(
            projectatlas_core::IndexCancellation::new(),
            Instant::now(),
        );
        let expired_result = store.load_symbols_for_paths_bounded(
            &["src/a.rs".to_string()],
            SymbolBatchReadBudget::new(1, 3, MAX_SYMBOL_BATCH_DECODED_BYTES)?,
            Some(&expired),
        );
        require(
            matches!(
                expired_result,
                Err(DbError::IndexWork(IndexWorkFailure::DeadlineExceeded {
                    stage: IndexWorkStage::RepositoryTraversal
                }))
            ),
            "expired symbol control returned a partial batch instead of a typed error",
        )?;

        let mut plan_statement = store.connection.prepare(
            "EXPLAIN QUERY PLAN
             SELECT
                 path,
                 language,
                 name,
                 kind,
                 signature,
                 line_start,
                 line_end,
                 parent,
                 parser,
                 detail,
                 exported,
                 documentation,
                 source_byte_start,
                 source_byte_end,
                 source_column_start,
                 source_column_end,
                 length(CAST(path AS BLOB))
                     + COALESCE(length(CAST(language AS BLOB)), 0)
                     + length(CAST(name AS BLOB))
                     + length(CAST(signature AS BLOB))
                     + COALESCE(length(CAST(parent AS BLOB)), 0)
                     + COALESCE(length(CAST(detail AS BLOB)), 0)
                     + COALESCE(length(CAST(documentation AS BLOB)), 0)
             FROM symbols
             WHERE path IN (?1, ?2)
             ORDER BY path, line_start, name
             LIMIT ?3",
        )?;
        let plan = plan_statement
            .query_map(params!["src/a.rs", "src/b.rs", 4], |row| {
                row.get::<_, String>(3)
            })?
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        require(
            plan.contains("idx_symbols_path"),
            &format!("bounded symbol batch missed idx_symbols_path: {plan}"),
        )?;
        Ok(())
    }

    #[test]
    fn full_scan_removal_clears_source_parse_metadata() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/a.rs", "hash-a")])?;
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/a.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::new(),
            relations: Vec::new(),
        })?;
        require_eq(
            &store.load_source_parse_metadata("src/a.rs")?.is_some(),
            &true,
            "metadata exists before removal",
        )?;

        store.replace_scan(&[test_file_node("src/b.rs", "hash-b")])?;
        require_eq(
            &store.load_source_parse_metadata("src/a.rs")?,
            &None,
            "metadata cleared after full scan removal",
        )?;
        Ok(())
    }

    #[test]
    fn call_relations_are_limited_per_target() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let mut relations = Vec::new();
        for index in 0..5 {
            relations.push(SymbolRelation {
                path: format!("src/a{index}.rs"),
                source_name: format!("alpha_caller_{index}"),
                target_name: "alpha".to_string(),
                kind: RelationKind::Calls,
                line: index + 1,
                context: "alpha();".to_string(),
                parser: ParserKind::TreeSitter,
            });
        }
        relations.push(SymbolRelation {
            path: "src/z.rs".to_string(),
            source_name: "beta_caller".to_string(),
            target_name: "beta".to_string(),
            kind: RelationKind::Calls,
            line: 99,
            context: "beta();".to_string(),
            parser: ParserKind::TreeSitter,
        });
        store.replace_symbol_graph(&SymbolGraph {
            path: "src/main.rs".to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: Vec::new(),
            relations,
        })?;

        let loaded =
            store.load_call_relations_to_targets(&["alpha".to_string(), "beta".to_string()], 2)?;
        let alpha_count = loaded
            .iter()
            .filter(|relation| relation.target_name == "alpha")
            .count();
        let beta_count = loaded
            .iter()
            .filter(|relation| relation.target_name == "beta")
            .count();
        require_eq(&alpha_count, &2, "alpha per-target limit")?;
        require_eq(&beta_count, &1, "beta preserved despite alpha skew")?;
        let mut statement = store.connection.prepare(
            "EXPLAIN QUERY PLAN
             SELECT path, source_name, target_name, kind, line, context, parser
             FROM (
                 SELECT path, source_name, target_name, kind, line, context, parser,
                     ROW_NUMBER() OVER (
                         PARTITION BY target_name
                         ORDER BY path, line, source_name, target_name
                     ) AS target_row
                 FROM symbol_relations INDEXED BY idx_symbol_relations_target
                 WHERE kind = 'calls' AND target_name IN ('alpha', 'beta')
             )
             WHERE target_row <= 2
             ORDER BY path, line, source_name, target_name",
        )?;
        let plan = statement
            .query_map([], |row| row.get::<_, String>(3))?
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        require(
            plan.contains(
                "SEARCH symbol_relations USING INDEX idx_symbol_relations_target (target_name=?)",
            ),
            &format!("call target lookup missed exact-target index: {plan}"),
        )?;
        Ok(())
    }

    #[test]
    fn stores_health_resolution_ids() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[
            test_file_node("src/a.rs", "hash-a"),
            test_file_node("src/b.rs", "hash-b"),
        ])?;
        store.set_purpose("src/a.rs", "Shared purpose", PurposeSource::Agent)?;
        store.set_purpose("src/b.rs", "Shared purpose", PurposeSource::Agent)?;
        let duplicate = store
            .unresolved_health_findings(&[])?
            .into_iter()
            .find(|finding| finding.category == "duplicate-purpose")
            .ok_or_else(|| io::Error::other("duplicate-purpose finding missing"))?;
        let duplicate_id = duplicate.id.clone();
        store.resolve_health_finding(&HealthResolution {
            finding_id: duplicate_id.clone(),
            category: duplicate.category,
            path: duplicate.path,
            related_path: duplicate.related_path,
            rationale: "Paths intentionally mirror agent skill variants.".to_string(),
        })?;
        let ids = store.resolved_health_ids()?;
        require_eq(&ids, &vec![duplicate_id], "resolved ids")?;
        Ok(())
    }

    #[test]
    fn health_resolution_accepts_all_scope_agent_review_findings() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let mut asset_file = test_file_node("assets/logo.svg", "hash-logo");
        asset_file.extension = Some(".svg".to_string());
        asset_file.language = None;
        store.replace_scan(&[test_folder_node("assets"), asset_file])?;
        store.set_purpose(
            "assets/logo.svg",
            "Imported SVG brand asset purpose",
            PurposeSource::Imported,
        )?;

        let page = store.unresolved_health_findings_page(
            &[],
            &HealthQuery {
                start_index: 0,
                limit: 20,
                category: Some(CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED.to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        let finding = page
            .findings
            .iter()
            .find(|finding| finding.path == "assets/logo.svg")
            .ok_or_else(|| io::Error::other("asset review finding missing"))?;
        store.resolve_health_finding(&HealthResolution {
            finding_id: finding.id.clone(),
            category: finding.category.clone(),
            path: finding.path.clone(),
            related_path: finding.related_path.clone(),
            rationale: "Asset purpose imported from legacy metadata and intentionally accepted."
                .to_string(),
        })?;

        let remaining = store.unresolved_health_findings_page(
            &store.resolved_health_ids()?,
            &HealthQuery {
                start_index: 0,
                limit: 20,
                category: Some(CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED.to_string()),
                severity: Some(Severity::Warning),
                path_prefix: Some(".".to_string()),
                summary_only: false,
                scope: HealthScope::all(),
            },
        )?;
        require_eq(
            &health_paths(&remaining).contains(&"assets/logo.svg"),
            &false,
            "resolved all-scope asset review finding",
        )?;
        Ok(())
    }

    #[test]
    fn purpose_set_reports_unindexed_path_without_sqlite_leak() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash")])?;
        let error = match store.set_purpose("no/such/file.rs", "Missing file", PurposeSource::Agent)
        {
            Ok(()) => return Err(io::Error::other("missing path should fail").into()),
            Err(error) => error,
        };

        require_eq(
            &error.to_string().contains("no/such/file.rs"),
            &true,
            "path named in error",
        )?;
        require_eq(
            &error.to_string().contains("sqlite error"),
            &false,
            "raw sqlite error hidden",
        )?;
        store.replace_scan(&[])?;
        let error = match store.set_purpose("src/main.rs", "Removed file", PurposeSource::Agent) {
            Ok(()) => return Err(io::Error::other("stale indexed path should fail").into()),
            Err(error) => error,
        };
        require_eq(
            &error.to_string().contains("src/main.rs"),
            &true,
            "stale path named in error",
        )?;
        Ok(())
    }

    #[test]
    fn health_resolution_requires_active_finding_tuple() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        store.replace_scan(&[test_file_node("src/main.rs", "hash")])?;
        let error = match store.resolve_health_finding(&HealthResolution {
            finding_id: "missing-id".to_string(),
            category: "duplicate-purpose".to_string(),
            path: "no/such/file.rs".to_string(),
            related_path: None,
            rationale: "typo".to_string(),
        }) {
            Ok(()) => {
                return Err(io::Error::other("nonexistent health finding should fail").into());
            }
            Err(error) => error,
        };

        require_eq(
            &error.to_string().contains("not active"),
            &true,
            "inactive finding rejected",
        )?;
        Ok(())
    }

    /// Write released schema-8 source, authored, telemetry, and publication state.
    fn write_released_schema_eight_compatibility_fixture(
        db_path: &Path,
        root: &Path,
    ) -> Result<(), Box<dyn Error>> {
        write_schema_eight_compatibility_fixture(
            db_path,
            root,
            schema::create_released_schema_eight,
        )
    }

    /// Write evolved released schema-8 source, authored, telemetry, and publication state.
    fn write_evolved_released_schema_eight_compatibility_fixture(
        db_path: &Path,
        root: &Path,
    ) -> Result<(), Box<dyn Error>> {
        write_schema_eight_compatibility_fixture(
            db_path,
            root,
            schema::create_evolved_released_schema_eight,
        )
    }

    /// Write one captured schema-8 layout with representative durable state.
    fn write_schema_eight_compatibility_fixture(
        db_path: &Path,
        root: &Path,
        create_schema: fn(&Connection) -> DbResult<()>,
    ) -> Result<(), Box<dyn Error>> {
        let connection = Connection::open(db_path)?;
        create_schema(&connection)?;
        schema::configure_writable(&connection)?;
        connection.execute_batch("BEGIN IMMEDIATE")?;
        let write_result = (|| -> DbResult<()> {
            set_metadata(
                &connection,
                PROJECT_ROOT_KEY,
                &normalize_native_path_display(root),
            )?;
            set_metadata(&connection, "custom_setting", "preserved")?;
            set_metadata(&connection, INDEX_PUBLICATION_STATE_KEY, "complete")?;
            set_metadata(
                &connection,
                INDEX_PUBLICATION_FINGERPRINT_KEY,
                "untrusted-contract",
            )?;
            set_metadata(&connection, INDEX_PUBLICATION_GENERATION_KEY, "7")?;
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

                INSERT INTO file_texts(path, content_hash, byte_count, line_count, content)
                VALUES('src/lib.rs', 'hash-legacy', 6, 1, 'legacy');
                ",
            )?;
            connection.execute(
                "
                INSERT INTO health_resolutions(
                    finding_id, category, path, rationale
                )
                VALUES('schema-review', ?1, 'src/lib.rs', 'Reviewed schema fixture')
                ",
                [CATEGORY_DUPLICATE_PURPOSE],
            )?;
            record_released_schema_eight_usage_event(
                &connection,
                &usage_from_estimates(
                    "schema-session",
                    "summary",
                    Some("src/lib.rs".to_string()),
                    None,
                    100,
                    20,
                ),
            )
        })();
        match write_result {
            Ok(()) => connection.execute_batch("COMMIT")?,
            Err(error) => {
                if let Err(rollback) = connection.execute_batch("ROLLBACK") {
                    return Err(DbError::TransactionRollback {
                        operation: Box::new(error),
                        rollback,
                    }
                    .into());
                }
                return Err(error.into());
            }
        }
        Ok(())
    }

    /// Write representative source, authored, telemetry, and publication state at one schema.
    fn write_schema_compatibility_fixture(
        db_path: &Path,
        root: &Path,
        schema_version: i64,
        label: &str,
    ) -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::open(db_path)?;
        store.set_project_root(root)?;
        populate_schema_compatibility_fixture(&mut store, label)?;
        set_metadata(
            &store.connection,
            SCHEMA_VERSION_KEY,
            &schema_version.to_string(),
        )?;
        store
            .connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        Ok(())
    }

    /// Populate representative source, authored, telemetry, and publication state.
    pub(crate) fn populate_schema_compatibility_fixture(
        store: &mut AtlasStore,
        label: &str,
    ) -> Result<(), Box<dyn Error>> {
        {
            let mut publication = store.begin_index_publication("untrusted-contract")?;
            write_test_projection(&mut publication, label)?;
            publication.complete()?;
        }
        store.set_purpose(
            "src/lib.rs",
            "Schema compatibility source",
            PurposeSource::Agent,
        )?;
        store.connection.execute(
            "
            INSERT INTO health_resolutions(
                finding_id,
                category,
                path,
                rationale
            )
            VALUES('schema-review', ?1, 'src/lib.rs', 'Reviewed schema fixture')
            ",
            [CATEGORY_DUPLICATE_PURPOSE],
        )?;
        store.record_usage(&usage_from_estimates(
            "schema-session",
            "summary",
            Some("src/lib.rs".to_string()),
            None,
            100,
            20,
        ))?;
        set_metadata(&store.connection, "custom_setting", "preserved")?;
        Ok(())
    }

    /// Build a representative Rust file node for store tests.
    fn test_file_node(path: &str, hash: &str) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::File,
            parent_path: normalized_parent(path),
            extension: Some(".rs".to_string()),
            language: Some("rust".to_string()),
            size_bytes: Some(12),
            mtime_ns: Some(10),
            content_hash: Some(hash.to_string()),
        }
    }

    /// Build one persisted symbol with configurable retained payload.
    fn batch_test_symbol(
        path: &str,
        name: &str,
        line_start: usize,
        documentation_bytes: usize,
    ) -> CodeSymbol {
        CodeSymbol {
            path: path.to_string(),
            language: Some("rust".to_string()),
            name: name.to_string(),
            kind: SymbolKind::Function,
            signature: format!("fn {name}()"),
            exported: false,
            documentation: Some("d".repeat(documentation_bytes)),
            line_start,
            line_end: line_start.saturating_add(1),
            source_selector: None,
            parent: None,
            parser: ParserKind::TreeSitter,
            detail: Some("function_item".to_string()),
        }
    }

    /// Assert the exact selector on the heading row and the legacy row remains absent.
    fn require_symbol_source_selectors(
        symbols: &[CodeSymbol],
        expected: Option<SymbolSourceSelector>,
        context: &str,
    ) -> Result<(), Box<dyn Error>> {
        let heading = symbols
            .iter()
            .find(|symbol| symbol.name == "Overview")
            .ok_or_else(|| io::Error::other(format!("{context} omitted the heading symbol")))?;
        require_eq(&heading.source_selector, &expected, context)?;
        if let Some(compatibility) = symbols.iter().find(|symbol| symbol.name == "Compatibility") {
            require_eq(
                &compatibility.source_selector,
                &None,
                &format!("{context} changed the selector-free symbol"),
            )?;
        }
        Ok(())
    }

    /// Exercise every DB-owned symbol row decoder against one persisted selector.
    fn verify_symbol_source_selector_read_paths(
        store: &AtlasStore,
        path: &str,
        expected: SymbolSourceSelector,
    ) -> Result<(), Box<dyn Error>> {
        require_symbol_source_selectors(
            &store.load_symbols(Some(path), None, 10)?,
            Some(expected),
            "symbol list",
        )?;
        require_symbol_source_selectors(
            &store.load_symbols_by_kinds(path, &[SymbolKind::Heading], 10)?,
            Some(expected),
            "kind-filtered symbols",
        )?;
        require_symbol_source_selectors(
            &store.load_symbols_by_names(&["Overview".to_string()])?,
            Some(expected),
            "name-filtered symbols",
        )?;
        let named = store
            .load_symbol_by_name(path, "Overview")?
            .ok_or_else(|| io::Error::other("exact-name symbol lookup omitted the heading"))?;
        require_eq(
            &named.source_selector,
            &Some(expected),
            "single exact-name symbol",
        )?;
        require_symbol_source_selectors(
            &store.load_symbols_by_exact_file_and_name(path, "Overview")?,
            Some(expected),
            "exact-file-and-name symbols",
        )?;
        require_symbol_source_selectors(
            &store
                .load_symbols_for_paths_bounded(
                    &[path.to_string()],
                    SymbolBatchReadBudget::new(1, 10, MAX_SYMBOL_BATCH_DECODED_BYTES)?,
                    None,
                )?
                .rows,
            Some(expected),
            "bounded exact-path symbols",
        )?;
        let classified =
            store.load_classified_symbols(Some(path), None, ContentSelection::Documentation, 10)?;
        require(
            classified
                .iter()
                .all(|row| row.classification == ContentClassification::Documentation),
            "classified symbol read changed the owning file classification",
        )?;
        require_symbol_source_selectors(
            &classified
                .into_iter()
                .map(|row| row.symbol)
                .collect::<Vec<_>>(),
            Some(expected),
            "classified symbols",
        )?;
        let graphs = store.load_symbol_graphs_for_paths(&[path.to_string()])?;
        require_eq(&graphs.len(), &1, "reconstructed graph count")?;
        require_symbol_source_selectors(
            &graphs[0].symbols,
            Some(expected),
            "reconstructed symbol graph",
        )?;
        Ok(())
    }

    /// Copy one selected queue candidate into the public conditional-write contract.
    fn conditional_purpose_request(
        task: &str,
        candidate: &PurposeCurationCandidate,
        purpose: &str,
    ) -> PurposeConditionalApplyRequest {
        PurposeConditionalApplyRequest {
            task: task.to_string(),
            path: candidate.node.node.path.clone(),
            work_key: candidate.work_key.clone(),
            state_token: candidate.state_token.clone(),
            purpose: purpose.to_string(),
        }
    }

    /// Replace every source-derived projection used by publication tests.
    fn write_test_projection(store: &mut AtlasStore, label: &str) -> DbResult<()> {
        let path = "src/lib.rs";
        let hash = format!("hash-{label}");
        store.replace_scan(&[test_file_node(path, &hash)])?;
        store.replace_file_texts_for_paths(
            &[path.to_string()],
            &[IndexedFileText {
                path: path.to_string(),
                content_hash: Some(hash),
                byte_count: label.len(),
                line_count: 1,
                content: label.to_string(),
            }],
        )?;
        store.replace_symbol_graph(&SymbolGraph {
            path: path.to_string(),
            language: Some("rust".to_string()),
            parser: ParserKind::TreeSitter,
            symbols: vec![CodeSymbol {
                path: path.to_string(),
                language: Some("rust".to_string()),
                name: format!("{label}_symbol"),
                kind: SymbolKind::Function,
                signature: format!("fn {label}_symbol()"),
                exported: true,
                documentation: None,
                line_start: 1,
                line_end: 1,
                source_selector: None,
                parent: None,
                parser: ParserKind::TreeSitter,
                detail: Some("function_item".to_string()),
            }],
            relations: vec![SymbolRelation {
                path: path.to_string(),
                source_name: format!("{label}_symbol"),
                target_name: format!("{label}_target"),
                kind: RelationKind::Calls,
                line: 1,
                context: format!("{label}_target();"),
                parser: ParserKind::TreeSitter,
            }],
        })?;
        store.set_node_summary(path, &format!("{label} summary"))?;
        Ok(())
    }

    /// Assert that one snapshot exposes a coherent projection generation.
    fn require_test_projection(
        store: &AtlasStore,
        generation: u64,
        label: &str,
    ) -> Result<(), Box<dyn Error>> {
        let path = "src/lib.rs";
        let publication = store
            .index_publication()?
            .ok_or_else(|| io::Error::other("publication missing"))?;
        require_eq(
            &publication.generation,
            &IndexGeneration::new(generation),
            "publication generation",
        )?;
        let node = store
            .load_node_by_path(path)?
            .ok_or_else(|| io::Error::other("projection node missing"))?;
        require_eq(
            &node.node.content_hash,
            &Some(format!("hash-{label}")),
            "projection node hash",
        )?;
        require_eq(
            &node.summary,
            &Some(format!("{label} summary")),
            "projection node summary",
        )?;
        let text = store
            .load_file_text(path)?
            .ok_or_else(|| io::Error::other("projection text missing"))?;
        require_eq(&text.content, &label.to_string(), "projection text")?;
        let symbols = store.load_symbols(Some(path), None, 10)?;
        require_eq(
            &symbols.first().map(|symbol| symbol.name.as_str()),
            &Some(format!("{label}_symbol")).as_deref(),
            "projection symbol",
        )?;
        let relations = store.load_symbol_relations(Some(path), None, 10)?;
        require_eq(
            &relations
                .first()
                .map(|relation| relation.target_name.as_str()),
            &Some(format!("{label}_target")).as_deref(),
            "projection relation",
        )?;
        let metadata = store
            .load_source_parse_metadata(path)?
            .ok_or_else(|| io::Error::other("projection parse metadata missing"))?;
        require_eq(&metadata.symbol_count, &1, "projection symbol metadata")?;
        require_eq(&metadata.relation_count, &1, "projection relation metadata")?;
        Ok(())
    }

    /// Assert the connection settings required for every production writer.
    fn require_writable_connection_profile(connection: &Connection) -> Result<(), Box<dyn Error>> {
        let foreign_keys =
            connection.pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0))?;
        require_eq(&foreign_keys, &1, "writable foreign-key enforcement")?;
        require_wal_profile(connection)?;
        let synchronous =
            connection.pragma_query_value(None, "synchronous", |row| row.get::<_, i64>(0))?;
        require_eq(&synchronous, &2, "writable FULL synchronous mode")?;
        require_busy_timeout(connection)
    }

    /// Assert the connection settings required for every production reader.
    fn require_read_connection_profile(connection: &Connection) -> Result<(), Box<dyn Error>> {
        let query_only =
            connection.pragma_query_value(None, "query_only", |row| row.get::<_, i64>(0))?;
        require_eq(&query_only, &1, "read query-only mode")?;
        require_wal_profile(connection)?;
        require_busy_timeout(connection)
    }

    /// Assert that a connection observes the selected durable journal mode.
    fn require_wal_profile(connection: &Connection) -> Result<(), Box<dyn Error>> {
        let journal_mode =
            connection.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))?;
        require_eq(
            &journal_mode.to_ascii_lowercase(),
            &"wal".to_string(),
            "WAL journal mode",
        )
    }

    /// Assert the bounded contention wait shared by ordinary connections.
    fn require_busy_timeout(connection: &Connection) -> Result<(), Box<dyn Error>> {
        let busy_timeout =
            connection.pragma_query_value(None, "busy_timeout", |row| row.get::<_, i64>(0))?;
        require_eq(
            &u128::try_from(busy_timeout)?,
            &SQLITE_BUSY_TIMEOUT.as_millis(),
            "bounded connection busy timeout",
        )
    }

    /// Build a representative folder node for store tests.
    fn test_folder_node(path: &str) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::Folder,
            parent_path: normalized_parent(path),
            extension: None,
            language: None,
            size_bytes: None,
            mtime_ns: Some(10),
            content_hash: None,
        }
    }

    /// Return the default low-cost purpose review query used by agent linting.
    fn low_query() -> HealthQuery {
        HealthQuery {
            start_index: 0,
            limit: 20,
            category: Some(CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED.to_string()),
            severity: Some(Severity::Warning),
            path_prefix: Some(".".to_string()),
            summary_only: false,
            scope: HealthScope::purpose_default(),
        }
    }

    /// Collect health finding paths in returned order.
    fn health_paths(page: &HealthFindingsPage) -> Vec<&str> {
        page.findings
            .iter()
            .map(|finding| finding.path.as_str())
            .collect()
    }

    /// Require a test condition without panicking.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }

    /// Require two test values to be equal without panicking.
    fn require_eq<T>(actual: &T, expected: &T, label: &str) -> Result<(), Box<dyn Error>>
    where
        T: Debug + PartialEq,
    {
        if actual == expected {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "{label} mismatch: expected {expected:?}, got {actual:?}"
            ))
            .into())
        }
    }
}
