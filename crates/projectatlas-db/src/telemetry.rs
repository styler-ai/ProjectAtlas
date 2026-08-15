//! Persist bounded token telemetry in the authoritative project database.

use crate::{DbError, DbResult};
use projectatlas_core::graph::ProjectInstanceId;
use projectatlas_core::telemetry::{
    TOKEN_AVERAGE_POLICY_OVERFLOW_EVIDENCE, TOKEN_BASELINE_DIRECTORY_WALK, TokenAccountingTotals,
    TokenBucketOverview, TokenOverview, TokenTrendPeriod, TokenTrendReport, TokenTrendWindow,
    UsageDetailAvailability, UsageEvent, UsageInstanceId, UsageInstanceOwner,
    average_modeled_baseline_tokens,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Version of the persisted telemetry retention contract.
const POLICY_VERSION: u32 = 1;
/// Version of the deterministic logical-byte accounting contract.
const LOGICAL_BYTE_VERSION: u32 = 1;
/// Reserved dimension value that owns aggregated overflow detail.
const OVERFLOW_DIMENSION: &str = "<overflow>";
/// Persisted state for a runtime that may still accept events.
const INSTANCE_ACTIVE: &str = "active";
/// Persisted state for a runtime that completed cleanly.
const INSTANCE_SEALED: &str = "sealed";
/// Persisted state for a runtime retired by bounded maintenance.
const INSTANCE_EXPIRED: &str = "expired";
/// Dedupe scope whose contribution belongs only to one event.
const DEDUPE_SCOPE_EVENT: &str = "event";
/// Number of seconds in a UTC reporting day.
const SECONDS_PER_DAY: i64 = 86_400;
/// Stable owner reported when `SQLite` rejects an aggregate addition overflow.
const AGGREGATE_COUNTER_FIELD: &str = "aggregate_counter";
/// Domain separator for bounded representations of predecessor telemetry text.
const LEGACY_TEXT_HASH_DOMAIN: &[u8] = b"projectatlas:legacy-telemetry-text:v1\0";
/// Aggregate rows written directly by alias-routed MCP telemetry.
const WORKTREE_USAGE_ROUTED: &str = "routed";
/// Aggregate rows replaced by monotonic local-database synchronization.
const WORKTREE_USAGE_SYNCHRONIZED: &str = "synchronized";
/// Maximum encoded in-memory size of one local aggregate transfer.
const MAX_WORKTREE_USAGE_SNAPSHOT_BYTES: usize = 32 * 1_024 * 1_024;

/// Capacity policy used while a modeled baseline is active.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BaselineAdmission {
    /// Enforce every runtime baseline row and witness-byte bound.
    BoundedRuntime,
    /// Preserve exact predecessor totals before sealing upgrade-owned baselines.
    SupportedUpgrade,
}

/// Select the reporting dimension admitted for one event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DimensionAdmission {
    /// Derive the normalized dimension from the validated event.
    Event,
    /// Route predecessor detail that cannot be represented exactly to overflow.
    Overflow,
}

/// Detail that became unavailable while bounding one predecessor event.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct LegacyDetailLoss {
    /// Raw event fields were replaced by bounded opaque representations.
    raw: bool,
    /// At least one reporting dimension could not be retained exactly.
    dimension: bool,
    /// The predecessor caller label could not be retained exactly.
    label: bool,
}

/// Bind one aggregate value to the common aggregate table parameter order.
macro_rules! aggregate_params {
    ($first:expr, $second:expr, $value:expr) => {
        params![
            $first,
            $second,
            $value.calls,
            $value.estimated_without,
            $value.estimated_with,
            $value.observed_without,
            $value.observed_with,
            $value.modeled_without,
            $value.modeled_with,
            $value.deduped_modeled_without,
            $value.deduped_modeled_with,
            $value.repeated_baselines,
            $value.observed_file_read_replacements,
            $value.modeled_file_reads_avoided,
        ]
    };
}

/// Bind one aggregate value to the common daily table parameter order.
macro_rules! daily_aggregate_params {
    ($first:expr, $day:expr, $dimension:expr, $value:expr) => {
        params![
            $first,
            $day,
            $dimension,
            $value.calls,
            $value.estimated_without,
            $value.estimated_with,
            $value.observed_without,
            $value.observed_with,
            $value.modeled_without,
            $value.modeled_with,
            $value.deduped_modeled_without,
            $value.deduped_modeled_with,
            $value.repeated_baselines,
            $value.observed_file_read_replacements,
            $value.modeled_file_reads_avoided,
        ]
    };
}

/// Bind one worktree aggregate value to its explicit source and time scope.
macro_rules! worktree_aggregate_params {
    ($registration:expr, $source:expr, $day:expr, $dimension:expr, $value:expr) => {
        params![
            $registration,
            $source,
            $day,
            $dimension,
            $value.calls,
            $value.estimated_without,
            $value.estimated_with,
            $value.observed_without,
            $value.observed_with,
            $value.modeled_without,
            $value.modeled_with,
            $value.deduped_modeled_without,
            $value.deduped_modeled_with,
            $value.repeated_baselines,
            $value.observed_file_read_replacements,
            $value.modeled_file_reads_avoided,
        ]
    };
}

/// Generate one opaque compatibility identity without panicking on entropy failure.
pub(crate) fn generate_usage_instance_id() -> DbResult<UsageInstanceId> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_source| DbError::TelemetryIdentityUnavailable)?;
    UsageInstanceId::from_bytes(bytes).map_err(Into::into)
}

/// Retention limits for telemetry inside one authoritative database.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct TelemetryRetentionPolicy {
    /// Maximum retained raw events.
    pub max_raw_rows: usize,
    /// Maximum logical bytes represented by retained raw events.
    pub max_raw_logical_bytes: usize,
    /// Maximum raw-event age in seconds.
    pub max_raw_age_seconds: u64,
    /// Maximum normalized reporting dimensions, including generic overflow.
    ///
    /// One additional reserved directory-walk overflow row may retain the average-policy
    /// discriminator after this bound is reached.
    pub max_dimensions: usize,
    /// Maximum active runtime instances.
    pub max_active_instances: usize,
    /// Maximum retained runtime instances.
    pub max_retained_instances: usize,
    /// Maximum retained label tombstones.
    pub max_label_tombstones: usize,
    /// Maximum retained instance tombstones.
    pub max_instance_tombstones: usize,
    /// Maximum caller labels with retained detail state in the database.
    pub max_retained_labels: usize,
    /// Maximum baselines admitted for one active instance.
    pub max_baselines_per_instance: usize,
    /// Maximum active baseline rows across the database.
    pub max_active_baseline_rows: usize,
    /// Maximum logical witness bytes across active baselines.
    pub max_baseline_logical_bytes: usize,
    /// Maximum retained daily aggregate rows.
    pub max_daily_rows: usize,
    /// Maximum rows touched by one maintenance pass.
    pub prune_batch_rows: usize,
    /// Maximum caller-label bytes.
    pub max_label_bytes: usize,
    /// Maximum command bytes.
    pub max_command_bytes: usize,
    /// Maximum path bytes.
    pub max_path_bytes: usize,
    /// Maximum query bytes.
    pub max_query_bytes: usize,
    /// Maximum normalized dimension bytes.
    pub max_dimension_bytes: usize,
    /// Maximum modeled-baseline witness bytes.
    pub max_baseline_witness_bytes: usize,
    /// Active-instance idle timeout in seconds.
    pub max_active_idle_seconds: u64,
    /// Permitted future clock skew in seconds.
    pub future_clock_tolerance_seconds: u64,
    /// Writes between passive checkpoint attempts.
    pub checkpoint_write_interval: usize,
    /// Retained daily trend history.
    pub retained_trend_days: u64,
    /// Retained sealed-instance history.
    pub retained_instance_seconds: u64,
    /// Retained caller-label history.
    pub retained_label_seconds: u64,
    /// Retained tombstone history.
    pub retained_tombstone_seconds: u64,
}

impl Default for TelemetryRetentionPolicy {
    fn default() -> Self {
        Self {
            max_raw_rows: 50_000,
            max_raw_logical_bytes: 64 * 1_024 * 1_024,
            max_raw_age_seconds: 30 * 24 * 60 * 60,
            max_dimensions: 128,
            max_active_instances: 64,
            max_retained_instances: 4_096,
            max_label_tombstones: 1_024,
            max_instance_tombstones: 4_096,
            max_retained_labels: 256,
            max_baselines_per_instance: 1_024,
            max_active_baseline_rows: 16_384,
            max_baseline_logical_bytes: 16 * 1_024 * 1_024,
            max_daily_rows: 100_000,
            prune_batch_rows: 512,
            max_label_bytes: 128,
            max_command_bytes: 96,
            max_path_bytes: 4_096,
            max_query_bytes: 4_096,
            max_dimension_bytes: 96,
            max_baseline_witness_bytes: 1_024,
            max_active_idle_seconds: 24 * 60 * 60,
            future_clock_tolerance_seconds: 5 * 60,
            checkpoint_write_interval: 1_024,
            retained_trend_days: 400,
            retained_instance_seconds: 90 * 24 * 60 * 60,
            retained_label_seconds: 365 * 24 * 60 * 60,
            retained_tombstone_seconds: 730 * 24 * 60 * 60,
        }
    }
}

impl TelemetryRetentionPolicy {
    /// Validate that every hard limit can make forward progress.
    ///
    /// # Errors
    ///
    /// Returns an error for zero or contradictory limits.
    pub fn validate(self) -> DbResult<Self> {
        for (field, value) in [
            ("max_raw_rows", self.max_raw_rows),
            ("max_raw_logical_bytes", self.max_raw_logical_bytes),
            ("max_dimensions", self.max_dimensions),
            ("max_active_instances", self.max_active_instances),
            ("max_retained_instances", self.max_retained_instances),
            ("max_label_tombstones", self.max_label_tombstones),
            ("max_instance_tombstones", self.max_instance_tombstones),
            ("max_retained_labels", self.max_retained_labels),
            (
                "max_baselines_per_instance",
                self.max_baselines_per_instance,
            ),
            ("max_active_baseline_rows", self.max_active_baseline_rows),
            (
                "max_baseline_logical_bytes",
                self.max_baseline_logical_bytes,
            ),
            ("max_daily_rows", self.max_daily_rows),
            ("prune_batch_rows", self.prune_batch_rows),
            ("max_label_bytes", self.max_label_bytes),
            ("max_command_bytes", self.max_command_bytes),
            ("max_path_bytes", self.max_path_bytes),
            ("max_query_bytes", self.max_query_bytes),
            ("max_dimension_bytes", self.max_dimension_bytes),
            (
                "max_baseline_witness_bytes",
                self.max_baseline_witness_bytes,
            ),
            ("checkpoint_write_interval", self.checkpoint_write_interval),
        ] {
            if value == 0 {
                return Err(DbError::TelemetryLimitInvalid { field, value });
            }
        }
        if self.max_raw_age_seconds == 0
            || self.max_active_idle_seconds == 0
            || self.retained_trend_days == 0
            || self.retained_instance_seconds == 0
            || self.retained_label_seconds == 0
            || self.retained_tombstone_seconds == 0
            || self.max_retained_instances < self.max_active_instances
            || self.max_daily_rows < 2
        {
            return Err(DbError::TelemetryLimitInvalid {
                field: "telemetry_retention_policy",
                value: 0,
            });
        }
        Ok(self)
    }
}

/// Spill cleanup state for the selected storage design.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpillCleanupState {
    /// Telemetry has no secondary spill database or file owner.
    NotApplicable,
}

/// Persisted passive-checkpoint lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryCheckpointState {
    /// The telemetry write threshold has not yet requested a checkpoint.
    NotDue,
    /// Every WAL frame visible to the passive attempt was checkpointed.
    Completed,
    /// A reader or writer prevented the passive attempt from completing.
    Busy,
    /// `SQLite` rejected the passive attempt.
    Error,
}

impl TelemetryCheckpointState {
    /// Return the stable `SQLite` representation.
    const fn as_str(self) -> &'static str {
        match self {
            Self::NotDue => "not_due",
            Self::Completed => "completed",
            Self::Busy => "busy",
            Self::Error => "error",
        }
    }

    /// Decode one checked `SQLite` value.
    fn from_str(value: &str) -> DbResult<Self> {
        match value {
            "not_due" => Ok(Self::NotDue),
            "completed" => Ok(Self::Completed),
            "busy" => Ok(Self::Busy),
            "error" => Ok(Self::Error),
            _ => Err(DbError::InvalidEnum {
                field: "usage_retention_state.checkpoint_state",
                value: value.to_string(),
            }),
        }
    }
}

/// Planner-statistics maintenance policy currently owned by `ProjectAtlas`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannerStatisticsPolicy {
    /// No `ProjectAtlas` lifecycle currently runs `ANALYZE` or `PRAGMA optimize`.
    NotConfigured,
}

/// Availability of `SQLite` planner statistics in the selected database.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannerStatisticsState {
    /// No `sqlite_stat1` table has been initialized.
    NotInitialized,
    /// `SQLite` planner statistics are present.
    Available,
}

/// Content-free retention and page-lifecycle state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TelemetryRetentionState {
    /// Retention policy format version.
    pub policy_version: u32,
    /// Logical-byte accounting version.
    pub logical_byte_version: u32,
    /// Retained raw rows.
    pub raw_rows: usize,
    /// Maximum retained raw rows.
    pub max_raw_rows: usize,
    /// Maximum retained raw age in seconds.
    pub max_raw_age_seconds: u64,
    /// Retained raw logical bytes.
    pub raw_logical_bytes: usize,
    /// Maximum retained raw logical bytes.
    pub max_raw_logical_bytes: usize,
    /// Active baseline rows.
    pub baseline_rows: usize,
    /// Maximum baselines for one active instance.
    pub max_baselines_per_instance: usize,
    /// Maximum active baseline rows.
    pub max_active_baseline_rows: usize,
    /// Active baseline witness bytes.
    pub baseline_logical_bytes: usize,
    /// Maximum active baseline witness bytes.
    pub max_baseline_logical_bytes: usize,
    /// Normalized dimension rows.
    pub dimension_rows: usize,
    /// Maximum normalized dimensions including overflow.
    pub max_dimensions: usize,
    /// Retained runtime-instance rows.
    pub instance_rows: usize,
    /// Active runtime-instance rows for the selected project.
    pub active_instance_rows: usize,
    /// Maximum active runtime instances per project.
    pub max_active_instances: usize,
    /// Maximum retained runtime-instance rows.
    pub max_retained_instances: usize,
    /// Retained caller-label state rows in the authoritative database.
    pub retained_label_rows: usize,
    /// Maximum retained caller-label state rows in the authoritative database.
    pub max_retained_labels: usize,
    /// Retained daily aggregate rows.
    pub daily_rows: usize,
    /// Maximum retained daily aggregate rows.
    pub max_daily_rows: usize,
    /// Daily trend retention in days.
    pub retained_trend_days: u64,
    /// Retained label tombstones.
    pub label_tombstone_rows: usize,
    /// Maximum retained label tombstones.
    pub max_label_tombstones: usize,
    /// Retained runtime-instance tombstones.
    pub instance_tombstone_rows: usize,
    /// Maximum retained runtime-instance tombstones.
    pub max_instance_tombstones: usize,
    /// Lifetime pruned raw rows.
    pub pruned_raw_rows: usize,
    /// Lifetime pruned runtime instances.
    pub pruned_instance_rows: usize,
    /// Lifetime evicted tombstones.
    pub evicted_tombstones: usize,
    /// Whether more bounded maintenance is pending.
    pub maintenance_pending: bool,
    /// Fixed maximum rows touched by one maintenance category pass.
    pub prune_batch_rows: usize,
    /// Writes accumulated since the last passive checkpoint attempt.
    pub writes_since_checkpoint: usize,
    /// Writes between passive checkpoint attempts.
    pub checkpoint_write_interval: usize,
    /// Epoch of the most recent checkpoint attempt.
    pub last_checkpoint_epoch: u64,
    /// Oldest retained raw-event epoch, when detail exists.
    pub oldest_retained_epoch: Option<u64>,
    /// Whether anomalous wall-clock movement was observed.
    pub clock_anomaly: bool,
    /// Spill cleanup state; always not applicable for the one-database design.
    pub spill_cleanup: SpillCleanupState,
    /// Most recent `ProjectAtlas` passive-checkpoint state.
    pub checkpoint_state: TelemetryCheckpointState,
    /// Connection-local `SQLite` automatic-checkpoint threshold in WAL pages.
    pub wal_autocheckpoint_pages: usize,
    /// Reusable database pages observed live by the reporting connection.
    pub freelist_pages: usize,
    /// Total database pages observed live by the reporting connection.
    pub page_count: usize,
    /// `SQLite` database page size in bytes.
    pub page_size: usize,
    /// Active journal mode reported by `SQLite`.
    pub journal_mode: String,
    /// Active synchronous mode reported by `SQLite`.
    pub synchronous_mode: String,
    /// Busy timeout observed on the reporting connection in milliseconds.
    pub connection_busy_timeout_ms: u64,
    /// Busy timeout required for ordinary read/write connections in milliseconds.
    pub normal_busy_timeout_ms: u64,
    /// Busy timeout required for best-effort telemetry writers in milliseconds.
    pub telemetry_busy_timeout_ms: u64,
    /// Planner-statistics lifecycle policy.
    pub statistics_policy: PlannerStatisticsPolicy,
    /// Current planner-statistics availability.
    pub statistics_state: PlannerStatisticsState,
}

/// Normalized aggregate dimension persisted once and referenced by identifier.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DimensionValues {
    /// Token-savings evidence bucket.
    token_savings_bucket: String,
    /// Token-count provider.
    provider: String,
    /// Token-count model.
    model: String,
    /// Tokenizer backend.
    tokenizer_backend: String,
    /// Accuracy classification.
    accuracy: String,
    /// Baseline scenario kind.
    baseline_kind: String,
    /// Baseline confidence.
    confidence: String,
    /// Accounting layer.
    accounting_layer: String,
    /// Estimate method.
    estimate_method: String,
    /// Baseline denominator kind.
    denominator_kind: String,
    /// Dedupe scope.
    dedupe_scope: String,
    /// Whether this is the reserved overflow dimension.
    overflow: bool,
}

impl DimensionValues {
    /// Normalize the reporting dimensions carried by one event.
    fn from_event(event: &UsageEvent) -> Self {
        Self {
            token_savings_bucket: event.token_savings_bucket.clone(),
            provider: event.provider.clone(),
            model: event.model.clone(),
            tokenizer_backend: event.tokenizer_backend.clone(),
            accuracy: event.accuracy.clone(),
            baseline_kind: event.baseline_kind.clone(),
            confidence: event.confidence.clone(),
            accounting_layer: event.report_accounting_layer().to_string(),
            estimate_method: event.estimate_method.clone(),
            denominator_kind: event.report_denominator_kind().to_string(),
            dedupe_scope: event.report_dedupe_scope().to_string(),
            overflow: false,
        }
    }

    /// Construct a reserved overflow dimension while retaining the average-policy discriminator.
    fn overflow(denominator_kind: &str) -> Self {
        Self {
            token_savings_bucket: OVERFLOW_DIMENSION.to_string(),
            provider: OVERFLOW_DIMENSION.to_string(),
            model: OVERFLOW_DIMENSION.to_string(),
            tokenizer_backend: OVERFLOW_DIMENSION.to_string(),
            accuracy: OVERFLOW_DIMENSION.to_string(),
            baseline_kind: OVERFLOW_DIMENSION.to_string(),
            confidence: OVERFLOW_DIMENSION.to_string(),
            accounting_layer: OVERFLOW_DIMENSION.to_string(),
            estimate_method: OVERFLOW_DIMENSION.to_string(),
            denominator_kind: if denominator_kind == TOKEN_BASELINE_DIRECTORY_WALK {
                TOKEN_BASELINE_DIRECTORY_WALK.to_string()
            } else {
                OVERFLOW_DIMENSION.to_string()
            },
            dedupe_scope: OVERFLOW_DIMENSION.to_string(),
            overflow: true,
        }
    }
}

/// Exact nonnegative aggregate components stored in `SQLite` integers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct AggregateCounters {
    /// Number of represented events.
    calls: i64,
    /// Total estimated tokens without `ProjectAtlas`.
    estimated_without: i64,
    /// Total estimated tokens emitted with `ProjectAtlas`.
    estimated_with: i64,
    /// Observed baseline tokens.
    observed_without: i64,
    /// Observed emitted tokens.
    observed_with: i64,
    /// Modeled baseline tokens before deduplication.
    modeled_without: i64,
    /// Modeled emitted tokens before deduplication.
    modeled_with: i64,
    /// Positive component of signed deduped modeled savings.
    deduped_modeled_without: i64,
    /// Negative component of signed deduped modeled savings.
    deduped_modeled_with: i64,
    /// Repeated modeled-baseline observations.
    repeated_baselines: i64,
    /// Observed full-file reads replaced by bounded output.
    observed_file_read_replacements: i64,
    /// Modeled file reads avoided by narrowing.
    modeled_file_reads_avoided: i64,
}

/// One bounded normalized aggregate row carried between local atlas databases.
#[derive(Clone, Debug, Eq, PartialEq)]
struct WorktreeUsageSnapshotRow {
    /// `-1` for lifetime totals, otherwise the UTC day epoch.
    day_epoch: i64,
    /// Source-database normalized dimension identity.
    dimension_id: i64,
    /// Exact nonnegative counters.
    counters: AggregateCounters,
}

/// Opaque bounded aggregate transfer from one exact worktree atlas.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeUsageSnapshot {
    /// Exact source atlas identity.
    project_instance_id: ProjectInstanceId,
    /// Monotonic aggregate revision captured with the rows.
    revision: u64,
    /// Referenced normalized dimensions keyed by source identifier.
    dimensions: BTreeMap<i64, DimensionValues>,
    /// Lifetime and retained daily aggregate rows.
    rows: Vec<WorktreeUsageSnapshotRow>,
    /// Deterministic bounded logical transfer size.
    logical_bytes: usize,
}

impl WorktreeUsageSnapshot {
    /// Return the exact source atlas identity.
    #[must_use]
    pub const fn project_instance_id(&self) -> ProjectInstanceId {
        self.project_instance_id
    }

    /// Return the monotonic source aggregate revision.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Return the bounded aggregate row count.
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Return deterministic logical transfer bytes.
    #[must_use]
    pub const fn logical_bytes(&self) -> usize {
        self.logical_bytes
    }
}

/// Result of one monotonic worktree aggregate synchronization attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeUsageSyncState {
    /// A strictly newer snapshot replaced only the selected origin.
    Synchronized,
    /// The same or an older snapshot left the accepted origin unchanged.
    Current,
}

/// Persisted singleton counter selected for a bounded exact update.
#[derive(Clone, Copy, Debug)]
enum RetentionCounter {
    /// Retained raw event rows.
    RawRows,
    /// Logical bytes represented by retained raw events.
    RawLogicalBytes,
    /// Active modeled-baseline rows.
    BaselineRows,
    /// Witness bytes represented by active modeled baselines.
    BaselineLogicalBytes,
    /// Normalized aggregate dimension rows.
    DimensionRows,
    /// Retained runtime-instance rows.
    InstanceRows,
    /// Retained caller-label rows.
    LabelRows,
    /// Retained global and instance daily aggregate rows.
    DailyRows,
    /// Retained caller-label tombstones.
    LabelTombstoneRows,
    /// Retained runtime-instance tombstones.
    InstanceTombstoneRows,
}

impl RetentionCounter {
    /// Return the fixed query that reads this counter.
    const fn select_sql(self) -> &'static str {
        match self {
            Self::RawRows => "SELECT raw_rows FROM usage_retention_state WHERE singleton = 1",
            Self::RawLogicalBytes => {
                "SELECT raw_logical_bytes FROM usage_retention_state WHERE singleton = 1"
            }
            Self::BaselineRows => {
                "SELECT baseline_rows FROM usage_retention_state WHERE singleton = 1"
            }
            Self::BaselineLogicalBytes => {
                "SELECT baseline_logical_bytes FROM usage_retention_state WHERE singleton = 1"
            }
            Self::DimensionRows => {
                "SELECT dimension_rows FROM usage_retention_state WHERE singleton = 1"
            }
            Self::InstanceRows => {
                "SELECT instance_rows FROM usage_retention_state WHERE singleton = 1"
            }
            Self::LabelRows => "SELECT label_rows FROM usage_retention_state WHERE singleton = 1",
            Self::DailyRows => "SELECT daily_rows FROM usage_retention_state WHERE singleton = 1",
            Self::LabelTombstoneRows => {
                "SELECT label_tombstone_rows FROM usage_retention_state WHERE singleton = 1"
            }
            Self::InstanceTombstoneRows => {
                "SELECT instance_tombstone_rows FROM usage_retention_state WHERE singleton = 1"
            }
        }
    }

    /// Return the fixed statement that replaces this counter.
    const fn update_sql(self) -> &'static str {
        match self {
            Self::RawRows => "UPDATE usage_retention_state SET raw_rows = ?1 WHERE singleton = 1",
            Self::RawLogicalBytes => {
                "UPDATE usage_retention_state SET raw_logical_bytes = ?1 WHERE singleton = 1"
            }
            Self::BaselineRows => {
                "UPDATE usage_retention_state SET baseline_rows = ?1 WHERE singleton = 1"
            }
            Self::BaselineLogicalBytes => {
                "UPDATE usage_retention_state SET baseline_logical_bytes = ?1 WHERE singleton = 1"
            }
            Self::DimensionRows => {
                "UPDATE usage_retention_state SET dimension_rows = ?1 WHERE singleton = 1"
            }
            Self::InstanceRows => {
                "UPDATE usage_retention_state SET instance_rows = ?1 WHERE singleton = 1"
            }
            Self::LabelRows => {
                "UPDATE usage_retention_state SET label_rows = ?1 WHERE singleton = 1"
            }
            Self::DailyRows => {
                "UPDATE usage_retention_state SET daily_rows = ?1 WHERE singleton = 1"
            }
            Self::LabelTombstoneRows => {
                "UPDATE usage_retention_state SET label_tombstone_rows = ?1 WHERE singleton = 1"
            }
            Self::InstanceTombstoneRows => {
                "UPDATE usage_retention_state SET instance_tombstone_rows = ?1 WHERE singleton = 1"
            }
        }
    }

    /// Return the stable field name used by typed diagnostics.
    const fn field(self) -> &'static str {
        match self {
            Self::RawRows => "raw_rows",
            Self::RawLogicalBytes => "raw_logical_bytes",
            Self::BaselineRows => "baseline_rows",
            Self::BaselineLogicalBytes => "baseline_logical_bytes",
            Self::DimensionRows => "dimension_rows",
            Self::InstanceRows => "instance_rows",
            Self::LabelRows => "label_rows",
            Self::DailyRows => "daily_rows",
            Self::LabelTombstoneRows => "label_tombstone_rows",
            Self::InstanceTombstoneRows => "instance_tombstone_rows",
        }
    }
}

impl AggregateCounters {
    /// Reject corrupt or caller-forged negative components.
    fn validate_nonnegative(self) -> DbResult<()> {
        for (field, value) in [
            ("calls", self.calls),
            ("estimated_without", self.estimated_without),
            ("estimated_with", self.estimated_with),
            ("observed_without", self.observed_without),
            ("observed_with", self.observed_with),
            ("modeled_without", self.modeled_without),
            ("modeled_with", self.modeled_with),
            ("deduped_modeled_without", self.deduped_modeled_without),
            ("deduped_modeled_with", self.deduped_modeled_with),
            ("repeated_baselines", self.repeated_baselines),
            (
                "observed_file_read_replacements",
                self.observed_file_read_replacements,
            ),
            (
                "modeled_file_reads_avoided",
                self.modeled_file_reads_avoided,
            ),
        ] {
            if value < 0 {
                return Err(DbError::TelemetryIntegerOverflow { field });
            }
        }
        Ok(())
    }

    /// Add every component while rejecting integer overflow.
    fn checked_add(self, other: Self) -> DbResult<Self> {
        macro_rules! add {
            ($field:ident) => {
                self.$field
                    .checked_add(other.$field)
                    .ok_or(DbError::TelemetryIntegerOverflow {
                        field: stringify!($field),
                    })?
            };
        }
        Ok(Self {
            calls: add!(calls),
            estimated_without: add!(estimated_without),
            estimated_with: add!(estimated_with),
            observed_without: add!(observed_without),
            observed_with: add!(observed_with),
            modeled_without: add!(modeled_without),
            modeled_with: add!(modeled_with),
            deduped_modeled_without: add!(deduped_modeled_without),
            deduped_modeled_with: add!(deduped_modeled_with),
            repeated_baselines: add!(repeated_baselines),
            observed_file_read_replacements: add!(observed_file_read_replacements),
            modeled_file_reads_avoided: add!(modeled_file_reads_avoided),
        })
    }
}

/// Seed telemetry state after fresh-schema creation.
pub(crate) fn initialize_empty_storage(connection: &Connection) -> DbResult<()> {
    let policy = TelemetryRetentionPolicy::default().validate()?;
    ensure_overflow_dimension(connection, OVERFLOW_DIMENSION)?;
    refresh_retention_state(connection, policy, now_epoch_seconds()?, 0, 0, 0, 0)
}

/// Reset copied source telemetry to the exact empty-storage contract during hydration.
pub(crate) fn reset_usage_storage_for_hydration(connection: &Connection) -> DbResult<()> {
    connection.execute_batch(
        "DELETE FROM usage_events;
         DELETE FROM usage_instances;
         DELETE FROM usage_global_aggregates;
         DELETE FROM usage_daily_aggregates;
         DELETE FROM usage_labels;
         DELETE FROM usage_label_tombstones;
         DELETE FROM usage_instance_tombstones;
         DELETE FROM usage_bucket_dimensions;
         UPDATE usage_retention_state SET
             raw_rows = 0,
             raw_logical_bytes = 0,
             baseline_rows = 0,
             baseline_logical_bytes = 0,
             dimension_rows = 0,
             instance_rows = 0,
             label_rows = 0,
             daily_rows = 0,
             label_tombstone_rows = 0,
             instance_tombstone_rows = 0,
             pruned_raw_rows = 0,
             pruned_instance_rows = 0,
             evicted_tombstones = 0,
             writes_since_checkpoint = 0,
             last_maintenance_epoch = 0,
             last_checkpoint_epoch = 0,
             oldest_retained_epoch = NULL,
             raw_detail_complete = 1,
             dimension_detail_complete = 1,
             label_history_complete = 1,
             maintenance_pending = 0,
             clock_anomaly = 0,
             spill_state = 'not_applicable',
             checkpoint_state = 'not_due';",
    )?;
    initialize_empty_storage(connection)
}

/// Convert schema-10 raw usage inside the outer schema transaction.
pub(crate) fn migrate_legacy_usage(connection: &Connection) -> DbResult<()> {
    let policy = TelemetryRetentionPolicy::default().validate()?;
    let (project, _) = crate::project_identity::ensure_project_identity(connection)?;
    ensure_overflow_dimension(connection, OVERFLOW_DIMENSION)?;
    let mut statement = connection.prepare(
        "SELECT session_id, command, path, query,
                estimated_tokens_without_projectatlas,
                estimated_tokens_with_projectatlas, estimated_tokens_saved,
                token_savings_bucket, provider, model, tokenizer_backend,
                accuracy, baseline_kind, confidence, calculation_trace,
                accounting_layer, estimate_method, denominator_kind,
                baseline_identity, baseline_fingerprint, dedupe_scope,
                created_at, unixepoch(created_at)
         FROM usage_events_legacy
         ORDER BY session_id, id",
    )?;
    let mut rows = statement.query([])?;
    let mut previous: Option<UsageInstanceId> = None;
    while let Some(row) = rows.next()? {
        let label = row.get::<_, String>(0)?;
        let created_text = row.get::<_, String>(21)?;
        let created_at = row
            .get::<_, Option<i64>>(22)?
            .ok_or_else(|| DbError::InvalidEnum {
                field: "usage_events_legacy.created_at",
                value: created_text,
            })?;
        let event = UsageEvent {
            session_id: label.clone(),
            command: row.get(1)?,
            path: row.get(2)?,
            query: row.get(3)?,
            estimated_tokens_without_projectatlas: row.get(4)?,
            estimated_tokens_with_projectatlas: row.get(5)?,
            estimated_tokens_saved: row.get(6)?,
            token_savings_bucket: row.get(7)?,
            provider: row.get(8)?,
            model: row.get(9)?,
            tokenizer_backend: row.get(10)?,
            accuracy: row.get(11)?,
            baseline_kind: row.get(12)?,
            confidence: row.get(13)?,
            calculation_trace: row.get(14)?,
            accounting_layer: row.get(15)?,
            estimate_method: row.get(16)?,
            denominator_kind: row.get(17)?,
            baseline_identity: row.get(18)?,
            baseline_fingerprint: row.get(19)?,
            dedupe_scope: row.get(20)?,
        };
        let instance = migrated_instance_id(project, &label)?;
        if previous != Some(instance) {
            if let Some(previous) = previous {
                seal_usage_instance_for_project(connection, project, previous, created_at)?;
            }
            previous = Some(instance);
        }
        let (event, detail_loss) = normalize_legacy_event(event, policy)?;
        validate_event(&event, policy)?;
        record_usage_at(
            connection,
            project,
            instance,
            UsageInstanceOwner::MigratedLegacy,
            None,
            false,
            &event,
            policy,
            created_at,
            false,
            BaselineAdmission::SupportedUpgrade,
            if detail_loss.dimension {
                DimensionAdmission::Overflow
            } else {
                DimensionAdmission::Event
            },
        )?;
        mark_legacy_detail_loss(connection, project, instance, &event, detail_loss)?;
    }
    drop(rows);
    drop(statement);
    if let Some(instance) = previous {
        seal_usage_instance_for_project(connection, project, instance, now_epoch_seconds()?)?;
    }
    reconcile_retention_counters(connection)?;
    converge_retention(connection, project, policy, now_epoch_seconds()?)
}

/// Persist one event against the project identity captured by the adapter.
pub(crate) fn record_usage_for_project(
    connection: &Connection,
    project: ProjectInstanceId,
    instance_id: UsageInstanceId,
    owner: UsageInstanceOwner,
    worktree_registration_id: Option<i64>,
    event: &UsageEvent,
    policy: TelemetryRetentionPolicy,
    seal_after_record: bool,
) -> DbResult<()> {
    crate::project_identity::require_bound_project_identity(connection, project)?;
    let policy = policy.validate()?;
    validate_event(event, policy)?;
    record_usage_at(
        connection,
        project,
        instance_id,
        owner,
        worktree_registration_id,
        true,
        event,
        policy,
        now_epoch_seconds()?,
        seal_after_record,
        BaselineAdmission::BoundedRuntime,
        DimensionAdmission::Event,
    )
}

#[allow(clippy::too_many_arguments)]
/// Apply one validated event inside its caller-owned write transaction.
fn record_usage_at(
    connection: &Connection,
    project: ProjectInstanceId,
    instance_id: UsageInstanceId,
    owner: UsageInstanceOwner,
    worktree_registration_id: Option<i64>,
    track_aggregate_revision: bool,
    event: &UsageEvent,
    policy: TelemetryRetentionPolicy,
    now: i64,
    seal_after_record: bool,
    baseline_admission: BaselineAdmission,
    dimension_admission: DimensionAdmission,
) -> DbResult<()> {
    expire_idle_instances(connection, project, instance_id, policy, now)?;
    let instance_exists = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM usage_instances
             WHERE project_instance_id = ?1 AND runtime_instance_id = ?2
         )",
        params![
            project.as_bytes().as_slice(),
            instance_id.as_bytes().as_slice()
        ],
        |row| row.get::<_, i64>(0),
    )?;
    let reserve_instances = usize::from(instance_exists == 0);
    let (pruned_instances, instance_raw_rows) =
        prune_instances_once(connection, policy, now, reserve_instances)?;
    let instance_row_id = ensure_active_instance(
        connection,
        project,
        instance_id,
        owner,
        event_label(event),
        policy,
        now,
    )?;
    if track_aggregate_revision || worktree_registration_id.is_some() {
        bind_worktree_origin(connection, instance_row_id, worktree_registration_id)?;
    }
    ensure_label(connection, project, event_label(event), policy, now)?;
    let dimension = match dimension_admission {
        DimensionAdmission::Event => DimensionValues::from_event(event),
        DimensionAdmission::Overflow => DimensionValues::overflow(event.report_denominator_kind()),
    };
    let dimension_id = ensure_dimension(connection, &dimension, policy)?;
    let logical_bytes = logical_event_bytes(event, event_label(event))?;
    let delta = aggregate_delta(
        connection,
        instance_row_id,
        event,
        policy,
        baseline_admission,
    )?;
    insert_raw_event(
        connection,
        instance_row_id,
        dimension_id,
        event,
        now,
        logical_bytes,
    )?;
    prune_daily_once(connection, policy, now)?;
    apply_aggregates(
        connection,
        project,
        instance_row_id,
        dimension_id,
        now,
        delta,
        policy,
    )?;
    if let Some(registration_id) = worktree_registration_id {
        upsert_routed_worktree_aggregates(
            connection,
            registration_id,
            dimension_id,
            now,
            delta,
            policy,
        )?;
    }
    if track_aggregate_revision {
        increment_aggregate_revision(connection, project, delta.calls)?;
    }
    touch_instance(connection, instance_row_id, now, policy)?;
    if seal_after_record {
        seal_usage_instance_for_project(connection, project, instance_id, now)?;
    }
    let pruned_raw = prune_raw_once(connection, policy, now)?
        .checked_add(instance_raw_rows)
        .ok_or(DbError::TelemetryIntegerOverflow {
            field: "pruned_raw_rows",
        })?;
    let evicted_tombstones = prune_tombstones_once(connection, policy, now)?;
    prune_labels_once(connection, policy, now)?;
    refresh_retention_state(
        connection,
        policy,
        now,
        pruned_raw,
        pruned_instances,
        evicted_tombstones,
        1,
    )?;
    Ok(())
}

/// Seal one cleanly completed runtime instance.
pub(crate) fn seal_usage_instance(
    connection: &Connection,
    instance_id: UsageInstanceId,
) -> DbResult<()> {
    let project = current_project(connection)?;
    seal_usage_instance_for_project(connection, project, instance_id, now_epoch_seconds()?)
}

/// Seal one active instance under an explicitly captured project identity.
fn seal_usage_instance_for_project(
    connection: &Connection,
    project: ProjectInstanceId,
    instance_id: UsageInstanceId,
    now: i64,
) -> DbResult<()> {
    let changed = connection.execute(
        "UPDATE usage_instances
         SET state = ?3, sealed_at_epoch = CASE
                 WHEN last_seen_at_epoch > ?4 THEN last_seen_at_epoch ELSE ?4 END
         WHERE project_instance_id = ?1 AND runtime_instance_id = ?2 AND state = ?5",
        params![
            project.as_bytes().as_slice(),
            instance_id.as_bytes().as_slice(),
            INSTANCE_SEALED,
            now,
            INSTANCE_ACTIVE,
        ],
    )?;
    if changed == 0 {
        return Err(DbError::TelemetryInstanceInactive);
    }
    let row_id = connection.query_row(
        "SELECT instance_row_id FROM usage_instances
         WHERE project_instance_id = ?1 AND runtime_instance_id = ?2",
        params![
            project.as_bytes().as_slice(),
            instance_id.as_bytes().as_slice()
        ],
        |row| row.get::<_, i64>(0),
    )?;
    delete_instance_baselines(connection, row_id)?;
    Ok(())
}

/// Seal active instances before a project-identity rotation.
pub(crate) fn seal_project_usage_instances(
    connection: &Connection,
    project: ProjectInstanceId,
) -> DbResult<usize> {
    crate::project_identity::require_bound_project_identity(connection, project)?;
    let now = now_epoch_seconds()?;
    let project_bytes = project.as_bytes();
    let (baseline_rows, baseline_bytes) = connection.query_row(
        "SELECT COUNT(*), COALESCE(SUM(b.witness_logical_bytes), 0)
         FROM usage_instance_baselines AS b
         JOIN usage_instances AS i USING(instance_row_id)
         WHERE i.project_instance_id = ?1 AND i.state = ?2",
        params![project_bytes.as_slice(), INSTANCE_ACTIVE],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )?;
    connection.execute(
        "DELETE FROM usage_instance_baselines
         WHERE instance_row_id IN (
             SELECT instance_row_id FROM usage_instances
             WHERE project_instance_id = ?1 AND state = ?2
         )",
        params![project_bytes.as_slice(), INSTANCE_ACTIVE],
    )?;
    decrement_retention_counter(
        connection,
        RetentionCounter::BaselineRows,
        count_usize("project_baseline_rows", baseline_rows)?,
    )?;
    decrement_retention_counter(
        connection,
        RetentionCounter::BaselineLogicalBytes,
        count_usize("project_baseline_logical_bytes", baseline_bytes)?,
    )?;
    connection
        .execute(
            "UPDATE usage_instances
             SET state = ?2,
                 sealed_at_epoch = CASE
                     WHEN last_seen_at_epoch > ?3 THEN last_seen_at_epoch ELSE ?3 END
             WHERE project_instance_id = ?1 AND state = ?4",
            params![
                project_bytes.as_slice(),
                INSTANCE_SEALED,
                now,
                INSTANCE_ACTIVE,
            ],
        )
        .map_err(Into::into)
}

/// Run due post-commit maintenance only for the adapter's captured project identity.
pub(crate) fn maintain_after_commit_for_project(
    connection: &Connection,
    expected_root: Option<&str>,
    project: ProjectInstanceId,
    policy: TelemetryRetentionPolicy,
) -> DbResult<()> {
    maintain_after_commit_for_project_with_checkpoint(
        connection,
        expected_root,
        project,
        policy,
        |connection| Ok(passive_checkpoint_state(connection)),
    )
}

/// Run due maintenance with an explicit passive-checkpoint boundary.
fn maintain_after_commit_for_project_with_checkpoint(
    connection: &Connection,
    expected_root: Option<&str>,
    project: ProjectInstanceId,
    policy: TelemetryRetentionPolicy,
    checkpoint: impl FnOnce(&Connection) -> DbResult<TelemetryCheckpointState>,
) -> DbResult<()> {
    crate::project_identity::require_bound_project_identity(connection, project)?;
    let policy = policy.validate()?;
    let checkpoint_start_writes = count_usize(
        "writes_since_checkpoint",
        connection.query_row(
            "SELECT writes_since_checkpoint FROM usage_retention_state WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )?,
    )?;
    if checkpoint_start_writes < policy.checkpoint_write_interval {
        return Ok(());
    }
    crate::schema::validate_active_binding(connection, expected_root, Some(project))?;
    let checkpoint_data_version =
        connection.query_row("PRAGMA data_version", [], |row| row.get::<_, i64>(0))?;
    let state = checkpoint(connection)?;
    crate::with_validated_write_transaction(
        connection,
        expected_root,
        Some(project),
        |transaction| {
            let current_data_version =
                transaction.query_row("PRAGMA data_version", [], |row| row.get::<_, i64>(0))?;
            let current_writes = count_usize(
                "writes_since_checkpoint",
                transaction.query_row(
                    "SELECT writes_since_checkpoint
                     FROM usage_retention_state WHERE singleton = 1",
                    [],
                    |row| row.get::<_, i64>(0),
                )?,
            )?;
            let retained_writes = writes_after_checkpoint_attempt(
                state,
                checkpoint_start_writes,
                current_writes,
                current_data_version == checkpoint_data_version,
            );
            let now = now_epoch_seconds()?;
            let (raw_rows, raw_bytes, old_raw) = raw_pressure(transaction, policy, now)?;
            let retention_pending = raw_rows > policy.max_raw_rows
                || raw_bytes > policy.max_raw_logical_bytes
                || old_raw > 0
                || retention_counter(transaction, RetentionCounter::InstanceRows)?
                    > policy.max_retained_instances
                || retention_counter(transaction, RetentionCounter::LabelRows)?
                    > policy.max_retained_labels
                || retention_counter(transaction, RetentionCounter::DailyRows)?
                    > policy.max_daily_rows
                || retention_counter(transaction, RetentionCounter::LabelTombstoneRows)?
                    > policy.max_label_tombstones
                || retention_counter(transaction, RetentionCounter::InstanceTombstoneRows)?
                    > policy.max_instance_tombstones
                || aged_maintenance_pending(transaction, policy, now)?;
            let completed = state == TelemetryCheckpointState::Completed;
            transaction.execute(
                "UPDATE usage_retention_state
                 SET writes_since_checkpoint = ?1,
                     last_checkpoint_epoch = ?2,
                     checkpoint_state = ?3,
                     maintenance_pending = ?4
                 WHERE singleton = 1",
                params![
                    to_i64("writes_since_checkpoint", retained_writes)?,
                    now,
                    state.as_str(),
                    i64::from(
                        !completed
                            || retention_pending
                            || retained_writes >= policy.checkpoint_write_interval
                    ),
                ],
            )?;
            Ok(())
        },
    )
}

/// Run one passive checkpoint and classify its bounded `SQLite` result.
fn passive_checkpoint_state(connection: &Connection) -> TelemetryCheckpointState {
    let result = connection.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    });
    match result {
        Ok((0, log_frames, checkpointed_frames)) if checkpointed_frames == log_frames => {
            TelemetryCheckpointState::Completed
        }
        Ok(_) => TelemetryCheckpointState::Busy,
        Err(_) => TelemetryCheckpointState::Error,
    }
}

/// Preserve writes that committed after a checkpoint attempt began.
const fn writes_after_checkpoint_attempt(
    state: TelemetryCheckpointState,
    checkpoint_start_writes: usize,
    current_writes: usize,
    data_version_unchanged: bool,
) -> usize {
    if matches!(state, TelemetryCheckpointState::Completed) && data_version_unchanged {
        current_writes.saturating_sub(checkpoint_start_writes)
    } else {
        current_writes
    }
}

/// Return content-free bounded telemetry state.
pub(crate) fn retention_state(connection: &Connection) -> DbResult<TelemetryRetentionState> {
    let project = current_project(connection)?;
    retention_state_for_project(connection, project)
}

/// Read retention state scoped to the selected project identity.
fn retention_state_for_project(
    connection: &Connection,
    project: ProjectInstanceId,
) -> DbResult<TelemetryRetentionState> {
    crate::project_identity::require_bound_project_identity(connection, project)?;
    let row = connection.query_row(
        "SELECT policy_version, logical_byte_version, raw_rows, raw_logical_bytes,
                baseline_rows, baseline_logical_bytes, dimension_rows, instance_rows,
                daily_rows, label_tombstone_rows, instance_tombstone_rows,
                pruned_raw_rows, pruned_instance_rows, evicted_tombstones,
                maintenance_pending, clock_anomaly, spill_state, checkpoint_state
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
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, i64>(10)?,
                row.get::<_, i64>(11)?,
                row.get::<_, i64>(12)?,
                row.get::<_, i64>(13)?,
                row.get::<_, i64>(14)?,
                row.get::<_, i64>(15)?,
                row.get::<_, String>(16)?,
                row.get::<_, String>(17)?,
            ))
        },
    )?;
    if row.16 != "not_applicable" {
        return Err(DbError::InvalidEnum {
            field: "usage_retention_state.spill_state",
            value: row.16,
        });
    }
    let policy = TelemetryRetentionPolicy::default().validate()?;
    let lifecycle = connection.query_row(
        "SELECT writes_since_checkpoint, last_checkpoint_epoch, oldest_retained_epoch
         FROM usage_retention_state WHERE singleton = 1",
        [],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        },
    )?;
    let active_instances = connection.query_row(
        "SELECT COUNT(*) FROM usage_instances
         WHERE project_instance_id = ?1 AND state = ?2",
        params![project.as_bytes().as_slice(), INSTANCE_ACTIVE],
        |row| row.get::<_, i64>(0),
    )?;
    let retained_labels = retention_counter(connection, RetentionCounter::LabelRows)?;
    let journal_mode =
        connection.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))?;
    let synchronous = connection.query_row("PRAGMA synchronous", [], |row| row.get::<_, i64>(0))?;
    let busy_timeout =
        connection.query_row("PRAGMA busy_timeout", [], |row| row.get::<_, i64>(0))?;
    let statistics = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'sqlite_stat1'
         )",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    let checkpoint_state = TelemetryCheckpointState::from_str(&row.17)?;
    let wal_autocheckpoint =
        connection.query_row("PRAGMA wal_autocheckpoint", [], |row| row.get::<_, i64>(0))?;
    let normal_busy_timeout_ms =
        duration_millis("normal_busy_timeout_ms", crate::SQLITE_BUSY_TIMEOUT)?;
    let telemetry_busy_timeout_ms = duration_millis(
        "telemetry_busy_timeout_ms",
        crate::SQLITE_TELEMETRY_BUSY_TIMEOUT,
    )?;
    Ok(TelemetryRetentionState {
        policy_version: count_u32("policy_version", row.0)?,
        logical_byte_version: count_u32("logical_byte_version", row.1)?,
        raw_rows: count_usize("raw_rows", row.2)?,
        max_raw_rows: policy.max_raw_rows,
        max_raw_age_seconds: policy.max_raw_age_seconds,
        raw_logical_bytes: count_usize("raw_logical_bytes", row.3)?,
        max_raw_logical_bytes: policy.max_raw_logical_bytes,
        baseline_rows: count_usize("baseline_rows", row.4)?,
        max_baselines_per_instance: policy.max_baselines_per_instance,
        max_active_baseline_rows: policy.max_active_baseline_rows,
        baseline_logical_bytes: count_usize("baseline_logical_bytes", row.5)?,
        max_baseline_logical_bytes: policy.max_baseline_logical_bytes,
        dimension_rows: count_usize("dimension_rows", row.6)?,
        max_dimensions: policy.max_dimensions,
        instance_rows: count_usize("instance_rows", row.7)?,
        active_instance_rows: count_usize("active_instance_rows", active_instances)?,
        max_active_instances: policy.max_active_instances,
        max_retained_instances: policy.max_retained_instances,
        retained_label_rows: retained_labels,
        max_retained_labels: policy.max_retained_labels,
        daily_rows: count_usize("daily_rows", row.8)?,
        max_daily_rows: policy.max_daily_rows,
        retained_trend_days: policy.retained_trend_days,
        label_tombstone_rows: count_usize("label_tombstone_rows", row.9)?,
        max_label_tombstones: policy.max_label_tombstones,
        instance_tombstone_rows: count_usize("instance_tombstone_rows", row.10)?,
        max_instance_tombstones: policy.max_instance_tombstones,
        pruned_raw_rows: count_usize("pruned_raw_rows", row.11)?,
        pruned_instance_rows: count_usize("pruned_instance_rows", row.12)?,
        evicted_tombstones: count_usize("evicted_tombstones", row.13)?,
        maintenance_pending: bool_from_sql("maintenance_pending", row.14)?,
        prune_batch_rows: policy.prune_batch_rows,
        writes_since_checkpoint: count_usize("writes_since_checkpoint", lifecycle.0)?,
        checkpoint_write_interval: policy.checkpoint_write_interval,
        last_checkpoint_epoch: count_u64("last_checkpoint_epoch", lifecycle.1)?,
        oldest_retained_epoch: lifecycle
            .2
            .map(|value| count_u64("oldest_retained_epoch", value))
            .transpose()?,
        clock_anomaly: bool_from_sql("clock_anomaly", row.15)?,
        spill_cleanup: SpillCleanupState::NotApplicable,
        checkpoint_state,
        wal_autocheckpoint_pages: count_usize("wal_autocheckpoint", wal_autocheckpoint)?,
        freelist_pages: count_usize(
            "freelist_pages",
            pragma_count(connection, "freelist_count")?,
        )?,
        page_count: count_usize("page_count", pragma_count(connection, "page_count")?)?,
        page_size: count_usize("page_size", pragma_count(connection, "page_size")?)?,
        journal_mode,
        synchronous_mode: synchronous_mode(synchronous)?.to_string(),
        connection_busy_timeout_ms: count_u64("busy_timeout", busy_timeout)?,
        normal_busy_timeout_ms,
        telemetry_busy_timeout_ms,
        statistics_policy: PlannerStatisticsPolicy::NotConfigured,
        statistics_state: if statistics == 0 {
            PlannerStatisticsState::NotInitialized
        } else {
            PlannerStatisticsState::Available
        },
    })
}

/// Load retained raw usage events.
pub(crate) fn usage_events(
    connection: &Connection,
    caller_label: Option<&str>,
) -> DbResult<Vec<UsageEvent>> {
    let project = current_project(connection)?;
    usage_events_for_project(connection, project, caller_label)
}

/// Load retained raw events for one captured project and optional label.
fn usage_events_for_project(
    connection: &Connection,
    project: ProjectInstanceId,
    caller_label: Option<&str>,
) -> DbResult<Vec<UsageEvent>> {
    crate::project_identity::require_bound_project_identity(connection, project)?;
    let sql = if caller_label.is_some() {
        raw_event_select("AND i.caller_label = ?2")
    } else {
        raw_event_select("")
    };
    let mut statement = connection.prepare(&sql)?;
    let mut rows = if let Some(label) = caller_label {
        statement.query(params![project.as_bytes().as_slice(), label])?
    } else {
        statement.query([project.as_bytes().as_slice()])?
    };
    let mut events = Vec::new();
    while let Some(row) = rows.next()? {
        events.push(map_usage_event(row)?);
    }
    Ok(events)
}

/// Export one bounded normalized aggregate snapshot from the exact local atlas.
pub(crate) fn export_worktree_usage_snapshot(
    connection: &Connection,
) -> DbResult<WorktreeUsageSnapshot> {
    let owned_snapshot = connection
        .is_autocommit()
        .then(|| {
            rusqlite::Transaction::new_unchecked(
                connection,
                rusqlite::TransactionBehavior::Deferred,
            )
        })
        .transpose()?;
    let connection = owned_snapshot.as_deref().unwrap_or(connection);
    let project = current_project(connection)?;
    crate::project_identity::require_bound_project_identity(connection, project)?;
    let policy = TelemetryRetentionPolicy::default().validate()?;
    let revision = connection
        .query_row(
            "SELECT revision FROM usage_aggregate_revisions
             WHERE project_instance_id = ?1",
            [project.as_bytes().as_slice()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0);
    let revision =
        u64::try_from(revision).map_err(|_source| DbError::TelemetryIntegerOverflow {
            field: "usage_aggregate_revisions.revision",
        })?;
    let maximum_rows = policy
        .max_daily_rows
        .checked_add(policy.max_dimensions)
        .and_then(|value| value.checked_add(2))
        .ok_or(DbError::TelemetryIntegerOverflow {
            field: "worktree_snapshot_rows",
        })?;
    let query_limit = to_i64("worktree_snapshot_rows", maximum_rows.saturating_add(1))?;
    let mut statement = connection.prepare(
        "SELECT -1 AS day_epoch, dimension_id,
                calls, estimated_without, estimated_with, observed_without,
                observed_with, modeled_without, modeled_with,
                deduped_modeled_without, deduped_modeled_with, repeated_baselines,
                observed_file_read_replacements, modeled_file_reads_avoided
         FROM usage_global_aggregates
         WHERE project_instance_id = ?1
         UNION ALL
         SELECT day_epoch, dimension_id,
                calls, estimated_without, estimated_with, observed_without,
                observed_with, modeled_without, modeled_with,
                deduped_modeled_without, deduped_modeled_with, repeated_baselines,
                observed_file_read_replacements, modeled_file_reads_avoided
         FROM usage_daily_aggregates
         WHERE project_instance_id = ?1
         ORDER BY day_epoch, dimension_id
         LIMIT ?2",
    )?;
    let rows = statement
        .query_map(params![project.as_bytes().as_slice(), query_limit], |row| {
            Ok(WorktreeUsageSnapshotRow {
                day_epoch: row.get(0)?,
                dimension_id: row.get(1)?,
                counters: read_counters_offset(row, 2).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Integer,
                        Box::new(error),
                    )
                })?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() > maximum_rows {
        return Err(DbError::WorktreeTelemetrySnapshotLimit {
            resource: "rows",
            limit: maximum_rows,
            observed: rows.len(),
        });
    }
    let referenced_dimensions = rows
        .iter()
        .map(|row| row.dimension_id)
        .collect::<BTreeSet<_>>();
    let mut dimensions = BTreeMap::new();
    let mut dimension_statement = connection.prepare(
        "SELECT dimension_id,
                token_savings_bucket, provider, model, tokenizer_backend,
                accuracy, baseline_kind, confidence, accounting_layer,
                estimate_method, denominator_kind, dedupe_scope, overflow
         FROM usage_bucket_dimensions
         ORDER BY dimension_id",
    )?;
    let mut dimension_rows = dimension_statement.query([])?;
    while let Some(row) = dimension_rows.next()? {
        let dimension_id = row.get::<_, i64>(0)?;
        if referenced_dimensions.contains(&dimension_id) {
            dimensions.insert(dimension_id, read_dimension(row, 1)?);
        }
    }
    drop(dimension_rows);
    drop(dimension_statement);
    drop(statement);
    if dimensions.len() != referenced_dimensions.len() {
        return Err(DbError::WorktreeRegistrationRow {
            reason: "aggregate snapshot references a missing dimension",
        });
    }
    let logical_bytes = validate_worktree_usage_snapshot(&dimensions, &rows, policy)?;
    let snapshot = WorktreeUsageSnapshot {
        project_instance_id: project,
        revision,
        dimensions,
        rows,
        logical_bytes,
    };
    if let Some(snapshot) = owned_snapshot {
        snapshot.commit()?;
    }
    Ok(snapshot)
}

/// Replace one registration's synchronized aggregate rows when the revision advances.
pub(crate) fn synchronize_worktree_usage_snapshot(
    connection: &Connection,
    registration_id: i64,
    snapshot: &WorktreeUsageSnapshot,
) -> DbResult<WorktreeUsageSyncState> {
    let policy = TelemetryRetentionPolicy::default().validate()?;
    let (project_bytes, accepted_revision) = connection.query_row(
        "SELECT project_instance_id, accepted_telemetry_revision
         FROM worktree_registrations
         WHERE registration_id = ?1 AND state = 'active'",
        [registration_id],
        |row| Ok((row.get::<_, Option<Vec<u8>>>(0)?, row.get::<_, i64>(1)?)),
    )?;
    let Some(project_bytes) = project_bytes else {
        return Err(DbError::WorktreeTelemetryProjectMismatch { registration_id });
    };
    let project_bytes: [u8; 16] =
        project_bytes
            .try_into()
            .map_err(|value: Vec<u8>| DbError::InvalidBlobLength {
                field: "worktree_registrations.project_instance_id",
                expected: 16,
                found: value.len(),
            })?;
    let project = ProjectInstanceId::from_bytes(project_bytes).map_err(DbError::from)?;
    if project != snapshot.project_instance_id {
        return Err(DbError::WorktreeTelemetryProjectMismatch { registration_id });
    }
    let accepted_revision =
        u64::try_from(accepted_revision).map_err(|_source| DbError::WorktreeRegistrationRow {
            reason: "negative accepted telemetry revision",
        })?;
    if snapshot.revision <= accepted_revision {
        return Ok(WorktreeUsageSyncState::Current);
    }
    let logical_bytes =
        validate_worktree_usage_snapshot(&snapshot.dimensions, &snapshot.rows, policy)?;
    if logical_bytes != snapshot.logical_bytes {
        return Err(DbError::WorktreeRegistrationRow {
            reason: "aggregate snapshot logical-byte contract changed",
        });
    }
    let incoming_daily_rows = snapshot
        .rows
        .iter()
        .filter(|row| row.day_epoch >= 0)
        .count();
    let other_daily_rows = connection.query_row(
        "SELECT COUNT(*) FROM worktree_usage_aggregates
         WHERE day_epoch >= 0
           AND NOT (registration_id = ?1 AND source_kind = ?2)",
        params![registration_id, WORKTREE_USAGE_SYNCHRONIZED],
        |row| row.get::<_, i64>(0),
    )?;
    let projected_daily_rows = count_usize("worktree_daily_rows", other_daily_rows)?
        .checked_add(incoming_daily_rows)
        .ok_or(DbError::TelemetryIntegerOverflow {
            field: "worktree_daily_rows",
        })?;
    if projected_daily_rows > policy.max_daily_rows {
        return Err(DbError::WorktreeTelemetrySnapshotLimit {
            resource: "daily_rows",
            limit: policy.max_daily_rows,
            observed: projected_daily_rows,
        });
    }
    let mut target_dimensions = BTreeMap::new();
    for (source_id, dimension) in &snapshot.dimensions {
        target_dimensions.insert(*source_id, ensure_dimension(connection, dimension, policy)?);
    }
    connection.execute(
        "DELETE FROM worktree_usage_aggregates
         WHERE registration_id = ?1 AND source_kind = ?2",
        params![registration_id, WORKTREE_USAGE_SYNCHRONIZED],
    )?;
    let mut insert = connection.prepare_cached(
        "INSERT INTO worktree_usage_aggregates(
            registration_id, source_kind, day_epoch, dimension_id,
            calls, estimated_without, estimated_with, observed_without,
            observed_with, modeled_without, modeled_with,
            deduped_modeled_without, deduped_modeled_with, repeated_baselines,
            observed_file_read_replacements, modeled_file_reads_avoided
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
    )?;
    for row in &snapshot.rows {
        let dimension_id =
            target_dimensions
                .get(&row.dimension_id)
                .ok_or(DbError::WorktreeRegistrationRow {
                    reason: "aggregate snapshot dimension mapping is incomplete",
                })?;
        insert.execute(worktree_aggregate_params!(
            registration_id,
            WORKTREE_USAGE_SYNCHRONIZED,
            row.day_epoch,
            dimension_id,
            row.counters
        ))?;
    }
    let revision =
        i64::try_from(snapshot.revision).map_err(|_source| DbError::TelemetryIntegerOverflow {
            field: "usage_aggregate_revisions.revision",
        })?;
    let updated = connection.execute(
        "UPDATE worktree_registrations
         SET accepted_telemetry_revision = ?2
         WHERE registration_id = ?1 AND state = 'active'
           AND accepted_telemetry_revision < ?2",
        params![registration_id, revision],
    )?;
    if updated != 1 {
        return Err(DbError::WorktreeRegistrationRow {
            reason: "aggregate snapshot revision changed during synchronization",
        });
    }
    Ok(WorktreeUsageSyncState::Synchronized)
}

/// Validate snapshot counters, dimensions, days, row bounds, and logical size.
fn validate_worktree_usage_snapshot(
    dimensions: &BTreeMap<i64, DimensionValues>,
    rows: &[WorktreeUsageSnapshotRow],
    policy: TelemetryRetentionPolicy,
) -> DbResult<usize> {
    let maximum_rows = policy
        .max_daily_rows
        .checked_add(policy.max_dimensions)
        .and_then(|value| value.checked_add(2))
        .ok_or(DbError::TelemetryIntegerOverflow {
            field: "worktree_snapshot_rows",
        })?;
    if rows.len() > maximum_rows {
        return Err(DbError::WorktreeTelemetrySnapshotLimit {
            resource: "rows",
            limit: maximum_rows,
            observed: rows.len(),
        });
    }
    let mut logical_bytes = 24usize;
    for dimension in dimensions.values() {
        validate_dimension_values(dimension, policy.max_dimension_bytes)?;
        logical_bytes = logical_bytes
            .checked_add(9)
            .and_then(|value| value.checked_add(dimension_logical_bytes(dimension)))
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "worktree_snapshot_bytes",
            })?;
    }
    for row in rows {
        if row.day_epoch < -1 || !dimensions.contains_key(&row.dimension_id) {
            return Err(DbError::WorktreeRegistrationRow {
                reason: "aggregate snapshot row has an invalid day or dimension",
            });
        }
        row.counters.validate_nonnegative()?;
        logical_bytes =
            logical_bytes
                .checked_add(112)
                .ok_or(DbError::TelemetryIntegerOverflow {
                    field: "worktree_snapshot_bytes",
                })?;
    }
    if logical_bytes > MAX_WORKTREE_USAGE_SNAPSHOT_BYTES {
        return Err(DbError::WorktreeTelemetrySnapshotLimit {
            resource: "logical_bytes",
            limit: MAX_WORKTREE_USAGE_SNAPSHOT_BYTES,
            observed: logical_bytes,
        });
    }
    Ok(logical_bytes)
}

/// Validate one normalized dimension at the transfer trust boundary.
fn validate_dimension_values(dimension: &DimensionValues, maximum_bytes: usize) -> DbResult<()> {
    for (field, value) in [
        (
            "token_savings_bucket",
            dimension.token_savings_bucket.as_str(),
        ),
        ("provider", dimension.provider.as_str()),
        ("model", dimension.model.as_str()),
        ("tokenizer_backend", dimension.tokenizer_backend.as_str()),
        ("accuracy", dimension.accuracy.as_str()),
        ("baseline_kind", dimension.baseline_kind.as_str()),
        ("confidence", dimension.confidence.as_str()),
        ("accounting_layer", dimension.accounting_layer.as_str()),
        ("estimate_method", dimension.estimate_method.as_str()),
        ("denominator_kind", dimension.denominator_kind.as_str()),
        ("dedupe_scope", dimension.dedupe_scope.as_str()),
    ] {
        validate_required_text(field, value, maximum_bytes)?;
    }
    Ok(())
}

/// Count normalized dimension text bytes without serializing the snapshot.
fn dimension_logical_bytes(dimension: &DimensionValues) -> usize {
    [
        &dimension.token_savings_bucket,
        &dimension.provider,
        &dimension.model,
        &dimension.tokenizer_backend,
        &dimension.accuracy,
        &dimension.baseline_kind,
        &dimension.confidence,
        &dimension.accounting_layer,
        &dimension.estimate_method,
        &dimension.denominator_kind,
        &dimension.dedupe_scope,
    ]
    .iter()
    .map(|value| value.len())
    .sum()
}

/// Build an exact all-time overview from bounded component aggregates.
pub(crate) fn token_overview(
    connection: &Connection,
    caller_label: Option<&str>,
) -> DbResult<TokenOverview> {
    let project = current_project(connection)?;
    token_overview_for_project(connection, project, caller_label)
}

/// Build the control atlas's combined native-main and synchronized-worktree overview.
pub(crate) fn repository_token_overview(connection: &Connection) -> DbResult<TokenOverview> {
    let project = current_project(connection)?;
    crate::project_identity::require_bound_project_identity(connection, project)?;
    let mut aggregates = load_overview_aggregates(connection, project, None)?;
    let worktree_aggregates = load_worktree_overview_aggregates(connection, None, true)?;
    let has_worktree_aggregates = worktree_aggregates_exist(connection)?;
    aggregates.extend(worktree_aggregates);
    let (buckets, totals, average_policy_complete) = aggregate_report_rows(aggregates)?;
    let mut overview = TokenOverview::from_buckets(buckets);
    overview.apply_accounting_totals(totals);
    if !average_policy_complete {
        overview.average_policy.evidence = TOKEN_AVERAGE_POLICY_OVERFLOW_EVIDENCE.to_string();
    }
    let native_detail = detail_availability(connection, project, None)?;
    overview.set_detail_availability(if has_worktree_aggregates {
        UsageDetailAvailability::Partial
    } else {
        native_detail
    });
    Ok(overview)
}

/// Build exact retained routed plus synchronized totals for one worktree origin.
pub(crate) fn worktree_token_overview(
    connection: &Connection,
    registration_id: i64,
) -> DbResult<TokenOverview> {
    let aggregates = load_worktree_overview_aggregates(connection, Some(registration_id), false)?;
    let (buckets, totals, average_policy_complete) = aggregate_report_rows(aggregates)?;
    let mut overview = TokenOverview::from_buckets(buckets);
    overview.apply_accounting_totals(totals);
    if !average_policy_complete {
        overview.average_policy.evidence = TOKEN_AVERAGE_POLICY_OVERFLOW_EVIDENCE.to_string();
    }
    overview.set_detail_availability(UsageDetailAvailability::Partial);
    Ok(overview)
}

/// Aggregate all-time token totals for one project and optional label.
fn token_overview_for_project(
    connection: &Connection,
    project: ProjectInstanceId,
    caller_label: Option<&str>,
) -> DbResult<TokenOverview> {
    crate::project_identity::require_bound_project_identity(connection, project)?;
    let aggregates = load_overview_aggregates(connection, project, caller_label)?;
    let (buckets, totals, average_policy_complete) = aggregate_report_rows(aggregates)?;
    let mut overview = TokenOverview::from_buckets(buckets);
    overview.apply_accounting_totals(totals);
    if !average_policy_complete {
        overview.average_policy.evidence = TOKEN_AVERAGE_POLICY_OVERFLOW_EVIDENCE.to_string();
    }
    overview.set_detail_availability(detail_availability(connection, project, caller_label)?);
    Ok(overview)
}

/// Build trends from bounded daily component aggregates.
pub(crate) fn token_trends(
    connection: &Connection,
    caller_label: Option<&str>,
    window: TokenTrendWindow,
) -> DbResult<TokenTrendReport> {
    let project = current_project(connection)?;
    token_trends_for_project(connection, project, caller_label, window)
}

/// Build combined native-main and synchronized-worktree trends for the control atlas.
pub(crate) fn repository_token_trends(
    connection: &Connection,
    window: TokenTrendWindow,
) -> DbResult<TokenTrendReport> {
    let project = current_project(connection)?;
    crate::project_identity::require_bound_project_identity(connection, project)?;
    let mut rows = load_daily_aggregates(connection, project, None, window)?;
    let worktree_rows = load_worktree_daily_aggregates(connection, None, true, window)?;
    let has_worktree_rows = worktree_aggregates_exist(connection)?;
    rows.extend(worktree_rows);
    token_trend_report(
        rows,
        None,
        window,
        if has_worktree_rows {
            UsageDetailAvailability::Partial
        } else {
            detail_availability(connection, project, None)?
        },
    )
}

/// Build exact retained routed plus synchronized trends for one worktree origin.
pub(crate) fn worktree_token_trends(
    connection: &Connection,
    registration_id: i64,
    window: TokenTrendWindow,
) -> DbResult<TokenTrendReport> {
    let rows = load_worktree_daily_aggregates(connection, Some(registration_id), false, window)?;
    token_trend_report(rows, None, window, UsageDetailAvailability::Partial)
}

/// Aggregate bounded daily token trends for one project and optional label.
fn token_trends_for_project(
    connection: &Connection,
    project: ProjectInstanceId,
    caller_label: Option<&str>,
    window: TokenTrendWindow,
) -> DbResult<TokenTrendReport> {
    crate::project_identity::require_bound_project_identity(connection, project)?;
    let rows = load_daily_aggregates(connection, project, caller_label, window)?;
    token_trend_report(
        rows,
        caller_label.map(str::to_owned),
        window,
        detail_availability(connection, project, caller_label)?,
    )
}

/// Convert normalized daily rows into one public trend report.
fn token_trend_report(
    rows: Vec<(String, DimensionValues, AggregateCounters)>,
    caller_label: Option<String>,
    window: TokenTrendWindow,
    detail: UsageDetailAvailability,
) -> DbResult<TokenTrendReport> {
    let mut by_period = BTreeMap::<String, BTreeMap<DimensionValues, AggregateCounters>>::new();
    for (period, dimension, counters) in rows {
        let entry = by_period
            .entry(period)
            .or_default()
            .entry(dimension)
            .or_default();
        *entry = entry.checked_add(counters)?;
    }
    let periods = by_period
        .into_iter()
        .map(|(period, rows)| {
            let buckets = rows
                .into_iter()
                .map(|(dimension, counters)| bucket_from_counters(dimension, counters))
                .collect::<DbResult<Vec<_>>>()?;
            Ok(TokenTrendPeriod::from_buckets(period, buckets))
        })
        .collect::<DbResult<Vec<_>>>()?;
    let mut report = TokenTrendReport::new(caller_label, window, periods);
    report.set_detail_availability(detail);
    Ok(report)
}

/// Load the required current project identity from the authoritative database.
fn current_project(connection: &Connection) -> DbResult<ProjectInstanceId> {
    crate::project_identity::load_project_identity(connection)?
        .ok_or(DbError::ProjectInstanceIdentityMissing)
}

/// Derive one deterministic nonzero runtime identity for a legacy caller label.
fn migrated_instance_id(
    project: ProjectInstanceId,
    caller_label: &str,
) -> DbResult<UsageInstanceId> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"projectatlas:migrated-usage-instance:v2\0");
    hasher.update(&project.as_bytes());
    hasher.update(&(caller_label.len() as u64).to_le_bytes());
    hasher.update(caller_label.as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    UsageInstanceId::from_bytes(bytes).map_err(Into::into)
}

/// Bound predecessor text without weakening current-event admission.
fn normalize_legacy_event(
    mut event: UsageEvent,
    policy: TelemetryRetentionPolicy,
) -> DbResult<(UsageEvent, LegacyDetailLoss)> {
    let baseline_identity = event.effective_baseline_identity().into_owned();
    let baseline_fingerprint = event.effective_baseline_fingerprint().into_owned();
    let mut loss = LegacyDetailLoss::default();

    if !event.session_id.is_empty() {
        loss.label = normalize_legacy_required_text(
            "session_id",
            &mut event.session_id,
            policy.max_label_bytes,
        )?;
    }
    loss.raw |=
        normalize_legacy_required_text("command", &mut event.command, policy.max_command_bytes)?;
    loss.raw |= normalize_legacy_optional_text("path", &mut event.path, policy.max_path_bytes)?;
    loss.raw |= normalize_legacy_optional_text("query", &mut event.query, policy.max_query_bytes)?;

    for (field, value) in [
        ("token_savings_bucket", &mut event.token_savings_bucket),
        ("provider", &mut event.provider),
        ("model", &mut event.model),
        ("tokenizer_backend", &mut event.tokenizer_backend),
        ("accuracy", &mut event.accuracy),
        ("baseline_kind", &mut event.baseline_kind),
        ("confidence", &mut event.confidence),
        ("accounting_layer", &mut event.accounting_layer),
        ("estimate_method", &mut event.estimate_method),
        ("denominator_kind", &mut event.denominator_kind),
        ("dedupe_scope", &mut event.dedupe_scope),
    ] {
        loss.dimension |= normalize_legacy_required_text(field, value, policy.max_dimension_bytes)?;
    }

    loss.raw |= normalize_legacy_required_text(
        "calculation_trace",
        &mut event.calculation_trace,
        256.min(policy.max_baseline_witness_bytes),
    )?;
    event.baseline_identity = baseline_identity;
    loss.raw |= normalize_legacy_required_text(
        "baseline_identity",
        &mut event.baseline_identity,
        policy.max_baseline_witness_bytes,
    )?;
    event.baseline_fingerprint = baseline_fingerprint;
    loss.raw |= normalize_legacy_required_text(
        "baseline_fingerprint",
        &mut event.baseline_fingerprint,
        256.min(policy.max_baseline_witness_bytes),
    )?;
    Ok((event, loss))
}

/// Replace an empty or oversized required predecessor value deterministically.
fn normalize_legacy_required_text(
    field: &'static str,
    value: &mut String,
    limit: usize,
) -> DbResult<bool> {
    if !value.is_empty() && value.len() <= limit {
        return Ok(false);
    }
    *value = legacy_text_token(field, value, limit)?;
    Ok(true)
}

/// Replace an oversized optional predecessor value deterministically.
fn normalize_legacy_optional_text(
    field: &'static str,
    value: &mut Option<String>,
    limit: usize,
) -> DbResult<bool> {
    let Some(text) = value else {
        return Ok(false);
    };
    if text.len() <= limit {
        return Ok(false);
    }
    *text = legacy_text_token(field, text, limit)?;
    Ok(true)
}

/// Produce one bounded opaque value while preserving predecessor equality.
fn legacy_text_token(field: &'static str, value: &str, limit: usize) -> DbResult<String> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(LEGACY_TEXT_HASH_DOMAIN);
    for bytes in [field.as_bytes(), value.as_bytes()] {
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    let token = format!("legacy:{field}:{}", hasher.finalize().to_hex());
    validate_required_text(field, &token, limit)?;
    Ok(token)
}

/// Record which predecessor detail is no longer available exactly.
fn mark_legacy_detail_loss(
    connection: &Connection,
    project: ProjectInstanceId,
    instance: UsageInstanceId,
    event: &UsageEvent,
    loss: LegacyDetailLoss,
) -> DbResult<()> {
    if loss == LegacyDetailLoss::default() {
        return Ok(());
    }
    if loss.raw {
        connection.execute(
            "UPDATE usage_instances SET raw_detail_complete = 0
             WHERE project_instance_id = ?1 AND runtime_instance_id = ?2
               AND raw_detail_complete <> 0",
            params![
                project.as_bytes().as_slice(),
                instance.as_bytes().as_slice()
            ],
        )?;
    }
    if (loss.raw || loss.label)
        && let Some(label) = event_label(event)
    {
        connection.execute(
            "UPDATE usage_labels SET detail_complete = 0
             WHERE project_instance_id = ?1 AND caller_label = ?2
               AND detail_complete <> 0",
            params![project.as_bytes().as_slice(), label],
        )?;
    }
    connection.execute(
        "UPDATE usage_retention_state
         SET raw_detail_complete = CASE WHEN ?1 <> 0 THEN 0 ELSE raw_detail_complete END,
             dimension_detail_complete =
                 CASE WHEN ?2 <> 0 THEN 0 ELSE dimension_detail_complete END,
             label_history_complete =
                 CASE WHEN ?3 <> 0 THEN 0 ELSE label_history_complete END
         WHERE singleton = 1
           AND ((?1 <> 0 AND raw_detail_complete <> 0)
             OR (?2 <> 0 AND dimension_detail_complete <> 0)
             OR (?3 <> 0 AND label_history_complete <> 0))",
        params![
            i64::from(loss.raw),
            i64::from(loss.dimension),
            i64::from(loss.label),
        ],
    )?;
    Ok(())
}

/// Normalize an empty compatibility session label to an absent caller label.
fn event_label(event: &UsageEvent) -> Option<&str> {
    (!event.session_id.is_empty()).then_some(event.session_id.as_str())
}

/// Validate every bounded event field before any telemetry mutation.
pub(crate) fn validate_event(event: &UsageEvent, policy: TelemetryRetentionPolicy) -> DbResult<()> {
    validate_optional_text("session_id", event_label(event), policy.max_label_bytes)?;
    validate_required_text("command", &event.command, policy.max_command_bytes)?;
    validate_optional_text("path", event.path.as_deref(), policy.max_path_bytes)?;
    validate_optional_text("query", event.query.as_deref(), policy.max_query_bytes)?;
    for (field, value) in [
        ("token_savings_bucket", event.token_savings_bucket.as_str()),
        ("provider", event.provider.as_str()),
        ("model", event.model.as_str()),
        ("tokenizer_backend", event.tokenizer_backend.as_str()),
        ("accuracy", event.accuracy.as_str()),
        ("baseline_kind", event.baseline_kind.as_str()),
        ("confidence", event.confidence.as_str()),
        ("accounting_layer", event.report_accounting_layer()),
        ("estimate_method", event.estimate_method.as_str()),
        ("denominator_kind", event.report_denominator_kind()),
        ("dedupe_scope", event.report_dedupe_scope()),
    ] {
        validate_required_text(field, value, policy.max_dimension_bytes)?;
    }
    validate_required_text(
        "calculation_trace",
        &event.calculation_trace,
        256.min(policy.max_baseline_witness_bytes),
    )?;
    let baseline_identity = event.effective_baseline_identity();
    let baseline_fingerprint = event.effective_baseline_fingerprint();
    validate_required_text(
        "baseline_identity",
        baseline_identity.as_ref(),
        policy.max_baseline_witness_bytes,
    )?;
    validate_required_text(
        "baseline_fingerprint",
        baseline_fingerprint.as_ref(),
        256.min(policy.max_baseline_witness_bytes),
    )?;
    let _ = option_usize_to_i64(
        "estimated_tokens_without_projectatlas",
        event.estimated_tokens_without_projectatlas,
    )?;
    let _ = option_usize_to_i64(
        "estimated_tokens_with_projectatlas",
        event.estimated_tokens_with_projectatlas,
    )?;
    let _ = option_isize_to_i64("estimated_tokens_saved", event.estimated_tokens_saved)?;
    Ok(())
}

/// Require one nonempty UTF-8 field within its byte limit.
fn validate_required_text(field: &'static str, value: &str, limit: usize) -> DbResult<()> {
    if value.is_empty() || value.len() > limit {
        return Err(DbError::TelemetryFieldTooLarge {
            field,
            bytes: value.len(),
            limit,
        });
    }
    Ok(())
}

/// Validate an optional UTF-8 field against its byte limit.
fn validate_optional_text(field: &'static str, value: Option<&str>, limit: usize) -> DbResult<()> {
    if let Some(value) = value
        && value.len() > limit
    {
        return Err(DbError::TelemetryFieldTooLarge {
            field,
            bytes: value.len(),
            limit,
        });
    }
    Ok(())
}

/// Return the identifier of the reserved overflow dimension, inserting it once.
fn ensure_overflow_dimension(connection: &Connection, denominator_kind: &str) -> DbResult<i64> {
    ensure_dimension_unbounded(connection, &DimensionValues::overflow(denominator_kind))
}

/// Resolve a normalized dimension or route it to bounded overflow.
fn ensure_dimension(
    connection: &Connection,
    dimension: &DimensionValues,
    policy: TelemetryRetentionPolicy,
) -> DbResult<i64> {
    if let Some(id) = find_dimension(connection, dimension)? {
        return Ok(id);
    }
    if retention_counter(connection, RetentionCounter::DimensionRows)? >= policy.max_dimensions {
        connection.execute(
            "UPDATE usage_retention_state SET dimension_detail_complete = 0 WHERE singleton = 1",
            [],
        )?;
        return ensure_overflow_dimension(connection, &dimension.denominator_kind);
    }
    ensure_dimension_unbounded(connection, dimension)
}

/// Insert a known-admissible dimension and return its identifier.
fn ensure_dimension_unbounded(
    connection: &Connection,
    dimension: &DimensionValues,
) -> DbResult<i64> {
    let inserted = connection.execute(
        "INSERT OR IGNORE INTO usage_bucket_dimensions(
             token_savings_bucket, provider, model, tokenizer_backend,
             accuracy, baseline_kind, confidence, accounting_layer,
             estimate_method, denominator_kind, dedupe_scope, overflow
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            dimension.token_savings_bucket,
            dimension.provider,
            dimension.model,
            dimension.tokenizer_backend,
            dimension.accuracy,
            dimension.baseline_kind,
            dimension.confidence,
            dimension.accounting_layer,
            dimension.estimate_method,
            dimension.denominator_kind,
            dimension.dedupe_scope,
            i64::from(dimension.overflow),
        ],
    )?;
    if inserted != 0 {
        increment_retention_counter(connection, RetentionCounter::DimensionRows, 1)?;
    }
    find_dimension(connection, dimension)?.ok_or(DbError::SchemaPostcondition { expected: 11 })
}

/// Find one exact normalized dimension through its unique key.
fn find_dimension(connection: &Connection, dimension: &DimensionValues) -> DbResult<Option<i64>> {
    connection
        .query_row(
            "SELECT dimension_id FROM usage_bucket_dimensions
             WHERE token_savings_bucket = ?1 AND provider = ?2 AND model = ?3
               AND tokenizer_backend = ?4 AND accuracy = ?5
               AND baseline_kind = ?6 AND confidence = ?7
               AND accounting_layer = ?8 AND estimate_method = ?9
               AND denominator_kind = ?10 AND dedupe_scope = ?11
               AND overflow = ?12",
            params![
                dimension.token_savings_bucket,
                dimension.provider,
                dimension.model,
                dimension.tokenizer_backend,
                dimension.accuracy,
                dimension.baseline_kind,
                dimension.confidence,
                dimension.accounting_layer,
                dimension.estimate_method,
                dimension.denominator_kind,
                dimension.dedupe_scope,
                i64::from(dimension.overflow),
            ],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

/// Resolve or create one active runtime instance without reopening retired state.
fn ensure_active_instance(
    connection: &Connection,
    project: ProjectInstanceId,
    runtime: UsageInstanceId,
    owner: UsageInstanceOwner,
    caller_label: Option<&str>,
    policy: TelemetryRetentionPolicy,
    now: i64,
) -> DbResult<i64> {
    let existing = connection
        .query_row(
            "SELECT instance_row_id, owner, caller_label, state
             FROM usage_instances
             WHERE project_instance_id = ?1 AND runtime_instance_id = ?2",
            params![project.as_bytes().as_slice(), runtime.as_bytes().as_slice()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    if let Some((row_id, stored_owner, stored_label, state)) = existing {
        if stored_owner != owner.as_str() || stored_label.as_deref() != caller_label {
            return Err(DbError::TelemetryInstanceMismatch);
        }
        if state != INSTANCE_ACTIVE {
            return Err(DbError::TelemetryInstanceInactive);
        }
        return Ok(row_id);
    }
    let retired = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM usage_instance_tombstones
             WHERE project_instance_id = ?1 AND runtime_instance_id = ?2
         )",
        params![project.as_bytes().as_slice(), runtime.as_bytes().as_slice()],
        |row| row.get::<_, i64>(0),
    )?;
    if retired != 0 {
        return Err(DbError::TelemetryInstanceInactive);
    }
    let active = connection.query_row(
        "SELECT COUNT(*) FROM usage_instances
         WHERE project_instance_id = ?1 AND state = ?2",
        params![project.as_bytes().as_slice(), INSTANCE_ACTIVE],
        |row| row.get::<_, i64>(0),
    )?;
    if count_usize("active_usage_instances", active)? >= policy.max_active_instances {
        return Err(DbError::TelemetryInstanceCapacity);
    }
    if retention_counter(connection, RetentionCounter::InstanceRows)?
        >= policy.max_retained_instances
    {
        return Err(DbError::TelemetryInstanceCapacity);
    }
    connection.execute(
        "INSERT INTO usage_instances(
             project_instance_id, runtime_instance_id, owner, caller_label, state,
             started_at_epoch, last_seen_at_epoch
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![
            project.as_bytes().as_slice(),
            runtime.as_bytes().as_slice(),
            owner.as_str(),
            caller_label,
            INSTANCE_ACTIVE,
            now,
        ],
    )?;
    increment_retention_counter(connection, RetentionCounter::InstanceRows, 1)?;
    Ok(connection.last_insert_rowid())
}

/// Retain or refresh one optional caller label under the database-wide cap.
fn ensure_label(
    connection: &Connection,
    project: ProjectInstanceId,
    caller_label: Option<&str>,
    policy: TelemetryRetentionPolicy,
    now: i64,
) -> DbResult<()> {
    let Some(label) = caller_label else {
        return Ok(());
    };
    let exists = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM usage_labels
             WHERE project_instance_id = ?1 AND caller_label = ?2
         )",
        params![project.as_bytes().as_slice(), label],
        |row| row.get::<_, i64>(0),
    )?;
    let prior_history = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM usage_label_tombstones
             WHERE project_instance_id = ?1 AND caller_label = ?2
         )",
        params![project.as_bytes().as_slice(), label],
        |row| row.get::<_, i64>(0),
    )?;
    if exists == 0
        && retention_counter(connection, RetentionCounter::LabelRows)? >= policy.max_retained_labels
        && !evict_oldest_label(connection, policy, now)?
    {
        return Err(DbError::TelemetryInstanceCapacity);
    }
    let inserted = connection.execute(
        "INSERT INTO usage_labels(
             project_instance_id, caller_label, last_seen_at_epoch, detail_complete
         ) VALUES(?1, ?2, ?3, ?4)
         ON CONFLICT(project_instance_id, caller_label) DO UPDATE SET
             last_seen_at_epoch = excluded.last_seen_at_epoch",
        params![
            project.as_bytes().as_slice(),
            label,
            now,
            i64::from(prior_history == 0),
        ],
    )?;
    if inserted != 0 && exists == 0 {
        increment_retention_counter(connection, RetentionCounter::LabelRows, 1)?;
    }
    Ok(())
}

/// Record that caller-label detail became incomplete or expired.
fn upsert_label_tombstone(
    connection: &Connection,
    project: &[u8],
    label: &str,
    now: i64,
    runtime: Option<&[u8]>,
) -> DbResult<()> {
    let exists = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM usage_label_tombstones
             WHERE project_instance_id = ?1 AND caller_label = ?2
         )",
        params![project, label],
        |row| row.get::<_, i64>(0),
    )?;
    connection.execute(
        "INSERT INTO usage_label_tombstones(
             project_instance_id, caller_label, expired_at_epoch, last_instance_id
         ) VALUES(?1, ?2, ?3, ?4)
         ON CONFLICT(project_instance_id, caller_label) DO UPDATE SET
             expired_at_epoch = excluded.expired_at_epoch,
             last_instance_id = excluded.last_instance_id",
        params![project, label, now, runtime],
    )?;
    if exists == 0 {
        increment_retention_counter(connection, RetentionCounter::LabelTombstoneRows, 1)?;
    }
    Ok(())
}

/// Evict the oldest globally eligible inactive caller-label state.
fn evict_oldest_label(
    connection: &Connection,
    policy: TelemetryRetentionPolicy,
    now: i64,
) -> DbResult<bool> {
    let candidate = connection
        .query_row(
            "SELECT project_instance_id, caller_label FROM usage_labels
             WHERE NOT EXISTS(
                   SELECT 1 FROM usage_instances AS i
                   WHERE i.project_instance_id = usage_labels.project_instance_id
                     AND i.caller_label = usage_labels.caller_label
                     AND i.state = ?1
               )
               AND (
                   SELECT COUNT(*) FROM usage_instances AS i
                   WHERE i.project_instance_id = usage_labels.project_instance_id
                     AND i.caller_label = usage_labels.caller_label
               ) <= ?2
             ORDER BY last_seen_at_epoch, project_instance_id, caller_label LIMIT 1",
            params![
                INSTANCE_ACTIVE,
                to_i64("prune_batch_rows", policy.prune_batch_rows)?,
            ],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((project, label)) = candidate else {
        return Ok(false);
    };
    let runtime = connection
        .query_row(
            "SELECT runtime_instance_id FROM usage_instances
             WHERE project_instance_id = ?1 AND caller_label = ?2
             ORDER BY last_seen_at_epoch DESC, instance_row_id DESC LIMIT 1",
            params![project, label],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    upsert_label_tombstone(
        connection,
        project.as_slice(),
        &label,
        now,
        runtime.as_deref(),
    )?;
    connection.execute(
        "UPDATE usage_instances SET caller_label = NULL
         WHERE project_instance_id = ?1 AND caller_label = ?2 AND state <> ?3",
        params![project, label, INSTANCE_ACTIVE],
    )?;
    let deleted = connection.execute(
        "DELETE FROM usage_labels
         WHERE project_instance_id = ?1 AND caller_label = ?2",
        params![project, label],
    )?;
    decrement_retention_counter(connection, RetentionCounter::LabelRows, deleted)?;
    connection.execute(
        "UPDATE usage_retention_state SET label_history_complete = 0 WHERE singleton = 1",
        [],
    )?;
    Ok(true)
}

/// Calculate the exact aggregate delta contributed by one event.
fn aggregate_delta(
    connection: &Connection,
    instance_row_id: i64,
    event: &UsageEvent,
    policy: TelemetryRetentionPolicy,
    baseline_admission: BaselineAdmission,
) -> DbResult<AggregateCounters> {
    let (Some(without_source), Some(with_source)) = (
        event.estimated_tokens_without_projectatlas,
        event.estimated_tokens_with_projectatlas,
    ) else {
        return Ok(AggregateCounters::default());
    };
    let without = to_i64("estimated_tokens_without_projectatlas", without_source)?;
    let with = to_i64("estimated_tokens_with_projectatlas", with_source)?;
    let observed = event.is_observed();
    let modeled = event.is_modeled();
    let mut counters = AggregateCounters {
        calls: 1,
        estimated_without: without,
        estimated_with: with,
        observed_without: if observed { without } else { 0 },
        observed_with: if observed { with } else { 0 },
        modeled_without: if modeled { without } else { 0 },
        modeled_with: if modeled { with } else { 0 },
        observed_file_read_replacements: i64::from(
            observed && event.is_observed_file_read_replacement(without_source),
        ),
        modeled_file_reads_avoided: i64::from(
            modeled && event.is_modeled_file_read_avoidance(without_source),
        ),
        ..AggregateCounters::default()
    };
    if modeled {
        let contribution = if event.report_dedupe_scope() == DEDUPE_SCOPE_EVENT {
            without
                .checked_sub(with)
                .ok_or(DbError::TelemetryIntegerOverflow {
                    field: "event_modeled_contribution",
                })?
        } else {
            let (adjustment, repeated) = update_modeled_baseline(
                connection,
                instance_row_id,
                event,
                without,
                with,
                policy,
                baseline_admission,
            )?;
            counters.repeated_baselines = repeated;
            adjustment
        };
        let (positive, negative) = signed_components(contribution)?;
        counters.deduped_modeled_without = positive;
        counters.deduped_modeled_with = negative;
    }
    Ok(counters)
}

/// Update one active modeled baseline and return its signed adjustment.
fn update_modeled_baseline(
    connection: &Connection,
    instance_row_id: i64,
    event: &UsageEvent,
    without: i64,
    with: i64,
    policy: TelemetryRetentionPolicy,
    baseline_admission: BaselineAdmission,
) -> DbResult<(i64, i64)> {
    let key = event.modeled_baseline_key();
    let identity = event.effective_baseline_identity();
    let fingerprint = event.effective_baseline_fingerprint();
    let existing = connection
        .query_row(
            "SELECT baseline_identity, baseline_fingerprint, denominator_kind,
                    maximum_without, emitted_with, calls
             FROM usage_instance_baselines
             WHERE instance_row_id = ?1 AND baseline_key = ?2",
            params![instance_row_id, key.as_slice()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )
        .optional()?;
    if let Some((stored_identity, stored_fingerprint, denominator, old_max, old_with, calls)) =
        existing
    {
        if stored_identity != identity
            || stored_fingerprint != fingerprint
            || denominator != event.report_denominator_kind()
        {
            return Err(DbError::TelemetryBaselineCollision);
        }
        let previous = old_max
            .checked_sub(old_with)
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "previous_modeled_baseline",
            })?;
        let maximum = old_max.max(without);
        let emitted = old_with
            .checked_add(with)
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "modeled_baseline_emitted_with",
            })?;
        let current = maximum
            .checked_sub(emitted)
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "current_modeled_baseline",
            })?;
        let adjustment =
            current
                .checked_sub(previous)
                .ok_or(DbError::TelemetryIntegerOverflow {
                    field: "modeled_baseline_adjustment",
                })?;
        let calls = calls
            .checked_add(1)
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "modeled_baseline_calls",
            })?;
        connection.execute(
            "UPDATE usage_instance_baselines
             SET maximum_without = ?3, emitted_with = ?4, calls = ?5
             WHERE instance_row_id = ?1 AND baseline_key = ?2",
            params![instance_row_id, key.as_slice(), maximum, emitted, calls],
        )?;
        return Ok((adjustment, 1));
    }
    let instance_count = connection.query_row(
        "SELECT COUNT(*) FROM usage_instance_baselines WHERE instance_row_id = ?1",
        [instance_row_id],
        |row| row.get::<_, i64>(0),
    )?;
    let total_count = retention_counter(connection, RetentionCounter::BaselineRows)?;
    let total_bytes = retention_counter(connection, RetentionCounter::BaselineLogicalBytes)?;
    let witness_bytes = identity
        .len()
        .checked_add(fingerprint.len())
        .and_then(|value| value.checked_add(event.report_denominator_kind().len()))
        .ok_or(DbError::TelemetryIntegerOverflow {
            field: "baseline_witness_logical_bytes",
        })?;
    let projected_bytes =
        total_bytes
            .checked_add(witness_bytes)
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "baseline_logical_bytes",
            })?;
    let exceeds_runtime_limit = count_usize("instance_baseline_rows", instance_count)?
        >= policy.max_baselines_per_instance
        || total_count >= policy.max_active_baseline_rows
        || projected_bytes > policy.max_baseline_logical_bytes;
    if baseline_admission == BaselineAdmission::BoundedRuntime && exceeds_runtime_limit {
        return Err(DbError::TelemetryBaselineCapacity);
    }
    connection.execute(
        "INSERT INTO usage_instance_baselines(
             instance_row_id, baseline_key, baseline_identity, baseline_fingerprint,
             denominator_kind, maximum_without, emitted_with, calls, witness_logical_bytes
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8)",
        params![
            instance_row_id,
            key.as_slice(),
            identity,
            fingerprint,
            event.report_denominator_kind(),
            without,
            with,
            to_i64("baseline_witness_logical_bytes", witness_bytes)?,
        ],
    )?;
    increment_retention_counter(connection, RetentionCounter::BaselineRows, 1)?;
    increment_retention_counter(
        connection,
        RetentionCounter::BaselineLogicalBytes,
        witness_bytes,
    )?;
    Ok((
        without
            .checked_sub(with)
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "initial_modeled_baseline",
            })?,
        0,
    ))
}

/// Delete one instance's baseline witnesses and decrement exact counters.
fn delete_instance_baselines(connection: &Connection, instance_row_id: i64) -> DbResult<usize> {
    let (rows, bytes) = connection.query_row(
        "SELECT COUNT(*), COALESCE(SUM(witness_logical_bytes), 0)
         FROM usage_instance_baselines WHERE instance_row_id = ?1",
        [instance_row_id],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )?;
    let rows = count_usize("instance_baseline_rows", rows)?;
    let bytes = count_usize("instance_baseline_logical_bytes", bytes)?;
    if rows != 0 {
        connection.execute(
            "DELETE FROM usage_instance_baselines WHERE instance_row_id = ?1",
            [instance_row_id],
        )?;
        decrement_retention_counter(connection, RetentionCounter::BaselineRows, rows)?;
        decrement_retention_counter(connection, RetentionCounter::BaselineLogicalBytes, bytes)?;
    }
    Ok(rows)
}

/// Persist one bounded raw event row.
fn insert_raw_event(
    connection: &Connection,
    instance_row_id: i64,
    dimension_id: i64,
    event: &UsageEvent,
    created_at: i64,
    logical_bytes: usize,
) -> DbResult<()> {
    connection.execute(
        "INSERT INTO usage_events(
             instance_row_id, dimension_id, command, path, query,
             estimated_tokens_without_projectatlas,
             estimated_tokens_with_projectatlas, estimated_tokens_saved,
             calculation_trace, baseline_identity, baseline_fingerprint,
             created_at_epoch, logical_bytes
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            instance_row_id,
            dimension_id,
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
            option_isize_to_i64("estimated_tokens_saved", event.estimated_tokens_saved)?,
            event.calculation_trace,
            event.effective_baseline_identity(),
            event.effective_baseline_fingerprint(),
            created_at,
            to_i64("raw_logical_bytes", logical_bytes)?,
        ],
    )?;
    increment_retention_counter(connection, RetentionCounter::RawRows, 1)?;
    increment_retention_counter(connection, RetentionCounter::RawLogicalBytes, logical_bytes)?;
    Ok(())
}

/// Bind one retained runtime instance to exactly one routed worktree origin.
fn bind_worktree_origin(
    connection: &Connection,
    instance_row_id: i64,
    registration_id: Option<i64>,
) -> DbResult<()> {
    let existing = connection
        .query_row(
            "SELECT registration_id FROM usage_instance_worktree_origins
             WHERE instance_row_id = ?1",
            [instance_row_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(registration_id) = registration_id else {
        return if existing.is_some() {
            Err(DbError::WorktreeTelemetryOriginConflict)
        } else {
            Ok(())
        };
    };
    let registration_exists = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM worktree_registrations WHERE registration_id = ?1
         )",
        [registration_id],
        |row| row.get::<_, bool>(0),
    )?;
    if !registration_exists {
        return Err(DbError::WorktreeTelemetryProjectMismatch { registration_id });
    }
    match existing {
        Some(existing_id) if existing_id != registration_id => {
            Err(DbError::WorktreeTelemetryOriginConflict)
        }
        Some(_) => Ok(()),
        None => {
            connection.execute(
                "INSERT INTO usage_instance_worktree_origins(instance_row_id, registration_id)
                 VALUES(?1, ?2)",
                params![instance_row_id, registration_id],
            )?;
            Ok(())
        }
    }
}

/// Advance the local aggregate revision inside the accepted-event transaction.
fn increment_aggregate_revision(
    connection: &Connection,
    project: ProjectInstanceId,
    calls: i64,
) -> DbResult<()> {
    if calls == 0 {
        return Ok(());
    }
    connection
        .execute(
            "INSERT INTO usage_aggregate_revisions(project_instance_id, revision)
             VALUES(?1, 1)
             ON CONFLICT(project_instance_id) DO UPDATE SET
                 revision = usage_aggregate_revisions.revision + 1",
            [project.as_bytes().as_slice()],
        )
        .map_err(aggregate_write_error)?;
    Ok(())
}

/// Retain exact routed worktree totals separately from replaceable local snapshots.
fn upsert_routed_worktree_aggregates(
    connection: &Connection,
    registration_id: i64,
    dimension_id: i64,
    created_at: i64,
    delta: AggregateCounters,
    policy: TelemetryRetentionPolicy,
) -> DbResult<()> {
    if delta.calls == 0 {
        return Ok(());
    }
    upsert_worktree_aggregate(
        connection,
        registration_id,
        WORKTREE_USAGE_ROUTED,
        -1,
        dimension_id,
        delta,
    )?;
    let day = created_at - created_at.rem_euclid(SECONDS_PER_DAY);
    reserve_routed_worktree_daily_row(
        connection,
        registration_id,
        day,
        dimension_id,
        policy.max_daily_rows,
    )?;
    upsert_worktree_aggregate(
        connection,
        registration_id,
        WORKTREE_USAGE_ROUTED,
        day,
        dimension_id,
        delta,
    )
}

/// Upsert one routed or synchronized worktree aggregate row.
fn upsert_worktree_aggregate(
    connection: &Connection,
    registration_id: i64,
    source_kind: &str,
    day_epoch: i64,
    dimension_id: i64,
    delta: AggregateCounters,
) -> DbResult<()> {
    connection
        .execute(
            "INSERT INTO worktree_usage_aggregates(
                registration_id, source_kind, day_epoch, dimension_id,
                calls, estimated_without, estimated_with, observed_without,
                observed_with, modeled_without, modeled_with,
                deduped_modeled_without, deduped_modeled_with, repeated_baselines,
                observed_file_read_replacements, modeled_file_reads_avoided
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
             ON CONFLICT(registration_id, source_kind, day_epoch, dimension_id) DO UPDATE SET
                calls = worktree_usage_aggregates.calls + excluded.calls,
                estimated_without = worktree_usage_aggregates.estimated_without + excluded.estimated_without,
                estimated_with = worktree_usage_aggregates.estimated_with + excluded.estimated_with,
                observed_without = worktree_usage_aggregates.observed_without + excluded.observed_without,
                observed_with = worktree_usage_aggregates.observed_with + excluded.observed_with,
                modeled_without = worktree_usage_aggregates.modeled_without + excluded.modeled_without,
                modeled_with = worktree_usage_aggregates.modeled_with + excluded.modeled_with,
                deduped_modeled_without = worktree_usage_aggregates.deduped_modeled_without + excluded.deduped_modeled_without,
                deduped_modeled_with = worktree_usage_aggregates.deduped_modeled_with + excluded.deduped_modeled_with,
                repeated_baselines = worktree_usage_aggregates.repeated_baselines + excluded.repeated_baselines,
                observed_file_read_replacements = worktree_usage_aggregates.observed_file_read_replacements + excluded.observed_file_read_replacements,
                modeled_file_reads_avoided = worktree_usage_aggregates.modeled_file_reads_avoided + excluded.modeled_file_reads_avoided",
            worktree_aggregate_params!(
                registration_id,
                source_kind,
                day_epoch,
                dimension_id,
                delta
            ),
        )
        .map_err(aggregate_write_error)?;
    Ok(())
}

/// Keep routed daily attribution bounded while lifetime totals remain exact.
fn reserve_routed_worktree_daily_row(
    connection: &Connection,
    registration_id: i64,
    day_epoch: i64,
    dimension_id: i64,
    max_daily_rows: usize,
) -> DbResult<()> {
    let exists = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM worktree_usage_aggregates
             WHERE registration_id = ?1 AND source_kind = ?2
               AND day_epoch = ?3 AND dimension_id = ?4
         )",
        params![
            registration_id,
            WORKTREE_USAGE_ROUTED,
            day_epoch,
            dimension_id
        ],
        |row| row.get::<_, bool>(0),
    )?;
    if exists {
        return Ok(());
    }
    let daily_rows = connection.query_row(
        "SELECT COUNT(*) FROM worktree_usage_aggregates WHERE day_epoch >= 0",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    let daily_rows = count_usize("worktree_daily_rows", daily_rows)?;
    if daily_rows < max_daily_rows {
        return Ok(());
    }
    let removed = connection.execute(
        "DELETE FROM worktree_usage_aggregates
         WHERE (registration_id, source_kind, day_epoch, dimension_id) IN (
             SELECT registration_id, source_kind, day_epoch, dimension_id
             FROM worktree_usage_aggregates
             WHERE day_epoch >= 0
               AND NOT (
                   registration_id = ?1 AND source_kind = ?2
                   AND day_epoch = ?3 AND dimension_id = ?4
               )
             ORDER BY day_epoch, registration_id, source_kind, dimension_id
             LIMIT 1
         )",
        params![
            registration_id,
            WORKTREE_USAGE_ROUTED,
            day_epoch,
            dimension_id
        ],
    )?;
    if removed != 1 {
        return Err(DbError::WorktreeTelemetrySnapshotLimit {
            resource: "daily_rows",
            limit: max_daily_rows,
            observed: daily_rows.saturating_add(1),
        });
    }
    Ok(())
}

/// Apply one event delta to all retained aggregate scopes.
fn apply_aggregates(
    connection: &Connection,
    project: ProjectInstanceId,
    instance_row_id: i64,
    dimension_id: i64,
    created_at: i64,
    delta: AggregateCounters,
    policy: TelemetryRetentionPolicy,
) -> DbResult<()> {
    if delta.calls == 0 {
        return Ok(());
    }
    upsert_global(connection, project, dimension_id, delta)?;
    upsert_instance(connection, instance_row_id, dimension_id, delta)?;
    let day = created_at - created_at.rem_euclid(SECONDS_PER_DAY);
    prepare_daily_capacity(
        connection,
        project,
        instance_row_id,
        day,
        dimension_id,
        policy,
    )?;
    upsert_global_daily(connection, project, day, dimension_id, delta)?;
    upsert_instance_daily(connection, instance_row_id, day, dimension_id, delta)?;
    Ok(())
}

/// Upsert exact all-time aggregates for one project dimension.
fn upsert_global(
    connection: &Connection,
    project: ProjectInstanceId,
    dimension_id: i64,
    delta: AggregateCounters,
) -> DbResult<()> {
    connection.execute(
        "INSERT INTO usage_global_aggregates VALUES(
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT(project_instance_id, dimension_id) DO UPDATE SET
             calls = usage_global_aggregates.calls + excluded.calls,
             estimated_without = usage_global_aggregates.estimated_without + excluded.estimated_without,
             estimated_with = usage_global_aggregates.estimated_with + excluded.estimated_with,
             observed_without = usage_global_aggregates.observed_without + excluded.observed_without,
             observed_with = usage_global_aggregates.observed_with + excluded.observed_with,
             modeled_without = usage_global_aggregates.modeled_without + excluded.modeled_without,
             modeled_with = usage_global_aggregates.modeled_with + excluded.modeled_with,
             deduped_modeled_without = usage_global_aggregates.deduped_modeled_without + excluded.deduped_modeled_without,
             deduped_modeled_with = usage_global_aggregates.deduped_modeled_with + excluded.deduped_modeled_with,
             repeated_baselines = usage_global_aggregates.repeated_baselines + excluded.repeated_baselines,
             observed_file_read_replacements = usage_global_aggregates.observed_file_read_replacements + excluded.observed_file_read_replacements,
             modeled_file_reads_avoided = usage_global_aggregates.modeled_file_reads_avoided + excluded.modeled_file_reads_avoided",
        aggregate_params!(project.as_bytes().as_slice(), dimension_id, delta),
    )
    .map_err(aggregate_write_error)?;
    Ok(())
}

/// Upsert exact all-time aggregates for one runtime dimension.
fn upsert_instance(
    connection: &Connection,
    instance_row_id: i64,
    dimension_id: i64,
    delta: AggregateCounters,
) -> DbResult<()> {
    connection.execute(
        "INSERT INTO usage_instance_aggregates VALUES(
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT(instance_row_id, dimension_id) DO UPDATE SET
             calls = usage_instance_aggregates.calls + excluded.calls,
             estimated_without = usage_instance_aggregates.estimated_without + excluded.estimated_without,
             estimated_with = usage_instance_aggregates.estimated_with + excluded.estimated_with,
             observed_without = usage_instance_aggregates.observed_without + excluded.observed_without,
             observed_with = usage_instance_aggregates.observed_with + excluded.observed_with,
             modeled_without = usage_instance_aggregates.modeled_without + excluded.modeled_without,
             modeled_with = usage_instance_aggregates.modeled_with + excluded.modeled_with,
             deduped_modeled_without = usage_instance_aggregates.deduped_modeled_without + excluded.deduped_modeled_without,
             deduped_modeled_with = usage_instance_aggregates.deduped_modeled_with + excluded.deduped_modeled_with,
             repeated_baselines = usage_instance_aggregates.repeated_baselines + excluded.repeated_baselines,
             observed_file_read_replacements = usage_instance_aggregates.observed_file_read_replacements + excluded.observed_file_read_replacements,
             modeled_file_reads_avoided = usage_instance_aggregates.modeled_file_reads_avoided + excluded.modeled_file_reads_avoided",
        aggregate_params!(instance_row_id, dimension_id, delta),
    )
    .map_err(aggregate_write_error)?;
    Ok(())
}

/// Upsert one bounded project-wide daily aggregate row.
fn upsert_global_daily(
    connection: &Connection,
    project: ProjectInstanceId,
    day: i64,
    dimension_id: i64,
    delta: AggregateCounters,
) -> DbResult<()> {
    connection.execute(
        "INSERT INTO usage_daily_aggregates VALUES(
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT(project_instance_id, day_epoch, dimension_id) DO UPDATE SET
             calls = usage_daily_aggregates.calls + excluded.calls,
             estimated_without = usage_daily_aggregates.estimated_without + excluded.estimated_without,
             estimated_with = usage_daily_aggregates.estimated_with + excluded.estimated_with,
             observed_without = usage_daily_aggregates.observed_without + excluded.observed_without,
             observed_with = usage_daily_aggregates.observed_with + excluded.observed_with,
             modeled_without = usage_daily_aggregates.modeled_without + excluded.modeled_without,
             modeled_with = usage_daily_aggregates.modeled_with + excluded.modeled_with,
             deduped_modeled_without = usage_daily_aggregates.deduped_modeled_without + excluded.deduped_modeled_without,
             deduped_modeled_with = usage_daily_aggregates.deduped_modeled_with + excluded.deduped_modeled_with,
             repeated_baselines = usage_daily_aggregates.repeated_baselines + excluded.repeated_baselines,
             observed_file_read_replacements = usage_daily_aggregates.observed_file_read_replacements + excluded.observed_file_read_replacements,
             modeled_file_reads_avoided = usage_daily_aggregates.modeled_file_reads_avoided + excluded.modeled_file_reads_avoided",
        daily_aggregate_params!(project.as_bytes().as_slice(), day, dimension_id, delta),
    )
    .map_err(aggregate_write_error)?;
    Ok(())
}

/// Upsert one bounded instance-specific daily aggregate row.
fn upsert_instance_daily(
    connection: &Connection,
    instance_row_id: i64,
    day: i64,
    dimension_id: i64,
    delta: AggregateCounters,
) -> DbResult<()> {
    connection.execute(
        "INSERT INTO usage_instance_daily_aggregates VALUES(
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT(instance_row_id, day_epoch, dimension_id) DO UPDATE SET
             calls = usage_instance_daily_aggregates.calls + excluded.calls,
             estimated_without = usage_instance_daily_aggregates.estimated_without + excluded.estimated_without,
             estimated_with = usage_instance_daily_aggregates.estimated_with + excluded.estimated_with,
             observed_without = usage_instance_daily_aggregates.observed_without + excluded.observed_without,
             observed_with = usage_instance_daily_aggregates.observed_with + excluded.observed_with,
             modeled_without = usage_instance_daily_aggregates.modeled_without + excluded.modeled_without,
             modeled_with = usage_instance_daily_aggregates.modeled_with + excluded.modeled_with,
             deduped_modeled_without = usage_instance_daily_aggregates.deduped_modeled_without + excluded.deduped_modeled_without,
             deduped_modeled_with = usage_instance_daily_aggregates.deduped_modeled_with + excluded.deduped_modeled_with,
             repeated_baselines = usage_instance_daily_aggregates.repeated_baselines + excluded.repeated_baselines,
             observed_file_read_replacements = usage_instance_daily_aggregates.observed_file_read_replacements + excluded.observed_file_read_replacements,
             modeled_file_reads_avoided = usage_instance_daily_aggregates.modeled_file_reads_avoided + excluded.modeled_file_reads_avoided",
        daily_aggregate_params!(instance_row_id, day, dimension_id, delta),
    )
    .map_err(aggregate_write_error)?;
    Ok(())
}

/// Preserve the typed telemetry overflow contract for native `SQLite` additions.
fn aggregate_write_error(error: rusqlite::Error) -> DbError {
    if matches!(
        &error,
        rusqlite::Error::SqliteFailure(sqlite_error, _)
            if sqlite_error.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_DATATYPE
    ) {
        DbError::TelemetryIntegerOverflow {
            field: AGGREGATE_COUNTER_FIELD,
        }
    } else {
        error.into()
    }
}

/// Identify one daily row that can be evicted without removing the current targets.
#[derive(Debug, Eq, PartialEq)]
enum DailyEvictionCandidate {
    /// One project-wide daily aggregate.
    Global {
        /// Owning project identity bytes.
        project: Vec<u8>,
        /// UTC day epoch.
        day: i64,
        /// Normalized dimension identifier.
        dimension_id: i64,
    },
    /// One runtime-specific daily aggregate.
    Instance {
        /// Owning runtime row.
        instance_row_id: i64,
        /// UTC day epoch.
        day: i64,
        /// Normalized dimension identifier.
        dimension_id: i64,
    },
}

impl DailyEvictionCandidate {
    /// Return the stable ordering key used to merge both indexed retention heads.
    fn ordering_key(&self) -> (i64, u8, Vec<u8>, i64) {
        match self {
            Self::Global {
                project,
                day,
                dimension_id,
            } => (*day, 0, project.clone(), *dimension_id),
            Self::Instance {
                instance_row_id,
                day,
                dimension_id,
            } => (
                *day,
                1,
                instance_row_id.to_be_bytes().to_vec(),
                *dimension_id,
            ),
        }
    }
}

/// Reserve both current daily rows together, evicting only the exact oldest pressure.
fn prepare_daily_capacity(
    connection: &Connection,
    project: ProjectInstanceId,
    instance_row_id: i64,
    day: i64,
    dimension_id: i64,
    policy: TelemetryRetentionPolicy,
) -> DbResult<()> {
    let global_exists = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM usage_daily_aggregates
             WHERE project_instance_id = ?1 AND day_epoch = ?2 AND dimension_id = ?3
         )",
        params![project.as_bytes().as_slice(), day, dimension_id],
        |row| row.get::<_, i64>(0),
    )?;
    let instance_exists = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM usage_instance_daily_aggregates
             WHERE instance_row_id = ?1 AND day_epoch = ?2 AND dimension_id = ?3
         )",
        params![instance_row_id, day, dimension_id],
        |row| row.get::<_, i64>(0),
    )?;
    let required = usize::from(global_exists == 0)
        .checked_add(usize::from(instance_exists == 0))
        .ok_or(DbError::TelemetryIntegerOverflow {
            field: "daily_rows",
        })?;
    if required == 0 {
        return Ok(());
    }
    let current = retention_counter(connection, RetentionCounter::DailyRows)?;
    if current > policy.max_daily_rows {
        return Err(DbError::TelemetryLimitInvalid {
            field: "daily_rows",
            value: current,
        });
    }
    let projected = current
        .checked_add(required)
        .ok_or(DbError::TelemetryIntegerOverflow {
            field: "daily_rows",
        })?;
    let evictions = projected.saturating_sub(policy.max_daily_rows);
    for _ in 0..evictions {
        let candidate = oldest_daily_eviction_candidate(
            connection,
            project,
            instance_row_id,
            day,
            dimension_id,
        )?
        .ok_or(DbError::TelemetryLimitInvalid {
            field: "max_daily_rows",
            value: policy.max_daily_rows,
        })?;
        delete_daily_eviction_candidate(connection, &candidate)?;
    }
    if evictions != 0 {
        decrement_retention_counter(connection, RetentionCounter::DailyRows, evictions)?;
        connection.execute(
            "UPDATE usage_retention_state SET label_history_complete = 0 WHERE singleton = 1",
            [],
        )?;
    }
    increment_retention_counter(connection, RetentionCounter::DailyRows, required)
}

/// Load the exact oldest evictable row from the two indexed daily tables.
fn oldest_daily_eviction_candidate(
    connection: &Connection,
    project: ProjectInstanceId,
    instance_row_id: i64,
    day: i64,
    dimension_id: i64,
) -> DbResult<Option<DailyEvictionCandidate>> {
    let global = connection
        .query_row(
            "SELECT project_instance_id, day_epoch, dimension_id
             FROM usage_daily_aggregates
             WHERE NOT (
                 project_instance_id = ?1 AND day_epoch = ?2 AND dimension_id = ?3
             )
             ORDER BY day_epoch, project_instance_id, dimension_id LIMIT 1",
            params![project.as_bytes().as_slice(), day, dimension_id],
            |row| {
                Ok(DailyEvictionCandidate::Global {
                    project: row.get(0)?,
                    day: row.get(1)?,
                    dimension_id: row.get(2)?,
                })
            },
        )
        .optional()?;
    let instance = connection
        .query_row(
            "SELECT instance_row_id, day_epoch, dimension_id
             FROM usage_instance_daily_aggregates
             WHERE NOT (
                 instance_row_id = ?1 AND day_epoch = ?2 AND dimension_id = ?3
             )
             ORDER BY day_epoch, instance_row_id, dimension_id LIMIT 1",
            params![instance_row_id, day, dimension_id],
            |row| {
                Ok(DailyEvictionCandidate::Instance {
                    instance_row_id: row.get(0)?,
                    day: row.get(1)?,
                    dimension_id: row.get(2)?,
                })
            },
        )
        .optional()?;
    Ok(match (global, instance) {
        (Some(global), Some(instance)) => {
            if global.ordering_key() <= instance.ordering_key() {
                Some(global)
            } else {
                Some(instance)
            }
        }
        (Some(candidate), None) | (None, Some(candidate)) => Some(candidate),
        (None, None) => None,
    })
}

/// Delete one selected daily aggregate row.
fn delete_daily_eviction_candidate(
    connection: &Connection,
    candidate: &DailyEvictionCandidate,
) -> DbResult<()> {
    let deleted = match candidate {
        DailyEvictionCandidate::Global {
            project,
            day,
            dimension_id,
        } => connection.execute(
            "DELETE FROM usage_daily_aggregates
             WHERE project_instance_id = ?1 AND day_epoch = ?2 AND dimension_id = ?3",
            params![project, day, dimension_id],
        )?,
        DailyEvictionCandidate::Instance {
            instance_row_id,
            day,
            dimension_id,
        } => connection.execute(
            "DELETE FROM usage_instance_daily_aggregates
             WHERE instance_row_id = ?1 AND day_epoch = ?2 AND dimension_id = ?3",
            params![instance_row_id, day, dimension_id],
        )?,
    };
    if deleted != 1 {
        return Err(DbError::TelemetryIntegerOverflow {
            field: "daily_rows",
        });
    }
    Ok(())
}

/// Advance one instance's monotonic last-seen state and record clock anomalies.
fn touch_instance(
    connection: &Connection,
    instance_row_id: i64,
    now: i64,
    policy: TelemetryRetentionPolicy,
) -> DbResult<()> {
    let previous = connection.query_row(
        "SELECT last_seen_at_epoch FROM usage_instances WHERE instance_row_id = ?1",
        [instance_row_id],
        |row| row.get::<_, i64>(0),
    )?;
    let tolerance = to_i64(
        "future_clock_tolerance_seconds",
        policy.future_clock_tolerance_seconds,
    )?;
    let anomaly = previous
        > now
            .checked_add(tolerance)
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "future_clock_tolerance",
            })?;
    let observed = previous.max(now);
    connection.execute(
        "UPDATE usage_instances
         SET last_seen_at_epoch = ?2,
             clock_anomaly = CASE WHEN ?3 = 1 THEN 1 ELSE clock_anomaly END
         WHERE instance_row_id = ?1",
        params![instance_row_id, observed, i64::from(anomaly)],
    )?;
    if anomaly {
        connection.execute(
            "UPDATE usage_retention_state SET clock_anomaly = 1 WHERE singleton = 1",
            [],
        )?;
    }
    Ok(())
}

/// Expire a bounded page of idle active instances except the current runtime.
fn expire_idle_instances(
    connection: &Connection,
    project: ProjectInstanceId,
    current: UsageInstanceId,
    policy: TelemetryRetentionPolicy,
    now: i64,
) -> DbResult<usize> {
    let cutoff = epoch_cutoff(
        now,
        policy.max_active_idle_seconds,
        "max_active_idle_seconds",
    )?;
    let mut statement = connection.prepare_cached(
        "SELECT instance_row_id FROM usage_instances
         WHERE project_instance_id = ?1 AND state = ?2
           AND runtime_instance_id <> ?3 AND last_seen_at_epoch < ?4
         ORDER BY last_seen_at_epoch, instance_row_id LIMIT ?5",
    )?;
    let rows = statement.query_map(
        params![
            project.as_bytes().as_slice(),
            INSTANCE_ACTIVE,
            current.as_bytes().as_slice(),
            cutoff,
            to_i64("prune_batch_rows", policy.prune_batch_rows)?,
        ],
        |row| row.get::<_, i64>(0),
    )?;
    let ids = rows.collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for row_id in &ids {
        connection.execute(
            "UPDATE usage_instances
             SET state = ?2, sealed_at_epoch = last_seen_at_epoch
             WHERE instance_row_id = ?1 AND state = ?3",
            params![row_id, INSTANCE_EXPIRED, INSTANCE_ACTIVE],
        )?;
        delete_instance_baselines(connection, *row_id)?;
    }
    Ok(ids.len())
}

/// Remove one bounded page of expired, sealed, or capacity-reserved instances.
fn prune_instances_once(
    connection: &Connection,
    policy: TelemetryRetentionPolicy,
    now: i64,
    reserve_instances: usize,
) -> DbResult<(usize, usize)> {
    let active_project = current_project(connection)?.as_bytes();
    let projected = retention_counter(connection, RetentionCounter::InstanceRows)?
        .checked_add(reserve_instances)
        .ok_or(DbError::TelemetryIntegerOverflow {
            field: "instance_rows",
        })?;
    let excess = projected.saturating_sub(policy.max_retained_instances);
    let cutoff = epoch_cutoff(
        now,
        policy.retained_instance_seconds,
        "retained_instance_seconds",
    )?;
    let limit = if excess == 0 {
        policy.prune_batch_rows
    } else {
        excess.min(policy.prune_batch_rows)
    };
    let mut statement = connection.prepare_cached(
        "SELECT instance_row_id, project_instance_id, runtime_instance_id, caller_label
         FROM usage_instances
         WHERE state IN (?1, ?2) AND (?3 > 0 OR last_seen_at_epoch < ?4)
         ORDER BY last_seen_at_epoch, instance_row_id LIMIT ?5",
    )?;
    let rows = statement.query_map(
        params![
            INSTANCE_SEALED,
            INSTANCE_EXPIRED,
            to_i64("excess_instance_rows", excess)?,
            cutoff,
            to_i64("prune_batch_rows", limit)?,
        ],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        },
    )?;
    let candidates = rows.collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut raw_rows = 0usize;
    for (row_id, project, runtime, caller_label) in &candidates {
        let (raw, raw_bytes) = connection.query_row(
            "SELECT COUNT(*), COALESCE(SUM(logical_bytes), 0)
             FROM usage_events WHERE instance_row_id = ?1",
            [row_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?;
        raw_rows = raw_rows
            .checked_add(count_usize("pruned_instance_raw_rows", raw)?)
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "pruned_instance_raw_rows",
            })?;
        let daily_rows = connection.query_row(
            "SELECT COUNT(*) FROM usage_instance_daily_aggregates WHERE instance_row_id = ?1",
            [row_id],
            |row| row.get::<_, i64>(0),
        )?;
        let tombstone_exists = connection.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM usage_instance_tombstones
                 WHERE project_instance_id = ?1 AND runtime_instance_id = ?2
             )",
            params![project, runtime],
            |row| row.get::<_, i64>(0),
        )?;
        connection.execute(
            "INSERT INTO usage_instance_tombstones(
                 project_instance_id, runtime_instance_id, retired_at_epoch
             ) VALUES(?1, ?2, ?3)
             ON CONFLICT(project_instance_id, runtime_instance_id) DO UPDATE SET
                 retired_at_epoch = excluded.retired_at_epoch",
            params![project, runtime, now],
        )?;
        if tombstone_exists == 0 {
            increment_retention_counter(connection, RetentionCounter::InstanceTombstoneRows, 1)?;
        }
        if let Some(label) = caller_label {
            connection.execute(
                "UPDATE usage_labels SET detail_complete = 0
                 WHERE project_instance_id = ?1 AND caller_label = ?2",
                params![project, label],
            )?;
            upsert_label_tombstone(connection, project, label, now, Some(runtime.as_slice()))?;
        }
        delete_instance_baselines(connection, *row_id)?;
        connection.execute(
            "DELETE FROM usage_instances WHERE instance_row_id = ?1",
            [row_id],
        )?;
        decrement_retention_counter(connection, RetentionCounter::InstanceRows, 1)?;
        decrement_retention_counter(
            connection,
            RetentionCounter::RawRows,
            count_usize("instance_raw_rows", raw)?,
        )?;
        decrement_retention_counter(
            connection,
            RetentionCounter::RawLogicalBytes,
            count_usize("instance_raw_logical_bytes", raw_bytes)?,
        )?;
        decrement_retention_counter(
            connection,
            RetentionCounter::DailyRows,
            count_usize("instance_daily_rows", daily_rows)?,
        )?;
        if project.as_slice() != active_project.as_slice() {
            let project_still_retained = connection.query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM usage_instances WHERE project_instance_id = ?1 LIMIT 1
                 )",
                [project],
                |row| row.get::<_, i64>(0),
            )?;
            if project_still_retained == 0 {
                connection.execute(
                    "DELETE FROM usage_global_aggregates WHERE project_instance_id = ?1",
                    [project],
                )?;
            }
        }
    }
    Ok((candidates.len(), raw_rows))
}

/// Read raw row, logical-byte, and age pressure through bounded counters and probes.
fn raw_pressure(
    connection: &Connection,
    policy: TelemetryRetentionPolicy,
    now: i64,
) -> DbResult<(usize, usize, usize)> {
    let cutoff = epoch_cutoff(now, policy.max_raw_age_seconds, "max_raw_age_seconds")?;
    let old_rows = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM usage_events
             WHERE created_at_epoch < ?1
             ORDER BY created_at_epoch, id LIMIT 1
         )",
        [cutoff],
        |row| row.get::<_, i64>(0),
    )?;
    Ok((
        retention_counter(connection, RetentionCounter::RawRows)?,
        retention_counter(connection, RetentionCounter::RawLogicalBytes)?,
        usize::from(old_rows != 0),
    ))
}

/// Remove one oldest bounded raw-event page when a raw budget is exceeded.
fn prune_raw_once(
    connection: &Connection,
    policy: TelemetryRetentionPolicy,
    now: i64,
) -> DbResult<usize> {
    let (rows, bytes, old_rows) = raw_pressure(connection, policy, now)?;
    if rows <= policy.max_raw_rows && bytes <= policy.max_raw_logical_bytes && old_rows == 0 {
        return Ok(0);
    }
    let cutoff = epoch_cutoff(now, policy.max_raw_age_seconds, "max_raw_age_seconds")?;
    let mut statement = connection.prepare_cached(
        "SELECT created_at_epoch, logical_bytes FROM usage_events
         ORDER BY created_at_epoch, id LIMIT ?1",
    )?;
    let selected = statement.query_map(
        [to_i64("prune_batch_rows", policy.prune_batch_rows)?],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )?;
    let selected = selected.collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut remaining_rows = rows;
    let mut remaining_bytes = bytes;
    let mut delete_count = 0usize;
    let mut logical_bytes = 0usize;
    for (created_at, event_bytes) in selected {
        if created_at >= cutoff
            && remaining_rows <= policy.max_raw_rows
            && remaining_bytes <= policy.max_raw_logical_bytes
        {
            break;
        }
        let event_bytes = count_usize("raw_logical_bytes", event_bytes)?;
        remaining_rows = remaining_rows
            .checked_sub(1)
            .ok_or(DbError::TelemetryIntegerOverflow { field: "raw_rows" })?;
        remaining_bytes =
            remaining_bytes
                .checked_sub(event_bytes)
                .ok_or(DbError::TelemetryIntegerOverflow {
                    field: "raw_logical_bytes",
                })?;
        delete_count = delete_count
            .checked_add(1)
            .ok_or(DbError::TelemetryIntegerOverflow { field: "raw_rows" })?;
        logical_bytes =
            logical_bytes
                .checked_add(event_bytes)
                .ok_or(DbError::TelemetryIntegerOverflow {
                    field: "raw_logical_bytes",
                })?;
    }
    if delete_count != 0 {
        let limit = to_i64("pruned_raw_rows", delete_count)?;
        connection.execute(
            "UPDATE usage_labels SET detail_complete = 0
             WHERE (project_instance_id, caller_label) IN (
                 SELECT DISTINCT i.project_instance_id, i.caller_label
                 FROM usage_events AS e
                 JOIN usage_instances AS i USING(instance_row_id)
                 WHERE i.caller_label IS NOT NULL
                   AND e.id IN (
                       SELECT id FROM usage_events
                       ORDER BY created_at_epoch, id LIMIT ?1
                   )
             )",
            [limit],
        )?;
        connection.execute(
            "UPDATE usage_instances SET raw_detail_complete = 0
             WHERE instance_row_id IN (
                 SELECT instance_row_id FROM usage_events
                 ORDER BY created_at_epoch, id LIMIT ?1
             )",
            [limit],
        )?;
        let deleted = connection.execute(
            "DELETE FROM usage_events WHERE id IN (
                 SELECT id FROM usage_events
                 ORDER BY created_at_epoch, id LIMIT ?1
             )",
            [limit],
        )?;
        if deleted != delete_count {
            return Err(DbError::TelemetryIntegerOverflow { field: "raw_rows" });
        }
        decrement_retention_counter(connection, RetentionCounter::RawRows, delete_count)?;
        decrement_retention_counter(connection, RetentionCounter::RawLogicalBytes, logical_bytes)?;
        connection.execute(
            "UPDATE usage_retention_state SET raw_detail_complete = 0 WHERE singleton = 1",
            [],
        )?;
    }
    Ok(delete_count)
}

/// Remove one bounded page of old or excess daily aggregate rows.
fn prune_daily_once(
    connection: &Connection,
    policy: TelemetryRetentionPolicy,
    now: i64,
) -> DbResult<usize> {
    let days = policy
        .retained_trend_days
        .checked_mul(SECONDS_PER_DAY as u64)
        .ok_or(DbError::TelemetryIntegerOverflow {
            field: "retained_trend_seconds",
        })?;
    let cutoff = epoch_cutoff(now, days, "retained_trend_seconds")?;
    let limit = to_i64("prune_batch_rows", policy.prune_batch_rows)?;
    let global = connection.execute(
        "DELETE FROM usage_daily_aggregates
         WHERE (project_instance_id, day_epoch, dimension_id) IN (
             SELECT project_instance_id, day_epoch, dimension_id
             FROM usage_daily_aggregates WHERE day_epoch < ?1
             ORDER BY day_epoch, project_instance_id, dimension_id LIMIT ?2
         )",
        params![cutoff, limit],
    )?;
    let remaining = policy.prune_batch_rows.saturating_sub(global);
    let instance = if remaining == 0 {
        0
    } else {
        connection.execute(
            "DELETE FROM usage_instance_daily_aggregates
             WHERE (instance_row_id, day_epoch, dimension_id) IN (
                 SELECT instance_row_id, day_epoch, dimension_id
                 FROM usage_instance_daily_aggregates WHERE day_epoch < ?1
                 ORDER BY day_epoch, instance_row_id, dimension_id LIMIT ?2
             )",
            params![cutoff, to_i64("daily_prune_remaining", remaining)?],
        )?
    };
    let deleted = global
        .checked_add(instance)
        .ok_or(DbError::TelemetryIntegerOverflow {
            field: "pruned_daily_rows",
        })?;
    if deleted != 0 {
        decrement_retention_counter(connection, RetentionCounter::DailyRows, deleted)?;
    }
    Ok(deleted)
}

/// Remove one bounded page of old inactive caller-label state.
fn prune_labels_once(
    connection: &Connection,
    policy: TelemetryRetentionPolicy,
    now: i64,
) -> DbResult<usize> {
    let cutoff = epoch_cutoff(now, policy.retained_label_seconds, "retained_label_seconds")?;
    let label = connection
        .query_row(
            "SELECT project_instance_id, caller_label FROM usage_labels AS labels
             WHERE last_seen_at_epoch < ?1
               AND NOT EXISTS(
                   SELECT 1 FROM usage_instances AS i
                   WHERE i.project_instance_id = labels.project_instance_id
                     AND i.caller_label = labels.caller_label
                     AND i.state = ?2
               )
               AND (
                   SELECT COUNT(*) FROM usage_instances AS i
                   WHERE i.project_instance_id = labels.project_instance_id
                     AND i.caller_label = labels.caller_label
               ) <= ?3
             ORDER BY last_seen_at_epoch, project_instance_id, caller_label LIMIT 1",
            params![
                cutoff,
                INSTANCE_ACTIVE,
                to_i64("prune_batch_rows", policy.prune_batch_rows)?,
            ],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((project, label)) = label else {
        return Ok(0);
    };
    let runtime = connection
        .query_row(
            "SELECT runtime_instance_id FROM usage_instances
             WHERE project_instance_id = ?1 AND caller_label = ?2
             ORDER BY last_seen_at_epoch DESC, instance_row_id DESC LIMIT 1",
            params![project, label],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    upsert_label_tombstone(connection, &project, &label, now, runtime.as_deref())?;
    connection.execute(
        "UPDATE usage_instances SET caller_label = NULL
         WHERE project_instance_id = ?1 AND caller_label = ?2 AND state <> ?3",
        params![project, label, INSTANCE_ACTIVE],
    )?;
    let deleted = connection.execute(
        "DELETE FROM usage_labels WHERE project_instance_id = ?1 AND caller_label = ?2",
        params![project, label],
    )?;
    decrement_retention_counter(connection, RetentionCounter::LabelRows, deleted)?;
    connection.execute(
        "UPDATE usage_retention_state SET label_history_complete = 0 WHERE singleton = 1",
        [],
    )?;
    Ok(deleted)
}

/// Remove one bounded page of old or excess label and runtime tombstones.
fn prune_tombstones_once(
    connection: &Connection,
    policy: TelemetryRetentionPolicy,
    now: i64,
) -> DbResult<usize> {
    let cutoff = epoch_cutoff(
        now,
        policy.retained_tombstone_seconds,
        "retained_tombstone_seconds",
    )?;
    let label_rows = retention_counter(connection, RetentionCounter::LabelTombstoneRows)?;
    let mut statement = connection.prepare_cached(
        "SELECT expired_at_epoch FROM usage_label_tombstones
         ORDER BY expired_at_epoch, project_instance_id, caller_label LIMIT ?1",
    )?;
    let rows = statement.query_map(
        [to_i64("prune_batch_rows", policy.prune_batch_rows)?],
        |row| row.get::<_, i64>(0),
    )?;
    let mut remaining_label_rows = label_rows;
    let mut label_limit = 0usize;
    for expired_at in rows {
        let expired_at = expired_at?;
        if expired_at >= cutoff && remaining_label_rows <= policy.max_label_tombstones {
            break;
        }
        remaining_label_rows =
            remaining_label_rows
                .checked_sub(1)
                .ok_or(DbError::TelemetryIntegerOverflow {
                    field: "label_tombstone_rows",
                })?;
        label_limit = label_limit
            .checked_add(1)
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "label_tombstone_rows",
            })?;
    }
    drop(statement);
    let label = connection.execute(
        "DELETE FROM usage_label_tombstones
         WHERE (project_instance_id, caller_label) IN (
             SELECT project_instance_id, caller_label FROM usage_label_tombstones
             ORDER BY expired_at_epoch, project_instance_id, caller_label LIMIT ?1
         )",
        [to_i64("label_tombstone_prune_rows", label_limit)?],
    )?;
    if label != label_limit {
        return Err(DbError::TelemetryIntegerOverflow {
            field: "label_tombstone_rows",
        });
    }
    let remaining = policy.prune_batch_rows.saturating_sub(label);
    let instance = if remaining == 0 {
        0
    } else {
        let instance_rows = retention_counter(connection, RetentionCounter::InstanceTombstoneRows)?;
        let mut statement = connection.prepare_cached(
            "SELECT retired_at_epoch FROM usage_instance_tombstones
             ORDER BY retired_at_epoch, project_instance_id, runtime_instance_id LIMIT ?1",
        )?;
        let rows = statement
            .query_map([to_i64("tombstone_prune_remaining", remaining)?], |row| {
                row.get::<_, i64>(0)
            })?;
        let mut remaining_instance_rows = instance_rows;
        let mut instance_limit = 0usize;
        for retired_at in rows {
            let retired_at = retired_at?;
            if retired_at >= cutoff && remaining_instance_rows <= policy.max_instance_tombstones {
                break;
            }
            remaining_instance_rows = remaining_instance_rows.checked_sub(1).ok_or(
                DbError::TelemetryIntegerOverflow {
                    field: "instance_tombstone_rows",
                },
            )?;
            instance_limit =
                instance_limit
                    .checked_add(1)
                    .ok_or(DbError::TelemetryIntegerOverflow {
                        field: "instance_tombstone_rows",
                    })?;
        }
        drop(statement);
        let deleted = connection.execute(
            "DELETE FROM usage_instance_tombstones
             WHERE (project_instance_id, runtime_instance_id) IN (
                 SELECT project_instance_id, runtime_instance_id
                 FROM usage_instance_tombstones
                 ORDER BY retired_at_epoch, project_instance_id, runtime_instance_id LIMIT ?1
             )",
            [to_i64("instance_tombstone_prune_rows", instance_limit)?],
        )?;
        if deleted != instance_limit {
            return Err(DbError::TelemetryIntegerOverflow {
                field: "instance_tombstone_rows",
            });
        }
        deleted
    };
    if label != 0 {
        decrement_retention_counter(connection, RetentionCounter::LabelTombstoneRows, label)?;
    }
    if instance != 0 {
        decrement_retention_counter(
            connection,
            RetentionCounter::InstanceTombstoneRows,
            instance,
        )?;
    }
    label
        .checked_add(instance)
        .ok_or(DbError::TelemetryIntegerOverflow {
            field: "evicted_tombstones",
        })
}

/// Rebuild persisted counter state once during the supported telemetry migration.
/// Rebuild persisted retention counters during a supported schema upgrade.
fn reconcile_retention_counters(connection: &Connection) -> DbResult<()> {
    let counts = connection.query_row(
        "SELECT
             (SELECT COUNT(*) FROM usage_events),
             (SELECT COALESCE(SUM(logical_bytes), 0) FROM usage_events),
             (SELECT COUNT(*) FROM usage_instance_baselines),
             (SELECT COALESCE(SUM(witness_logical_bytes), 0)
                FROM usage_instance_baselines),
             (SELECT COUNT(*) FROM usage_bucket_dimensions),
             (SELECT COUNT(*) FROM usage_instances),
             (SELECT COUNT(*) FROM usage_labels),
             (SELECT COUNT(*) FROM usage_daily_aggregates)
                + (SELECT COUNT(*) FROM usage_instance_daily_aggregates),
             (SELECT COUNT(*) FROM usage_label_tombstones),
             (SELECT COUNT(*) FROM usage_instance_tombstones)",
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
            ))
        },
    )?;
    for (field, value) in [
        ("raw_rows", counts.0),
        ("raw_logical_bytes", counts.1),
        ("baseline_rows", counts.2),
        ("baseline_logical_bytes", counts.3),
        ("dimension_rows", counts.4),
        ("instance_rows", counts.5),
        ("label_rows", counts.6),
        ("daily_rows", counts.7),
        ("label_tombstone_rows", counts.8),
        ("instance_tombstone_rows", counts.9),
    ] {
        let _ = count_usize(field, value)?;
    }
    connection.execute(
        "UPDATE usage_retention_state
         SET raw_rows = ?1,
             raw_logical_bytes = ?2,
             baseline_rows = ?3,
             baseline_logical_bytes = ?4,
             dimension_rows = ?5,
             instance_rows = ?6,
             label_rows = ?7,
             daily_rows = ?8,
             label_tombstone_rows = ?9,
             instance_tombstone_rows = ?10
         WHERE singleton = 1",
        params![
            counts.0, counts.1, counts.2, counts.3, counts.4, counts.5, counts.6, counts.7,
            counts.8, counts.9,
        ],
    )?;
    Ok(())
}

/// Probe indexed retention paths for additional age-based maintenance work.
fn aged_maintenance_pending(
    connection: &Connection,
    policy: TelemetryRetentionPolicy,
    now: i64,
) -> DbResult<bool> {
    let instance_cutoff = epoch_cutoff(
        now,
        policy.retained_instance_seconds,
        "retained_instance_seconds",
    )?;
    let trend_seconds = policy
        .retained_trend_days
        .checked_mul(SECONDS_PER_DAY as u64)
        .ok_or(DbError::TelemetryIntegerOverflow {
            field: "retained_trend_seconds",
        })?;
    let daily_cutoff = epoch_cutoff(now, trend_seconds, "retained_trend_seconds")?;
    let label_cutoff = epoch_cutoff(now, policy.retained_label_seconds, "retained_label_seconds")?;
    let tombstone_cutoff = epoch_cutoff(
        now,
        policy.retained_tombstone_seconds,
        "retained_tombstone_seconds",
    )?;
    let pending = connection.query_row(
        "SELECT
             EXISTS(
                 SELECT 1 FROM usage_instances
                 WHERE state IN (?1, ?2) AND last_seen_at_epoch < ?3 LIMIT 1
             ) OR EXISTS(
                 SELECT 1 FROM usage_daily_aggregates
                 WHERE day_epoch < ?4 LIMIT 1
             ) OR EXISTS(
                 SELECT 1 FROM usage_instance_daily_aggregates
                 WHERE day_epoch < ?4 LIMIT 1
             ) OR EXISTS(
                 SELECT 1 FROM usage_labels AS labels
                 WHERE last_seen_at_epoch < ?5
                   AND NOT EXISTS(
                       SELECT 1 FROM usage_instances AS i
                       WHERE i.project_instance_id = labels.project_instance_id
                         AND i.caller_label = labels.caller_label
                         AND i.state = ?6
                   )
                   AND (
                       SELECT COUNT(*) FROM usage_instances AS i
                       WHERE i.project_instance_id = labels.project_instance_id
                         AND i.caller_label = labels.caller_label
                   ) <= ?7
                 LIMIT 1
             ) OR EXISTS(
                 SELECT 1 FROM usage_label_tombstones
                 WHERE expired_at_epoch < ?8 LIMIT 1
             ) OR EXISTS(
                 SELECT 1 FROM usage_instance_tombstones
                 WHERE retired_at_epoch < ?8 LIMIT 1
             )",
        params![
            INSTANCE_SEALED,
            INSTANCE_EXPIRED,
            instance_cutoff,
            daily_cutoff,
            label_cutoff,
            INSTANCE_ACTIVE,
            to_i64("prune_batch_rows", policy.prune_batch_rows)?,
            tombstone_cutoff,
        ],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(pending != 0)
}

#[allow(clippy::too_many_arguments)]
/// Refresh content-free lifecycle state from exact counters and bounded probes.
fn refresh_retention_state(
    connection: &Connection,
    policy: TelemetryRetentionPolicy,
    now: i64,
    pruned_raw: usize,
    pruned_instances: usize,
    evicted_tombstones: usize,
    writes_added: usize,
) -> DbResult<()> {
    let counters = connection.query_row(
        "SELECT raw_rows, raw_logical_bytes, instance_rows, label_rows, daily_rows,
                label_tombstone_rows, instance_tombstone_rows,
                pruned_raw_rows, pruned_instance_rows, evicted_tombstones,
                writes_since_checkpoint
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
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, i64>(10)?,
            ))
        },
    )?;
    let raw_rows = count_usize("raw_rows", counters.0)?;
    let raw_bytes = count_usize("raw_logical_bytes", counters.1)?;
    let instance_rows = count_usize("instance_rows", counters.2)?;
    let label_rows = count_usize("label_rows", counters.3)?;
    let daily_rows = count_usize("daily_rows", counters.4)?;
    let label_tombstones = count_usize("label_tombstone_rows", counters.5)?;
    let instance_tombstones = count_usize("instance_tombstone_rows", counters.6)?;
    let cutoff = epoch_cutoff(now, policy.max_raw_age_seconds, "max_raw_age_seconds")?;
    let old_raw = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM usage_events
             WHERE created_at_epoch < ?1 LIMIT 1
         )",
        [cutoff],
        |row| row.get::<_, i64>(0),
    )?;
    let oldest = connection
        .query_row(
            "SELECT created_at_epoch FROM usage_events
         ORDER BY created_at_epoch, id LIMIT 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let pruned_raw_total = checked_count_add("pruned_raw_rows", counters.7, pruned_raw)?;
    let pruned_instance_total =
        checked_count_add("pruned_instance_rows", counters.8, pruned_instances)?;
    let evicted_total = checked_count_add("evicted_tombstones", counters.9, evicted_tombstones)?;
    let writes = checked_count_add("writes_since_checkpoint", counters.10, writes_added)?;
    let maintenance_pending = raw_rows > policy.max_raw_rows
        || raw_bytes > policy.max_raw_logical_bytes
        || old_raw > 0
        || instance_rows > policy.max_retained_instances
        || label_rows > policy.max_retained_labels
        || daily_rows > policy.max_daily_rows
        || label_tombstones > policy.max_label_tombstones
        || instance_tombstones > policy.max_instance_tombstones
        || count_usize("writes_since_checkpoint", writes)? >= policy.checkpoint_write_interval
        || aged_maintenance_pending(connection, policy, now)?;
    connection.execute(
        "UPDATE usage_retention_state
         SET policy_version = ?1,
             logical_byte_version = ?2,
             pruned_raw_rows = ?3,
             pruned_instance_rows = ?4,
             evicted_tombstones = ?5,
             writes_since_checkpoint = ?6,
             last_maintenance_epoch = ?7,
             oldest_retained_epoch = ?8,
             maintenance_pending = ?9
         WHERE singleton = 1",
        params![
            i64::from(POLICY_VERSION),
            i64::from(LOGICAL_BYTE_VERSION),
            pruned_raw_total,
            pruned_instance_total,
            evicted_total,
            writes,
            now,
            oldest,
            i64::from(maintenance_pending),
        ],
    )?;
    Ok(())
}

/// Converge all bounded retention categories during a supported upgrade.
fn converge_retention(
    connection: &Connection,
    project: ProjectInstanceId,
    policy: TelemetryRetentionPolicy,
    now: i64,
) -> DbResult<()> {
    let mut pruned_raw = 0usize;
    loop {
        let (rows, bytes, old_rows) = raw_pressure(connection, policy, now)?;
        if rows <= policy.max_raw_rows && bytes <= policy.max_raw_logical_bytes && old_rows == 0 {
            break;
        }
        let deleted = prune_raw_once(connection, policy, now)?;
        if deleted == 0 {
            break;
        }
        pruned_raw = pruned_raw
            .checked_add(deleted)
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "pruned_raw_rows",
            })?;
    }
    let mut pruned_instances = 0usize;
    loop {
        if retention_counter(connection, RetentionCounter::InstanceRows)?
            <= policy.max_retained_instances
        {
            break;
        }
        let (deleted, raw) = prune_instances_once(connection, policy, now, 0)?;
        if deleted == 0 {
            break;
        }
        pruned_instances =
            pruned_instances
                .checked_add(deleted)
                .ok_or(DbError::TelemetryIntegerOverflow {
                    field: "pruned_instance_rows",
                })?;
        pruned_raw = pruned_raw
            .checked_add(raw)
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "pruned_raw_rows",
            })?;
    }
    while prune_daily_once(connection, policy, now)? == policy.prune_batch_rows {}
    let mut evicted = 0usize;
    loop {
        let deleted = prune_tombstones_once(connection, policy, now)?;
        evicted = evicted
            .checked_add(deleted)
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "evicted_tombstones",
            })?;
        if deleted < policy.prune_batch_rows {
            break;
        }
    }
    refresh_retention_state(
        connection,
        policy,
        now,
        pruned_raw,
        pruned_instances,
        evicted,
        0,
    )?;
    retention_state_for_project(connection, project).map(|_| ())
}

/// Build the shared raw-event projection with a fixed internal predicate suffix.
fn raw_event_select(extra_predicate: &str) -> String {
    format!(
        "SELECT COALESCE(i.caller_label, ''), e.command, e.path, e.query,
                e.estimated_tokens_without_projectatlas,
                e.estimated_tokens_with_projectatlas, e.estimated_tokens_saved,
                d.token_savings_bucket, d.provider, d.model, d.tokenizer_backend,
                d.accuracy, d.baseline_kind, d.confidence, e.calculation_trace,
                d.accounting_layer, d.estimate_method, d.denominator_kind,
                e.baseline_identity, e.baseline_fingerprint, d.dedupe_scope
         FROM usage_events AS e
         JOIN usage_instances AS i USING(instance_row_id)
         JOIN usage_bucket_dimensions AS d USING(dimension_id)
         WHERE i.project_instance_id = ?1 {extra_predicate}
         ORDER BY e.id"
    )
}

/// Decode one retained raw event and its normalized dimensions.
fn map_usage_event(row: &rusqlite::Row<'_>) -> DbResult<UsageEvent> {
    Ok(UsageEvent {
        session_id: row.get(0)?,
        command: row.get(1)?,
        path: row.get(2)?,
        query: row.get(3)?,
        estimated_tokens_without_projectatlas: row.get(4)?,
        estimated_tokens_with_projectatlas: row.get(5)?,
        estimated_tokens_saved: row.get(6)?,
        token_savings_bucket: row.get(7)?,
        provider: row.get(8)?,
        model: row.get(9)?,
        tokenizer_backend: row.get(10)?,
        accuracy: row.get(11)?,
        baseline_kind: row.get(12)?,
        confidence: row.get(13)?,
        calculation_trace: row.get(14)?,
        accounting_layer: row.get(15)?,
        estimate_method: row.get(16)?,
        denominator_kind: row.get(17)?,
        baseline_identity: row.get(18)?,
        baseline_fingerprint: row.get(19)?,
        dedupe_scope: row.get(20)?,
    })
}

/// Load exact all-time aggregate rows for a project or caller label.
fn load_overview_aggregates(
    connection: &Connection,
    project: ProjectInstanceId,
    caller_label: Option<&str>,
) -> DbResult<Vec<(DimensionValues, AggregateCounters)>> {
    let (sql, label) = if let Some(label) = caller_label {
        (
            "SELECT d.token_savings_bucket, d.provider, d.model, d.tokenizer_backend,
                    d.accuracy, d.baseline_kind, d.confidence, d.accounting_layer,
                    d.estimate_method, d.denominator_kind, d.dedupe_scope, d.overflow,
                    a.calls, a.estimated_without, a.estimated_with,
                    a.observed_without, a.observed_with, a.modeled_without, a.modeled_with,
                    a.deduped_modeled_without, a.deduped_modeled_with,
                    a.repeated_baselines, a.observed_file_read_replacements,
                    a.modeled_file_reads_avoided
             FROM (
                 SELECT aggregate.dimension_id,
                        SUM(aggregate.calls) AS calls,
                        SUM(aggregate.estimated_without) AS estimated_without,
                        SUM(aggregate.estimated_with) AS estimated_with,
                        SUM(aggregate.observed_without) AS observed_without,
                        SUM(aggregate.observed_with) AS observed_with,
                        SUM(aggregate.modeled_without) AS modeled_without,
                        SUM(aggregate.modeled_with) AS modeled_with,
                        SUM(aggregate.deduped_modeled_without) AS deduped_modeled_without,
                        SUM(aggregate.deduped_modeled_with) AS deduped_modeled_with,
                        SUM(aggregate.repeated_baselines) AS repeated_baselines,
                        SUM(aggregate.observed_file_read_replacements)
                            AS observed_file_read_replacements,
                        SUM(aggregate.modeled_file_reads_avoided)
                            AS modeled_file_reads_avoided
                 FROM usage_instance_aggregates AS aggregate
                 JOIN usage_instances AS instance USING(instance_row_id)
                 WHERE instance.project_instance_id = ?1 AND instance.caller_label = ?2
                 GROUP BY aggregate.dimension_id
             ) AS a
             JOIN usage_bucket_dimensions AS d USING(dimension_id)
             ORDER BY d.dimension_id",
            Some(label),
        )
    } else {
        (
            "SELECT d.token_savings_bucket, d.provider, d.model, d.tokenizer_backend,
                    d.accuracy, d.baseline_kind, d.confidence, d.accounting_layer,
                    d.estimate_method, d.denominator_kind, d.dedupe_scope, d.overflow,
                    a.calls, a.estimated_without, a.estimated_with,
                    a.observed_without, a.observed_with, a.modeled_without, a.modeled_with,
                    a.deduped_modeled_without, a.deduped_modeled_with,
                    a.repeated_baselines, a.observed_file_read_replacements,
                    a.modeled_file_reads_avoided
             FROM usage_global_aggregates AS a
             JOIN usage_bucket_dimensions AS d USING(dimension_id)
             WHERE a.project_instance_id = ?1
             ORDER BY d.dimension_id",
            None,
        )
    };
    let mut statement = connection.prepare_cached(sql)?;
    let mut rows = if let Some(label) = label {
        statement.query(params![project.as_bytes().as_slice(), label])?
    } else {
        statement.query([project.as_bytes().as_slice()])?
    };
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push((read_dimension(row, 0)?, read_counters_offset(row, 12)?));
    }
    Ok(result)
}

/// Load bounded worktree lifetime aggregates, grouped by normalized dimension.
fn load_worktree_overview_aggregates(
    connection: &Connection,
    registration_id: Option<i64>,
    synchronized_only: bool,
) -> DbResult<Vec<(DimensionValues, AggregateCounters)>> {
    let predicate = if registration_id.is_some() {
        "a.registration_id = ?1"
    } else if synchronized_only {
        "a.source_kind = ?1"
    } else {
        return Err(DbError::WorktreeRegistrationRow {
            reason: "worktree aggregate scope is unbounded",
        });
    };
    let sql = format!(
        "SELECT d.token_savings_bucket, d.provider, d.model, d.tokenizer_backend,
                d.accuracy, d.baseline_kind, d.confidence, d.accounting_layer,
                d.estimate_method, d.denominator_kind, d.dedupe_scope, d.overflow,
                SUM(a.calls), SUM(a.estimated_without), SUM(a.estimated_with),
                SUM(a.observed_without), SUM(a.observed_with),
                SUM(a.modeled_without), SUM(a.modeled_with),
                SUM(a.deduped_modeled_without), SUM(a.deduped_modeled_with),
                SUM(a.repeated_baselines), SUM(a.observed_file_read_replacements),
                SUM(a.modeled_file_reads_avoided)
         FROM worktree_usage_aggregates AS a
              INDEXED BY idx_worktree_usage_aggregates_day_registration
         JOIN usage_bucket_dimensions AS d USING(dimension_id)
         WHERE a.day_epoch = -1 AND {predicate}
         GROUP BY a.dimension_id
         ORDER BY a.dimension_id"
    );
    let mut statement = connection.prepare_cached(&sql)?;
    let mut rows = if let Some(registration_id) = registration_id {
        statement.query([registration_id])?
    } else {
        statement.query([WORKTREE_USAGE_SYNCHRONIZED])?
    };
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push((read_dimension(row, 0)?, read_counters_offset(row, 12)?));
    }
    Ok(result)
}

/// Return whether any routed or synchronized worktree totals exist.
fn worktree_aggregates_exist(connection: &Connection) -> DbResult<bool> {
    connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1
                 FROM worktree_usage_aggregates
                      INDEXED BY idx_worktree_usage_aggregates_day_registration
                 WHERE day_epoch = -1
                 LIMIT 1
             )",
            [],
            |row| row.get(0),
        )
        .map_err(DbError::from)
}

/// Combine normalized aggregate rows into one overview and bounded buckets.
fn aggregate_report_rows(
    rows: Vec<(DimensionValues, AggregateCounters)>,
) -> DbResult<(Vec<TokenBucketOverview>, TokenAccountingTotals, bool)> {
    let mut by_dimension = BTreeMap::<DimensionValues, AggregateCounters>::new();
    for (dimension, counters) in rows {
        let entry = by_dimension.entry(dimension).or_default();
        *entry = entry.checked_add(counters)?;
    }
    let mut totals = TokenAccountingTotals::default();
    let mut buckets = Vec::with_capacity(by_dimension.len());
    let mut average_directory_without = 0u128;
    let mut average_directory_with = 0u128;
    let mut average_policy_complete = true;
    for (dimension, counters) in by_dimension {
        if dimension.overflow
            && dimension.denominator_kind == OVERFLOW_DIMENSION
            && counters.modeled_without > 0
        {
            average_policy_complete = false;
        }
        totals.measured_tokens_saved = totals
            .measured_tokens_saved
            .checked_add(component_difference(
                counters.observed_without,
                counters.observed_with,
            ))
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "measured_tokens_saved",
            })?;
        totals.gross_modeled_tokens_avoided = totals
            .gross_modeled_tokens_avoided
            .checked_add(component_difference(
                counters.modeled_without,
                counters.modeled_with,
            ))
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "gross_modeled_tokens_avoided",
            })?;
        let deduped_modeled_delta = component_difference(
            counters.deduped_modeled_without,
            counters.deduped_modeled_with,
        );
        totals.deduped_modeled_tokens_avoided = totals
            .deduped_modeled_tokens_avoided
            .checked_add(deduped_modeled_delta)
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "deduped_modeled_tokens_avoided",
            })?;
        if dimension.denominator_kind == TOKEN_BASELINE_DIRECTORY_WALK {
            let emitted_with = count_u128("modeled_with", counters.modeled_with)?;
            let retained_without = if deduped_modeled_delta >= 0 {
                emitted_with.checked_add(deduped_modeled_delta.unsigned_abs())
            } else {
                emitted_with.checked_sub(deduped_modeled_delta.unsigned_abs())
            }
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "average_directory_without",
            })?;
            average_directory_without = average_directory_without
                .checked_add(retained_without)
                .ok_or(DbError::TelemetryIntegerOverflow {
                    field: "average_directory_without",
                })?;
            average_directory_with = average_directory_with.checked_add(emitted_with).ok_or(
                DbError::TelemetryIntegerOverflow {
                    field: "average_directory_with",
                },
            )?;
        } else {
            totals.average_modeled_tokens_avoided = totals
                .average_modeled_tokens_avoided
                .checked_add(deduped_modeled_delta)
                .ok_or(DbError::TelemetryIntegerOverflow {
                    field: "average_modeled_tokens_avoided",
                })?;
        }
        totals.repeated_baselines_deduped = totals
            .repeated_baselines_deduped
            .checked_add(count_u128(
                "repeated_baselines",
                counters.repeated_baselines,
            )?)
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "repeated_baselines",
            })?;
        totals.observed_file_read_replacements = totals
            .observed_file_read_replacements
            .checked_add(count_u128(
                "observed_file_read_replacements",
                counters.observed_file_read_replacements,
            )?)
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "observed_file_read_replacements",
            })?;
        totals.modeled_file_reads_avoided = totals
            .modeled_file_reads_avoided
            .checked_add(count_u128(
                "modeled_file_reads_avoided",
                counters.modeled_file_reads_avoided,
            )?)
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "modeled_file_reads_avoided",
            })?;
        buckets.push(bucket_from_counters(dimension, counters)?);
    }
    let average_directory_without =
        average_modeled_baseline_tokens(TOKEN_BASELINE_DIRECTORY_WALK, average_directory_without);
    let average_directory_delta = i128::try_from(average_directory_without)
        .ok()
        .and_then(|without| {
            i128::try_from(average_directory_with)
                .ok()
                .and_then(|with| without.checked_sub(with))
        })
        .ok_or(DbError::TelemetryIntegerOverflow {
            field: "average_modeled_tokens_avoided",
        })?;
    totals.average_modeled_tokens_avoided = totals
        .average_modeled_tokens_avoided
        .checked_add(average_directory_delta)
        .ok_or(DbError::TelemetryIntegerOverflow {
            field: "average_modeled_tokens_avoided",
        })?;
    Ok((buckets, totals, average_policy_complete))
}

/// Convert one normalized counter row into the public bucket contract.
fn bucket_from_counters(
    dimension: DimensionValues,
    counters: AggregateCounters,
) -> DbResult<TokenBucketOverview> {
    Ok(TokenBucketOverview::from_totals(
        dimension.token_savings_bucket,
        dimension.provider,
        dimension.model,
        dimension.tokenizer_backend,
        dimension.accuracy,
        dimension.baseline_kind,
        dimension.confidence,
        dimension.accounting_layer,
        dimension.estimate_method,
        dimension.denominator_kind,
        dimension.dedupe_scope,
        count_u128("calls", counters.calls)?,
        count_u128("estimated_without", counters.estimated_without)?,
        count_u128("estimated_with", counters.estimated_with)?,
    ))
}

/// Load retained daily aggregates for one project or caller label.
fn load_daily_aggregates(
    connection: &Connection,
    project: ProjectInstanceId,
    caller_label: Option<&str>,
    window: TokenTrendWindow,
) -> DbResult<Vec<(String, DimensionValues, AggregateCounters)>> {
    let period_expression = match window {
        TokenTrendWindow::Day => "strftime('%Y-%m-%d', a.day_epoch, 'unixepoch')",
        TokenTrendWindow::Week => "strftime('%Y-W%W', a.day_epoch, 'unixepoch')",
        TokenTrendWindow::Month => "strftime('%Y-%m', a.day_epoch, 'unixepoch')",
        TokenTrendWindow::Year => "strftime('%Y', a.day_epoch, 'unixepoch')",
    };
    let (table, join, predicate) = if caller_label.is_some() {
        (
            "usage_instance_daily_aggregates",
            "JOIN usage_instances AS i USING(instance_row_id)",
            "i.project_instance_id = ?1 AND i.caller_label = ?2",
        )
    } else {
        ("usage_daily_aggregates", "", "a.project_instance_id = ?1")
    };
    let sql = format!(
        "SELECT grouped.period,
                d.token_savings_bucket, d.provider, d.model, d.tokenizer_backend,
                d.accuracy, d.baseline_kind, d.confidence, d.accounting_layer,
                d.estimate_method, d.denominator_kind, d.dedupe_scope, d.overflow,
                grouped.calls, grouped.estimated_without, grouped.estimated_with,
                grouped.observed_without, grouped.observed_with,
                grouped.modeled_without, grouped.modeled_with,
                grouped.deduped_modeled_without, grouped.deduped_modeled_with,
                grouped.repeated_baselines, grouped.observed_file_read_replacements,
                grouped.modeled_file_reads_avoided
         FROM (
             SELECT {period_expression} AS period, a.dimension_id,
                    SUM(a.calls) AS calls,
                    SUM(a.estimated_without) AS estimated_without,
                    SUM(a.estimated_with) AS estimated_with,
                    SUM(a.observed_without) AS observed_without,
                    SUM(a.observed_with) AS observed_with,
                    SUM(a.modeled_without) AS modeled_without,
                    SUM(a.modeled_with) AS modeled_with,
                    SUM(a.deduped_modeled_without) AS deduped_modeled_without,
                    SUM(a.deduped_modeled_with) AS deduped_modeled_with,
                    SUM(a.repeated_baselines) AS repeated_baselines,
                    SUM(a.observed_file_read_replacements)
                        AS observed_file_read_replacements,
                    SUM(a.modeled_file_reads_avoided) AS modeled_file_reads_avoided
             FROM {table} AS a {join}
             WHERE {predicate}
             GROUP BY period, a.dimension_id
         ) AS grouped
         JOIN usage_bucket_dimensions AS d USING(dimension_id)
         ORDER BY grouped.period, grouped.dimension_id"
    );
    let mut statement = connection.prepare(&sql)?;
    let mut rows = if let Some(label) = caller_label {
        statement.query(params![project.as_bytes().as_slice(), label])?
    } else {
        statement.query([project.as_bytes().as_slice()])?
    };
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push((
            row.get::<_, String>(0)?,
            read_dimension(row, 1)?,
            read_counters_offset(row, 13)?,
        ));
    }
    Ok(result)
}

/// Load bounded worktree daily aggregates for combined or exact-origin trends.
fn load_worktree_daily_aggregates(
    connection: &Connection,
    registration_id: Option<i64>,
    synchronized_only: bool,
    window: TokenTrendWindow,
) -> DbResult<Vec<(String, DimensionValues, AggregateCounters)>> {
    let period_expression = match window {
        TokenTrendWindow::Day => "strftime('%Y-%m-%d', a.day_epoch, 'unixepoch')",
        TokenTrendWindow::Week => "strftime('%Y-W%W', a.day_epoch, 'unixepoch')",
        TokenTrendWindow::Month => "strftime('%Y-%m', a.day_epoch, 'unixepoch')",
        TokenTrendWindow::Year => "strftime('%Y', a.day_epoch, 'unixepoch')",
    };
    let predicate = if registration_id.is_some() {
        "a.registration_id = ?1"
    } else if synchronized_only {
        "a.source_kind = ?1"
    } else {
        return Err(DbError::WorktreeRegistrationRow {
            reason: "worktree trend scope is unbounded",
        });
    };
    let sql = format!(
        "SELECT grouped.period,
                d.token_savings_bucket, d.provider, d.model, d.tokenizer_backend,
                d.accuracy, d.baseline_kind, d.confidence, d.accounting_layer,
                d.estimate_method, d.denominator_kind, d.dedupe_scope, d.overflow,
                grouped.calls, grouped.estimated_without, grouped.estimated_with,
                grouped.observed_without, grouped.observed_with,
                grouped.modeled_without, grouped.modeled_with,
                grouped.deduped_modeled_without, grouped.deduped_modeled_with,
                grouped.repeated_baselines, grouped.observed_file_read_replacements,
                grouped.modeled_file_reads_avoided
         FROM (
             SELECT {period_expression} AS period, a.dimension_id,
                    SUM(a.calls) AS calls,
                    SUM(a.estimated_without) AS estimated_without,
                    SUM(a.estimated_with) AS estimated_with,
                    SUM(a.observed_without) AS observed_without,
                    SUM(a.observed_with) AS observed_with,
                    SUM(a.modeled_without) AS modeled_without,
                    SUM(a.modeled_with) AS modeled_with,
                    SUM(a.deduped_modeled_without) AS deduped_modeled_without,
                    SUM(a.deduped_modeled_with) AS deduped_modeled_with,
                    SUM(a.repeated_baselines) AS repeated_baselines,
                    SUM(a.observed_file_read_replacements)
                        AS observed_file_read_replacements,
                    SUM(a.modeled_file_reads_avoided) AS modeled_file_reads_avoided
             FROM worktree_usage_aggregates AS a
                  INDEXED BY idx_worktree_usage_aggregates_day_registration
             WHERE a.day_epoch >= 0 AND {predicate}
             GROUP BY period, a.dimension_id
         ) AS grouped
         JOIN usage_bucket_dimensions AS d USING(dimension_id)
         ORDER BY grouped.period, grouped.dimension_id"
    );
    let mut statement = connection.prepare(&sql)?;
    let mut rows = if let Some(registration_id) = registration_id {
        statement.query([registration_id])?
    } else {
        statement.query([WORKTREE_USAGE_SYNCHRONIZED])?
    };
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push((
            row.get::<_, String>(0)?,
            read_dimension(row, 1)?,
            read_counters_offset(row, 13)?,
        ));
    }
    Ok(result)
}

/// Classify retained, partial, expired, or unavailable caller detail.
fn detail_availability(
    connection: &Connection,
    project: ProjectInstanceId,
    caller_label: Option<&str>,
) -> DbResult<UsageDetailAvailability> {
    let state = connection.query_row(
        "SELECT raw_detail_complete, dimension_detail_complete, label_history_complete
         FROM usage_retention_state WHERE singleton = 1",
        [],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        },
    )?;
    let global_complete = bool_from_sql("raw_detail_complete", state.0)?
        && bool_from_sql("dimension_detail_complete", state.1)?
        && bool_from_sql("label_history_complete", state.2)?;
    let Some(label) = caller_label else {
        return Ok(if global_complete {
            UsageDetailAvailability::Retained
        } else {
            UsageDetailAvailability::Partial
        });
    };
    let instances = connection.query_row(
        "SELECT COUNT(*), MIN(raw_detail_complete)
         FROM usage_instances
         WHERE project_instance_id = ?1 AND caller_label = ?2",
        params![project.as_bytes().as_slice(), label],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
    )?;
    let label_detail = connection
        .query_row(
            "SELECT detail_complete FROM usage_labels
             WHERE project_instance_id = ?1 AND caller_label = ?2",
            params![project.as_bytes().as_slice(), label],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if instances.0 > 0 {
        return Ok(
            if global_complete && instances.1 == Some(1) && label_detail == Some(1) {
                UsageDetailAvailability::Retained
            } else {
                UsageDetailAvailability::Partial
            },
        );
    }
    let tombstone = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM usage_label_tombstones
             WHERE project_instance_id = ?1 AND caller_label = ?2
         )",
        params![project.as_bytes().as_slice(), label],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(if tombstone != 0 || label_detail == Some(0) {
        UsageDetailAvailability::Expired
    } else {
        UsageDetailAvailability::Unavailable
    })
}

/// Decode one normalized dimension beginning at the selected column offset.
fn read_dimension(row: &rusqlite::Row<'_>, offset: usize) -> DbResult<DimensionValues> {
    Ok(DimensionValues {
        token_savings_bucket: row.get(offset)?,
        provider: row.get(offset + 1)?,
        model: row.get(offset + 2)?,
        tokenizer_backend: row.get(offset + 3)?,
        accuracy: row.get(offset + 4)?,
        baseline_kind: row.get(offset + 5)?,
        confidence: row.get(offset + 6)?,
        accounting_layer: row.get(offset + 7)?,
        estimate_method: row.get(offset + 8)?,
        denominator_kind: row.get(offset + 9)?,
        dedupe_scope: row.get(offset + 10)?,
        overflow: bool_from_sql("usage_bucket_dimensions.overflow", row.get(offset + 11)?)?,
    })
}

/// Decode aggregate counters beginning at the selected column offset.
fn read_counters_offset(row: &rusqlite::Row<'_>, offset: usize) -> DbResult<AggregateCounters> {
    Ok(AggregateCounters {
        calls: row.get(offset)?,
        estimated_without: row.get(offset + 1)?,
        estimated_with: row.get(offset + 2)?,
        observed_without: row.get(offset + 3)?,
        observed_with: row.get(offset + 4)?,
        modeled_without: row.get(offset + 5)?,
        modeled_with: row.get(offset + 6)?,
        deduped_modeled_without: row.get(offset + 7)?,
        deduped_modeled_with: row.get(offset + 8)?,
        repeated_baselines: row.get(offset + 9)?,
        observed_file_read_replacements: row.get(offset + 10)?,
        modeled_file_reads_avoided: row.get(offset + 11)?,
    })
}

/// Calculate deterministic retained logical bytes for one raw event.
fn logical_event_bytes(event: &UsageEvent, label: Option<&str>) -> DbResult<usize> {
    let identity = event.effective_baseline_identity();
    let fingerprint = event.effective_baseline_fingerprint();
    [
        label.unwrap_or_default(),
        event.command.as_str(),
        event.path.as_deref().unwrap_or_default(),
        event.query.as_deref().unwrap_or_default(),
        event.token_savings_bucket.as_str(),
        event.provider.as_str(),
        event.model.as_str(),
        event.tokenizer_backend.as_str(),
        event.accuracy.as_str(),
        event.baseline_kind.as_str(),
        event.confidence.as_str(),
        event.calculation_trace.as_str(),
        event.report_accounting_layer(),
        event.estimate_method.as_str(),
        event.report_denominator_kind(),
        identity.as_ref(),
        fingerprint.as_ref(),
        event.report_dedupe_scope(),
    ]
    .into_iter()
    .try_fold(96_usize, |total, value| {
        total
            .checked_add(value.len())
            .ok_or(DbError::TelemetryIntegerOverflow {
                field: "raw_logical_bytes",
            })
    })
}

/// Encode one signed value as separate nonnegative positive and negative components.
fn signed_components(value: i64) -> DbResult<(i64, i64)> {
    if value >= 0 {
        Ok((value, 0))
    } else {
        Ok((
            0,
            value
                .checked_neg()
                .ok_or(DbError::TelemetryIntegerOverflow {
                    field: "signed_component",
                })?,
        ))
    }
}

/// Reconstruct a signed aggregate from its nonnegative components.
fn component_difference(without: i64, with: i64) -> i128 {
    i128::from(without) - i128::from(with)
}

/// Add an unsigned count to a persisted `SQLite` integer exactly.
fn checked_count_add(field: &'static str, previous: i64, value: usize) -> DbResult<i64> {
    previous
        .checked_add(to_i64(field, value)?)
        .ok_or(DbError::TelemetryIntegerOverflow { field })
}

/// Convert one bounded duration to milliseconds without truncation.
fn duration_millis(field: &'static str, duration: Duration) -> DbResult<u64> {
    duration
        .as_secs()
        .checked_mul(1_000)
        .and_then(|milliseconds| milliseconds.checked_add(u64::from(duration.subsec_millis())))
        .ok_or(DbError::TelemetryIntegerOverflow { field })
}

/// Read one persisted retention counter as an unsigned Rust count.
fn retention_counter(connection: &Connection, counter: RetentionCounter) -> DbResult<usize> {
    let value = connection.query_row(counter.select_sql(), [], |row| row.get::<_, i64>(0))?;
    count_usize(counter.field(), value)
}

/// Increase one persisted retention counter with checked arithmetic.
fn increment_retention_counter(
    connection: &Connection,
    counter: RetentionCounter,
    amount: usize,
) -> DbResult<()> {
    let value = retention_counter(connection, counter)?
        .checked_add(amount)
        .ok_or(DbError::TelemetryIntegerOverflow {
            field: counter.field(),
        })?;
    connection.execute(counter.update_sql(), [to_i64(counter.field(), value)?])?;
    Ok(())
}

/// Decrease one persisted retention counter with checked arithmetic.
fn decrement_retention_counter(
    connection: &Connection,
    counter: RetentionCounter,
    amount: usize,
) -> DbResult<()> {
    let current = retention_counter(connection, counter)?;
    let value = current
        .checked_sub(amount)
        .ok_or(DbError::TelemetryIntegerOverflow {
            field: counter.field(),
        })?;
    connection.execute(counter.update_sql(), [to_i64(counter.field(), value)?])?;
    Ok(())
}

/// Decode a strict zero-or-one `SQLite` boolean.
fn bool_from_sql(field: &'static str, value: i64) -> DbResult<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(DbError::InvalidEnum {
            field,
            value: value.to_string(),
        }),
    }
}

/// Convert an optional unsigned count to a `SQLite` integer.
fn option_usize_to_i64(field: &'static str, value: Option<usize>) -> DbResult<Option<i64>> {
    value.map(|value| to_i64(field, value)).transpose()
}

/// Convert an optional signed count to a `SQLite` integer.
fn option_isize_to_i64(field: &'static str, value: Option<isize>) -> DbResult<Option<i64>> {
    value
        .map(|value| {
            i64::try_from(value).map_err(|_source| DbError::TelemetryIntegerOverflow { field })
        })
        .transpose()
}

/// Convert one integer-like value into the exact `SQLite` integer range.
fn to_i64(field: &'static str, value: impl TryInto<i64>) -> DbResult<i64> {
    value
        .try_into()
        .map_err(|_source| DbError::TelemetryIntegerOverflow { field })
}

/// Decode one nonnegative persisted count as `usize`.
fn count_usize(field: &'static str, value: i64) -> DbResult<usize> {
    usize::try_from(value).map_err(|_source| DbError::TelemetryIntegerOverflow { field })
}

/// Decode one nonnegative persisted count as `u32`.
fn count_u32(field: &'static str, value: i64) -> DbResult<u32> {
    u32::try_from(value).map_err(|_source| DbError::TelemetryIntegerOverflow { field })
}

/// Decode one nonnegative persisted count as `u64`.
fn count_u64(field: &'static str, value: i64) -> DbResult<u64> {
    u64::try_from(value).map_err(|_source| DbError::TelemetryIntegerOverflow { field })
}

/// Decode one nonnegative persisted count as `u128`.
fn count_u128(field: &'static str, value: i64) -> DbResult<u128> {
    u128::try_from(value).map_err(|_source| DbError::TelemetryIntegerOverflow { field })
}

/// Calculate a nonnegative retention cutoff from an epoch and duration.
fn epoch_cutoff(now: i64, seconds: u64, field: &'static str) -> DbResult<i64> {
    let seconds = to_i64(field, seconds)?;
    Ok(now.checked_sub(seconds).unwrap_or(0))
}

/// Read the current Unix epoch in the persisted integer range.
fn now_epoch_seconds() -> DbResult<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_source| DbError::TelemetryIntegerOverflow {
            field: "system_time",
        })?;
    to_i64("system_time", duration.as_secs())
}

/// Read one allowlisted numeric `SQLite` pragma.
fn pragma_count(connection: &Connection, pragma: &str) -> DbResult<i64> {
    let sql = match pragma {
        "freelist_count" => "PRAGMA freelist_count",
        "page_count" => "PRAGMA page_count",
        "page_size" => "PRAGMA page_size",
        _ => {
            return Err(DbError::InvalidEnum {
                field: "telemetry_pragma",
                value: pragma.to_string(),
            });
        }
    };
    connection
        .query_row(sql, [], |row| row.get(0))
        .map_err(Into::into)
}

/// Decode the numeric `SQLite` synchronous pragma into its stable name.
fn synchronous_mode(value: i64) -> DbResult<&'static str> {
    match value {
        0 => Ok("off"),
        1 => Ok("normal"),
        2 => Ok("full"),
        3 => Ok("extra"),
        _ => Err(DbError::InvalidEnum {
            field: "pragma.synchronous",
            value: value.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AtlasStore, WorktreeAlias, WorktreeRegistrationState};
    use projectatlas_core::telemetry::{
        TOKEN_BASELINE_DIRECTORY_WALK, TOKEN_DEDUPE_SCOPE_EVENT, usage_from_estimates,
        usage_from_text,
    };
    use projectatlas_core::{Node, NodeKind, normalized_parent};
    use rusqlite::{Transaction, TransactionBehavior};
    use std::cell::RefCell;
    use std::error::Error;
    use std::fs;
    use std::io;
    use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

    thread_local! {
        /// Statements executed by the connection under the intended-scale worktree probe.
        static WORKTREE_TRACE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        /// One deterministic pause between revision and aggregate reads.
        static WORKTREE_SNAPSHOT_EXPORT_BLOCKER: RefCell<Option<SnapshotExportBlocker>> = const { RefCell::new(None) };
    }

    struct SnapshotExportBlocker {
        entered: SyncSender<()>,
        resume: Receiver<()>,
    }

    fn record_worktree_statement(sql: &str) {
        WORKTREE_TRACE.with(|statements| statements.borrow_mut().push(sql.to_string()));
    }

    fn block_worktree_snapshot_aggregate_query(sql: &str) {
        if !sql.contains("SELECT -1 AS day_epoch") {
            return;
        }
        WORKTREE_SNAPSHOT_EXPORT_BLOCKER.with(|slot| {
            let Some(blocker) = slot.borrow_mut().take() else {
                return;
            };
            if blocker.entered.send(()).is_ok() {
                let _resume = blocker.resume.recv_timeout(Duration::from_secs(10));
            }
        });
    }

    struct TestDatabase {
        temp: tempfile::TempDir,
        connection: Connection,
        project: ProjectInstanceId,
    }

    struct ProductionTestDatabase {
        _temp: tempfile::TempDir,
        root: std::path::PathBuf,
        database_path: std::path::PathBuf,
        store: AtlasStore,
        project: ProjectInstanceId,
    }

    fn test_database() -> Result<TestDatabase, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let database_path = temp.path().join("projectatlas.db");
        let connection = Connection::open(&database_path)?;
        crate::schema::initialize(&connection, None)?;
        let project = crate::project_identity::ensure_project_identity(&connection)?.0;
        Ok(TestDatabase {
            temp,
            connection,
            project,
        })
    }

    fn production_database() -> Result<ProductionTestDatabase, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("repository");
        let atlas = root.join(".projectatlas");
        fs::create_dir_all(&atlas)?;
        let database_path = atlas.join("projectatlas.db");
        let store = AtlasStore::open_for_project(&database_path, &root)?;
        let project = store
            .validated_project_instance_id
            .ok_or(DbError::ProjectInstanceIdentityMissing)?;
        Ok(ProductionTestDatabase {
            _temp: temp,
            root,
            database_path,
            store,
            project,
        })
    }

    fn store_at(root: &std::path::Path) -> Result<AtlasStore, Box<dyn Error>> {
        let atlas = root.join(".projectatlas");
        fs::create_dir_all(&atlas)?;
        Ok(AtlasStore::open_for_project(
            &atlas.join("projectatlas.db"),
            root,
        )?)
    }

    /// Return a test error instead of panicking inside a fallible test.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(std::io::Error::other(message).into())
        }
    }

    /// Compare test values without panicking inside a fallible test.
    fn require_eq<T>(actual: &T, expected: &T, label: &str) -> Result<(), Box<dyn Error>>
    where
        T: std::fmt::Debug + PartialEq,
    {
        if actual == expected {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "{label} mismatch: expected {expected:?}, found {actual:?}"
            ))
            .into())
        }
    }

    fn instance(byte: u8) -> Result<UsageInstanceId, Box<dyn Error>> {
        Ok(UsageInstanceId::from_bytes([byte; 16])?)
    }

    fn event(label: &str, without: usize, with: usize) -> UsageEvent {
        let mut event = usage_from_estimates(
            label,
            "summary",
            Some("src/lib.rs".to_string()),
            None,
            without,
            with,
        );
        event.baseline_identity = "source:src/lib.rs".to_string();
        event.baseline_fingerprint = "source:src/lib.rs:v1".to_string();
        event
    }

    fn directory_event(label: &str, without: usize, with: usize) -> UsageEvent {
        let mut event = event(label, without, with);
        event.command = "folders".to_string();
        event.denominator_kind = TOKEN_BASELINE_DIRECTORY_WALK.to_string();
        event.baseline_identity = "directory:src".to_string();
        event.baseline_fingerprint = "directory:src:v1".to_string();
        event
    }

    fn record_transaction(
        connection: &Connection,
        project: ProjectInstanceId,
        instance: UsageInstanceId,
        owner: UsageInstanceOwner,
        event: &UsageEvent,
        policy: TelemetryRetentionPolicy,
        seal_after_record: bool,
    ) -> DbResult<()> {
        record_transaction_at(
            connection,
            project,
            instance,
            owner,
            event,
            policy,
            seal_after_record,
            now_epoch_seconds()?,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn record_transaction_at(
        connection: &Connection,
        project: ProjectInstanceId,
        instance: UsageInstanceId,
        owner: UsageInstanceOwner,
        event: &UsageEvent,
        policy: TelemetryRetentionPolicy,
        seal_after_record: bool,
        now: i64,
    ) -> DbResult<()> {
        let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
        let result = (|| {
            crate::project_identity::require_bound_project_identity(&transaction, project)?;
            let policy = policy.validate()?;
            validate_event(event, policy)?;
            record_usage_at(
                &transaction,
                project,
                instance,
                owner,
                None,
                true,
                event,
                policy,
                now,
                seal_after_record,
                BaselineAdmission::BoundedRuntime,
                DimensionAdmission::Event,
            )
        })();
        match result {
            Ok(()) => transaction.commit().map_err(Into::into),
            Err(error) => {
                transaction.rollback()?;
                Err(error)
            }
        }
    }

    fn scalar_count(connection: &Connection, sql: &str) -> Result<usize, Box<dyn Error>> {
        let value = connection.query_row(sql, [], |row| row.get::<_, i64>(0))?;
        Ok(usize::try_from(value)?)
    }

    fn query_plan(connection: &Connection, sql: &str) -> Result<Vec<String>, Box<dyn Error>> {
        let mut statement = connection.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
        let rows = statement.query_map([], |row| row.get::<_, String>(3))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn assert_plan_uses(plan: &[String], owner: &str) {
        assert!(
            plan.iter().any(|detail| detail.contains(owner)),
            "expected query plan to use {owner}; plan was {plan:?}"
        );
    }

    #[test]
    fn retention_policy_rejects_zero_and_inverted_instance_limits() {
        let policy = TelemetryRetentionPolicy {
            prune_batch_rows: 0,
            ..TelemetryRetentionPolicy::default()
        };
        assert!(policy.validate().is_err());

        let default_policy = TelemetryRetentionPolicy::default();
        let policy = TelemetryRetentionPolicy {
            max_retained_instances: default_policy.max_active_instances - 1,
            ..default_policy
        };
        assert!(policy.validate().is_err());
    }

    #[test]
    fn oversized_event_fields_are_rejected_at_the_typed_storage_boundary() {
        type Mutator = fn(&mut UsageEvent, String);

        let policy = TelemetryRetentionPolicy::default();
        let dimension_limit = policy.max_dimension_bytes;
        let short_witness_limit = 256.min(policy.max_baseline_witness_bytes);
        let cases: [(&str, usize, Mutator); 18] = [
            ("session_id", policy.max_label_bytes, |event, value| {
                event.session_id = value;
            }),
            ("command", policy.max_command_bytes, |event, value| {
                event.command = value;
            }),
            ("path", policy.max_path_bytes, |event, value| {
                event.path = Some(value);
            }),
            ("query", policy.max_query_bytes, |event, value| {
                event.query = Some(value);
            }),
            ("token_savings_bucket", dimension_limit, |event, value| {
                event.token_savings_bucket = value;
            }),
            ("provider", dimension_limit, |event, value| {
                event.provider = value;
            }),
            ("model", dimension_limit, |event, value| {
                event.model = value;
            }),
            ("tokenizer_backend", dimension_limit, |event, value| {
                event.tokenizer_backend = value;
            }),
            ("accuracy", dimension_limit, |event, value| {
                event.accuracy = value;
            }),
            ("baseline_kind", dimension_limit, |event, value| {
                event.baseline_kind = value;
            }),
            ("confidence", dimension_limit, |event, value| {
                event.confidence = value;
            }),
            ("accounting_layer", dimension_limit, |event, value| {
                event.accounting_layer = value;
            }),
            ("estimate_method", dimension_limit, |event, value| {
                event.estimate_method = value;
            }),
            ("denominator_kind", dimension_limit, |event, value| {
                event.denominator_kind = value;
            }),
            ("dedupe_scope", dimension_limit, |event, value| {
                event.dedupe_scope = value;
            }),
            ("calculation_trace", short_witness_limit, |event, value| {
                event.calculation_trace = value;
            }),
            (
                "baseline_identity",
                policy.max_baseline_witness_bytes,
                |event, value| {
                    event.baseline_identity = value;
                },
            ),
            (
                "baseline_fingerprint",
                short_witness_limit,
                |event, value| {
                    event.baseline_fingerprint = value;
                },
            ),
        ];
        for (field, limit, mutate) in cases {
            let mut candidate = event("bounded", 10, 1);
            mutate(&mut candidate, "x".repeat(limit + 1));
            assert!(matches!(
                validate_event(&candidate, policy),
                Err(DbError::TelemetryFieldTooLarge {
                    field: found,
                    bytes,
                    limit: found_limit,
                }) if found == field && bytes == limit + 1 && found_limit == limit
            ));
        }
    }

    #[test]
    fn spill_cleanup_is_not_applicable() {
        assert_eq!(
            SpillCleanupState::NotApplicable,
            SpillCleanupState::NotApplicable
        );
        assert_eq!(POLICY_VERSION, 1);
        assert_eq!(LOGICAL_BYTE_VERSION, 1);
    }

    #[test]
    fn on_disk_instances_preserve_exact_aggregates_and_atomic_sealing() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let database = test_database()?;
            let policy = TelemetryRetentionPolicy::default();
            let first = instance(1)?;
            let second = instance(2)?;
            record_transaction(
                &database.connection,
                database.project,
                first,
                UsageInstanceOwner::McpProcess,
                &event("agent", 100, 10),
                policy,
                false,
            )?;
            record_transaction(
                &database.connection,
                database.project,
                first,
                UsageInstanceOwner::McpProcess,
                &event("agent", 100, 95),
                policy,
                true,
            )?;
            record_transaction(
                &database.connection,
                database.project,
                second,
                UsageInstanceOwner::McpProcess,
                &event("agent", 40, 10),
                policy,
                false,
            )?;

            let overview =
                token_overview_for_project(&database.connection, database.project, Some("agent"))?;
            assert_eq!(overview.calls, 3);
            assert_eq!(overview.deduped_modeled_tokens_avoided, 25);
            assert_eq!(overview.repeated_baselines_deduped, 1);
            assert_eq!(
                overview.detail_availability,
                UsageDetailAvailability::Retained
            );
            assert_eq!(
                scalar_count(
                    &database.connection,
                    "SELECT COUNT(*) FROM usage_instances WHERE caller_label = 'agent'",
                )?,
                2
            );
            assert_eq!(
                scalar_count(
                    &database.connection,
                    "SELECT COUNT(*) FROM usage_instance_baselines",
                )?,
                1
            );

            let inactive = record_transaction(
                &database.connection,
                database.project,
                first,
                UsageInstanceOwner::McpProcess,
                &event("agent", 20, 5),
                policy,
                false,
            );
            assert!(matches!(inactive, Err(DbError::TelemetryInstanceInactive)));
            let mismatched = record_transaction(
                &database.connection,
                database.project,
                second,
                UsageInstanceOwner::CliInvocation,
                &event("agent", 20, 5),
                policy,
                false,
            );
            assert!(matches!(
                mismatched,
                Err(DbError::TelemetryInstanceMismatch)
            ));

            let database_path = database.temp.path().join("projectatlas.db");
            drop(database.connection);
            let reopened = Connection::open(database_path)?;
            crate::schema::initialize(&reopened, None)?;
            let reopened_overview =
                token_overview_for_project(&reopened, database.project, Some("agent"))?;
            assert_eq!(reopened_overview.calls, overview.calls);
            assert_eq!(
                reopened_overview.deduped_modeled_tokens_avoided,
                overview.deduped_modeled_tokens_avoided
            );
            Ok(())
        })();
        assert!(result.is_ok(), "on-disk telemetry test failed: {result:?}");
    }

    #[test]
    fn sqlite_average_and_maximum_match_raw_event_accounting() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let database = test_database()?;
            let policy = TelemetryRetentionPolicy::default();
            let mut second_folder_scope = directory_event("agent", 5, 1);
            second_folder_scope.baseline_identity = "directory:tests".to_string();
            second_folder_scope.baseline_fingerprint = "directory:tests:v1".to_string();
            let events = vec![
                directory_event("agent", 101, 20),
                directory_event("agent", 101, 10),
                second_folder_scope,
                event("agent", 80, 20),
                usage_from_text(
                    "agent",
                    "summary",
                    Some("src/other.rs".to_string()),
                    None,
                    "abcdabcd",
                    "ab",
                ),
            ];
            let expected = TokenOverview::from_events(&events);

            record_transaction(
                &database.connection,
                database.project,
                instance(21)?,
                UsageInstanceOwner::McpProcess,
                &events[0],
                policy,
                false,
            )?;
            record_transaction(
                &database.connection,
                database.project,
                instance(21)?,
                UsageInstanceOwner::McpProcess,
                &events[1],
                policy,
                true,
            )?;
            for (identity, event) in [(22, &events[2]), (23, &events[3]), (24, &events[4])] {
                record_transaction(
                    &database.connection,
                    database.project,
                    instance(identity)?,
                    UsageInstanceOwner::McpProcess,
                    event,
                    policy,
                    true,
                )?;
            }

            let actual =
                token_overview_for_project(&database.connection, database.project, Some("agent"))?;
            assert_eq!(actual.average_modeled_tokens_avoided, 82);
            assert_eq!(actual.average_tokens_avoided, 83);
            assert_eq!(actual.maximum_tokens_avoided, 136);
            assert_eq!(actual.tokens_avoided, actual.average_tokens_avoided);
            assert_eq!(
                actual.average_modeled_tokens_avoided,
                expected.average_modeled_tokens_avoided
            );
            assert_eq!(
                actual.average_tokens_avoided,
                expected.average_tokens_avoided
            );
            assert_eq!(
                actual.maximum_tokens_avoided,
                expected.maximum_tokens_avoided
            );
            assert_eq!(
                actual.deduped_modeled_tokens_avoided,
                expected.deduped_modeled_tokens_avoided
            );
            assert_eq!(actual.buckets, expected.buckets);
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "SQLite average/maximum parity test failed: {result:?}"
        );
    }

    #[test]
    fn directory_overflow_retains_average_policy_and_raw_parity() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let database = test_database()?;
            let policy = TelemetryRetentionPolicy {
                max_dimensions: 2,
                ..TelemetryRetentionPolicy::default()
            };
            let selected = event("overflow-average", 100, 10);
            let folder = directory_event("overflow-average", 101, 20);
            let expected = TokenOverview::from_events(&[selected.clone(), folder.clone()]);

            record_transaction(
                &database.connection,
                database.project,
                instance(25)?,
                UsageInstanceOwner::McpProcess,
                &selected,
                policy,
                false,
            )?;
            record_transaction(
                &database.connection,
                database.project,
                instance(25)?,
                UsageInstanceOwner::McpProcess,
                &folder,
                policy,
                true,
            )?;

            let actual = token_overview_for_project(
                &database.connection,
                database.project,
                Some("overflow-average"),
            )?;
            assert_eq!(actual.average_tokens_avoided, 120);
            assert_eq!(actual.maximum_tokens_avoided, 171);
            assert_eq!(
                actual.average_tokens_avoided,
                expected.average_tokens_avoided
            );
            assert_eq!(
                actual.maximum_tokens_avoided,
                expected.maximum_tokens_avoided
            );
            assert_eq!(actual.detail_availability, UsageDetailAvailability::Partial);
            assert_eq!(
                scalar_count(
                    &database.connection,
                    "SELECT COUNT(*) FROM usage_bucket_dimensions",
                )?,
                3
            );
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "directory overflow average-policy test failed: {result:?}"
        );
    }

    #[test]
    fn sqlite_and_raw_accounting_narrow_once_at_signed_bounds() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let database = test_database()?;
            let policy = TelemetryRetentionPolicy::default();
            let bound = isize::MAX as usize;
            let mut events = vec![
                event("wide", bound, 0),
                event("wide", bound, 0),
                event("wide", 0, bound),
            ];
            for (index, event) in events.iter_mut().enumerate() {
                event.provider = format!("wide-{index}");
                event.dedupe_scope = TOKEN_DEDUPE_SCOPE_EVENT.to_string();
            }
            let expected = TokenOverview::from_events(&events);

            for (index, event) in events.iter().enumerate() {
                record_transaction(
                    &database.connection,
                    database.project,
                    instance(30 + u8::try_from(index)?)?,
                    UsageInstanceOwner::McpProcess,
                    event,
                    policy,
                    true,
                )?;
            }

            let actual =
                token_overview_for_project(&database.connection, database.project, Some("wide"))?;
            assert_eq!(expected.average_tokens_avoided, isize::MAX);
            assert_eq!(
                actual.average_tokens_avoided,
                expected.average_tokens_avoided
            );
            assert_eq!(
                actual.maximum_tokens_avoided,
                expected.maximum_tokens_avoided
            );
            assert_eq!(
                actual.deduped_modeled_tokens_avoided,
                expected.deduped_modeled_tokens_avoided
            );
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "SQLite wide signed accounting parity test failed: {result:?}"
        );
    }

    #[test]
    fn bounded_raw_and_label_retention_preserve_global_truth() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let database = test_database()?;
            let mut policy = TelemetryRetentionPolicy {
                max_raw_rows: 2,
                prune_batch_rows: 1,
                max_retained_labels: 1,
                ..TelemetryRetentionPolicy::default()
            };
            policy.checkpoint_write_interval = usize::MAX;
            let first = instance(3)?;
            record_transaction(
                &database.connection,
                database.project,
                first,
                UsageInstanceOwner::McpProcess,
                &event("first", 100, 10),
                policy,
                true,
            )?;
            let second = instance(4)?;
            record_transaction(
                &database.connection,
                database.project,
                second,
                UsageInstanceOwner::McpProcess,
                &event("second", 80, 20),
                policy,
                false,
            )?;
            record_transaction(
                &database.connection,
                database.project,
                second,
                UsageInstanceOwner::McpProcess,
                &event("second", 70, 20),
                policy,
                false,
            )?;

            let state = retention_state_for_project(&database.connection, database.project)?;
            assert_eq!(state.raw_rows, 2);
            assert_eq!(state.retained_label_rows, 1);
            assert_eq!(state.label_tombstone_rows, 1);
            assert_eq!(state.spill_cleanup, SpillCleanupState::NotApplicable);
            let global = token_overview_for_project(&database.connection, database.project, None)?;
            assert_eq!(global.calls, 3);
            assert_eq!(global.deduped_modeled_tokens_avoided, 130);
            let expired =
                token_overview_for_project(&database.connection, database.project, Some("first"))?;
            assert_eq!(
                expired.detail_availability,
                UsageDetailAvailability::Expired
            );
            let unavailable = token_overview_for_project(
                &database.connection,
                database.project,
                Some("never-recorded"),
            )?;
            assert_eq!(
                unavailable.detail_availability,
                UsageDetailAvailability::Unavailable
            );
            let retained =
                token_overview_for_project(&database.connection, database.project, Some("second"))?;
            assert_eq!(retained.calls, 2);
            assert_eq!(
                retained.detail_availability,
                UsageDetailAvailability::Partial
            );

            record_transaction(
                &database.connection,
                database.project,
                second,
                UsageInstanceOwner::McpProcess,
                &event("second", 60, 20),
                policy,
                false,
            )?;
            assert_eq!(
                token_overview_for_project(&database.connection, database.project, Some("second"))?
                    .calls,
                3
            );
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "bounded telemetry retention test failed: {result:?}"
        );
    }

    #[test]
    fn raw_age_and_logical_byte_retention_preserve_exact_aggregates() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let aged = test_database()?;
            let aged_policy = TelemetryRetentionPolicy {
                max_raw_age_seconds: 10,
                prune_batch_rows: 1,
                checkpoint_write_interval: usize::MAX,
                ..TelemetryRetentionPolicy::default()
            };
            let aged_runtime = instance(20)?;
            record_transaction_at(
                &aged.connection,
                aged.project,
                aged_runtime,
                UsageInstanceOwner::McpProcess,
                &event("aged", 100, 10),
                aged_policy,
                false,
                1_000,
            )?;
            record_transaction_at(
                &aged.connection,
                aged.project,
                aged_runtime,
                UsageInstanceOwner::McpProcess,
                &event("aged", 100, 10),
                aged_policy,
                false,
                1_020,
            )?;
            let aged_state = retention_state_for_project(&aged.connection, aged.project)?;
            assert_eq!(aged_state.raw_rows, 1);
            assert_eq!(aged_state.pruned_raw_rows, 1);
            let aged_overview =
                token_overview_for_project(&aged.connection, aged.project, Some("aged"))?;
            assert_eq!(aged_overview.calls, 2);
            assert_eq!(aged_overview.deduped_modeled_tokens_avoided, 80);
            assert_eq!(
                aged_overview.detail_availability,
                UsageDetailAvailability::Partial
            );

            let byte_bounded = test_database()?;
            let sample = event("bytes", 100, 10);
            let one_event_bytes = logical_event_bytes(&sample, Some("bytes"))?;
            let byte_policy = TelemetryRetentionPolicy {
                max_raw_logical_bytes: one_event_bytes + 1,
                prune_batch_rows: 1,
                checkpoint_write_interval: usize::MAX,
                ..TelemetryRetentionPolicy::default()
            };
            let byte_runtime = instance(21)?;
            record_transaction_at(
                &byte_bounded.connection,
                byte_bounded.project,
                byte_runtime,
                UsageInstanceOwner::McpProcess,
                &sample,
                byte_policy,
                false,
                2_000,
            )?;
            record_transaction_at(
                &byte_bounded.connection,
                byte_bounded.project,
                byte_runtime,
                UsageInstanceOwner::McpProcess,
                &sample,
                byte_policy,
                false,
                2_001,
            )?;
            let byte_state =
                retention_state_for_project(&byte_bounded.connection, byte_bounded.project)?;
            assert_eq!(byte_state.raw_rows, 1);
            assert!(byte_state.raw_logical_bytes <= byte_policy.max_raw_logical_bytes);
            assert_eq!(byte_state.pruned_raw_rows, 1);
            let byte_overview = token_overview_for_project(
                &byte_bounded.connection,
                byte_bounded.project,
                Some("bytes"),
            )?;
            assert_eq!(byte_overview.calls, 2);
            assert_eq!(byte_overview.deduped_modeled_tokens_avoided, 80);
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "raw telemetry budget test failed: {result:?}"
        );
    }

    #[test]
    fn production_batch_raw_retention_removes_only_the_required_oldest_prefix() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let aged = test_database()?;
            let aged_policy = TelemetryRetentionPolicy {
                max_raw_rows: 10,
                max_raw_age_seconds: 10,
                prune_batch_rows: TelemetryRetentionPolicy::default().prune_batch_rows,
                checkpoint_write_interval: usize::MAX,
                ..TelemetryRetentionPolicy::default()
            };
            let runtime = instance(22)?;
            for now in [1_000, 1_020, 1_021] {
                record_transaction_at(
                    &aged.connection,
                    aged.project,
                    runtime,
                    UsageInstanceOwner::McpProcess,
                    &event("aged-prefix", 100, 10),
                    aged_policy,
                    false,
                    now,
                )?;
            }
            let epochs = aged
                .connection
                .prepare("SELECT created_at_epoch FROM usage_events ORDER BY created_at_epoch")?
                .query_map([], |row| row.get::<_, i64>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            assert_eq!(epochs, vec![1_020, 1_021]);

            let capped = test_database()?;
            let capped_policy = TelemetryRetentionPolicy {
                max_raw_rows: 2,
                prune_batch_rows: TelemetryRetentionPolicy::default().prune_batch_rows,
                checkpoint_write_interval: usize::MAX,
                ..TelemetryRetentionPolicy::default()
            };
            let runtime = instance(23)?;
            for now in [2_000, 2_001, 2_002] {
                record_transaction_at(
                    &capped.connection,
                    capped.project,
                    runtime,
                    UsageInstanceOwner::McpProcess,
                    &event("row-prefix", 100, 10),
                    capped_policy,
                    false,
                    now,
                )?;
            }
            let epochs = capped
                .connection
                .prepare("SELECT created_at_epoch FROM usage_events ORDER BY created_at_epoch")?
                .query_map([], |row| row.get::<_, i64>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            assert_eq!(epochs, vec![2_001, 2_002]);
            assert_eq!(
                retention_state_for_project(&capped.connection, capped.project)?.pruned_raw_rows,
                1
            );
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "exact raw-prefix retention test failed: {result:?}"
        );
    }

    #[test]
    fn production_batch_tombstone_retention_removes_only_exact_excess() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let database = test_database()?;
            let policy = TelemetryRetentionPolicy {
                max_label_tombstones: 2,
                max_instance_tombstones: 2,
                prune_batch_rows: TelemetryRetentionPolicy::default().prune_batch_rows,
                checkpoint_write_interval: usize::MAX,
                ..TelemetryRetentionPolicy::default()
            };
            for (offset, label) in ["first", "second", "third"].into_iter().enumerate() {
                upsert_label_tombstone(
                    &database.connection,
                    database.project.as_bytes().as_slice(),
                    label,
                    1_000 + i64::try_from(offset)?,
                    None,
                )?;
            }
            for (offset, runtime) in [31_u8, 32, 33].into_iter().enumerate() {
                database.connection.execute(
                    "INSERT INTO usage_instance_tombstones(
                         project_instance_id, runtime_instance_id, retired_at_epoch
                     ) VALUES(?1, ?2, ?3)",
                    params![
                        database.project.as_bytes().as_slice(),
                        instance(runtime)?.as_bytes().as_slice(),
                        1_000 + i64::try_from(offset)?,
                    ],
                )?;
            }
            increment_retention_counter(
                &database.connection,
                RetentionCounter::InstanceTombstoneRows,
                3,
            )?;
            let deleted = prune_tombstones_once(&database.connection, policy, 1_100)?;
            assert_eq!(deleted, 2);
            assert_eq!(
                scalar_count(
                    &database.connection,
                    "SELECT COUNT(*) FROM usage_label_tombstones",
                )?,
                2
            );
            assert_eq!(
                scalar_count(
                    &database.connection,
                    "SELECT COUNT(*) FROM usage_instance_tombstones",
                )?,
                2
            );
            assert_eq!(
                database.connection.query_row(
                    "SELECT caller_label FROM usage_label_tombstones
                     ORDER BY expired_at_epoch LIMIT 1",
                    [],
                    |row| row.get::<_, String>(0),
                )?,
                "second"
            );
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "exact tombstone retention test failed: {result:?}"
        );
    }

    #[test]
    fn daily_capacity_prunes_old_history_before_reserving_both_current_rows() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let database = test_database()?;
            let policy = TelemetryRetentionPolicy {
                max_daily_rows: 4,
                retained_trend_days: 1,
                prune_batch_rows: TelemetryRetentionPolicy::default().prune_batch_rows,
                checkpoint_write_interval: usize::MAX,
                ..TelemetryRetentionPolicy::default()
            };
            let runtime = instance(24)?;
            let mut first = event("daily", 100, 10);
            first.provider = "first-provider".to_string();
            let mut second = event("daily", 90, 10);
            second.provider = "second-provider".to_string();
            for value in [&first, &second] {
                record_transaction_at(
                    &database.connection,
                    database.project,
                    runtime,
                    UsageInstanceOwner::McpProcess,
                    value,
                    policy,
                    false,
                    SECONDS_PER_DAY,
                )?;
            }
            assert_eq!(
                retention_counter(&database.connection, RetentionCounter::DailyRows)?,
                4
            );

            let mut current = event("daily", 80, 10);
            current.provider = "current-provider".to_string();
            let current_epoch = 3 * SECONDS_PER_DAY;
            record_transaction_at(
                &database.connection,
                database.project,
                runtime,
                UsageInstanceOwner::McpProcess,
                &current,
                policy,
                false,
                current_epoch,
            )?;
            assert_eq!(
                retention_counter(&database.connection, RetentionCounter::DailyRows)?,
                2
            );
            let current_rows = database.connection.query_row(
                "SELECT
                     (SELECT COUNT(*) FROM usage_daily_aggregates AS aggregate
                      JOIN usage_bucket_dimensions AS dimension USING(dimension_id)
                      WHERE aggregate.day_epoch = ?1 AND dimension.provider = ?2),
                     (SELECT COUNT(*) FROM usage_instance_daily_aggregates AS aggregate
                      JOIN usage_bucket_dimensions AS dimension USING(dimension_id)
                      WHERE aggregate.day_epoch = ?1 AND dimension.provider = ?2)",
                params![current_epoch, "current-provider"],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )?;
            assert_eq!(current_rows, (1, 1));
            let trends = token_trends_for_project(
                &database.connection,
                database.project,
                Some("daily"),
                TokenTrendWindow::Day,
            )?;
            assert_eq!(trends.periods.len(), 1);
            assert_eq!(trends.periods[0].calls, 1);
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "daily capacity reservation test failed: {result:?}"
        );
    }

    #[test]
    fn label_reports_group_many_instances_inside_sqlite() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let database = test_database()?;
            let policy = TelemetryRetentionPolicy {
                checkpoint_write_interval: usize::MAX,
                ..TelemetryRetentionPolicy::default()
            };
            for runtime in 1_u8..=48 {
                record_transaction_at(
                    &database.connection,
                    database.project,
                    instance(runtime)?,
                    UsageInstanceOwner::McpProcess,
                    &event("grouped", 100, 10),
                    policy,
                    true,
                    10 * SECONDS_PER_DAY,
                )?;
            }
            let overview = token_overview_for_project(
                &database.connection,
                database.project,
                Some("grouped"),
            )?;
            assert_eq!(overview.calls, 48);
            assert_eq!(overview.buckets.len(), 1);
            let trends = token_trends_for_project(
                &database.connection,
                database.project,
                Some("grouped"),
                TokenTrendWindow::Day,
            )?;
            assert_eq!(trends.periods.len(), 1);
            assert_eq!(trends.periods[0].calls, 48);
            assert_eq!(trends.periods[0].buckets.len(), 1);
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "SQLite-side telemetry grouping test failed: {result:?}"
        );
    }

    #[test]
    fn new_project_runtime_reclaims_inactive_instance_and_label_capacity() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let database = test_database()?;
            let policy = TelemetryRetentionPolicy {
                max_active_instances: 1,
                max_retained_instances: 1,
                max_retained_labels: 1,
                ..TelemetryRetentionPolicy::default()
            };
            let old_project = database.project;
            let old_runtime = instance(30)?;
            record_transaction(
                &database.connection,
                old_project,
                old_runtime,
                UsageInstanceOwner::McpProcess,
                &event("old-project", 100, 10),
                policy,
                false,
            )?;
            assert_eq!(
                seal_project_usage_instances(&database.connection, old_project)?,
                1
            );
            let sealed_state = retention_state_for_project(&database.connection, old_project)?;
            assert_eq!(sealed_state.baseline_rows, 0);

            let new_project = ProjectInstanceId::from_bytes([31; 16])?;
            database.connection.execute(
                "UPDATE project_identity SET project_instance_id = ?1 WHERE singleton = 1",
                [new_project.as_bytes().as_slice()],
            )?;
            let new_runtime = old_runtime;
            record_transaction(
                &database.connection,
                new_project,
                new_runtime,
                UsageInstanceOwner::McpProcess,
                &event("new-project", 80, 20),
                policy,
                false,
            )?;

            assert_eq!(
                scalar_count(&database.connection, "SELECT COUNT(*) FROM usage_instances")?,
                1
            );
            assert_eq!(
                scalar_count(&database.connection, "SELECT COUNT(*) FROM usage_labels")?,
                1
            );
            assert_eq!(
                scalar_count(
                    &database.connection,
                    "SELECT COUNT(*) FROM usage_global_aggregates",
                )?,
                1
            );
            assert_eq!(
                scalar_count(
                    &database.connection,
                    "SELECT COUNT(*) FROM usage_labels WHERE caller_label = 'new-project'",
                )?,
                1
            );
            let old_instance_tombstones = database.connection.query_row(
                "SELECT COUNT(*) FROM usage_instance_tombstones
             WHERE project_instance_id = ?1",
                [old_project.as_bytes().as_slice()],
                |row| row.get::<_, i64>(0),
            )?;
            assert_eq!(old_instance_tombstones, 1);
            let old_label_tombstones = database.connection.query_row(
                "SELECT COUNT(*) FROM usage_label_tombstones
             WHERE project_instance_id = ?1",
                [old_project.as_bytes().as_slice()],
                |row| row.get::<_, i64>(0),
            )?;
            assert_eq!(old_label_tombstones, 1);
            let overview = token_overview_for_project(&database.connection, new_project, None)?;
            assert_eq!(overview.calls, 1);
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "project-rotation telemetry capacity test failed: {result:?}"
        );
    }

    #[test]
    fn active_labels_and_reserved_overflow_dimension_remain_disjoint() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let database = test_database()?;
            let policy = TelemetryRetentionPolicy {
                max_retained_labels: 1,
                max_dimensions: 2,
                ..TelemetryRetentionPolicy::default()
            };
            let active = instance(5)?;
            let mut reserved = event("active", 100, 10);
            reserved.token_savings_bucket = OVERFLOW_DIMENSION.to_string();
            reserved.provider = OVERFLOW_DIMENSION.to_string();
            reserved.model = OVERFLOW_DIMENSION.to_string();
            reserved.tokenizer_backend = OVERFLOW_DIMENSION.to_string();
            reserved.accuracy = OVERFLOW_DIMENSION.to_string();
            reserved.baseline_kind = OVERFLOW_DIMENSION.to_string();
            reserved.confidence = OVERFLOW_DIMENSION.to_string();
            reserved.accounting_layer = OVERFLOW_DIMENSION.to_string();
            reserved.estimate_method = OVERFLOW_DIMENSION.to_string();
            reserved.denominator_kind = OVERFLOW_DIMENSION.to_string();
            reserved.dedupe_scope = OVERFLOW_DIMENSION.to_string();
            record_transaction(
                &database.connection,
                database.project,
                active,
                UsageInstanceOwner::McpProcess,
                &reserved,
                policy,
                false,
            )?;
            assert_eq!(
                scalar_count(
                    &database.connection,
                    "SELECT COUNT(*) FROM usage_bucket_dimensions
                 WHERE token_savings_bucket = '<overflow>'",
                )?,
                2
            );
            assert_eq!(
                scalar_count(
                    &database.connection,
                    "SELECT COUNT(DISTINCT overflow) FROM usage_bucket_dimensions
                 WHERE token_savings_bucket = '<overflow>'",
                )?,
                2
            );

            let rejected = record_transaction(
                &database.connection,
                database.project,
                instance(6)?,
                UsageInstanceOwner::McpProcess,
                &event("replacement", 50, 10),
                policy,
                false,
            );
            assert!(matches!(rejected, Err(DbError::TelemetryInstanceCapacity)));
            assert_eq!(
                scalar_count(
                    &database.connection,
                    "SELECT COUNT(*) FROM usage_labels WHERE caller_label = 'active'",
                )?,
                1
            );
            assert_eq!(
                scalar_count(
                    &database.connection,
                    "SELECT COUNT(*) FROM usage_label_tombstones",
                )?,
                0
            );
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "active-label overflow sentinel test failed: {result:?}"
        );
    }

    #[test]
    fn dimension_capacity_uses_reserved_overflow_and_reports_partial_detail() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let database = test_database()?;
            let policy = TelemetryRetentionPolicy {
                max_dimensions: 2,
                ..TelemetryRetentionPolicy::default()
            };
            let runtime = instance(35)?;
            record_transaction(
                &database.connection,
                database.project,
                runtime,
                UsageInstanceOwner::McpProcess,
                &event("dimensions", 100, 10),
                policy,
                false,
            )?;
            let mut overflowed = event("dimensions", 50, 10);
            overflowed.provider = "another-provider".to_string();
            overflowed.baseline_identity = "source:src/other.rs".to_string();
            overflowed.baseline_fingerprint = "source:src/other.rs:v1".to_string();
            record_transaction(
                &database.connection,
                database.project,
                runtime,
                UsageInstanceOwner::McpProcess,
                &overflowed,
                policy,
                false,
            )?;

            assert_eq!(
                scalar_count(
                    &database.connection,
                    "SELECT COUNT(*) FROM usage_bucket_dimensions",
                )?,
                2
            );
            let overflow_calls = database.connection.query_row(
                "SELECT aggregate.calls
             FROM usage_global_aggregates AS aggregate
             JOIN usage_bucket_dimensions AS dimension USING(dimension_id)
             WHERE aggregate.project_instance_id = ?1 AND dimension.overflow = 1",
                [database.project.as_bytes().as_slice()],
                |row| row.get::<_, i64>(0),
            )?;
            assert_eq!(overflow_calls, 1);
            let overview = token_overview_for_project(
                &database.connection,
                database.project,
                Some("dimensions"),
            )?;
            assert_eq!(overview.calls, 2);
            assert_eq!(
                overview.detail_availability,
                UsageDetailAvailability::Partial
            );
            assert_eq!(
                overview.average_policy.evidence,
                TOKEN_AVERAGE_POLICY_OVERFLOW_EVIDENCE
            );
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "dimension overflow retention test failed: {result:?}"
        );
    }

    #[test]
    fn rejected_events_and_read_only_reports_leave_storage_unchanged() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let database = test_database()?;
            let policy = TelemetryRetentionPolicy::default();
            let runtime = instance(7)?;
            record_transaction(
                &database.connection,
                database.project,
                runtime,
                UsageInstanceOwner::McpProcess,
                &event("reader", 100, 10),
                policy,
                false,
            )?;
            let counters_before = database.connection.query_row(
                "SELECT raw_rows, baseline_rows, instance_rows, daily_rows,
                    writes_since_checkpoint
             FROM usage_retention_state WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )?;
            let mut oversized = event("reader", 10, 5);
            oversized.query = Some("x".repeat(policy.max_query_bytes + 1));
            let rejected = record_transaction(
                &database.connection,
                database.project,
                runtime,
                UsageInstanceOwner::McpProcess,
                &oversized,
                policy,
                false,
            );
            assert!(matches!(
                rejected,
                Err(DbError::TelemetryFieldTooLarge { .. })
            ));
            let _ =
                usage_events_for_project(&database.connection, database.project, Some("reader"))?;
            let _ =
                token_overview_for_project(&database.connection, database.project, Some("reader"))?;
            let _ = token_trends_for_project(
                &database.connection,
                database.project,
                Some("reader"),
                TokenTrendWindow::Day,
            )?;
            let state = retention_state_for_project(&database.connection, database.project)?;
            assert_eq!(state.spill_cleanup, SpillCleanupState::NotApplicable);
            let counters_after = database.connection.query_row(
                "SELECT raw_rows, baseline_rows, instance_rows, daily_rows,
                    writes_since_checkpoint
             FROM usage_retention_state WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )?;
            assert_eq!(counters_after, counters_before);

            let storage_before = database.connection.query_row(
                "SELECT
                 (SELECT COUNT(*) FROM usage_events),
                 (SELECT COUNT(*) FROM usage_instance_baselines),
                 (SELECT calls FROM usage_global_aggregates),
                 raw_rows,
                 baseline_rows
             FROM usage_retention_state WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )?;
            database.connection.execute_batch(
                "CREATE TEMP TRIGGER reject_telemetry_aggregate_update
             BEFORE UPDATE ON usage_global_aggregates
             BEGIN
                 SELECT RAISE(ABORT, 'injected telemetry aggregate failure');
             END;",
            )?;
            let mut late_failure = event("reader", 200, 20);
            late_failure.baseline_identity = "source:src/lib.rs:second".to_string();
            late_failure.baseline_fingerprint = "source:src/lib.rs:v2".to_string();
            assert!(
                record_transaction(
                    &database.connection,
                    database.project,
                    runtime,
                    UsageInstanceOwner::McpProcess,
                    &late_failure,
                    policy,
                    false,
                )
                .is_err()
            );
            database
                .connection
                .execute_batch("DROP TRIGGER reject_telemetry_aggregate_update")?;
            let storage_after = database.connection.query_row(
                "SELECT
                 (SELECT COUNT(*) FROM usage_events),
                 (SELECT COUNT(*) FROM usage_instance_baselines),
                 (SELECT calls FROM usage_global_aggregates),
                 raw_rows,
                 baseline_rows
             FROM usage_retention_state WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )?;
            assert_eq!(storage_after, storage_before);

            let lookalike = database.temp.path().join("usage-telemetry-spill.db");
            std::fs::write(&lookalike, b"not owned by ProjectAtlas")?;
            maintain_after_commit_for_project(
                &database.connection,
                None,
                database.project,
                policy,
            )?;
            assert_eq!(std::fs::read(lookalike)?, b"not owned by ProjectAtlas");
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "telemetry rejection rollback test failed: {result:?}"
        );
    }

    #[test]
    fn baseline_capacity_collision_and_integer_overflow_roll_back_completely() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let database = test_database()?;
            let policy = TelemetryRetentionPolicy {
                max_baselines_per_instance: 1,
                ..TelemetryRetentionPolicy::default()
            };

            let capacity_runtime = instance(40)?;
            record_transaction(
                &database.connection,
                database.project,
                capacity_runtime,
                UsageInstanceOwner::McpProcess,
                &event("capacity", 100, 10),
                policy,
                false,
            )?;
            let capacity_before = database.connection.query_row(
                "SELECT
                 (SELECT COUNT(*) FROM usage_events),
                 (SELECT COUNT(*) FROM usage_instance_baselines),
                 (SELECT SUM(calls) FROM usage_global_aggregates),
                 raw_rows,
                 baseline_rows
             FROM usage_retention_state WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )?;
            let mut second_baseline = event("capacity", 90, 10);
            second_baseline.baseline_identity = "source:src/other.rs".to_string();
            second_baseline.baseline_fingerprint = "source:src/other.rs:v1".to_string();
            assert!(matches!(
                record_transaction(
                    &database.connection,
                    database.project,
                    capacity_runtime,
                    UsageInstanceOwner::McpProcess,
                    &second_baseline,
                    policy,
                    false,
                ),
                Err(DbError::TelemetryBaselineCapacity)
            ));
            let capacity_after = database.connection.query_row(
                "SELECT
                 (SELECT COUNT(*) FROM usage_events),
                 (SELECT COUNT(*) FROM usage_instance_baselines),
                 (SELECT SUM(calls) FROM usage_global_aggregates),
                 raw_rows,
                 baseline_rows
             FROM usage_retention_state WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )?;
            assert_eq!(capacity_after, capacity_before);

            let collision_runtime = instance(41)?;
            let collision_event = event("collision", 60, 10);
            record_transaction(
                &database.connection,
                database.project,
                collision_runtime,
                UsageInstanceOwner::McpProcess,
                &collision_event,
                TelemetryRetentionPolicy::default(),
                false,
            )?;
            database.connection.execute(
                "UPDATE usage_instance_baselines
             SET baseline_identity = 'source:src/foo.rs'
             WHERE instance_row_id = (
                 SELECT instance_row_id FROM usage_instances
                 WHERE project_instance_id = ?1 AND runtime_instance_id = ?2
             )",
                params![
                    database.project.as_bytes().as_slice(),
                    collision_runtime.as_bytes().as_slice()
                ],
            )?;
            let collision_raw_before =
                scalar_count(&database.connection, "SELECT COUNT(*) FROM usage_events")?;
            assert!(matches!(
                record_transaction(
                    &database.connection,
                    database.project,
                    collision_runtime,
                    UsageInstanceOwner::McpProcess,
                    &collision_event,
                    TelemetryRetentionPolicy::default(),
                    false,
                ),
                Err(DbError::TelemetryBaselineCollision)
            ));
            assert_eq!(
                scalar_count(&database.connection, "SELECT COUNT(*) FROM usage_events")?,
                collision_raw_before
            );

            let overflow_runtime = instance(42)?;
            record_transaction(
                &database.connection,
                database.project,
                overflow_runtime,
                UsageInstanceOwner::McpProcess,
                &event("overflow", 50, 10),
                TelemetryRetentionPolicy::default(),
                false,
            )?;
            database
                .connection
                .execute("UPDATE usage_global_aggregates SET calls = ?1", [i64::MAX])?;
            let overflow_raw_before =
                scalar_count(&database.connection, "SELECT COUNT(*) FROM usage_events")?;
            let overflow_baselines_before = scalar_count(
                &database.connection,
                "SELECT COUNT(*) FROM usage_instance_baselines",
            )?;
            assert!(matches!(
                record_transaction(
                    &database.connection,
                    database.project,
                    overflow_runtime,
                    UsageInstanceOwner::McpProcess,
                    &event("overflow", 50, 10),
                    TelemetryRetentionPolicy::default(),
                    false,
                ),
                Err(DbError::TelemetryIntegerOverflow {
                    field: AGGREGATE_COUNTER_FIELD
                })
            ));
            assert_eq!(
                scalar_count(&database.connection, "SELECT COUNT(*) FROM usage_events")?,
                overflow_raw_before
            );
            assert_eq!(
                scalar_count(
                    &database.connection,
                    "SELECT COUNT(*) FROM usage_instance_baselines",
                )?,
                overflow_baselines_before
            );
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "telemetry baseline rollback test failed: {result:?}"
        );
    }

    #[test]
    fn production_store_reports_live_page_and_connection_policy_state() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let mut database = production_database()?;
            let initial = database.store.telemetry_retention_state()?;
            assert!(initial.page_count > 0);
            assert_eq!(initial.checkpoint_state, TelemetryCheckpointState::NotDue);
            assert_eq!(
                initial.statistics_policy,
                PlannerStatisticsPolicy::NotConfigured
            );
            assert_eq!(
                initial.statistics_state,
                PlannerStatisticsState::NotInitialized
            );
            assert_eq!(
                initial.connection_busy_timeout_ms,
                initial.normal_busy_timeout_ms
            );
            assert_eq!(initial.normal_busy_timeout_ms, 5_000);
            assert_eq!(initial.telemetry_busy_timeout_ms, 25);
            assert!(initial.wal_autocheckpoint_pages > 0);

            let nodes = (0..1_024)
                .map(|index| {
                    let path = format!("src/generated/{index:04}.rs");
                    Node {
                        parent_path: normalized_parent(&path),
                        path,
                        kind: NodeKind::File,
                        extension: Some(".rs".to_string()),
                        language: Some("rust".to_string()),
                        size_bytes: Some(4_096),
                        mtime_ns: Some(i64::from(index)),
                        content_hash: Some(format!("hash-{index:04}")),
                    }
                })
                .collect::<Vec<_>>();
            database.store.replace_scan(&nodes)?;
            let grown = database.store.telemetry_retention_state()?;
            let live_grown_pages = pragma_count(&database.store.connection, "page_count")?;
            assert_eq!(
                grown.page_count,
                count_usize("page_count", live_grown_pages)?
            );
            assert!(grown.page_count >= initial.page_count);

            database.store.replace_scan(&[])?;
            let deleted = database.store.telemetry_retention_state()?;
            assert_eq!(
                deleted.freelist_pages,
                count_usize(
                    "freelist_pages",
                    pragma_count(&database.store.connection, "freelist_count")?,
                )?
            );
            assert_eq!(
                deleted.page_count,
                count_usize(
                    "page_count",
                    pragma_count(&database.store.connection, "page_count")?,
                )?
            );
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "production page-policy report test failed: {result:?}"
        );
    }

    #[test]
    fn checkpoint_finalization_preserves_writes_committed_after_attempt() {
        assert_eq!(
            writes_after_checkpoint_attempt(
                TelemetryCheckpointState::Completed,
                1_024,
                1_025,
                true,
            ),
            1
        );
        assert_eq!(
            writes_after_checkpoint_attempt(
                TelemetryCheckpointState::Completed,
                1_024,
                1_025,
                false,
            ),
            1_025
        );
        assert_eq!(
            writes_after_checkpoint_attempt(TelemetryCheckpointState::Busy, 1_024, 1_025, true,),
            1_025
        );
        assert_eq!(
            writes_after_checkpoint_attempt(TelemetryCheckpointState::Error, 1_024, 1_025, true,),
            1_025
        );
    }

    #[test]
    fn overlapping_checkpoint_finalizers_preserve_later_event_debt() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let database = production_database()?;
            let policy = TelemetryRetentionPolicy::default();
            let first = AtlasStore::open_for_project(&database.database_path, &database.root)?;
            let second = AtlasStore::open_for_project(&database.database_path, &database.root)?;
            let event_writer =
                AtlasStore::open_for_project(&database.database_path, &database.root)?;
            let first_instance = instance(91)?;
            let second_instance = instance(92)?;
            let first_event = event("checkpoint-overlap-first", 100, 10);
            let second_event = event("checkpoint-overlap-second", 100, 10);
            database.store.connection.execute(
                "UPDATE usage_retention_state
                 SET writes_since_checkpoint = ?1 WHERE singleton = 1",
                [to_i64(
                    "checkpoint_write_interval",
                    policy.checkpoint_write_interval,
                )?],
            )?;

            maintain_after_commit_for_project_with_checkpoint(
                &first.connection,
                first.validated_project_root.as_deref(),
                database.project,
                policy,
                |connection| {
                    let state = passive_checkpoint_state(connection);
                    record_transaction(
                        &event_writer.connection,
                        database.project,
                        first_instance,
                        UsageInstanceOwner::McpProcess,
                        &first_event,
                        policy,
                        false,
                    )?;
                    maintain_after_commit_for_project_with_checkpoint(
                        &second.connection,
                        second.validated_project_root.as_deref(),
                        database.project,
                        policy,
                        |connection| {
                            let state = passive_checkpoint_state(connection);
                            record_transaction(
                                &event_writer.connection,
                                database.project,
                                second_instance,
                                UsageInstanceOwner::McpProcess,
                                &second_event,
                                policy,
                                false,
                            )?;
                            Ok(state)
                        },
                    )?;
                    Ok(state)
                },
            )?;

            let retention = database.store.telemetry_retention_state()?;
            assert_eq!(
                retention.writes_since_checkpoint,
                policy.checkpoint_write_interval + 2,
                "overlapping checkpoint finalizers erased later event debt"
            );
            assert!(retention.maintenance_pending);
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "overlapping checkpoint test failed: {result:?}"
        );
    }

    #[test]
    fn production_store_checkpoint_retries_and_rejects_stale_schema_or_binding() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let database = production_database()?;
            let runtime = instance(90)?;
            database.store.record_usage_for_instance(
                runtime,
                UsageInstanceOwner::McpProcess,
                &event("checkpoint", 100, 10),
                false,
            )?;
            let reader =
                AtlasStore::open_read_only_for_project(&database.database_path, &database.root)?;
            let _: i64 =
                reader
                    .connection
                    .query_row("SELECT COUNT(*) FROM usage_events", [], |row| row.get(0))?;
            database.store.record_usage_for_instance(
                runtime,
                UsageInstanceOwner::McpProcess,
                &event("checkpoint", 90, 10),
                false,
            )?;
            let policy = TelemetryRetentionPolicy::default();
            database.store.connection.execute(
                "UPDATE usage_retention_state
                 SET writes_since_checkpoint = ?1 WHERE singleton = 1",
                [to_i64(
                    "checkpoint_write_interval",
                    policy.checkpoint_write_interval,
                )?],
            )?;
            maintain_after_commit_for_project(
                &database.store.connection,
                database.store.validated_project_root.as_deref(),
                database.project,
                policy,
            )?;
            let busy = database.store.telemetry_retention_state()?;
            assert_eq!(busy.checkpoint_state, TelemetryCheckpointState::Busy);
            assert_eq!(
                busy.writes_since_checkpoint,
                policy.checkpoint_write_interval
            );

            reader.finish_index_read_snapshot()?;
            maintain_after_commit_for_project(
                &database.store.connection,
                database.store.validated_project_root.as_deref(),
                database.project,
                policy,
            )?;
            let completed = database.store.telemetry_retention_state()?;
            assert_eq!(
                completed.checkpoint_state,
                TelemetryCheckpointState::Completed
            );
            assert_eq!(completed.writes_since_checkpoint, 0);

            database.store.connection.execute(
                "UPDATE usage_retention_state
                 SET writes_since_checkpoint = ?1 WHERE singleton = 1",
                [to_i64(
                    "checkpoint_write_interval",
                    policy.checkpoint_write_interval,
                )?],
            )?;
            let newer_owner = Connection::open(&database.database_path)?;
            let future_schema = crate::schema::SCHEMA_VERSION + 1;
            newer_owner.execute(
                "UPDATE metadata SET value = ?2 WHERE key = ?1",
                params![crate::schema::SCHEMA_VERSION_KEY, future_schema.to_string()],
            )?;
            let wal_path = crate::schema::sqlite_sidecar_path(&database.database_path, "-wal");
            let wal_before = fs::read(&wal_path)?;
            let maintenance_before = database.store.telemetry_retention_state()?;
            let Err(error) = maintain_after_commit_for_project(
                &database.store.connection,
                database.store.validated_project_root.as_deref(),
                database.project,
                policy,
            ) else {
                return Err(std::io::Error::other(
                    "post-commit maintenance accepted a newer schema",
                )
                .into());
            };
            assert!(matches!(
                error,
                DbError::SchemaVersion { found, expected }
                    if found == future_schema && expected == crate::schema::SCHEMA_VERSION
            ));
            assert_eq!(
                database.store.telemetry_retention_state()?,
                maintenance_before,
                "newer-schema maintenance refusal changed retention state"
            );
            assert_eq!(
                fs::read(&wal_path)?,
                wal_before,
                "newer-schema maintenance refusal checkpointed the WAL"
            );
            assert_eq!(
                newer_owner.query_row(
                    "SELECT value FROM metadata WHERE key = ?1",
                    [crate::schema::SCHEMA_VERSION_KEY],
                    |row| row.get::<_, String>(0),
                )?,
                future_schema.to_string(),
                "newer-schema owner stopped observing its schema"
            );
            newer_owner.execute(
                "UPDATE metadata SET value = ?2 WHERE key = ?1",
                params![
                    crate::schema::SCHEMA_VERSION_KEY,
                    crate::schema::SCHEMA_VERSION.to_string()
                ],
            )?;

            database.store.connection.execute(
                "UPDATE usage_retention_state
                 SET writes_since_checkpoint = ?1 WHERE singleton = 1",
                [to_i64(
                    "checkpoint_write_interval",
                    policy.checkpoint_write_interval,
                )?],
            )?;
            let transition_before = database.store.telemetry_retention_state()?;
            let Err(error) = maintain_after_commit_for_project_with_checkpoint(
                &database.store.connection,
                database.store.validated_project_root.as_deref(),
                database.project,
                policy,
                |connection| {
                    let state = passive_checkpoint_state(connection);
                    newer_owner.execute(
                        "UPDATE metadata SET value = ?2 WHERE key = ?1",
                        params![crate::schema::SCHEMA_VERSION_KEY, future_schema.to_string()],
                    )?;
                    Ok(state)
                },
            ) else {
                return Err(std::io::Error::other(
                    "maintenance finalized after a schema transition at the checkpoint boundary",
                )
                .into());
            };
            assert!(matches!(
                error,
                DbError::SchemaVersion { found, expected }
                    if found == future_schema && expected == crate::schema::SCHEMA_VERSION
            ));
            assert_eq!(
                database.store.telemetry_retention_state()?,
                transition_before,
                "checkpoint-boundary schema transition changed retention state"
            );
            assert_eq!(
                newer_owner.query_row(
                    "SELECT value FROM metadata WHERE key = ?1",
                    [crate::schema::SCHEMA_VERSION_KEY],
                    |row| row.get::<_, String>(0),
                )?,
                future_schema.to_string(),
                "checkpoint-boundary refusal changed the concurrent owner's schema"
            );
            newer_owner.execute(
                "UPDATE metadata SET value = ?2 WHERE key = ?1",
                params![
                    crate::schema::SCHEMA_VERSION_KEY,
                    crate::schema::SCHEMA_VERSION.to_string()
                ],
            )?;

            let old_project = database.project;
            let captured = database.store.captured_project_binding()?;
            assert_eq!(captured.project_instance_id, old_project);
            let detached = AtlasStore::transition_project_root(
                &database.database_path,
                &database.root,
                crate::ProjectRootTransition::Detach,
            )?;
            assert_ne!(detached.project_instance_id, old_project);
            assert!(
                database
                    .store
                    .revalidate_captured_project_binding()
                    .is_err()
            );
            let stale = maintain_after_commit_for_project(
                &database.store.connection,
                database.store.validated_project_root.as_deref(),
                old_project,
                policy,
            );
            assert!(stale.is_err());
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "production checkpoint lifecycle test failed: {result:?}"
        );
    }

    #[test]
    fn production_store_reuses_freed_raw_pages_without_request_path_vacuum() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let database = production_database()?;
            let runtime = instance(91)?;
            let generous = TelemetryRetentionPolicy {
                max_raw_rows: 200,
                max_raw_logical_bytes: 2 * 1_024 * 1_024,
                checkpoint_write_interval: usize::MAX,
                ..TelemetryRetentionPolicy::default()
            };
            let mut large = event("page-reuse", 100, 10);
            large.query = Some("x".repeat(3_500));
            for now in 10_000..10_128 {
                record_transaction_at(
                    &database.store.connection,
                    database.project,
                    runtime,
                    UsageInstanceOwner::McpProcess,
                    &large,
                    generous,
                    false,
                    now,
                )?;
            }
            let allocated = database.store.telemetry_retention_state()?;
            let compact = TelemetryRetentionPolicy {
                max_raw_rows: 16,
                checkpoint_write_interval: usize::MAX,
                ..generous
            };
            let transaction = Transaction::new_unchecked(
                &database.store.connection,
                TransactionBehavior::Immediate,
            )?;
            let mut pruned = 0usize;
            loop {
                let deleted = prune_raw_once(&transaction, compact, 10_128)?;
                pruned = pruned
                    .checked_add(deleted)
                    .ok_or(DbError::TelemetryIntegerOverflow {
                        field: "pruned_raw_rows",
                    })?;
                if deleted == 0 {
                    break;
                }
            }
            refresh_retention_state(&transaction, compact, 10_128, pruned, 0, 0, 0)?;
            transaction.commit()?;
            let after_prune = database.store.telemetry_retention_state()?;
            assert_eq!(after_prune.raw_rows, 16);
            assert!(after_prune.freelist_pages > 0);
            assert_eq!(after_prune.page_count, allocated.page_count);

            let refill = TelemetryRetentionPolicy {
                max_raw_rows: 80,
                ..compact
            };
            for now in 20_000..20_064 {
                record_transaction_at(
                    &database.store.connection,
                    database.project,
                    runtime,
                    UsageInstanceOwner::McpProcess,
                    &large,
                    refill,
                    false,
                    now,
                )?;
            }
            let after_refill = database.store.telemetry_retention_state()?;
            assert_eq!(after_refill.raw_rows, 80);
            assert!(after_refill.page_count <= allocated.page_count);
            assert!(after_refill.freelist_pages < after_prune.freelist_pages);
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "production page-reuse test failed: {result:?}"
        );
    }

    #[test]
    fn routed_and_synchronized_worktree_usage_remain_exact_monotonic_and_retained()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let control_root = temp.path().join("control");
        let worktree_root = temp.path().join("feature");
        let other_root = temp.path().join("other");
        let common = temp.path().join("common.git");
        let administrative = common.join("worktrees/feature");
        for path in [&control_root, &worktree_root, &other_root, &administrative] {
            fs::create_dir_all(path)?;
        }
        let control = store_at(&control_root)?;
        let worktree = store_at(&worktree_root)?;
        let other = store_at(&other_root)?;
        let worktree_project = worktree
            .validated_project_instance_id
            .ok_or(DbError::ProjectInstanceIdentityMissing)?;
        let alias = WorktreeAlias::parse("issue-430")?;
        let registration = control.register_worktree(
            &alias,
            &common,
            &administrative,
            &"11".repeat(32),
            &worktree_root,
            Some(worktree_project),
            10,
        )?;

        let routed_instance = instance(41)?;
        control.record_usage_for_worktree_instance(
            routed_instance,
            UsageInstanceOwner::McpProcess,
            registration.registration_id,
            &event("routed", 100, 20),
            false,
        )?;
        require_eq(
            &control.token_overview(None)?.calls,
            &1,
            "routed native total",
        )?;
        require_eq(
            &control.repository_token_overview()?.calls,
            &1,
            "routed repository total",
        )?;
        require_eq(
            &control.registered_worktree_token_overview(&alias)?.calls,
            &1,
            "routed origin total",
        )?;
        require(
            matches!(
                control.record_usage_for_instance(
                    routed_instance,
                    UsageInstanceOwner::McpProcess,
                    &event("routed", 100, 20),
                    false,
                ),
                Err(DbError::WorktreeTelemetryOriginConflict)
            ),
            "runtime instance crossed telemetry origins",
        )?;
        require_eq(
            &control.repository_token_overview()?.calls,
            &1,
            "origin-conflict rollback total",
        )?;

        worktree.record_usage(&event("local", 80, 20))?;
        let first = worktree.export_worktree_usage_snapshot()?;
        require_eq(&first.revision(), &1, "first local revision")?;
        require(
            first.row_count() >= 2,
            "first snapshot omitted aggregate rows",
        )?;
        require(
            first.logical_bytes() > 0,
            "first snapshot omitted logical bytes",
        )?;
        require_eq(
            &control.synchronize_worktree_usage(&alias, &first)?,
            &WorktreeUsageSyncState::Synchronized,
            "first synchronization",
        )?;
        require_eq(
            &control.repository_token_overview()?.calls,
            &2,
            "first combined total",
        )?;
        require_eq(
            &control.registered_worktree_token_overview(&alias)?.calls,
            &2,
            "first exact worktree total",
        )?;
        require_eq(
            &control.synchronize_worktree_usage(&alias, &first)?,
            &WorktreeUsageSyncState::Current,
            "stale synchronization",
        )?;
        require_eq(
            &control.repository_token_overview()?.calls,
            &2,
            "stale synchronization total",
        )?;

        worktree.record_usage(&event("local", 90, 20))?;
        let second = worktree.export_worktree_usage_snapshot()?;
        require_eq(&second.revision(), &2, "second local revision")?;
        require_eq(
            &control.synchronize_worktree_usage(&alias, &second)?,
            &WorktreeUsageSyncState::Synchronized,
            "second synchronization",
        )?;
        require_eq(
            &control.repository_token_overview()?.calls,
            &3,
            "second combined total",
        )?;
        require_eq(
            &control.registered_worktree_token_overview(&alias)?.calls,
            &3,
            "second exact worktree total",
        )?;

        let mut invalid = second;
        invalid.revision = 3;
        invalid
            .rows
            .first_mut()
            .ok_or_else(|| std::io::Error::other("invalid snapshot row missing"))?
            .counters
            .calls = -1;
        require(
            matches!(
                control.synchronize_worktree_usage(&alias, &invalid),
                Err(DbError::TelemetryIntegerOverflow { field: "calls" })
            ),
            "invalid snapshot was not rejected",
        )?;
        require_eq(
            &control
                .worktree_registration(&alias)?
                .accepted_telemetry_revision,
            &2,
            "revision after invalid snapshot",
        )?;
        require_eq(
            &control.repository_token_overview()?.calls,
            &3,
            "total after invalid snapshot",
        )?;

        let other_snapshot = other.export_worktree_usage_snapshot()?;
        require(
            matches!(
                control.synchronize_worktree_usage(&alias, &other_snapshot),
                Err(DbError::WorktreeTelemetryProjectMismatch { .. })
            ),
            "mismatched project snapshot was not rejected",
        )?;
        let contending_local = store_at(&worktree_root)?;
        let local_reader = AtlasStore::open_read_only_for_project(
            &worktree_root.join(".projectatlas").join("projectatlas.db"),
            &worktree_root,
        )?;
        let (retired, final_sync) =
            local_reader.with_exclusive_worktree_usage_snapshot(|snapshot| {
                let blocked = contending_local.record_usage(&event("after-export", 70, 20));
                if !blocked.as_ref().is_err_and(DbError::is_write_unavailable) {
                    return Err(DbError::WorktreeRegistrationRow {
                        reason: "local writer was not excluded during final synchronization",
                    });
                }
                control.retire_worktree_with_usage_snapshot(&alias, snapshot, 20)
            })?;
        require_eq(
            &retired.state,
            &WorktreeRegistrationState::Retired,
            "writer-excluded retirement state",
        )?;
        require_eq(
            &final_sync,
            &WorktreeUsageSyncState::Current,
            "writer-excluded final synchronization",
        )?;
        contending_local.record_usage(&event("after-retirement", 70, 20))?;
        require_eq(
            &contending_local.token_overview(None)?.calls,
            &3,
            "local usage after retirement",
        )?;
        require_eq(
            &control.repository_token_overview()?.calls,
            &3,
            "retired repository total",
        )?;
        Ok(())
    }

    #[test]
    fn worktree_usage_export_keeps_revision_and_aggregates_in_one_read_snapshot()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("worktree");
        fs::create_dir(&root)?;
        let database_path = root.join("projectatlas.db");
        let mut worktree = AtlasStore::open_for_project(&database_path, &root)?;
        worktree.record_usage(&event("snapshot", 100, 20))?;

        let (entered_sender, entered_receiver) = sync_channel(1);
        let (resume_sender, resume_receiver) = sync_channel(1);
        WORKTREE_SNAPSHOT_EXPORT_BLOCKER.with(|slot| {
            *slot.borrow_mut() = Some(SnapshotExportBlocker {
                entered: entered_sender,
                resume: resume_receiver,
            });
        });
        worktree
            .connection
            .trace(Some(block_worktree_snapshot_aggregate_query));

        let writer_root = root.clone();
        let writer_database = database_path.clone();
        let writer = std::thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                entered_receiver
                    .recv_timeout(Duration::from_secs(10))
                    .map_err(|error| error.to_string())?;
                let writer = AtlasStore::open_for_project(&writer_database, &writer_root)
                    .map_err(|error| error.to_string())?;
                writer
                    .record_usage(&event("snapshot", 90, 20))
                    .map_err(|error| error.to_string())
            })();
            let _resume = resume_sender.send(());
            result
        });

        let snapshot_result = worktree.export_worktree_usage_snapshot();
        worktree.connection.trace(None);
        WORKTREE_SNAPSHOT_EXPORT_BLOCKER.with(|slot| {
            slot.borrow_mut().take();
        });
        writer
            .join()
            .map_err(|_panic| io::Error::other("snapshot writer panicked"))?
            .map_err(io::Error::other)?;
        let snapshot = snapshot_result?;
        require_eq(&snapshot.revision(), &1, "exported snapshot revision")?;
        let exported_calls = snapshot
            .rows
            .iter()
            .filter(|row| row.day_epoch == -1)
            .map(|row| row.counters.calls)
            .sum::<i64>();
        require_eq(&exported_calls, &1, "exported aggregate calls")?;

        let current = worktree.export_worktree_usage_snapshot()?;
        require_eq(&current.revision(), &2, "current snapshot revision")?;
        let current_calls = current
            .rows
            .iter()
            .filter(|row| row.day_epoch == -1)
            .map(|row| row.counters.calls)
            .sum::<i64>();
        require_eq(&current_calls, &2, "current aggregate calls")?;
        let reader = AtlasStore::open_read_only_for_project(&database_path, &root)?;
        let read_only = reader.export_worktree_usage_snapshot()?;
        require_eq(&read_only.revision(), &2, "read-only snapshot revision")?;
        reader.finish_index_read_snapshot()?;
        Ok(())
    }

    #[test]
    fn worktree_continuity_high_registration_aggregate_has_bounded_sql_and_rows()
    -> Result<(), Box<dyn Error>> {
        const ORIGINS: usize = 128;
        const EXPECTED_STATEMENTS: usize = 11_279;
        const EXPECTED_CHANGED_ROWS: u64 = 3_203;

        let temp = tempfile::tempdir()?;
        let control_root = temp.path().join("control");
        fs::create_dir_all(&control_root)?;
        let database = control_root.join("projectatlas.db");
        let mut control = AtlasStore::open_for_project(&database, &control_root)?;
        let project = control.captured_project_binding()?.project_instance_id;
        let common = temp.path().join("common.git");
        let event = event("worktree-scale", 100, 20);

        WORKTREE_TRACE.with(|statements| statements.borrow_mut().clear());
        control.connection.trace(Some(record_worktree_statement));
        let changed_before = control.connection.total_changes();
        for index in 0..ORIGINS {
            let suffix = index + 1;
            let alias = WorktreeAlias::parse(&format!("worktree-{suffix:03}"))?;
            let registration = control.register_worktree(
                &alias,
                &common,
                &common.join(format!("worktrees/{suffix:03}")),
                &format!("{suffix:064x}"),
                &temp.path().join(format!("worktree-{suffix:03}")),
                Some(ProjectInstanceId::from_bytes([u8::try_from(suffix)?; 16])?),
                u64::try_from(suffix)?,
            )?;
            record_usage_for_project(
                &control.connection,
                project,
                instance(u8::try_from(suffix)?)?,
                UsageInstanceOwner::McpProcess,
                Some(registration.registration_id),
                &event,
                TelemetryRetentionPolicy::default(),
                true,
            )?;
        }
        let registrations = control.worktree_registrations(false)?;
        let overview = control.repository_token_overview()?;
        control.connection.trace(None);
        let changed_rows = control.connection.total_changes() - changed_before;
        let statements =
            WORKTREE_TRACE.with(|statements| std::mem::take(&mut *statements.borrow_mut()));

        require_eq(&registrations.len(), &ORIGINS, "active registration count")?;
        require_eq(&overview.calls, &ORIGINS, "repository aggregate calls")?;
        require_eq(
            &statements.len(),
            &EXPECTED_STATEMENTS,
            "worktree scale statement count",
        )?;
        require_eq(
            &changed_rows,
            &EXPECTED_CHANGED_ROWS,
            "worktree scale changed rows",
        )?;
        Ok(())
    }

    #[test]
    fn hot_telemetry_queries_use_owned_indexes_without_duplicate_primary_key_indexes() {
        let result = (|| -> Result<(), Box<dyn Error>> {
            let database = test_database()?;
            assert_plan_uses(
                &query_plan(
                    &database.connection,
                    "SELECT instance_row_id FROM usage_instances
                 WHERE project_instance_id = X'01010101010101010101010101010101'
                   AND runtime_instance_id = X'02020202020202020202020202020202'",
                )?,
                "sqlite_autoindex_usage_instances_1",
            );
            assert_plan_uses(
                &query_plan(
                    &database.connection,
                    "SELECT maximum_without FROM usage_instance_baselines
                 WHERE instance_row_id = 1
                   AND baseline_key = zeroblob(32)",
                )?,
                "PRIMARY KEY",
            );
            assert_plan_uses(
                &query_plan(
                    &database.connection,
                    "SELECT id FROM usage_events
                 WHERE created_at_epoch < 1
                 ORDER BY created_at_epoch, id LIMIT 1",
                )?,
                "idx_usage_created_at",
            );
            assert_plan_uses(
                &query_plan(
                    &database.connection,
                    "SELECT calls FROM usage_global_aggregates
                 WHERE project_instance_id = X'01010101010101010101010101010101'
                   AND dimension_id = 1",
                )?,
                "PRIMARY KEY",
            );
            assert_plan_uses(
                &query_plan(
                    &database.connection,
                    "SELECT instance_row_id FROM usage_instances
                 WHERE project_instance_id = X'01010101010101010101010101010101'
                   AND caller_label = 'agent' AND state = 'active'
                 ORDER BY started_at_epoch, instance_row_id LIMIT 1",
                )?,
                "idx_usage_instances_label_state",
            );
            assert_plan_uses(
                &query_plan(
                    &database.connection,
                    "SELECT project_instance_id FROM usage_daily_aggregates
                 WHERE day_epoch < 1 ORDER BY day_epoch LIMIT 1",
                )?,
                "idx_usage_daily_retention",
            );
            assert_plan_uses(
                &query_plan(
                    &database.connection,
                    "SELECT project_instance_id FROM usage_label_tombstones
                 WHERE expired_at_epoch < 1 ORDER BY expired_at_epoch LIMIT 1",
                )?,
                "idx_usage_label_tombstones_retention",
            );
            for redundant in [
                "idx_usage_baselines_active",
                "idx_usage_daily_range",
                "idx_usage_instance_daily_range",
            ] {
                let exists = database.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'index' AND name = ?1)",
                    [redundant],
                    |row| row.get::<_, i64>(0),
                )?;
                assert_eq!(exists, 0, "redundant index {redundant} must stay absent");
            }
            Ok(())
        })();
        assert!(
            result.is_ok(),
            "telemetry query-plan test failed: {result:?}"
        );
    }
}
